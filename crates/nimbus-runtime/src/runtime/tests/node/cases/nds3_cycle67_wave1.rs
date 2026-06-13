const NDS3_CYCLE67_VM_GLOBAL_PROPERTY_INTERCEPTORS_PATHS: &[&str] =
    &["test/parallel/test-vm-global-property-interceptors.js"];

const NDS3_CYCLE67_VM_GLOBAL_PROPERTY_INTERCEPTORS_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle67_vm_global_property_interceptors_batch() {
    let fixture_paths = NDS3_CYCLE67_VM_GLOBAL_PROPERTY_INTERCEPTORS_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle67-vm-global-property-interceptors-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE67_VM_GLOBAL_PROPERTY_INTERCEPTORS_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle67_vm_global_property_interceptors_batch() {
    let fixture_paths = NDS3_CYCLE67_VM_GLOBAL_PROPERTY_INTERCEPTORS_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle67-vm-global-property-interceptors-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE67_VM_GLOBAL_PROPERTY_INTERCEPTORS_EXTRA_DIRS,
    );
}
