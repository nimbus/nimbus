use std::borrow::Cow;

use nimbus_core::{CollectionName, Result};

use crate::sql::dialect::{SqlSession, SqlValue};
use crate::sql::resource_paths::{
    ResourcePathRow, SqlResourcePathSession, SqlResourcePathStore, sql_resource_path_facade,
};

use super::dialect::{PostgresHandle, PostgresSession};
use super::*;

// PostgreSQL indexes the raw keys, so a lookup uses them directly.
impl<H: PostgresHandle> SqlResourcePathSession for PostgresSession<H> {
    const LOCATOR_LOOKUP_COLUMN: &'static str = "locator_key";
    const PATH_LOOKUP_COLUMN: &'static str = "document_path_key";

    fn lookup_key(key: &[u8]) -> Cow<'_, [u8]> {
        Cow::Borrowed(key)
    }

    async fn upsert_binding_row(
        &mut self,
        schema_name: &str,
        row: ResourcePathRow<'_>,
    ) -> Result<()> {
        let query = format!(
            "INSERT INTO {} (
                locator_key,
                document_path_key,
                collection_group,
                binding_blob,
                locator_blob
             ) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(locator_key) DO UPDATE SET
                document_path_key = EXCLUDED.document_path_key,
                collection_group = EXCLUDED.collection_group,
                binding_blob = EXCLUDED.binding_blob,
                locator_blob = EXCLUDED.locator_blob",
            Self::table(schema_name, "resource_path_bindings")
        );
        self.execute_sql(
            &query,
            &[
                SqlValue::Bytes(row.locator_key),
                SqlValue::Bytes(row.document_path_key),
                SqlValue::Text(row.collection_group),
                SqlValue::Bytes(row.binding_blob),
                SqlValue::Bytes(row.locator_blob),
            ],
        )
        .await?;
        Ok(())
    }

    async fn collection_group_binding_rows(
        &mut self,
        schema_name: &str,
        collection_group: &CollectionName,
    ) -> Result<Vec<tokio_postgres::Row>> {
        let query = format!(
            "SELECT binding_blob FROM {} WHERE collection_group = ?",
            Self::table(schema_name, "resource_path_bindings")
        );
        self.fetch_all(&query, &[SqlValue::Text(collection_group.as_str())])
            .await
    }
}

impl SqlResourcePathStore for PostgresTenantStore {}

sql_resource_path_facade!(PostgresTenantStore, PostgresWriteTransaction);
