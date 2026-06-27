use std::sync::Arc;
use std::thread;

use nimbus_kv::{NimbusKvStore, TieringConfig, TieringMode};

#[test]
fn durable_cache_round_trip_survives_restart() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.redb");

    let store = NimbusKvStore::durable_at(&path, TieringConfig::durable())
        .expect("durable store should open");
    store.set("session", "alpha", None).expect("set succeeds");
    assert_eq!(
        store.get(b"session", 0).expect("get succeeds"),
        Some(b"alpha".to_vec())
    );
    drop(store);

    let reopened = NimbusKvStore::durable_at(&path, TieringConfig::durable())
        .expect("durable store should reopen");
    assert_eq!(
        reopened.get(b"session", 0).expect("get succeeds"),
        Some(b"alpha".to_vec())
    );
}

#[test]
fn no_disk_mode_is_volatile() {
    let store = NimbusKvStore::no_disk(TieringConfig::no_disk()).expect("no-disk store opens");
    assert_eq!(store.tiering().mode, TieringMode::NoDisk);
    store.set("session", "alpha", None).expect("set succeeds");
    assert_eq!(
        store.get(b"session", 0).expect("get succeeds"),
        Some(b"alpha".to_vec())
    );
    drop(store);

    let restarted = NimbusKvStore::no_disk(TieringConfig::no_disk()).expect("no-disk store opens");
    assert_eq!(restarted.get(b"session", 0).expect("get succeeds"), None);
}

#[test]
fn no_cache_mode_reads_straight_through() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.redb");
    let store = NimbusKvStore::no_cache_at(&path).expect("no-cache store opens");
    assert_eq!(store.tiering().mode, TieringMode::NoCache);

    store.set("direct", "value", None).expect("set succeeds");
    assert_eq!(
        store.get(b"direct", 0).expect("get succeeds"),
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
    store
        .set("lease", "token", Some(1_000))
        .expect("set with expire_at succeeds");
    assert_eq!(
        store.get(b"lease", 0).expect("get succeeds"),
        Some(b"token".to_vec())
    );

    assert!(store.expire(b"lease", 2_000, 100).expect("expire succeeds"));
    assert_eq!(
        store.get(b"lease", 1_500).expect("get succeeds"),
        Some(b"token".to_vec())
    );

    assert!(
        store
            .expire(b"lease", 1_600, 1_500)
            .expect("expire succeeds")
    );
    assert_eq!(store.get(b"lease", 1_700).expect("get succeeds"), None);
}

fn assert_concurrent_incr_reaches_expected_value(store: NimbusKvStore) {
    let store = Arc::new(store);
    let mut handles = Vec::new();
    for _ in 0..8 {
        let store = Arc::clone(&store);
        handles.push(thread::spawn(move || {
            for _ in 0..50 {
                store.incr(b"counter", 0).expect("INCR should succeed");
            }
        }));
    }
    for handle in handles {
        handle.join().expect("INCR thread should join");
    }
    assert_eq!(
        store.get(b"counter", 0).expect("get succeeds"),
        Some(b"400".to_vec())
    );
}
