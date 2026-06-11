// NDS3 cycle-13 wave-4 NIMBUS-LOCAL runtime-semantics promotions.
//
// These three fixtures green on BOTH the node22 and node24 supported lanes via
// real dynamic green-guard PASSes against the cycle-13 wave-4 binary. Every fix
// they depend on lives in nimbus-runtime itself (no fork edit, so the promotion
// holds against the pinned `v2.8.2-nimbus.25` deno fork that CI builds):
//
//   * test-process-env-ignore-getter-setter / test-process-env-deprecation:
//     the `process.env` proxy in `bootstrap/source.rs` now rejects accessor and
//     partial data descriptors with an `ERR_INVALID_OBJECT_DEFINE_PROPERTY`
//     TypeError (`defineProperty` trap) and emits the DEP0104 DeprecationWarning
//     before coercing a non-string assignment to a string. Both use only
//     `globalThis.process.emitWarning`, `Reflect`, and standard JS already
//     present in the pinned fork.
//   * test-async-wrap-uncaughtexception: `default_postlude_behavior_for_fixture`
//     in the harness keys this fixture onto the single-emit ProcessLifecycleDrain
//     arm so its lone `process.on('beforeExit', mustCall())` handler fires once,
//     byte-identically across both lanes.
//
// Each was adversarially probe-verified one process per lane against the rebuilt
// binary: `summary: selected=1, passed=1, skipped=0, failed=0`. They are genuine
// v8_isolate_required gaps on both lanes, so promoting all three drops node22 and
// node24 by three each with no fork edit required.
//
// The #[test] calls run_node_compat_watchpoint_path_batch_with_lane_extra_dirs
// directly (not through a wrapper) so the classifier's static execution-marker
// scan attributes each fixture to each lane, and the batch helper re-executes
// each fixture and fails on any assertion mismatch or skip-to-empty -- the
// dynamic green-guard that keeps the promotion honest.

const NDS3_CYCLE13_W4_NIMBUS_LOCAL_PATHS: &[&str] = &[
    "test/parallel/test-process-env-ignore-getter-setter.js",
    "test/parallel/test-process-env-deprecation.js",
    "test/parallel/test-async-wrap-uncaughtexception.js",
];

const NDS3_CYCLE13_W4_NIMBUS_LOCAL_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle13_w4_nimbus_local_batch() {
    let fixture_paths = NDS3_CYCLE13_W4_NIMBUS_LOCAL_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle13-w4-nimbus-local-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE13_W4_NIMBUS_LOCAL_EXTRA_DIRS,
    );
}

#[test]
fn node24_supported_lane_executes_cycle13_w4_nimbus_local_batch() {
    let fixture_paths = NDS3_CYCLE13_W4_NIMBUS_LOCAL_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-supported-lane-executes-cycle13-w4-nimbus-local-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE13_W4_NIMBUS_LOCAL_EXTRA_DIRS,
    );
}
