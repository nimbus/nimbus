const NDS3_CYCLE42_STREAM_READABLE_COMPOSE_PATHS: &[&str] =
    &["test/parallel/test-stream-readable-compose.js"];
const NDS3_CYCLE42_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle42_stream_readable_compose_batch() {
    let fixture_paths = NDS3_CYCLE42_STREAM_READABLE_COMPOSE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle42-stream-readable-compose-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE42_EXTRA_DIRS,
    );
}
