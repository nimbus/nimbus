use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use nimbus_core::{DependencySet, MutationCap, TenantId};
use serde::Serialize;
use tracing::warn;

const DEFAULT_COMMIT_TRACE_THRESHOLD_MS: u64 = 500;
// Ordinary point reads and indexed queries have dependency sets many orders of
// magnitude smaller than this. One thousand is intentionally generous: it
// avoids noise while flagging commits whose conflict validation and future
// in-memory-window footprint deserve investigation.
const DEFAULT_WIDE_READ_SET_WARN_THRESHOLD: usize = 1_000;
const DEFAULT_OVERLOAD_ERROR_REPORT_EVERY: usize = 100;
const DEFAULT_SHADOW_CAP_REPORT_EVERY: usize = 100;

/// Cumulative per-tenant committer observations.
///
/// Phase durations are wall-clock nanoseconds summed once per committer sample.
/// A journal batch is one sample and can contain multiple commits; direct and
/// execution-unit commits are one sample each. `queue_wait_nanos` is summed per
/// committed request on the journal path and records committer-inbox wait on
/// the direct and execution-unit paths. `shadow_window_size` is the cumulative
/// number of recent commit entries examined by paths A and B;
/// `shadow_window_truncated_total` counts observations whose scan was clamped
/// to the trailing `NIMBUS_SHADOW_CONFLICT_WINDOW_MAX` commits (conflicts
/// older than the clamp are not counted — nonzero truncation means the
/// conflict totals are a lower bound). `journal_batch_size_sum /
/// journal_batch_count` is the effective journal batch size, recorded for the
/// exact record slice passed to the journal path's single durable append.
/// `commit_count / sample_count` remains the blended average across journal,
/// direct, and execution-unit paths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CommitPhaseMetricsSnapshot {
    pub sample_count: u64,
    pub commit_count: u64,
    pub journal_batch_size_sum: u64,
    pub journal_batch_count: u64,
    pub queue_wait_nanos: u64,
    pub prepare_nanos: u64,
    pub conflict_check_nanos: u64,
    pub durable_append_nanos: u64,
    pub apply_nanos: u64,
    pub publish_nanos: u64,
    pub total_commit_nanos: u64,
    pub shadow_conflict_total: u64,
    pub shadow_window_size: u64,
    pub shadow_window_truncated_total: u64,
    pub shadow_checks_sampled: u64,
    pub mutation_conflict_retries_total: u64,
    pub mutation_conflict_exhausted_total: u64,
    pub prepared_payload_bytes_current: u64,
    pub prepared_payload_bytes_peak: u64,
    pub prepared_payload_bytes_total: u64,
    pub reprepare_total: u64,
    pub overload_errors_total: u64,
    pub overload_errors_reported_total: u64,
    /// Point-in-time number of accepted messages waiting in the bounded
    /// per-tenant committer inbox.
    pub committer_inbox_depth: u64,
    /// Cumulative sends rejected after waiting for the committer inbox.
    pub committer_send_timeout_total: u64,
    pub shadow_cap_read_bytes_total: u64,
    pub shadow_cap_write_bytes_total: u64,
    pub shadow_cap_documents_scanned_total: u64,
    pub shadow_cap_documents_written_total: u64,
    pub shadow_cap_index_range_calls_total: u64,
    pub shadow_cap_logs_total: u64,
}

pub(crate) struct CommitPhaseMetrics {
    sample_count: AtomicU64,
    commit_count: AtomicU64,
    journal_batch_size_sum: AtomicU64,
    journal_batch_count: AtomicU64,
    queue_wait_nanos: AtomicU64,
    prepare_nanos: AtomicU64,
    conflict_check_nanos: AtomicU64,
    durable_append_nanos: AtomicU64,
    apply_nanos: AtomicU64,
    publish_nanos: AtomicU64,
    total_commit_nanos: AtomicU64,
    shadow_conflict_total: AtomicU64,
    shadow_window_size: AtomicU64,
    shadow_window_truncated_total: AtomicU64,
    shadow_checks_sampled: AtomicU64,
    shadow_sample_ticks: AtomicU64,
    mutation_conflict_retries_total: AtomicU64,
    mutation_conflict_exhausted_total: AtomicU64,
    prepared_payload_bytes_current: AtomicU64,
    prepared_payload_bytes_peak: AtomicU64,
    prepared_payload_bytes_total: AtomicU64,
    reprepare_total: AtomicU64,
    overload_errors_total: AtomicU64,
    overload_errors_reported_total: AtomicU64,
    overload_error_report_ticks: AtomicU64,
    shadow_cap_violations: [AtomicU64; 5],
    shadow_cap_logs_total: AtomicU64,
    shadow_cap_report_ticks: AtomicU64,
}

impl CommitPhaseMetrics {
    pub(crate) fn new() -> Self {
        Self {
            sample_count: AtomicU64::new(0),
            commit_count: AtomicU64::new(0),
            journal_batch_size_sum: AtomicU64::new(0),
            journal_batch_count: AtomicU64::new(0),
            queue_wait_nanos: AtomicU64::new(0),
            prepare_nanos: AtomicU64::new(0),
            conflict_check_nanos: AtomicU64::new(0),
            durable_append_nanos: AtomicU64::new(0),
            apply_nanos: AtomicU64::new(0),
            publish_nanos: AtomicU64::new(0),
            total_commit_nanos: AtomicU64::new(0),
            shadow_conflict_total: AtomicU64::new(0),
            shadow_window_size: AtomicU64::new(0),
            shadow_window_truncated_total: AtomicU64::new(0),
            shadow_checks_sampled: AtomicU64::new(0),
            shadow_sample_ticks: AtomicU64::new(0),
            mutation_conflict_retries_total: AtomicU64::new(0),
            mutation_conflict_exhausted_total: AtomicU64::new(0),
            prepared_payload_bytes_current: AtomicU64::new(0),
            prepared_payload_bytes_peak: AtomicU64::new(0),
            prepared_payload_bytes_total: AtomicU64::new(0),
            reprepare_total: AtomicU64::new(0),
            overload_errors_total: AtomicU64::new(0),
            overload_errors_reported_total: AtomicU64::new(0),
            overload_error_report_ticks: AtomicU64::new(0),
            shadow_cap_violations: std::array::from_fn(|_| AtomicU64::new(0)),
            shadow_cap_logs_total: AtomicU64::new(0),
            shadow_cap_report_ticks: AtomicU64::new(0),
        }
    }

    /// Deterministic shadow-observation sampler: the first eligible
    /// observation is always taken, then every `every`-th after it. Ticks
    /// advance only for eligible (non-empty) observations so sparse
    /// workloads still produce data.
    pub(crate) fn shadow_sample_tick(&self, every: usize) -> bool {
        let tick = self.shadow_sample_ticks.fetch_add(1, Ordering::Relaxed);
        every <= 1 || tick.is_multiple_of(every as u64)
    }

    pub(crate) fn record_sample(
        &self,
        commit_count: u64,
        phases: CommitPhaseDurations,
        total: Duration,
    ) {
        self.sample_count.fetch_add(1, Ordering::Relaxed);
        self.commit_count.fetch_add(commit_count, Ordering::Relaxed);
        self.queue_wait_nanos
            .fetch_add(duration_nanos(phases.queue_wait), Ordering::Relaxed);
        self.prepare_nanos
            .fetch_add(duration_nanos(phases.prepare), Ordering::Relaxed);
        self.conflict_check_nanos
            .fetch_add(duration_nanos(phases.conflict_check), Ordering::Relaxed);
        self.durable_append_nanos
            .fetch_add(duration_nanos(phases.durable_append), Ordering::Relaxed);
        self.apply_nanos
            .fetch_add(duration_nanos(phases.apply), Ordering::Relaxed);
        self.publish_nanos
            .fetch_add(duration_nanos(phases.publish), Ordering::Relaxed);
        self.total_commit_nanos
            .fetch_add(duration_nanos(total), Ordering::Relaxed);
    }

    pub(crate) fn record_journal_batch(&self, batch_size: u64) {
        self.journal_batch_size_sum
            .fetch_add(batch_size, Ordering::Relaxed);
        self.journal_batch_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_shadow_check(
        &self,
        window_size: usize,
        conflicting: bool,
        truncated: bool,
    ) {
        self.shadow_window_size.fetch_add(
            u64::try_from(window_size).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        if conflicting {
            self.shadow_conflict_total.fetch_add(1, Ordering::Relaxed);
        }
        if truncated {
            self.shadow_window_truncated_total
                .fetch_add(1, Ordering::Relaxed);
        }
        self.shadow_checks_sampled.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_mutation_conflict_retry(&self) {
        self.mutation_conflict_retries_total
            .fetch_add(1, Ordering::Relaxed);
        self.reprepare_total.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn accept_prepared_payload(&self, bytes: u64) {
        let current = self
            .prepared_payload_bytes_current
            .fetch_add(bytes, Ordering::AcqRel)
            .saturating_add(bytes);
        self.prepared_payload_bytes_peak
            .fetch_max(current, Ordering::Relaxed);
        self.prepared_payload_bytes_total
            .fetch_add(bytes, Ordering::Relaxed);
    }

    pub(crate) fn release_prepared_payload(&self, bytes: u64) {
        self.prepared_payload_bytes_current
            .fetch_sub(bytes, Ordering::AcqRel);
    }

    pub(crate) fn record_mutation_conflict_exhausted(&self) {
        self.mutation_conflict_exhausted_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Records every overload-class error and deterministically selects only
    /// the first and each configured Nth successor for reporting.
    pub(crate) fn record_overload_error(&self) -> bool {
        let every = env_positive_usize(
            "NIMBUS_OVERLOAD_ERROR_REPORT_EVERY",
            DEFAULT_OVERLOAD_ERROR_REPORT_EVERY,
        );
        self.record_overload_error_with_sample_rate(every)
    }

    fn record_overload_error_with_sample_rate(&self, every: usize) -> bool {
        self.overload_errors_total.fetch_add(1, Ordering::Relaxed);
        let tick = self
            .overload_error_report_ticks
            .fetch_add(1, Ordering::Relaxed);
        let sampled = every <= 1 || tick.is_multiple_of(every as u64);
        if sampled {
            self.overload_errors_reported_total
                .fetch_add(1, Ordering::Relaxed);
        }
        sampled
    }

    pub(crate) fn record_shadow_cap_violation(&self, cap: MutationCap) -> bool {
        self.shadow_cap_violations[cap_index(cap)].fetch_add(1, Ordering::Relaxed);
        let every = env_positive_usize(
            "NIMBUS_SHADOW_CAP_REPORT_EVERY",
            DEFAULT_SHADOW_CAP_REPORT_EVERY,
        );
        let tick = self.shadow_cap_report_ticks.fetch_add(1, Ordering::Relaxed);
        let sampled = every <= 1 || tick.is_multiple_of(every as u64);
        if sampled {
            self.shadow_cap_logs_total.fetch_add(1, Ordering::Relaxed);
        }
        sampled
    }

    #[cfg(test)]
    pub(crate) fn shadow_cap_violations(&self, cap: MutationCap) -> u64 {
        self.shadow_cap_violations[cap_index(cap)].load(Ordering::Relaxed)
    }

    pub(crate) fn snapshot(&self) -> CommitPhaseMetricsSnapshot {
        CommitPhaseMetricsSnapshot {
            sample_count: self.sample_count.load(Ordering::Relaxed),
            commit_count: self.commit_count.load(Ordering::Relaxed),
            journal_batch_size_sum: self.journal_batch_size_sum.load(Ordering::Relaxed),
            journal_batch_count: self.journal_batch_count.load(Ordering::Relaxed),
            queue_wait_nanos: self.queue_wait_nanos.load(Ordering::Relaxed),
            prepare_nanos: self.prepare_nanos.load(Ordering::Relaxed),
            conflict_check_nanos: self.conflict_check_nanos.load(Ordering::Relaxed),
            durable_append_nanos: self.durable_append_nanos.load(Ordering::Relaxed),
            apply_nanos: self.apply_nanos.load(Ordering::Relaxed),
            publish_nanos: self.publish_nanos.load(Ordering::Relaxed),
            total_commit_nanos: self.total_commit_nanos.load(Ordering::Relaxed),
            shadow_conflict_total: self.shadow_conflict_total.load(Ordering::Relaxed),
            shadow_window_size: self.shadow_window_size.load(Ordering::Relaxed),
            shadow_window_truncated_total: self
                .shadow_window_truncated_total
                .load(Ordering::Relaxed),
            shadow_checks_sampled: self.shadow_checks_sampled.load(Ordering::Relaxed),
            mutation_conflict_retries_total: self
                .mutation_conflict_retries_total
                .load(Ordering::Relaxed),
            mutation_conflict_exhausted_total: self
                .mutation_conflict_exhausted_total
                .load(Ordering::Relaxed),
            prepared_payload_bytes_current: self
                .prepared_payload_bytes_current
                .load(Ordering::Relaxed),
            prepared_payload_bytes_peak: self.prepared_payload_bytes_peak.load(Ordering::Relaxed),
            prepared_payload_bytes_total: self.prepared_payload_bytes_total.load(Ordering::Relaxed),
            reprepare_total: self.reprepare_total.load(Ordering::Relaxed),
            overload_errors_total: self.overload_errors_total.load(Ordering::Relaxed),
            overload_errors_reported_total: self
                .overload_errors_reported_total
                .load(Ordering::Relaxed),
            committer_inbox_depth: 0,
            committer_send_timeout_total: 0,
            shadow_cap_read_bytes_total: self.shadow_cap_violations[0].load(Ordering::Relaxed),
            shadow_cap_write_bytes_total: self.shadow_cap_violations[1].load(Ordering::Relaxed),
            shadow_cap_documents_scanned_total: self.shadow_cap_violations[2]
                .load(Ordering::Relaxed),
            shadow_cap_documents_written_total: self.shadow_cap_violations[3]
                .load(Ordering::Relaxed),
            shadow_cap_index_range_calls_total: self.shadow_cap_violations[4]
                .load(Ordering::Relaxed),
            shadow_cap_logs_total: self.shadow_cap_logs_total.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CommitPhaseDurations {
    pub(crate) queue_wait: Duration,
    pub(crate) prepare: Duration,
    pub(crate) conflict_check: Duration,
    pub(crate) durable_append: Duration,
    pub(crate) apply: Duration,
    pub(crate) publish: Duration,
}

impl CommitPhaseDurations {
    pub(crate) fn add_queue_wait(&mut self, elapsed: Duration) {
        self.queue_wait = self.queue_wait.saturating_add(elapsed);
    }

    pub(crate) fn add_prepare(&mut self, elapsed: Duration) {
        self.prepare = self.prepare.saturating_add(elapsed);
    }

    pub(crate) fn add_conflict_check(&mut self, elapsed: Duration) {
        self.conflict_check = self.conflict_check.saturating_add(elapsed);
    }

    pub(crate) fn add_publish(&mut self, elapsed: Duration) {
        self.publish = self.publish.saturating_add(elapsed);
    }
}

pub(crate) struct CommitTraceSample<'a> {
    pub(crate) tenant_id: &'a TenantId,
    pub(crate) path: &'static str,
    pub(crate) commit_count: u64,
    pub(crate) phases: CommitPhaseDurations,
    pub(crate) total: Duration,
}

pub(crate) fn maybe_emit_commit_trace(sample: CommitTraceSample<'_>) {
    let Some(threshold) = commit_trace_threshold() else {
        return;
    };
    if let Some(line) = commit_trace_line(&sample, threshold) {
        eprintln!("{line}");
    }
}

pub(in crate::engine) fn maybe_warn_wide_read_set(
    tenant_id: &TenantId,
    dependencies: &DependencySet,
) {
    let cardinality = dependency_cardinality(dependencies);
    let threshold = env_positive_usize(
        "NIMBUS_WIDE_READ_SET_WARN_THRESHOLD",
        DEFAULT_WIDE_READ_SET_WARN_THRESHOLD,
    );
    if cardinality > threshold {
        warn!(
            tenant = %tenant_id,
            cardinality,
            threshold,
            "wide mutation read set exceeds warning threshold"
        );
    }
}

fn commit_trace_threshold() -> Option<Duration> {
    std::env::var_os("NIMBUS_COMMIT_TRACE_THRESHOLD_MS")?;
    let threshold_ms = std::env::var("NIMBUS_COMMIT_TRACE_THRESHOLD_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_COMMIT_TRACE_THRESHOLD_MS);
    Some(Duration::from_millis(threshold_ms))
}

fn commit_trace_line(sample: &CommitTraceSample<'_>, threshold: Duration) -> Option<String> {
    if sample.total <= threshold {
        return None;
    }
    Some(format!(
        "commit-trace tenant={} path={} commits={} queue_wait={:?} prepare={:?} conflict_check={:?} durable_append={:?} apply={:?} publish={:?} total={:?}",
        sample.tenant_id,
        sample.path,
        sample.commit_count,
        sample.phases.queue_wait,
        sample.phases.prepare,
        sample.phases.conflict_check,
        sample.phases.durable_append,
        sample.phases.apply,
        sample.phases.publish,
        sample.total,
    ))
}

fn dependency_cardinality(dependencies: &DependencySet) -> usize {
    dependencies
        .tables
        .len()
        .saturating_add(dependencies.documents.len())
        .saturating_add(dependencies.index_ranges.len())
        .saturating_add(dependencies.predicates.len())
        .saturating_add(dependencies.paginated_windows.len())
}

fn cap_index(cap: MutationCap) -> usize {
    match cap {
        MutationCap::ReadBytes => 0,
        MutationCap::WriteBytes => 1,
        MutationCap::DocumentsScanned => 2,
        MutationCap::DocumentsWritten => 3,
        MutationCap::IndexRangeCalls => 4,
    }
}

pub(super) fn env_positive_usize(key: &str, default: usize) -> usize {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn duration_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_phase_metrics_snapshot_accumulates_samples_and_shadow_observations() {
        let metrics = CommitPhaseMetrics::new();
        metrics.record_sample(
            3,
            CommitPhaseDurations {
                queue_wait: Duration::from_nanos(11),
                prepare: Duration::from_nanos(12),
                conflict_check: Duration::from_nanos(13),
                durable_append: Duration::from_nanos(14),
                apply: Duration::from_nanos(15),
                publish: Duration::from_nanos(16),
            },
            Duration::from_nanos(81),
        );
        metrics.record_journal_batch(3);
        metrics.record_shadow_check(4, true, false);
        metrics.record_shadow_check(2, false, true);
        metrics.record_mutation_conflict_retry();
        metrics.record_mutation_conflict_exhausted();

        assert_eq!(
            metrics.snapshot(),
            CommitPhaseMetricsSnapshot {
                sample_count: 1,
                commit_count: 3,
                journal_batch_size_sum: 3,
                journal_batch_count: 1,
                queue_wait_nanos: 11,
                prepare_nanos: 12,
                conflict_check_nanos: 13,
                durable_append_nanos: 14,
                apply_nanos: 15,
                publish_nanos: 16,
                total_commit_nanos: 81,
                shadow_conflict_total: 1,
                shadow_window_size: 6,
                shadow_window_truncated_total: 1,
                shadow_checks_sampled: 2,
                mutation_conflict_retries_total: 1,
                mutation_conflict_exhausted_total: 1,
                prepared_payload_bytes_current: 0,
                prepared_payload_bytes_peak: 0,
                prepared_payload_bytes_total: 0,
                reprepare_total: 1,
                overload_errors_total: 0,
                overload_errors_reported_total: 0,
                committer_inbox_depth: 0,
                committer_send_timeout_total: 0,
                shadow_cap_read_bytes_total: 0,
                shadow_cap_write_bytes_total: 0,
                shadow_cap_documents_scanned_total: 0,
                shadow_cap_documents_written_total: 0,
                shadow_cap_index_range_calls_total: 0,
                shadow_cap_logs_total: 0,
            }
        );
    }

    #[test]
    fn overload_error_reporting_is_first_always_then_one_in_n() {
        let metrics = CommitPhaseMetrics::new();
        let sampled = (0..201)
            .filter(|_| metrics.record_overload_error_with_sample_rate(100))
            .count();

        assert_eq!(sampled, 3, "ticks 0, 100, and 200 should be reported");
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.overload_errors_total, 201);
        assert_eq!(snapshot.overload_errors_reported_total, 3);
    }

    #[test]
    fn commit_trace_line_is_silent_below_threshold_and_complete_above_it() {
        let tenant_id = TenantId::new("trace-tenant").expect("tenant should parse");
        let sample = CommitTraceSample {
            tenant_id: &tenant_id,
            path: "execution-unit",
            commit_count: 1,
            phases: CommitPhaseDurations {
                queue_wait: Duration::from_millis(1),
                prepare: Duration::from_millis(2),
                conflict_check: Duration::from_millis(3),
                durable_append: Duration::from_millis(4),
                apply: Duration::from_millis(5),
                publish: Duration::from_millis(6),
            },
            total: Duration::from_millis(21),
        };

        assert!(commit_trace_line(&sample, Duration::from_millis(21)).is_none());
        let line = commit_trace_line(&sample, Duration::from_millis(20))
            .expect("sample above the threshold should emit");
        for field in [
            "queue_wait=",
            "prepare=",
            "conflict_check=",
            "durable_append=",
            "apply=",
            "publish=",
            "total=",
        ] {
            assert!(line.contains(field), "trace should contain {field}: {line}");
        }
    }
}
