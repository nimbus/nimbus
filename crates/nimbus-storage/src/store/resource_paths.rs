use nimbus_core::{
    CollectionName, DocumentLocator, DocumentPath, Error, ResourcePathBinding, Result,
};
use redb::{ReadableTable, TableError};

use crate::keys::{
    collection_group_binding_key, collection_group_prefix, document_path_key, prefix_end,
    resource_locator_key,
};

use super::{
    COLLECTION_GROUP_BINDINGS, RESOURCE_PATH_BINDINGS, RESOURCE_PATH_LOOKUP, TenantReadSnapshot,
    TenantStore, TenantWriteTransaction, map_redb_error,
};

impl TenantStore {
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

impl TenantWriteTransaction {
    pub fn upsert_resource_path_binding(&mut self, binding: &ResourcePathBinding) -> Result<()> {
        self.check_cancel()?;
        upsert_resource_path_binding_in_write_txn(self.write_txn()?, binding)
    }

    pub fn remove_resource_path_binding(
        &mut self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        self.check_cancel()?;
        remove_resource_path_binding_in_write_txn(self.write_txn()?, locator)
    }
}

pub(super) fn upsert_resource_path_binding_in_write_txn(
    write_txn: &redb::WriteTransaction,
    binding: &ResourcePathBinding,
) -> Result<()> {
    let locator_key = resource_locator_key(&binding.locator);
    let path_key = document_path_key(&binding.document_path);
    let collection_group_key = collection_group_binding_key(binding);
    let encoded_binding = encode_binding(binding)?;
    let encoded_locator = encode_locator(&binding.locator)?;

    {
        let lookup = write_txn
            .open_table(RESOURCE_PATH_LOOKUP)
            .map_err(map_redb_error)?;
        if let Some(existing_locator) = lookup
            .get(path_key.as_slice())
            .map_err(map_redb_error)?
            .map(|value| decode_locator(value.value()))
            .transpose()?
            && existing_locator != binding.locator
        {
            return Err(Error::AlreadyExists(format!(
                "document path already bound: {}",
                binding.document_path
            )));
        }
    }

    let existing_binding = {
        let bindings = write_txn
            .open_table(RESOURCE_PATH_BINDINGS)
            .map_err(map_redb_error)?;
        bindings
            .get(locator_key.as_slice())
            .map_err(map_redb_error)?
            .map(|value| decode_binding(value.value()))
            .transpose()?
    };
    if existing_binding.as_ref() == Some(binding) {
        return Ok(());
    }

    if let Some(existing_binding) = existing_binding.as_ref() {
        let existing_path_key = document_path_key(&existing_binding.document_path);
        let existing_group_key = collection_group_binding_key(existing_binding);
        {
            let mut lookup = write_txn
                .open_table(RESOURCE_PATH_LOOKUP)
                .map_err(map_redb_error)?;
            lookup
                .remove(existing_path_key.as_slice())
                .map_err(map_redb_error)?;
        }
        {
            let mut collection_groups = write_txn
                .open_table(COLLECTION_GROUP_BINDINGS)
                .map_err(map_redb_error)?;
            collection_groups
                .remove(existing_group_key.as_slice())
                .map_err(map_redb_error)?;
        }
    }

    {
        let mut bindings = write_txn
            .open_table(RESOURCE_PATH_BINDINGS)
            .map_err(map_redb_error)?;
        bindings
            .insert(locator_key.as_slice(), encoded_binding.as_slice())
            .map_err(map_redb_error)?;
    }
    {
        let mut lookup = write_txn
            .open_table(RESOURCE_PATH_LOOKUP)
            .map_err(map_redb_error)?;
        lookup
            .insert(path_key.as_slice(), encoded_locator.as_slice())
            .map_err(map_redb_error)?;
    }
    {
        let mut collection_groups = write_txn
            .open_table(COLLECTION_GROUP_BINDINGS)
            .map_err(map_redb_error)?;
        collection_groups
            .insert(collection_group_key.as_slice(), encoded_binding.as_slice())
            .map_err(map_redb_error)?;
    }
    Ok(())
}

pub(super) fn remove_resource_path_binding_in_write_txn(
    write_txn: &redb::WriteTransaction,
    locator: &DocumentLocator,
) -> Result<Option<ResourcePathBinding>> {
    let locator_key = resource_locator_key(locator);
    let existing_binding = {
        let mut bindings = write_txn
            .open_table(RESOURCE_PATH_BINDINGS)
            .map_err(map_redb_error)?;
        bindings
            .remove(locator_key.as_slice())
            .map_err(map_redb_error)?
            .map(|value| decode_binding(value.value()))
            .transpose()?
    };

    let Some(existing_binding) = existing_binding else {
        return Ok(None);
    };

    let existing_path_key = document_path_key(&existing_binding.document_path);
    let existing_group_key = collection_group_binding_key(&existing_binding);
    {
        let mut lookup = write_txn
            .open_table(RESOURCE_PATH_LOOKUP)
            .map_err(map_redb_error)?;
        lookup
            .remove(existing_path_key.as_slice())
            .map_err(map_redb_error)?;
    }
    {
        let mut collection_groups = write_txn
            .open_table(COLLECTION_GROUP_BINDINGS)
            .map_err(map_redb_error)?;
        collection_groups
            .remove(existing_group_key.as_slice())
            .map_err(map_redb_error)?;
    }
    Ok(Some(existing_binding))
}

impl TenantReadSnapshot {
    pub fn scan_resource_path_bindings(&self) -> Result<Vec<ResourcePathBinding>> {
        let bindings = match self.read_txn.open_table(RESOURCE_PATH_BINDINGS) {
            Ok(bindings) => bindings,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };

        let mut results = Vec::new();
        for item in bindings.iter().map_err(map_redb_error)? {
            let (_, value) = item.map_err(map_redb_error)?;
            results.push(decode_binding(value.value())?);
        }
        Ok(results)
    }

    pub fn resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        let bindings = match self.read_txn.open_table(RESOURCE_PATH_BINDINGS) {
            Ok(bindings) => bindings,
            Err(TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(map_redb_error(error)),
        };
        bindings
            .get(resource_locator_key(locator).as_slice())
            .map_err(map_redb_error)?
            .map(|value| decode_binding(value.value()))
            .transpose()
    }

    pub fn locator_for_document_path(
        &self,
        document_path: &DocumentPath,
    ) -> Result<Option<DocumentLocator>> {
        let lookup = match self.read_txn.open_table(RESOURCE_PATH_LOOKUP) {
            Ok(lookup) => lookup,
            Err(TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(map_redb_error(error)),
        };
        lookup
            .get(document_path_key(document_path).as_slice())
            .map_err(map_redb_error)?
            .map(|value| decode_locator(value.value()))
            .transpose()
    }

    pub fn scan_collection_group_bindings(
        &self,
        collection_group: &CollectionName,
    ) -> Result<Vec<ResourcePathBinding>> {
        let collection_groups = match self.read_txn.open_table(COLLECTION_GROUP_BINDINGS) {
            Ok(collection_groups) => collection_groups,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };
        let prefix = collection_group_prefix(collection_group);
        let end = prefix_end(&prefix);

        let mut bindings = Vec::new();
        if let Some(end) = end {
            for item in collection_groups
                .range(prefix.as_slice()..end.as_slice())
                .map_err(map_redb_error)?
            {
                let (key, value) = item.map_err(map_redb_error)?;
                if !key.value().starts_with(prefix.as_slice()) {
                    break;
                }
                bindings.push(decode_binding(value.value())?);
            }
        } else {
            for item in collection_groups
                .range(prefix.as_slice()..)
                .map_err(map_redb_error)?
            {
                let (key, value) = item.map_err(map_redb_error)?;
                if !key.value().starts_with(prefix.as_slice()) {
                    break;
                }
                bindings.push(decode_binding(value.value())?);
            }
        }
        Ok(bindings)
    }
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

#[cfg(test)]
mod tests {
    use nimbus_core::{DocumentId, TableName};

    use super::*;

    fn binding(table: &str, id: &str, path: &[&str]) -> ResourcePathBinding {
        ResourcePathBinding::new(
            DocumentLocator::new(
                TableName::new(table).expect("table name should parse"),
                DocumentId::from_key(id).expect("document id should parse"),
            ),
            DocumentPath::from_segments(path).expect("document path should parse"),
        )
    }

    #[test]
    fn resource_path_binding_roundtrips_cities_sf() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let binding = binding("cities_store", "loc_sf", &["cities", "SF"]);

        store
            .upsert_resource_path_binding(&binding)
            .expect("binding should persist");

        assert_eq!(
            store
                .resource_path_binding(&binding.locator)
                .expect("binding lookup should succeed"),
            Some(binding.clone())
        );
        assert_eq!(
            store
                .locator_for_document_path(&binding.document_path)
                .expect("path lookup should succeed"),
            Some(binding.locator.clone())
        );
    }

    #[test]
    fn resource_path_binding_roundtrips_deep_subcollection_path() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let binding = binding(
            "collection_c_store",
            "loc_c3",
            &["a", "1", "b", "2", "c", "3"],
        );

        store
            .upsert_resource_path_binding(&binding)
            .expect("binding should persist");

        assert_eq!(
            store
                .resource_path_binding(&binding.locator)
                .expect("binding lookup should succeed"),
            Some(binding)
        );
    }

    #[test]
    fn resource_path_binding_accepts_reserved_dotted_and_unicode_collection_names() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let bindings = vec![
            binding("reserved_store", "loc_reserved", &["__meta__", "doc-1"]),
            binding("dotted_store", "loc_dotted", &["cities.v2", "SF"]),
            binding("unicode_store", "loc_unicode", &["日本語", "東京"]),
        ];

        for binding in &bindings {
            store
                .upsert_resource_path_binding(binding)
                .expect("binding should persist");
        }

        for binding in bindings {
            assert_eq!(
                store
                    .locator_for_document_path(&binding.document_path)
                    .expect("path lookup should succeed"),
                Some(binding.locator)
            );
        }
    }

    #[test]
    fn collection_group_scan_returns_nested_paths_without_parent_field_tricks() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let bindings = vec![
            binding(
                "landmarks_sf_store",
                "loc_landmark_sf",
                &["cities", "SF", "landmarks", "GG"],
            ),
            binding(
                "landmarks_nyc_store",
                "loc_landmark_nyc",
                &["cities", "NYC", "landmarks", "GG"],
            ),
            binding(
                "landmarks_unicode_store",
                "loc_landmark_unicode",
                &["日本語", "東京", "landmarks", "大阪城"],
            ),
        ];

        for binding in &bindings {
            store
                .upsert_resource_path_binding(binding)
                .expect("binding should persist");
        }

        let mut found_paths = store
            .scan_collection_group_bindings(
                &CollectionName::new("landmarks").expect("collection group should parse"),
            )
            .expect("collection-group scan should succeed")
            .into_iter()
            .map(|binding| binding.document_path.to_string())
            .collect::<Vec<_>>();
        found_paths.sort();

        assert_eq!(
            found_paths,
            vec![
                "cities/NYC/landmarks/GG".to_string(),
                "cities/SF/landmarks/GG".to_string(),
                "日本語/東京/landmarks/大阪城".to_string(),
            ]
        );
    }

    #[test]
    fn duplicate_document_path_binding_is_rejected() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let original = binding(
            "landmarks_store_a",
            "loc_a",
            &["cities", "SF", "landmarks", "GG"],
        );
        let conflicting = binding(
            "landmarks_store_b",
            "loc_b",
            &["cities", "SF", "landmarks", "GG"],
        );

        store
            .upsert_resource_path_binding(&original)
            .expect("original binding should persist");
        let error = store
            .upsert_resource_path_binding(&conflicting)
            .expect_err("duplicate path should fail");

        assert!(matches!(error, Error::AlreadyExists(_)));
    }
}
