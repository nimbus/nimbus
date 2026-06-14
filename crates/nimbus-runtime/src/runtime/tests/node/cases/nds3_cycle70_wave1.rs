const NDS3_CYCLE70_VM_MODULE_REFERRER_REALM_PATHS: &[&str] =
    &["test/parallel/test-vm-module-referrer-realm.mjs"];

const NDS3_CYCLE70_VM_MODULE_REFERRER_REALM_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle70_vm_module_referrer_realm_batch() {
    let fixture_paths = NDS3_CYCLE70_VM_MODULE_REFERRER_REALM_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle70-vm-module-referrer-realm-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE70_VM_MODULE_REFERRER_REALM_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle70_vm_module_referrer_realm_batch() {
    let fixture_paths = NDS3_CYCLE70_VM_MODULE_REFERRER_REALM_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle70-vm-module-referrer-realm-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE70_VM_MODULE_REFERRER_REALM_EXTRA_DIRS,
    );
}
