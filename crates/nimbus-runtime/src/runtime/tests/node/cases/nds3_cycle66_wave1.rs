const NDS3_CYCLE66_VM_GLOBAL_PROPERTY_PROTOTYPE_PATHS: &[&str] =
    &["test/parallel/test-vm-global-property-prototype.js"];

const NDS3_CYCLE66_VM_GLOBAL_PROPERTY_PROTOTYPE_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle66_vm_global_property_prototype_batch() {
    let fixture_paths: Vec<String> = NDS3_CYCLE66_VM_GLOBAL_PROPERTY_PROTOTYPE_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle66-vm-global-property-prototype-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE66_VM_GLOBAL_PROPERTY_PROTOTYPE_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle66_vm_global_property_prototype_batch() {
    let fixture_paths: Vec<String> = NDS3_CYCLE66_VM_GLOBAL_PROPERTY_PROTOTYPE_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle66-vm-global-property-prototype-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE66_VM_GLOBAL_PROPERTY_PROTOTYPE_EXTRA_DIRS,
    );
}
