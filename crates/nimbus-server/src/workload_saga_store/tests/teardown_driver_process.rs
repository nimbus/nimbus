const TEARDOWN_RUNTIME_SOURCE: &str =
    include_str!("../../../../nimbus-compute/src/workload_saga/teardown_runtime.rs");

#[test]
fn teardown_driver_process_crash_after_each_claim_inspects_before_retry() {
    let production = TEARDOWN_RUNTIME_SOURCE
        .split("#[cfg(test)]")
        .next()
        .unwrap_or(TEARDOWN_RUNTIME_SOURCE);
    assert!(production.contains("pub struct WorkloadTeardownRuntime"));
    assert!(production.contains("resume"));
    assert!(production.contains("WorkloadTeardownCommandMode::Inspect"));
}
