use std::time::Duration;

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

pub(crate) fn env_positive_usize(key: &str, default: usize) -> usize {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

pub(crate) fn env_nonnegative_u64(key: &str, default: u64) -> u64 {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default)
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

    pub(crate) fn from_env(
        base: usize,
        max_key: &str,
        default_max: usize,
        coalesce_key: &str,
        default_coalesce_micros: u64,
    ) -> Self {
        Self::new(
            base,
            env_positive_usize(max_key, default_max),
            env_nonnegative_u64(coalesce_key, default_coalesce_micros),
        )
    }
}

pub(crate) fn committer_publisher_batch_policy() -> BatchPolicy {
    BatchPolicy::from_env(
        COMMITTER_PUBLISHER_BATCH_BASE,
        COMMITTER_PUBLISHER_BATCH_MAX_ENV,
        COMMITTER_PUBLISHER_BATCH_MAX_DEFAULT,
        COMMITTER_PUBLISHER_COALESCE_ENV,
        COMMITTER_PUBLISHER_COALESCE_DEFAULT_MICROS,
    )
}

pub(crate) fn committer_publisher_batch_max() -> usize {
    committer_publisher_batch_policy().max
}

pub(crate) fn mutation_journal_batch_policy() -> BatchPolicy {
    BatchPolicy::from_env(
        MUTATION_JOURNAL_BATCH_BASE,
        MUTATION_JOURNAL_BATCH_MAX_ENV,
        MUTATION_JOURNAL_BATCH_MAX_DEFAULT,
        MUTATION_JOURNAL_COALESCE_ENV,
        MUTATION_JOURNAL_COALESCE_DEFAULT_MICROS,
    )
}

pub(crate) fn mutation_journal_batch_max() -> usize {
    mutation_journal_batch_policy().max
}
