use super::support::*;
use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, CollectionName, DocumentId, DocumentLocator, DocumentPath,
    FieldReference, FieldSchema, FieldType, IndexDefinition, PrincipalContext, QueryDirection,
    ResourcePathBinding, StructuredCursor, StructuredOrder, StructuredQuery, WriteKey,
    WritePrecondition, WriteSetMode,
};

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn typed_postgres_config_collection_group_queries_use_path_binding_metadata() {
    with_postgres_service_config(|service_config, _provider_config| async move {
        let tenant_id = TenantId::new("pg-collection-group").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should create"),
        );
        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");

        let direct_table = TableName::new("landmarks_direct").expect("table name should build");
        let nested_table = TableName::new("landmarks_nested").expect("table name should build");
        let other_table = TableName::new("landmarks_other").expect("table name should build");
        for table in [&direct_table, &nested_table, &other_table] {
            service
                .set_table_schema_async(
                    tenant_id.clone(),
                    TableSchema {
                        table: table.clone(),
                        fields: vec![FieldSchema {
                            name: "rank".to_string(),
                            field_type: FieldType::Number,
                            required: false,
                        }],
                        indexes: vec![IndexDefinition {
                            name: "by_rank".to_string(),
                            fields: vec!["rank".to_string()],
                        }],
                        access_policy: None,
                    },
                )
                .await
                .expect("landmarks schema should persist");
        }

        seed_bound_collection_group_document(
            &service,
            &tenant_id,
            direct_table.clone(),
            "aa-top",
            &["cities", "SF", "landmarks", "aa-top"],
            [("rank", json!(1))],
        );
        seed_bound_collection_group_document(
            &service,
            &tenant_id,
            direct_table,
            "bb-top",
            &["cities", "SF", "landmarks", "bb-top"],
            [("rank", json!(2))],
        );
        seed_bound_collection_group_document(
            &service,
            &tenant_id,
            nested_table,
            "zz-top",
            &["cities", "SF", "districts", "1", "landmarks", "zz-top"],
            [("rank", json!(3))],
        );
        seed_bound_collection_group_document(
            &service,
            &tenant_id,
            other_table,
            "cc-top",
            &["cities", "LA", "landmarks", "cc-top"],
            [("rank", json!(4))],
        );

        let rows = service
            .query_collection_group_documents_structured_with_principal_cancellable(
                &tenant_id,
                &CollectionName::new("landmarks").expect("collection group should parse"),
                Some(
                    &DocumentPath::from_segments(["cities", "SF"])
                        .expect("ancestor path should parse"),
                ),
                &StructuredQuery {
                    order_by: vec![StructuredOrder {
                        field: FieldReference::new("__name__"),
                        direction: QueryDirection::Ascending,
                    }],
                    start_at: Some(StructuredCursor {
                        values: vec![json!("cities/SF/landmarks/aa-top")],
                        before: true,
                    }),
                    ..StructuredQuery::default()
                },
                &PrincipalContext::anonymous(),
                &mut || Ok(()),
            )
            .expect("collection-group query should succeed on postgres providers");

        assert_eq!(
            rows.into_iter()
                .map(|(path, document)| (path.to_string(), document.get_field("rank").cloned()))
                .collect::<Vec<_>>(),
            vec![
                ("cities/SF/landmarks/aa-top".to_string(), Some(json!(1))),
                ("cities/SF/landmarks/bb-top".to_string(), Some(json!(2))),
            ],
            "postgres collection-group queries should use the persisted path bindings and full document-path cursors"
        );

        service.quiesce().await;
    })
    .await;
}

fn seed_bound_collection_group_document(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: TableName,
    document_id: &str,
    document_path: &[&str],
    fields: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
) {
    let document_id = DocumentId::from_key(document_id).expect("document id should parse");
    let document_path = DocumentPath::from_segments(document_path.iter().copied())
        .expect("document path should parse");
    let batch = AtomicWriteBatch::new(vec![AtomicWrite::Set {
        key: WriteKey::from(ResourcePathBinding::new(
            DocumentLocator::new(table, document_id),
            document_path,
        )),
        document: serde_json::Map::from_iter(
            fields
                .into_iter()
                .map(|(field, value)| (field.to_string(), value)),
        ),
        mode: WriteSetMode::Overwrite,
        precondition: WritePrecondition::default(),
        transforms: Vec::new(),
    }])
    .expect("seed write batch should build");
    service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("seed execution unit should begin")
        .execute_atomic_write_batch(batch)
        .expect("seed write batch should commit");
}
