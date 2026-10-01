use std::sync::Arc;

use nimbus_core::{
    DependencySet, Document, DocumentId, Error, Mutation, PrincipalContext, Result, Schema,
    SequenceNumber, Timestamp, WriteOp,
};

use crate::tenant::TenantRuntime;

use super::prepare_write_op;

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
    let (table, table_id, previous, current, existing_binding, result_document_id) = match mutation
    {
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
            let document =
                Document::with_id_at(id.clone(), table.clone(), fields.clone(), Timestamp(0));
            (
                table,
                table_id,
                None,
                Some(document),
                None,
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
            (
                table,
                base.table_id,
                Some(previous),
                Some(current),
                base.resource_path_binding,
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
            (
                table,
                base.table_id,
                Some(previous),
                None,
                base.resource_path_binding,
                None,
            )
        }
    };
    let prepared = prepare_write_op(
        schema.get_table(table),
        principal,
        previous,
        current,
        existing_binding,
        None,
    )?;
    let (write, indexes) = prepared.into_write_op(table_id)?;
    let indexes = indexes.to_vec();
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
