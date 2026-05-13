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
fn node22_loader_context_followup_module_commonjs_remainder_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-followup-module-commonjs-remainder-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_FOLLOWUP_MODULE_COMMONJS_REMAINDER_BATCH,
    );
}

#[test]
fn node20_loader_context_followup_module_commonjs_remainder_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-followup-module-commonjs-remainder-batch",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_FOLLOWUP_MODULE_COMMONJS_REMAINDER_BATCH,
    );
}

#[test]
fn node24_loader_context_followup_module_commonjs_remainder_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-followup-module-commonjs-remainder-batch",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_FOLLOWUP_MODULE_COMMONJS_REMAINDER_BATCH,
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

