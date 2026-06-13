// NDS3 cycle-57 wave-1: vm module dynamic-import callback parity.
//
// Fork fix (deno_node node_options.ts + vm.js, NON-OOM, fork .11): the VM
// module flag is parsed from fixture execArgv, custom dynamic-import callbacks
// receive Node-shaped import attributes plus the evaluation phase, and invalid
// callback results reject with ERR_VM_MODULE_NOT_MODULE.
const NDS3_CYCLE57_PATHS: &[&str] = &["test/parallel/test-vm-module-dynamic-import.js"];
const NDS3_CYCLE57_EXTRA_DIRS: &[&str] = &["test/common", "test/fixtures"];

#[test]
fn node22_supported_lane_executes_cycle57_vm_module_dynamic_import_batch() {
    let fixture_paths = NDS3_CYCLE57_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle57-vm-module-dynamic-import-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE57_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle57_vm_module_dynamic_import_batch() {
    let fixture_paths = NDS3_CYCLE57_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle57-vm-module-dynamic-import-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE57_EXTRA_DIRS,
    );
}
