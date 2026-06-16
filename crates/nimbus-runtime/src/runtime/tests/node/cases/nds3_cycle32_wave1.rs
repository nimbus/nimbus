const NDS3_CYCLE32_ASSERT_CALLTRACKER_PATHS: &[&str] =
    &["test/parallel/test-assert-calltracker-calls.js"];
const NDS3_CYCLE32_ASSERT_CALLTRACKER_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle32_assert_calltracker_batch() {
    let fixture_paths = NDS3_CYCLE32_ASSERT_CALLTRACKER_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle32-assert-calltracker-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE32_ASSERT_CALLTRACKER_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle32_assert_calltracker_batch() {
    let fixture_paths = NDS3_CYCLE32_ASSERT_CALLTRACKER_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle32-assert-calltracker-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE32_ASSERT_CALLTRACKER_EXTRA_DIRS,
    );
}
