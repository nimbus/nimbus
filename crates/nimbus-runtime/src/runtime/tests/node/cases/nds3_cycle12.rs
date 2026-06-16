// NDS3 cycle-12a EventSource + perf_hooks resource-timing parity promotions.
//
// Two nimbus-runtime-local bootstrap fixes green these official fixtures on both
// the node22 supported lane and the node24 default lane:
//
//   * test-eventsource-disabled.js asserts that the EventSource global is absent
//     by default (`typeof EventSource === 'undefined'`) because Node 22/24 gate
//     it behind --experimental-eventsource. 98_global_scope_shared.js no longer
//     registers EventSource on the default global scope; the ext script stays
//     imported for opt-in callers.
//   * test-performance-resourcetimingbuffersize.js and
//     test-performance-resourcetimingbufferfull.js exercise the W3C Resource
//     Timing buffer contract: performance.setResourceTimingBufferSize() with
//     WebIDL unsigned-long coercion, the 250-entry default primary buffer, the
//     secondary overflow buffer, and the 'resourcetimingbufferfull' event that
//     drains it. perf_hooks.js now implements bufferResourceTiming() plus
//     setResourceTimingBufferSize() matching Node's lib/internal/perf/observe.js.
//
// All three fixtures require ../common, so the batch runs with the test/common
// extra dir. The single batch helper executes each fixture and fails the test on
// any assertion mismatch, so this is the dynamic green-guard that keeps the
// promotion honest.

const NDS3_CYCLE12_PROMOTED_PATHS: &[&str] = &[
    "test/parallel/test-eventsource-disabled.js",
    "test/parallel/test-performance-resourcetimingbuffersize.js",
    "test/parallel/test-performance-resourcetimingbufferfull.js",
];

const NDS3_CYCLE12_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle12_promoted_batch() {
    let fixture_paths = NDS3_CYCLE12_PROMOTED_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle12-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE12_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle12_promoted_batch() {
    let fixture_paths = NDS3_CYCLE12_PROMOTED_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle12-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE12_EXTRA_DIRS,
    );
}
