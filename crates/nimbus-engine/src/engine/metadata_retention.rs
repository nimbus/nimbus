//! Engine-owned lifecycle for durable metadata-history retention.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use nimbus_core::{Error, MonotonicClock, Result, SequenceNumber};
use nimbus_storage::{RetentionHistoryState, RetentionHistorySummary};
use serde::Serialize;
use tokio::sync::{Notify, oneshot};
use tokio_util::sync::CancellationToken;

use crate::persistence_config::MetadataRetentionProfile;
use crate::tenant::TenantRuntime;

use super::Engine;

const RETENTION_RETRY_DELAY: Duration = Duration::from_secs(1);
const RETENTION_PERIODIC_RECHECK: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetadataRetentionRunResult {
    pub compacted: bool,
    pub desired_floor: SequenceNumber,
    pub confirmed_floor: SequenceNumber,
    pub physical_floor: SequenceNumber,
    pub journal_records_pruned: u64,
    pub document_versions_pruned: u64,
    pub index_versions_pruned: u64,
    pub duration_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetadataRetentionDiagnosticsSnapshot {
    pub profile: MetadataRetentionProfile,
    pub controller_running: bool,
    pub maintenance_running: bool,
    pub run_count: u64,
    pub success_count: u64,
    pub retention_failure_count: u64,
    pub desired_floor: SequenceNumber,
    pub confirmed_floor: SequenceNumber,
    pub physical_floor: SequenceNumber,
    pub retention_floor_lag_sequences: u64,
    pub retention_journal_records_pruned: u64,
    pub retention_document_versions_pruned: u64,
    pub retention_index_versions_pruned: u64,
    pub retention_last_duration_millis: Option<u64>,
    pub last_failure: Option<String>,
    pub next_eligible_floor: Option<SequenceNumber>,
    pub next_retry_in_millis: Option<u64>,
}

#[derive(Default)]
struct RetentionMetrics {
    maintenance_running: bool,
    run_count: u64,
    success_count: u64,
    failure_count: u64,
    desired_floor: SequenceNumber,
    confirmed_floor: SequenceNumber,
    physical_floor: SequenceNumber,
    journal_records_pruned: u64,
    document_versions_pruned: u64,
    index_versions_pruned: u64,
    last_duration: Option<Duration>,
    last_failure: Option<String>,
    next_retry_at: Option<Instant>,
}

struct ManualRequestQueue {
    accepting: bool,
    pending: VecDeque<oneshot::Sender<Result<MetadataRetentionRunResult>>>,
}

struct ControllerCompletion {
    finished: Mutex<bool>,
    blocking: Condvar,
    asynchronous: Notify,
}

impl ControllerCompletion {
    fn new() -> Self {
        Self {
            finished: Mutex::new(false),
            blocking: Condvar::new(),
            asynchronous: Notify::new(),
        }
    }

    fn finish(&self) {
        *self
            .finished
            .lock()
            .expect("metadata-retention completion lock should not be poisoned") = true;
        self.blocking.notify_all();
        self.asynchronous.notify_waiters();
    }

    fn wait_blocking(&self) {
        let mut finished = self
            .finished
            .lock()
            .expect("metadata-retention completion lock should not be poisoned");
        while !*finished {
            finished = self
                .blocking
                .wait(finished)
                .expect("metadata-retention completion wait should not be poisoned");
        }
    }

    async fn wait(&self) {
        loop {
            if *self
                .finished
                .lock()
                .expect("metadata-retention completion lock should not be poisoned")
            {
                return;
            }
            let notified = self.asynchronous.notified();
            if *self
                .finished
                .lock()
                .expect("metadata-retention completion lock should not be poisoned")
            {
                return;
            }
            notified.await;
        }
    }
}

pub(crate) struct MetadataRetentionController {
    profile: MetadataRetentionProfile,
    monotonic_clock: Arc<dyn MonotonicClock>,
    started: AtomicBool,
    running: AtomicBool,
    shutdown: CancellationToken,
    wake: Notify,
    next_sequence_hint: AtomicU64,
    #[cfg(test)]
    state_inspection_count: AtomicU64,
    manual_requests: Mutex<ManualRequestQueue>,
    metrics: Mutex<RetentionMetrics>,
    completion: ControllerCompletion,
}

impl MetadataRetentionController {
    pub(crate) fn new(
        profile: MetadataRetentionProfile,
        monotonic_clock: Arc<dyn MonotonicClock>,
    ) -> Arc<Self> {
        let next_sequence_hint = profile
            .minimum_window_sequences()
            .zip(profile.maintenance_step_sequences())
            .map(|(window, step)| window.saturating_add(step))
            .unwrap_or(u64::MAX);
        Arc::new(Self {
            profile,
            monotonic_clock,
            started: AtomicBool::new(false),
            running: AtomicBool::new(false),
            shutdown: CancellationToken::new(),
            wake: Notify::new(),
            next_sequence_hint: AtomicU64::new(next_sequence_hint),
            #[cfg(test)]
            state_inspection_count: AtomicU64::new(0),
            manual_requests: Mutex::new(ManualRequestQueue {
                accepting: false,
                pending: VecDeque::new(),
            }),
            metrics: Mutex::new(RetentionMetrics::default()),
            completion: ControllerCompletion::new(),
        })
    }

    pub(crate) fn mark_started(&self) -> Result<()> {
        if self.shutdown.is_cancelled() {
            return Err(Error::ResourceExhausted(
                "tenant metadata-retention controller is shutting down".to_string(),
            ));
        }
        self.started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                Error::Internal(
                    "tenant metadata-retention controller started more than once".to_string(),
                )
            })?;
        if self.shutdown.is_cancelled() {
            self.completion.finish();
            return Err(Error::ResourceExhausted(
                "tenant metadata-retention controller is shutting down".to_string(),
            ));
        }
        self.manual_requests
            .lock()
            .expect("metadata-retention request queue should not be poisoned")
            .accepting = true;
        Ok(())
    }

    pub(crate) fn notify_progress(&self, durable_head: SequenceNumber) {
        if durable_head.0 >= self.next_sequence_hint.load(Ordering::Acquire) {
            self.wake.notify_one();
        }
    }

    #[cfg(test)]
    pub(crate) fn wake_for_testing(&self) {
        self.wake.notify_one();
    }

    #[cfg(test)]
    pub(crate) fn state_inspection_count_for_testing(&self) -> u64 {
        self.state_inspection_count.load(Ordering::Acquire)
    }

    pub(crate) fn shutdown(&self) {
        self.shutdown.cancel();
        self.stop_manual_requests(Error::ResourceExhausted(
            "tenant metadata-retention controller is shutting down".to_string(),
        ));
        self.wake.notify_waiters();
    }

    pub(crate) fn wait_finished_blocking(&self) {
        if self.started.load(Ordering::Acquire) {
            self.completion.wait_blocking();
        }
    }

    pub(crate) async fn wait_finished(&self) {
        if self.started.load(Ordering::Acquire) {
            self.completion.wait().await;
        }
    }

    pub(crate) async fn request_run(&self) -> Result<MetadataRetentionRunResult> {
        let (sender, receiver) = oneshot::channel();
        {
            let mut requests = self
                .manual_requests
                .lock()
                .expect("metadata-retention request queue should not be poisoned");
            if self.shutdown.is_cancelled() || !requests.accepting {
                return Err(Error::ResourceExhausted(
                    "tenant metadata-retention controller is not accepting requests".to_string(),
                ));
            }
            requests.pending.push_back(sender);
        }
        self.wake.notify_one();
        receiver.await.map_err(|_| {
            Error::ResourceExhausted(
                "tenant metadata-retention controller stopped before the request completed"
                    .to_string(),
            )
        })?
    }

    pub(crate) fn snapshot(&self) -> MetadataRetentionDiagnosticsSnapshot {
        let metrics = self
            .metrics
            .lock()
            .expect("metadata-retention metrics lock should not be poisoned");
        let now = self.monotonic_clock.now();
        let next_retry_in_millis = metrics.next_retry_at.map(|retry_at| {
            retry_at
                .saturating_duration_since(now)
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX)
        });
        MetadataRetentionDiagnosticsSnapshot {
            profile: self.profile,
            controller_running: self.running.load(Ordering::Acquire),
            maintenance_running: metrics.maintenance_running,
            run_count: metrics.run_count,
            success_count: metrics.success_count,
            retention_failure_count: metrics.failure_count,
            desired_floor: metrics.desired_floor,
            confirmed_floor: metrics.confirmed_floor,
            physical_floor: metrics.physical_floor,
            retention_floor_lag_sequences: metrics
                .desired_floor
                .0
                .saturating_sub(metrics.confirmed_floor.0),
            retention_journal_records_pruned: metrics.journal_records_pruned,
            retention_document_versions_pruned: metrics.document_versions_pruned,
            retention_index_versions_pruned: metrics.index_versions_pruned,
            retention_last_duration_millis: metrics
                .last_duration
                .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX)),
            last_failure: metrics.last_failure.clone(),
            next_eligible_floor: self
                .profile
                .maintenance_step_sequences()
                .map(|_| SequenceNumber(self.next_sequence_hint.load(Ordering::Acquire))),
            next_retry_in_millis,
        }
    }

    pub(crate) async fn run(
        self: Arc<Self>,
        runtime: Weak<TenantRuntime>,
        engine_shutdown: CancellationToken,
    ) {
        self.running.store(true, Ordering::Release);
        struct Finish(Arc<MetadataRetentionController>);
        impl Drop for Finish {
            fn drop(&mut self) {
                self.0.running.store(false, Ordering::Release);
                let error = Error::ResourceExhausted(
                    "tenant metadata-retention controller stopped".to_string(),
                );
                self.0.stop_manual_requests(error);
                self.0.completion.finish();
            }
        }
        let _finish = Finish(self.clone());

        // Evaluate durable state once at load. A tenant can already be beyond
        // the maintenance step before this process starts.
        self.wake.notify_one();
        loop {
            let recheck_delay = self.next_recheck_delay();
            tokio::select! {
                _ = self.shutdown.cancelled() => break,
                _ = engine_shutdown.cancelled() => break,
                _ = self.wake.notified() => {},
                _ = tokio::time::sleep(recheck_delay) => {},
            }
            if self.shutdown.is_cancelled() || engine_shutdown.is_cancelled() {
                break;
            }
            let manual_requests = self.drain_manual_requests();
            let forced = !manual_requests.is_empty();
            let Some(runtime) = runtime.upgrade() else {
                break;
            };
            let result = self.run_if_eligible(runtime, forced).await;
            if forced {
                for request in manual_requests {
                    let _ = request.send(result.clone().and_then(|result| {
                        result.ok_or_else(|| {
                            Error::Internal(
                                "forced metadata-retention run produced no result".to_string(),
                            )
                        })
                    }));
                }
            }
        }
    }

    fn drain_manual_requests(&self) -> Vec<oneshot::Sender<Result<MetadataRetentionRunResult>>> {
        self.manual_requests
            .lock()
            .expect("metadata-retention request queue should not be poisoned")
            .pending
            .drain(..)
            .collect()
    }

    fn stop_manual_requests(&self, error: Error) {
        let requests = {
            let mut queue = self
                .manual_requests
                .lock()
                .expect("metadata-retention request queue should not be poisoned");
            queue.accepting = false;
            queue.pending.drain(..).collect::<Vec<_>>()
        };
        for request in requests {
            let _ = request.send(Err(error.clone()));
        }
    }

    async fn run_if_eligible(
        &self,
        runtime: Arc<TenantRuntime>,
        forced: bool,
    ) -> Result<Option<MetadataRetentionRunResult>> {
        let tenant_id = runtime.tenant_id().clone();
        let _operation = runtime.enter_operation(&tenant_id)?;
        let config = self.profile.retention_config();
        if !forced && !self.retry_due() {
            return Ok(None);
        }
        let inspection_started = self.monotonic_clock.now();
        let state_runtime = runtime.clone();
        let state = match tokio::task::spawn_blocking(move || {
            state_runtime.store.retention_history_state(config)
        })
        .await
        {
            Ok(Ok(state)) => state,
            Ok(Err(error)) => {
                let duration = self
                    .monotonic_clock
                    .now()
                    .saturating_duration_since(inspection_started);
                self.record_failure(&error, duration, true);
                tracing::warn!(
                    tenant_id = %tenant_id,
                    error = %error,
                    "tenant metadata-retention state inspection failed; retry remains eligible"
                );
                return Err(error);
            }
            Err(join_error) => {
                let error = Error::Internal(format!(
                    "metadata-retention state task panicked before completion: {join_error}"
                ));
                let duration = self
                    .monotonic_clock
                    .now()
                    .saturating_duration_since(inspection_started);
                self.record_failure(&error, duration, true);
                tracing::warn!(
                    tenant_id = %tenant_id,
                    error = %error,
                    "tenant metadata-retention state inspection failed; retry remains eligible"
                );
                return Err(error);
            }
        };
        #[cfg(test)]
        self.state_inspection_count.fetch_add(1, Ordering::AcqRel);
        self.observe_state(&state);

        let Some(step) = self.profile.maintenance_step_sequences() else {
            self.record_successful_inspection_without_maintenance();
            return Ok(forced.then(|| run_result_from_state(false, &state, Duration::ZERO)));
        };
        if !forced && state.latest_sequence.0 < self.next_sequence_hint.load(Ordering::Acquire) {
            self.record_successful_inspection_without_maintenance();
            return Ok(None);
        }

        {
            let mut metrics = self
                .metrics
                .lock()
                .expect("metadata-retention metrics lock should not be poisoned");
            metrics.maintenance_running = true;
            metrics.run_count = metrics.run_count.saturating_add(1);
        }
        let started = self.monotonic_clock.now();
        let prepare_runtime = runtime.clone();
        let prepared = match tokio::task::spawn_blocking(move || {
            prepare_runtime.store.prepare_retained_history(config)
        })
        .await
        {
            Ok(Ok(prepared)) => prepared,
            Ok(Err(error)) => {
                let duration = self
                    .monotonic_clock
                    .now()
                    .saturating_duration_since(started);
                self.record_failure(&error, duration, false);
                tracing::warn!(
                    tenant_id = %tenant_id,
                    error = %error,
                    "tenant metadata-retention checkpoint preparation failed; retry remains eligible"
                );
                return Err(error);
            }
            Err(join_error) => {
                let error = Error::Internal(format!(
                    "metadata-retention preparation task panicked before completion: {join_error}"
                ));
                let duration = self
                    .monotonic_clock
                    .now()
                    .saturating_duration_since(started);
                self.record_failure(&error, duration, false);
                tracing::warn!(
                    tenant_id = %tenant_id,
                    error = %error,
                    "tenant metadata-retention checkpoint preparation failed; retry remains eligible"
                );
                return Err(error);
            }
        };
        let compact_runtime = runtime.clone();
        let compact = runtime
            .submit_internal_committer_async(move || {
                compact_runtime.finalize_retained_history_in_actor(prepared)
            })
            .await;
        let duration = self
            .monotonic_clock
            .now()
            .saturating_duration_since(started);
        match compact {
            Ok(summary) => {
                self.record_success(&summary, duration);
                self.update_next_sequence_hint(
                    summary.watermarks.document_versions.latest_sequence,
                    step,
                );
                Ok(Some(run_result_from_summary(&summary, duration)))
            }
            Err(error) => {
                self.record_failure(&error, duration, false);
                tracing::warn!(
                    tenant_id = %tenant_id,
                    error = %error,
                    "tenant metadata-retention maintenance failed; retry remains eligible"
                );
                Err(error)
            }
        }
    }

    fn observe_state(&self, state: &RetentionHistoryState) {
        let mut metrics = self
            .metrics
            .lock()
            .expect("metadata-retention metrics lock should not be poisoned");
        metrics.desired_floor = state.desired_floor;
        metrics.confirmed_floor = state.confirmed_floor;
        metrics.physical_floor = state.physical_floor;
    }

    fn update_next_sequence_hint(&self, latest_sequence: SequenceNumber, step: u64) {
        self.next_sequence_hint
            .store(latest_sequence.0.saturating_add(step), Ordering::Release);
    }

    fn retry_due(&self) -> bool {
        self.metrics
            .lock()
            .expect("metadata-retention metrics lock should not be poisoned")
            .next_retry_at
            .is_none_or(|retry_at| self.monotonic_clock.now() >= retry_at)
    }

    fn next_recheck_delay(&self) -> Duration {
        let now = self.monotonic_clock.now();
        self.metrics
            .lock()
            .expect("metadata-retention metrics lock should not be poisoned")
            .next_retry_at
            .map(|retry_at| retry_at.saturating_duration_since(now))
            .unwrap_or(RETENTION_PERIODIC_RECHECK)
            .min(RETENTION_PERIODIC_RECHECK)
    }

    fn record_successful_inspection_without_maintenance(&self) {
        self.metrics
            .lock()
            .expect("metadata-retention metrics lock should not be poisoned")
            .next_retry_at = None;
    }

    fn record_success(&self, summary: &RetentionHistorySummary, duration: Duration) {
        let mut metrics = self
            .metrics
            .lock()
            .expect("metadata-retention metrics lock should not be poisoned");
        metrics.maintenance_running = false;
        metrics.success_count = metrics.success_count.saturating_add(1);
        metrics.desired_floor = summary.after.desired_floor;
        metrics.confirmed_floor = summary.after.confirmed_floor;
        metrics.physical_floor = summary.after.physical_floor;
        metrics.journal_records_pruned = metrics
            .journal_records_pruned
            .saturating_add(summary.journal_records_pruned);
        metrics.document_versions_pruned = metrics
            .document_versions_pruned
            .saturating_add(summary.document_versions_pruned);
        metrics.index_versions_pruned = metrics
            .index_versions_pruned
            .saturating_add(summary.index_versions_pruned);
        metrics.last_duration = Some(duration);
        metrics.last_failure = None;
        metrics.next_retry_at = None;
    }

    fn record_failure(&self, error: &Error, duration: Duration, count_run: bool) {
        let mut metrics = self
            .metrics
            .lock()
            .expect("metadata-retention metrics lock should not be poisoned");
        metrics.maintenance_running = false;
        if count_run {
            metrics.run_count = metrics.run_count.saturating_add(1);
        }
        metrics.failure_count = metrics.failure_count.saturating_add(1);
        metrics.last_duration = Some(duration);
        metrics.last_failure = Some(error.to_string());
        let now = self.monotonic_clock.now();
        metrics.next_retry_at = Some(now.checked_add(RETENTION_RETRY_DELAY).unwrap_or(now));
    }
}

fn run_result_from_summary(
    summary: &RetentionHistorySummary,
    duration: Duration,
) -> MetadataRetentionRunResult {
    MetadataRetentionRunResult {
        compacted: true,
        desired_floor: summary.after.desired_floor,
        confirmed_floor: summary.after.confirmed_floor,
        physical_floor: summary.after.physical_floor,
        journal_records_pruned: summary.journal_records_pruned,
        document_versions_pruned: summary.document_versions_pruned,
        index_versions_pruned: summary.index_versions_pruned,
        duration_millis: duration.as_millis().try_into().unwrap_or(u64::MAX),
    }
}

fn run_result_from_state(
    compacted: bool,
    state: &RetentionHistoryState,
    duration: Duration,
) -> MetadataRetentionRunResult {
    MetadataRetentionRunResult {
        compacted,
        desired_floor: state.desired_floor,
        confirmed_floor: state.confirmed_floor,
        physical_floor: state.physical_floor,
        journal_records_pruned: 0,
        document_versions_pruned: 0,
        index_versions_pruned: 0,
        duration_millis: duration.as_millis().try_into().unwrap_or(u64::MAX),
    }
}

impl Engine {
    /// Runs one retention cycle for a loaded tenant, independent of the
    /// automatic maintenance threshold.
    pub async fn run_metadata_retention_now(
        self: &Arc<Self>,
        tenant_id: nimbus_core::TenantId,
    ) -> Result<MetadataRetentionRunResult> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        runtime.metadata_retention_controller().request_run().await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use nimbus_core::{
        Error, ManualMonotonicClock, ManualWallClock, SequenceNumber, TableName, TenantId,
        Timestamp,
    };
    use nimbus_storage::{EmbeddedProviderKind, FaultPoint, NoopFaultInjector};
    use nimbus_testing::{BlockingFaultInjector, CountedFaultInjector};
    use serde_json::json;
    use tempfile::TempDir;
    use tokio::time::timeout;

    use crate::{EnginePersistenceConfig, MetadataRetentionProfile};

    use super::{Engine, MetadataRetentionDiagnosticsSnapshot};

    async fn engine_with_profile(
        profile: MetadataRetentionProfile,
        monotonic_clock: Arc<ManualMonotonicClock>,
        faults: Arc<dyn nimbus_storage::FaultInjector>,
    ) -> (TempDir, Arc<Engine>, TenantId) {
        let data_dir = tempfile::tempdir().expect("metadata-retention tempdir should build");
        let config =
            EnginePersistenceConfig::embedded(data_dir.path(), EmbeddedProviderKind::Sqlite)
                .with_metadata_retention(profile);
        let engine = Arc::new(
            Engine::new_with_simulation_clocks_and_persistence_config(
                config,
                Arc::new(ManualWallClock::new(Timestamp(10_000))),
                monotonic_clock,
                faults,
            )
            .await
            .expect("metadata-retention engine should build"),
        );
        let tenant_id = TenantId::new("metadata-retention").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("metadata-retention tenant should create");
        (data_dir, engine, tenant_id)
    }

    async fn insert_documents(engine: &Arc<Engine>, tenant_id: &TenantId, count: usize) {
        let table = TableName::new("retention_events").expect("table should build");
        for index in 0..count {
            engine
                .insert_document_async(
                    tenant_id.clone(),
                    table.clone(),
                    serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                )
                .await
                .expect("metadata-retention seed insert should succeed");
        }
    }

    async fn wait_for_diagnostics(
        engine: &Arc<Engine>,
        tenant_id: &TenantId,
        predicate: impl Fn(&MetadataRetentionDiagnosticsSnapshot) -> bool,
    ) -> MetadataRetentionDiagnosticsSnapshot {
        timeout(Duration::from_secs(5), async {
            loop {
                let snapshot = engine
                    .tenant_engine_diagnostics_async(tenant_id.clone())
                    .await
                    .expect("metadata-retention diagnostics should load")
                    .metadata_retention;
                if predicate(&snapshot) {
                    return snapshot;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("metadata-retention diagnostics should reach the expected state")
    }

    #[tokio::test]
    async fn shipped_profile_is_bounded_and_retain_all_is_explicit_in_diagnostics() {
        assert!(matches!(
            MetadataRetentionProfile::shipped(),
            MetadataRetentionProfile::Bounded {
                document_version_window_sequences: 100_000,
                index_version_window_sequences: 100_000,
                cdc_window_sequences: 50_000,
                pitr_window_sequences: 100_000,
                maintenance_step_sequences: 10_000,
            }
        ));

        let (_data_dir, engine, tenant_id) = engine_with_profile(
            MetadataRetentionProfile::retain_all(),
            Arc::new(ManualMonotonicClock::new()),
            Arc::new(NoopFaultInjector),
        )
        .await;
        let diagnostics =
            wait_for_diagnostics(&engine, &tenant_id, |snapshot| snapshot.controller_running).await;
        assert_eq!(diagnostics.profile, MetadataRetentionProfile::retain_all());
        assert_eq!(diagnostics.next_eligible_floor, None);
        engine.quiesce().await;
    }

    #[tokio::test]
    async fn automatic_maintenance_advances_after_one_sequence_step() {
        let profile = MetadataRetentionProfile::bounded(2, 2, 2, 2, 2)
            .expect("small bounded profile should build");
        let (_data_dir, engine, tenant_id) = engine_with_profile(
            profile,
            Arc::new(ManualMonotonicClock::new()),
            Arc::new(NoopFaultInjector),
        )
        .await;
        insert_documents(&engine, &tenant_id, 6).await;
        let diagnostics = wait_for_diagnostics(&engine, &tenant_id, |snapshot| {
            snapshot.success_count > 0 && snapshot.confirmed_floor > SequenceNumber(0)
        })
        .await;
        assert_eq!(diagnostics.retention_failure_count, 0);
        assert_eq!(
            diagnostics.confirmed_floor, diagnostics.physical_floor,
            "checkpoint publication and physical pruning must advance together"
        );
        engine.quiesce().await;
    }

    #[tokio::test]
    async fn automatic_maintenance_honors_the_smallest_resource_window() {
        let profile = MetadataRetentionProfile::bounded(2, 2, 100, 100, 2)
            .expect("resource-specific profile should build");
        let (_data_dir, engine, tenant_id) = engine_with_profile(
            profile,
            Arc::new(ManualMonotonicClock::new()),
            Arc::new(NoopFaultInjector),
        )
        .await;
        insert_documents(&engine, &tenant_id, 4).await;
        let diagnostics =
            wait_for_diagnostics(&engine, &tenant_id, |snapshot| snapshot.success_count == 1).await;
        assert_eq!(
            diagnostics.confirmed_floor,
            SequenceNumber(0),
            "document maintenance must not require journal-floor movement"
        );
        assert_eq!(diagnostics.next_eligible_floor, Some(SequenceNumber(6)));
        engine.quiesce().await;
    }

    #[tokio::test]
    async fn manual_requests_never_overlap_one_controller_run() {
        let faults = BlockingFaultInjector::new(FaultPoint::RetentionCheckpointBeforeCommit);
        let (_data_dir, engine, tenant_id) = engine_with_profile(
            MetadataRetentionProfile::bounded(1_000, 1_000, 1_000, 1_000, 10)
                .expect("manual profile should build"),
            Arc::new(ManualMonotonicClock::new()),
            faults.clone(),
        )
        .await;
        insert_documents(&engine, &tenant_id, 1).await;

        let mut first = tokio::spawn({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            async move { engine.run_metadata_retention_now(tenant_id).await }
        });
        faults.wait_until_entered().await;
        let mut second = tokio::spawn({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            async move { engine.run_metadata_retention_now(tenant_id).await }
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(!first.is_finished());
        assert!(!second.is_finished());
        let diagnostics = engine
            .tenant_engine_diagnostics_async(tenant_id.clone())
            .await
            .expect("running retention diagnostics should load")
            .metadata_retention;
        assert!(diagnostics.maintenance_running);
        assert_eq!(diagnostics.run_count, 1);

        faults.release();
        timeout(Duration::from_secs(5), &mut first)
            .await
            .expect("first manual run should finish")
            .expect("first manual task should join")
            .expect("first manual run should succeed");
        timeout(Duration::from_secs(5), &mut second)
            .await
            .expect("second manual run should finish")
            .expect("second manual task should join")
            .expect("second manual run should succeed");
        engine.quiesce().await;
    }

    #[tokio::test]
    async fn automatic_failure_retries_after_deterministic_clock_advance() {
        let monotonic_clock = Arc::new(ManualMonotonicClock::new());
        let faults = CountedFaultInjector::fail_first_n_calls(
            FaultPoint::RetentionCheckpointBeforeCommit,
            1,
        );
        let (_data_dir, engine, tenant_id) = engine_with_profile(
            MetadataRetentionProfile::bounded(2, 2, 2, 2, 2).expect("retry profile should build"),
            monotonic_clock.clone(),
            faults.clone(),
        )
        .await;
        insert_documents(&engine, &tenant_id, 6).await;
        wait_for_diagnostics(&engine, &tenant_id, |snapshot| {
            snapshot.retention_failure_count == 1
        })
        .await;

        monotonic_clock.advance(Duration::from_secs(1));
        engine
            .tenant_runtime_for_testing(&tenant_id)
            .expect("retry runtime should remain loaded")
            .metadata_retention_controller()
            .wake_for_testing();
        let diagnostics =
            wait_for_diagnostics(&engine, &tenant_id, |snapshot| snapshot.success_count == 1).await;
        assert_eq!(faults.failure_count(), 1);
        assert_eq!(diagnostics.run_count, 2);
        assert_eq!(diagnostics.last_failure, None);
        engine.quiesce().await;
    }

    #[tokio::test]
    async fn successful_ineligible_retry_rearms_the_periodic_delay() {
        let monotonic_clock = Arc::new(ManualMonotonicClock::new());
        let (_data_dir, engine, tenant_id) = engine_with_profile(
            MetadataRetentionProfile::bounded(1_000, 1_000, 1_000, 1_000, 10)
                .expect("ineligible retry profile should build"),
            monotonic_clock.clone(),
            Arc::new(NoopFaultInjector),
        )
        .await;
        let controller = engine
            .tenant_runtime_for_testing(&tenant_id)
            .expect("ineligible retry runtime should remain loaded")
            .metadata_retention_controller();

        timeout(Duration::from_secs(5), async {
            while controller.state_inspection_count_for_testing() == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("initial retention state inspection should finish");
        let initial_inspections = controller.state_inspection_count_for_testing();

        controller.record_failure(
            &Error::Internal("injected state inspection failure".to_string()),
            Duration::ZERO,
            true,
        );
        monotonic_clock.advance(Duration::from_secs(1));
        controller.wake_for_testing();
        timeout(Duration::from_secs(5), async {
            while controller.state_inspection_count_for_testing() == initial_inspections {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("eligible retry should inspect retention state once");
        let inspections_after_retry = controller.state_inspection_count_for_testing();
        assert_eq!(inspections_after_retry, initial_inspections + 1);

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            controller.state_inspection_count_for_testing(),
            inspections_after_retry,
            "a successful ineligible retry must wait for the periodic delay instead of busy-looping"
        );
        let diagnostics = controller.snapshot();
        assert_eq!(diagnostics.next_retry_in_millis, None);
        assert_eq!(
            diagnostics.last_failure.as_deref(),
            Some("internal error: injected state inspection failure"),
            "clearing a recovered retry deadline must retain the last failure for diagnostics"
        );
        engine.quiesce().await;
    }

    #[tokio::test]
    async fn below_threshold_mutation_does_not_wait_for_retention_finalize() {
        let faults = BlockingFaultInjector::new(FaultPoint::RetentionCheckpointBeforeCommit);
        let (_data_dir, engine, tenant_id) = engine_with_profile(
            MetadataRetentionProfile::bounded(1_000, 1_000, 1_000, 1_000, 10)
                .expect("below-threshold profile should build"),
            Arc::new(ManualMonotonicClock::new()),
            faults.clone(),
        )
        .await;
        timeout(
            Duration::from_secs(2),
            insert_documents(&engine, &tenant_id, 1),
        )
        .await
        .expect("below-threshold mutation must not wait for retention");
        assert!(
            timeout(Duration::from_millis(100), faults.wait_until_entered())
                .await
                .is_err(),
            "below-threshold maintenance must not enter the finalize transaction"
        );
        engine.quiesce().await;
    }

    #[tokio::test]
    async fn eligible_checkpoint_preparation_does_not_hold_the_committer_route() {
        let faults = BlockingFaultInjector::new(FaultPoint::RetentionCheckpointAfterPrepare);
        let (_data_dir, engine, tenant_id) = engine_with_profile(
            MetadataRetentionProfile::bounded(2, 2, 2, 2, 2)
                .expect("prepare-off-route profile should build"),
            Arc::new(ManualMonotonicClock::new()),
            faults.clone(),
        )
        .await;
        insert_documents(&engine, &tenant_id, 4).await;
        faults.wait_until_entered().await;

        timeout(
            Duration::from_secs(2),
            insert_documents(&engine, &tenant_id, 1),
        )
        .await
        .expect("checkpoint preparation must not occupy the committer route");
        faults.release();
        wait_for_diagnostics(&engine, &tenant_id, |snapshot| snapshot.success_count == 1).await;
        engine.quiesce().await;
    }

    #[tokio::test]
    async fn quiesce_waits_for_running_maintenance_to_drain() {
        let faults = BlockingFaultInjector::new(FaultPoint::RetentionCheckpointBeforeCommit);
        let (_data_dir, engine, tenant_id) = engine_with_profile(
            MetadataRetentionProfile::bounded(1_000, 1_000, 1_000, 1_000, 10)
                .expect("shutdown profile should build"),
            Arc::new(ManualMonotonicClock::new()),
            faults.clone(),
        )
        .await;
        insert_documents(&engine, &tenant_id, 1).await;
        let manual = tokio::spawn({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            async move { engine.run_metadata_retention_now(tenant_id).await }
        });
        faults.wait_until_entered().await;
        let mut quiesce = tokio::spawn({
            let engine = engine.clone();
            async move { engine.quiesce().await }
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(
            !quiesce.is_finished(),
            "engine quiesce must drain an accepted retention transaction"
        );
        faults.release();
        timeout(Duration::from_secs(5), &mut quiesce)
            .await
            .expect("quiesce should finish after retention releases")
            .expect("quiesce task should join");
        let _ = manual.await;
    }

    #[tokio::test]
    async fn shutdown_rejects_late_manual_requests_without_waiting() {
        let (_data_dir, engine, tenant_id) = engine_with_profile(
            MetadataRetentionProfile::shipped(),
            Arc::new(ManualMonotonicClock::new()),
            Arc::new(NoopFaultInjector),
        )
        .await;
        let controller = engine
            .tenant_runtime_for_testing(&tenant_id)
            .expect("retention runtime should remain loaded")
            .metadata_retention_controller();
        engine.quiesce().await;

        let result = timeout(Duration::from_secs(1), controller.request_run())
            .await
            .expect("late manual request should not wait");
        assert!(result.is_err());
    }
}
