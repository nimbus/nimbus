use std::borrow::Cow;

use nimbus_core::{CollectionName, Result};

use crate::sql::dialect::{SqlSession, SqlValue};
use crate::sql::resource_paths::{
    ResourcePathRow, SqlResourcePathSession, SqlResourcePathStore, sql_resource_path_facade,
};

use super::dialect::{MySqlHandle, MySqlSession};
use super::*;

// MySQL cannot index unbounded keys, so it indexes their SHA-256 hashes. The
// shared lookups compare the stored raw key to detect a hash collision.
impl<H: MySqlHandle> SqlResourcePathSession for MySqlSession<H> {
    const LOCATOR_LOOKUP_COLUMN: &'static str = "locator_hash";
    const PATH_LOOKUP_COLUMN: &'static str = "document_path_hash";

    fn lookup_key(key: &[u8]) -> Cow<'_, [u8]> {
        Cow::Owned(hashed_key(key))
    }

    async fn upsert_binding_row(
        &mut self,
        database_name: &str,
        row: ResourcePathRow<'_>,
    ) -> Result<()> {
        let query = format!(
            "INSERT INTO {} (
                locator_hash,
                locator_key,
                document_path_hash,
                document_path_key,
                collection_group_hash,
                binding_blob,
                locator_blob
             ) VALUES (?, ?, ?, ?, ?, ?, ?)
             ON DUPLICATE KEY UPDATE
                locator_key = VALUES(locator_key),
                document_path_hash = VALUES(document_path_hash),
                document_path_key = VALUES(document_path_key),
                collection_group_hash = VALUES(collection_group_hash),
                binding_blob = VALUES(binding_blob),
                locator_blob = VALUES(locator_blob)",
            Self::table(database_name, "resource_path_bindings")
        );
        let locator_hash = hashed_key(row.locator_key);
        let path_hash = hashed_key(row.document_path_key);
        let collection_group_hash = hashed_key(row.collection_group.as_bytes());
        self.execute_sql(
            &query,
            &[
                SqlValue::Bytes(&locator_hash),
                SqlValue::Bytes(row.locator_key),
                SqlValue::Bytes(&path_hash),
                SqlValue::Bytes(row.document_path_key),
                SqlValue::Bytes(&collection_group_hash),
                SqlValue::Bytes(row.binding_blob),
                SqlValue::Bytes(row.locator_blob),
            ],
        )
        .await?;
        Ok(())
    }

    async fn collection_group_binding_rows(
        &mut self,
        database_name: &str,
        collection_group: &CollectionName,
    ) -> Result<Vec<Row>> {
        let query = format!(
            "SELECT binding_blob FROM {} WHERE collection_group_hash = ?",
            Self::table(database_name, "resource_path_bindings")
        );
        let collection_group_hash = hashed_key(collection_group.as_str().as_bytes());
        self.fetch_all(&query, &[SqlValue::Bytes(&collection_group_hash)])
            .await
    }
}

impl SqlResourcePathStore for MySqlTenantStore {}

sql_resource_path_facade!(MySqlTenantStore, MySqlWriteTransaction);

fn hashed_key(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}
