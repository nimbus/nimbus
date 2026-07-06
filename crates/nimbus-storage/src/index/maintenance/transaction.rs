use nimbus_core::{Document, DocumentId, IndexDefinition, Result, TableName};
use serde_json::Value;

use crate::store::TenantWriteTransaction;

impl TenantWriteTransaction {
    pub fn insert_document_with_indexes(
        &mut self,
        document: &Document,
        indexes: &[IndexDefinition],
    ) -> Result<()> {
        self.apply_document_insert(document, indexes, None, None)
    }

    pub fn update_document_with_indexes_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<()>
    where
        F: FnOnce(&Document, &Document) -> Result<()>,
    {
        let old_document = self.read_existing_document_for_point_write(table, id)?;
        let mut new_document = old_document.clone();
        for (field, value) in patch {
            new_document.set_field(field.clone(), value.clone());
        }
        new_document.update_time = self.now();
        validate(&old_document, &new_document)?;
        self.apply_point_document_update(&old_document, &new_document, indexes)
    }

    pub fn delete_document_with_indexes_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<Document>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        let old_document = self.read_existing_document_for_point_write(table, id)?;
        validate(&old_document)?;
        self.apply_point_document_delete(&old_document, indexes)?;
        Ok(old_document)
    }
}
