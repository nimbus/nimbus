const NDS3_CYCLE37_GLOBAL_SURFACE_PATHS: &[&str] = &["test/parallel/test-global.js"];
const NDS3_CYCLE37_GLOBAL_SURFACE_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures/global"];

#[test]
fn node22_supported_lane_executes_cycle37_global_surface_batch() {
    let fixture_paths = NDS3_CYCLE37_GLOBAL_SURFACE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle37-global-surface-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE37_GLOBAL_SURFACE_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle37_global_surface_batch() {
    let fixture_paths = NDS3_CYCLE37_GLOBAL_SURFACE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle37-global-surface-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE37_GLOBAL_SURFACE_EXTRA_DIRS,
    );
}
