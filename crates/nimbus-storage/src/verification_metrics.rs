use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;

/// The closed set of verification execution modes used for metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterializedVerificationMetricMode {
    FullScrub,
    Incremental,
}

impl MaterializedVerificationMetricMode {
    pub const ALL: [Self; 2] = [Self::FullScrub, Self::Incremental];

    pub const fn label(self) -> &'static str {
        match self {
            Self::FullScrub => "full_scrub",
            Self::Incremental => "incremental",
        }
    }
}

/// One completed verification observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterializedVerificationObservation {
    pub mode: MaterializedVerificationMetricMode,
    pub duration: Duration,
    pub verified_leaves: usize,
    pub rebuilt: bool,
    pub mismatch_count: usize,
}

/// Fixed-shape process metrics for bounded materialized verification.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct MaterializedVerificationMetricsSnapshot {
    pub full_scrub_total: u64,
    pub incremental_total: u64,
    pub full_scrub_duration_nanos_total: u64,
    pub incremental_duration_nanos_total: u64,
    pub resident_index_bytes_current: u64,
    pub resident_index_bytes_peak: u64,
    pub verified_leaves_total: u64,
    pub rebuild_total: u64,
    pub mismatch_total: u64,
    pub sessions_current: u64,
    pub evictions_total: u64,
}

/// Lock-free counters owned by one verification-session registry.
#[derive(Debug, Default)]
pub struct MaterializedVerificationMetrics {
    full_scrub_total: AtomicU64,
    incremental_total: AtomicU64,
    full_scrub_duration_nanos_total: AtomicU64,
    incremental_duration_nanos_total: AtomicU64,
    resident_index_bytes_current: AtomicU64,
    resident_index_bytes_peak: AtomicU64,
    verified_leaves_total: AtomicU64,
    rebuild_total: AtomicU64,
    mismatch_total: AtomicU64,
    sessions_current: AtomicU64,
    evictions_total: AtomicU64,
}

impl MaterializedVerificationMetrics {
    pub fn record(&self, observation: MaterializedVerificationObservation) {
        match observation.mode {
            MaterializedVerificationMetricMode::FullScrub => {
                self.full_scrub_total.fetch_add(1, Ordering::Relaxed);
                self.full_scrub_duration_nanos_total.fetch_add(
                    u64::try_from(observation.duration.as_nanos()).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
            }
            MaterializedVerificationMetricMode::Incremental => {
                self.incremental_total.fetch_add(1, Ordering::Relaxed);
                self.incremental_duration_nanos_total.fetch_add(
                    u64::try_from(observation.duration.as_nanos()).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
            }
        }
        self.verified_leaves_total.fetch_add(
            u64::try_from(observation.verified_leaves).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        if observation.rebuilt {
            self.rebuild_total.fetch_add(1, Ordering::Relaxed);
        }
        self.mismatch_total.fetch_add(
            u64::try_from(observation.mismatch_count).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }

    pub fn set_registry_usage(&self, sessions: usize, resident_index_bytes: usize) {
        self.sessions_current.store(
            u64::try_from(sessions).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        self.set_resident_index_bytes(resident_index_bytes);
    }

    pub fn record_evictions(&self, count: usize) {
        self.evictions_total
            .fetch_add(u64::try_from(count).unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MaterializedVerificationMetricsSnapshot {
        MaterializedVerificationMetricsSnapshot {
            full_scrub_total: self.full_scrub_total.load(Ordering::Relaxed),
            incremental_total: self.incremental_total.load(Ordering::Relaxed),
            full_scrub_duration_nanos_total: self
                .full_scrub_duration_nanos_total
                .load(Ordering::Relaxed),
            incremental_duration_nanos_total: self
                .incremental_duration_nanos_total
                .load(Ordering::Relaxed),
            resident_index_bytes_current: self.resident_index_bytes_current.load(Ordering::Relaxed),
            resident_index_bytes_peak: self.resident_index_bytes_peak.load(Ordering::Relaxed),
            verified_leaves_total: self.verified_leaves_total.load(Ordering::Relaxed),
            rebuild_total: self.rebuild_total.load(Ordering::Relaxed),
            mismatch_total: self.mismatch_total.load(Ordering::Relaxed),
            sessions_current: self.sessions_current.load(Ordering::Relaxed),
            evictions_total: self.evictions_total.load(Ordering::Relaxed),
        }
    }

    fn set_resident_index_bytes(&self, bytes: usize) {
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        self.resident_index_bytes_current
            .store(bytes, Ordering::Relaxed);
        self.resident_index_bytes_peak
            .fetch_max(bytes, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_metrics_have_bounded_labels() {
        assert_eq!(
            MaterializedVerificationMetricMode::ALL.map(|mode| mode.label()),
            ["full_scrub", "incremental"]
        );

        let metrics = MaterializedVerificationMetrics::default();
        for mode in MaterializedVerificationMetricMode::ALL {
            metrics.record(MaterializedVerificationObservation {
                mode,
                duration: Duration::from_millis(2),
                verified_leaves: 3,
                rebuilt: mode == MaterializedVerificationMetricMode::FullScrub,
                mismatch_count: 0,
            });
        }
        metrics.set_registry_usage(1, 128);
        metrics.record_evictions(1);

        let value = serde_json::to_value(metrics.snapshot()).expect("metrics should serialize");
        let fields = value
            .as_object()
            .expect("metrics must have a fixed object shape");
        assert_eq!(fields.len(), 11);
        assert!(!fields.keys().any(|key| key.contains("tenant")));
        assert!(fields.values().all(serde_json::Value::is_u64));
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.full_scrub_duration_nanos_total, 2_000_000);
        assert_eq!(snapshot.incremental_duration_nanos_total, 2_000_000);
    }
}
