use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use nimbus_blob::{BlobStore, ErasureBlobStore, ErasureConfig, LocalPackStore};
use nimbus_core::TenantId;
use nimbus_engine::Engine;
use nimbus_storage::{
    ObjectChunkRef, ObjectManifest, ObjectManifestAttributes, ObjectPlacement,
    ObjectStorePlacementTarget, ObjectStoreProviderCredentials, ObjectStoreProviderKind,
    PlacementPolicy, PointInTimeRestoreArchive,
};
use tempfile::tempdir;

use crate::config::{
    BUCKET_ENV, ERASURE_DATA_ENV, ERASURE_DRIVES_ENV, ERASURE_PARITY_ENV, ERASURE_STRIPE_ENV,
    LOCAL_LEG_ENV, MASTER_KEY_FILE_ENV, MODE_ENV, PROVIDER_ENV,
};
use crate::{
    ErasureLegConfig, LocalLeg, ObjectStorageConfig, ObjectStorageEnv, ObjectStorageResolver,
    object_backup_roots,
};

struct MapEnv(BTreeMap<String, String>);

impl ObjectStorageEnv for MapEnv {
    fn get(&self, key: &str) -> nimbus_core::Result<Option<String>> {
        Ok(self.0.get(key).cloned())
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
fn erasure_env_config_round_trips_and_rejects_bad_stripe() {
    let temp = tempdir().unwrap();
    let drives = (0..3)
        .map(|index| temp.path().join(format!("drive-{index}")))
        .collect::<Vec<_>>();
    let drive_list = drives
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(",");
    let base = BTreeMap::from([
        (LOCAL_LEG_ENV.to_string(), "erasure".to_string()),
        (ERASURE_DRIVES_ENV.to_string(), drive_list),
        (ERASURE_DATA_ENV.to_string(), "2".to_string()),
        (ERASURE_PARITY_ENV.to_string(), "1".to_string()),
        (ERASURE_STRIPE_ENV.to_string(), "64".to_string()),
    ]);

    let config = ObjectStorageConfig::from_sources(None, &MapEnv(base.clone())).unwrap();
    let LocalLeg::Erasure(erasure) = config.local_leg() else {
        panic!("erasure env must select the erasure local leg");
    };
    assert_eq!(erasure.drives, drives);
    assert_eq!(erasure.data_shards, 2);
    assert_eq!(erasure.parity_shards, 1);
    assert_eq!(erasure.stripe_width, 64);

    let mut bad = base;
    bad.insert(ERASURE_STRIPE_ENV.to_string(), "65".to_string());
    let error = ObjectStorageConfig::from_sources(None, &MapEnv(bad)).unwrap_err();
    assert!(error.to_string().contains("stripe width"), "{error}");
    assert!(error.to_string().contains("multiple"), "{error}");
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

    // Read-only inspection coexists with the resolver's live (locked) store.
    let raw_local = LocalPackStore::open_read_only(resolver.object_blob_root(&tenant)).unwrap();
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

fn erasure_config(drives: Vec<std::path::PathBuf>) -> ObjectStorageConfig {
    ObjectStorageConfig::local_only().with_local_leg(LocalLeg::Erasure(ErasureLegConfig {
        drives,
        data_shards: 2,
        parity_shards: 1,
        stripe_width: 64,
    }))
}

#[tokio::test]
async fn resolver_builds_erasure_local_leg_per_tenant() {
    let temp = tempdir().unwrap();
    let drives = (0..3)
        .map(|index| temp.path().join(format!("erasure-drive-{index}")))
        .collect::<Vec<_>>();
    let engine = Arc::new(Engine::new(temp.path().join("engine")).unwrap());
    let resolver = ObjectStorageResolver::with_config(engine, erasure_config(drives.clone()));
    let tenant_a = TenantId::new("tenant-a").unwrap();
    let tenant_b = TenantId::new("tenant-b").unwrap();

    let store_a = resolver.blob_store(&tenant_a).unwrap();
    store_a
        .put(Bytes::from_static(b"tenant a bytes"))
        .await
        .unwrap();
    let store_b = resolver.blob_store(&tenant_b).unwrap();
    store_b
        .put(Bytes::from_static(b"tenant b bytes"))
        .await
        .unwrap();

    let roots_a = drives
        .iter()
        .map(|drive| drive.join("tenant-a"))
        .collect::<Vec<_>>();
    let roots_b = drives
        .iter()
        .map(|drive| drive.join("tenant-b"))
        .collect::<Vec<_>>();
    assert!(roots_a.iter().all(|root| root.exists()));
    assert!(roots_b.iter().all(|root| root.exists()));
    assert!(roots_a.iter().zip(&roots_b).all(|(a, b)| a != b));

    drop(store_a);
    drop(store_b);
    drop(resolver);
    let error = ErasureBlobStore::open(ErasureConfig::new("tenant-b", roots_a, 2, 1, 64).unwrap())
        .unwrap_err();
    assert_eq!(
        error.storage_kind(),
        Some(nimbus_core::StorageErrorKind::Corruption),
        "per-tenant erasure leg identity must reject a foreign tenant"
    );
}

#[tokio::test]
async fn resolver_erasure_leg_is_encrypted_below_placement() {
    const MARKER: &[u8] = b"nimbus-erasure-encryption-plaintext-marker-7d5f8b3c";

    let temp = tempdir().unwrap();
    let drives = (0..3)
        .map(|index| temp.path().join(format!("erasure-drive-{index}")))
        .collect::<Vec<_>>();
    let engine = Arc::new(Engine::new(temp.path().join("engine")).unwrap());
    let resolver = ObjectStorageResolver::with_config(engine, erasure_config(drives.clone()));
    let tenant = tenant();
    let store = resolver.blob_store(&tenant).unwrap();
    let plaintext = Bytes::from_static(MARKER);
    let hash = store.put(plaintext.clone()).await.unwrap();

    assert_eq!(store.get(&hash).await.unwrap(), plaintext);
    for root in drives.iter().map(|drive| drive.join("tenant-a")) {
        assert!(
            !tree_contains_bytes(&root, MARKER),
            "raw erasure drive {} must not contain the plaintext marker",
            root.display()
        );
    }
}

fn tree_contains_bytes(root: &Path, needle: &[u8]) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if tree_contains_bytes(&path, needle) {
                return true;
            }
        } else if let Ok(bytes) = std::fs::read(path)
            && bytes.windows(needle.len()).any(|window| window == needle)
        {
            return true;
        }
    }
    false
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
    let raw_local = LocalPackStore::open_read_only(resolver.object_blob_root(&tenant)).unwrap();
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

    let root = resolver.object_blob_root(&tenant);
    let store = resolver.blob_store(&tenant).unwrap();
    let hash = store
        .put(Bytes::from_static(b"tiered encrypted bytes"))
        .await
        .unwrap();
    // Release the root lock before mutating the local leg out-of-band.
    drop(store);
    drop(resolver);

    // A writable maintenance handle may open a bound root without declaring
    // an identity (identity-agnostic tools like backup/GC enumerate roots).
    let raw_local = LocalPackStore::open(&root).unwrap();
    raw_local.release(&hash).await.unwrap();
    assert!(
        !raw_local.has(&hash).await.unwrap(),
        "test starts with a cold-tier-only copy"
    );
    drop(raw_local);

    let resolver = ObjectStorageResolver::with_config(
        engine,
        ObjectStorageConfig::new(PlacementPolicy::Tier { target }),
    );
    let store = resolver.blob_store(&tenant).unwrap();
    assert_eq!(
        store.get(&hash).await.unwrap(),
        Bytes::from_static(b"tiered encrypted bytes")
    );
    // Read-only inspection coexists with the live resolver store.
    let raw_local = LocalPackStore::open_read_only(&root).unwrap();
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

#[tokio::test]
async fn resolver_live_root_is_shared_with_same_process_side_open() {
    let temp = tempdir().unwrap();
    let engine = Arc::new(Engine::new(temp.path()).unwrap());
    let resolver = ObjectStorageResolver::new(engine);
    let tenant = tenant();
    let store = resolver.blob_store(&tenant).unwrap();
    let hash = store.put(Bytes::from_static(b"live bytes")).await.unwrap();

    // A same-process writable side-open (e.g. an in-process backup task, or a
    // second resolver over the same engine) aliases the SAME live pack state:
    // no Busy, immediate visibility. Cross-process exclusion stays on the
    // flock (`root_lock_excludes_second_process` in nimbus-blob).
    let raw_local = LocalPackStore::open(resolver.object_blob_root(&tenant)).unwrap();
    assert!(
        raw_local.has(&hash).await.unwrap(),
        "side-open shares the live root state"
    );
}

#[tokio::test]
async fn tenant_root_identity_refuses_foreign_tenant() {
    let temp = tempdir().unwrap();
    let engine = Arc::new(Engine::new(temp.path()).unwrap());
    let resolver = ObjectStorageResolver::new(engine);
    let tenant = tenant();
    let root = resolver.object_blob_root(&tenant);
    let _hash = resolver
        .blob_store(&tenant)
        .unwrap()
        .put(Bytes::from_static(b"bound"))
        .await
        .unwrap();
    drop(resolver);

    // Opening tenant-a's root while claiming to be tenant-b fails closed.
    let err = LocalPackStore::open_with_options(
        &root,
        nimbus_blob::LocalPackStoreOptions {
            identity: Some(crate::tenant_root_identity(
                &TenantId::new("tenant-b").unwrap(),
            )),
            ..nimbus_blob::LocalPackStoreOptions::default()
        },
    )
    .unwrap_err();
    assert_eq!(
        err.storage_kind(),
        Some(nimbus_core::StorageErrorKind::Corruption),
        "a root bound to tenant-a refuses to open as tenant-b"
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
        version: nimbus_storage::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
        applied_sequence: nimbus_core::SequenceNumber(0),
        durable_head: nimbus_core::SequenceNumber(0),
        table_identities: Vec::new(),
        schema: nimbus_core::Schema::default(),
        documents: Vec::new(),
        scheduled_execution_ids: Vec::new(),
    };
    let document = manifest.to_document().unwrap();
    snapshot
        .table_identities
        .push(nimbus_storage::TableIdentitySnapshotEntry {
            namespace: "default".to_string(),
            table: document.table.clone(),
            table_id: nimbus_core::TableId::new(),
            state: nimbus_core::TableState::Active,
        });
    snapshot.documents.push(document);
    let archive = PointInTimeRestoreArchive {
        version: 1,
        target_sequence: nimbus_core::SequenceNumber(0),
        target_timestamp: nimbus_core::Timestamp(0),
        base_snapshot: snapshot.clone(),
        journal_tail: Vec::new(),
        storage_format_version: nimbus_storage::CURRENT_STORAGE_FORMAT_VERSION,
        document_version_storage_format: nimbus_storage::CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT,
        index_version_storage_format: nimbus_storage::CURRENT_INDEX_VERSION_STORAGE_FORMAT,
        target_position: snapshot
            .materialized_position()
            .expect("archive base position should compute"),
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

struct BrokenEnv;

impl ObjectStorageEnv for BrokenEnv {
    fn get(&self, key: &str) -> nimbus_core::Result<Option<String>> {
        if key == "NIMBUS_OBJECT_STORAGE_LOCAL_LEG" {
            Err(nimbus_core::Error::InvalidInput(format!(
                "environment variable {key} is set but not valid UTF-8"
            )))
        } else {
            Ok(None)
        }
    }
}

#[test]
fn invalid_env_value_fails_closed_instead_of_defaulting_to_pack() {
    // Review fix (EOW round 5, P2): a set-but-invalid (non-UTF-8)
    // NIMBUS_OBJECT_STORAGE_LOCAL_LEG must fail configuration, not
    // silently start against the pack root.
    let err = ObjectStorageConfig::from_sources(None, &BrokenEnv)
        .expect_err("invalid env value must fail closed");
    assert!(err.to_string().contains("not valid UTF-8"));
}
