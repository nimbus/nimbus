const NDS3_CYCLE69_VM_TIMEOUT_ESCAPE_PROMISE_MODULE_PATHS: &[&str] =
    &["test/parallel/test-vm-timeout-escape-promise-module.js"];

const NDS3_CYCLE69_VM_TIMEOUT_ESCAPE_PROMISE_MODULE_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle69_vm_timeout_escape_promise_module_batch() {
    let fixture_paths = NDS3_CYCLE69_VM_TIMEOUT_ESCAPE_PROMISE_MODULE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle69-vm-timeout-escape-promise-module-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE69_VM_TIMEOUT_ESCAPE_PROMISE_MODULE_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle69_vm_timeout_escape_promise_module_batch() {
    let fixture_paths = NDS3_CYCLE69_VM_TIMEOUT_ESCAPE_PROMISE_MODULE_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle69-vm-timeout-escape-promise-module-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE69_VM_TIMEOUT_ESCAPE_PROMISE_MODULE_EXTRA_DIRS,
    );
}
