use std::sync::Arc;

use nimbus_core::{
    AccessAction, DependencySet, Document, DocumentId, Error, Mutation, PrincipalContext, Result,
    Schema, SequenceNumber, TableSchema, Timestamp, WriteOp, WriteOpType,
};

use crate::tenant::TenantRuntime;

use super::enforce_mutation_authorization;

/// A path-A/C single-document prepare built entirely from the caller's current
/// in-memory full-image window. Constructing this value performs no storage I/O
/// and is small enough to stay on the caller's async task.
pub(super) struct WindowPreparedWrite {
    pub(super) snapshot_sequence: SequenceNumber,
    pub(super) dependencies: DependencySet,
    pub(super) write: WriteOp,
    pub(super) indexes: Vec<nimbus_core::IndexDefinition>,
    pub(super) normalized_mutation: Mutation,
    pub(super) schema: Arc<Schema>,
    pub(super) result_document_id: Option<DocumentId>,
}

pub(super) fn prepare_single_document_write_from_window(
    runtime: &TenantRuntime,
    mutation: &Mutation,
    principal: &PrincipalContext,
) -> Result<Option<WindowPreparedWrite>> {
    if !runtime.store.has_process_local_sequence_authority() {
        return Ok(None);
    }
    let snapshot_sequence = runtime.applied_head();

    let schema = runtime.schema();
    let (write, indexes, result_document_id) = match mutation {
        Mutation::Insert {
            table,
            id: Some(id),
            fields,
        } => {
            if !runtime
                .write_log
                .current_prepare_view_available(snapshot_sequence)
            {
                return Ok(None);
            }
            let Some(table_id) = runtime.prepared_table_id_if_known(table) else {
                return Ok(None);
            };
            let table_schema = schema.get_table(table);
            if let Some(table_schema) = table_schema {
                table_schema.validate(fields)?;
            }
            let document =
                Document::with_id_at(id.clone(), table.clone(), fields.clone(), Timestamp(0));
            enforce_mutation_authorization(
                table_schema,
                AccessAction::Create,
                principal,
                Some(&document),
                None,
            )?;
            (
                WriteOp {
                    table: table.clone(),
                    table_id,
                    op_type: WriteOpType::Insert,
                    doc_id: id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(document),
                },
                table_indexes(table_schema),
                Some(id.clone()),
            )
        }
        Mutation::Insert { id: None, .. } => {
            return Err(Error::Internal(
                "window prepare requires a normalized insert id".to_string(),
            ));
        }
        Mutation::Update { table, id, patch } => {
            let Some(base) = runtime
                .write_log
                .current_document_state(snapshot_sequence, table, id)
            else {
                return Ok(None);
            };
            let Some(previous) = base.document else {
                return Err(Error::DocumentNotFound(id.clone()));
            };
            let mut current = previous.clone();
            for (field, value) in patch {
                current.fields.insert(field.clone(), value.clone());
            }
            let table_schema = schema.get_table(table);
            if let Some(table_schema) = table_schema {
                table_schema.validate(&current.fields)?;
            }
            enforce_mutation_authorization(
                table_schema,
                AccessAction::Update,
                principal,
                Some(&current),
                Some(&previous),
            )?;
            (
                WriteOp {
                    table: table.clone(),
                    table_id: base.table_id,
                    op_type: WriteOpType::Update,
                    doc_id: id.clone(),
                    resource_path_binding: base.resource_path_binding,
                    trigger_write_origin: None,
                    previous: Some(previous),
                    current: Some(current),
                },
                table_indexes(table_schema),
                Some(id.clone()),
            )
        }
        Mutation::Delete { table, id } => {
            let Some(base) = runtime
                .write_log
                .current_document_state(snapshot_sequence, table, id)
            else {
                return Ok(None);
            };
            let Some(previous) = base.document else {
                return Err(Error::DocumentNotFound(id.clone()));
            };
            let table_schema = schema.get_table(table);
            enforce_mutation_authorization(
                table_schema,
                AccessAction::Delete,
                principal,
                None,
                Some(&previous),
            )?;
            (
                WriteOp {
                    table: table.clone(),
                    table_id: base.table_id,
                    op_type: WriteOpType::Delete,
                    doc_id: id.clone(),
                    resource_path_binding: base.resource_path_binding,
                    trigger_write_origin: None,
                    previous: Some(previous),
                    current: None,
                },
                table_indexes(table_schema),
                None,
            )
        }
    };
    let mut dependencies = DependencySet::default();
    dependencies.record_document(&write.table, &write.table_id, write.doc_id.clone());
    Ok(Some(WindowPreparedWrite {
        snapshot_sequence,
        dependencies,
        write,
        indexes,
        normalized_mutation: mutation.clone(),
        schema,
        result_document_id,
    }))
}

fn table_indexes(table_schema: Option<&TableSchema>) -> Vec<nimbus_core::IndexDefinition> {
    table_schema
        .map(|table_schema| table_schema.indexes.clone())
        .unwrap_or_default()
}
