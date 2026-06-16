const NDS3_CYCLE98_VM_MODULE_IMPORT_META_PATHS: &[&str] =
    &["test/parallel/test-vm-module-import-meta.js"];

const NDS3_CYCLE98_VM_MODULE_IMPORT_META_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle98_vm_module_import_meta() {
    let fixture_paths = NDS3_CYCLE98_VM_MODULE_IMPORT_META_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle98-vm-module-import-meta",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE98_VM_MODULE_IMPORT_META_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle98_vm_module_import_meta() {
    let fixture_paths = NDS3_CYCLE98_VM_MODULE_IMPORT_META_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle98-vm-module-import-meta",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE98_VM_MODULE_IMPORT_META_EXTRA_DIRS,
    );
}
