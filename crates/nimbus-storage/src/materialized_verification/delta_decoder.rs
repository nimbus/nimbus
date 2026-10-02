//! Canonical decoder from applied journal records to logical verification leaves.
//!
//! The decoder is the only path that builds materialized-state deltas and the
//! snapshot seed. Each leaf uses the canonical identity and value encoders.

use std::collections::HashMap;

use nimbus_core::{
    DocumentLocator, Error, Result, SchemaChangeEvent, TableId, TenantEventKind, TenantEventRecord,
    TriggerDeliveryCursor, WriteOp,
};

use super::{
    LogicalLeafKey, LogicalLeafKind, MaterializedStateDelta, MaterializedStateLeaf,
    MaterializedVerificationSeed,
};
use crate::materialized_position::{
    canonical_document_identity, canonical_document_value,
    canonical_resource_path_binding_identity, canonical_resource_path_binding_value,
    canonical_scheduled_execution_identity, canonical_scheduled_execution_value,
    canonical_schema_identity, canonical_schema_identity_for_name, canonical_schema_value,
    canonical_table_identity_identity, canonical_table_identity_value,
    canonical_trigger_delivery_cursor_identity, canonical_trigger_delivery_cursor_value,
};
use crate::{MaterializedJournalSnapshot, TableIdentitySnapshotEntry};

pub(super) fn deltas_for_validated_record(
    record: &TenantEventRecord,
    table_identities: &mut HashMap<TableId, TableIdentitySnapshotEntry>,
) -> Result<Vec<MaterializedStateDelta>> {
    let mut deltas = Vec::new();
    if record.events.is_empty() {
        append_document_deltas(&record.writes, table_identities, &mut deltas)?;
        if let Some(execution_id) = record.scheduled_execution_id.as_deref() {
            append_scheduled_execution_delta(execution_id, &mut deltas)?;
        }
        return Ok(deltas);
    }
    for event in &record.events {
        match event {
            TenantEventKind::DocumentWrite { writes } => {
                append_document_deltas(writes, table_identities, &mut deltas)?;
            }
            TenantEventKind::SchemaChange { change } => match change.as_ref() {
                SchemaChangeEvent::SetTable {
                    table_id, current, ..
                } => {
                    append_default_table_identity_deltas(
                        &current.table,
                        table_id,
                        table_identities,
                        true,
                        &mut deltas,
                    )?;
                    deltas.push(MaterializedStateDelta::Upsert(MaterializedStateLeaf::new(
                        LogicalLeafKind::Schema,
                        canonical_schema_identity(current)?,
                        canonical_schema_value(current)?,
                    )?));
                }
                SchemaChangeEvent::DeleteTable {
                    table, previous, ..
                } => {
                    let identity = if let Some(previous) = previous {
                        canonical_schema_identity(previous)?
                    } else {
                        canonical_schema_identity_for_name(table.as_str())?
                    };
                    deltas.push(MaterializedStateDelta::Remove(LogicalLeafKey::new(
                        LogicalLeafKind::Schema,
                        &identity,
                    )?));
                }
            },
            // A lifecycle event can delete an unbounded document family, and
            // the event does not carry those document IDs. Rebuild instead of
            // pretending that a partial delta is exact.
            TenantEventKind::TableLifecycle { .. } => {
                deltas.push(MaterializedStateDelta::Invalidate)
            }
            TenantEventKind::ScheduledExecution { execution_id } => {
                append_scheduled_execution_delta(execution_id, &mut deltas)?;
            }
            TenantEventKind::TriggerDelivery { cursor } => {
                append_trigger_delivery_cursor_delta(*cursor, &mut deltas)?;
            }
            TenantEventKind::IndexLifecycle { .. } | TenantEventKind::Barrier { .. } => {}
        }
    }
    Ok(deltas)
}

fn append_document_deltas(
    writes: &[WriteOp],
    table_identities: &mut HashMap<TableId, TableIdentitySnapshotEntry>,
    deltas: &mut Vec<MaterializedStateDelta>,
) -> Result<()> {
    for write in writes {
        let allow_create = write.previous.is_none() && write.current.is_some();
        append_default_table_identity_deltas(
            &write.table,
            &write.table_id,
            table_identities,
            allow_create,
            deltas,
        )?;
        if let Some(current) = &write.current {
            deltas.push(MaterializedStateDelta::Upsert(MaterializedStateLeaf::new(
                LogicalLeafKind::Document,
                canonical_document_identity(current)?,
                canonical_document_value(current)?,
            )?));
        } else if let Some(previous) = &write.previous {
            deltas.push(MaterializedStateDelta::Remove(LogicalLeafKey::new(
                LogicalLeafKind::Document,
                &canonical_document_identity(previous)?,
            )?));
        } else {
            return Err(Error::Internal(format!(
                "materialized document write {} has neither a previous nor current image",
                write.doc_id
            )));
        }
        append_resource_path_binding_delta(write, deltas)?;
    }
    Ok(())
}

fn append_resource_path_binding_delta(
    write: &WriteOp,
    deltas: &mut Vec<MaterializedStateDelta>,
) -> Result<()> {
    if write.current.is_some() {
        if let Some(binding) = write.resource_path_binding.as_ref() {
            deltas.push(MaterializedStateDelta::Upsert(MaterializedStateLeaf::new(
                LogicalLeafKind::ResourcePathBinding,
                canonical_resource_path_binding_identity(&binding.locator)?,
                canonical_resource_path_binding_value(binding)?,
            )?));
        }
        return Ok(());
    }

    let locator = DocumentLocator::new(write.table.clone(), write.doc_id.clone());
    deltas.push(MaterializedStateDelta::Remove(LogicalLeafKey::new(
        LogicalLeafKind::ResourcePathBinding,
        &canonical_resource_path_binding_identity(&locator)?,
    )?));
    Ok(())
}

fn append_default_table_identity_deltas(
    table: &nimbus_core::TableName,
    table_id: &TableId,
    table_identities: &mut HashMap<TableId, TableIdentitySnapshotEntry>,
    allow_create: bool,
    deltas: &mut Vec<MaterializedStateDelta>,
) -> Result<()> {
    if let Some(identity) = table_identities.get(table_id) {
        if identity.table != *table {
            return Err(Error::Internal(format!(
                "materialized write table {} disagrees with table id {} owned by {}",
                table, table_id, identity.table
            )));
        }
        if identity.is_active() {
            append_table_identity_upsert(identity, deltas)?;
            return Ok(());
        }
        if identity.state == nimbus_core::TableState::Deleting {
            return Err(Error::Internal(format!(
                "materialized write references deleting table identity {}",
                table_id
            )));
        }
    }
    if !table_identities.contains_key(table_id) && !allow_create {
        return Err(Error::Internal(format!(
            "materialized write for unknown table identity {} cannot be exact",
            table_id
        )));
    }

    if let Some(previous_active) = table_identities
        .values()
        .find(|identity| identity.table == *table && identity.is_active())
        .cloned()
    {
        deltas.push(MaterializedStateDelta::Remove(LogicalLeafKey::new(
            LogicalLeafKind::TableIdentity,
            &canonical_table_identity_identity(&previous_active)?,
        )?));
        let deleting = TableIdentitySnapshotEntry {
            namespace: crate::table_identity::deleting_table_namespace(&previous_active.table_id),
            table: previous_active.table,
            table_id: previous_active.table_id.clone(),
            state: nimbus_core::TableState::Deleting,
        };
        append_table_identity_upsert(&deleting, deltas)?;
        table_identities.insert(deleting.table_id.clone(), deleting);
    }

    if let Some(staged_hidden) = table_identities.get(table_id).cloned() {
        deltas.push(MaterializedStateDelta::Remove(LogicalLeafKey::new(
            LogicalLeafKind::TableIdentity,
            &canonical_table_identity_identity(&staged_hidden)?,
        )?));
    }
    let identity = TableIdentitySnapshotEntry::default_namespace(table.clone(), table_id.clone());
    append_table_identity_upsert(&identity, deltas)?;
    table_identities.insert(table_id.clone(), identity);
    Ok(())
}

fn append_table_identity_upsert(
    identity: &TableIdentitySnapshotEntry,
    deltas: &mut Vec<MaterializedStateDelta>,
) -> Result<()> {
    deltas.push(MaterializedStateDelta::Upsert(MaterializedStateLeaf::new(
        LogicalLeafKind::TableIdentity,
        canonical_table_identity_identity(identity)?,
        canonical_table_identity_value(identity)?,
    )?));
    Ok(())
}

fn append_scheduled_execution_delta(
    execution_id: &str,
    deltas: &mut Vec<MaterializedStateDelta>,
) -> Result<()> {
    deltas.push(MaterializedStateDelta::Upsert(MaterializedStateLeaf::new(
        LogicalLeafKind::ScheduledExecution,
        canonical_scheduled_execution_identity(execution_id)?,
        canonical_scheduled_execution_value(execution_id)?,
    )?));
    Ok(())
}

fn append_trigger_delivery_cursor_delta(
    cursor: TriggerDeliveryCursor,
    deltas: &mut Vec<MaterializedStateDelta>,
) -> Result<()> {
    let identity = canonical_trigger_delivery_cursor_identity()?;
    if cursor == TriggerDeliveryCursor::default() {
        deltas.push(MaterializedStateDelta::Remove(LogicalLeafKey::new(
            LogicalLeafKind::TriggerDeliveryCursor,
            &identity,
        )?));
    } else {
        deltas.push(MaterializedStateDelta::Upsert(MaterializedStateLeaf::new(
            LogicalLeafKind::TriggerDeliveryCursor,
            identity,
            canonical_trigger_delivery_cursor_value(cursor)?,
        )?));
    }
    Ok(())
}

pub(super) fn canonical_snapshot_seed(
    snapshot: &MaterializedJournalSnapshot,
) -> Result<MaterializedVerificationSeed> {
    let state = snapshot.canonical_state()?;
    let mut leaves = Vec::with_capacity(
        state.table_identities().len()
            + state.schema_tables().len()
            + state.documents().len()
            + state.resource_path_bindings().len()
            + state.scheduled_execution_ids().len(),
    );
    let mut table_identities = HashMap::with_capacity(state.table_identities().len());
    for identity in state.table_identities() {
        table_identities.insert(identity.table_id.clone(), identity.clone());
        leaves.push((
            LogicalLeafKey::new(
                LogicalLeafKind::TableIdentity,
                &canonical_table_identity_identity(identity)?,
            )?,
            canonical_table_identity_value(identity)?,
        ));
    }
    for table in state.schema_tables() {
        leaves.push((
            LogicalLeafKey::new(LogicalLeafKind::Schema, &canonical_schema_identity(table)?)?,
            canonical_schema_value(table)?,
        ));
    }
    for document in state.documents() {
        leaves.push((
            LogicalLeafKey::new(
                LogicalLeafKind::Document,
                &canonical_document_identity(document)?,
            )?,
            canonical_document_value(document)?,
        ));
    }
    for binding in state.resource_path_bindings() {
        leaves.push((
            LogicalLeafKey::new(
                LogicalLeafKind::ResourcePathBinding,
                &canonical_resource_path_binding_identity(&binding.locator)?,
            )?,
            canonical_resource_path_binding_value(binding)?,
        ));
    }
    for execution_id in state.scheduled_execution_ids() {
        leaves.push((
            LogicalLeafKey::new(
                LogicalLeafKind::ScheduledExecution,
                &canonical_scheduled_execution_identity(execution_id)?,
            )?,
            canonical_scheduled_execution_value(execution_id)?,
        ));
    }
    if state.trigger_delivery_cursor() != TriggerDeliveryCursor::default() {
        leaves.push((
            LogicalLeafKey::new(
                LogicalLeafKind::TriggerDeliveryCursor,
                &canonical_trigger_delivery_cursor_identity()?,
            )?,
            canonical_trigger_delivery_cursor_value(state.trigger_delivery_cursor())?,
        ));
    }
    Ok(MaterializedVerificationSeed {
        leaves,
        table_identities,
    })
}
