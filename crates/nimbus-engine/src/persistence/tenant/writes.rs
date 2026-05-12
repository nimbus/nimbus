use super::*;

impl TenantPersistence {
    pub(crate) fn apply_execution_unit_batch_with_origin(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
        trigger_write_origin: Option<&nimbus_core::TriggerWriteOrigin>,
    ) -> Result<Option<CommitEntry>> {
        match_tenant_persistence!(self, |store| {
            store.apply_execution_unit_batch_with_origin(writes, schedule_ops, trigger_write_origin)
        })
    }

    pub(crate) fn insert(&self, document: &Document) -> Result<CommitEntry> {
        match_tenant_persistence!(self, |store| store.insert(document))
    }

    pub(crate) fn insert_with_indexes(
        &self,
        document: &Document,
        indexes: &[nimbus_core::IndexDefinition],
    ) -> Result<CommitEntry> {
        match_tenant_persistence!(self, |store| store.insert_with_indexes(document, indexes))
    }

    pub(crate) fn insert_once(
        &self,
        document: &Document,
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        match_tenant_persistence!(self, |store| store.insert_once(document, execution_id))
    }

    pub(crate) fn insert_with_indexes_once(
        &self,
        document: &Document,
        indexes: &[nimbus_core::IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        match_tenant_persistence!(self, |store| {
            store.insert_with_indexes_once(document, indexes, execution_id)
        })
    }

    pub(crate) fn update_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.update_validated(table, id, patch, validate)
        })
    }

    pub(crate) fn update_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.update_validated_once(table, id, patch, execution_id, validate)
        })
    }

    pub(crate) fn update_with_indexes_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        indexes: &[nimbus_core::IndexDefinition],
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.update_with_indexes_validated(table, id, patch, indexes, validate)
        })
    }

    pub(crate) fn update_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        indexes: &[nimbus_core::IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.update_with_indexes_validated_once(
                table,
                id,
                patch,
                indexes,
                execution_id,
                validate,
            )
        })
    }

    pub(crate) fn delete_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.delete_validated_returning_document(table, id, validate)
        })
    }

    pub(crate) fn delete_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.delete_validated_once(table, id, execution_id, validate)
        })
    }

    pub(crate) fn delete_with_indexes_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[nimbus_core::IndexDefinition],
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.delete_with_indexes_validated_returning_document(table, id, indexes, validate)
        })
    }

    pub(crate) fn delete_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[nimbus_core::IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        match_tenant_persistence!(self, |store| {
            store.delete_with_indexes_validated_once(table, id, indexes, execution_id, validate)
        })
    }
}
