/// Wall-clock millis for resource stamping. Pure plumbing (no struct here
/// holds test-observable state), so this routes through the one canonical
/// `SystemClock` implementation (CO7) rather than injecting a per-call
/// clock.
pub(super) fn now_millis() -> u64 {
    nimbus_core::clock::system_now_millis()
}

pub(super) fn next_version(next: &mut u64, prefix: &str) -> String {
    *next = next.saturating_add(1).max(1);
    format!("{prefix}-v{}", *next)
}

#[cfg(test)]
mod tests {
    use super::*;

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
