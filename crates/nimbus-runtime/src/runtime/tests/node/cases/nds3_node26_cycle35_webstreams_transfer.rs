// NDS3 node26 cycle-35 WebStreams transfer and structuredClone promotion.
//
// These Node26 fixtures share the WebStreams transfer bridge surface: cloned
// readable/writable/transform streams must preserve their brands, reject invalid
// duplicate transfer combinations, and allow an otherwise-idle isolate to exit.

const NDS3_NODE26_CYCLE35_WEBSTREAMS_TRANSFER_PATHS: &[&str] = &[
    "test/parallel/test-structuredClone-global.js",
    "test/parallel/test-webstreams-clone-unref.js",
    "test/parallel/test-whatwg-webstreams-transform-stream-members.js",
];
const NDS3_NODE26_CYCLE35_WEBSTREAMS_TRANSFER_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node26_current_lane_executes_cycle35_webstreams_transfer_batch() {
    let fixture_paths = NDS3_NODE26_CYCLE35_WEBSTREAMS_TRANSFER_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-cycle35-webstreams-transfer-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        NDS3_NODE26_CYCLE35_WEBSTREAMS_TRANSFER_EXTRA_DIRS,
    );
}
