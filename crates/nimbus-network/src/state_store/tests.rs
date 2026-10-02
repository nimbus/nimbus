use std::convert::Infallible;
use std::sync::mpsc;
use std::sync::{Arc, Barrier, Mutex};

use serde::{Deserialize, Serialize};
use tempfile::tempdir;

use super::test_support::{NetworkStateDurabilityEvent, transaction_with_durability_observer};
use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct FixtureState {
    owner: Option<String>,
    cleanup_pending: BTreeMap<String, String>,
}

#[derive(Default, Deserialize)]
struct RefusesSerialization;

impl Serialize for RefusesSerialization {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Err(serde::ser::Error::custom(
            "intentional payload serialization failure",
        ))
    }
}

fn fixture_partition() -> NetworkStatePartition {
    NetworkStatePartition::TenantIpam(
        TenantId::new("tenant-a").expect("fixture tenant should parse"),
    )
}

#[test]
fn transaction_round_trip_and_restart_share_one_authority() {
    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    store
        .transaction(&fixture_partition(), |state: &mut FixtureState| {
            state.owner = Some("attachment-a".to_owned());
            Ok::<_, Infallible>(())
        })
        .expect("transaction should commit");

    let restarted =
        LocalNetworkStateStore::open(root.path()).expect("store should restart cleanly");
    let state: FixtureState = restarted
        .read(&fixture_partition())
        .expect("partition should read")
        .expect("partition should exist");
    assert_eq!(state.owner.as_deref(), Some("attachment-a"));
    assert_eq!(
        store.authority_path(),
        restarted.authority_path(),
        "all handles must resolve one authority file"
    );
}

#[test]
fn separately_opened_store_handles_serialize_same_process_transactions() {
    let directory = tempdir().expect("state root");
    let root = Arc::new(directory.path().to_path_buf());
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|worker| {
            let root = Arc::clone(&root);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let store = LocalNetworkStateStore::open(root.as_ref()).expect("store should open");
                store
                    .transaction(&fixture_partition(), |state: &mut FixtureState| {
                        state
                            .cleanup_pending
                            .insert(format!("worker-{worker}"), "committed".to_owned());
                        Ok::<_, Infallible>(())
                    })
                    .expect("transaction should commit under the shared lock");
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("worker should not panic");
    }

    let store = LocalNetworkStateStore::open(root.as_ref()).expect("store should reopen");
    let state: FixtureState = store
        .read(&fixture_partition())
        .expect("partition should read")
        .expect("partition should exist");
    assert_eq!(
        state.cleanup_pending.len(),
        8,
        "each separately opened handle must publish exactly one serialized update"
    );
}

#[cfg(unix)]
#[test]
fn symlink_aliases_share_the_same_process_lock_domain() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("state root");
    let alias_parent = tempdir().expect("alias parent");
    let alias = alias_parent.path().join("state-alias");
    symlink(directory.path(), &alias).expect("state-root alias should create");

    let direct = LocalNetworkStateStore::open(directory.path()).expect("direct store should open");
    let aliased = LocalNetworkStateStore::open(&alias).expect("aliased store should open");

    assert!(
        Arc::ptr_eq(&direct.process_lock, &aliased.process_lock),
        "path aliases for one authority inode must serialize through one in-process lock"
    );
}

#[test]
fn transactions_preserve_sibling_partitions_in_the_same_envelope() {
    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    let segment_partition = NetworkStatePartition::SegmentAllocations;
    let ipam_partition = fixture_partition();
    store
        .transaction(&segment_partition, |state: &mut FixtureState| {
            state.owner = Some("segment-owner".to_owned());
            Ok::<_, Infallible>(())
        })
        .expect("segment partition should commit");
    store
        .transaction(&ipam_partition, |state: &mut FixtureState| {
            state.owner = Some("ipam-owner".to_owned());
            Ok::<_, Infallible>(())
        })
        .expect("IPAM partition should commit");

    let segment: FixtureState = store
        .read(&segment_partition)
        .expect("segment partition should read")
        .expect("segment partition should exist");
    let ipam: FixtureState = store
        .read(&ipam_partition)
        .expect("IPAM partition should read")
        .expect("IPAM partition should exist");
    assert_eq!(segment.owner.as_deref(), Some("segment-owner"));
    assert_eq!(ipam.owner.as_deref(), Some("ipam-owner"));
    assert!(
        !store.filesystem_kind().is_empty(),
        "startup must record the detected filesystem kind"
    );
}

#[test]
fn tenant_ipam_inventory_is_deterministic_and_read_only_across_reopen() {
    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    for tenant in ["tenant-z", "tenant-a", "tenant-m"] {
        let partition = NetworkStatePartition::TenantIpam(
            TenantId::new(tenant).expect("fixture tenant should parse"),
        );
        store
            .transaction(&partition, |state: &mut FixtureState| {
                state.owner = Some(tenant.to_owned());
                Ok::<_, Infallible>(())
            })
            .expect("tenant IPAM partition should commit");
    }
    let before = fs::read(store.authority_path()).expect("authority should read");

    let expected = ["tenant-a", "tenant-m", "tenant-z"];
    assert_eq!(
        store
            .tenant_ipam_tenants()
            .expect("inventory should validate")
            .iter()
            .map(TenantId::as_str)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        fs::read(store.authority_path()).expect("authority should reread"),
        before,
        "partition enumeration must not rewrite authority"
    );

    let reopened = LocalNetworkStateStore::open(root.path()).expect("store should reopen cleanly");
    assert_eq!(
        reopened
            .tenant_ipam_tenants()
            .expect("reopened inventory should validate")
            .iter()
            .map(TenantId::as_str)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        fs::read(reopened.authority_path()).expect("reopened authority should read"),
        before,
        "fresh-process-style reopen must preserve inventory bytes"
    );
}

#[test]
fn tenant_ipam_inventory_rejects_unknown_and_malformed_partition_keys() {
    for invalid_key in ["future-network-authority", "tenant-ipam/"] {
        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        store
            .transaction(&fixture_partition(), |state: &mut FixtureState| {
                state.owner = Some("seed".to_owned());
                Ok::<_, Infallible>(())
            })
            .expect("seed should commit");
        let mut envelope: StoreEnvelope =
            serde_json::from_slice(&fs::read(store.authority_path()).expect("authority"))
                .expect("authority envelope should parse");
        envelope
            .body
            .records
            .insert(invalid_key.to_owned(), serde_json::json!({}));
        envelope.checksum = checksum_body(&envelope.body).expect("tampered checksum should render");
        let bytes = serde_json::to_vec_pretty(&envelope).expect("tampered envelope should render");
        fs::write(store.authority_path(), &bytes).expect("tampered authority should write");

        let error = store
            .tenant_ipam_tenants()
            .expect_err("invalid partition keys must fail closed");
        assert!(
            matches!(error, NetworkStateStoreError::Corrupt { .. }),
            "invalid partition key {invalid_key:?} produced {error:?}"
        );
        assert_eq!(
            fs::read(store.authority_path()).expect("rejected authority should remain"),
            bytes,
            "rejected inventory must not mutate checksum-valid bad state"
        );
    }
}

#[test]
fn absent_partition_noop_preserves_exact_authority_and_revision() {
    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    store
        .transaction(
            &NetworkStatePartition::SegmentAllocations,
            |state: &mut FixtureState| {
                state.owner = Some("sibling-owner".to_owned());
                Ok::<_, Infallible>(())
            },
        )
        .expect("sibling partition should commit");
    let before = fs::read(store.authority_path()).expect("authority bytes should read");
    let before_envelope: Value =
        serde_json::from_slice(&before).expect("authority envelope should parse");

    store
        .transaction(&fixture_partition(), |_state: &mut FixtureState| {
            Ok::<_, Infallible>(())
        })
        .expect("absent partition no-op should succeed");

    let after = fs::read(store.authority_path()).expect("authority bytes should reread");
    let after_envelope: Value =
        serde_json::from_slice(&after).expect("authority envelope should reparse");
    assert_eq!(
        after, before,
        "a locked absent-partition no-op must not rewrite shared authority"
    );
    assert_eq!(
        after_envelope["body"]["revision"], before_envelope["body"]["revision"],
        "a no-op must not consume a global authority revision"
    );
    assert!(
        store
            .read::<FixtureState>(&fixture_partition())
            .expect("absent partition should read")
            .is_none(),
        "a no-op must not materialize a default partition"
    );
}

#[test]
fn closure_rejection_does_not_publish_partial_state() {
    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    store
        .transaction(&fixture_partition(), |state: &mut FixtureState| {
            state.owner = Some("committed".to_owned());
            Ok::<_, &'static str>(())
        })
        .expect("seed should commit");

    let rejected = store.transaction(&fixture_partition(), |state: &mut FixtureState| {
        state.owner = Some("must-not-land".to_owned());
        Err::<(), _>("domain rejection")
    });
    assert!(matches!(
        rejected,
        Err(NetworkStateTransactionError::Operation("domain rejection"))
    ));
    let state: FixtureState = store
        .read(&fixture_partition())
        .expect("partition should read")
        .expect("partition should exist");
    assert_eq!(state.owner.as_deref(), Some("committed"));
}

#[test]
fn payload_serialization_failure_is_a_store_error_and_publishes_nothing() {
    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");

    let error = store
        .transaction(&fixture_partition(), |_state: &mut RefusesSerialization| {
            Ok::<_, Infallible>(())
        })
        .expect_err("payload serialization must fail closed");
    assert!(matches!(
        error,
        NetworkStateTransactionError::Store(NetworkStateStoreError::Serialization {
            partition: NetworkStatePartition::TenantIpam(_),
            ..
        })
    ));
    assert!(
        !store.authority_path().exists(),
        "a serialization failure must not publish an authority file"
    );
}

#[test]
fn exhausted_revision_rejects_mutation_without_replacing_authority() {
    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    store
        .transaction(&fixture_partition(), |state: &mut FixtureState| {
            state.owner = Some("still-live".to_owned());
            Ok::<_, Infallible>(())
        })
        .expect("seed should commit");
    let mut envelope: StoreEnvelope =
        serde_json::from_slice(&fs::read(store.authority_path()).expect("read authority"))
            .expect("parse authority");
    envelope.body.revision = u64::MAX;
    envelope.checksum = checksum_body(&envelope.body).expect("checksum should render");
    let exhausted_bytes = serde_json::to_vec_pretty(&envelope).expect("render authority");
    fs::write(store.authority_path(), &exhausted_bytes).expect("write exhausted authority");

    let error = store
        .transaction(&fixture_partition(), |state: &mut FixtureState| {
            state.owner = Some("must-not-land".to_owned());
            Ok::<_, Infallible>(())
        })
        .expect_err("revision exhaustion must fail closed");
    assert!(matches!(
        error,
        NetworkStateTransactionError::Store(NetworkStateStoreError::RevisionExhausted { .. })
    ));
    assert_eq!(
        fs::read(store.authority_path()).expect("read authority after rejection"),
        exhausted_bytes,
        "revision exhaustion must not replace durable authority"
    );
}

#[test]
fn durability_events_are_file_sync_then_replace_then_parent_sync() {
    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&events);
    transaction_with_durability_observer(
        &store,
        &fixture_partition(),
        move |event| recorded.lock().expect("event lock").push(event),
        |state: &mut FixtureState| {
            state.owner = Some("durable".to_owned());
            Ok::<_, Infallible>(())
        },
    )
    .expect("observed transaction should commit");

    assert_eq!(
        *events.lock().expect("event lock"),
        [
            NetworkStateDurabilityEvent::StateFileSynced,
            NetworkStateDurabilityEvent::StateReplaced,
            NetworkStateDurabilityEvent::ParentDirectorySynced,
        ]
    );
}

#[test]
fn truncated_state_fails_closed_with_authority_path() {
    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    store
        .transaction(&fixture_partition(), |state: &mut FixtureState| {
            state.owner = Some("truncate-me".to_owned());
            Ok::<_, Infallible>(())
        })
        .expect("seed should commit");
    fs::write(store.authority_path(), b"{").expect("truncate authority");

    let error = LocalNetworkStateStore::open(root.path())
        .expect_err("truncated authority must refuse startup");
    let rendered = error.to_string();
    assert!(rendered.contains("corrupt"));
    assert!(rendered.contains(&store.authority_path().display().to_string()));
}

#[test]
fn checksum_rejects_semantically_valid_tampering() {
    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    store
        .transaction(&fixture_partition(), |state: &mut FixtureState| {
            state.owner = Some("live".to_owned());
            Ok::<_, Infallible>(())
        })
        .expect("seed should commit");
    let mut envelope: Value =
        serde_json::from_slice(&fs::read(store.authority_path()).expect("read authority"))
            .expect("parse authority");
    envelope["body"]["records"][fixture_partition().key()]["owner"] = Value::Null;
    fs::write(
        store.authority_path(),
        serde_json::to_vec_pretty(&envelope).expect("render tampered authority"),
    )
    .expect("write tampered authority");

    let error = LocalNetworkStateStore::open(root.path())
        .expect_err("checksum mismatch must refuse startup");
    assert!(matches!(
        error,
        NetworkStateStoreError::ChecksumMismatch { .. }
    ));
}

#[test]
fn incompatible_version_is_distinct_from_corruption() {
    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    store
        .transaction(&fixture_partition(), |state: &mut FixtureState| {
            state.owner = Some("future-version".to_owned());
            Ok::<_, Infallible>(())
        })
        .expect("seed should commit");
    let mut envelope: Value =
        serde_json::from_slice(&fs::read(store.authority_path()).expect("read authority"))
            .expect("parse authority");
    envelope["version"] = Value::from(FORMAT_VERSION + 1);
    fs::write(
        store.authority_path(),
        serde_json::to_vec_pretty(&envelope).expect("render future authority"),
    )
    .expect("write future authority");

    let error =
        LocalNetworkStateStore::open(root.path()).expect_err("future version must refuse startup");
    assert!(matches!(
        error,
        NetworkStateStoreError::IncompatibleVersion {
            found: 3,
            supported: 2,
            ..
        }
    ));
}

#[test]
fn stale_stage_is_removed_without_changing_committed_state() {
    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    store
        .transaction(&fixture_partition(), |state: &mut FixtureState| {
            state.owner = Some("committed".to_owned());
            Ok::<_, Infallible>(())
        })
        .expect("seed should commit");
    let stale = store.store_root.join(format!("{TEMP_PREFIX}crash.stage"));
    fs::write(&stale, b"partial future state").expect("write stale stage");

    let restarted = LocalNetworkStateStore::open(root.path()).expect("restart should clean stage");
    assert!(!stale.exists(), "crash stage must be removed under lock");
    let state: FixtureState = restarted
        .read(&fixture_partition())
        .expect("partition should read")
        .expect("partition should exist");
    assert_eq!(state.owner.as_deref(), Some("committed"));
}

#[test]
fn startup_removes_crash_leftovers_from_the_durability_probe() {
    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    let stale_stage = store.store_root.join(format!("{PROBE_PREFIX}crash.stage"));
    let stale_done = store.store_root.join(format!("{PROBE_PREFIX}crash.done"));
    fs::write(&stale_stage, b"partially written probe").expect("write probe stage");
    fs::write(&stale_done, b"renamed probe").expect("write probe destination");

    LocalNetworkStateStore::open(root.path()).expect("restart should clean probe leftovers");

    assert!(!stale_stage.exists(), "stale probe stage must be removed");
    assert!(!stale_done.exists(), "stale renamed probe must be removed");
}

#[cfg(unix)]
#[test]
fn authority_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    use super::owner_files::{OWNER_DIRECTORY_MODE, OWNER_FILE_MODE};

    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    store
        .transaction(&fixture_partition(), |state: &mut FixtureState| {
            state.owner = Some("private-authority".to_owned());
            Ok::<_, Infallible>(())
        })
        .expect("seed should commit");

    let state_mode = fs::metadata(store.authority_path())
        .expect("state metadata")
        .permissions()
        .mode()
        & 0o777;
    let lock_mode = fs::metadata(&store.lock_path)
        .expect("lock metadata")
        .permissions()
        .mode()
        & 0o777;
    let root_mode = fs::metadata(&store.store_root)
        .expect("root metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(state_mode, OWNER_FILE_MODE);
    assert_eq!(lock_mode, OWNER_FILE_MODE);
    assert_eq!(root_mode, OWNER_DIRECTORY_MODE);
}

#[cfg(unix)]
#[test]
fn insecure_authority_permissions_fail_closed_on_restart() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempdir().expect("state root");
    let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    store
        .transaction(&fixture_partition(), |state: &mut FixtureState| {
            state.owner = Some("permission-check".to_owned());
            Ok::<_, Infallible>(())
        })
        .expect("seed should commit");
    fs::set_permissions(store.authority_path(), fs::Permissions::from_mode(0o644))
        .expect("weaken authority permissions");

    let error = LocalNetworkStateStore::open(root.path())
        .expect_err("group/world-readable authority must refuse startup");
    assert!(matches!(
        error,
        NetworkStateStoreError::InsecurePermissions { mode: 0o644, .. }
    ));
}

#[test]
fn cleanup_pending_payload_survives_repeated_restart() {
    let root = tempdir().expect("state root");
    let mut store = LocalNetworkStateStore::open(root.path()).expect("store should open");
    store
        .transaction(&fixture_partition(), |state: &mut FixtureState| {
            state.cleanup_pending.insert(
                "portlease-a".to_owned(),
                "provider-delete-ambiguous".to_owned(),
            );
            Ok::<_, Infallible>(())
        })
        .expect("cleanup-pending state should commit");

    for _ in 0..3 {
        store = LocalNetworkStateStore::open(root.path()).expect("restart should open");
        let state: FixtureState = store
            .read(&fixture_partition())
            .expect("partition should read")
            .expect("partition should exist");
        assert_eq!(
            state.cleanup_pending.get("portlease-a").map(String::as_str),
            Some("provider-delete-ambiguous")
        );
    }
}

#[test]
fn contended_lock_times_out_without_an_unlocked_read() {
    let root = tempdir().expect("state root");
    let options = LocalNetworkStateStoreOptions {
        lock_timeout: Duration::from_millis(50),
        lock_retry_interval: Duration::from_millis(2),
    };
    let holder =
        LocalNetworkStateStore::open_with_options(root.path(), options).expect("holder open");
    let contender =
        LocalNetworkStateStore::open_with_options(root.path(), options).expect("contender open");
    let (held_tx, held_rx) = mpsc::sync_channel(0);
    let (release_tx, release_rx) = mpsc::sync_channel(0);

    let holder_thread = std::thread::spawn(move || {
        transaction_with_durability_observer(
            &holder,
            &fixture_partition(),
            |event| {
                if event == NetworkStateDurabilityEvent::StateFileSynced {
                    held_tx.send(()).expect("held signal should deliver");
                    release_rx.recv().expect("release signal should deliver");
                }
            },
            |state: &mut FixtureState| {
                state.owner = Some("holder".to_owned());
                Ok::<_, Infallible>(())
            },
        )
        .expect("holder transaction should finish after release");
    });
    held_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("holder must reach the synced stage");

    let error = contender
        .read::<FixtureState>(&fixture_partition())
        .expect_err("contender must fail closed while the authority lock is held");
    assert!(
        matches!(error, NetworkStateStoreError::LockTimeout { .. }),
        "contender must report a bounded lock timeout: {error}"
    );
    release_tx.send(()).expect("holder release should deliver");
    holder_thread.join().expect("holder thread should join");

    let state: FixtureState = contender
        .read(&fixture_partition())
        .expect("read should succeed after release")
        .expect("partition should exist");
    assert_eq!(state.owner.as_deref(), Some("holder"));
}
