// NDS3 cycle-90 CommonJS-to-ESM snapshot promotion.
//
// Deno fork tag v2.8.3-nimbus.38 snapshots CommonJS `module.exports` after a
// successful initial load and has generated ESM wrappers consume that snapshot.
// The official ESM snapshot fixture dynamically greened on the published tag in
// both required lanes.

const NDS3_CYCLE90_ESM_CJS_SNAPSHOT_PATHS: &[&str] =
    &["test/es-module/test-esm-snapshot.mjs"];

const NDS3_CYCLE90_ESM_CJS_SNAPSHOT_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures",
    "test/fixtures/es-modules",
];

#[test]
fn node22_supported_lane_executes_cycle90_esm_cjs_snapshot_batch() {
    let fixture_paths = NDS3_CYCLE90_ESM_CJS_SNAPSHOT_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle90-esm-cjs-snapshot-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE90_ESM_CJS_SNAPSHOT_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle90_esm_cjs_snapshot_batch() {
    let fixture_paths = NDS3_CYCLE90_ESM_CJS_SNAPSHOT_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle90-esm-cjs-snapshot-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE90_ESM_CJS_SNAPSHOT_EXTRA_DIRS,
    );
}
