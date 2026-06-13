const NDS3_CYCLE38_WEBSTREAMS_BYOB_PATHS: &[&str] =
    &["test/parallel/test-whatwg-readablebytestream-bad-buffers-and-views.js"];
const NDS3_CYCLE38_WEBSTREAMS_BYOB_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle38_webstreams_byob_batch() {
    let fixture_paths = NDS3_CYCLE38_WEBSTREAMS_BYOB_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle38-webstreams-byob-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE38_WEBSTREAMS_BYOB_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle38_webstreams_byob_batch() {
    let fixture_paths = NDS3_CYCLE38_WEBSTREAMS_BYOB_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle38-webstreams-byob-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE38_WEBSTREAMS_BYOB_EXTRA_DIRS,
    );
}
