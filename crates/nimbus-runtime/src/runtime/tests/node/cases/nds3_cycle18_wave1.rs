// NDS3 cycle-18 wave-1: vm.SourceTextModule.hasAsyncGraph() (Node 24+).
//
// Fork fix (deno_node, NON-OOM): added `op_vm_module_is_graph_async` bound to
// V8 `Module::IsGraphAsync` (exported by the pinned prebuilt rusty_v8,
// src/module.rs:493 -- no new binding, no from-source V8 build) and wired
// `SourceTextModule.prototype.hasAsyncGraph()` in ext/node/polyfills/vm.js with
// the pre-instantiate ERR_VM_MODULE_STATUS guard. Green-guarded by the batch
// helper (re-executes the fixture; fails on skip/assert-mismatch/empty).
// node24-only gap (hasAsyncGraph is a Node 24 addition).

const NDS3_CYCLE18_PATHS: &[&str] = &["test/parallel/test-vm-module-hasasyncgraph.js"];
const NDS3_CYCLE18_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle18_vm_hasasyncgraph_batch() {
    let fixture_paths = NDS3_CYCLE18_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle18-vm-hasasyncgraph-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE18_EXTRA_DIRS,
    );
}
