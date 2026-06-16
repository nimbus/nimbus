const NDS3_CYCLE34_ASSERT_DEEP_PATHS: &[&str] = &["test/parallel/test-assert-deep.js"];
const NDS3_CYCLE34_ASSERT_DEEP_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle34_assert_deep_batch() {
    let fixture_paths = NDS3_CYCLE34_ASSERT_DEEP_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle34-assert-deep-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE34_ASSERT_DEEP_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle34_assert_deep_batch() {
    let fixture_paths = NDS3_CYCLE34_ASSERT_DEEP_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle34-assert-deep-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE34_ASSERT_DEEP_EXTRA_DIRS,
    );
}
