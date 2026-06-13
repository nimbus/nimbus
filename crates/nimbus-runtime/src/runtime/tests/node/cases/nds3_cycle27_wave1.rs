const NDS3_CYCLE27_STRUCTURED_CLONE_PATHS: &[&str] =
    &["test/parallel/test-structuredClone-global.js"];
const NDS3_CYCLE27_STRUCTURED_CLONE_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle27_structured_clone_batch() {
    let fixture_paths = NDS3_CYCLE27_STRUCTURED_CLONE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle27-structured-clone-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE27_STRUCTURED_CLONE_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle27_structured_clone_batch() {
    let fixture_paths = NDS3_CYCLE27_STRUCTURED_CLONE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle27-structured-clone-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE27_STRUCTURED_CLONE_EXTRA_DIRS,
    );
}
