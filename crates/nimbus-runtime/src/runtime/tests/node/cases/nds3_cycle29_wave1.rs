const NDS3_CYCLE29_GET_BUILTIN_PATHS: &[&str] =
    &["test/parallel/test-process-get-builtin.mjs"];
const NDS3_CYCLE29_GET_BUILTIN_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle29_get_builtin_batch() {
    let fixture_paths = NDS3_CYCLE29_GET_BUILTIN_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle29-get-builtin-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE29_GET_BUILTIN_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle29_get_builtin_batch() {
    let fixture_paths = NDS3_CYCLE29_GET_BUILTIN_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle29-get-builtin-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE29_GET_BUILTIN_EXTRA_DIRS,
    );
}
