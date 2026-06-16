const NDS3_CYCLE72_ESM_CJS_NAMED_ERROR_PATHS: &[&str] =
    &["test/es-module/test-esm-cjs-named-error.mjs"];

const NDS3_CYCLE72_ESM_CJS_NAMED_ERROR_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures/es-modules"];

#[test]
fn node24_default_lane_executes_cycle72_esm_cjs_named_error_batch() {
    let fixture_paths = NDS3_CYCLE72_ESM_CJS_NAMED_ERROR_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle72-esm-cjs-named-error-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE72_ESM_CJS_NAMED_ERROR_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle72_esm_cjs_named_error_batch() {
    let fixture_paths = NDS3_CYCLE72_ESM_CJS_NAMED_ERROR_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle72-esm-cjs-named-error-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE72_ESM_CJS_NAMED_ERROR_EXTRA_DIRS,
    );
}
