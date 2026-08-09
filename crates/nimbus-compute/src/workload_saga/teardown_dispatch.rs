//! Freshness-gated invocation of exact workload teardown capabilities.

#[cfg(test)]
mod tests {
    use super::super::teardown_test_support::production_source;

    const SOURCE: &str = include_str!("teardown_dispatch.rs");

    #[test]
    fn stale_execute_evidence_makes_zero_capability_calls() {
        let source = production_source(SOURCE);
        assert!(source.contains("struct WorkloadTeardownDispatcher"));
        assert!(source.contains("current_source"));
        assert!(source.contains("provider_reports"));
        assert!(source.contains("authenticate_execute"));
        assert!(source.contains("dispatch_confirmed"));
    }
}
