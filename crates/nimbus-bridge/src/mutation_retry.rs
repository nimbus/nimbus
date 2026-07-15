use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use nimbus_core::{Error, SequenceNumber};

const DEFAULT_MAX_ATTEMPTS: usize = 4;
const DEFAULT_INITIAL_BACKOFF_MS: u64 = 100;
const DEFAULT_MAX_BACKOFF_MS: u64 = 2_000;
static NEXT_JITTER_SEED: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationOccRetryPolicy {
    max_attempts: usize,
    initial_backoff: Duration,
    max_backoff: Duration,
    jitter_seed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationOccConflictDecision {
    NotRetryable,
    Retry {
        conflicting_sequence: Option<SequenceNumber>,
        backoff: Duration,
    },
    Exhausted,
}

impl MutationOccRetryPolicy {
    pub fn from_env() -> Self {
        Self {
            // The public knob keeps the plan's `MAX_RETRIES` name, but its
            // value is the total invocation-attempt cap. This makes exhausted
            // errors report exactly the configured number of attempts.
            max_attempts: env_positive_usize(
                "NIMBUS_MUTATION_OCC_MAX_RETRIES",
                DEFAULT_MAX_ATTEMPTS,
            ),
            initial_backoff: Duration::from_millis(env_positive_u64(
                "NIMBUS_MUTATION_OCC_INITIAL_BACKOFF_MS",
                DEFAULT_INITIAL_BACKOFF_MS,
            )),
            max_backoff: Duration::from_millis(env_positive_u64(
                "NIMBUS_MUTATION_OCC_MAX_BACKOFF_MS",
                DEFAULT_MAX_BACKOFF_MS,
            )),
            // Give concurrent invocations independent jitter even when they
            // conflict on the same commit sequence and failed attempt.
            jitter_seed: NEXT_JITTER_SEED.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub fn max_attempts(self) -> usize {
        self.max_attempts
    }

    pub fn classify(self, error: &Error, attempt: usize) -> MutationOccConflictDecision {
        let Error::Conflict {
            conflicting_sequence,
            retryable: true,
            ..
        } = error
        else {
            return MutationOccConflictDecision::NotRetryable;
        };
        if attempt >= self.max_attempts {
            return MutationOccConflictDecision::Exhausted;
        }
        MutationOccConflictDecision::Retry {
            conflicting_sequence: *conflicting_sequence,
            backoff: self.jittered_backoff(attempt, *conflicting_sequence),
        }
    }

    fn jittered_backoff(
        self,
        failed_attempt: usize,
        conflicting_sequence: Option<SequenceNumber>,
    ) -> Duration {
        let exponent = u32::try_from(failed_attempt.saturating_sub(1))
            .unwrap_or(u32::MAX)
            .min(63);
        let initial_ms = u64::try_from(self.initial_backoff.as_millis()).unwrap_or(u64::MAX);
        let max_ms = u64::try_from(self.max_backoff.as_millis()).unwrap_or(u64::MAX);
        let ceiling_ms = initial_ms.saturating_mul(1_u64 << exponent).min(max_ms);
        let floor_ms = ceiling_ms / 2;
        let jitter_width = ceiling_ms.saturating_sub(floor_ms).saturating_add(1);
        let sequence = conflicting_sequence.map_or(0, |sequence| sequence.0);
        let seed = sequence
            ^ u64::try_from(failed_attempt)
                .unwrap_or(u64::MAX)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ self.jitter_seed.rotate_left(17);
        let jitter_ms = splitmix64(seed) % jitter_width;
        Duration::from_millis(floor_ms.saturating_add(jitter_ms))
    }
}

fn env_positive_usize(key: &str, default: usize) -> usize {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn env_positive_u64(key: &str, default: u64) -> u64 {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_retries_only_retryable_conflicts_and_exhausts_at_attempt_cap() {
        let policy = MutationOccRetryPolicy {
            max_attempts: 2,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(2),
            jitter_seed: 1,
        };
        let retryable = Error::retryable_conflict("race", Some(SequenceNumber(7)));

        assert!(matches!(
            policy.classify(&retryable, 1),
            MutationOccConflictDecision::Retry {
                conflicting_sequence: Some(SequenceNumber(7)),
                ..
            }
        ));
        assert_eq!(
            policy.classify(&retryable, 2),
            MutationOccConflictDecision::Exhausted
        );
        assert_eq!(
            policy.classify(&Error::conflict("not OCC"), 1),
            MutationOccConflictDecision::NotRetryable
        );
    }

    #[test]
    fn backoff_timing_test() {
        let policy = MutationOccRetryPolicy {
            max_attempts: 8,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_millis(200),
            jitter_seed: 1,
        };

        let first = policy.jittered_backoff(1, Some(SequenceNumber(1)));
        let second = policy.jittered_backoff(2, Some(SequenceNumber(1)));
        let capped = policy.jittered_backoff(8, Some(SequenceNumber(1)));
        assert!((Duration::from_millis(50)..=Duration::from_millis(100)).contains(&first));
        assert!((Duration::from_millis(100)..=Duration::from_millis(200)).contains(&second));
        assert!(second >= first);
        assert!((Duration::from_millis(100)..=Duration::from_millis(200)).contains(&capped));

        let invocation_delays = (1..=16)
            .map(|jitter_seed| {
                MutationOccRetryPolicy {
                    jitter_seed,
                    ..policy
                }
                .jittered_backoff(1, Some(SequenceNumber(1)))
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert!(
            invocation_delays.len() > 1,
            "independent invocations should not share one deterministic delay"
        );
    }
}
