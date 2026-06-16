const NDS3_CYCLE60_DOMAIN_CAPTURE_AFTER_LOAD_PATHS: &[&str] =
    &["test/parallel/test-domain-set-uncaught-exception-capture-after-load.js"];

const NDS3_CYCLE60_DOMAIN_CAPTURE_AFTER_LOAD_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle60_domain_capture_after_load_batch() {
    let fixture_paths: Vec<String> = NDS3_CYCLE60_DOMAIN_CAPTURE_AFTER_LOAD_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle60-domain-capture-after-load-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE60_DOMAIN_CAPTURE_AFTER_LOAD_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle60_domain_capture_after_load_batch() {
    let fixture_paths: Vec<String> = NDS3_CYCLE60_DOMAIN_CAPTURE_AFTER_LOAD_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle60-domain-capture-after-load-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE60_DOMAIN_CAPTURE_AFTER_LOAD_EXTRA_DIRS,
    );
}
