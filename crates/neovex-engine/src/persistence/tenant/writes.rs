use super::*;

impl TenantPersistence {
    pub(crate) fn apply_execution_unit_batch(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
    ) -> Result<Option<CommitEntry>> {
        match self {
            Self::Redb(store) => store.apply_execution_unit_batch(writes, schedule_ops),
            Self::Sqlite(store) => store.apply_execution_unit_batch(writes, schedule_ops),
            Self::LibsqlReplica(store) => store.apply_execution_unit_batch(writes, schedule_ops),
            Self::Postgres(store) => store.apply_execution_unit_batch(writes, schedule_ops),
            Self::MySql(store) => store.apply_execution_unit_batch(writes, schedule_ops),
        }
    }

    pub(crate) fn insert(&self, document: &Document) -> Result<CommitEntry> {
        match self {
            Self::Redb(store) => store.insert(document),
            Self::Sqlite(store) => store.insert(document),
            Self::LibsqlReplica(store) => store.insert(document),
            Self::Postgres(store) => store.insert(document),
            Self::MySql(store) => store.insert(document),
        }
    }

    pub(crate) fn insert_with_indexes(
        &self,
        document: &Document,
        indexes: &[neovex_core::IndexDefinition],
    ) -> Result<CommitEntry> {
        match self {
            Self::Redb(store) => store.insert_with_indexes(document, indexes),
            Self::Sqlite(store) => store.insert_with_indexes(document, indexes),
            Self::LibsqlReplica(store) => store.insert_with_indexes(document, indexes),
            Self::Postgres(store) => store.insert_with_indexes(document, indexes),
            Self::MySql(store) => store.insert_with_indexes(document, indexes),
        }
    }

    pub(crate) fn insert_once(
        &self,
        document: &Document,
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        match self {
            Self::Redb(store) => store.insert_once(document, execution_id),
            Self::Sqlite(store) => store.insert_once(document, execution_id),
            Self::LibsqlReplica(store) => store.insert_once(document, execution_id),
            Self::Postgres(store) => store.insert_once(document, execution_id),
            Self::MySql(store) => store.insert_once(document, execution_id),
        }
    }

    pub(crate) fn insert_with_indexes_once(
        &self,
        document: &Document,
        indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        match self {
            Self::Redb(store) => store.insert_with_indexes_once(document, indexes, execution_id),
            Self::Sqlite(store) => store.insert_with_indexes_once(document, indexes, execution_id),
            Self::LibsqlReplica(store) => {
                store.insert_with_indexes_once(document, indexes, execution_id)
            }
            Self::Postgres(store) => {
                store.insert_with_indexes_once(document, indexes, execution_id)
            }
            Self::MySql(store) => store.insert_with_indexes_once(document, indexes, execution_id),
        }
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
        match self {
            Self::Redb(store) => store.update_validated(table, id, patch, validate),
            Self::Sqlite(store) => store.update_validated(table, id, patch, validate),
            Self::LibsqlReplica(store) => store.update_validated(table, id, patch, validate),
            Self::Postgres(store) => store.update_validated(table, id, patch, validate),
            Self::MySql(store) => store.update_validated(table, id, patch, validate),
        }
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
        match self {
            Self::Redb(store) => {
                store.update_validated_once(table, id, patch, execution_id, validate)
            }
            Self::Sqlite(store) => {
                store.update_validated_once(table, id, patch, execution_id, validate)
            }
            Self::LibsqlReplica(store) => {
                store.update_validated_once(table, id, patch, execution_id, validate)
            }
            Self::Postgres(store) => {
                store.update_validated_once(table, id, patch, execution_id, validate)
            }
            Self::MySql(store) => {
                store.update_validated_once(table, id, patch, execution_id, validate)
            }
        }
    }

    pub(crate) fn update_with_indexes_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        indexes: &[neovex_core::IndexDefinition],
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        match self {
            Self::Redb(store) => {
                store.update_with_indexes_validated(table, id, patch, indexes, validate)
            }
            Self::Sqlite(store) => {
                store.update_with_indexes_validated(table, id, patch, indexes, validate)
            }
            Self::LibsqlReplica(store) => {
                store.update_with_indexes_validated(table, id, patch, indexes, validate)
            }
            Self::Postgres(store) => {
                store.update_with_indexes_validated(table, id, patch, indexes, validate)
            }
            Self::MySql(store) => {
                store.update_with_indexes_validated(table, id, patch, indexes, validate)
            }
        }
    }

    pub(crate) fn update_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        match self {
            Self::Redb(store) => store.update_with_indexes_validated_once(
                table,
                id,
                patch,
                indexes,
                execution_id,
                validate,
            ),
            Self::Sqlite(store) => store.update_with_indexes_validated_once(
                table,
                id,
                patch,
                indexes,
                execution_id,
                validate,
            ),
            Self::LibsqlReplica(store) => store.update_with_indexes_validated_once(
                table,
                id,
                patch,
                indexes,
                execution_id,
                validate,
            ),
            Self::Postgres(store) => store.update_with_indexes_validated_once(
                table,
                id,
                patch,
                indexes,
                execution_id,
                validate,
            ),
            Self::MySql(store) => store.update_with_indexes_validated_once(
                table,
                id,
                patch,
                indexes,
                execution_id,
                validate,
            ),
        }
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
        match self {
            Self::Redb(store) => store.delete_validated_returning_document(table, id, validate),
            Self::Sqlite(store) => store.delete_validated_returning_document(table, id, validate),
            Self::LibsqlReplica(store) => {
                store.delete_validated_returning_document(table, id, validate)
            }
            Self::Postgres(store) => store.delete_validated_returning_document(table, id, validate),
            Self::MySql(store) => store.delete_validated_returning_document(table, id, validate),
        }
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
        match self {
            Self::Redb(store) => store.delete_validated_once(table, id, execution_id, validate),
            Self::Sqlite(store) => store.delete_validated_once(table, id, execution_id, validate),
            Self::LibsqlReplica(store) => {
                store.delete_validated_once(table, id, execution_id, validate)
            }
            Self::Postgres(store) => store.delete_validated_once(table, id, execution_id, validate),
            Self::MySql(store) => store.delete_validated_once(table, id, execution_id, validate),
        }
    }

    pub(crate) fn delete_with_indexes_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[neovex_core::IndexDefinition],
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        match self {
            Self::Redb(store) => {
                store.delete_with_indexes_validated_returning_document(table, id, indexes, validate)
            }
            Self::Sqlite(store) => {
                store.delete_with_indexes_validated_returning_document(table, id, indexes, validate)
            }
            Self::LibsqlReplica(store) => {
                store.delete_with_indexes_validated_returning_document(table, id, indexes, validate)
            }
            Self::Postgres(store) => {
                store.delete_with_indexes_validated_returning_document(table, id, indexes, validate)
            }
            Self::MySql(store) => {
                store.delete_with_indexes_validated_returning_document(table, id, indexes, validate)
            }
        }
    }

    pub(crate) fn delete_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        match self {
            Self::Redb(store) => {
                store.delete_with_indexes_validated_once(table, id, indexes, execution_id, validate)
            }
            Self::Sqlite(store) => {
                store.delete_with_indexes_validated_once(table, id, indexes, execution_id, validate)
            }
            Self::LibsqlReplica(store) => {
                store.delete_with_indexes_validated_once(table, id, indexes, execution_id, validate)
            }
            Self::Postgres(store) => {
                store.delete_with_indexes_validated_once(table, id, indexes, execution_id, validate)
            }
            Self::MySql(store) => {
                store.delete_with_indexes_validated_once(table, id, indexes, execution_id, validate)
            }
        }
    }
}
