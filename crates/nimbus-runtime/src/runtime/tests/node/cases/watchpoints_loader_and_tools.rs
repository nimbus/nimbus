#[test]
fn node22_loader_context_followup_worker_main_thread_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-worker-main-thread-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_WORKER_MAIN_THREAD_BATCH,
    );
}

#[test]
fn node20_loader_context_followup_worker_main_thread_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-followup-worker-main-thread-batch",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_FOLLOWUP_WORKER_MAIN_THREAD_BATCH,
    );
}

#[test]
fn node24_loader_context_followup_worker_main_thread_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-followup-worker-main-thread-batch",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_FOLLOWUP_WORKER_MAIN_THREAD_BATCH,
    );
}

#[test]
fn node22_loader_context_followup_worker_basic_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-worker-basic-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_WORKER_BASIC_BATCH,
    );
}

#[test]
fn node20_loader_context_followup_worker_basic_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-followup-worker-basic-batch",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_FOLLOWUP_WORKER_BASIC_BATCH,
    );
}

#[test]
fn node24_loader_context_followup_worker_basic_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-followup-worker-basic-batch",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_FOLLOWUP_WORKER_BASIC_BATCH,
    );
}

#[test]
fn node22_loader_context_followup_worker_bootstrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-worker-bootstrap-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_WORKER_BOOTSTRAP_BATCH,
    );
}

#[test]
fn node20_loader_context_followup_worker_bootstrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-followup-worker-bootstrap-batch",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_FOLLOWUP_WORKER_BOOTSTRAP_BATCH,
    );
}

#[test]
fn node24_loader_context_followup_worker_bootstrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-followup-worker-bootstrap-batch",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_FOLLOWUP_WORKER_BOOTSTRAP_BATCH,
    );
}

#[test]
fn node22_loader_context_followup_worker_contract_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-worker-contract-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_WORKER_CONTRACT_BATCH,
    );
}

#[test]
fn node20_loader_context_followup_worker_contract_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-followup-worker-contract-batch",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_FOLLOWUP_WORKER_CONTRACT_BATCH,
    );
}

#[test]
fn node24_loader_context_followup_worker_contract_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-followup-worker-contract-batch",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_FOLLOWUP_WORKER_CONTRACT_BATCH,
    );
}

#[test]
fn node22_loader_context_followup_worker_message_port_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-worker-message-port-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_WORKER_MESSAGE_PORT_BATCH,
    );
}

#[test]
fn node22_loader_context_followup_worker_message_channel_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-worker-message-channel-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_WORKER_MESSAGE_CHANNEL_BATCH,
    );
}

#[test]
fn node22_loader_context_followup_worker_onmessage_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-worker-onmessage.js",
        "node22/test/parallel/test-worker-onmessage.js",
        &[],
    );
}

#[test]
fn node22_loader_context_followup_worker_ref_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-worker-ref.js",
        "node22/test/parallel/test-worker-ref.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned native-addon/FFI gap: test-module-loading-error.js requires attempting to dlopen a .node fixture, while the default Nimbus Node-compat runtime intentionally has no ffi grant"]
fn node22_loader_context_followup_module_commonjs_remainder_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-module-commonjs-remainder-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_MODULE_COMMONJS_REMAINDER_BATCH,
    );
}

#[test]
#[ignore = "Pinned native-addon/FFI gap: test-module-loading-error.js requires attempting to dlopen a .node fixture, while the default Nimbus Node-compat runtime intentionally has no ffi grant"]
fn node20_loader_context_followup_module_commonjs_remainder_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-followup-module-commonjs-remainder-batch",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_FOLLOWUP_MODULE_COMMONJS_REMAINDER_BATCH,
    );
}

#[test]
#[ignore = "Pinned native-addon/FFI gap: test-module-loading-error.js requires attempting to dlopen a .node fixture, while the default Nimbus Node-compat runtime intentionally has no ffi grant"]
fn node24_loader_context_followup_module_commonjs_remainder_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-followup-module-commonjs-remainder-batch",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_FOLLOWUP_MODULE_COMMONJS_REMAINDER_BATCH,
    );
}

#[test]
fn node24_loader_context_global_paths_preserve_local_precedence_regression() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-module-loading-globalpaths.js",
        "node24/test/parallel/test-module-loading-globalpaths.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

const LOADER_CONTEXT_MODULE_EXTRA_RUNTIME_FILES: &[&str] = &[
    "test/fixtures/baz.js",
    "test/fixtures/empty.js",
    "test/fixtures/pkgexports.mjs",
    "test/fixtures/simple.wasm",
    "test/fixtures/value.cjs",
];

const LOADER_CONTEXT_MODULE_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/module-hooks",
    "test/fixtures/es-module-loaders",
    "test/fixtures/es-module-specifiers",
    "test/fixtures/es-modules",
    "test/fixtures/internal-modules",
    "test/fixtures/module-extension-over-directory",
    "test/fixtures/module-hooks",
    "test/fixtures/module-require",
    "test/fixtures/module-require-symlink",
    "test/fixtures/node_modules",
    "test/fixtures/packages",
    "test/fixtures/snapshot",
    "test/fixtures/typescript",
    "test/fixtures/wpt/wasm",
];

const LOADER_CONTEXT_MODULE_LOW_ROI_PATHS: &[&str] = &[
    "test/module-hooks/test-module-hooks-load-async-and-sync.js",
    "test/module-hooks/test-module-hooks-preload.js",
    "test/module-hooks/test-module-hooks-require-esm.js",
    "test/parallel/test-module-loading-error.js",
    "test/parallel/test-module-main-preserve-symlinks-fail.js",
    "test/parallel/test-module-print-timing.mjs",
    "test/parallel/test-module-run-main-monkey-patch.js",
];

fn loader_context_module_runnable_fixture_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths = node_compat_required_gap_paths_for_owner(lane, "loader-context/module");
    fixture_paths.retain(|path| {
        !LOADER_CONTEXT_MODULE_LOW_ROI_PATHS
            .iter()
            .any(|low_roi_path| path == low_roi_path)
    });
    fixture_paths
}

const LOADER_CONTEXT_MODULE_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/module-hooks/test-module-hooks-custom-conditions-cjs.js",
    "test/module-hooks/test-module-hooks-custom-conditions.mjs",
    "test/module-hooks/test-module-hooks-import-wasm.mjs",
    "test/module-hooks/test-module-hooks-load-buffers.js",
    "test/module-hooks/test-module-hooks-load-builtin-override-commonjs.js",
    "test/module-hooks/test-module-hooks-load-builtin-override-json.js",
    "test/module-hooks/test-module-hooks-load-builtin-override-module.js",
    "test/module-hooks/test-module-hooks-load-builtin-require.js",
    "test/module-hooks/test-module-hooks-load-chained.js",
    "test/module-hooks/test-module-hooks-load-context-merged-esm.mjs",
    "test/module-hooks/test-module-hooks-load-context-merged.js",
    "test/module-hooks/test-module-hooks-load-context-optional-esm.mjs",
    "test/module-hooks/test-module-hooks-load-context-optional.js",
    "test/module-hooks/test-module-hooks-load-detection.js",
    "test/module-hooks/test-module-hooks-load-esm-mock.js",
    "test/module-hooks/test-module-hooks-load-esm.js",
    "test/module-hooks/test-module-hooks-load-import-cjs.js",
    "test/module-hooks/test-module-hooks-load-invalid.js",
    "test/module-hooks/test-module-hooks-load-mock.js",
    "test/module-hooks/test-module-hooks-load-short-circuit-required-middle.js",
    "test/module-hooks/test-module-hooks-load-short-circuit-required-start.js",
    "test/module-hooks/test-module-hooks-load-short-circuit.js",
    "test/module-hooks/test-module-hooks-load-url-change-import.mjs",
    "test/module-hooks/test-module-hooks-load-url-change-require.js",
    "test/module-hooks/test-module-hooks-resolve-builtin-builtin-import.mjs",
    "test/module-hooks/test-module-hooks-resolve-builtin-builtin-require.js",
    "test/module-hooks/test-module-hooks-resolve-builtin-on-disk-import.mjs",
    "test/module-hooks/test-module-hooks-resolve-context-merged-esm.mjs",
    "test/module-hooks/test-module-hooks-resolve-context-merged.js",
    "test/module-hooks/test-module-hooks-resolve-context-optional-esm.mjs",
    "test/module-hooks/test-module-hooks-resolve-context-optional.js",
    "test/module-hooks/test-module-hooks-resolve-import-cjs.js",
    "test/module-hooks/test-module-hooks-resolve-invalid.js",
    "test/module-hooks/test-module-hooks-resolve-short-circuit-required-middle.js",
    "test/module-hooks/test-module-hooks-resolve-short-circuit-required-start.js",
    "test/module-hooks/test-module-hooks-resolve-short-circuit.js",
];

const LOADER_CONTEXT_MODULE_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/module-hooks/test-module-hooks-resolve-require-resolve-builtin.js",
    "test/module-hooks/test-module-hooks-resolve-require-resolve-consistency.js",
    "test/module-hooks/test-module-hooks-resolve-require-resolve-create-require.js",
    "test/module-hooks/test-module-hooks-resolve-require-resolve-fallthrough.js",
    "test/module-hooks/test-module-hooks-resolve-require-resolve-imported-cjs.js",
    "test/module-hooks/test-module-hooks-resolve-require-resolve-loaded-with-source.js",
    "test/module-hooks/test-module-hooks-resolve-require-resolve-paths.js",
    "test/module-hooks/test-module-hooks-resolve-require-resolve-redirect.js",
];

fn loader_context_module_promoted_fixture_paths(groups: &[&[&str]]) -> Vec<String> {
    groups
        .iter()
        .flat_map(|group| group.iter().copied())
        .map(str::to_string)
        .collect()
}

#[test]
fn node22_supported_lane_executes_loader_context_module_promoted_batch_fixture() {
    let fixture_paths =
        loader_context_module_promoted_fixture_paths(&[LOADER_CONTEXT_MODULE_PROMOTED_COMMON_PATHS]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-loader-context-module-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        LOADER_CONTEXT_MODULE_EXTRA_RUNTIME_FILES,
        LOADER_CONTEXT_MODULE_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_loader_context_module_promoted_batch_fixture() {
    let fixture_paths = loader_context_module_promoted_fixture_paths(&[
        LOADER_CONTEXT_MODULE_PROMOTED_COMMON_PATHS,
        LOADER_CONTEXT_MODULE_PROMOTED_NODE24_ONLY_PATHS,
    ]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-loader-context-module-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        LOADER_CONTEXT_MODULE_EXTRA_RUNTIME_FILES,
        LOADER_CONTEXT_MODULE_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked loader-context/module required-gap inventory; native/self-exec CLI paths are excluded by the kill rule and remain gaps"]
fn node22_supported_lane_loader_context_module_watchpoint() {
    let fixture_paths = loader_context_module_runnable_fixture_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-loader-context-module-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        LOADER_CONTEXT_MODULE_EXTRA_RUNTIME_FILES,
        LOADER_CONTEXT_MODULE_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked loader-context/module required-gap inventory; native/self-exec CLI paths are excluded by the kill rule and remain gaps"]
fn node24_default_lane_loader_context_module_watchpoint() {
    let fixture_paths = loader_context_module_runnable_fixture_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-loader-context-module-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        LOADER_CONTEXT_MODULE_EXTRA_RUNTIME_FILES,
        LOADER_CONTEXT_MODULE_EXTRA_DIRS,
    );
}

#[test]
fn node22_loader_context_followup_inspector_front_edge_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-inspector-front-edge-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_INSPECTOR_FRONT_EDGE_BATCH,
    );
}

#[test]
fn node20_loader_context_followup_inspector_front_edge_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-followup-inspector-front-edge-batch",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_FOLLOWUP_INSPECTOR_FRONT_EDGE_BATCH,
    );
}

#[test]
fn node24_loader_context_followup_inspector_front_edge_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-followup-inspector-front-edge-batch",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_FOLLOWUP_INSPECTOR_FRONT_EDGE_BATCH,
    );
}

#[test]
fn node22_loader_context_followup_module_wrapper_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-regression.js",
        "node22/test/parallel/test-module-wrapper-regression.js",
        &[],
    );
}

#[test]
fn node22_loader_context_followup_module_wrapper_identity_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-identity-regression.js",
        "node22/test/parallel/test-module-wrapper-identity-regression.js",
        &[],
    );
}

#[test]
fn node22_loader_context_followup_module_wrapper_direct_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-direct-regression.js",
        "node22/test/parallel/test-module-wrapper-direct-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_loader_context_followup_module_wrapper_direct_no_common_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-direct-no-common-regression.js",
        "node22/test/parallel/test-module-wrapper-direct-no-common-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_loader_context_followup_module_wrapper_spawn_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-spawn-regression.js",
        "node22/test/parallel/test-module-wrapper-spawn-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_loader_context_followup_module_wrapper_spawn_require_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-spawn-require-regression.js",
        "node22/test/parallel/test-module-wrapper-spawn-require-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_loader_context_followup_module_wrapper_spawn_wrap_call_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-spawn-wrap-call-regression.js",
        "node22/test/parallel/test-module-wrapper-spawn-wrap-call-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_loader_context_followup_module_wrapper_spawn_node_shape_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-spawn-node-shape-regression.js",
        "node22/test/parallel/test-module-wrapper-spawn-node-shape-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_loader_context_followup_module_wrapper_spawn_newline_wrap_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-spawn-newline-wrap-regression.js",
        "node22/test/parallel/test-module-wrapper-spawn-newline-wrap-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_loader_context_followup_module_wrapper_official_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper.js",
        "node22/test/parallel/test-module-wrapper.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_loader_context_followup_vm_basic_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-basic.js",
        "node22/test/parallel/test-vm-basic.js",
        &[],
    );
}

#[test]
fn node22_loader_context_followup_vm_context_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context.js",
        "node22/test/parallel/test-vm-context.js",
        &[],
    );
}

#[test]
fn node22_loader_context_followup_vm_run_in_new_context_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-run-in-new-context.js",
        "node22/test/parallel/test-vm-run-in-new-context.js",
        &[],
    );
}

#[test]
fn node22_loader_context_followup_vm_context_regression_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-vm-context-regression-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_VM_CONTEXT_REGRESSION_BATCH,
    );
}

#[test]
fn node22_loader_context_followup_vm_context_remainder_regression_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-vm-context-remainder-regression-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_VM_CONTEXT_REMAINDER_REGRESSION_BATCH,
    );
}

#[test]
fn node22_loader_context_followup_vm_shared_context_errors_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-shared-context-errors.js",
        "node22/test/parallel/test-vm-context-regression-shared-context-errors.js",
        &[],
    );
}

#[test]
fn node22_loader_context_followup_vm_remainder_combined_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-remainder-combined.js",
        "node22/test/parallel/test-vm-context-regression-remainder-combined.js",
        &[],
    );
}

#[test]
fn node22_loader_context_followup_vm_official_minus_proxy_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-official-minus-proxy.js",
        "node22/test/parallel/test-vm-context-regression-official-minus-proxy.js",
        &[],
    );
}

#[test]
fn node22_loader_context_followup_vm_preamble_plus_proxy_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-preamble-plus-proxy.js",
        "node22/test/parallel/test-vm-context-regression-preamble-plus-proxy.js",
        &[],
    );
}

#[test]
fn node22_loader_context_followup_vm_delete_then_proxy_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-delete-then-proxy.js",
        "node22/test/parallel/test-vm-context-regression-delete-then-proxy.js",
        &[],
    );
}

#[test]
fn node22_loader_context_followup_vm_shared_errors_plus_proxy_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-shared-errors-plus-proxy.js",
        "node22/test/parallel/test-vm-context-regression-shared-errors-plus-proxy.js",
        &[],
    );
}

#[test]
fn node22_loader_context_followup_vm_remainder_plus_proxy_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-remainder-plus-proxy.js",
        "node22/test/parallel/test-vm-context-regression-remainder-plus-proxy.js",
        &[],
    );
}

#[test]
fn node22_loader_context_followup_vm_multi_context_plus_proxy_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-multi-context-plus-proxy.js",
        "node22/test/parallel/test-vm-context-regression-multi-context-plus-proxy.js",
        &[],
    );
}

const LOADER_CONTEXT_VM_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures/es-modules",
    "test/fixtures/keys",
];

const LOADER_CONTEXT_VM_FATAL_ABORT_PATHS: &[&str] =
    &["test/parallel/test-vm-module-evaluate-while-evaluating.js"];
const LOADER_CONTEXT_VM_FATAL_ABORT_PREFIXES: &[&str] = &["test/parallel/test-vm-module-"];

fn loader_context_vm_runnable_fixture_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths = node_compat_required_gap_paths_for_owner(lane, "loader-context/vm");
    fixture_paths.retain(|path| {
        !LOADER_CONTEXT_VM_FATAL_ABORT_PATHS
            .iter()
            .any(|fatal_path| path == fatal_path)
            && !LOADER_CONTEXT_VM_FATAL_ABORT_PREFIXES
                .iter()
                .any(|fatal_prefix| path.starts_with(fatal_prefix))
    });
    fixture_paths
}

const LOADER_CONTEXT_VM_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-vm-attributes-property-not-on-sandbox.js",
    "test/parallel/test-vm-codegen.js",
    "test/parallel/test-vm-context-async-script.js",
    "test/parallel/test-vm-context-dont-contextify.js",
    "test/parallel/test-vm-context-property-forwarding.js",
    "test/parallel/test-vm-create-and-run-in-context.js",
    "test/parallel/test-vm-create-context-accessors.js",
    "test/parallel/test-vm-create-context-circular-reference.js",
    "test/parallel/test-vm-createcacheddata.js",
    "test/parallel/test-vm-cross-context.js",
    "test/parallel/test-vm-data-property-writable.js",
    "test/parallel/test-vm-deleting-property.js",
    "test/parallel/test-vm-function-declaration.js",
    "test/parallel/test-vm-function-redefinition.js",
    "test/parallel/test-vm-getters.js",
    "test/parallel/test-vm-global-assignment.js",
    "test/parallel/test-vm-global-configurable-properties.js",
    "test/parallel/test-vm-global-define-property.js",
    "test/parallel/test-vm-global-get-own.js",
    "test/parallel/test-vm-global-identity.js",
    "test/parallel/test-vm-global-non-writable-properties.js",
    "test/parallel/test-vm-global-setter.js",
    "test/parallel/test-vm-harmony-symbols.js",
    "test/parallel/test-vm-indexed-properties.js",
    "test/parallel/test-vm-inherited_properties.js",
    "test/parallel/test-vm-is-context.js",
    "test/parallel/test-vm-low-stack-space.js",
    "test/parallel/test-vm-new-script-new-context.js",
    "test/parallel/test-vm-new-script-this-context.js",
    "test/parallel/test-vm-options-validation.js",
    "test/parallel/test-vm-ownkeys.js",
    "test/parallel/test-vm-ownpropertynames.js",
    "test/parallel/test-vm-ownpropertysymbols.js",
    "test/parallel/test-vm-parse-abort-on-uncaught-exception.js",
    "test/parallel/test-vm-preserves-property.js",
    "test/parallel/test-vm-property-not-on-sandbox.js",
    "test/parallel/test-vm-proxies.js",
    "test/parallel/test-vm-proxy-failure-CP.js",
    "test/parallel/test-vm-script-throw-in-tostring.js",
    "test/parallel/test-vm-set-property-proxy.js",
    "test/parallel/test-vm-set-proto-null-on-globalthis.js",
    "test/parallel/test-vm-source-map-url.js",
    "test/parallel/test-vm-static-this.js",
    "test/parallel/test-vm-strict-assign.js",
    "test/parallel/test-vm-symbols.js",
    "test/parallel/test-vm-timeout-escape-promise-2.js",
    "test/parallel/test-vm-timeout-escape-promise.js",
    "test/parallel/test-vm-timeout.js",
    "test/parallel/test-vm-util-lazy-properties.js",
];

const LOADER_CONTEXT_VM_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-vm-context.js",
    "test/parallel/test-vm-global-contextual-store.js",
];

fn loader_context_vm_promoted_fixture_paths(groups: &[&[&str]]) -> Vec<String> {
    groups
        .iter()
        .flat_map(|group| group.iter().copied())
        .map(str::to_string)
        .collect()
}

#[test]
fn node22_supported_lane_executes_loader_context_vm_promoted_batch_fixture() {
    let fixture_paths =
        loader_context_vm_promoted_fixture_paths(&[LOADER_CONTEXT_VM_PROMOTED_COMMON_PATHS]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-loader-context-vm-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        LOADER_CONTEXT_VM_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_loader_context_vm_promoted_batch_fixture() {
    let fixture_paths = loader_context_vm_promoted_fixture_paths(&[
        LOADER_CONTEXT_VM_PROMOTED_COMMON_PATHS,
        LOADER_CONTEXT_VM_PROMOTED_NODE24_ONLY_PATHS,
    ]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-loader-context-vm-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        LOADER_CONTEXT_VM_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked loader-context/vm required-gap inventory; keep ignored until root-cause clusters are fixed or precisely classified"]
fn node22_supported_lane_loader_context_vm_watchpoint() {
    let fixture_paths = loader_context_vm_runnable_fixture_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-loader-context-vm-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        LOADER_CONTEXT_VM_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked loader-context/vm required-gap inventory; keep ignored until root-cause clusters are fixed or precisely classified"]
fn node24_default_lane_loader_context_vm_watchpoint() {
    let fixture_paths = loader_context_vm_runnable_fixture_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-loader-context-vm-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        LOADER_CONTEXT_VM_EXTRA_DIRS,
    );
}

const LOADER_CONTEXT_DOMAIN_EXTRA_DIRS: &[&str] = &["test/common", "test/fixtures/keys"];

const LOADER_CONTEXT_DOMAIN_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-domain-crypto.js",
    "test/parallel/test-domain-error-types.js",
    "test/parallel/test-domain-fs-enoent-stream.js",
    "test/parallel/test-domain-http-server.js",
    "test/parallel/test-domain-implicit-fs.js",
    "test/parallel/test-domain-multi.js",
    "test/parallel/test-domain-nested-throw.js",
    "test/parallel/test-domain-safe-exit.js",
    "test/parallel/test-domain-stack.js",
    "test/parallel/test-domain-thrown-error-handler-stack.js",
    "test/parallel/test-domain-timers-uncaught-exception.js",
    "test/parallel/test-domain-top-level-error-handler-clears-stack.js",
    "test/parallel/test-domain-vm-promise-isolation.js",
];

fn loader_context_domain_required_fixture_paths(lane: NodeCompatLane) -> Vec<String> {
    node_compat_required_gap_paths_for_owner(lane, "loader-context/domain")
}

#[test]
fn node22_supported_lane_executes_loader_context_domain_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = LOADER_CONTEXT_DOMAIN_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-loader-context-domain-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        LOADER_CONTEXT_DOMAIN_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_loader_context_domain_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = LOADER_CONTEXT_DOMAIN_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-loader-context-domain-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        LOADER_CONTEXT_DOMAIN_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked loader-context/domain required-gap inventory; keep ignored until root-cause clusters are fixed or precisely classified"]
fn node22_supported_lane_loader_context_domain_watchpoint() {
    let fixture_paths = loader_context_domain_required_fixture_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-loader-context-domain-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        LOADER_CONTEXT_DOMAIN_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked loader-context/domain required-gap inventory; keep ignored until root-cause clusters are fixed or precisely classified"]
fn node24_default_lane_loader_context_domain_watchpoint() {
    let fixture_paths = loader_context_domain_required_fixture_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-loader-context-domain-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        LOADER_CONTEXT_DOMAIN_EXTRA_DIRS,
    );
}

#[test]
fn node22_loader_context_followup_v8_helper_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-v8-helper-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_V8_HELPER_BATCH,
    );
}

#[test]
fn node20_loader_context_followup_v8_helper_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-followup-v8-helper-batch",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_FOLLOWUP_V8_HELPER_BATCH,
    );
}

#[test]
fn node24_loader_context_followup_v8_helper_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-followup-v8-helper-batch",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_FOLLOWUP_V8_HELPER_BATCH,
    );
}

#[test]
fn node22_loader_context_followup_v8_green_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-v8-green-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_V8_GREEN_BATCH,
    );
}

#[test]
fn node20_loader_context_followup_v8_green_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-followup-v8-green-batch",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_FOLLOWUP_V8_GREEN_BATCH,
    );
}

#[test]
fn node24_loader_context_followup_v8_green_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-followup-v8-green-batch",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_FOLLOWUP_V8_GREEN_BATCH,
    );
}

#[test]
#[ignore = "Pinned V8 wire-format boundary: Nimbus runs on the v8_deno_core V8 build, so Node's exact serialized-byte fixture remains a platform boundary even though the functional v8 helper subset is green"]
fn node24_loader_context_v8_serdes_wire_format_watchpoint() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-v8-serdes.js",
        "node24/test/parallel/test-v8-serdes.js",
        &[],
        NodeCompatLane::Node24,
    );
}

#[test]
fn node22_loader_context_followup_vm_basic_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-vm-basic-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_VM_BASIC_BATCH,
    );
}

#[test]
fn node20_loader_context_followup_vm_basic_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-followup-vm-basic-batch",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_FOLLOWUP_VM_BASIC_BATCH,
    );
}

#[test]
fn node24_loader_context_followup_vm_basic_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-followup-vm-basic-batch",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_FOLLOWUP_VM_BASIC_BATCH,
    );
}

#[test]
fn node22_node_tools_domain_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-domain-foundation-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_DOMAIN_FOUNDATION_BATCH,
    );
}

#[test]
fn node20_node_tools_domain_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-node-tools-domain-foundation-batch",
        NodeCompatLane::Node20,
        NODE_TOOLS_DOMAIN_FOUNDATION_BATCH,
    );
}

#[test]
fn node24_node_tools_domain_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-node-tools-domain-foundation-batch",
        NodeCompatLane::Node24,
        NODE_TOOLS_DOMAIN_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_node_tools_domain_promise_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-domain-promise.js",
        "node22/test/parallel/test-domain-promise.js",
        &[],
    );
}

#[test]
fn node22_node_tools_constants_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-constants-foundation-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_CONSTANTS_FOUNDATION_BATCH,
    );
}

#[test]
fn node20_node_tools_constants_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-node-tools-constants-foundation-batch",
        NodeCompatLane::Node20,
        NODE_TOOLS_CONSTANTS_FOUNDATION_BATCH,
    );
}

#[test]
fn node24_node_tools_constants_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-node-tools-constants-foundation-batch",
        NodeCompatLane::Node24,
        NODE_TOOLS_CONSTANTS_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_node_tools_trace_events_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-trace-events-foundation-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_TRACE_EVENTS_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_node_tools_sys_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-sys-foundation-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_SYS_FOUNDATION_BATCH,
    );
}

#[test]
fn node20_node_tools_sys_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-node-tools-sys-foundation-batch",
        NodeCompatLane::Node20,
        NODE_TOOLS_SYS_FOUNDATION_BATCH,
    );
}

#[test]
fn node24_node_tools_sys_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-node-tools-sys-foundation-batch",
        NodeCompatLane::Node24,
        NODE_TOOLS_SYS_FOUNDATION_BATCH,
    );
}
