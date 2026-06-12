// NDS3 cycle-24 wave-1: vm.SourceTextModule link-failure -> 'errored' status.
// Fork fix (deno_node vm.js, NON-OOM): link() rejection now sets the module to
// `errored` (was reverting to null -> "unlinked"), matching Node's status
// machine. Greens test-vm-module-errors.js (asserts m.status==='errored' after a
// link that returns a non-Module / different-context module).
const NDS3_CYCLE24_PATHS: &[&str] = &["test/parallel/test-vm-module-errors.js"];
const NDS3_CYCLE24_EXTRA_DIRS: &[&str] = &["test/common"];
#[test]
fn node22_supported_lane_executes_cycle24_vm_errors_batch() {
    let fp=NDS3_CYCLE24_PATHS.iter().copied().map(str::to_string).collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs("node22-supported-lane-executes-cycle24-vm-errors-batch",NodeCompatLane::Node22,&fp,&[],NDS3_CYCLE24_EXTRA_DIRS);
}
#[test]
fn node24_default_lane_executes_cycle24_vm_errors_batch() {
    let fp=NDS3_CYCLE24_PATHS.iter().copied().map(str::to_string).collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs("node24-default-lane-executes-cycle24-vm-errors-batch",NodeCompatLane::Node24,&fp,&[],NDS3_CYCLE24_EXTRA_DIRS);
}
