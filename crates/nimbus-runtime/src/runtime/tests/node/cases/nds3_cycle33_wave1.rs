const NDS3_CYCLE33_ASSERT_PATHS: &[&str] = &["test/parallel/test-assert.js"];
const NDS3_CYCLE33_ASSERT_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle33_assert_batch() {
    let fixture_paths = NDS3_CYCLE33_ASSERT_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle33-assert-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE33_ASSERT_EXTRA_DIRS,
    );
}
