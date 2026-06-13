const NDS3_CYCLE55_FS_PROMISES_PATHS: &[&str] = &["test/parallel/test-fs-promises.js"];
const NDS3_CYCLE55_EXTRA_DIRS: &[&str] = &["test/common", "test/fixtures"];

#[test]
fn node22_default_lane_executes_cycle55_fs_promises_batch() {
    let fixture_paths = NDS3_CYCLE55_FS_PROMISES_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-default-lane-executes-cycle55-fs-promises-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE55_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle55_fs_promises_batch() {
    let fixture_paths = NDS3_CYCLE55_FS_PROMISES_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle55-fs-promises-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE55_EXTRA_DIRS,
    );
}
