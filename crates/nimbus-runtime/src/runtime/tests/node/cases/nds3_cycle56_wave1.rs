// NDS3 cycle-56 wave-1: vm dynamic-import callback missing-flag error.
//
// Fork fix (deno_node vm.js, NON-OOM, fork .10): user-provided
// `importModuleDynamically` callbacks now defer to Node's missing
// `--experimental-vm-modules` error instead of invoking the callback without the
// flag. Greens test-vm-dynamic-import-callback-missing-flag.js on both gate
// lanes.
const NDS3_CYCLE56_PATHS: &[&str] =
    &["test/parallel/test-vm-dynamic-import-callback-missing-flag.js"];
const NDS3_CYCLE56_EXTRA_DIRS: &[&str] = &["test/common", "test/fixtures"];

#[test]
fn node22_supported_lane_executes_cycle56_vm_dynamic_import_missing_flag_batch() {
    let fixture_paths = NDS3_CYCLE56_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle56-vm-dynamic-import-missing-flag-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE56_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle56_vm_dynamic_import_missing_flag_batch() {
    let fixture_paths = NDS3_CYCLE56_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle56-vm-dynamic-import-missing-flag-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE56_EXTRA_DIRS,
    );
}
