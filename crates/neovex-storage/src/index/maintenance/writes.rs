use neovex_core::{CommitEntry, Document, DocumentId, IndexDefinition, Result, TableName};
use serde_json::Value;

use crate::store::TenantStore;

fn require_commit(commit: Option<CommitEntry>, error_message: &str) -> Result<CommitEntry> {
    commit.ok_or_else(|| neovex_core::Error::Internal(error_message.to_string()))
}

impl TenantStore {
    /// Inserts a document and maintains indexes atomically.
    pub fn insert_with_indexes(
        &self,
        document: &Document,
        indexes: &[IndexDefinition],
    ) -> Result<CommitEntry> {
        require_commit(
            self.insert_with_indexes_once(document, indexes, None)?,
            "non-deduplicated indexed insert should commit",
        )
    }

    /// Inserts a document and maintains indexes once for the provided scheduled execution id.
    pub fn insert_with_indexes_once(
        &self,
        document: &Document,
        indexes: &[IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id)? {
                return Ok(false);
            }
            transaction.insert_document_with_indexes(document, indexes)?;
            Ok(true)
        })?;
        Ok(if committed.value {
            Some(require_commit(
                committed.commit,
                "deduplicated indexed insert should record a commit entry",
            )?)
        } else {
            None
        })
    }

    /// Updates a document and maintains indexes atomically.
    pub fn update_with_indexes(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        indexes: &[IndexDefinition],
    ) -> Result<CommitEntry> {
        self.update_with_indexes_validated(table, id, patch, indexes, |_, _| Ok(()))
    }

    /// Updates a document and maintains indexes atomically after validating the merged result.
    pub fn update_with_indexes_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()>,
    {
        require_commit(
            self.update_with_indexes_validated_once(table, id, patch, indexes, None, validate)?,
            "non-deduplicated indexed update should commit",
        )
    }

    /// Updates a document and maintains indexes once for the provided scheduled execution id.
    pub fn update_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        indexes: &[IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()>,
    {
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id)? {
                return Ok(false);
            }
            transaction
                .update_document_with_indexes_validated(table, id, patch, indexes, validate)?;
            Ok(true)
        })?;
        Ok(if committed.value {
            Some(require_commit(
                committed.commit,
                "deduplicated indexed update should record a commit entry",
            )?)
        } else {
            None
        })
    }

    /// Deletes a document and removes index entries atomically.
    pub fn delete_with_indexes(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
    ) -> Result<CommitEntry> {
        self.delete_with_indexes_validated_once(table, id, indexes, None, |_| Ok(()))?
            .map(|(commit, _)| commit)
            .ok_or_else(|| {
                neovex_core::Error::Internal(
                    "non-deduplicated indexed delete should commit".to_string(),
                )
            })
    }

    /// Deletes a document and removes index entries once for the provided scheduled execution id.
    pub fn delete_with_indexes_once(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<(CommitEntry, Document)>> {
        self.delete_with_indexes_validated_once(table, id, indexes, execution_id, |_| Ok(()))
    }

    /// Deletes a document and removes index entries atomically, returning the removed snapshot.
    pub fn delete_with_indexes_returning_document(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
    ) -> Result<(CommitEntry, Document)> {
        self.delete_with_indexes_validated_returning_document(table, id, indexes, |_| Ok(()))
    }

    /// Deletes a document and removes index entries atomically after validating the removed snapshot.
    pub fn delete_with_indexes_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        self.delete_with_indexes_validated_once(table, id, indexes, None, validate)?
            .ok_or_else(|| {
                neovex_core::Error::Internal(
                    "non-deduplicated indexed delete should commit".to_string(),
                )
            })
    }

    /// Deletes a document and removes index entries once for the provided scheduled execution id, returning the removed snapshot.
    pub fn delete_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id)? {
                return Ok(None);
            }
            let removed_document =
                transaction.delete_document_with_indexes_validated(table, id, indexes, validate)?;
            Ok(Some(removed_document))
        })?;
        Ok(if let Some(removed_document) = committed.value {
            Some((
                require_commit(
                    committed.commit,
                    "deduplicated indexed delete should record a commit entry",
                )?,
                removed_document,
            ))
        } else {
            None
        })
    }
}
