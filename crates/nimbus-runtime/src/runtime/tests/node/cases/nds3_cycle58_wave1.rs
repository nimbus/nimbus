const NDS3_CYCLE58_PATHS: &[&str] = &[
    "test/parallel/test-timers-immediate-unref-simple.js",
    "test/parallel/test-timers-immediate-unref.js",
    "test/parallel/test-timers-immediate-unref-nested-once.js",
];

const NDS3_CYCLE58_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle58_timers_immediate_unref_batch() {
    let fixture_paths = NDS3_CYCLE58_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle58-timers-immediate-unref-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE58_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle58_timers_immediate_unref_batch() {
    let fixture_paths = NDS3_CYCLE58_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle58-timers-immediate-unref-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE58_EXTRA_DIRS,
    );
}
