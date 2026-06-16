const NDS3_CYCLE68_VM_MODULE_BASIC_PATHS: &[&str] =
    &["test/parallel/test-vm-module-basic.js"];

const NDS3_CYCLE68_VM_MODULE_BASIC_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle68_vm_module_basic_batch() {
    let fixture_paths = NDS3_CYCLE68_VM_MODULE_BASIC_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle68-vm-module-basic-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE68_VM_MODULE_BASIC_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle68_vm_module_basic_batch() {
    let fixture_paths = NDS3_CYCLE68_VM_MODULE_BASIC_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle68-vm-module-basic-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE68_VM_MODULE_BASIC_EXTRA_DIRS,
    );
}
