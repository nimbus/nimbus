use neovex_core::{
    CollectionName, DocumentLocator, DocumentPath, Error, ResourcePathBinding, Result,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::keys::{document_path_key, resource_locator_key};

use super::{SqliteReadSnapshot, SqliteTenantStore, SqliteWriteTransaction, map_sqlite_error};

impl SqliteTenantStore {
    pub fn upsert_resource_path_binding(&self, binding: &ResourcePathBinding) -> Result<()> {
        self.execute_write(|transaction| transaction.upsert_resource_path_binding(binding))?;
        Ok(())
    }

    pub fn remove_resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        Ok(self
            .execute_write(|transaction| transaction.remove_resource_path_binding(locator))?
            .value)
    }

    pub fn resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        self.read_snapshot()?.resource_path_binding(locator)
    }

    pub fn locator_for_document_path(
        &self,
        document_path: &DocumentPath,
    ) -> Result<Option<DocumentLocator>> {
        self.read_snapshot()?
            .locator_for_document_path(document_path)
    }

    pub fn scan_collection_group_bindings(
        &self,
        collection_group: &CollectionName,
    ) -> Result<Vec<ResourcePathBinding>> {
        self.read_snapshot()?
            .scan_collection_group_bindings(collection_group)
    }
}

impl SqliteWriteTransaction {
    pub fn resource_path_binding(
        &mut self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        self.check_cancel()?;
        let locator_key = resource_locator_key(locator);
        let conn = self.connection_mut()?;
        load_resource_path_binding_by_locator_key(conn, locator_key.as_slice())
    }

    pub fn upsert_resource_path_binding(&mut self, binding: &ResourcePathBinding) -> Result<()> {
        self.check_cancel()?;
        let path_key = document_path_key(&binding.document_path);
        let locator_key = resource_locator_key(&binding.locator);

        let existing_locator = {
            let conn = self.connection_mut()?;
            load_locator_for_document_path_key(conn, path_key.as_slice())?
        };
        if existing_locator
            .as_ref()
            .is_some_and(|locator| locator != &binding.locator)
        {
            return Err(Error::AlreadyExists(format!(
                "document path already bound: {}",
                binding.document_path
            )));
        }

        let existing_binding = {
            let conn = self.connection_mut()?;
            load_resource_path_binding_by_locator_key(conn, locator_key.as_slice())?
        };
        if existing_binding.as_ref() == Some(binding) {
            return Ok(());
        }

        let encoded_binding = encode_binding(binding)?;
        let encoded_locator = encode_locator(&binding.locator)?;
        self.connection_mut()?
            .execute(
                "INSERT INTO resource_path_bindings (
                    locator_key,
                    document_path_key,
                    collection_group,
                    binding_blob,
                    locator_blob
                 ) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(locator_key) DO UPDATE SET
                    document_path_key = excluded.document_path_key,
                    collection_group = excluded.collection_group,
                    binding_blob = excluded.binding_blob,
                    locator_blob = excluded.locator_blob",
                params![
                    locator_key.as_slice(),
                    path_key.as_slice(),
                    binding.collection_group().as_str(),
                    encoded_binding.as_slice(),
                    encoded_locator.as_slice(),
                ],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn remove_resource_path_binding(
        &mut self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        self.check_cancel()?;
        let locator_key = resource_locator_key(locator);
        let existing = {
            let conn = self.connection_mut()?;
            load_resource_path_binding_by_locator_key(conn, locator_key.as_slice())?
        };
        if existing.is_none() {
            return Ok(None);
        }
        self.connection_mut()?
            .execute(
                "DELETE FROM resource_path_bindings WHERE locator_key = ?1",
                params![locator_key.as_slice()],
            )
            .map_err(map_sqlite_error)?;
        Ok(existing)
    }
}

impl SqliteReadSnapshot {
    pub fn scan_resource_path_bindings(&self) -> Result<Vec<ResourcePathBinding>> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT binding_blob
                 FROM resource_path_bindings
                 ORDER BY document_path_key",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = statement.query([]).map_err(map_sqlite_error)?;
        let mut bindings = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            let blob = row.get::<_, Vec<u8>>(0).map_err(map_sqlite_error)?;
            bindings.push(decode_binding(blob.as_slice())?);
        }
        Ok(bindings)
    }

    pub fn resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        load_resource_path_binding_by_locator_key(
            &self.conn,
            resource_locator_key(locator).as_slice(),
        )
    }

    pub fn locator_for_document_path(
        &self,
        document_path: &DocumentPath,
    ) -> Result<Option<DocumentLocator>> {
        load_locator_for_document_path_key(&self.conn, document_path_key(document_path).as_slice())
    }

    pub fn scan_collection_group_bindings(
        &self,
        collection_group: &CollectionName,
    ) -> Result<Vec<ResourcePathBinding>> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT binding_blob
                 FROM resource_path_bindings
                 WHERE collection_group = ?1
                 ORDER BY document_path_key",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = statement
            .query(params![collection_group.as_str()])
            .map_err(map_sqlite_error)?;
        let mut bindings = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            let blob = row.get::<_, Vec<u8>>(0).map_err(map_sqlite_error)?;
            bindings.push(decode_binding(blob.as_slice())?);
        }
        Ok(bindings)
    }
}

fn load_resource_path_binding_by_locator_key(
    conn: &Connection,
    locator_key: &[u8],
) -> Result<Option<ResourcePathBinding>> {
    conn.query_row(
        "SELECT binding_blob
         FROM resource_path_bindings
         WHERE locator_key = ?1",
        params![locator_key],
        |row| {
            let blob = row.get::<_, Vec<u8>>(0)?;
            decode_binding(blob.as_slice()).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    blob.len(),
                    rusqlite::types::Type::Blob,
                    Box::new(std::io::Error::other(error.to_string())),
                )
            })
        },
    )
    .optional()
    .map_err(map_sqlite_error)
}

fn load_locator_for_document_path_key(
    conn: &Connection,
    document_path_key: &[u8],
) -> Result<Option<DocumentLocator>> {
    conn.query_row(
        "SELECT locator_blob
         FROM resource_path_bindings
         WHERE document_path_key = ?1",
        params![document_path_key],
        |row| {
            let blob = row.get::<_, Vec<u8>>(0)?;
            decode_locator(blob.as_slice()).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    blob.len(),
                    rusqlite::types::Type::Blob,
                    Box::new(std::io::Error::other(error.to_string())),
                )
            })
        },
    )
    .optional()
    .map_err(map_sqlite_error)
}

fn encode_binding(binding: &ResourcePathBinding) -> Result<Vec<u8>> {
    rmp_serde::to_vec(binding).map_err(|error| Error::Serialization(error.to_string()))
}

fn decode_binding(bytes: &[u8]) -> Result<ResourcePathBinding> {
    rmp_serde::from_slice(bytes).map_err(|error| Error::Serialization(error.to_string()))
}

fn encode_locator(locator: &DocumentLocator) -> Result<Vec<u8>> {
    rmp_serde::to_vec(locator).map_err(|error| Error::Serialization(error.to_string()))
}

fn decode_locator(bytes: &[u8]) -> Result<DocumentLocator> {
    rmp_serde::from_slice(bytes).map_err(|error| Error::Serialization(error.to_string()))
}
