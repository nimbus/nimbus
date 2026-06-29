use super::*;
use crate::{
    LibsqlReplicaTenantStore, MySqlTenantStore, OBJECT_MANIFEST_TABLE, ObjectManifest,
    ObjectMetaStore, PostgresTenantStore,
};

fn manifest(key: &str, blob_hash: &str) -> ObjectManifest {
    let mut metadata = serde_json::Map::new();
    metadata.insert("owner".to_string(), json!("storage-tests"));
    ObjectManifest::whole(
        key,
        12,
        blob_hash,
        Some("text/plain".to_string()),
        metadata,
        "\"etag\"",
    )
    .expect("manifest should be valid")
}

fn assert_object_meta_store_impl<T: ObjectMetaStore>() {}

#[test]
fn object_meta_store_trait_covers_all_tenant_stores() {
    assert_object_meta_store_impl::<TenantStore>();
    assert_object_meta_store_impl::<SqliteTenantStore>();
    assert_object_meta_store_impl::<PostgresTenantStore>();
    assert_object_meta_store_impl::<MySqlTenantStore>();
    assert_object_meta_store_impl::<LibsqlReplicaTenantStore>();
}

#[test]
fn object_meta_store_round_trips_manifest_through_redb() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = manifest("photos/2026/launch.txt", "hash-a");

    let commit = store
        .put_object_manifest(&first)
        .expect("manifest put should commit");
    let fetched = store
        .get_object_manifest(&first.key)
        .expect("manifest get should succeed")
        .expect("manifest should exist");

    assert_eq!(commit.sequence, SequenceNumber(1));
    assert_eq!(commit.writes.len(), 1);
    assert_eq!(commit.writes[0].table.as_str(), OBJECT_MANIFEST_TABLE);
    assert_eq!(fetched, first);
}

#[test]
fn object_meta_store_updates_existing_manifest_atomically_through_redb() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = manifest("objects/report.pdf", "hash-a");
    let mut second = manifest("objects/report.pdf", "hash-b");
    second.size = 99;
    second.etag = "\"etag-2\"".to_string();

    store
        .put_object_manifest(&first)
        .expect("initial manifest put should commit");
    let commit = store
        .put_object_manifest(&second)
        .expect("manifest update should commit");
    let fetched = store
        .get_object_manifest(&second.key)
        .expect("manifest get should succeed")
        .expect("manifest should exist");

    assert_eq!(commit.sequence, SequenceNumber(2));
    assert_eq!(commit.writes.len(), 1);
    assert_eq!(fetched, second);
}

#[test]
fn object_meta_store_lists_by_prefix_and_deletes_through_redb() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let keep = manifest("alpha/keep.txt", "hash-a");
    let drop = manifest("alpha/drop.txt", "hash-b");
    let other = manifest("beta/other.txt", "hash-c");

    store.put_object_manifest(&keep).unwrap();
    store.put_object_manifest(&drop).unwrap();
    store.put_object_manifest(&other).unwrap();

    let listed = store
        .list_object_manifests("alpha/", 10)
        .expect("manifest list should succeed");
    assert_eq!(
        listed
            .iter()
            .map(|manifest| manifest.key.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha/drop.txt", "alpha/keep.txt"]
    );

    let (commit, deleted) = store
        .delete_object_manifest(&drop.key)
        .expect("manifest delete should succeed")
        .expect("manifest should exist");
    assert_eq!(commit.sequence, SequenceNumber(4));
    assert_eq!(deleted, drop);
    assert!(
        store
            .get_object_manifest("alpha/drop.txt")
            .expect("manifest get should succeed")
            .is_none()
    );
}

#[test]
fn object_meta_store_persists_through_sqlite() {
    let dir = tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.sqlite3");
    let manifest = manifest("durable/object.txt", "hash-sqlite");

    {
        let store = SqliteTenantStore::open(&path).expect("sqlite store should open");
        store
            .put_object_manifest(&manifest)
            .expect("manifest put should commit");
    }

    let reopened = SqliteTenantStore::open(&path).expect("sqlite store should reopen");
    let fetched = reopened
        .get_object_manifest(&manifest.key)
        .expect("manifest get should succeed")
        .expect("manifest should exist");
    assert_eq!(fetched, manifest);
}

#[test]
fn object_meta_store_rejects_invalid_keys_before_document_write() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let invalid = ObjectManifest::whole("", 1, "hash", None, serde_json::Map::new(), "\"etag\"");

    assert!(matches!(invalid, Err(Error::InvalidInput(_))));
    assert_eq!(
        store
            .list_object_manifests("", 1)
            .expect("empty prefix list should succeed")
            .len(),
        0
    );
}
