use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use tempfile::{TempDir, tempdir};
use tokio::runtime::Handle as TokioRuntimeHandle;
use tokio::sync::Notify;
use tokio::time::{Duration, timeout};

use super::*;

const REFRESH_GATE_RELEASE_TIMEOUT: Duration = Duration::from_secs(60);

struct TestReplica {
    _tempdir: TempDir,
    store: LibsqlReplicaTenantStore,
}

struct RefreshGate {
    entered: Notify,
    release_gate: (Mutex<bool>, Condvar),
}

impl RefreshGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Notify::new(),
            release_gate: (Mutex::new(false), Condvar::new()),
        })
    }

    async fn wait_until_entered(&self) {
        self.entered.notified().await;
    }

    fn block(&self) {
        self.entered.notify_one();
        let (lock, cvar) = &self.release_gate;
        let released = lock
            .lock()
            .expect("refresh gate should acquire release lock");
        let (released, _) = cvar
            .wait_timeout_while(released, REFRESH_GATE_RELEASE_TIMEOUT, |released| {
                !*released
            })
            .expect("refresh gate should wait for release");
        assert!(
            *released,
            "replica refresh gate was not released within {REFRESH_GATE_RELEASE_TIMEOUT:?}; the \
             test likely exited before calling release()"
        );
    }

    fn release(&self) {
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("refresh gate should acquire release lock");
        *released = true;
        cvar.notify_all();
    }
}

async fn test_replica(refresh_override: TestRefreshOverride) -> TestReplica {
    let tempdir = tempdir().expect("replica tempdir should create");
    let replica_cache_dir = tempdir.path().join("replicas");
    std::fs::create_dir_all(&replica_cache_dir).expect("replica cache root should create");
    let tenant_id = TenantId::new("freshness-test").expect("tenant id should build");
    let replica_dir = replica_cache_dir.join(tenant_id.as_str());
    std::fs::create_dir_all(&replica_dir).expect("tenant replica dir should create");
    let replica_path = replica_dir.join("cache-0.sqlite3");
    let local_store = Arc::new(
        SqliteTenantStore::open_with_simulation_and_max_read_connections(
            &replica_path,
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
            2,
        )
        .expect("local replica cache should open"),
    );
    let metadata_database = Arc::new(
        Builder::new_remote("http://127.0.0.1:1".to_string(), String::new())
            .connector(libsql_transport_connector().expect("libsql connector should build"))
            .build()
            .await
            .expect("metadata database should open"),
    );
    let remote_database = Arc::new(
        Builder::new_remote("http://127.0.0.1:1".to_string(), String::new())
            .connector(libsql_transport_connector().expect("libsql connector should build"))
            .build()
            .await
            .expect("remote database should open"),
    );
    let provider = LibsqlReplicaProvider {
        primary_url: "memory://primary".to_string(),
        auth_token: None,
        admin_api_url: "http://localhost".to_string(),
        admin_auth_header: None,
        metadata_namespace: "nimbus_provider".to_string(),
        tenant_namespace_prefix: "tenant_".to_string(),
        replica_cache_dir,
        encryption_provider: None,
        runtime_handle: TokioRuntimeHandle::current(),
        clock: Arc::new(SystemClock),
        remote_fault_injector: Arc::new(NoopFaultInjector),
        replica_fault_injector: Arc::new(NoopFaultInjector),
        tenant_read_parallelism: 1,
        metadata_database,
    };
    let mut store = LibsqlReplicaTenantStore::new(
        provider,
        tenant_id,
        "tenant_freshness_test".to_string(),
        remote_database,
        local_store,
        replica_path,
    );
    store.refresh_override = Some(refresh_override);

    TestReplica {
        _tempdir: tempdir,
        store,
    }
}

fn apply_empty_records_through(
    store: &LibsqlReplicaTenantStore,
    target: SequenceNumber,
) -> Result<ReplicaRefreshOutcome> {
    let local = store.active_cache_store()?;
    let current = local.journal_progress()?;
    if current.durable_head.0 < target.0 {
        let records = ((current.durable_head.0 + 1)..=target.0)
            .map(|sequence| {
                TenantEventRecord::from_events(
                    SequenceNumber(sequence),
                    Timestamp(sequence),
                    Vec::new(),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        local.append_durable_records_batch(&records)?;
    }
    let progress = local.recover_durable_journal()?;
    Ok(ReplicaRefreshOutcome {
        path: LibsqlReplicaRefreshPath::IncrementalCatchUp,
        progress,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn freshness_barrier_waits_for_background_refresh_completion() {
    let gate = RefreshGate::new();
    let calls = Arc::new(AtomicU64::new(0));
    let refresh_override = {
        let gate = gate.clone();
        let calls = calls.clone();
        Arc::new(move |store: &LibsqlReplicaTenantStore| {
            calls.fetch_add(1, Ordering::SeqCst);
            gate.block();
            apply_empty_records_through(store, SequenceNumber(1))
        })
    };
    let replica = test_replica(refresh_override).await;
    let store = replica.store.clone();

    store.note_required_cache_sequence_with_cause(
        SequenceNumber(1),
        LibsqlReplicaRefreshCause::CommitBarrier,
    );
    timeout(Duration::from_secs(1), gate.wait_until_entered())
        .await
        .expect("background refresh should start");

    let barrier_store = store.clone();
    let barrier = tokio::task::spawn_blocking(move || barrier_store.ensure_local_cache_current());
    tokio::time::sleep(Duration::from_millis(25)).await;
    gate.release();

    timeout(Duration::from_secs(1), barrier)
        .await
        .expect("barrier should complete after background refresh")
        .expect("barrier task should join")
        .expect("barrier should succeed");
    let stats = store
        .replica_freshness_stats()
        .expect("freshness stats should load");
    assert_eq!(stats.required_sequence, SequenceNumber(1));
    assert_eq!(stats.local_applied_sequence, SequenceNumber(1));
    assert_eq!(
        stats.last_barrier_path,
        LibsqlReplicaBarrierPath::WaitedForBackgroundRefresh
    );
    assert_eq!(stats.barrier_waited_for_background_refresh_count, 1);
    assert!(
        calls.load(Ordering::SeqCst) >= 1,
        "background refresh should be responsible for satisfying the barrier"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn freshness_barrier_falls_back_to_sync_refresh_after_background_error() {
    let gate = RefreshGate::new();
    let calls = Arc::new(AtomicU64::new(0));
    let refresh_override = {
        let gate = gate.clone();
        let calls = calls.clone();
        Arc::new(move |store: &LibsqlReplicaTenantStore| {
            let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call == 1 {
                gate.block();
                return Err(Error::Internal(
                    "scripted background refresh failure".to_string(),
                ));
            }
            apply_empty_records_through(store, SequenceNumber(1))
        })
    };
    let replica = test_replica(refresh_override).await;
    let store = replica.store.clone();

    store.note_required_cache_sequence_with_cause(
        SequenceNumber(1),
        LibsqlReplicaRefreshCause::CommitBarrier,
    );
    timeout(Duration::from_secs(1), gate.wait_until_entered())
        .await
        .expect("background refresh should start");

    let barrier_store = store.clone();
    let barrier = tokio::task::spawn_blocking(move || barrier_store.ensure_local_cache_current());
    tokio::time::sleep(Duration::from_millis(25)).await;
    gate.release();

    timeout(Duration::from_secs(1), barrier)
        .await
        .expect("barrier should complete after synchronous fallback")
        .expect("barrier task should join")
        .expect("barrier should succeed");
    let stats = store
        .replica_freshness_stats()
        .expect("freshness stats should load");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(stats.required_sequence, SequenceNumber(1));
    assert_eq!(stats.local_applied_sequence, SequenceNumber(1));
    assert_eq!(
        stats.last_barrier_path,
        LibsqlReplicaBarrierPath::IncrementalCatchUp
    );
    assert_eq!(stats.barrier_incremental_catch_up_count, 1);
    assert_eq!(stats.refresh_error_count, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn freshness_background_refresh_reschedules_for_mid_refresh_sequence_bump() {
    let calls = Arc::new(AtomicU64::new(0));
    let refresh_override = {
        let calls = calls.clone();
        Arc::new(move |store: &LibsqlReplicaTenantStore| {
            let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call == 1 {
                store.note_required_cache_sequence_with_cause(
                    SequenceNumber(2),
                    LibsqlReplicaRefreshCause::CommitBarrier,
                );
                return apply_empty_records_through(store, SequenceNumber(1));
            }
            apply_empty_records_through(store, SequenceNumber(2))
        })
    };
    let replica = test_replica(refresh_override).await;
    let store = replica.store.clone();

    store.note_required_cache_sequence_with_cause(
        SequenceNumber(1),
        LibsqlReplicaRefreshCause::CommitBarrier,
    );
    timeout(Duration::from_secs(1), async {
        loop {
            if calls.load(Ordering::SeqCst) >= 2
                && store
                    .applied_sequence()
                    .expect("applied sequence should load")
                    == SequenceNumber(2)
                && !store.refresh_inflight.load(Ordering::Acquire)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("background refresh should reschedule and catch up");
    let stats = store
        .replica_freshness_stats()
        .expect("freshness stats should load");
    assert_eq!(stats.required_sequence, SequenceNumber(2));
    assert_eq!(stats.local_applied_sequence, SequenceNumber(2));
    assert_eq!(stats.incremental_refresh_count, 2);
    assert_eq!(stats.refresh_error_count, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recovered_remote_progress_remains_required_after_cache_wins_refresh_race() {
    let refresh_override: TestRefreshOverride =
        Arc::new(|store| apply_empty_records_through(store, SequenceNumber(2)));
    let replica = test_replica(refresh_override).await;
    let store = replica.store.clone();

    // Model the schema-mismatch refresh winning before journal recovery: the
    // derivative cache is already current, but no remote head has yet been
    // retained as a freshness requirement.
    let progress = apply_empty_records_through(&store, SequenceNumber(2))
        .expect("cache should refresh through the remote writes")
        .progress;
    let before = store
        .replica_freshness_stats()
        .expect("freshness stats should load before recovery observation");
    assert_eq!(before.local_applied_sequence, SequenceNumber(2));
    assert_eq!(before.required_sequence, SequenceNumber(0));

    store.note_recovered_remote_progress(progress.durable_head);

    let observed = store
        .replica_freshness_stats()
        .expect("freshness stats should load after recovery observation");
    assert_eq!(observed.required_sequence, SequenceNumber(2));
    assert_eq!(observed.local_applied_sequence, SequenceNumber(2));

    timeout(Duration::from_secs(1), async {
        while store.refresh_inflight.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("recovery observation refresh should settle");
    let settled = store
        .replica_freshness_stats()
        .expect("freshness stats should load after refresh settles");
    assert_eq!(
        settled.last_refresh_cause,
        LibsqlReplicaRefreshCause::DurableJournalReplay
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recovery_retains_newer_progress_returned_after_cache_barrier() {
    let refresh_override: TestRefreshOverride =
        Arc::new(|store| apply_empty_records_through(store, SequenceNumber(2)));
    let replica = test_replica(refresh_override).await;
    let store = replica.store.clone();

    // Recovery initially observes only the schema write. While its cache
    // barrier runs, a snapshot can include the following document write and
    // cause recovery to return a newer durable head than it observed at entry.
    store.note_recovered_remote_progress(SequenceNumber(1));
    let returned = store.retain_recovered_progress(JournalProgress {
        durable_head: SequenceNumber(2),
        applied_head: SequenceNumber(2),
    });

    let freshness = store
        .replica_freshness_stats()
        .expect("freshness stats should retain returned recovery progress");
    assert_eq!(returned.durable_head, SequenceNumber(2));
    assert_eq!(freshness.required_sequence, returned.durable_head);

    timeout(Duration::from_secs(1), async {
        while store.refresh_inflight.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("retained recovery progress refresh should settle");
}
