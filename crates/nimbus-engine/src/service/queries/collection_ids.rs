use std::collections::BTreeSet;

use nimbus_core::{CollectionName, DocumentPath, ResourcePathBinding, Result, TenantId};

use crate::service::Service;

impl Service {
    /// Lists the immediate child collection ids underneath the database root
    /// or a specific ancestor document using shared resource path bindings.
    pub fn list_collection_ids_for_parent(
        &self,
        tenant_id: &TenantId,
        parent_document_path: Option<&DocumentPath>,
    ) -> Result<Vec<CollectionName>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let snapshot = runtime.store.read_snapshot()?;
        list_collection_ids_from_bindings(
            snapshot.scan_resource_path_bindings()?,
            parent_document_path,
        )
    }
}

fn list_collection_ids_from_bindings(
    bindings: Vec<ResourcePathBinding>,
    parent_document_path: Option<&DocumentPath>,
) -> Result<Vec<CollectionName>> {
    let mut collection_ids = BTreeSet::new();
    for binding in bindings {
        if let Some(collection_id) = binding
            .document_path
            .direct_child_collection_for_ancestor(parent_document_path)
        {
            collection_ids.insert(collection_id.clone());
        }
    }
    Ok(collection_ids.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use nimbus_core::{
        AtomicWrite, AtomicWriteBatch, DocumentId, DocumentLocator, PrincipalContext, TableName,
        WriteKey, WritePrecondition, WriteSetMode,
    };
    use nimbus_testing::ServiceFixture;
    use serde_json::{Map, json};
    use std::sync::Arc;

    use super::*;

    fn seed_bound_document(
        service: &Arc<Service>,
        tenant_id: &TenantId,
        table: &str,
        document_id: &str,
        document_path: &[&str],
    ) {
        let batch = AtomicWriteBatch::new(vec![AtomicWrite::Set {
            key: WriteKey::from(ResourcePathBinding::new(
                DocumentLocator::new(
                    TableName::new(table).expect("table name should parse"),
                    DocumentId::from_key(document_id).expect("document id should parse"),
                ),
                DocumentPath::from_segments(document_path.iter().copied())
                    .expect("document path should parse"),
            )),
            document: Map::from_iter([("name".to_string(), json!(document_id))]),
            mode: WriteSetMode::Overwrite,
            precondition: WritePrecondition::default(),
            transforms: Vec::new(),
        }])
        .expect("seed batch should build");
        service
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("seed execution unit should begin")
            .execute_atomic_write_batch(batch)
            .expect("seed write should commit");
    }

    #[test]
    fn list_collection_ids_for_parent_reads_path_metadata_for_root_and_nested_parents() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
        let service = fixture.service();

        seed_bound_document(&service, &tenant_id, "cities_root", "SF", &["cities", "SF"]);
        seed_bound_document(
            &service,
            &tenant_id,
            "cities_root",
            "NYC",
            &["cities", "NYC"],
        );
        seed_bound_document(
            &service,
            &tenant_id,
            "countries_root",
            "JP",
            &["countries", "JP"],
        );
        seed_bound_document(
            &service,
            &tenant_id,
            "landmarks_store",
            "bridge",
            &["cities", "SF", "landmarks", "bridge"],
        );
        seed_bound_document(
            &service,
            &tenant_id,
            "neighborhoods_store",
            "soma",
            &["cities", "SF", "neighborhoods", "soma"],
        );
        seed_bound_document(
            &service,
            &tenant_id,
            "photos_store",
            "p1",
            &["cities", "SF", "landmarks", "bridge", "photos", "p1"],
        );

        assert_eq!(
            service
                .list_collection_ids_for_parent(&tenant_id, None)
                .expect("root collection ids should load")
                .into_iter()
                .map(|collection| collection.to_string())
                .collect::<Vec<_>>(),
            vec!["cities".to_string(), "countries".to_string()],
        );
        assert_eq!(
            service
                .list_collection_ids_for_parent(
                    &tenant_id,
                    Some(
                        &DocumentPath::from_segments(["cities", "SF"])
                            .expect("parent path should parse"),
                    ),
                )
                .expect("nested collection ids should load")
                .into_iter()
                .map(|collection| collection.to_string())
                .collect::<Vec<_>>(),
            vec!["landmarks".to_string(), "neighborhoods".to_string()],
        );
        assert_eq!(
            service
                .list_collection_ids_for_parent(
                    &tenant_id,
                    Some(
                        &DocumentPath::from_segments(["cities", "SF", "landmarks", "bridge"])
                            .expect("deep parent path should parse"),
                    ),
                )
                .expect("deep collection ids should load")
                .into_iter()
                .map(|collection| collection.to_string())
                .collect::<Vec<_>>(),
            vec!["photos".to_string()],
        );
        assert!(
            service
                .list_collection_ids_for_parent(
                    &tenant_id,
                    Some(
                        &DocumentPath::from_segments(["cities", "NYC"])
                            .expect("leaf parent path should parse"),
                    ),
                )
                .expect("leaf collection ids should load")
                .is_empty(),
            "documents without nested subcollections should return no child collection ids"
        );
    }
}
