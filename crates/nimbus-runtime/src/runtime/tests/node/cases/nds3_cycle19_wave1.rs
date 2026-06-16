// NDS3 cycle-19 wave-1: vm.SourceTextModule.createCachedData() (Node 24+).
//
// Fork fix (deno_node, NON-OOM, fork .31): added op_vm_module_create_cached_data
// (V8 Module::GetUnboundModuleScript -> UnboundModuleScript::CreateCodeCache,
// prebuilt rusty_v8) + cachedData construction support
// (Source::new_with_cached_data + CompileOptions::ConsumeCodeCache +
// op_vm_module_cached_data_rejected) + ERR_VM_MODULE_CACHED_DATA_REJECTED. Greens
// test-vm-module-cached-data.js (createCachedData round-trip + rejection +
// cannot-create-after-evaluate) on both gate lanes.
const NDS3_CYCLE19_PATHS: &[&str] = &["test/parallel/test-vm-module-cached-data.js"];
const NDS3_CYCLE19_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle19_vm_cacheddata_batch() {
    let fixture_paths = NDS3_CYCLE19_PATHS.iter().copied().map(str::to_string).collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle19-vm-cacheddata-batch",
        NodeCompatLane::Node22, &fixture_paths, &[], NDS3_CYCLE19_EXTRA_DIRS);
}

#[test]
fn node24_default_lane_executes_cycle19_vm_cacheddata_batch() {
    let fixture_paths = NDS3_CYCLE19_PATHS.iter().copied().map(str::to_string).collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle19-vm-cacheddata-batch",
        NodeCompatLane::Node24, &fixture_paths, &[], NDS3_CYCLE19_EXTRA_DIRS);
}
