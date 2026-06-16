const NDS3_CYCLE63_VM_MODULE_AFTER_EVALUATE_PATHS: &[&str] =
    &["test/parallel/test-vm-module-after-evaluate.js"];

const NDS3_CYCLE63_VM_MODULE_AFTER_EVALUATE_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle63_vm_module_after_evaluate_batch() {
    let fixture_paths: Vec<String> = NDS3_CYCLE63_VM_MODULE_AFTER_EVALUATE_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle63-vm-module-after-evaluate-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE63_VM_MODULE_AFTER_EVALUATE_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle63_vm_module_after_evaluate_batch() {
    let fixture_paths: Vec<String> = NDS3_CYCLE63_VM_MODULE_AFTER_EVALUATE_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle63-vm-module-after-evaluate-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE63_VM_MODULE_AFTER_EVALUATE_EXTRA_DIRS,
    );
}
