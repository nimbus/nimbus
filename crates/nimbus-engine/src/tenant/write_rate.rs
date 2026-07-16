use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use nimbus_core::{Error, Result, Timestamp};
use serde::Serialize;
use tracing::warn;

use super::TenantRuntime;

pub(crate) const DEFAULT_PROPOSED_TENANT_WRITE_BYTES_PER_SEC: u64 = 1 << 20;
pub(crate) const DEFAULT_TENANT_WRITE_RATE_WINDOW_MS: u64 = 1_000;
const DEFAULT_TENANT_WRITE_RATE_REPORT_EVERY: u64 = 100;

#[derive(Debug, Clone, Copy)]
struct TenantWriteRateConfig {
    proposed_bytes_per_sec: u64,
    enforced_bytes_per_sec: Option<u64>,
    window_ms: u64,
}

impl TenantWriteRateConfig {
    fn from_env() -> Self {
        Self {
            proposed_bytes_per_sec: env_positive_u64("NIMBUS_PROPOSED_TENANT_WRITE_BYTES_PER_SEC")
                .unwrap_or(DEFAULT_PROPOSED_TENANT_WRITE_BYTES_PER_SEC),
            enforced_bytes_per_sec: env_positive_u64("NIMBUS_TENANT_WRITE_BYTES_PER_SEC"),
            window_ms: env_positive_u64("NIMBUS_TENANT_WRITE_RATE_WINDOW_MS")
                .unwrap_or(DEFAULT_TENANT_WRITE_RATE_WINDOW_MS),
        }
    }

    #[cfg(test)]
    const fn for_tests(
        proposed_bytes_per_sec: u64,
        enforced_bytes_per_sec: Option<u64>,
        window_ms: u64,
    ) -> Self {
        Self {
            proposed_bytes_per_sec,
            enforced_bytes_per_sec,
            window_ms,
        }
    }
}

static TENANT_WRITE_RATE_CONFIG: LazyLock<TenantWriteRateConfig> =
    LazyLock::new(TenantWriteRateConfig::from_env);

#[derive(Debug, Clone, Copy)]
struct WriteEvent {
    timestamp: Timestamp,
    bytes: u64,
}

#[derive(Debug, Default)]
struct SlidingWindow {
    events: VecDeque<WriteEvent>,
    bytes: u64,
}

impl SlidingWindow {
    fn prune(&mut self, now: Timestamp, window_ms: u64) {
        while self
            .events
            .front()
            .is_some_and(|event| event.timestamp.0.saturating_add(window_ms) <= now.0)
        {
            let event = self
                .events
                .pop_front()
                .expect("front event must remain present while pruning");
            self.bytes = self.bytes.saturating_sub(event.bytes);
        }
    }

    fn retry_after(&self, now: Timestamp, bytes: u64, limit: u64, window_ms: u64) -> Duration {
        let projected = self.bytes.saturating_add(bytes);
        let mut bytes_to_expire = projected.saturating_sub(limit);
        for event in &self.events {
            bytes_to_expire = bytes_to_expire.saturating_sub(event.bytes);
            if bytes_to_expire == 0 {
                let retry_at = event.timestamp.0.saturating_add(window_ms);
                return Duration::from_millis(retry_at.saturating_sub(now.0).max(1));
            }
        }
        // A single mutation larger than the limit cannot fit in the current
        // configuration. Return one whole window as a stable backoff hint.
        Duration::from_millis(window_ms)
    }

    fn push(&mut self, now: Timestamp, bytes: u64) {
        self.bytes = self.bytes.saturating_add(bytes);
        if let Some(event) = self.events.back_mut()
            && event.timestamp == now
        {
            event.bytes = event.bytes.saturating_add(bytes);
            return;
        }
        self.events.push_back(WriteEvent {
            timestamp: now,
            bytes,
        });
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct TenantWriteRateStats {
    pub shadow_violations_total: u64,
    pub shadow_logs_total: u64,
    pub rejections_total: u64,
}

pub(super) struct TenantWriteRateLimiter {
    window: Mutex<SlidingWindow>,
    shadow_violations_total: AtomicU64,
    shadow_logs_total: AtomicU64,
    shadow_report_ticks: AtomicU64,
    rejections_total: AtomicU64,
}

impl TenantWriteRateLimiter {
    pub(super) fn new() -> Self {
        Self {
            window: Mutex::new(SlidingWindow::default()),
            shadow_violations_total: AtomicU64::new(0),
            shadow_logs_total: AtomicU64::new(0),
            shadow_report_ticks: AtomicU64::new(0),
            rejections_total: AtomicU64::new(0),
        }
    }

    pub(super) fn stats(&self) -> TenantWriteRateStats {
        TenantWriteRateStats {
            shadow_violations_total: self.shadow_violations_total.load(Ordering::Relaxed),
            shadow_logs_total: self.shadow_logs_total.load(Ordering::Relaxed),
            rejections_total: self.rejections_total.load(Ordering::Relaxed),
        }
    }

    fn check(
        &self,
        tenant: &nimbus_core::TenantId,
        now: Timestamp,
        bytes: u64,
        config: TenantWriteRateConfig,
    ) -> Result<()> {
        if bytes == 0 {
            return Ok(());
        }
        let window_limit = |bytes_per_sec: u64| {
            let scaled = u128::from(bytes_per_sec)
                .saturating_mul(u128::from(config.window_ms))
                .saturating_add(999)
                / 1_000;
            u64::try_from(scaled).unwrap_or(u64::MAX)
        };
        let proposed_limit = window_limit(config.proposed_bytes_per_sec);
        let enforced_limit = config.enforced_bytes_per_sec.map(window_limit);

        let (projected, retry_after) = {
            let mut window = self
                .window
                .lock()
                .expect("tenant write rate window lock should not be poisoned");
            let now = window
                .events
                .back()
                .map_or(now, |event| Timestamp(now.0.max(event.timestamp.0)));
            window.prune(now, config.window_ms);
            let projected = window.bytes.saturating_add(bytes);
            let retry_after = enforced_limit
                .filter(|limit| projected > *limit)
                .map(|limit| window.retry_after(now, bytes, limit, config.window_ms));
            if retry_after.is_none() {
                window.push(now, bytes);
            }
            (projected, retry_after)
        };

        if projected > proposed_limit {
            self.shadow_violations_total.fetch_add(1, Ordering::Relaxed);
            let every = env_positive_u64("NIMBUS_TENANT_WRITE_RATE_REPORT_EVERY")
                .unwrap_or(DEFAULT_TENANT_WRITE_RATE_REPORT_EVERY);
            let tick = self.shadow_report_ticks.fetch_add(1, Ordering::Relaxed);
            if every <= 1 || tick.is_multiple_of(every) {
                self.shadow_logs_total.fetch_add(1, Ordering::Relaxed);
                warn!(
                    tenant = %tenant,
                    observed_bytes = projected,
                    limit_bytes = proposed_limit,
                    window_ms = config.window_ms,
                    "tenant would exceed proposed write rate"
                );
            }
        }

        if let Some(retry_after) = retry_after {
            self.rejections_total.fetch_add(1, Ordering::Relaxed);
            return Err(Error::rate_limited(
                format!(
                    "tenant write rate exceeds {} bytes per second",
                    config.enforced_bytes_per_sec.unwrap_or_default()
                ),
                retry_after,
            ));
        }
        Ok(())
    }
}

impl TenantRuntime {
    pub(crate) fn check_tenant_write_rate(&self, now: Timestamp, bytes: u64) -> Result<()> {
        self.write_rate
            .check(&self.tenant_id, now, bytes, *TENANT_WRITE_RATE_CONFIG)
    }
}

fn env_positive_u64(key: &str) -> Option<u64> {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use nimbus_core::{Retryability, TenantId};
    use nimbus_storage::{Clock, ManualClock};

    use super::*;

    fn tenant() -> TenantId {
        TenantId::new("write-rate-tests").expect("tenant should parse")
    }

    #[test]
    fn hot_workload_shadow_mode_counts_and_samples_without_rejecting() {
        let limiter = TenantWriteRateLimiter::new();
        let config = TenantWriteRateConfig::for_tests(100, None, 1_000);
        let now = Timestamp(10_000);

        limiter
            .check(&tenant(), now, 60, config)
            .expect("first write should be under the proposed rate");
        limiter
            .check(&tenant(), now, 60, config)
            .expect("shadow mode must not reject a burst");
        limiter
            .check(&tenant(), now, 10, config)
            .expect("shadow mode must continue admitting a hot workload");

        assert_eq!(
            limiter.stats(),
            TenantWriteRateStats {
                shadow_violations_total: 2,
                shadow_logs_total: 1,
                rejections_total: 0,
            }
        );
    }

    #[test]
    fn enforced_write_rate_returns_rate_limited_with_retry_after() {
        let limiter = TenantWriteRateLimiter::new();
        let config = TenantWriteRateConfig::for_tests(u64::MAX, Some(100), 1_000);
        let now = Timestamp(20_000);

        limiter
            .check(&tenant(), now, 80, config)
            .expect("under-limit write should be admitted");
        let error = limiter
            .check(&tenant(), Timestamp(20_250), 30, config)
            .expect_err("over-limit write should be rejected");

        assert!(matches!(error, Error::RateLimited { .. }));
        assert_eq!(error.retryability(), Retryability::RetryableAfterBackoff);
        assert_eq!(error.retry_after(), Some(Duration::from_millis(750)));
        assert_eq!(limiter.stats().rejections_total, 1);
    }

    #[test]
    fn under_limit_traffic_is_unaffected() {
        let limiter = TenantWriteRateLimiter::new();
        let config = TenantWriteRateConfig::for_tests(100, Some(100), 1_000);

        limiter
            .check(&tenant(), Timestamp(30_000), 40, config)
            .expect("first write should pass");
        limiter
            .check(&tenant(), Timestamp(30_100), 60, config)
            .expect("traffic exactly at the rate should pass");
        assert_eq!(limiter.stats(), TenantWriteRateStats::default());
    }

    #[test]
    fn sliding_window_uses_manual_clock_and_releases_expired_bytes() {
        let limiter = TenantWriteRateLimiter::new();
        let config = TenantWriteRateConfig::for_tests(u64::MAX, Some(100), 1_000);
        let clock = ManualClock::new(Timestamp(40_000));

        limiter
            .check(&tenant(), clock.now(), 100, config)
            .expect("initial write should pass");
        clock.advance_ms(999);
        assert!(matches!(
            limiter.check(&tenant(), clock.now(), 1, config),
            Err(Error::RateLimited { .. })
        ));
        clock.advance_ms(1);
        limiter
            .check(&tenant(), clock.now(), 100, config)
            .expect("expired bytes should leave the sliding window");
    }
}
