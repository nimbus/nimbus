use neovex_core::{
    CollectionName, DocumentLocator, DocumentPath, Error, ResourcePathBinding, Result,
};

use crate::keys::{document_path_key, resource_locator_key};

use super::*;

impl PostgresTenantStore {
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
        let schema_name = self.schema_name.clone();
        let locator_key = resource_locator_key(locator);
        self.block_on(async move {
            let client = provider.client().await?;
            load_resource_path_binding_by_locator_key_from_session(
                &client,
                &schema_name,
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
        let schema_name = self.schema_name.clone();
        let path_key = document_path_key(document_path);
        self.block_on(async move {
            let client = provider.client().await?;
            load_locator_for_document_path_key_from_session(
                &client,
                &schema_name,
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
        let schema_name = self.schema_name.clone();
        let collection_group = collection_group.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_collection_group_bindings_from_session(&client, &schema_name, &collection_group)
                .await
        })
    }
}

impl PostgresWriteTransaction {
    pub fn resource_path_binding(
        &mut self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        self.check_cancel()?;
        let schema_name = self.schema_name.clone();
        let client = self.session()?;
        let locator_key = resource_locator_key(locator);
        self.block_on(async move {
            load_resource_path_binding_by_locator_key_from_session(
                client,
                &schema_name,
                locator_key.as_slice(),
            )
            .await
        })
    }

    pub fn upsert_resource_path_binding(&mut self, binding: &ResourcePathBinding) -> Result<()> {
        self.check_cancel()?;
        let locator_key = resource_locator_key(&binding.locator);
        let path_key = document_path_key(&binding.document_path);

        let schema_name = self.schema_name.clone();
        let client = self.session()?;
        let path_key_for_lookup = path_key.clone();
        let existing_locator = self.block_on(async move {
            load_locator_for_document_path_key_from_session(
                client,
                &schema_name,
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

        let schema_name = self.schema_name.clone();
        let client = self.session()?;
        let locator_key_for_lookup = locator_key.clone();
        let existing_binding = self.block_on(async move {
            load_resource_path_binding_by_locator_key_from_session(
                client,
                &schema_name,
                locator_key_for_lookup.as_slice(),
            )
            .await
        })?;
        if existing_binding.as_ref() == Some(binding) {
            return Ok(());
        }

        let query = format!(
            "INSERT INTO {} (
                locator_key,
                document_path_key,
                collection_group,
                binding_blob,
                locator_blob
             ) VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT(locator_key) DO UPDATE SET
                document_path_key = EXCLUDED.document_path_key,
                collection_group = EXCLUDED.collection_group,
                binding_blob = EXCLUDED.binding_blob,
                locator_blob = EXCLUDED.locator_blob",
            qualified_table(&self.schema_name, "resource_path_bindings")
        );
        let collection_group = binding.collection_group().as_str().to_string();
        let binding_blob = encode_binding(binding)?;
        let locator_blob = encode_locator(&binding.locator)?;
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(
                    query.as_str(),
                    &[
                        &locator_key,
                        &path_key,
                        &collection_group,
                        &binding_blob,
                        &locator_blob,
                    ],
                )
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })
    }

    pub fn remove_resource_path_binding(
        &mut self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        self.check_cancel()?;
        let locator_key = resource_locator_key(locator);
        let schema_name = self.schema_name.clone();
        let client = self.session()?;
        let locator_key_for_lookup = locator_key.clone();
        let existing = self.block_on(async move {
            load_resource_path_binding_by_locator_key_from_session(
                client,
                &schema_name,
                locator_key_for_lookup.as_slice(),
            )
            .await
        })?;
        if existing.is_none() {
            return Ok(None);
        }

        let query = format!(
            "DELETE FROM {} WHERE locator_key = $1",
            qualified_table(&self.schema_name, "resource_path_bindings")
        );
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&locator_key])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })?;
        Ok(existing)
    }
}

impl PostgresReadSnapshot {
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
    session: &C,
    schema_name: &str,
) -> Result<Vec<ResourcePathBinding>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT binding_blob
         FROM {}
         ORDER BY document_path_key",
        qualified_table(schema_name, "resource_path_bindings")
    );
    let rows = session
        .query(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    rows.into_iter()
        .map(|row| decode_binding(row.get::<_, Vec<u8>>(0).as_slice()))
        .collect()
}

async fn load_resource_path_binding_by_locator_key_from_session<C>(
    session: &C,
    schema_name: &str,
    locator_key: &[u8],
) -> Result<Option<ResourcePathBinding>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT binding_blob FROM {} WHERE locator_key = $1",
        qualified_table(schema_name, "resource_path_bindings")
    );
    session
        .query_opt(query.as_str(), &[&locator_key])
        .await
        .map_err(map_postgres_error)?
        .map(|row| decode_binding(row.get::<_, Vec<u8>>(0).as_slice()))
        .transpose()
}

async fn load_locator_for_document_path_key_from_session<C>(
    session: &C,
    schema_name: &str,
    document_path_key: &[u8],
) -> Result<Option<DocumentLocator>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT locator_blob FROM {} WHERE document_path_key = $1",
        qualified_table(schema_name, "resource_path_bindings")
    );
    session
        .query_opt(query.as_str(), &[&document_path_key])
        .await
        .map_err(map_postgres_error)?
        .map(|row| decode_locator(row.get::<_, Vec<u8>>(0).as_slice()))
        .transpose()
}

async fn load_collection_group_bindings_from_session<C>(
    session: &C,
    schema_name: &str,
    collection_group: &CollectionName,
) -> Result<Vec<ResourcePathBinding>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT binding_blob
         FROM {}
         WHERE collection_group = $1
         ORDER BY document_path_key",
        qualified_table(schema_name, "resource_path_bindings")
    );
    let collection_group = collection_group.as_str().to_string();
    let rows = session
        .query(query.as_str(), &[&collection_group])
        .await
        .map_err(map_postgres_error)?;
    rows.into_iter()
        .map(|row| decode_binding(row.get::<_, Vec<u8>>(0).as_slice()))
        .collect()
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
