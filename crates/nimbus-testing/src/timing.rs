use std::time::Duration;

pub fn usize_env_or(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

pub fn duration_ms_env_or(name: &str, default: Duration) -> Duration {
    let default_ms = default.as_millis().min(u64::MAX as u128) as u64;
    Duration::from_millis(
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(default_ms),
    )
}

pub fn ci_or_local_duration(local: Duration, ci: Duration) -> Duration {
    if std::env::var_os("CI").is_some() {
        ci
    } else {
        local
    }
}
