const NDS3_CYCLE96_ESM_ASYNC_LOADER_HOOK_PATHS: &[&str] = &[
    "test/es-module/test-esm-loader-mock.mjs",
    "test/es-module/test-esm-virtual-json.mjs",
];

const NDS3_CYCLE96_ESM_ASYNC_LOADER_HOOK_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures/es-module-loaders"];

#[test]
fn node22_default_lane_executes_cycle96_esm_async_loader_hooks() {
    let fixture_paths = NDS3_CYCLE96_ESM_ASYNC_LOADER_HOOK_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-default-lane-executes-cycle96-esm-async-loader-hooks",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE96_ESM_ASYNC_LOADER_HOOK_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle96_esm_async_loader_hooks() {
    let fixture_paths = NDS3_CYCLE96_ESM_ASYNC_LOADER_HOOK_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle96-esm-async-loader-hooks",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE96_ESM_ASYNC_LOADER_HOOK_EXTRA_DIRS,
    );
}
