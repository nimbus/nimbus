// NDS3 cycle-75 event-loop utilization parity promotion.
//
// Nimbus's local perf_hooks bootstrap now provides Node-shaped
// eventLoopUtilization() cumulative and delta readings. The implementation keeps
// the pre-loop zero snapshot, exposes nodeTiming.idleTime from the same source,
// and lets active time grow from elapsed isolate time, which is enough for these
// official fixtures' idle/drift/delta assertions without granting host event-loop
// authority.

const NDS3_CYCLE75_EVENT_LOOP_NODE22_PATHS: &[&str] =
    &["test/parallel/test-performance-eventlooputil.js"];
const NDS3_CYCLE75_EVENT_LOOP_NODE24_PATHS: &[&str] =
    &["test/parallel/test-perf-hooks-eventlooputilization.js"];
const NDS3_CYCLE75_EVENT_LOOP_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle75_event_loop_utilization_batch() {
    let fixture_paths = NDS3_CYCLE75_EVENT_LOOP_NODE22_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle75-event-loop-utilization-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE75_EVENT_LOOP_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle75_event_loop_utilization_batch() {
    let fixture_paths = NDS3_CYCLE75_EVENT_LOOP_NODE24_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle75-event-loop-utilization-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE75_EVENT_LOOP_EXTRA_DIRS,
    );
}
