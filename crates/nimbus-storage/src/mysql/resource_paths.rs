use nimbus_core::{
    CollectionName, DocumentLocator, DocumentPath, Error, ResourcePathBinding, Result,
};

use crate::keys::{document_path_key, resource_locator_key};

use super::*;

impl MySqlTenantStore {
    pub fn upsert_resource_path_binding(&self, binding: &ResourcePathBinding) -> Result<()> {
        let binding = binding.clone();
        self.execute_write(move |transaction| transaction.upsert_resource_path_binding(&binding))?;
        Ok(())
    }

    pub fn remove_resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        let locator = locator.clone();
        Ok(self
            .execute_write(move |transaction| transaction.remove_resource_path_binding(&locator))?
            .value)
    }

    pub fn resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let locator_key = resource_locator_key(locator);
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_resource_path_binding_by_locator_key_from_session(
                &mut conn,
                &database_name,
                locator_key.as_slice(),
            )
            .await
        })
    }

    pub fn locator_for_document_path(
        &self,
        document_path: &DocumentPath,
    ) -> Result<Option<DocumentLocator>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let path_key = document_path_key(document_path);
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_locator_for_document_path_key_from_session(
                &mut conn,
                &database_name,
                path_key.as_slice(),
            )
            .await
        })
    }

    pub fn scan_collection_group_bindings(
        &self,
        collection_group: &CollectionName,
    ) -> Result<Vec<ResourcePathBinding>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let collection_group = collection_group.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_collection_group_bindings_from_session(
                &mut conn,
                &database_name,
                &collection_group,
            )
            .await
        })
    }
}

impl MySqlWriteTransaction {
    pub fn resource_path_binding(
        &mut self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        self.check_cancel()?;
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let conn = self.session()?;
        let locator_key = resource_locator_key(locator);
        Self::block_on(&runtime_handle, async move {
            load_resource_path_binding_by_locator_key_from_session(
                conn,
                &database_name,
                locator_key.as_slice(),
            )
            .await
        })
    }

    pub fn upsert_resource_path_binding(&mut self, binding: &ResourcePathBinding) -> Result<()> {
        self.check_cancel()?;
        let locator_key = resource_locator_key(&binding.locator);
        let path_key = document_path_key(&binding.document_path);

        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let conn = self.session()?;
        let path_key_for_lookup = path_key.clone();
        let existing_locator = Self::block_on(&runtime_handle, async move {
            load_locator_for_document_path_key_from_session(
                conn,
                &database_name,
                path_key_for_lookup.as_slice(),
            )
            .await
        })?;
        if existing_locator
            .as_ref()
            .is_some_and(|locator| locator != &binding.locator)
        {
            return Err(Error::AlreadyExists(format!(
                "document path already bound: {}",
                binding.document_path
            )));
        }

        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let conn = self.session()?;
        let locator_key_for_lookup = locator_key.clone();
        let existing_binding = Self::block_on(&runtime_handle, async move {
            load_resource_path_binding_by_locator_key_from_session(
                conn,
                &database_name,
                locator_key_for_lookup.as_slice(),
            )
            .await
        })?;
        if existing_binding.as_ref() == Some(binding) {
            return Ok(());
        }

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
            qualified_table(&self.database_name, "resource_path_bindings")
        );
        let locator_hash = hashed_key(locator_key.as_slice());
        let path_hash = hashed_key(path_key.as_slice());
        let collection_group_hash = hashed_key(binding.collection_group().as_str().as_bytes());
        let binding_blob = encode_binding(binding)?;
        let locator_blob = encode_locator(&binding.locator)?;
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(
                query,
                (
                    locator_hash,
                    locator_key,
                    path_hash,
                    path_key,
                    collection_group_hash,
                    binding_blob,
                    locator_blob,
                ),
            )
            .await
            .map_err(map_mysql_error)
        })
    }

    pub fn remove_resource_path_binding(
        &mut self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        self.check_cancel()?;
        let locator_key = resource_locator_key(locator);
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let conn = self.session()?;
        let locator_key_for_lookup = locator_key.clone();
        let existing = Self::block_on(&runtime_handle, async move {
            load_resource_path_binding_by_locator_key_from_session(
                conn,
                &database_name,
                locator_key_for_lookup.as_slice(),
            )
            .await
        })?;
        if existing.is_none() {
            return Ok(None);
        }

        let query = format!(
            "DELETE FROM {} WHERE locator_hash = ?",
            qualified_table(&self.database_name, "resource_path_bindings")
        );
        let locator_hash = hashed_key(locator_key.as_slice());
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (locator_hash,))
                .await
                .map_err(map_mysql_error)
        })?;
        Ok(existing)
    }
}

impl MySqlReadSnapshot {
    pub fn scan_resource_path_bindings(&self) -> Result<Vec<ResourcePathBinding>> {
        let mut bindings = self.resource_path_bindings.clone();
        bindings.sort_by_key(|binding| document_path_key(&binding.document_path));
        Ok(bindings)
    }

    pub fn resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        Ok(self
            .resource_path_bindings
            .iter()
            .find(|binding| &binding.locator == locator)
            .cloned())
    }

    pub fn locator_for_document_path(
        &self,
        document_path: &DocumentPath,
    ) -> Result<Option<DocumentLocator>> {
        Ok(self
            .resource_path_bindings
            .iter()
            .find(|binding| &binding.document_path == document_path)
            .map(|binding| binding.locator.clone()))
    }

    pub fn scan_collection_group_bindings(
        &self,
        collection_group: &CollectionName,
    ) -> Result<Vec<ResourcePathBinding>> {
        let mut bindings = self
            .resource_path_bindings
            .iter()
            .filter(|binding| binding.collection_group() == collection_group)
            .cloned()
            .collect::<Vec<_>>();
        bindings.sort_by_key(|binding| document_path_key(&binding.document_path));
        Ok(bindings)
    }
}

pub(super) async fn load_resource_path_bindings_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<Vec<ResourcePathBinding>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT binding_blob FROM {}",
        qualified_table(database_name, "resource_path_bindings")
    );
    let rows: Vec<Row> = session.query(query).await.map_err(map_mysql_error)?;
    let mut bindings = rows
        .into_iter()
        .map(|row| {
            let (binding_blob,): (Vec<u8>,) = mysql_async::from_row(row);
            decode_binding(binding_blob.as_slice())
        })
        .collect::<Result<Vec<_>>>()?;
    bindings.sort_by_key(|binding| document_path_key(&binding.document_path));
    Ok(bindings)
}

async fn load_resource_path_binding_by_locator_key_from_session<C>(
    session: &mut C,
    database_name: &str,
    locator_key: &[u8],
) -> Result<Option<ResourcePathBinding>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT locator_key, binding_blob FROM {} WHERE locator_hash = ?",
        qualified_table(database_name, "resource_path_bindings")
    );
    let row = session
        .exec_first::<Row, _, _>(query, (hashed_key(locator_key),))
        .await
        .map_err(map_mysql_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let (stored_locator_key, binding_blob): (Vec<u8>, Vec<u8>) = mysql_async::from_row(row);
    if stored_locator_key != locator_key {
        return Err(Error::Internal(
            "MySQL resource locator hash collision while loading path binding".to_string(),
        ));
    }
    decode_binding(binding_blob.as_slice()).map(Some)
}

async fn load_locator_for_document_path_key_from_session<C>(
    session: &mut C,
    database_name: &str,
    document_path_key: &[u8],
) -> Result<Option<DocumentLocator>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT document_path_key, locator_blob FROM {} WHERE document_path_hash = ?",
        qualified_table(database_name, "resource_path_bindings")
    );
    let row = session
        .exec_first::<Row, _, _>(query, (hashed_key(document_path_key),))
        .await
        .map_err(map_mysql_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let (stored_document_path_key, locator_blob): (Vec<u8>, Vec<u8>) = mysql_async::from_row(row);
    if stored_document_path_key != document_path_key {
        return Err(Error::Internal(
            "MySQL document path hash collision while loading resource locator".to_string(),
        ));
    }
    decode_locator(locator_blob.as_slice()).map(Some)
}

async fn load_collection_group_bindings_from_session<C>(
    session: &mut C,
    database_name: &str,
    collection_group: &CollectionName,
) -> Result<Vec<ResourcePathBinding>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT binding_blob FROM {} WHERE collection_group_hash = ?",
        qualified_table(database_name, "resource_path_bindings")
    );
    let rows = session
        .exec::<Row, _, _>(query, (hashed_key(collection_group.as_str().as_bytes()),))
        .await
        .map_err(map_mysql_error)?;
    let mut bindings = rows
        .into_iter()
        .map(|row| {
            let (binding_blob,): (Vec<u8>,) = mysql_async::from_row(row);
            decode_binding(binding_blob.as_slice())
        })
        .collect::<Result<Vec<_>>>()?;
    bindings.retain(|binding| binding.collection_group() == collection_group);
    bindings.sort_by_key(|binding| document_path_key(&binding.document_path));
    Ok(bindings)
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

fn hashed_key(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}
