// NDS3 cycle-10 perf_hooks parity promotions.
//
// Two nimbus-runtime-local perf_hooks.js fixes green these official fixtures on
// both the node22 supported lane and the node24 default lane:
//
//   * test-tojson-perf_hooks.js asserts that performance.toJSON() exposes Node's
//     { nodeTiming, timeOrigin, eventLoopUtilization } shape. deno_web's
//     performance.toJSON() only emitted { timeOrigin }; perf_hooks.js now wraps
//     it to match Node's lib/internal/perf/performance.js shape.
//   * test-performance-timeline.mjs asserts that getEntriesByName()/
//     getEntriesByType() throw ERR_MISSING_ARGS ("The \"name\"/\"type\" argument
//     must be specified") when called with zero arguments, while still treating
//     an explicit undefined argument as a real (one-argument) lookup. perf_hooks.js
//     now guards both wrappers on arguments.length === 0, matching Node's
//     Performance#getEntriesBy* contract, and sorts every getEntries* result
//     ascending by startTime as Node does.
//
// Both fixtures require ../common, so the batch runs with the test/common extra
// dir. The single batch helper executes each fixture and fails the test on any
// assertion mismatch, so this is the dynamic green-guard that keeps the
// promotion honest.

const NDS3_CYCLE10_PERF_PROMOTED_PATHS: &[&str] = &[
    "test/parallel/test-tojson-perf_hooks.js",
    "test/parallel/test-performance-timeline.mjs",
];

const NDS3_CYCLE10_PERF_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle10_perf_promoted_batch() {
    let fixture_paths = NDS3_CYCLE10_PERF_PROMOTED_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle10-perf-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE10_PERF_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle10_perf_promoted_batch() {
    let fixture_paths = NDS3_CYCLE10_PERF_PROMOTED_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle10-perf-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE10_PERF_EXTRA_DIRS,
    );
}
