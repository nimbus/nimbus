use nimbus_core::{
    CollectionName, DocumentLocator, DocumentPath, Error, ResourcePathBinding, Result,
};

use crate::keys::{document_path_key, resource_locator_key};

use super::{LibsqlReplicaWriteTransaction, map_libsql_error};

impl super::LibsqlReplicaTenantStore {
    pub fn upsert_resource_path_binding(&self, binding: &ResourcePathBinding) -> Result<()> {
        let binding = binding.clone();
        self.execute_write(move |transaction| transaction.upsert_resource_path_binding(&binding))?;
        Ok(())
    }

    pub fn resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        self.active_cache_store()?.resource_path_binding(locator)
    }

    pub fn locator_for_document_path(
        &self,
        document_path: &DocumentPath,
    ) -> Result<Option<DocumentLocator>> {
        self.active_cache_store()?
            .locator_for_document_path(document_path)
    }

    pub fn scan_collection_group_bindings(
        &self,
        collection_group: &CollectionName,
    ) -> Result<Vec<ResourcePathBinding>> {
        self.active_cache_store()?
            .scan_collection_group_bindings(collection_group)
    }
}

impl LibsqlReplicaWriteTransaction {
    pub fn upsert_resource_path_binding(&mut self, binding: &ResourcePathBinding) -> Result<()> {
        self.check_cancel()?;
        let locator_key = resource_locator_key(&binding.locator);
        let path_key = document_path_key(&binding.document_path);

        let existing_locator = self
            .store
            .block_on(load_locator_for_document_path_key_remote(
                self.session()?,
                path_key.clone(),
            ))?;
        if existing_locator
            .as_ref()
            .is_some_and(|locator| locator != &binding.locator)
        {
            return Err(Error::AlreadyExists(format!(
                "document path already bound: {}",
                binding.document_path
            )));
        }

        let existing_binding = self.store.block_on(load_resource_path_binding_remote(
            self.session()?,
            locator_key.clone(),
        ))?;
        if existing_binding.as_ref() == Some(binding) {
            return Ok(());
        }

        let binding_blob = encode_binding(binding)?;
        let locator_blob = encode_locator(&binding.locator)?;
        self.store.block_on(async {
            self.session()?
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
                    libsql::params![
                        locator_key,
                        path_key,
                        binding.collection_group().as_str().to_string(),
                        binding_blob,
                        locator_blob,
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok::<(), Error>(())
        })
    }

    pub fn remove_resource_path_binding(
        &mut self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        self.check_cancel()?;
        let locator_key = resource_locator_key(locator);
        let existing = self.store.block_on(load_resource_path_binding_remote(
            self.session()?,
            locator_key.clone(),
        ))?;
        if existing.is_none() {
            return Ok(None);
        }
        self.store.block_on(async {
            self.session()?
                .execute(
                    "DELETE FROM resource_path_bindings WHERE locator_key = ?1",
                    libsql::params![locator_key],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok::<(), Error>(())
        })?;
        Ok(existing)
    }
}

async fn load_resource_path_binding_remote(
    conn: &libsql::Transaction,
    locator_key: Vec<u8>,
) -> Result<Option<ResourcePathBinding>> {
    let mut rows = conn
        .query(
            "SELECT binding_blob FROM resource_path_bindings WHERE locator_key = ?1",
            libsql::params![locator_key],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(None);
    };
    let blob = row.get::<Vec<u8>>(0).map_err(map_libsql_error)?;
    decode_binding(blob.as_slice()).map(Some)
}

async fn load_locator_for_document_path_key_remote(
    conn: &libsql::Transaction,
    document_path_key: Vec<u8>,
) -> Result<Option<DocumentLocator>> {
    let mut rows = conn
        .query(
            "SELECT locator_blob FROM resource_path_bindings WHERE document_path_key = ?1",
            libsql::params![document_path_key],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(None);
    };
    let blob = row.get::<Vec<u8>>(0).map_err(map_libsql_error)?;
    decode_locator(blob.as_slice()).map(Some)
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
