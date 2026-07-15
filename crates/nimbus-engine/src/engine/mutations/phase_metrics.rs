use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use nimbus_core::{DependencySet, TenantId};
use serde::Serialize;
use tracing::warn;

const DEFAULT_COMMIT_TRACE_THRESHOLD_MS: u64 = 500;
// Ordinary point reads and indexed queries have dependency sets many orders of
// magnitude smaller than this. One thousand is intentionally generous: it
// avoids noise while flagging commits whose conflict validation and future
// in-memory-window footprint deserve investigation.
const DEFAULT_WIDE_READ_SET_WARN_THRESHOLD: usize = 1_000;

/// Cumulative per-tenant committer observations.
///
/// Phase durations are wall-clock nanoseconds summed once per committer sample.
/// A journal batch is one sample and can contain multiple commits; direct and
/// execution-unit commits are one sample each. `queue_wait_nanos` is summed per
/// committed request on the journal path and records sequence-gate wait on the
/// direct and execution-unit paths. `shadow_window_size` is the cumulative
/// number of recent commit entries examined by paths A and B;
/// `shadow_window_truncated_total` counts observations whose scan was clamped
/// to the trailing `NIMBUS_SHADOW_CONFLICT_WINDOW_MAX` commits (conflicts
/// older than the clamp are not counted — nonzero truncation means the
/// conflict totals are a lower bound).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CommitPhaseMetricsSnapshot {
    pub sample_count: u64,
    pub commit_count: u64,
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
}

pub(crate) struct CommitPhaseMetrics {
    sample_count: AtomicU64,
    commit_count: AtomicU64,
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
}

impl CommitPhaseMetrics {
    pub(crate) fn new() -> Self {
        Self {
            sample_count: AtomicU64::new(0),
            commit_count: AtomicU64::new(0),
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
        }
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
    }

    pub(crate) fn snapshot(&self) -> CommitPhaseMetricsSnapshot {
        CommitPhaseMetricsSnapshot {
            sample_count: self.sample_count.load(Ordering::Relaxed),
            commit_count: self.commit_count.load(Ordering::Relaxed),
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

pub(super) fn env_positive_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
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
        metrics.record_shadow_check(4, true, false);
        metrics.record_shadow_check(2, false, true);

        assert_eq!(
            metrics.snapshot(),
            CommitPhaseMetricsSnapshot {
                sample_count: 1,
                commit_count: 3,
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
            }
        );
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
