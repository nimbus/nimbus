const NDS3_CYCLE59_EVENT_LOOP_TIMERS_PATHS: &[&str] = &[
    "test/parallel/test-timers-immediate-queue-throw.js",
    "test/parallel/test-timers-reset-process-domain-on-throw.js",
];

const NDS3_CYCLE59_EVENT_LOOP_TIMERS_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle59_event_loop_timers_batch() {
    let fixture_paths: Vec<String> = NDS3_CYCLE59_EVENT_LOOP_TIMERS_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle59-event-loop-timers-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE59_EVENT_LOOP_TIMERS_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle59_event_loop_timers_batch() {
    let fixture_paths: Vec<String> = NDS3_CYCLE59_EVENT_LOOP_TIMERS_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle59-event-loop-timers-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE59_EVENT_LOOP_TIMERS_EXTRA_DIRS,
    );
}
