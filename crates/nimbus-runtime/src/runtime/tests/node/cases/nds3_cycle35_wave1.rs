const NDS3_CYCLE35_PREPARE_STACK_TRACE_PATHS: &[&str] =
    &["test/parallel/test-error-prepare-stack-trace.js"];
const NDS3_CYCLE35_PREPARE_STACK_TRACE_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle35_error_prepare_stack_trace_batch() {
    let fixture_paths = NDS3_CYCLE35_PREPARE_STACK_TRACE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle35-error-prepare-stack-trace-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE35_PREPARE_STACK_TRACE_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle35_error_prepare_stack_trace_batch() {
    let fixture_paths = NDS3_CYCLE35_PREPARE_STACK_TRACE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle35-error-prepare-stack-trace-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE35_PREPARE_STACK_TRACE_EXTRA_DIRS,
    );
}
