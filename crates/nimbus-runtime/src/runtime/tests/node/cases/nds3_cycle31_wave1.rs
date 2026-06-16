const NDS3_CYCLE31_CONSOLE_PATHS: &[&str] = &["test/parallel/test-console.js"];
const NDS3_CYCLE31_CONSOLE_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle31_console_batch() {
    let fixture_paths = NDS3_CYCLE31_CONSOLE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle31-console-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE31_CONSOLE_EXTRA_DIRS,
    );
}
