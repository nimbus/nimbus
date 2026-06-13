const NDS3_CYCLE64_DOMAIN_ASYNC_ID_PATHS: &[&str] =
    &["test/parallel/test-domain-async-id-map-leak.js"];

const NDS3_CYCLE64_DOMAIN_ASYNC_ID_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle64_domain_async_id_batch() {
    let fixture_paths: Vec<String> = NDS3_CYCLE64_DOMAIN_ASYNC_ID_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle64-domain-async-id-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE64_DOMAIN_ASYNC_ID_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle64_domain_async_id_batch() {
    let fixture_paths: Vec<String> = NDS3_CYCLE64_DOMAIN_ASYNC_ID_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle64-domain-async-id-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE64_DOMAIN_ASYNC_ID_EXTRA_DIRS,
    );
}
