const NDS3_CYCLE53_FS_SYMLINK_PATHS: &[&str] = &["test/parallel/test-fs-symlink.js"];
const NDS3_CYCLE53_EXTRA_DIRS: &[&str] = &["test/common", "test/fixtures"];

#[test]
fn node24_default_lane_executes_cycle53_fs_symlink_batch() {
    let fixture_paths = NDS3_CYCLE53_FS_SYMLINK_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle53-fs-symlink-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE53_EXTRA_DIRS,
    );
}
