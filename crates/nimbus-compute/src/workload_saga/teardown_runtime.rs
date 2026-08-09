//! Explicit retained runtime for exact workload teardown keys.

#[cfg(test)]
mod tests {
    use super::super::teardown_test_support::production_source;

    const SOURCE: &str = include_str!("teardown_runtime.rs");

    #[test]
    fn cancellation_before_runtime_submission_makes_zero_calls() {
        let source = production_source(SOURCE);
        assert!(source.contains("struct WorkloadTeardownRuntime"));
        assert!(source.contains("RetainedWorkloadTeardown"));
        assert!(source.contains("submit"));
        assert!(source.contains("tokio::spawn"));
    }
}
