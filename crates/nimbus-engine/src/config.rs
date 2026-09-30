//! Typed engine configuration.
//!
//! [`EngineConfig::from_env`] is the only runtime environment parse in this
//! crate. A composition root calls it once and passes the value through
//! [`crate::EnginePersistenceConfig`]. The engine hands it to every tenant
//! runtime. Tests build the struct directly and never set process variables.
//!
//! This module owns the tuning knobs that are plain numbers. A module whose
//! configuration carries its own rules (mutation caps, the tenant write rate,
//! and the write-log window) owns a `from_lookup` parse that this module
//! composes. `EngineTestHarness::from_env` is the only other environment
//! read. It exists only in test builds and serves test-harness inputs that
//! select checkers and replay scenarios but never change engine behavior.
//! `docs/private/operating/engine-storage-config.md` lists every variable.

use std::ffi::OsString;
use std::str::FromStr;
use std::time::Duration;

use nimbus_storage::StorageConfig;

use crate::engine::{MutationCapConfig, WriteLogConfig};
use crate::tenant::TenantWriteRateConfig;

/// Reads one variable by name. `from_env` passes the process environment and
/// tests pass a fixed map.
pub(crate) type EnvLookup<'a> = &'a dyn Fn(&str) -> Option<OsString>;

pub(crate) const COMMITTER_PUBLISHER_BATCH_BASE: usize = 32;
pub(crate) const COMMITTER_PUBLISHER_BATCH_MAX_DEFAULT: usize = 256;
pub(crate) const COMMITTER_PUBLISHER_BATCH_MAX_ENV: &str = "NIMBUS_COMMITTER_PUBLISHER_BATCH_MAX";
pub(crate) const COMMITTER_PUBLISHER_COALESCE_DEFAULT_MICROS: u64 = 750;
pub(crate) const COMMITTER_PUBLISHER_COALESCE_ENV: &str =
    "NIMBUS_COMMITTER_PUBLISHER_COALESCE_MICROS";
pub(crate) const MUTATION_JOURNAL_BATCH_BASE: usize = 32;
pub(crate) const MUTATION_JOURNAL_BATCH_MAX_DEFAULT: usize = 256;
pub(crate) const MUTATION_JOURNAL_BATCH_MAX_ENV: &str = "NIMBUS_MUTATION_JOURNAL_BATCH_MAX";
pub(crate) const MUTATION_JOURNAL_COALESCE_DEFAULT_MICROS: u64 = 0;
pub(crate) const MUTATION_JOURNAL_COALESCE_ENV: &str = "NIMBUS_MUTATION_JOURNAL_COALESCE_MICROS";

const DEFAULT_MUTATION_OCC_MAX_ATTEMPTS: usize = 4;
const DEFAULT_MUTATION_OCC_INITIAL_BACKOFF_MS: u64 = 100;
const DEFAULT_MUTATION_OCC_MAX_BACKOFF_MS: u64 = 2_000;
const DEFAULT_PUBLISHER_RETRY_LIMIT: usize = 4;
const DEFAULT_PUBLISHER_RETRY_INITIAL_MS: u64 = 1;
const DEFAULT_PUBLISHER_RETRY_MAX_MS: u64 = 100;

const DEFAULT_COMMITTER_INBOX_SIZE: usize = 128;
const DEFAULT_COMMITTER_SEND_TIMEOUT_MS: u64 = 500;
const DEFAULT_PUBLISHER_QUEUE_CAPACITY: usize = 32;
const DEFAULT_PUBLISHER_SEND_TIMEOUT_MS: u64 = 500;
const DEFAULT_OBSERVER_QUEUE_CAPACITY: usize = 4_096;
const DEFAULT_OBSERVER_QUEUE_HIGH_WATERMARK: usize = 3_072;
const DEFAULT_TENANT_MUTATION_ISOLATE_CEILING: usize = 16;

/// Upper bound on how many recent commits one shadow observation may scan.
///
/// The observation window opens at the request's enqueue-time snapshot, so
/// under sustained load the un-clamped window grows with queue depth — and
/// because the scan runs on the serial committer, an unbounded scan feeds
/// back into longer gate holds and deeper queues (measured as a collapse
/// from ~16.6k to ~0.6k mut/s at N=256 before this bound existed). The
/// clamp keeps the per-observation cost constant; conflicts older than the
/// window are not counted and the truncation is recorded instead, so the
/// metric stays honest about what it skipped.
const DEFAULT_SHADOW_CONFLICT_WINDOW_MAX: usize = 64;

/// Observe only every N-th eligible batch/mutation. Even a bounded scan is
/// a storage read of full commit entries on the serial committer; at
/// saturation the observation *frequency* — one scan per batch — is itself
/// a material tax (measured ~95% of under-gate time at N=256 with
/// per-request unsampled observation). Shadow metrics exist to
/// characterize workloads, so a deterministic sample is sufficient; the
/// first eligible observation is always taken.
const DEFAULT_SHADOW_CONFLICT_SAMPLE_EVERY: usize = 16;

const DEFAULT_COMMIT_TRACE_THRESHOLD_MS: u64 = 500;
// Ordinary point reads and indexed queries have dependency sets many orders of
// magnitude smaller than this. One thousand is intentionally generous: it
// avoids noise while flagging commits whose conflict validation and future
// in-memory-window footprint deserve investigation.
const DEFAULT_WIDE_READ_SET_WARN_THRESHOLD: usize = 1_000;
const DEFAULT_OVERLOAD_ERROR_REPORT_EVERY: usize = 100;
const DEFAULT_SHADOW_CAP_REPORT_EVERY: usize = 100;

/// Engine settings that a composition root resolves once.
///
/// `Default` is the shipped behavior and equals a parse of an empty
/// environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    pub(crate) mutation_journal_batch: BatchPolicy,
    pub(crate) committer_publisher_batch: BatchPolicy,
    pub(crate) mutation_occ_retry: RetryPolicy,
    pub(crate) publisher_retry: RetryPolicy,
    pub(crate) committer_inbox: QueueLimits,
    pub(crate) publisher_queue: QueueLimits,
    pub(crate) observer_queue: ObserverQueueLimits,
    pub(crate) tenant_mutation_isolate_ceiling: usize,
    /// Prepare permits per tenant. `None` derives the count from host
    /// parallelism.
    pub(crate) prepare_concurrency: Option<usize>,
    pub(crate) write_log: WriteLogConfig,
    pub(crate) shadow_conflicts: ShadowConflictConfig,
    pub(crate) mutation_caps: MutationCapConfig,
    pub(crate) tenant_write_rate: TenantWriteRateConfig,
    pub(crate) diagnostics: EngineDiagnosticsConfig,
    pub(crate) storage: StorageConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self::from_lookup(&|_| None)
    }
}

impl EngineConfig {
    /// Parses every engine and storage variable from the process environment.
    ///
    /// An absent, malformed, or out-of-range value keeps its default.
    pub fn from_env() -> Self {
        Self::from_lookup(&|key| std::env::var_os(key))
    }

    pub(crate) fn from_lookup(lookup: EnvLookup<'_>) -> Self {
        Self {
            mutation_journal_batch: BatchPolicy::from_lookup(
                lookup,
                MUTATION_JOURNAL_BATCH_BASE,
                MUTATION_JOURNAL_BATCH_MAX_ENV,
                MUTATION_JOURNAL_BATCH_MAX_DEFAULT,
                MUTATION_JOURNAL_COALESCE_ENV,
                MUTATION_JOURNAL_COALESCE_DEFAULT_MICROS,
            ),
            committer_publisher_batch: BatchPolicy::from_lookup(
                lookup,
                COMMITTER_PUBLISHER_BATCH_BASE,
                COMMITTER_PUBLISHER_BATCH_MAX_ENV,
                COMMITTER_PUBLISHER_BATCH_MAX_DEFAULT,
                COMMITTER_PUBLISHER_COALESCE_ENV,
                COMMITTER_PUBLISHER_COALESCE_DEFAULT_MICROS,
            ),
            mutation_occ_retry: RetryPolicy::new(
                positive_usize(
                    lookup,
                    "NIMBUS_MUTATION_OCC_MAX_RETRIES",
                    DEFAULT_MUTATION_OCC_MAX_ATTEMPTS,
                ),
                nonnegative_u64(
                    lookup,
                    "NIMBUS_MUTATION_OCC_INITIAL_BACKOFF_MS",
                    DEFAULT_MUTATION_OCC_INITIAL_BACKOFF_MS,
                ),
                nonnegative_u64(
                    lookup,
                    "NIMBUS_MUTATION_OCC_MAX_BACKOFF_MS",
                    DEFAULT_MUTATION_OCC_MAX_BACKOFF_MS,
                ),
            ),
            publisher_retry: RetryPolicy::new(
                positive_usize(
                    lookup,
                    "NIMBUS_COMMITTER_PUBLISHER_RETRY_LIMIT",
                    DEFAULT_PUBLISHER_RETRY_LIMIT,
                ),
                nonnegative_u64(
                    lookup,
                    "NIMBUS_COMMITTER_PUBLISHER_RETRY_INITIAL_MS",
                    DEFAULT_PUBLISHER_RETRY_INITIAL_MS,
                ),
                nonnegative_u64(
                    lookup,
                    "NIMBUS_COMMITTER_PUBLISHER_RETRY_MAX_MS",
                    DEFAULT_PUBLISHER_RETRY_MAX_MS,
                ),
            ),
            committer_inbox: QueueLimits::from_lookup(
                lookup,
                "NIMBUS_COMMITTER_INBOX_SIZE",
                DEFAULT_COMMITTER_INBOX_SIZE,
                "NIMBUS_COMMITTER_SEND_TIMEOUT_MS",
                DEFAULT_COMMITTER_SEND_TIMEOUT_MS,
            ),
            publisher_queue: QueueLimits::from_lookup(
                lookup,
                "NIMBUS_COMMITTER_PUBLISHER_QUEUE_SIZE",
                DEFAULT_PUBLISHER_QUEUE_CAPACITY,
                "NIMBUS_COMMITTER_PUBLISHER_SEND_TIMEOUT_MS",
                DEFAULT_PUBLISHER_SEND_TIMEOUT_MS,
            ),
            observer_queue: ObserverQueueLimits {
                capacity: positive_usize(
                    lookup,
                    "NIMBUS_COMMITTED_OBSERVER_QUEUE_CAPACITY",
                    DEFAULT_OBSERVER_QUEUE_CAPACITY,
                ),
                high_watermark: positive_usize(
                    lookup,
                    "NIMBUS_COMMITTED_OBSERVER_QUEUE_HIGH_WATERMARK",
                    DEFAULT_OBSERVER_QUEUE_HIGH_WATERMARK,
                ),
            },
            tenant_mutation_isolate_ceiling: positive_usize(
                lookup,
                "NIMBUS_TENANT_MUTATION_ISOLATE_CEILING",
                DEFAULT_TENANT_MUTATION_ISOLATE_CEILING,
            ),
            prepare_concurrency: parsed::<usize>(lookup, "NIMBUS_PREPARE_CONCURRENCY")
                .filter(|value| *value > 0),
            write_log: WriteLogConfig::from_lookup(lookup),
            shadow_conflicts: ShadowConflictConfig {
                sample_every: positive_usize(
                    lookup,
                    "NIMBUS_SHADOW_CONFLICT_SAMPLE_EVERY",
                    DEFAULT_SHADOW_CONFLICT_SAMPLE_EVERY,
                ),
                window_max: positive_usize(
                    lookup,
                    "NIMBUS_SHADOW_CONFLICT_WINDOW_MAX",
                    DEFAULT_SHADOW_CONFLICT_WINDOW_MAX,
                ),
            },
            mutation_caps: MutationCapConfig::from_lookup(lookup),
            tenant_write_rate: TenantWriteRateConfig::from_lookup(lookup),
            diagnostics: EngineDiagnosticsConfig::from_lookup(lookup),
            storage: StorageConfig::from_lookup(lookup),
        }
    }

    /// Returns the number of prepare permits for one tenant runtime.
    pub(crate) fn resolved_prepare_concurrency(&self) -> usize {
        self.prepare_concurrency.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(std::num::NonZeroUsize::get)
                .unwrap_or(4)
                // SQLite's read-snapshot pool is deliberately small. Four
                // callers overlap CPU and serialization without turning pool
                // polling into the dominant prepare cost.
                .min(4)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BatchPolicy {
    pub(crate) base: usize,
    pub(crate) max: usize,
    pub(crate) coalesce: Duration,
}

impl BatchPolicy {
    pub(crate) fn new(base: usize, max: usize, coalesce_micros: u64) -> Self {
        Self {
            base,
            max: max.max(base),
            coalesce: Duration::from_micros(coalesce_micros),
        }
    }

    fn from_lookup(
        lookup: EnvLookup<'_>,
        base: usize,
        max_key: &str,
        default_max: usize,
        coalesce_key: &str,
        default_coalesce_micros: u64,
    ) -> Self {
        Self::new(
            base,
            positive_usize(lookup, max_key, default_max),
            nonnegative_u64(lookup, coalesce_key, default_coalesce_micros),
        )
    }
}

/// Bounded exponential backoff: attempt `n` waits `initial * 2^(n-1)`,
/// capped at `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RetryPolicy {
    pub(crate) max_attempts: usize,
    initial_backoff_ms: u64,
    max_backoff_ms: u64,
}

impl RetryPolicy {
    pub(crate) fn new(max_attempts: usize, initial_backoff_ms: u64, max_backoff_ms: u64) -> Self {
        Self {
            max_attempts,
            initial_backoff_ms,
            max_backoff_ms: max_backoff_ms.max(initial_backoff_ms),
        }
    }

    pub(crate) fn backoff(&self, attempt: usize) -> Duration {
        let shift = u32::try_from(attempt.saturating_sub(1))
            .unwrap_or(u32::MAX)
            .min(63);
        Duration::from_millis(
            self.initial_backoff_ms
                .saturating_mul(1u64 << shift)
                .min(self.max_backoff_ms),
        )
    }
}

/// Capacity and send timeout of one bounded tenant queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QueueLimits {
    pub(crate) capacity: usize,
    pub(crate) send_timeout: Duration,
}

impl QueueLimits {
    fn from_lookup(
        lookup: EnvLookup<'_>,
        capacity_key: &str,
        default_capacity: usize,
        send_timeout_key: &str,
        default_send_timeout_ms: u64,
    ) -> Self {
        Self {
            capacity: positive_usize(lookup, capacity_key, default_capacity),
            send_timeout: Duration::from_millis(nonnegative_u64(
                lookup,
                send_timeout_key,
                default_send_timeout_ms,
            )),
        }
    }
}

/// Requested committed-observer queue limits. The observer handoff clamps
/// them against the largest live dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObserverQueueLimits {
    pub(crate) capacity: usize,
    pub(crate) high_watermark: usize,
}

/// Sampling and scan bounds for shadow conflict observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShadowConflictConfig {
    pub(crate) sample_every: usize,
    pub(crate) window_max: usize,
}

/// Operator diagnostics: log sampling rates, warning thresholds, and timing
/// output on standard error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EngineDiagnosticsConfig {
    pub(crate) overload_error_report_every: usize,
    pub(crate) shadow_cap_report_every: usize,
    pub(crate) wide_read_set_warn_threshold: usize,
    /// `None` disables commit traces. A present but malformed variable
    /// selects the default threshold.
    pub(crate) commit_trace_threshold: Option<Duration>,
    pub(crate) query_profile: bool,
    pub(crate) tenant_load_profile: bool,
}

impl EngineDiagnosticsConfig {
    fn from_lookup(lookup: EnvLookup<'_>) -> Self {
        Self {
            overload_error_report_every: positive_usize(
                lookup,
                "NIMBUS_OVERLOAD_ERROR_REPORT_EVERY",
                DEFAULT_OVERLOAD_ERROR_REPORT_EVERY,
            ),
            shadow_cap_report_every: positive_usize(
                lookup,
                "NIMBUS_SHADOW_CAP_REPORT_EVERY",
                DEFAULT_SHADOW_CAP_REPORT_EVERY,
            ),
            wide_read_set_warn_threshold: positive_usize(
                lookup,
                "NIMBUS_WIDE_READ_SET_WARN_THRESHOLD",
                DEFAULT_WIDE_READ_SET_WARN_THRESHOLD,
            ),
            commit_trace_threshold: is_set(lookup, "NIMBUS_COMMIT_TRACE_THRESHOLD_MS").then(|| {
                Duration::from_millis(nonnegative_u64(
                    lookup,
                    "NIMBUS_COMMIT_TRACE_THRESHOLD_MS",
                    DEFAULT_COMMIT_TRACE_THRESHOLD_MS,
                ))
            }),
            query_profile: is_set(lookup, "NIMBUS_QUERY_PROFILE"),
            tenant_load_profile: is_set(lookup, "NIMBUS_TENANT_LOAD_PROFILE"),
        }
    }
}

fn parsed<T: FromStr>(lookup: EnvLookup<'_>, key: &str) -> Option<T> {
    lookup(key)?.into_string().ok()?.trim().parse::<T>().ok()
}

/// Returns a positive integer, or `default` when the value is absent,
/// malformed, or zero.
pub(crate) fn positive_usize(lookup: EnvLookup<'_>, key: &str, default: usize) -> usize {
    parsed::<usize>(lookup, key)
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

/// Returns a non-negative integer, or `default` when the value is absent or
/// malformed.
pub(crate) fn nonnegative_u64(lookup: EnvLookup<'_>, key: &str, default: u64) -> u64 {
    parsed::<u64>(lookup, key).unwrap_or(default)
}

/// Returns a positive integer, or `None` when the value is absent,
/// malformed, or zero.
pub(crate) fn positive_u64(lookup: EnvLookup<'_>, key: &str) -> Option<u64> {
    parsed::<u64>(lookup, key).filter(|value| *value > 0)
}

fn is_set(lookup: EnvLookup<'_>, key: &str) -> bool {
    lookup(key).is_some()
}

/// Test-harness inputs for engine tests.
///
/// Harness inputs select checkers and replay scenarios. They never configure
/// engine behavior, so they stay out of [`EngineConfig`]. External provider
/// fixture inputs belong to `nimbus_storage::config::StorageTestHarness`,
/// because storage owns the fixture policy.
#[cfg(test)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EngineTestHarness {
    /// Crate root. Cargo and nextest set it at run time, and nextest remaps
    /// it for an archived run, so tests never read it at compile time.
    pub(crate) manifest_dir: Option<std::path::PathBuf>,
    /// Checksum-verified Elle CLI jar for the dedicated Elle lane.
    pub(crate) elle_cli_jar: Option<OsString>,
    /// Java binary for the Elle checker. `None` uses `java` from the path.
    pub(crate) elle_java_bin: Option<OsString>,
    /// Canonical PPSC failure scenario to replay. Not valid UTF-8 reads as
    /// absent.
    pub(crate) ppsc_replay_scenario_json: Option<String>,
    /// Backend for an embedded PPSC replay. Not valid UTF-8 reads as absent.
    pub(crate) ppsc_backend: Option<String>,
}

#[cfg(test)]
impl EngineTestHarness {
    pub(crate) const PPSC_REPLAY_SCENARIO_JSON_ENV: &'static str =
        "NIMBUS_PPSC_REPLAY_SCENARIO_JSON";
    pub(crate) const PPSC_BACKEND_ENV: &'static str = "NIMBUS_PPSC_BACKEND";

    /// Reads every harness input from the process environment.
    pub(crate) fn from_env() -> Self {
        Self::from_lookup(&|key| std::env::var_os(key))
    }

    pub(crate) fn from_lookup(lookup: EnvLookup<'_>) -> Self {
        let utf8 = |key| lookup(key).and_then(|value| value.into_string().ok());
        Self {
            manifest_dir: lookup("CARGO_MANIFEST_DIR").map(std::path::PathBuf::from),
            elle_cli_jar: lookup("NIMBUS_ELLE_CLI_JAR"),
            elle_java_bin: lookup("NIMBUS_ELLE_JAVA_BIN"),
            ppsc_replay_scenario_json: utf8(Self::PPSC_REPLAY_SCENARIO_JSON_ENV),
            ppsc_backend: utf8(Self::PPSC_BACKEND_ENV),
        }
    }

    /// Returns the crate root, or panics when the test runner did not set it.
    pub(crate) fn required_manifest_dir(&self) -> &std::path::Path {
        self.manifest_dir
            .as_deref()
            .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-engine tests")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn parse(values: &[(&str, &str)]) -> EngineConfig {
        let values: BTreeMap<String, OsString> = values
            .iter()
            .map(|(key, value)| ((*key).to_string(), OsString::from(value)))
            .collect();
        EngineConfig::from_lookup(&|key| values.get(key).cloned())
    }

    #[test]
    fn test_harness_parses_typed_inputs() {
        let values: BTreeMap<String, OsString> = [
            ("CARGO_MANIFEST_DIR", "/src/crates/nimbus-engine"),
            ("NIMBUS_ELLE_CLI_JAR", "/opt/elle-cli.jar"),
            (EngineTestHarness::PPSC_BACKEND_ENV, "redb"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), OsString::from(value)))
        .collect();
        let harness = EngineTestHarness::from_lookup(&|key| values.get(key).cloned());
        assert_eq!(
            harness.required_manifest_dir(),
            std::path::Path::new("/src/crates/nimbus-engine")
        );
        assert_eq!(
            harness.elle_cli_jar,
            Some(OsString::from("/opt/elle-cli.jar"))
        );
        assert_eq!(harness.elle_java_bin, None);
        assert_eq!(harness.ppsc_backend.as_deref(), Some("redb"));
        assert_eq!(harness.ppsc_replay_scenario_json, None);
    }

    #[test]
    fn empty_environment_yields_documented_defaults() {
        let config = parse(&[]);
        assert_eq!(config, EngineConfig::default());
        assert_eq!(config.mutation_journal_batch, BatchPolicy::new(32, 256, 0));
        assert_eq!(
            config.committer_publisher_batch,
            BatchPolicy::new(32, 256, 750)
        );
        assert_eq!(config.mutation_occ_retry, RetryPolicy::new(4, 100, 2_000));
        assert_eq!(config.publisher_retry, RetryPolicy::new(4, 1, 100));
        assert_eq!(config.committer_inbox.capacity, 128);
        assert_eq!(config.publisher_queue.capacity, 32);
        assert_eq!(config.observer_queue.capacity, 4_096);
        assert_eq!(config.observer_queue.high_watermark, 3_072);
        assert_eq!(config.tenant_mutation_isolate_ceiling, 16);
        assert_eq!(config.prepare_concurrency, None);
        assert_eq!(config.shadow_conflicts.sample_every, 16);
        assert_eq!(config.shadow_conflicts.window_max, 64);
        assert_eq!(config.diagnostics.commit_trace_threshold, None);
        assert!(!config.diagnostics.query_profile);
        assert!(!config.diagnostics.tenant_load_profile);
        assert_eq!(config.storage, StorageConfig::default());
        assert!((1..=4).contains(&config.resolved_prepare_concurrency()));
    }

    #[test]
    fn malformed_and_zero_values_keep_defaults() {
        let config = parse(&[
            ("NIMBUS_MUTATION_JOURNAL_BATCH_MAX", "0"),
            ("NIMBUS_COMMITTER_PUBLISHER_COALESCE_MICROS", "-5"),
            ("NIMBUS_MUTATION_OCC_MAX_RETRIES", "many"),
            ("NIMBUS_COMMITTER_INBOX_SIZE", "0"),
            ("NIMBUS_PREPARE_CONCURRENCY", "0"),
            ("NIMBUS_SHADOW_CONFLICT_SAMPLE_EVERY", ""),
        ]);
        assert_eq!(config, EngineConfig::default());
    }

    #[test]
    fn valid_values_override_defaults() {
        let config = parse(&[
            ("NIMBUS_MUTATION_JOURNAL_BATCH_MAX", " 8 "),
            ("NIMBUS_MUTATION_JOURNAL_COALESCE_MICROS", "40"),
            ("NIMBUS_MUTATION_OCC_MAX_RETRIES", "2"),
            ("NIMBUS_MUTATION_OCC_INITIAL_BACKOFF_MS", "0"),
            ("NIMBUS_MUTATION_OCC_MAX_BACKOFF_MS", "0"),
            ("NIMBUS_PREPARE_CONCURRENCY", "7"),
            ("NIMBUS_SHADOW_CONFLICT_SAMPLE_EVERY", "1"),
            ("NIMBUS_COMMIT_TRACE_THRESHOLD_MS", "20"),
            ("NIMBUS_QUERY_PROFILE", "0"),
            ("NIMBUS_REDB_OPEN_PROFILE", "1"),
        ]);
        // The batch maximum never drops below the fixed base.
        assert_eq!(config.mutation_journal_batch, BatchPolicy::new(32, 32, 40));
        assert_eq!(config.mutation_occ_retry, RetryPolicy::new(2, 0, 0));
        assert_eq!(config.mutation_occ_retry.backoff(3), Duration::ZERO);
        assert_eq!(config.resolved_prepare_concurrency(), 7);
        assert_eq!(config.shadow_conflicts.sample_every, 1);
        assert_eq!(
            config.diagnostics.commit_trace_threshold,
            Some(Duration::from_millis(20))
        );
        assert!(config.diagnostics.query_profile);
        assert!(config.storage.profile.redb_open);
    }

    #[test]
    fn present_but_malformed_trace_threshold_uses_default_threshold() {
        let config = parse(&[("NIMBUS_COMMIT_TRACE_THRESHOLD_MS", "soon")]);
        assert_eq!(
            config.diagnostics.commit_trace_threshold,
            Some(Duration::from_millis(DEFAULT_COMMIT_TRACE_THRESHOLD_MS))
        );
    }

    #[test]
    fn retry_backoff_doubles_until_the_cap() {
        let policy = RetryPolicy::new(8, 100, 350);
        assert_eq!(policy.backoff(1), Duration::from_millis(100));
        assert_eq!(policy.backoff(2), Duration::from_millis(200));
        assert_eq!(policy.backoff(3), Duration::from_millis(350));
        assert_eq!(policy.backoff(200), Duration::from_millis(350));
        // A cap below the initial delay rises to the initial delay.
        assert_eq!(
            RetryPolicy::new(1, 50, 10).backoff(4),
            Duration::from_millis(50)
        );
    }
}
