const NDS3_CYCLE52_FS_STAT_DATE_PATHS: &[&str] = &["test/parallel/test-fs-stat-date.mjs"];
const NDS3_CYCLE52_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle52_fs_stat_date_batch() {
    let fixture_paths = NDS3_CYCLE52_FS_STAT_DATE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle52-fs-stat-date-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE52_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle52_fs_stat_date_batch() {
    let fixture_paths = NDS3_CYCLE52_FS_STAT_DATE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle52-fs-stat-date-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE52_EXTRA_DIRS,
    );
}
