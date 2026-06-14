// Fork fix (nimbus/deno v2.8.3-nimbus.42): no-referrer dynamic imports, such
// as indirect eval import(), reject with Node's missing-callback error instead
// of falling through to the default module loader.
const NDS3_CYCLE95_ESM_DYNAMIC_IMPORT_PATHS: &[&str] =
    &["test/es-module/test-esm-dynamic-import.js"];
const NDS3_CYCLE95_ESM_DYNAMIC_IMPORT_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures/es-modules"];

#[test]
fn node22_default_lane_executes_cycle95_esm_dynamic_import() {
    let fixture_paths = NDS3_CYCLE95_ESM_DYNAMIC_IMPORT_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-default-lane-executes-cycle95-esm-dynamic-import",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE95_ESM_DYNAMIC_IMPORT_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle95_esm_dynamic_import() {
    let fixture_paths = NDS3_CYCLE95_ESM_DYNAMIC_IMPORT_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle95-esm-dynamic-import",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE95_ESM_DYNAMIC_IMPORT_EXTRA_DIRS,
    );
}
