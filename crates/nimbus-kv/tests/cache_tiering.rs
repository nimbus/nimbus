use std::sync::Arc;
use std::thread;

use nimbus_core::TenantId;
use nimbus_kv::{NimbusKvStore, TieringConfig, TieringMode};

fn tenant(id: &str) -> TenantId {
    TenantId::new(id).expect("valid tenant id")
}

#[test]
fn durable_cache_round_trip_survives_restart() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.redb");

    let store = NimbusKvStore::durable_at(&path, TieringConfig::durable())
        .expect("durable store should open");
    let tenant = tenant("tenant-a");
    store
        .set(&tenant, "session", "alpha", None)
        .expect("set succeeds");
    assert_eq!(
        store.get(&tenant, b"session", 0).expect("get succeeds"),
        Some(b"alpha".to_vec())
    );
    drop(store);

    let reopened = NimbusKvStore::durable_at(&path, TieringConfig::durable())
        .expect("durable store should reopen");
    assert_eq!(
        reopened.get(&tenant, b"session", 0).expect("get succeeds"),
        Some(b"alpha".to_vec())
    );
}

#[test]
fn no_disk_mode_is_volatile() {
    let store = NimbusKvStore::no_disk(TieringConfig::no_disk()).expect("no-disk store opens");
    assert_eq!(store.tiering().mode, TieringMode::NoDisk);
    let tenant = tenant("tenant-a");
    store
        .set(&tenant, "session", "alpha", None)
        .expect("set succeeds");
    assert_eq!(
        store.get(&tenant, b"session", 0).expect("get succeeds"),
        Some(b"alpha".to_vec())
    );
    drop(store);

    let restarted = NimbusKvStore::no_disk(TieringConfig::no_disk()).expect("no-disk store opens");
    assert_eq!(
        restarted.get(&tenant, b"session", 0).expect("get succeeds"),
        None
    );
}

#[test]
fn no_cache_mode_reads_straight_through() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.redb");
    let store = NimbusKvStore::no_cache_at(&path).expect("no-cache store opens");
    assert_eq!(store.tiering().mode, TieringMode::NoCache);

    let tenant = tenant("tenant-a");
    store
        .set(&tenant, "direct", "value", None)
        .expect("set succeeds");
    assert_eq!(
        store.get(&tenant, b"direct", 0).expect("get succeeds"),
        Some(b"value".to_vec())
    );
}

#[test]
fn concurrent_incr_cache_coherency_disk_backed_and_no_disk() {
    // concurrent INCR coherency must hold for disk-backed and no-disk modes.
    let dir = tempfile::tempdir().expect("tempdir should create");
    let durable =
        NimbusKvStore::durable_at(dir.path().join("tenant.redb"), TieringConfig::durable())
            .expect("durable store opens");
    assert_concurrent_incr_reaches_expected_value(durable);

    let no_disk = NimbusKvStore::no_disk(TieringConfig::no_disk()).expect("no-disk store opens");
    assert_concurrent_incr_reaches_expected_value(no_disk);
}

#[test]
fn cache_expire_at_coherency_prevents_logically_expired_cache_hit() {
    let store = NimbusKvStore::no_disk(TieringConfig::no_disk()).expect("no-disk store opens");
    let tenant = tenant("tenant-a");
    store
        .set(&tenant, "lease", "token", Some(1_000))
        .expect("set with expire_at succeeds");
    assert_eq!(
        store.get(&tenant, b"lease", 0).expect("get succeeds"),
        Some(b"token".to_vec())
    );

    assert!(
        store
            .expire(&tenant, b"lease", 2_000, 100)
            .expect("expire succeeds")
    );
    assert_eq!(
        store.get(&tenant, b"lease", 1_500).expect("get succeeds"),
        Some(b"token".to_vec())
    );

    assert!(
        store
            .expire(&tenant, b"lease", 1_600, 1_500)
            .expect("expire succeeds")
    );
    assert_eq!(
        store.get(&tenant, b"lease", 1_700).expect("get succeeds"),
        None
    );
}

#[test]
fn cache_keys_include_tenant_identity() {
    let store = NimbusKvStore::no_disk(TieringConfig::no_disk()).expect("no-disk store opens");
    let tenant_a = tenant("tenant-a");
    let tenant_b = tenant("tenant-b");

    store
        .set(&tenant_a, "shared", "alpha", None)
        .expect("tenant A set succeeds");
    store
        .set(&tenant_b, "shared", "bravo", None)
        .expect("tenant B set succeeds");

    assert_eq!(
        store.get(&tenant_a, b"shared", 0).expect("get succeeds"),
        Some(b"alpha".to_vec())
    );
    assert_eq!(
        store.get(&tenant_b, b"shared", 0).expect("get succeeds"),
        Some(b"bravo".to_vec())
    );
}

fn assert_concurrent_incr_reaches_expected_value(store: NimbusKvStore) {
    let store = Arc::new(store);
    let tenant = tenant("tenant-a");
    let mut handles = Vec::new();
    for _ in 0..8 {
        let store = Arc::clone(&store);
        let tenant = tenant.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..50 {
                store
                    .incr(&tenant, b"counter", 0)
                    .expect("INCR should succeed");
            }
        }));
    }
    for handle in handles {
        handle.join().expect("INCR thread should join");
    }
    assert_eq!(
        store.get(&tenant, b"counter", 0).expect("get succeeds"),
        Some(b"400".to_vec())
    );
}
