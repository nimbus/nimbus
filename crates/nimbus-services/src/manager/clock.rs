use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn now_millis() -> u64 {
    millis_since_epoch(SystemTime::now())
}

pub(super) fn next_version(next: &mut u64, prefix: &str) -> String {
    *next = next.saturating_add(1).max(1);
    format!("{prefix}-v{}", *next)
}

fn millis_since_epoch(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn millis_since_epoch_saturates_pre_epoch_to_zero() {
        assert_eq!(millis_since_epoch(UNIX_EPOCH - Duration::from_millis(1)), 0);
    }

    #[test]
    fn next_resource_version_starts_at_one_and_saturates() {
        let mut next = 0;
        assert_eq!(next_version(&mut next, "svcdef"), "svcdef-v1");
        assert_eq!(next_version(&mut next, "svcdef"), "svcdef-v2");

        let mut maxed = u64::MAX;
        assert_eq!(
            next_version(&mut maxed, "svcdef"),
            format!("svcdef-v{}", u64::MAX)
        );
    }
}
