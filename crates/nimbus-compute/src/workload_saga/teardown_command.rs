//! Confirmed and fenced workload teardown commands.

#[cfg(test)]
mod tests {
    use super::super::teardown_test_support::production_source;

    const SOURCE: &str = include_str!("teardown_command.rs");

    #[test]
    fn only_direct_claim_cas_winner_receives_execute() {
        let source = production_source(SOURCE);
        assert!(source.contains("struct ConfirmedWorkloadTeardownCommand"));
        assert!(source.contains("AppliedByThisCall"));
        assert!(source.contains("WorkloadTeardownCommandMode::Execute"));
        assert!(source.contains("fn from_confirmation"));
    }
}
