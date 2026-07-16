use super::*;

impl TenantPersistence {
    pub(crate) fn apply_execution_unit_batch_with_origin(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
        trigger_write_origin: Option<&nimbus_core::TriggerWriteOrigin>,
        commit_timestamp: Option<Timestamp>,
    ) -> Result<Option<CommitEntry>> {
        match_tenant_persistence!(self, |store| {
            store.apply_execution_unit_batch_with_origin(
                writes,
                schedule_ops,
                trigger_write_origin,
                commit_timestamp,
            )
        })
    }

    pub(crate) fn insert_with_indexes_once_at(
        &self,
        document: &Document,
        assignment: nimbus_storage::DirectWriteAssignment<'_>,
    ) -> Result<Option<CommitEntry>> {
        match_tenant_persistence!(self, |store| {
            store.insert_with_indexes_once_at(document, assignment)
        })
    }

    pub(crate) fn update_with_indexes_validated_once_at<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        assignment: nimbus_storage::DirectWriteAssignment<'_>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.update_with_indexes_validated_once_at(table, id, patch, assignment, validate)
        })
    }

    pub(crate) fn delete_with_indexes_validated_once_at<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        assignment: nimbus_storage::DirectWriteAssignment<'_>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.delete_with_indexes_validated_once_at(table, id, assignment, validate)
        })
    }
}
