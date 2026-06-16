const NDS3_CYCLE40_HTTPPARSER_REUSE_PATHS: &[&str] =
    &["test/async-hooks/test-httpparser-reuse.js"];
const NDS3_CYCLE40_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle40_httpparser_reuse_batch() {
    let fixture_paths = NDS3_CYCLE40_HTTPPARSER_REUSE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle40-httpparser-reuse-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE40_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle40_httpparser_reuse_batch() {
    let fixture_paths = NDS3_CYCLE40_HTTPPARSER_REUSE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle40-httpparser-reuse-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE40_EXTRA_DIRS,
    );
}
