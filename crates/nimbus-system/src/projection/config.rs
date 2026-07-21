use std::time::Duration;

pub(super) const TABLE_PROJECTION_OBSERVER: &str = "nimbus-system-table-projection";
pub(super) const PROJECTION_TENANT_SWEEP_INTERVAL: usize = 1_024;
pub(super) const PROJECTION_RETRY_BASE_BACKOFF: Duration = Duration::from_millis(50);
pub(super) const PROJECTION_RETRY_MAX_BACKOFF: Duration = Duration::from_secs(5);

const DEFAULT_PROJECTION_WORK_CAPACITY: usize = 1_024;
const DEFAULT_PROJECTION_WORK_HIGH_WATERMARK: usize = 768;
const DEFAULT_PROJECTION_AGGREGATE_WORK_CAPACITY: usize = 8_192;
const DEFAULT_PROJECTION_AGGREGATE_WORK_HIGH_WATERMARK: usize = 6_144;

pub(super) struct ProjectionWorkLimits {
    pub capacity: usize,
    pub high_watermark: usize,
    pub aggregate_capacity: usize,
    pub aggregate_high_watermark: usize,
}

impl ProjectionWorkLimits {
    pub fn from_env() -> Self {
        Self {
            capacity: env_positive_usize(
                "NIMBUS_SYSTEM_PROJECTION_WORK_CAPACITY",
                DEFAULT_PROJECTION_WORK_CAPACITY,
            ),
            high_watermark: env_positive_usize(
                "NIMBUS_SYSTEM_PROJECTION_WORK_HIGH_WATERMARK",
                DEFAULT_PROJECTION_WORK_HIGH_WATERMARK,
            ),
            aggregate_capacity: env_positive_usize(
                "NIMBUS_SYSTEM_PROJECTION_AGGREGATE_WORK_CAPACITY",
                DEFAULT_PROJECTION_AGGREGATE_WORK_CAPACITY,
            ),
            aggregate_high_watermark: env_positive_usize(
                "NIMBUS_SYSTEM_PROJECTION_AGGREGATE_WORK_HIGH_WATERMARK",
                DEFAULT_PROJECTION_AGGREGATE_WORK_HIGH_WATERMARK,
            ),
        }
    }
}

fn env_positive_usize(key: &str, default: usize) -> usize {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}
