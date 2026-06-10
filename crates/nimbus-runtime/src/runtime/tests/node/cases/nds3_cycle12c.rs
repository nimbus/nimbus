// NDS3 cycle-12c no-fork nimbus-runtime promotions. Two nimbus-runtime-local
// fixes green these official fixtures via real dynamic green-guard execution
// (confirmed by the cycle-12c probe batch, then this enforced batch):
//
//   * test-vm-access-process-env.js asserts a vm context can read
//     process.env.PATH. The node-compat fixture application grant now
//     allowlists PATH for env_read (runtime_limits_for_node_compat_fixture in
//     node/mod.rs), so the assertion exercises real vm/env wiring against the
//     genuine host value instead of failing on an empty allowlist. Production
//     application presets still omit PATH. Greens on node22 + node24.
//
//   * test-perf-hooks-timerify-histogram-sync.mjs needs the upstream sleepSync
//     helper (SharedArrayBuffer + Atomics.wait) from test/common/index.js,
//     which the auto-staged lane-less helper was missing. With sleepSync added,
//     the fork's perf_hooks timerify({histogram}) records >=1ns per call so the
//     fixture's histogram.max assertions hold. Greens on node24 (the lane where
//     it was a required gap).
//
// Both fixtures require ../common, so the batches run with the test/common
// extra dir. Each batch helper executes every listed fixture and fails the test
// on any assertion mismatch -- this is the dynamic green-guard that keeps the
// promotion honest.

const NDS3_CYCLE12C_NODE22_PATHS: &[&str] = &["test/parallel/test-vm-access-process-env.js"];

const NDS3_CYCLE12C_NODE24_PATHS: &[&str] = &[
    "test/parallel/test-vm-access-process-env.js",
    "test/parallel/test-perf-hooks-timerify-histogram-sync.mjs",
];

const NDS3_CYCLE12C_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle12c_promoted_batch() {
    let fixture_paths = NDS3_CYCLE12C_NODE22_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle12c-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE12C_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle12c_promoted_batch() {
    let fixture_paths = NDS3_CYCLE12C_NODE24_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle12c-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE12C_EXTRA_DIRS,
    );
}
