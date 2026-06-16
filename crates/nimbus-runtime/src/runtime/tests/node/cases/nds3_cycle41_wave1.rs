const NDS3_CYCLE41_STREAM_WRITABLE_SAMECB_PATHS: &[&str] =
    &["test/parallel/test-stream-writable-samecb-singletick.js"];
const NDS3_CYCLE41_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle41_stream_writable_samecb_batch() {
    let fixture_paths = NDS3_CYCLE41_STREAM_WRITABLE_SAMECB_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle41-stream-writable-samecb-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE41_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle41_stream_writable_samecb_batch() {
    let fixture_paths = NDS3_CYCLE41_STREAM_WRITABLE_SAMECB_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle41-stream-writable-samecb-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE41_EXTRA_DIRS,
    );
}
