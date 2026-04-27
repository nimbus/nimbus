use neovex_core::{
    CollectionName, DocumentLocator, DocumentPath, ResourcePathBinding, TableName,
    TriggerDeliveryCursor,
};

use super::support::*;

fn binding(table: &str, id: &str, path: &[&str]) -> ResourcePathBinding {
    ResourcePathBinding::new(
        DocumentLocator::new(
            TableName::new(table).expect("table name should parse"),
            neovex_core::DocumentId::from_key(id).expect("document id should parse"),
        ),
        DocumentPath::from_segments(path).expect("document path should parse"),
    )
}

#[test]
fn sqlite_resource_path_bindings_round_trip_without_table_name_delimiter_tricks() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let bindings = vec![
        binding("reserved_store", "loc_reserved", &["__meta__", "doc-1"]),
        binding("dotted_store", "loc_dotted", &["cities.v2", "SF"]),
        binding("unicode_store", "loc_unicode", &["日本語", "東京"]),
        binding("deep_store", "loc_deep", &["a", "1", "b", "2", "c", "3"]),
    ];

    for binding in &bindings {
        store
            .upsert_resource_path_binding(binding)
            .expect("binding should persist");
    }

    for binding in &bindings {
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

    let found = store
        .scan_collection_group_bindings(
            &CollectionName::new("c").expect("collection group should parse"),
        )
        .expect("collection-group scan should succeed");
    assert_eq!(found, vec![bindings[3].clone()]);
}

#[test]
fn sqlite_execution_unit_batch_persists_and_removes_resource_path_bindings_atomically() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let table = TableName::new("landmarks_store").expect("table name should parse");
    let document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("rank".to_string(), json!(1))]),
    );
    let binding = ResourcePathBinding::new(
        DocumentLocator::new(table.clone(), document.id.clone()),
        DocumentPath::from_segments(["cities", "SF", "landmarks", "golden-gate"])
            .expect("document path should parse"),
    );

    let commit = store
        .apply_execution_unit_batch(
            &[crate::ResolvedWrite::Insert {
                document: document.clone(),
                indexes: Vec::new(),
                resource_path_binding: Some(binding.clone()),
            }],
            &[],
        )
        .expect("insert batch should succeed")
        .expect("insert batch should emit a commit");
    assert_eq!(commit.sequence, SequenceNumber(1));
    assert_eq!(
        store
            .locator_for_document_path(&binding.document_path)
            .expect("path lookup should succeed"),
        Some(binding.locator.clone())
    );

    let delete_commit = store
        .apply_execution_unit_batch(
            &[crate::ResolvedWrite::Delete {
                previous: document.clone(),
                indexes: Vec::new(),
            }],
            &[],
        )
        .expect("delete batch should succeed")
        .expect("delete batch should emit a commit");
    assert_eq!(delete_commit.sequence, SequenceNumber(2));
    assert!(
        store
            .resource_path_binding(&binding.locator)
            .expect("binding lookup should succeed")
            .is_none(),
        "delete batch should remove the sidecar binding in the same transaction"
    );
    assert!(
        store
            .scan_collection_group_bindings(
                &CollectionName::new("landmarks").expect("collection group should parse"),
            )
            .expect("collection-group scan should succeed")
            .is_empty(),
        "delete batch should remove collection-group metadata too"
    );
}

#[test]
fn sqlite_trigger_delivery_cursor_round_trips_without_extra_sidecars() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");

    assert_eq!(
        store.trigger_delivery_cursor().expect("cursor should load"),
        TriggerDeliveryCursor::default()
    );

    store
        .set_trigger_delivery_cursor(TriggerDeliveryCursor::new(SequenceNumber(11)))
        .expect("cursor should persist");

    assert_eq!(
        store
            .trigger_delivery_cursor()
            .expect("cursor should round trip"),
        TriggerDeliveryCursor::new(SequenceNumber(11))
    );
}
