const NDS3_CYCLE97_VM_MODULE_TLA_PATHS: &[&str] =
    &["test/parallel/test-vm-module-hastoplevelawait.js"];

const NDS3_CYCLE97_VM_MODULE_TLA_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle97_vm_module_tla() {
    let fixture_paths = NDS3_CYCLE97_VM_MODULE_TLA_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle97-vm-module-tla",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE97_VM_MODULE_TLA_EXTRA_DIRS,
    );
}
