const NDS3_CYCLE36_EVENTS_UNCAUGHT_EXCEPTION_STACK_PATHS: &[&str] =
    &["test/parallel/test-events-uncaught-exception-stack.js"];
const NDS3_CYCLE36_EVENTS_UNCAUGHT_EXCEPTION_STACK_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle36_events_uncaught_exception_stack_batch() {
    let fixture_paths = NDS3_CYCLE36_EVENTS_UNCAUGHT_EXCEPTION_STACK_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle36-events-uncaught-exception-stack-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE36_EVENTS_UNCAUGHT_EXCEPTION_STACK_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle36_events_uncaught_exception_stack_batch() {
    let fixture_paths = NDS3_CYCLE36_EVENTS_UNCAUGHT_EXCEPTION_STACK_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle36-events-uncaught-exception-stack-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE36_EVENTS_UNCAUGHT_EXCEPTION_STACK_EXTRA_DIRS,
    );
}
