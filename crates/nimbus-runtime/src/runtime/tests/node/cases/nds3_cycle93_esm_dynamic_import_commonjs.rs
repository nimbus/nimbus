// Fork fix (nimbus/deno v2.8.3-nimbus.40): defer nextTick draining while a
// traced CommonJS dynamic import settles so the import continuation observes
// Node's ordering before process.nextTick callbacks run.
const NDS3_CYCLE93_ESM_DYNAMIC_IMPORT_COMMONJS_PATHS: &[&str] =
    &["test/es-module/test-esm-dynamic-import-commonjs.js"];
const NDS3_CYCLE93_ESM_DYNAMIC_IMPORT_COMMONJS_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures", "test/fixtures/es-modules"];

#[test]
fn node22_default_lane_executes_cycle93_esm_dynamic_import_commonjs() {
    let fixture_paths = NDS3_CYCLE93_ESM_DYNAMIC_IMPORT_COMMONJS_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-default-lane-executes-cycle93-esm-dynamic-import-commonjs",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE93_ESM_DYNAMIC_IMPORT_COMMONJS_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle93_esm_dynamic_import_commonjs() {
    let fixture_paths = NDS3_CYCLE93_ESM_DYNAMIC_IMPORT_COMMONJS_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle93-esm-dynamic-import-commonjs",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE93_ESM_DYNAMIC_IMPORT_COMMONJS_EXTRA_DIRS,
    );
}
