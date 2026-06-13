const NDS3_CYCLE39_FILE_WRITE_STREAM_PATHS: &[&str] =
    &["test/parallel/test-file-write-stream5.js"];
const NDS3_CYCLE39_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle39_file_write_stream_batch() {
    let fixture_paths = NDS3_CYCLE39_FILE_WRITE_STREAM_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle39-file-write-stream-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE39_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle39_file_write_stream_batch() {
    let fixture_paths = NDS3_CYCLE39_FILE_WRITE_STREAM_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle39-file-write-stream-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE39_EXTRA_DIRS,
    );
}
