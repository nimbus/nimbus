use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use nimbus_blob::{BlobStore, LocalPackStore};
use nimbus_core::TenantId;
use nimbus_engine::Engine;
use nimbus_storage::{
    ObjectChunkRef, ObjectManifest, ObjectManifestAttributes, ObjectPlacement,
    ObjectStorePlacementTarget, ObjectStoreProviderCredentials, ObjectStoreProviderKind,
    PlacementPolicy, PointInTimeRestoreArchive,
};
use tempfile::tempdir;

use crate::config::{BUCKET_ENV, MASTER_KEY_FILE_ENV, MODE_ENV, PROVIDER_ENV};
use crate::{ObjectStorageConfig, ObjectStorageEnv, ObjectStorageResolver, object_backup_roots};

struct MapEnv(BTreeMap<String, String>);

impl ObjectStorageEnv for MapEnv {
    fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
}

fn tenant() -> TenantId {
    TenantId::new("tenant-a").expect("tenant should parse")
}

fn memory_target() -> ObjectStorePlacementTarget {
    ObjectStorePlacementTarget::new(
        ObjectStoreProviderKind::Memory,
        "memory-bucket",
        ObjectStoreProviderCredentials::Anonymous,
    )
    .expect("target should build")
}

fn local_target(root: &Path) -> ObjectStorePlacementTarget {
    ObjectStorePlacementTarget::new(
        ObjectStoreProviderKind::Local,
        "local-bucket",
        ObjectStoreProviderCredentials::Anonymous,
    )
    .expect("target should build")
    .with_endpoint(root.display().to_string())
}

#[test]
fn env_default_is_overridden_by_programmatic_config() {
    let env = MapEnv(BTreeMap::from([
        (MODE_ENV.to_string(), "mirror".to_string()),
        (PROVIDER_ENV.to_string(), "memory".to_string()),
        (BUCKET_ENV.to_string(), "env-bucket".to_string()),
        (
            MASTER_KEY_FILE_ENV.to_string(),
            "/secure/object.master".to_string(),
        ),
    ]));

    let from_env = ObjectStorageConfig::from_sources(None, &env).unwrap();
    assert!(matches!(
        from_env.default_policy(),
        PlacementPolicy::Mirror { .. }
    ));
    assert_eq!(
        from_env.master_key_file(),
        Some(Path::new("/secure/object.master"))
    );

    let programmatic =
        ObjectStorageConfig::from_sources(Some(PlacementPolicy::LocalOnly), &env).unwrap();
    assert_eq!(programmatic.default_policy(), &PlacementPolicy::LocalOnly);
    assert_eq!(
        programmatic.master_key_file(),
        Some(Path::new("/secure/object.master"))
    );
}

#[test]
fn tenant_placement_override_wins_over_server_default() {
    let temp = tempdir().unwrap();
    let engine = Arc::new(Engine::new(temp.path()).unwrap());
    let tenant = tenant();
    let resolver = ObjectStorageResolver::with_config(
        engine.clone(),
        ObjectStorageConfig::new(PlacementPolicy::Mirror {
            target: memory_target(),
            require_ack: true,
        }),
    );

    assert!(matches!(
        resolver.effective_policy(&tenant).unwrap(),
        PlacementPolicy::Mirror { .. }
    ));

    engine
        .set_object_placement(ObjectPlacement::new(
            tenant.clone(),
            PlacementPolicy::LocalOnly,
            42,
        ))
        .unwrap();

    assert_eq!(
        resolver.effective_policy(&tenant).unwrap(),
        PlacementPolicy::LocalOnly
    );
}

#[tokio::test]
async fn resolver_builds_local_blob_store() {
    let temp = tempdir().unwrap();
    let engine = Arc::new(Engine::new(temp.path()).unwrap());
    let resolver = ObjectStorageResolver::new(engine);
    let tenant = tenant();

    let store = resolver.blob_store(&tenant).unwrap();
    let hash = store
        .put(Bytes::from_static(b"native bytes"))
        .await
        .unwrap();

    assert_eq!(
        store.get(&hash).await.unwrap(),
        Bytes::from_static(b"native bytes")
    );

    let raw_local = LocalPackStore::open(resolver.object_blob_root(&tenant)).unwrap();
    assert!(
        raw_local.has(&hash).await.unwrap(),
        "the raw pack stores the encrypted blob address returned by the resolver"
    );
    assert_ne!(
        raw_local.get(&hash).await.unwrap(),
        Bytes::from_static(b"native bytes"),
        "resolver-composed local packs store framed ciphertext, not plaintext"
    );
    assert!(
        resolver.object_master_key_path().exists(),
        "resolver auto-creates the object-storage master key"
    );
    assert!(
        nimbus_crypto::KeyManifest::manifest_path(&resolver.object_blob_key_path(&tenant)).exists(),
        "resolver creates the tenant blob DEK sidecar"
    );
}

#[tokio::test]
async fn encrypted_blob_stores_are_tenant_scoped() {
    let temp = tempdir().unwrap();
    let engine = Arc::new(Engine::new(temp.path()).unwrap());
    let resolver = ObjectStorageResolver::new(engine);
    let tenant_a = TenantId::new("tenant-a").unwrap();
    let tenant_b = TenantId::new("tenant-b").unwrap();

    let hash_a = resolver
        .blob_store(&tenant_a)
        .unwrap()
        .put(Bytes::from_static(b"same bytes"))
        .await
        .unwrap();
    let hash_b = resolver
        .blob_store(&tenant_b)
        .unwrap()
        .put(Bytes::from_static(b"same bytes"))
        .await
        .unwrap();

    assert_ne!(
        hash_a, hash_b,
        "different tenant blob DEKs must produce different ciphertext addresses"
    );
}

#[tokio::test]
async fn resolver_mirror_policy_round_trips_with_encrypted_legs() {
    let temp = tempdir().unwrap();
    let engine = Arc::new(Engine::new(temp.path()).unwrap());
    let resolver = ObjectStorageResolver::with_config(
        engine,
        ObjectStorageConfig::new(PlacementPolicy::Mirror {
            target: memory_target(),
            require_ack: true,
        }),
    );
    let tenant = tenant();

    let store = resolver.blob_store(&tenant).unwrap();
    let hash = store
        .put(Bytes::from_static(b"mirrored encrypted bytes"))
        .await
        .unwrap();

    assert_eq!(
        store.get(&hash).await.unwrap(),
        Bytes::from_static(b"mirrored encrypted bytes")
    );
    let raw_local = LocalPackStore::open(resolver.object_blob_root(&tenant)).unwrap();
    assert_ne!(
        raw_local.get(&hash).await.unwrap(),
        Bytes::from_static(b"mirrored encrypted bytes"),
        "mirror composition stores ciphertext in the local leg"
    );
}

#[tokio::test]
async fn resolver_tier_policy_rehydrates_encrypted_local_cache() {
    let temp = tempdir().unwrap();
    let engine = Arc::new(Engine::new(temp.path()).unwrap());
    let target = local_target(&temp.path().join("cold-tier"));
    let resolver = ObjectStorageResolver::with_config(
        engine.clone(),
        ObjectStorageConfig::new(PlacementPolicy::Tier {
            target: target.clone(),
        }),
    );
    let tenant = tenant();

    let store = resolver.blob_store(&tenant).unwrap();
    let hash = store
        .put(Bytes::from_static(b"tiered encrypted bytes"))
        .await
        .unwrap();

    let raw_local = LocalPackStore::open(resolver.object_blob_root(&tenant)).unwrap();
    raw_local.release(&hash).await.unwrap();
    assert!(
        !raw_local.has(&hash).await.unwrap(),
        "test starts with a cold-tier-only copy"
    );

    let resolver = ObjectStorageResolver::with_config(
        engine,
        ObjectStorageConfig::new(PlacementPolicy::Tier { target }),
    );
    let store = resolver.blob_store(&tenant).unwrap();
    assert_eq!(
        store.get(&hash).await.unwrap(),
        Bytes::from_static(b"tiered encrypted bytes")
    );
    let raw_local = LocalPackStore::open(resolver.object_blob_root(&tenant)).unwrap();
    assert!(
        raw_local.has(&hash).await.unwrap(),
        "tier read rehydrates the encrypted local cache"
    );
    assert_ne!(
        raw_local.get(&hash).await.unwrap(),
        Bytes::from_static(b"tiered encrypted bytes"),
        "rehydrated local cache stores ciphertext"
    );
}

#[test]
#[cfg(unix)]
fn resolver_auto_master_key_uses_private_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    let engine = Arc::new(Engine::new(temp.path()).unwrap());
    let resolver = ObjectStorageResolver::new(engine);
    let _store = resolver.blob_store(&tenant()).unwrap();

    let metadata = std::fs::metadata(resolver.object_master_key_path()).unwrap();
    assert_eq!(metadata.len(), 32);
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
}

#[test]
fn missing_secret_ref_credentials_fail_closed() {
    let temp = tempdir().unwrap();
    let engine = Arc::new(Engine::new(temp.path()).unwrap());
    let target = ObjectStorePlacementTarget::new(
        ObjectStoreProviderKind::S3,
        "bucket",
        ObjectStoreProviderCredentials::SecretRef {
            id: "secret/s3".to_string(),
        },
    )
    .unwrap();
    let resolver = ObjectStorageResolver::with_config(
        engine,
        ObjectStorageConfig::new(PlacementPolicy::CloudPrimary { target }),
    );

    let err = match resolver.blob_store(&tenant()) {
        Ok(_) => panic!("secret-ref placement should fail without a resolver"),
        Err(error) => error,
    };
    assert!(
        err.to_string()
            .contains("no credential resolver configured"),
        "{err}"
    );
}

#[test]
fn backup_roots_are_extracted_from_object_manifest_snapshot() {
    let first = nimbus_blob::BlobHash::of(b"first");
    let second = nimbus_blob::BlobHash::of(b"second");
    let third = nimbus_blob::BlobHash::of(b"third");
    let attrs = ObjectManifestAttributes::new("\"etag\"", 1);
    let manifest = ObjectManifest::chunked(
        "bucket",
        "key",
        11,
        vec![
            ObjectChunkRef {
                blob_hash: first.to_hex(),
                offset: 0,
                len: 5,
            },
            ObjectChunkRef {
                blob_hash: second.to_hex(),
                offset: 5,
                len: 6,
            },
            ObjectChunkRef {
                blob_hash: third.to_hex(),
                offset: 11,
                len: 0,
            },
        ],
        attrs,
    );
    assert!(
        manifest.is_err(),
        "manifest validation rejects zero-length chunks"
    );

    let attrs = ObjectManifestAttributes::new("\"etag\"", 1);
    let manifest = ObjectManifest::chunked(
        "bucket",
        "key",
        11,
        vec![
            ObjectChunkRef {
                blob_hash: first.to_hex(),
                offset: 0,
                len: 5,
            },
            ObjectChunkRef {
                blob_hash: second.to_hex(),
                offset: 5,
                len: 6,
            },
        ],
        attrs,
    )
    .unwrap();
    let mut snapshot = nimbus_storage::MaterializedJournalSnapshot {
        version: 0,
        applied_sequence: nimbus_core::SequenceNumber(0),
        durable_head: nimbus_core::SequenceNumber(0),
        table_identities: Vec::new(),
        schema: nimbus_core::Schema::default(),
        documents: Vec::new(),
        scheduled_execution_ids: Vec::new(),
    };
    snapshot.documents.push(manifest.to_document().unwrap());
    let archive = PointInTimeRestoreArchive {
        version: 1,
        target_sequence: nimbus_core::SequenceNumber(0),
        target_timestamp: nimbus_core::Timestamp(0),
        base_snapshot: snapshot,
        journal_tail: Vec::new(),
        storage_format_version: nimbus_storage::CURRENT_STORAGE_FORMAT_VERSION,
        document_version_storage_format: nimbus_storage::CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT,
        index_version_storage_format: nimbus_storage::CURRENT_INDEX_VERSION_STORAGE_FORMAT,
        target_fingerprint: String::new(),
    };

    assert_eq!(object_backup_roots(&archive).unwrap(), vec![first, second]);
}

#[test]
fn public_root_paths_are_stable() {
    let tenant = tenant();
    let temp = tempdir().unwrap();
    let root = crate::object_blob_root(temp.path(), &tenant);
    let master_key = crate::object_master_key_path(temp.path());
    let blob_key = crate::object_blob_key_path(temp.path(), &tenant);

    assert_eq!(root, temp.path().join("object-blobs").join("tenant-a"));
    assert_eq!(
        master_key,
        temp.path().join("keys").join("object-storage.master.key")
    );
    assert_eq!(blob_key, root.join("blob-key"));
}

#[test]
fn resolver_cache_is_tenant_scoped() {
    let temp = tempdir().unwrap();
    let engine = Arc::new(Engine::new(temp.path()).unwrap());
    let resolver = ObjectStorageResolver::new(engine);

    let mut roots = HashMap::new();
    for tenant in [
        TenantId::new("tenant-a").unwrap(),
        TenantId::new("tenant-b").unwrap(),
    ] {
        roots.insert(tenant.clone(), resolver.object_blob_root(&tenant));
        let _store = resolver.blob_store(&tenant).unwrap();
    }

    assert_ne!(
        roots.get(&TenantId::new("tenant-a").unwrap()),
        roots.get(&TenantId::new("tenant-b").unwrap())
    );
}
