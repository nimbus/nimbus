// Fork fix (nimbus/deno v2.8.3-nimbus.39): align require(esm) race-condition
// detection when a synchronous CJS require enters an ES module while a dynamic
// import graph is still pending.
const NDS3_CYCLE92_ESM_REQUIRE_RACE_PATHS: &[&str] =
    &["test/es-module/test-esm-require-race-condition.js"];
const NDS3_CYCLE92_ESM_REQUIRE_RACE_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures/import-require-cycle"];

#[test]
fn node24_default_lane_executes_cycle92_esm_require_race() {
    let fixture_paths = NDS3_CYCLE92_ESM_REQUIRE_RACE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle92-esm-require-race",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE92_ESM_REQUIRE_RACE_EXTRA_DIRS,
    );
}
