// NDS3 cycle-76 WebStreams transfer liveness promotion.
//
// Deno fork tag v2.8.3-nimbus.25 unrefs the internal MessagePorts used by
// transferred ReadableStream/WritableStream cross-realm bridge setup. The
// official fixture asserts cloned WebStreams preserve their brands and then lets
// process/runtime liveness prove that the unused bridge ports do not keep the
// isolate alive.

const NDS3_CYCLE76_WEBSTREAMS_CLONE_UNREF_PATHS: &[&str] =
    &["test/parallel/test-webstreams-clone-unref.js"];
const NDS3_CYCLE76_WEBSTREAMS_CLONE_UNREF_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle76_webstreams_clone_unref_batch() {
    let fixture_paths = NDS3_CYCLE76_WEBSTREAMS_CLONE_UNREF_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle76-webstreams-clone-unref-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE76_WEBSTREAMS_CLONE_UNREF_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle76_webstreams_clone_unref_batch() {
    let fixture_paths = NDS3_CYCLE76_WEBSTREAMS_CLONE_UNREF_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle76-webstreams-clone-unref-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE76_WEBSTREAMS_CLONE_UNREF_EXTRA_DIRS,
    );
}
