use super::common::{duration_from_nanos_f64, median_f64, student_t_critical_95};
use super::config::{BenchmarkLane, WorkloadKind};
use super::*;

#[derive(Debug, Default)]
pub(super) struct BenchmarkReport {
    pub(super) measurements: Vec<WorkloadMeasurement>,
    pub(super) sqlite_query_plans: Vec<SqliteQueryPlan>,
}

impl BenchmarkReport {
    pub(super) fn extend(&mut self, outcome: WorkloadOutcome) {
        self.measurements.extend(outcome.measurements);
        self.sqlite_query_plans.extend(outcome.sqlite_query_plans);
    }
}

#[derive(Debug, Default)]
pub(super) struct WorkloadOutcome {
    measurements: Vec<WorkloadMeasurement>,
    pub(super) sqlite_query_plans: Vec<SqliteQueryPlan>,
}

impl WorkloadOutcome {
    pub(super) fn push_measurements(
        &mut self,
        workload: WorkloadKind,
        lane: BenchmarkLane,
        operations_per_sample: u64,
        samples: BackendSamples,
    ) {
        self.measurements
            .extend(samples.into_measurements(workload, lane, operations_per_sample));
    }
}

#[derive(Debug, Clone)]
pub(super) struct WorkloadMeasurement {
    pub(super) workload: WorkloadKind,
    pub(super) lane: BenchmarkLane,
    pub(super) backend: EmbeddedProviderKind,
    pub(super) operations_per_sample: u64,
    pub(super) samples: Vec<Duration>,
}

impl WorkloadMeasurement {
    pub(super) fn stats(&self) -> SampleStats {
        SampleStats::from_samples(&self.samples, self.operations_per_sample)
    }
}

#[derive(Debug, Clone)]
pub(super) struct SqliteQueryPlan {
    pub(super) workload: WorkloadKind,
    pub(super) statement: String,
    pub(super) detail_rows: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct SampleStats {
    pub(super) sample_count: usize,
    pub(super) mean_per_operation: Duration,
    pub(super) median_per_operation: Duration,
    pub(super) p95_per_operation: Duration,
    pub(super) stddev_per_operation: Duration,
    pub(super) ci95_low_per_operation: Duration,
    pub(super) ci95_high_per_operation: Duration,
    pub(super) cv_percent: f64,
    pub(super) median_operations_per_second: f64,
}

impl SampleStats {
    fn from_samples(samples: &[Duration], operations_per_sample: u64) -> Self {
        assert!(!samples.is_empty(), "benchmark samples should not be empty");
        let ops = operations_per_sample.max(1) as f64;
        let mut per_operation_nanos = samples
            .iter()
            .map(|sample| sample.as_secs_f64() * 1_000_000_000.0 / ops)
            .collect::<Vec<_>>();
        per_operation_nanos.sort_by(f64::total_cmp);

        let sample_count = per_operation_nanos.len();
        let mean_nanos = per_operation_nanos.iter().sum::<f64>() / sample_count as f64;
        let median_nanos = median_f64(&per_operation_nanos);
        let p95_index = ((sample_count - 1) * 95) / 100;
        let p95_nanos = per_operation_nanos[p95_index];
        let variance = if sample_count > 1 {
            per_operation_nanos
                .iter()
                .map(|sample| (sample - mean_nanos).powi(2))
                .sum::<f64>()
                / (sample_count - 1) as f64
        } else {
            0.0
        };
        let stddev_nanos = variance.sqrt();
        let sem = if sample_count > 1 {
            stddev_nanos / (sample_count as f64).sqrt()
        } else {
            0.0
        };
        let ci_radius = student_t_critical_95(sample_count) * sem;
        let mean_per_operation = duration_from_nanos_f64(mean_nanos);
        let median_per_operation = duration_from_nanos_f64(median_nanos);
        let p95_per_operation = duration_from_nanos_f64(p95_nanos);
        Self {
            sample_count,
            mean_per_operation,
            median_per_operation,
            p95_per_operation,
            stddev_per_operation: duration_from_nanos_f64(stddev_nanos),
            ci95_low_per_operation: duration_from_nanos_f64((mean_nanos - ci_radius).max(0.0)),
            ci95_high_per_operation: duration_from_nanos_f64(mean_nanos + ci_radius),
            cv_percent: if mean_nanos <= f64::EPSILON {
                0.0
            } else {
                (stddev_nanos / mean_nanos) * 100.0
            },
            median_operations_per_second: if median_per_operation.is_zero() {
                f64::INFINITY
            } else {
                median_per_operation.as_secs_f64().recip()
            },
        }
    }
}

#[derive(Clone)]
pub(super) struct BackendPair<T> {
    pub(super) redb: T,
    pub(super) sqlite: T,
}

impl<T> BackendPair<T> {
    pub(super) fn get(&self, backend: EmbeddedProviderKind) -> &T {
        match backend {
            EmbeddedProviderKind::Redb => &self.redb,
            EmbeddedProviderKind::Sqlite => &self.sqlite,
        }
    }
}

#[derive(Default)]
pub(super) struct BackendSamples {
    redb: Vec<Duration>,
    sqlite: Vec<Duration>,
}

impl BackendSamples {
    pub(super) fn push(&mut self, backend: EmbeddedProviderKind, sample: Duration) {
        match backend {
            EmbeddedProviderKind::Redb => self.redb.push(sample),
            EmbeddedProviderKind::Sqlite => self.sqlite.push(sample),
        }
    }

    fn into_measurements(
        self,
        workload: WorkloadKind,
        lane: BenchmarkLane,
        operations_per_sample: u64,
    ) -> Vec<WorkloadMeasurement> {
        vec![
            WorkloadMeasurement {
                workload,
                lane,
                backend: EmbeddedProviderKind::Redb,
                operations_per_sample,
                samples: self.redb,
            },
            WorkloadMeasurement {
                workload,
                lane,
                backend: EmbeddedProviderKind::Sqlite,
                operations_per_sample,
                samples: self.sqlite,
            },
        ]
    }
}
