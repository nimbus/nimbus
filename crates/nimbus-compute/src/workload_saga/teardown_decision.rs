//! Compute projection of the portable workload teardown reducer.

#[cfg(test)]
mod tests {
    use super::super::teardown_test_support::production_source;

    const RECOVERY_SOURCE: &str = include_str!("recovery.rs");

    #[test]
    fn teardown_recovery_delegates_to_workloads_reducer() {
        let source = production_source(RECOVERY_SOURCE);
        assert!(source.contains("decide_teardown"));
        assert!(source.contains("WorkloadSagaAction::Teardown"));
        for raw_action in [
            "WithdrawPublication",
            "DrainWorkload",
            "StopWorkload",
            "DetachNetwork",
            "ReleaseNetwork",
            "RecordTerminalEvidence",
            "InspectCleanup",
            "AdvanceWithoutEffect",
        ] {
            assert!(
                !source.contains(raw_action),
                "recovery retains raw teardown authority `{raw_action}`"
            );
        }
    }
}
