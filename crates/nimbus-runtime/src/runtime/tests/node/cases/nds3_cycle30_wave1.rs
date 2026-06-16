const NDS3_CYCLE30_SOURCE_MAP_INVALID_URL_PATHS: &[&str] =
    &["test/parallel/test-source-map-invalid-url.js"];
const NDS3_CYCLE30_SOURCE_MAP_INVALID_URL_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle30_source_map_invalid_url_batch() {
    let fixture_paths = NDS3_CYCLE30_SOURCE_MAP_INVALID_URL_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle30-source-map-invalid-url-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE30_SOURCE_MAP_INVALID_URL_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle30_source_map_invalid_url_batch() {
    let fixture_paths = NDS3_CYCLE30_SOURCE_MAP_INVALID_URL_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle30-source-map-invalid-url-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE30_SOURCE_MAP_INVALID_URL_EXTRA_DIRS,
    );
}
