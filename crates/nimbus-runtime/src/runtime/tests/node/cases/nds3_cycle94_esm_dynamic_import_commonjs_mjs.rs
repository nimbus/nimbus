// Fork fix (nimbus/deno v2.8.3-nimbus.41): share the nextTick deferral
// counter with deno_core so ESM-origin dynamic imports of CommonJS modules
// resume their import continuation before process.nextTick callbacks drain.
const NDS3_CYCLE94_ESM_DYNAMIC_IMPORT_COMMONJS_MJS_PATHS: &[&str] =
    &["test/es-module/test-esm-dynamic-import-commonjs.mjs"];
const NDS3_CYCLE94_ESM_DYNAMIC_IMPORT_COMMONJS_MJS_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures", "test/fixtures/es-modules"];

#[test]
fn node22_default_lane_executes_cycle94_esm_dynamic_import_commonjs_mjs() {
    let fixture_paths = NDS3_CYCLE94_ESM_DYNAMIC_IMPORT_COMMONJS_MJS_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-default-lane-executes-cycle94-esm-dynamic-import-commonjs-mjs",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE94_ESM_DYNAMIC_IMPORT_COMMONJS_MJS_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle94_esm_dynamic_import_commonjs_mjs() {
    let fixture_paths = NDS3_CYCLE94_ESM_DYNAMIC_IMPORT_COMMONJS_MJS_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle94-esm-dynamic-import-commonjs-mjs",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE94_ESM_DYNAMIC_IMPORT_COMMONJS_MJS_EXTRA_DIRS,
    );
}
