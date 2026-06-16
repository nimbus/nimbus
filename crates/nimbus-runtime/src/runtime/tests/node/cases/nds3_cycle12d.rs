// NDS3 cycle-12d no-fork nimbus-runtime promotion. test-performance-gc.js
// registers a PerformanceObserver for 'gc' entries and asserts on them at
// beforeExit. The GC entry capture and beforeExit emission are genuinely
// supported (node22 already greens); the node24 lane only needed the
// single-emit ProcessLifecycleDrain postlude against the settled loop
// (default_postlude_behavior_for_fixture in node/mod.rs). Confirmed by the
// cycle-12d probe, then enforced by this non-ignored green-guard batch.
//
// The fixture requires ../common, so the batch runs with the test/common extra
// dir.

const NDS3_CYCLE12D_NODE24_PATHS: &[&str] = &["test/parallel/test-performance-gc.js"];

const NDS3_CYCLE12D_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle12d_promoted_batch() {
    let fixture_paths = NDS3_CYCLE12D_NODE24_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle12d-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE12D_EXTRA_DIRS,
    );
}
