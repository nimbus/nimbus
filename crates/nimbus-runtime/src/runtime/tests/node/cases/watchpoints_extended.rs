#[test]
fn node22_node_tools_sqlite_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-sqlite-foundation-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_SQLITE_FOUNDATION_BATCH,
    );
}

#[test]
#[ignore = "Pinned node-tools sqlite build-preset watchpoint: test-sqlite.js now narrows to the bundled percentile capability seam because the current bundled SQLCipher sqlite source does not expose percentile() even after the Node-style URI/path and SQLTagStore fixes"]
fn node22_node_tools_sqlite_build_preset_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-sqlite.js",
        "test/parallel/test-sqlite.js",
        NODE_TOOLS_SQLITE_NEXT_DB_EXTRA_FILES,
    );
}

#[test]
fn node22_node_tools_wasi_validation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-wasi-validation-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_WASI_VALIDATION_BATCH,
    );
}

#[test]
fn node22_node_tools_wasi_execution_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-wasi-execution-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_WASI_EXECUTION_BATCH,
    );
}

#[test]
fn node22_node_tools_wasi_filesystem_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-wasi-filesystem-foundation-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_WASI_FILESYSTEM_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_node_tools_wasi_preopen_io_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-wasi-preopen-io-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_WASI_PREOPEN_IO_BATCH,
    );
}

#[test]
fn node22_node_tools_wasi_io_subcase_watchpoint_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-wasi-io-subcase-watchpoint-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_WASI_IO_SUBCASE_WATCHPOINT_BATCH,
    );
}

#[test]
fn node22_node_tools_sea_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-sea-foundation-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_SEA_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_node_tools_repl_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-repl-foundation-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_REPL_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_node_tools_test_runner_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-test-runner-foundation-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_TEST_RUNNER_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_node_tools_test_runner_context_metadata_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-test-runner-context-metadata-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_TEST_RUNNER_CONTEXT_METADATA_BATCH,
    );
}

#[test]
fn node22_node_tools_test_runner_run_event_metadata_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-test-runner-run-event-metadata-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_TEST_RUNNER_RUN_EVENT_METADATA_BATCH,
    );
}

#[test]
fn node22_node_tools_test_runner_option_validation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-test-runner-option-validation-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_TEST_RUNNER_OPTION_VALIDATION_BATCH,
    );
}

#[test]
fn node22_node_tools_test_runner_plan_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-test-runner-plan-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_TEST_RUNNER_PLAN_BATCH,
    );
}

#[test]
fn node22_node_tools_test_runner_run_edge_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-test-runner-run-edge-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_TEST_RUNNER_RUN_EDGE_BATCH,
    );
}

#[test]
fn node22_node_tools_test_runner_reporters_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-test-runner-reporters-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_TEST_RUNNER_REPORTERS_BATCH,
    );
}

#[test]
fn node22_node_tools_test_runner_reporter_output_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-test-runner-reporter-output-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_TEST_RUNNER_REPORTER_OUTPUT_BATCH,
    );
}

#[test]
fn node22_node_tools_test_runner_cli_options_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-test-runner-cli-options-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_TEST_RUNNER_CLI_OPTIONS_BATCH,
    );
}

#[test]
fn node22_node_tools_test_runner_cli_randomize_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-test-runner-cli-randomize-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_TEST_RUNNER_CLI_RANDOMIZE_BATCH,
    );
}

#[test]
fn node22_node_tools_test_runner_cli_rerun_failures_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-test-runner-cli-rerun-failures-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_TEST_RUNNER_CLI_RERUN_FAILURES_BATCH,
    );
}

#[test]
fn node22_node_tools_cluster_worker_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-cluster-worker-foundation-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_CLUSTER_WORKER_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_node_tools_cluster_worker_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-node-tools-cluster-worker-lifecycle-batch",
        NodeCompatLane::Node22,
        NODE_TOOLS_CLUSTER_WORKER_LIFECYCLE_BATCH,
    );
}

#[test]
fn node22_node_tools_trace_events_category_used_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-trace-events-category-used.js",
        "node22/test/parallel/test-trace-events-category-used.js",
        &[],
    );
}

#[test]
fn node22_node_tools_trace_events_dynamic_enable_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-trace-events-dynamic-enable.js",
        "node22/test/parallel/test-trace-events-dynamic-enable.js",
        &[],
    );
}

#[test]
fn node22_loader_context_zlib_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-zlib-foundation-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_ZLIB_FOUNDATION_BATCH,
    );
}

#[test]
fn node20_loader_context_zlib_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-zlib-foundation-batch",
        NodeCompatLane::Node20,
        NODE22_LOADER_CONTEXT_ZLIB_FOUNDATION_BATCH,
    );
}

#[test]
fn node24_loader_context_zlib_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-zlib-foundation-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_ZLIB_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_loader_context_zlib_stream_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-zlib-stream-lifecycle-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_ZLIB_STREAM_LIFECYCLE_BATCH,
    );
}

#[test]
fn node20_loader_context_zlib_stream_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-zlib-stream-lifecycle-batch",
        NodeCompatLane::Node20,
        NODE22_LOADER_CONTEXT_ZLIB_STREAM_LIFECYCLE_BATCH,
    );
}

#[test]
fn node24_loader_context_zlib_stream_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-zlib-stream-lifecycle-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_ZLIB_STREAM_LIFECYCLE_BATCH,
    );
}

#[test]
fn node22_loader_context_zlib_decompression_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-zlib-decompression-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_ZLIB_DECOMPRESSION_BATCH,
    );
}

#[test]
fn node20_loader_context_zlib_decompression_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-zlib-decompression-batch",
        NodeCompatLane::Node20,
        NODE22_LOADER_CONTEXT_ZLIB_DECOMPRESSION_BATCH,
    );
}

#[test]
fn node24_loader_context_zlib_decompression_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-zlib-decompression-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_ZLIB_DECOMPRESSION_BATCH,
    );
}

#[test]
fn node22_loader_context_zlib_brotli_and_control_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-zlib-brotli-and-control-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_ZLIB_BROTLI_AND_CONTROL_BATCH,
    );
}

#[test]
fn node20_loader_context_zlib_brotli_and_control_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-zlib-brotli-and-control-batch",
        NodeCompatLane::Node20,
        NODE22_LOADER_CONTEXT_ZLIB_BROTLI_AND_CONTROL_BATCH,
    );
}

#[test]
fn node24_loader_context_zlib_brotli_and_control_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-zlib-brotli-and-control-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_ZLIB_BROTLI_AND_CONTROL_BATCH,
    );
}

#[test]
fn node22_loader_context_crypto_hash_random_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-crypto-hash-random-foundation-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_CRYPTO_HASH_RANDOM_FOUNDATION_BATCH,
    );
}

#[test]
fn node20_loader_context_crypto_hash_random_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-crypto-hash-random-foundation-batch",
        NodeCompatLane::Node20,
        NODE22_LOADER_CONTEXT_CRYPTO_HASH_RANDOM_FOUNDATION_BATCH,
    );
}

#[test]
fn node24_loader_context_crypto_hash_random_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-crypto-hash-random-foundation-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_CRYPTO_HASH_RANDOM_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_loader_context_crypto_kdf_and_stream_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-crypto-kdf-and-stream-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_CRYPTO_KDF_AND_STREAM_BATCH,
    );
}

#[test]
fn node20_loader_context_crypto_kdf_and_stream_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-crypto-kdf-and-stream-batch",
        NodeCompatLane::Node20,
        NODE22_LOADER_CONTEXT_CRYPTO_KDF_AND_STREAM_BATCH,
    );
}

#[test]
fn node24_loader_context_crypto_kdf_and_stream_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-crypto-kdf-and-stream-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_CRYPTO_KDF_AND_STREAM_BATCH,
    );
}

#[test]
fn node22_loader_context_crypto_cipher_and_padding_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-crypto-cipher-and-padding-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_CRYPTO_CIPHER_AND_PADDING_BATCH,
    );
}

#[test]
fn node20_loader_context_crypto_cipher_and_padding_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-crypto-cipher-and-padding-batch",
        NodeCompatLane::Node20,
        NODE22_LOADER_CONTEXT_CRYPTO_CIPHER_AND_PADDING_BATCH,
    );
}

#[test]
fn node24_loader_context_crypto_cipher_and_padding_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-crypto-cipher-and-padding-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_CRYPTO_CIPHER_AND_PADDING_BATCH,
    );
}

#[test]
fn node22_loader_context_crypto_dh_and_ecdh_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-crypto-dh-and-ecdh-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_CRYPTO_DH_AND_ECDH_BATCH,
    );
}

#[test]
fn node20_loader_context_crypto_dh_and_ecdh_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-crypto-dh-and-ecdh-batch",
        NodeCompatLane::Node20,
        NODE20_LOADER_CONTEXT_CRYPTO_DH_AND_ECDH_BATCH,
    );
}

#[test]
fn node24_loader_context_crypto_dh_and_ecdh_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-crypto-dh-and-ecdh-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_CRYPTO_DH_AND_ECDH_BATCH,
    );
}

#[test]
fn node22_loader_context_crypto_dh_safe_prime_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-crypto-dh-safe-prime-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_CRYPTO_DH_SAFE_PRIME_BATCH,
    );
}

#[test]
fn node20_loader_context_crypto_dh_safe_prime_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-crypto-dh-safe-prime-batch",
        NodeCompatLane::Node20,
        NODE20_LOADER_CONTEXT_CRYPTO_DH_SAFE_PRIME_BATCH,
    );
}

#[test]
fn node22_loader_context_crypto_dh_curves_and_stateless_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-crypto-dh-curves-and-stateless-batch",
        NodeCompatLane::Node22,
        NODE22_LOADER_CONTEXT_CRYPTO_DH_CURVES_AND_STATELESS_BATCH,
    );
}

#[test]
fn node20_loader_context_crypto_dh_curves_and_stateless_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-crypto-dh-curves-and-stateless-batch",
        NodeCompatLane::Node20,
        NODE22_LOADER_CONTEXT_CRYPTO_DH_CURVES_AND_STATELESS_BATCH,
    );
}

#[test]
fn node24_loader_context_crypto_dh_curves_and_stateless_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-crypto-dh-curves-and-stateless-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_CRYPTO_DH_CURVES_AND_STATELESS_BATCH,
    );
}

#[test]
fn node24_loader_context_crypto_dh_safe_prime_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-crypto-dh-safe-prime-batch",
        NodeCompatLane::Node24,
        NODE22_LOADER_CONTEXT_CRYPTO_DH_SAFE_PRIME_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node20 legacy-lane divergence: test-crypto-dh.js still expects the older OpenSSL invalid-secret message while the verified Node22 baseline now returns the newer unspecified-validation shape"]
fn node20_loader_context_crypto_dh_legacy_watchpoint_batch() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-crypto-dh-legacy-watchpoints",
        NodeCompatLane::Node20,
        NODE20_LOADER_CONTEXT_CRYPTO_DH_SUPPORTED_WATCHPOINT_BATCH,
    );
}

#[test]
#[ignore = "Pinned Deno-family crypto gap: authenticated-stream and DES3 wrap fixtures require cipher families not exposed by the embedded crypto backend yet"]
fn node22_loader_context_crypto_authenticated_and_aes_wrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-crypto-authenticated-and-aes-wrap-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_CRYPTO_AUTHENTICATED_AND_AES_WRAP_BATCH,
    );
}

#[test]
#[ignore = "Pinned Deno-family crypto gap: authenticated-stream and DES3 wrap fixtures require cipher families not exposed by the embedded crypto backend yet"]
fn node20_loader_context_crypto_authenticated_and_aes_wrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-crypto-authenticated-and-aes-wrap-batch",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_CRYPTO_AUTHENTICATED_AND_AES_WRAP_BATCH,
    );
}

#[test]
#[ignore = "Pinned Deno-family crypto gap: authenticated-stream/authenticated error text and DES3 wrap fixtures require cipher behavior not exposed by the embedded crypto backend yet"]
fn node24_loader_context_crypto_authenticated_and_aes_wrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-crypto-authenticated-and-aes-wrap-batch",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_CRYPTO_AUTHENTICATED_AND_AES_WRAP_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node20 legacy-lane divergence: test-crypto-authenticated.js still expects the older deprecation-warning ordering without DEP0182"]
fn node20_loader_context_crypto_authenticated_legacy_watchpoint_batch() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-crypto-authenticated-legacy-watchpoints",
        NodeCompatLane::Node20,
        NODE20_LOADER_CONTEXT_CRYPTO_AUTHENTICATED_SUPPORTED_WATCHPOINT_BATCH,
    );
}

#[test]
fn node20_loader_context_crypto_xof_extension_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-crypto-xof-extension-batch",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_CRYPTO_XOF_EXTENSION_BATCH,
    );
}

#[test]
fn node24_loader_context_crypto_xof_extension_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-crypto-xof-extension-batch",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_CRYPTO_XOF_EXTENSION_BATCH,
    );
}

#[test]
fn node24_https_hwm_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-https-hwm.js",
        "node24/test/parallel/test-https-hwm.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned Node20 legacy-lane divergence: test-https-hwm.js still times out on the current Node20 lane while the Node22/Node24 official files complete"]
fn node20_https_hwm_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-https-hwm.js",
        "node20/test/parallel/test-https-hwm.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned Node20 legacy-lane divergence: test-tls-connect-hwm-option.js still times out on the current Node20 lane while the Node22/Node24 official files complete"]
fn node20_tls_connect_hwm_option_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-tls-connect-hwm-option.js",
        "node20/test/parallel/test-tls-connect-hwm-option.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned networking host/preset boundary batch: these https files currently stop at explicit local-address or IPv6 capability boundaries rather than plain HTTPS semantics"]
fn node22_networking_https_address_boundary_batch_watchpoint() {
    run_node_compat_watchpoint_batch(
        "node22-networking-https-address-boundary-batch",
        "node22",
        NODE22_NETWORKING_HTTPS_ADDRESS_BOUNDARY_FIXTURES,
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned networking cross-family boundary batch: these dgram files currently depend on cluster/child-process script-path behavior rather than plain UDP runtime semantics"]
fn node22_networking_dgram_cluster_boundary_batch_watchpoint() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-cluster-boundary-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_CLUSTER_BOUNDARY_FIXTURES,
        &[],
    );
}

#[test]
#[ignore = "Pinned networking host/preset boundary batch: these dgram files currently depend on external-net or IPv6 capability beyond the current application preset"]
fn node22_networking_dgram_host_preset_boundary_batch_watchpoint() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-host-preset-boundary-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_HOST_PRESET_BOUNDARY_FIXTURES,
        &[],
    );
}

#[test]
#[ignore = "Pinned networking dgram watchpoint: test-dgram-reuseport.js now materializes ../common/udp but blocks in reusePort bind/lifecycle semantics, so it stays explicit until that owner seam is fixed"]
fn node22_dgram_reuseport_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-dgram-reuseport.js",
        "node22/test/parallel/test-dgram-reuseport.js",
        NODE22_COMMON_UDP_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned networking cross-family watchpoint: test-http-agent-reuse-drained-socket-only.js currently blocks in process.report.getReport() and then reaches process.exit(), so it stays explicit as a process/report and embedded-exit dependency rather than a pure http.Agent seam"]
fn node22_http_agent_reuse_drained_socket_only_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-http-agent-reuse-drained-socket-only.js",
        "node22/test/parallel/test-http-agent-reuse-drained-socket-only.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned networking/loader-context boundary watchpoint: test-https-agent-additional-options.js currently reaches the legacy TLSv1.1 secureProtocol path (TLSv1_1_method / minVersion TLSv1.1) that the current rustls-backed TLS owner layer does not negotiate"]
fn node22_https_agent_additional_options_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-https-agent-additional-options.js",
        "node22/test/parallel/test-https-agent-additional-options.js",
        COMMON_TLS_KEY_EXTRA_FILES,
    );
}

#[test]
fn node24_net_connect_abort_controller_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-net-connect-abort-controller.js",
        "node24/test/parallel/test-net-connect-abort-controller.js",
        &[],
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_http_agent_abort_controller_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-http-agent-abort-controller.js",
        "node24/test/parallel/test-http-agent-abort-controller.js",
        COMMON_COUNTDOWN_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_http_response_statuscode_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-http-response-statuscode.js",
        "node24/test/parallel/test-http-response-statuscode.js",
        COMMON_COUNTDOWN_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_http_response_splitting_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-http-response-splitting.js",
        "node24/test/parallel/test-http-response-splitting.js",
        COMMON_COUNTDOWN_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_http2_util_update_options_buffer_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-http2-util-update-options-buffer.js",
        "node24/test/parallel/test-http2-util-update-options-buffer.js",
        &[],
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_https_agent_abort_controller_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-https-agent-abort-controller.js",
        "node24/test/parallel/test-https-agent-abort-controller.js",
        COMMON_TLS_KEY_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_https_abortcontroller_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-https-abortcontroller.js",
        "node24/test/parallel/test-https-abortcontroller.js",
        COMMON_TLS_KEY_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_https_client_get_url_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-https-client-get-url.js",
        "node24/test/parallel/test-https-client-get-url.js",
        COMMON_TLS_KEY_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_https_strict_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-https-strict.js",
        "node24/test/parallel/test-https-strict.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_https_pfx_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-https-pfx.js",
        "node24/test/parallel/test-https-pfx.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_https_agent_keylog_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-https-agent-keylog.js",
        "node24/test/parallel/test-https-agent-keylog.js",
        COMMON_TLS_KEY_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_https_agent_sni_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-https-agent-sni.js",
        "node24/test/parallel/test-https-agent-sni.js",
        COMMON_TLS_KEY_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_https_client_override_global_agent_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-https-client-override-global-agent.js",
        "node24/test/parallel/test-https-client-override-global-agent.js",
        COMMON_TLS_KEY_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_https_resume_after_renew_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-https-resume-after-renew.js",
        "node24/test/parallel/test-https-resume-after-renew.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_tls_connect_abort_controller_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-tls-connect-abort-controller.js",
        "node24/test/parallel/test-tls-connect-abort-controller.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

const PROCESS_TIMERS_EXTRA_RUNTIME_FILES: &[&str] = &[
    "test/async-hooks/hook-checks.js",
    "test/async-hooks/init-hooks.js",
];

const PROCESS_TIMERS_EXTRA_DIRS: &[&str] = &["test/common"];

const PROCESS_TIMING_RUNTIME_RESIDUAL_OWNERS: &[&str] = &[
    "process-and-timing/timers",
    "process-and-timing/diagnostics-channel",
    "process-and-timing/perf-hooks",
    "process-and-timing/os",
];

const PROCESS_TIMING_RUNTIME_RESIDUAL_PREFIXES: &[&str] = &[
    "test/parallel/test-promise-",
    "test/parallel/test-promises-",
    "test/parallel/test-track-promises-",
    "test/parallel/test-queue-microtask",
    "test/parallel/test-abortcontroller",
    "test/parallel/test-aborted-util",
    "test/parallel/test-trace-events-",
];

const PROCESS_TIMING_RUNTIME_RESIDUAL_EXTRA_DIRS: &[&str] = &["test/common"];

fn process_timing_runtime_residual_fixture_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths: Vec<String> = PROCESS_TIMING_RUNTIME_RESIDUAL_OWNERS
        .iter()
        .flat_map(|owner| node_compat_required_gap_paths_for_owner(lane, owner))
        .collect();
    fixture_paths.extend(node_compat_required_gap_paths_for_selector(lane, |path| {
        PROCESS_TIMING_RUNTIME_RESIDUAL_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix))
    }));
    fixture_paths.sort();
    fixture_paths.dedup();
    fixture_paths
}

fn process_timers_runnable_fixture_paths(lane: NodeCompatLane) -> Vec<String> {
    node_compat_required_gap_paths_for_owner(lane, "process-and-timing/timers")
}

const PROCESS_TIMERS_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-timers-clearImmediate-als.js",
    "test/parallel/test-timers-destroyed.js",
    "test/parallel/test-timers-dispose.js",
    "test/parallel/test-timers-immediate-queue.js",
    "test/parallel/test-timers-interval-throw.js",
    "test/parallel/test-timers-nested.js",
    "test/parallel/test-timers-next-tick.js",
    "test/parallel/test-timers-ordering.js",
    "test/parallel/test-timers-promises-scheduler.js",
    "test/parallel/test-timers-promises.js",
    "test/parallel/test-timers-refresh-in-callback.js",
    "test/parallel/test-timers-refresh.js",
    "test/parallel/test-timers-same-timeout-wrong-list-deleted.js",
    "test/parallel/test-timers-setimmediate-infinite-loop.js",
    "test/parallel/test-timers-timeout-to-interval.js",
    "test/parallel/test-timers-timeout-with-non-integer.js",
    "test/parallel/test-timers-to-primitive.js",
    "test/parallel/test-timers-uncaught-exception.js",
    "test/parallel/test-timers-unref-throw-then-ref.js",
    "test/parallel/test-timers-unref.js",
    "test/parallel/test-timers-unrefd-interval-still-fires.js",
    "test/parallel/test-timers-unrefed-in-callback.js",
    "test/parallel/test-timers-user-call.js",
];

const PROCESS_TIMERS_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-timers-invalid-clear.js",
    "test/parallel/test-timers-max-duration-warning.js",
    "test/parallel/test-timers-nan-duration-emit-once-per-process.js",
    "test/parallel/test-timers-nan-duration-warning-promises.js",
    "test/parallel/test-timers-nan-duration-warning.js",
    "test/parallel/test-timers-negative-duration-warning-emit-once-per-process.js",
    "test/parallel/test-timers-negative-duration-warning.js",
    "test/parallel/test-timers-not-emit-duration-zero.js",
    "test/parallel/test-timers-unenroll-unref-interval.js",
];

const PROCESS_TIMERS_PROMOTED_NODE26_PATHS: &[&str] = &[
    "test/parallel/test-timers-clearImmediate-als.js",
    "test/parallel/test-timers-destroyed.js",
    "test/parallel/test-timers-dispose.js",
    "test/parallel/test-timers-immediate-queue-throw.js",
    "test/parallel/test-timers-immediate-queue.js",
    "test/parallel/test-timers-immediate-unref-nested-once.js",
    "test/parallel/test-timers-immediate-unref-simple.js",
    "test/parallel/test-timers-immediate-unref.js",
    "test/parallel/test-timers-interval-throw.js",
    "test/parallel/test-timers-invalid-clear.js",
    "test/parallel/test-timers-max-duration-warning.js",
    "test/parallel/test-timers-nan-duration-emit-once-per-process.js",
    "test/parallel/test-timers-nan-duration-warning-promises.js",
    "test/parallel/test-timers-nan-duration-warning.js",
    "test/parallel/test-timers-negative-duration-warning-emit-once-per-process.js",
    "test/parallel/test-timers-negative-duration-warning.js",
    "test/parallel/test-timers-nested.js",
    "test/parallel/test-timers-next-tick.js",
    "test/parallel/test-timers-not-emit-duration-zero.js",
    "test/parallel/test-timers-ordering.js",
    "test/parallel/test-timers-process-tampering.js",
    "test/parallel/test-timers-promises-scheduler.js",
    "test/parallel/test-timers-promises.js",
    "test/parallel/test-timers-refresh-in-callback.js",
    "test/parallel/test-timers-refresh.js",
    "test/parallel/test-timers-reset-process-domain-on-throw.js",
    "test/parallel/test-timers-same-timeout-wrong-list-deleted.js",
    "test/parallel/test-timers-setimmediate-infinite-loop.js",
    "test/parallel/test-timers-timeout-to-interval.js",
    "test/parallel/test-timers-timeout-with-non-integer.js",
    "test/parallel/test-timers-to-primitive.js",
    "test/parallel/test-timers-uncaught-exception.js",
    "test/parallel/test-timers-unenroll-unref-interval.js",
    "test/parallel/test-timers-unref-throw-then-ref.js",
    "test/parallel/test-timers-unref.js",
    "test/parallel/test-timers-unrefd-interval-still-fires.js",
    "test/parallel/test-timers-unrefed-in-beforeexit.js",
    "test/parallel/test-timers-unrefed-in-callback.js",
    "test/parallel/test-timers-user-call.js",
];

const PROCESS_TIMING_RUNTIME_RESIDUAL_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-perf-gc-crash.js",
    "test/parallel/test-promise-unhandled-error-with-reading-file.js",
    "test/parallel/test-promise-unhandled-error.js",
    "test/parallel/test-promise-unhandled-silent-no-hook.js",
    "test/parallel/test-promise-unhandled-warn-no-hook.js",
    "test/parallel/test-promise-unhandled-warn.js",
    "test/parallel/test-promises-unhandled-proxy-rejections.js",
    "test/parallel/test-promises-unhandled-rejections.js",
    "test/parallel/test-promises-unhandled-symbol-rejections.js",
    "test/parallel/test-promises-warning-on-unhandled-rejection.js",
    "test/parallel/test-timers-process-tampering.js",
    "test/parallel/test-trace-events-all.js",
    "test/parallel/test-trace-events-async-hooks.js",
    "test/parallel/test-trace-events-file-pattern.js",
    "test/parallel/test-trace-events-get-category-enabled-buffer.js",
    "test/parallel/test-trace-events-http.js",
    "test/parallel/test-trace-events-v8.js",
];

const PROCESS_TIMING_RUNTIME_RESIDUAL_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-perf-hooks-timerify-basic.js",
    "test/parallel/test-perf-hooks-timerify-constructor.js",
    "test/parallel/test-perf-hooks-timerify-error.js",
    "test/parallel/test-perf-hooks-timerify-histogram-async.mjs",
    "test/parallel/test-perf-hooks-timerify-invalid-args.js",
    "test/parallel/test-perf-hooks-timerify-multiple-wrapping.js",
    "test/parallel/test-perf-hooks-timerify-return-value.js",
    "test/parallel/test-trace-events-api.js",
    "test/parallel/test-trace-events-binding.js",
    "test/parallel/test-trace-events-bootstrap.js",
    "test/parallel/test-trace-events-category-used.js",
    "test/parallel/test-trace-events-console.js",
    "test/parallel/test-trace-events-environment.js",
    "test/parallel/test-trace-events-metadata.js",
    "test/parallel/test-trace-events-none.js",
    "test/parallel/test-trace-events-process-exit.js",
];

const PROCESS_TIMING_RUNTIME_RESIDUAL_PROMOTED_NODE26_EXTRA_PATHS: &[&str] = &[
    "test/parallel/test-perf-hooks-eventlooputilization.js",
    "test/parallel/test-perf-hooks-timerify-basic.js",
    "test/parallel/test-perf-hooks-timerify-constructor.js",
    "test/parallel/test-perf-hooks-timerify-error.js",
    "test/parallel/test-perf-hooks-timerify-histogram-async.mjs",
    "test/parallel/test-perf-hooks-timerify-histogram-sync.mjs",
    "test/parallel/test-perf-hooks-timerify-invalid-args.js",
    "test/parallel/test-perf-hooks-timerify-multiple-wrapping.js",
    "test/parallel/test-perf-hooks-timerify-return-value.js",
];

fn process_timers_promoted_fixture_paths(groups: &[&[&str]]) -> Vec<String> {
    groups
        .iter()
        .flat_map(|group| group.iter().copied())
        .map(str::to_string)
        .collect()
}

#[test]
fn node22_supported_lane_executes_process_timers_promoted_batch_fixture() {
    let fixture_paths =
        process_timers_promoted_fixture_paths(&[PROCESS_TIMERS_PROMOTED_COMMON_PATHS]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-process-timers-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        PROCESS_TIMERS_EXTRA_RUNTIME_FILES,
        PROCESS_TIMERS_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_process_timers_promoted_batch_fixture() {
    let fixture_paths = process_timers_promoted_fixture_paths(&[
        PROCESS_TIMERS_PROMOTED_COMMON_PATHS,
        PROCESS_TIMERS_PROMOTED_NODE24_ONLY_PATHS,
    ]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-process-timers-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        PROCESS_TIMERS_EXTRA_RUNTIME_FILES,
        PROCESS_TIMERS_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_process_timers_promoted_batch_fixture() {
    let fixture_paths =
        process_timers_promoted_fixture_paths(&[PROCESS_TIMERS_PROMOTED_NODE26_PATHS]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-process-timers-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        PROCESS_TIMERS_EXTRA_RUNTIME_FILES,
        PROCESS_TIMERS_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked process-and-timing/timers required-gap inventory; classify async-hooks, domain, beforeExit, and unref/ref root causes after the first wide run"]
fn node22_supported_lane_process_timers_watchpoint() {
    let fixture_paths = process_timers_runnable_fixture_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-process-timers-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        PROCESS_TIMERS_EXTRA_RUNTIME_FILES,
        PROCESS_TIMERS_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked process-and-timing/timers required-gap inventory; classify async-hooks, domain, beforeExit, and unref/ref root causes after the first wide run"]
fn node24_default_lane_process_timers_watchpoint() {
    let fixture_paths = process_timers_runnable_fixture_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-process-timers-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        PROCESS_TIMERS_EXTRA_RUNTIME_FILES,
        PROCESS_TIMERS_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 node26 broad pre-run: ROI-ranked process-and-timing/timers required-gap inventory; classify async-hooks, domain, beforeExit, and unref/ref root causes after the first wide run"]
fn node26_current_lane_process_timers_watchpoint() {
    let fixture_paths = process_timers_runnable_fixture_paths(NodeCompatLane::Node26);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-process-timers-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        PROCESS_TIMERS_EXTRA_RUNTIME_FILES,
        PROCESS_TIMERS_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_process_timing_runtime_residual_promoted_batch_fixture() {
    let fixture_paths = process_timers_promoted_fixture_paths(&[
        PROCESS_TIMING_RUNTIME_RESIDUAL_PROMOTED_COMMON_PATHS,
    ]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-process-timing-runtime-residual-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        PROCESS_TIMERS_EXTRA_RUNTIME_FILES,
        PROCESS_TIMING_RUNTIME_RESIDUAL_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_process_timing_runtime_residual_promoted_batch_fixture() {
    let fixture_paths = process_timers_promoted_fixture_paths(&[
        PROCESS_TIMING_RUNTIME_RESIDUAL_PROMOTED_COMMON_PATHS,
        PROCESS_TIMING_RUNTIME_RESIDUAL_PROMOTED_NODE24_ONLY_PATHS,
    ]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-process-timing-runtime-residual-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        PROCESS_TIMERS_EXTRA_RUNTIME_FILES,
        PROCESS_TIMING_RUNTIME_RESIDUAL_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_process_timing_runtime_residual_promoted_batch_fixture() {
    let fixture_paths = process_timers_promoted_fixture_paths(&[
        PROCESS_TIMING_RUNTIME_RESIDUAL_PROMOTED_COMMON_PATHS,
        PROCESS_TIMING_RUNTIME_RESIDUAL_PROMOTED_NODE26_EXTRA_PATHS,
    ]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-process-timing-runtime-residual-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        PROCESS_TIMERS_EXTRA_RUNTIME_FILES,
        PROCESS_TIMING_RUNTIME_RESIDUAL_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked process/timing runtime residual inventory; classify timers, diagnostics_channel, perf_hooks, promise tracking, trace_events, os, and microtask root causes before focused fixes"]
fn node22_supported_lane_process_timing_runtime_residual_watchpoint() {
    let fixture_paths =
        process_timing_runtime_residual_fixture_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-process-timing-runtime-residual-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        PROCESS_TIMERS_EXTRA_RUNTIME_FILES,
        PROCESS_TIMING_RUNTIME_RESIDUAL_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked process/timing runtime residual inventory; classify timers, diagnostics_channel, perf_hooks, promise tracking, trace_events, os, and microtask root causes before focused fixes"]
fn node24_default_lane_process_timing_runtime_residual_watchpoint() {
    let fixture_paths =
        process_timing_runtime_residual_fixture_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-process-timing-runtime-residual-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        PROCESS_TIMERS_EXTRA_RUNTIME_FILES,
        PROCESS_TIMING_RUNTIME_RESIDUAL_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 node26 broad pre-run: ROI-ranked process/timing runtime residual inventory; classify perf_hooks, trace_events, promise tracking, and os root causes before focused fixes"]
fn node26_current_lane_process_timing_runtime_residual_watchpoint() {
    let fixture_paths =
        process_timing_runtime_residual_fixture_paths(NodeCompatLane::Node26);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-process-timing-runtime-residual-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        PROCESS_TIMERS_EXTRA_RUNTIME_FILES,
        PROCESS_TIMING_RUNTIME_RESIDUAL_EXTRA_DIRS,
    );
}

const PROCESS_DIAGNOSTICS_CHANNEL_EXTRA_DIRS: &[&str] = &["test/common"];

fn process_diagnostics_channel_runnable_fixture_paths(lane: NodeCompatLane) -> Vec<String> {
    node_compat_required_gap_paths_for_owner(lane, "process-and-timing/diagnostics-channel")
}

const PROCESS_DIAGNOSTICS_CHANNEL_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-diagnostics-channel-bind-store.js",
    "test/parallel/test-diagnostics-channel-gc-maintains-subcriptions.js",
    "test/parallel/test-diagnostics-channel-gc-race-condition.js",
    "test/parallel/test-diagnostics-channel-http-server-start.js",
    "test/parallel/test-diagnostics-channel-http.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-body-multiple-buffers-and-strings.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-body-multiple-buffers.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-body-no-chunks.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-body-single-buffer.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-body-single-string.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-close-error.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-close.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-created.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-error.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-finish.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-start.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-close-error.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-close.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-created-start-timing.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-created.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-error.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-finish.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-start.js",
    "test/parallel/test-diagnostics-channel-memory-leak.js",
    "test/parallel/test-diagnostics-channel-module-import-error.js",
    "test/parallel/test-diagnostics-channel-module-import.js",
    "test/parallel/test-diagnostics-channel-module-require-error.js",
    "test/parallel/test-diagnostics-channel-module-require.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-args-types.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-callback-early-exit.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-callback-error.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-callback-run-stores.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-callback.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-has-subscribers.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-promise-early-exit.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-promise-error.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-promise-run-stores.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-promise-unhandled.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-promise.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-sync-early-exit.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-sync-error.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-sync-run-stores.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-sync.js",
];

const PROCESS_DIAGNOSTICS_CHANNEL_PROMOTED_NODE24_ONLY_PATHS: &[&str] =
    &["test/parallel/test-diagnostics-channel-web-locks.js"];

const PROCESS_DIAGNOSTICS_CHANNEL_PROMOTED_NODE26_PATHS: &[&str] = &[
    "test/parallel/test-diagnostics-channel-bind-store.js",
    "test/parallel/test-diagnostics-channel-bounded-channel-run-transform-error.js",
    "test/parallel/test-diagnostics-channel-bounded-channel-run.js",
    "test/parallel/test-diagnostics-channel-bounded-channel-scope-error.js",
    "test/parallel/test-diagnostics-channel-bounded-channel-scope-nested.js",
    "test/parallel/test-diagnostics-channel-bounded-channel-scope-transform-error.js",
    "test/parallel/test-diagnostics-channel-bounded-channel-scope.js",
    "test/parallel/test-diagnostics-channel-bounded-channel.js",
    "test/parallel/test-diagnostics-channel-gc-maintains-subcriptions.js",
    "test/parallel/test-diagnostics-channel-gc-race-condition.js",
    "test/parallel/test-diagnostics-channel-http-server-start.js",
    "test/parallel/test-diagnostics-channel-http.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-body-multiple-buffers-and-strings.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-body-multiple-buffers.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-body-no-chunks.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-body-single-buffer.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-body-single-string.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-close-error.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-close.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-created.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-error.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-finish.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-start.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-close-error.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-close.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-created-start-timing.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-created.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-error.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-finish.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-start.js",
    "test/parallel/test-diagnostics-channel-memory-leak.js",
    "test/parallel/test-diagnostics-channel-module-import-error.js",
    "test/parallel/test-diagnostics-channel-module-import.js",
    "test/parallel/test-diagnostics-channel-module-require-error.js",
    "test/parallel/test-diagnostics-channel-module-require.js",
    "test/parallel/test-diagnostics-channel-run-stores-scope-transform-error.js",
    "test/parallel/test-diagnostics-channel-run-stores-scope.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-args-types.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-callback-early-exit.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-callback-error.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-callback-run-stores.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-callback.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-has-subscribers.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-promise-early-exit.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-promise-error.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-promise-non-thenable.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-promise-run-stores.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-promise-thenable.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-promise-unhandled.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-promise.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-sync-early-exit.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-sync-error.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-sync-run-stores.js",
    "test/parallel/test-diagnostics-channel-tracing-channel-sync.js",
    "test/parallel/test-diagnostics-channel-web-locks.js",
];

#[test]
fn node22_supported_lane_executes_process_diagnostics_channel_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = PROCESS_DIAGNOSTICS_CHANNEL_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| path.to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-process-diagnostics-channel-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        PROCESS_DIAGNOSTICS_CHANNEL_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_process_diagnostics_channel_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = PROCESS_DIAGNOSTICS_CHANNEL_PROMOTED_COMMON_PATHS
        .iter()
        .chain(PROCESS_DIAGNOSTICS_CHANNEL_PROMOTED_NODE24_ONLY_PATHS.iter())
        .map(|path| path.to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-process-diagnostics-channel-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        PROCESS_DIAGNOSTICS_CHANNEL_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_process_diagnostics_channel_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = PROCESS_DIAGNOSTICS_CHANNEL_PROMOTED_NODE26_PATHS
        .iter()
        .map(|path| path.to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-process-diagnostics-channel-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        PROCESS_DIAGNOSTICS_CHANNEL_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked process-and-timing/diagnostics-channel required-gap inventory; classify async_hooks, subscriber lifecycle, http/http2/net instrumentation, and test-harness root causes after the first wide run"]
fn node22_supported_lane_process_diagnostics_channel_watchpoint() {
    let fixture_paths =
        process_diagnostics_channel_runnable_fixture_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-process-diagnostics-channel-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        PROCESS_DIAGNOSTICS_CHANNEL_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked process-and-timing/diagnostics-channel required-gap inventory; classify async_hooks, subscriber lifecycle, http/http2/net instrumentation, and test-harness root causes after the first wide run"]
fn node24_default_lane_process_diagnostics_channel_watchpoint() {
    let fixture_paths =
        process_diagnostics_channel_runnable_fixture_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-process-diagnostics-channel-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        PROCESS_DIAGNOSTICS_CHANNEL_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 node26 broad pre-run: ROI-ranked process-and-timing/diagnostics-channel required-gap inventory; classify async_hooks, subscriber lifecycle, http/http2/net instrumentation, and test-harness root causes after the first wide run"]
fn node26_current_lane_process_diagnostics_channel_watchpoint() {
    let fixture_paths =
        process_diagnostics_channel_runnable_fixture_paths(NodeCompatLane::Node26);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-process-diagnostics-channel-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        PROCESS_DIAGNOSTICS_CHANNEL_EXTRA_DIRS,
    );
}

const STREAMS_WEB_PLATFORM_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures", "test/fixtures/wpt"];

const STREAMS_WEB_PLATFORM_LOW_ROI_PATHS: &[&str] =
    &[
        "test/parallel/test-stream-base-typechecking.js",
        "test/parallel/test-webstreams-clone-unref.js",
        "test/parallel/test-whatwg-webstreams-transform-stream-members.js",
    ];

const STREAMS_WEB_PLATFORM_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/async-hooks/test-async-local-storage-stream-finished.js",
    "test/parallel/test-blob-createobjecturl.js",
    "test/parallel/test-blob-file-backed.js",
    "test/parallel/test-file-write-stream.js",
    "test/parallel/test-file-write-stream2.js",
    "test/parallel/test-file-write-stream3.js",
    "test/parallel/test-file-write-stream4.js",
    "test/parallel/test-filehandle-readablestream.js",
    "test/parallel/test-global-domexception.js",
    "test/parallel/test-global-setters.js",
    "test/parallel/test-js-stream-call-properties.js",
    "test/parallel/test-stream-iterator-helpers-test262-tests.mjs",
    "test/parallel/test-stream-readable-async-iterators.js",
    "test/parallel/test-stream-readable-to-web.mjs",
    "test/parallel/test-stream-readableListening-state.js",
    "test/parallel/test-stream-some-find-every.mjs",
    "test/parallel/test-stream-toWeb-allows-server-response.js",
    "test/parallel/test-stream-transform-destroy.js",
    "test/parallel/test-stream-wrap-drain.js",
    "test/parallel/test-stream-wrap-encoding.js",
    "test/parallel/test-stream-wrap.js",
    "test/parallel/test-stream2-base64-single-char-read-end.js",
    "test/parallel/test-stream2-basic.js",
    "test/parallel/test-stream2-compatibility.js",
    "test/parallel/test-stream2-decode-partial.js",
    "test/parallel/test-stream2-httpclient-response-end.js",
    "test/parallel/test-stream2-large-read-stall.js",
    "test/parallel/test-stream2-objects.js",
    "test/parallel/test-stream2-push.js",
    "test/parallel/test-stream2-read-correct-num-bytes-in-utf8.js",
    "test/parallel/test-stream2-read-sync-stack.js",
    "test/parallel/test-stream2-readable-empty-buffer-no-eof.js",
    "test/parallel/test-stream2-readable-legacy-drain.js",
    "test/parallel/test-stream2-readable-non-empty-end.js",
    "test/parallel/test-stream2-readable-wrap-destroy.js",
    "test/parallel/test-stream2-readable-wrap-empty.js",
    "test/parallel/test-stream2-readable-wrap-error.js",
    "test/parallel/test-stream2-readable-wrap.js",
    "test/parallel/test-stream2-set-encoding.js",
    "test/parallel/test-stream2-transform.js",
    "test/parallel/test-stream2-writable.js",
    "test/parallel/test-stream3-cork-end.js",
    "test/parallel/test-stream3-cork-uncork.js",
    "test/parallel/test-stream3-pause-then-read.js",
    "test/parallel/test-streams-highwatermark.js",
    "test/parallel/test-webstream-encoding-inspect.js",
    "test/parallel/test-webstream-readable-from.js",
    "test/parallel/test-webstream-string-tag.js",
    "test/parallel/test-webstream-structured-clone-no-leftovers.mjs",
    "test/parallel/test-webstreams-abort-controller.js",
    "test/parallel/test-webstreams-compose.js",
    "test/parallel/test-webstreams-finished.js",
    "test/parallel/test-whatwg-encoding-custom-fatal-streaming.js",
    "test/parallel/test-whatwg-encoding-custom-textdecoder-api-invalid-label.js",
    "test/parallel/test-whatwg-encoding-custom-textdecoder-fatal.js",
    "test/parallel/test-whatwg-encoding-custom-textdecoder-invalid-arg.js",
    "test/parallel/test-whatwg-encoding-custom-textdecoder-utf16-surrogates.js",
    "test/parallel/test-whatwg-url-custom-inspect.js",
    "test/parallel/test-whatwg-url-custom-parsing.js",
    "test/parallel/test-whatwg-url-custom-properties.js",
    "test/parallel/test-whatwg-url-invalidthis.js",
    "test/parallel/test-whatwg-webstreams-compression.js",
    "test/parallel/test-whatwg-webstreams-encoding.js",
    "test/parallel/test-wrap-js-stream-destroy.js",
    "test/parallel/test-wrap-js-stream-duplex.js",
    "test/parallel/test-wrap-js-stream-read-stop.js",
];

const STREAMS_WEB_PLATFORM_PROMOTED_NODE22_EXTRA_PATHS: &[&str] =
    &["test/parallel/test-stream-destroy.js"];

const STREAMS_WEB_PLATFORM_PROMOTED_NODE24_EXTRA_PATHS: &[&str] = &[
    "test/parallel/test-fastutf8stream-destroy.js",
    "test/parallel/test-fastutf8stream-end.js",
    "test/parallel/test-fastutf8stream-flush-mocks.js",
    "test/parallel/test-fastutf8stream-flush-sync.js",
    "test/parallel/test-fastutf8stream-flush.js",
    "test/parallel/test-fastutf8stream-fsync.js",
    "test/parallel/test-fastutf8stream-minlength.js",
    "test/parallel/test-fastutf8stream-mode.js",
    "test/parallel/test-fastutf8stream-partial-write-utf8.js",
    "test/parallel/test-fastutf8stream-periodicflush.js",
    "test/parallel/test-fastutf8stream-reopen.js",
    "test/parallel/test-fastutf8stream-retry.js",
    "test/parallel/test-fastutf8stream-write.js",
    "test/parallel/test-stream-readable-to-web-byob.js",
    "test/parallel/test-urlpattern-types.js",
    "test/parallel/test-webstreams-adapters-sync-write-error.js",
    "test/parallel/test-webstreams-adapters-writable-buffer-sources.js",
    "test/parallel/test-webstreams-compression-bad-chunks.js",
    "test/parallel/test-webstreams-compression-buffer-source.js",
    "test/parallel/test-webstreams-decompression-reject-trailing.js",
    "test/parallel/test-webstreams-duplex-fromweb-writev-unhandled-rejection.js",
];

const STREAMS_WEB_PLATFORM_PROMOTED_NODE26_PATHS: &[&str] = &[
    "test/parallel/test-fastutf8stream-destroy.js",
    "test/parallel/test-fastutf8stream-end.js",
    "test/parallel/test-fastutf8stream-flush-mocks.js",
    "test/parallel/test-fastutf8stream-flush-sync.js",
    "test/parallel/test-fastutf8stream-flush.js",
    "test/parallel/test-fastutf8stream-fsync.js",
    "test/parallel/test-fastutf8stream-minlength.js",
    "test/parallel/test-fastutf8stream-mode.js",
    "test/parallel/test-fastutf8stream-partial-write-utf8.js",
    "test/parallel/test-fastutf8stream-periodicflush.js",
    "test/parallel/test-fastutf8stream-reopen.js",
    "test/parallel/test-fastutf8stream-retry.js",
    "test/parallel/test-fastutf8stream-sync.js",
    "test/parallel/test-fastutf8stream-write.js",
    "test/parallel/test-file-write-stream.js",
    "test/parallel/test-file-write-stream2.js",
    "test/parallel/test-file-write-stream3.js",
    "test/parallel/test-file-write-stream4.js",
    "test/parallel/test-file-write-stream5.js",
    "test/parallel/test-filehandle-readablestream.js",
    "test/parallel/test-global-webstreams.js",
    "test/parallel/test-js-stream-call-properties.js",
    "test/parallel/test-stream-destroy.js",
    "test/parallel/test-stream-finished-async-local-storage.js",
    "test/parallel/test-stream-finished-bindAsyncResource-path.js",
    "test/parallel/test-stream-finished-default-path.js",
    "test/parallel/test-stream-iter-broadcast-backpressure.js",
    "test/parallel/test-stream-iter-broadcast-basic.js",
    "test/parallel/test-stream-iter-broadcast-coverage.js",
    "test/parallel/test-stream-iter-broadcast-from.js",
    "test/parallel/test-stream-iter-consumers-bytes.js",
    "test/parallel/test-stream-iter-consumers-merge.js",
    "test/parallel/test-stream-iter-consumers-tap.js",
    "test/parallel/test-stream-iter-consumers-text.js",
    "test/parallel/test-stream-iter-cross-realm.js",
    "test/parallel/test-stream-iter-disabled.js",
    "test/parallel/test-stream-iter-duplex.js",
    "test/parallel/test-stream-iter-from-async.js",
    "test/parallel/test-stream-iter-from-coverage.js",
    "test/parallel/test-stream-iter-from-sync.js",
    "test/parallel/test-stream-iter-from-writable-cache-options.js",
    "test/parallel/test-stream-iter-namespace.js",
    "test/parallel/test-stream-iter-pull-async.js",
    "test/parallel/test-stream-iter-pull-sync.js",
    "test/parallel/test-stream-iter-push-backpressure.js",
    "test/parallel/test-stream-iter-push-basic.js",
    "test/parallel/test-stream-iter-push-writer.js",
    "test/parallel/test-stream-iter-readable-interop-disabled.js",
    "test/parallel/test-stream-iter-readable-interop.js",
    "test/parallel/test-stream-iter-share-async.js",
    "test/parallel/test-stream-iter-share-coverage.js",
    "test/parallel/test-stream-iter-share-from.js",
    "test/parallel/test-stream-iter-share-sync.js",
    "test/parallel/test-stream-iter-sharedarraybuffer.js",
    "test/parallel/test-stream-iter-to-readable.js",
    "test/parallel/test-stream-iter-transform-compat.js",
    "test/parallel/test-stream-iter-transform-coverage.js",
    "test/parallel/test-stream-iter-transform-errors.js",
    "test/parallel/test-stream-iter-transform-output.js",
    "test/parallel/test-stream-iter-transform-roundtrip.js",
    "test/parallel/test-stream-iter-transform-sync.js",
    "test/parallel/test-stream-iter-validation.js",
    "test/parallel/test-stream-iter-writable-from.js",
    "test/parallel/test-stream-iter-writable-interop.js",
    "test/parallel/test-stream-iterator-helpers-test262-tests.mjs",
    "test/parallel/test-stream-readable-async-iterators.js",
    "test/parallel/test-stream-readable-compose.js",
    "test/parallel/test-stream-readable-readable-one.js",
    "test/parallel/test-stream-readable-to-web-byob.js",
    "test/parallel/test-stream-readable-to-web-termination-byob.js",
    "test/parallel/test-stream-readable-to-web.mjs",
    "test/parallel/test-stream-readableListening-state.js",
    "test/parallel/test-stream-some-find-every.mjs",
    "test/parallel/test-stream-toWeb-allows-server-response.js",
    "test/parallel/test-stream-transform-destroy.js",
    "test/parallel/test-stream-wrap-drain.js",
    "test/parallel/test-stream-wrap-encoding.js",
    "test/parallel/test-stream-wrap.js",
    "test/parallel/test-stream-writable-samecb-singletick.js",
    "test/parallel/test-stream2-base64-single-char-read-end.js",
    "test/parallel/test-stream2-compatibility.js",
    "test/parallel/test-stream2-decode-partial.js",
    "test/parallel/test-stream2-httpclient-response-end.js",
    "test/parallel/test-stream2-large-read-stall.js",
    "test/parallel/test-stream2-objects.js",
    "test/parallel/test-stream2-push.js",
    "test/parallel/test-stream2-read-correct-num-bytes-in-utf8.js",
    "test/parallel/test-stream2-read-sync-stack.js",
    "test/parallel/test-stream2-readable-empty-buffer-no-eof.js",
    "test/parallel/test-stream2-readable-legacy-drain.js",
    "test/parallel/test-stream2-readable-non-empty-end.js",
    "test/parallel/test-stream2-readable-wrap-destroy.js",
    "test/parallel/test-stream2-readable-wrap-empty.js",
    "test/parallel/test-stream2-readable-wrap-error.js",
    "test/parallel/test-stream2-readable-wrap.js",
    "test/parallel/test-stream2-set-encoding.js",
    "test/parallel/test-stream2-transform.js",
    "test/parallel/test-stream2-writable.js",
    "test/parallel/test-stream3-cork-end.js",
    "test/parallel/test-stream3-cork-uncork.js",
    "test/parallel/test-stream3-pause-then-read.js",
    "test/parallel/test-streams-highwatermark.js",
    "test/parallel/test-webstream-encoding-inspect.js",
    "test/parallel/test-webstream-readable-from.js",
    "test/parallel/test-webstream-string-tag.js",
    "test/parallel/test-webstream-structured-clone-no-leftovers.mjs",
    "test/parallel/test-webstreams-abort-controller.js",
    "test/parallel/test-webstreams-adapters-sync-write-error.js",
    "test/parallel/test-webstreams-adapters-writable-buffer-sources.js",
    "test/parallel/test-webstreams-compose.js",
    "test/parallel/test-webstreams-compression-bad-chunks.js",
    "test/parallel/test-webstreams-compression-buffer-source.js",
    "test/parallel/test-webstreams-decompression-reject-trailing.js",
    "test/parallel/test-webstreams-duplex-fromweb-writev-unhandled-rejection.js",
    "test/parallel/test-webstreams-finished.js",
    "test/parallel/test-wrap-js-stream-destroy.js",
    "test/parallel/test-wrap-js-stream-duplex.js",
    "test/parallel/test-wrap-js-stream-read-stop.js",
];

fn streams_web_platform_unpromoted_surface_path(path: &str) -> bool {
    path.contains("stream")
}

fn streams_web_platform_required_fixture_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths =
        node_compat_required_gap_paths_for_owner(lane, "streams-local-io/stream");
    fixture_paths.extend(
        node_compat_required_gap_paths_for_owner(lane, "node-compat/unpromoted-surface")
            .into_iter()
            .filter(|path| streams_web_platform_unpromoted_surface_path(path)),
    );
    fixture_paths.retain(|path| {
        !STREAMS_WEB_PLATFORM_LOW_ROI_PATHS
            .iter()
            .any(|low_roi_path| path == low_roi_path)
    });
    fixture_paths.sort();
    fixture_paths.dedup();
    fixture_paths
}

const WEB_PLATFORM_RESIDUAL_PREFIXES: &[&str] = &[
    "test/parallel/test-blob",
    "test/parallel/test-compression-decompression-stream",
    "test/parallel/test-global-customevent",
    "test/parallel/test-global-domexception",
    "test/parallel/test-global-setters",
    "test/parallel/test-urlpattern",
    "test/parallel/test-webstream",
    "test/parallel/test-webstreams-compression",
    "test/parallel/test-whatwg-encoding",
    "test/parallel/test-whatwg-url",
    "test/parallel/test-whatwg-webstreams",
];

const WEB_PLATFORM_RESIDUAL_LOW_ROI_PATHS: &[&str] = &[
    "test/parallel/test-whatwg-encoding-encodeinto-large.js",
    "test/parallel/test-webstreams-clone-unref.js",
    "test/parallel/test-whatwg-webstreams-transform-stream-members.js",
];

const WEB_PLATFORM_RESIDUAL_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures", "test/fixtures/wpt"];

fn web_platform_residual_required_gap_path(path: &str) -> bool {
    WEB_PLATFORM_RESIDUAL_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn web_platform_residual_required_fixture_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths = node_compat_required_gap_paths_for_selector(
        lane,
        web_platform_residual_required_gap_path,
    );
    fixture_paths.retain(|path| {
        !WEB_PLATFORM_RESIDUAL_LOW_ROI_PATHS
            .iter()
            .any(|low_roi_path| path == low_roi_path)
    });
    fixture_paths.sort();
    fixture_paths.dedup();
    fixture_paths
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked WHATWG/web-platform residual inventory; prior hang/stress paths are excluded by the kill rule"]
fn node22_supported_lane_web_platform_residual_watchpoint() {
    let fixture_paths = web_platform_residual_required_fixture_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-web-platform-residual-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        WEB_PLATFORM_RESIDUAL_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked WHATWG/web-platform residual inventory; prior hang/stress paths are excluded by the kill rule"]
fn node24_default_lane_web_platform_residual_watchpoint() {
    let fixture_paths = web_platform_residual_required_fixture_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-web-platform-residual-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        WEB_PLATFORM_RESIDUAL_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_streams_web_platform_promoted_batch_fixture() {
    let mut fixture_paths: Vec<String> = STREAMS_WEB_PLATFORM_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    fixture_paths.extend(
        STREAMS_WEB_PLATFORM_PROMOTED_NODE22_EXTRA_PATHS
            .iter()
            .map(|path| (*path).to_string()),
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-streams-web-platform-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        STREAMS_WEB_PLATFORM_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_streams_web_platform_promoted_batch_fixture() {
    let mut fixture_paths: Vec<String> = STREAMS_WEB_PLATFORM_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    fixture_paths.extend(
        STREAMS_WEB_PLATFORM_PROMOTED_NODE24_EXTRA_PATHS
            .iter()
            .map(|path| (*path).to_string()),
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-streams-web-platform-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        STREAMS_WEB_PLATFORM_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_streams_web_platform_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = STREAMS_WEB_PLATFORM_PROMOTED_NODE26_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-streams-web-platform-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        STREAMS_WEB_PLATFORM_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked streams/WebStreams required-gap inventory; excludes pinned hang diagnostics for stream-base-typechecking, webstreams-clone-unref, and WHATWG transform-stream-members"]
fn node22_supported_lane_streams_web_platform_watchpoint() {
    let fixture_paths = streams_web_platform_required_fixture_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-streams-web-platform-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        STREAMS_WEB_PLATFORM_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked streams/WebStreams required-gap inventory; excludes pinned hang diagnostics for stream-base-typechecking, webstreams-clone-unref, and WHATWG transform-stream-members"]
fn node24_default_lane_streams_web_platform_watchpoint() {
    let fixture_paths = streams_web_platform_required_fixture_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-streams-web-platform-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        STREAMS_WEB_PLATFORM_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 node26 broad pre-run: ROI-ranked streams/WebStreams required-gap inventory; excludes pinned hang diagnostics for stream-base-typechecking, webstreams-clone-unref, and WHATWG transform-stream-members"]
fn node26_current_lane_streams_web_platform_watchpoint() {
    let fixture_paths = streams_web_platform_required_fixture_paths(NodeCompatLane::Node26);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-streams-web-platform-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        STREAMS_WEB_PLATFORM_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "Pinned later-family dependency: test-stream-writable-samecb-singletick.js asserts async_hooks TickObject allocation counts, which are owned by the broader async_hooks/task-accounting family rather than the current pure-stream contract"]
fn node22_stream_writable_samecb_singletick_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-writable-samecb-singletick.js",
        "node22/test/parallel/test-stream-writable-samecb-singletick.js",
        &[],
    );
}

#[test]
fn node22_stream_finished_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-finished.js",
        "node22/test/parallel/test-stream-finished.js",
        &[],
    );
}

#[test]
fn node22_stream_pipeline_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-pipeline.js",
        "node22/test/parallel/test-stream-pipeline.js",
        &[],
    );
}

#[test]
fn node22_net_local_address_port_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-net-local-address-port.js",
        "node22/test/parallel/test-net-local-address-port.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 default-lane divergence: test-stream-pipeline.js currently returns an AbortError-style 'The operation was aborted' message where the staged Node24 fixture still expects the inner 'Boom!' pipeline error message"]
fn node24_stream_pipeline_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-pipeline.js",
        "node24/test/parallel/test-stream-pipeline.js",
        &[],
    );
}

#[test]
fn node20_readline_interface_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-interface.js",
        "node20/test/parallel/test-readline-interface.js",
        &[],
    );
}

#[test]
fn node22_readline_interface_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-interface.js",
        "node22/test/parallel/test-readline-interface.js",
        &[],
    );
}

#[test]
fn node24_readline_interface_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-interface.js",
        "node24/test/parallel/test-readline-interface.js",
        &[],
    );
}

#[test]
fn node20_readline_promises_interface_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-promises-interface.js",
        "node20/test/parallel/test-readline-promises-interface.js",
        &[],
    );
}

#[test]
fn node22_readline_promises_interface_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-promises-interface.js",
        "node22/test/parallel/test-readline-promises-interface.js",
        &[],
    );
}

#[test]
fn node24_readline_promises_interface_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-promises-interface.js",
        "node24/test/parallel/test-readline-promises-interface.js",
        &[],
    );
}

#[test]
fn node22_process_load_env_file_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-process-load-env-file.js",
        "node22/test/parallel/test-process-load-env-file.js",
        NODE22_PROCESS_LOAD_ENV_FILE_EXTRA_FILES,
    );
}

#[test]
fn node24_process_load_env_file_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-process-load-env-file.js",
        "node24/test/parallel/test-process-load-env-file.js",
        NODE24_PROCESS_LOAD_ENV_FILE_EXTRA_FILES,
    );
}

const PROCESS_HOST_EXTRA_DIRS: &[&str] = &["test/common"];

const PROCESS_HOST_LOW_ROI_PATHS: &[&str] = &[
    "test/abort/test-process-abort-exitcode.js",
    "test/parallel/test-process-argv-0.js",
    "test/parallel/test-process-dlopen-error-message-crash.js",
    "test/parallel/test-process-dlopen-undefined-exports.js",
    "test/parallel/test-process-euid-egid.js",
    "test/parallel/test-process-external-stdio-close-spawn.js",
    "test/parallel/test-process-external-stdio-close.js",
    "test/parallel/test-process-finalization.mjs",
    "test/parallel/test-process-getactivehandles.js",
    "test/parallel/test-process-getactiverequests.js",
    "test/parallel/test-process-getactiveresources-track-active-handles.js",
    "test/parallel/test-process-getactiveresources-track-active-requests.js",
    "test/parallel/test-process-getactiveresources-track-interval-lifetime.js",
    "test/parallel/test-process-getactiveresources-track-multiple-timers.js",
    "test/parallel/test-process-getactiveresources-track-timer-lifetime.js",
    "test/parallel/test-process-getactiveresources.js",
    "test/parallel/test-process-getgroups.js",
    "test/parallel/test-process-initgroups.js",
    "test/parallel/test-process-kill-null.js",
    "test/parallel/test-process-kill-pid.js",
    "test/parallel/test-process-ppid.js",
    "test/parallel/test-process-raw-debug.js",
    "test/parallel/test-process-really-exit.js",
    "test/parallel/test-process-redirect-warnings-env.js",
    "test/parallel/test-process-redirect-warnings.js",
    "test/parallel/test-process-setgroups.js",
    "test/parallel/test-process-title-cli.js",
    "test/parallel/test-process-uid-gid.js",
    "test/parallel/test-process-uncaught-exception-monitor.js",
    "test/parallel/test-process-versions.js",
    "test/parallel/test-process-warnings.mjs",
];

const PROCESS_HOST_LOW_ROI_PREFIXES: &[&str] = &["test/parallel/test-process-execve"];

fn process_host_runnable_fixture_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths =
        node_compat_required_gap_paths_for_owner(lane, "process-and-timing/process-host");
    fixture_paths.retain(|path| {
        !PROCESS_HOST_LOW_ROI_PATHS
            .iter()
            .any(|low_roi_path| path == low_roi_path)
            && !PROCESS_HOST_LOW_ROI_PREFIXES
                .iter()
                .any(|low_roi_prefix| path.starts_with(low_roi_prefix))
    });
    fixture_paths
}

const PROCESS_HOST_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-process-abort.js",
    "test/parallel/test-process-binding-internalbinding-allowlist.js",
    "test/parallel/test-process-binding.js",
    "test/parallel/test-process-constrained-memory.js",
    "test/parallel/test-process-domain-segfault.js",
    "test/parallel/test-process-env-allowed-flags.js",
    "test/parallel/test-process-env-delete.js",
    "test/parallel/test-process-env-sideeffects.js",
    "test/parallel/test-process-env-windows-error-reset.js",
    "test/parallel/test-process-exception-capture-errors.js",
    "test/parallel/test-process-exit-handler.js",
    "test/parallel/test-process-exit.js",
    "test/parallel/test-process-setsourcemapsenabled.js",
    "test/parallel/test-process-threadCpuUsage-main-thread.js",
    "test/parallel/test-process-umask-mask.js",
    "test/parallel/test-process-umask.js",
];

const PROCESS_HOST_PROMOTED_NODE26_EXTRA_PATHS: &[&str] = &[
    "test/parallel/test-process-beforeexit-throw-exit.js",
    "test/parallel/test-process-binding-util.js",
    "test/parallel/test-process-constants-noatime.js",
    "test/parallel/test-process-cpuUsage.js",
    "test/parallel/test-process-emit.js",
    "test/parallel/test-process-env-deprecation.js",
    "test/parallel/test-process-env-ignore-getter-setter.js",
    "test/parallel/test-process-exit-from-before-exit.js",
    "test/parallel/test-process-exit-recursive.js",
    "test/parallel/test-process-get-builtin.mjs",
    "test/parallel/test-process-ref-unref.js",
];

#[test]
fn node22_supported_lane_executes_process_host_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = PROCESS_HOST_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| path.to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-process-host-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        PROCESS_HOST_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_process_host_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = PROCESS_HOST_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| path.to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-process-host-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        PROCESS_HOST_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_process_host_promoted_batch_fixture() {
    let mut fixture_paths: Vec<String> = PROCESS_HOST_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| path.to_string())
        .collect();
    fixture_paths.extend(
        PROCESS_HOST_PROMOTED_NODE26_EXTRA_PATHS
            .iter()
            .map(|path| path.to_string()),
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-process-host-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        PROCESS_HOST_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked process-host required-gap inventory; host/native/subprocess-only paths are excluded by the kill rule and remain gaps"]
fn node22_supported_lane_process_host_watchpoint() {
    let fixture_paths = process_host_runnable_fixture_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-process-host-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        PROCESS_HOST_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked process-host required-gap inventory; host/native/subprocess-only paths are excluded by the kill rule and remain gaps"]
fn node24_default_lane_process_host_watchpoint() {
    let fixture_paths = process_host_runnable_fixture_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-process-host-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        PROCESS_HOST_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 node26 broad pre-run: ROI-ranked process-host required-gap inventory; host/native/subprocess-only paths are excluded by the kill rule and remain gaps"]
fn node26_current_lane_process_host_watchpoint() {
    let fixture_paths = process_host_runnable_fixture_paths(NodeCompatLane::Node26);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-process-host-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        PROCESS_HOST_EXTRA_DIRS,
    );
}

#[test]
fn node24_util_format_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-util-format.js",
        "node24/test/parallel/test-util-format.js",
        &[],
    );
}

#[test]
fn node24_perf_hooks_resourcetiming_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-perf-hooks-resourcetiming.js",
        "node24/test/parallel/test-perf-hooks-resourcetiming.js",
        &[],
    );
}

#[test]
fn node22_fs_glob_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-glob.mjs",
        "node22/test/parallel/test-fs-glob.mjs",
        NODE22_COMMON_INDEX_MJS_EXTRA_FILES,
    );
}

#[test]
fn node24_fs_glob_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-glob.mjs",
        "node24/test/parallel/test-fs-glob.mjs",
        NODE24_COMMON_INDEX_MJS_EXTRA_FILES,
    );
}

#[test]
fn node22_supported_lane_executes_fs_cp_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-supported-lane-executes-fs-cp-batch",
        NodeCompatLane::Node22,
        FS_CP_BATCH,
    );
}

#[test]
fn node24_default_lane_executes_fs_cp_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-default-lane-executes-fs-cp-batch",
        NodeCompatLane::Node24,
        FS_CP_BATCH,
    );
}

const ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES: &[&str] = &[
    "test/fixtures/baz.js",
    "test/fixtures/empty.cjs",
    "test/fixtures/empty.js",
    "test/fixtures/empty.json",
    "test/fixtures/experimental.json",
    "test/fixtures/invalid.json",
    "test/fixtures/is-object.js",
    "test/fixtures/module-loading-error.node",
    "test/fixtures/out-of-bound.wasm",
    "test/fixtures/pkgexports.mjs",
    "test/fixtures/printA.js",
    "test/fixtures/primitive-42.json",
    "test/fixtures/recursive-a.cjs",
    "test/fixtures/recursive-b.cjs",
    "test/fixtures/simple.wasm",
    "test/fixtures/value.cjs",
];

const ESM_MODULE_LOADER_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/es-module",
    "test/fixtures/cycles",
    "test/fixtures/es-module-url",
    "test/fixtures/es-module-loaders",
    "test/fixtures/es-module-require-cache",
    "test/fixtures/es-module-specifiers",
    "test/fixtures/es-modules",
    "test/fixtures/import-require-cycle",
    "test/fixtures/module-hooks",
    "test/fixtures/module-require-symlink",
    "test/fixtures/node_modules",
    "test/fixtures/packages",
    "test/fixtures/snapshot",
    "test/fixtures/syntax",
    "test/fixtures/test-module-loading-globalpaths",
    "test/fixtures/typescript",
    "test/fixtures/uncaught-exceptions",
];

const ESM_DATA_URL_CLUSTER_PATHS: &[&str] = &[
    "test/es-module/test-cjs-prototype-pollution.js",
    "test/es-module/test-esm-data-urls.js",
    "test/es-module/test-esm-import-assertion-warning.mjs",
    "test/es-module/test-esm-import-meta.mjs",
    "test/es-module/test-esm-invalid-data-urls.js",
    "test/es-module/test-esm-prototype-pollution.mjs",
    "test/es-module/test-esm-undefined-cjs-global-like-variables.js",
];

const ESM_MODULE_LOADER_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/es-module/test-esm-assert-strict.mjs",
    "test/es-module/test-esm-cjs-builtins.js",
    "test/es-module/test-esm-cjs-main.js",
    "test/es-module/test-esm-cyclic-dynamic-import.mjs",
    "test/es-module/test-esm-default-type.mjs",
    "test/es-module/test-esm-dns-promises.mjs",
    "test/es-module/test-esm-double-encoding.mjs",
    "test/es-module/test-esm-dynamic-import-mutating-fs.js",
    "test/es-module/test-esm-encoded-path.mjs",
    "test/es-module/test-esm-example-loader.mjs",
    "test/es-module/test-esm-forbidden-globals.mjs",
    "test/es-module/test-esm-fs-promises.mjs",
    "test/es-module/test-esm-import-attributes-1.mjs",
    "test/es-module/test-esm-import-attributes-2.mjs",
    "test/es-module/test-esm-import-attributes-3.mjs",
    "test/es-module/test-esm-import-json-named-export.mjs",
    "test/es-module/test-esm-in-require-cache-2.mjs",
    "test/es-module/test-esm-in-require-cache.js",
    "test/es-module/test-esm-json.mjs",
    "test/es-module/test-disable-require-module-with-detection.js",
    "test/es-module/test-esm-basic-imports.mjs",
    "test/es-module/test-esm-cjs-exports.js",
    "test/es-module/test-esm-custom-exports.mjs",
    "test/es-module/test-esm-loader-cache-clearing.js",
    "test/es-module/test-esm-loader-dependency.mjs",
    "test/es-module/test-esm-loader-event-loop.mjs",
    "test/es-module/test-esm-namespace.mjs",
    "test/es-module/test-esm-path-posix.mjs",
    "test/es-module/test-esm-path-win32.mjs",
    "test/es-module/test-esm-recursive-cjs-dependencies.mjs",
    "test/es-module/test-esm-require-cache.mjs",
    "test/es-module/test-esm-scope-node-modules.mjs",
    "test/es-module/test-esm-shared-loader-dep.mjs",
    "test/es-module/test-esm-shebang.mjs",
    "test/es-module/test-esm-preserve-symlinks.js",
    "test/es-module/test-esm-symlink.js",
    "test/es-module/test-esm-symlink-main.js",
    "test/es-module/test-esm-symlink-type.js",
    "test/es-module/test-esm-syntax-error.mjs",
    "test/es-module/test-esm-throw-undefined.mjs",
    "test/es-module/test-esm-tla.mjs",
    "test/es-module/test-esm-type-field.mjs",
    "test/es-module/test-esm-type-main.mjs",
    "test/es-module/test-esm-util-types.mjs",
    "test/es-module/test-esm-windows.js",
    "test/es-module/test-import-module-conditional-exports-module.mjs",
    "test/es-module/test-import-module-retry-require-errored.js",
    "test/es-module/test-import-preload-require-cycle.js",
    "test/es-module/test-loaders-hidden-from-users.js",
    "test/es-module/test-require-as-esm-interop.mjs",
    "test/es-module/test-require-esm-from-imported-cjs.js",
    "test/es-module/test-require-module-cached-tla.js",
    "test/es-module/test-require-module-conditional-exports.js",
    "test/es-module/test-require-module-conditional-exports-module.js",
    "test/es-module/test-require-module-cycle-cjs-esm-esm.js",
    "test/es-module/test-require-module-defined-esmodule.js",
    "test/es-module/test-require-module-detect-entry-point-aou.js",
    "test/es-module/test-require-module-detect-entry-point.js",
    "test/es-module/test-require-module-dont-detect-cjs.js",
    "test/es-module/test-require-module-default-extension.js",
    "test/es-module/test-require-module-dynamic-import-1.js",
    "test/es-module/test-require-module-dynamic-import-2.js",
    "test/es-module/test-require-module-dynamic-import-3.js",
    "test/es-module/test-require-module-dynamic-import-4.js",
    "test/es-module/test-require-module-implicit.js",
    "test/es-module/test-require-module-instantiated.mjs",
    "test/es-module/test-require-module.js",
    "test/es-module/test-require-module-preload.js",
    "test/es-module/test-require-module-retry-import-errored.js",
    "test/es-module/test-require-module-retry-import-errored-2.js",
    "test/es-module/test-require-module-retry-import-evaluating.js",
    "test/es-module/test-require-module-synchronous-rejection-handling.js",
    "test/es-module/test-require-module-tla-execution.js",
    "test/es-module/test-require-module-tla-nested.js",
    "test/es-module/test-require-module-tla-rejected.js",
    "test/es-module/test-require-module-tla-resolved.js",
    "test/es-module/test-require-module-tla-retry-import-2.js",
    "test/es-module/test-require-module-tla-retry-import.js",
    "test/es-module/test-require-module-tla-retry-require.js",
    "test/es-module/test-require-module-tla-unresolved.js",
    "test/es-module/test-require-module-transpiled.js",
    "test/es-module/test-require-module-twice.js",
    "test/es-module/test-require-module-with-detection.js",
    "test/es-module/test-vm-compile-function-leak.js",
    "test/es-module/test-vm-compile-function-lineoffset.js",
    "test/es-module/test-vm-contextified-script-leak.js",
    "test/es-module/test-vm-source-text-module-leak.js",
    "test/es-module/test-vm-synthetic-module-leak.js",
    "test/es-module/test-wasm-memory-out-of-bound.js",
    "test/es-module/test-wasm-simple.js",
    "test/module-hooks/test-module-hooks-builtin-require.js",
    "test/module-hooks/test-module-hooks-create-require-with-url.mjs",
    "test/module-hooks/test-module-hooks-custom-conditions-special-values.js",
    "test/module-hooks/test-module-hooks-load-builtin-import.mjs",
    "test/module-hooks/test-module-hooks-require-wasm.js",
    "test/module-hooks/test-module-hooks-resolve-builtin-on-disk-require-with-prefix.js",
    "test/module-hooks/test-module-hooks-resolve-builtin-on-disk-require.js",
    "test/module-hooks/test-module-hooks-resolve-load-builtin-override-both-prefix.js",
    "test/module-hooks/test-module-hooks-resolve-load-builtin-override-both.js",
    "test/module-hooks/test-module-hooks-resolve-load-builtin-redirect-prefix.js",
    "test/module-hooks/test-module-hooks-resolve-load-builtin-redirect.js",
    "test/module-hooks/test-module-hooks-resolve-load-import-inline-typescript-override.mjs",
    "test/module-hooks/test-module-hooks-resolve-load-import-inline-typescript.mjs",
    "test/module-hooks/test-module-hooks-resolve-load-require-inline-typescript-override.js",
    "test/module-hooks/test-module-hooks-resolve-load-require-inline-typescript.js",
    "test/parallel/test-module-circular-dependency-warning.js",
    "test/parallel/test-module-circular-symlinks.js",
    "test/parallel/test-module-globalpaths-nodepath.js",
    "test/parallel/test-module-main-preserve-symlinks-fail.js",
    "test/parallel/test-module-parent-setter-deprecation.js",
    "test/parallel/test-module-symlinked-peer-modules.js",
    "test/parallel/test-require-resolve-invalid-paths.js",
    "test/parallel/test-require-resolve-opts-paths-relative.js",
];

const ESM_MODULE_LOADER_PROMOTED_NODE22_ONLY_PATHS: &[&str] = &[];

const ESM_MODULE_LOADER_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/es-module/test-esm-wasm-escape-import-names.mjs",
    "test/es-module/test-esm-wasm-load-exports.mjs",
    "test/es-module/test-esm-wasm-source-phase-static.mjs",
    "test/es-module/test-import-require-tla-twice.js",
];

const ESM_MODULE_LOADER_PROMOTED_NODE26_PATHS: &[&str] = &[
    "test/es-module/test-cjs-prototype-pollution.js",
    "test/es-module/test-disable-require-module-with-detection.js",
    "test/es-module/test-dynamic-import-script-lifetime.js",
    "test/es-module/test-esm-assert-strict.mjs",
    "test/es-module/test-esm-basic-imports.mjs",
    "test/es-module/test-esm-cjs-builtins.js",
    "test/es-module/test-esm-cjs-exports.js",
    "test/es-module/test-esm-cjs-main.js",
    "test/es-module/test-esm-custom-exports.mjs",
    "test/es-module/test-esm-cyclic-dynamic-import.mjs",
    "test/es-module/test-esm-data-urls.js",
    "test/es-module/test-esm-default-type.mjs",
    "test/es-module/test-esm-dns-promises.mjs",
    "test/es-module/test-esm-double-encoding.mjs",
    "test/es-module/test-esm-dynamic-import-attribute.js",
    "test/es-module/test-esm-dynamic-import-attribute.mjs",
    "test/es-module/test-esm-dynamic-import-commonjs.js",
    "test/es-module/test-esm-dynamic-import-commonjs.mjs",
    "test/es-module/test-esm-dynamic-import-mutating-fs.js",
    "test/es-module/test-esm-dynamic-import.js",
    "test/es-module/test-esm-encoded-path.mjs",
    "test/es-module/test-esm-example-loader.mjs",
    "test/es-module/test-esm-exports.mjs",
    "test/es-module/test-esm-forbidden-globals.mjs",
    "test/es-module/test-esm-fs-promises.mjs",
    "test/es-module/test-esm-import-attributes-1.mjs",
    "test/es-module/test-esm-import-attributes-2.mjs",
    "test/es-module/test-esm-import-attributes-3.mjs",
    "test/es-module/test-esm-import-attributes-errors.js",
    "test/es-module/test-esm-import-attributes-errors.mjs",
    "test/es-module/test-esm-import-attributes-identity.mjs",
    "test/es-module/test-esm-import-json-named-export.mjs",
    "test/es-module/test-esm-import-meta-main.mjs",
    "test/es-module/test-esm-import-meta-resolve-hooks.mjs",
    "test/es-module/test-esm-import-meta.mjs",
    "test/es-module/test-esm-imports.mjs",
    "test/es-module/test-esm-in-require-cache-2.mjs",
    "test/es-module/test-esm-invalid-data-urls.js",
    "test/es-module/test-esm-json-cache.mjs",
    "test/es-module/test-esm-json.mjs",
    "test/es-module/test-esm-live-binding.mjs",
    "test/es-module/test-esm-loader-cache-clearing.js",
    "test/es-module/test-esm-loader-dependency.mjs",
    "test/es-module/test-esm-loader-event-loop.mjs",
    "test/es-module/test-esm-loader-mock.mjs",
    "test/es-module/test-esm-main-lookup.mjs",
    "test/es-module/test-esm-namespace.mjs",
    "test/es-module/test-esm-path-posix.mjs",
    "test/es-module/test-esm-path-win32.mjs",
    "test/es-module/test-esm-pkgname.mjs",
    "test/es-module/test-esm-preserve-symlinks.js",
    "test/es-module/test-esm-process.mjs",
    "test/es-module/test-esm-prototype-pollution.mjs",
    "test/es-module/test-esm-recursive-cjs-dependencies.mjs",
    "test/es-module/test-esm-require-cache.mjs",
    "test/es-module/test-esm-require-race-condition.js",
    "test/es-module/test-esm-scope-node-modules.mjs",
    "test/es-module/test-esm-shared-loader-dep.mjs",
    "test/es-module/test-esm-shebang.mjs",
    "test/es-module/test-esm-snapshot.mjs",
    "test/es-module/test-esm-symlink-main.js",
    "test/es-module/test-esm-symlink.js",
    "test/es-module/test-esm-syntax-error.mjs",
    "test/es-module/test-esm-throw-undefined.mjs",
    "test/es-module/test-esm-tla.mjs",
    "test/es-module/test-esm-type-field.mjs",
    "test/es-module/test-esm-type-main.mjs",
    "test/es-module/test-esm-undefined-cjs-global-like-variables.js",
    "test/es-module/test-esm-util-types.mjs",
    "test/es-module/test-esm-virtual-json.mjs",
    "test/es-module/test-esm-wasm-escape-import-names.mjs",
    "test/es-module/test-esm-wasm-load-exports.mjs",
    "test/es-module/test-esm-wasm-source-phase-static.mjs",
    "test/es-module/test-esm-windows.js",
    "test/es-module/test-import-cjs-jitless.mjs",
    "test/es-module/test-import-module-conditional-exports-module.mjs",
    "test/es-module/test-import-module-retry-require-errored.js",
    "test/es-module/test-import-preload-require-cycle.js",
    "test/es-module/test-import-require-tla-twice.js",
    "test/es-module/test-loaders-hidden-from-users.js",
    "test/es-module/test-require-esm-from-imported-cjs.js",
    "test/es-module/test-require-module-cached-tla.js",
    "test/es-module/test-require-module-conditional-exports.js",
    "test/es-module/test-require-module-detect-entry-point-aou.js",
    "test/es-module/test-require-module-detect-entry-point.js",
    "test/es-module/test-require-module-dont-detect-cjs.js",
    "test/es-module/test-require-module-dynamic-import-4.js",
    "test/es-module/test-require-module-tla-execution.js",
    "test/es-module/test-require-module-tla-nested.js",
    "test/es-module/test-require-module-tla-rejected.js",
    "test/es-module/test-require-module-tla-resolved.js",
    "test/es-module/test-require-module-tla-retry-import-2.js",
    "test/es-module/test-require-module-tla-retry-import.js",
    "test/es-module/test-require-module-tla-retry-require.js",
    "test/es-module/test-require-module-tla-unresolved.js",
    "test/es-module/test-require-module-transpiled.js",
    "test/es-module/test-wasm-memory-out-of-bound.js",
    "test/es-module/test-wasm-simple.js",
    "test/parallel/test-module-circular-dependency-warning.js",
    "test/parallel/test-module-main-preserve-symlinks-fail.js",
];

const ESM_MODULE_LOADER_NODE26_CYCLE21_PROMOTED_PATHS: &[&str] = &[
    "test/es-module/test-esm-cjs-named-error.mjs",
    "test/es-module/test-esm-error-cache.js",
    "test/es-module/test-esm-in-require-cache.js",
    "test/es-module/test-esm-register-deprecation.mjs",
    "test/es-module/test-esm-symlink-type.js",
    "test/es-module/test-extensionless-esm-type-commonjs.js",
    "test/es-module/test-require-as-esm-interop.mjs",
    "test/es-module/test-require-module-conditional-exports-module.js",
    "test/es-module/test-require-module-cycle-cjs-esm-esm.js",
    "test/es-module/test-require-module-default-extension.js",
    "test/es-module/test-require-module-defined-esmodule.js",
    "test/es-module/test-require-module-dynamic-import-1.js",
    "test/es-module/test-require-module-dynamic-import-2.js",
    "test/es-module/test-require-module-dynamic-import-3.js",
    "test/es-module/test-require-module-error-catching.js",
    "test/es-module/test-require-module-implicit.js",
    "test/es-module/test-require-module-instantiated.mjs",
    "test/es-module/test-require-module-preload.js",
    "test/es-module/test-require-module-retry-import-errored-2.js",
    "test/es-module/test-require-module-retry-import-errored.js",
    "test/es-module/test-require-module-retry-import-evaluating.js",
    "test/es-module/test-require-module-synchronous-rejection-handling.js",
    "test/es-module/test-require-module-twice.js",
    "test/es-module/test-require-module-with-detection.js",
    "test/es-module/test-require-module.js",
    "test/module-hooks/test-module-hooks-create-require-with-url.mjs",
    "test/module-hooks/test-module-hooks-import-wasm.mjs",
    "test/module-hooks/test-module-hooks-load-builtin-import.mjs",
    "test/module-hooks/test-module-hooks-load-builtin-override-module.js",
    "test/module-hooks/test-module-hooks-load-builtin-require.js",
    "test/module-hooks/test-module-hooks-load-chained.js",
    "test/module-hooks/test-module-hooks-load-detection.js",
    "test/module-hooks/test-module-hooks-load-import-cjs-custom-source.js",
    "test/module-hooks/test-module-hooks-load-import-cjs.js",
    "test/module-hooks/test-module-hooks-load-url-change-require.js",
    "test/module-hooks/test-module-hooks-resolve-builtin-builtin-import.mjs",
    "test/module-hooks/test-module-hooks-resolve-builtin-builtin-require.js",
    "test/module-hooks/test-module-hooks-resolve-builtin-on-disk-require-with-prefix.js",
    "test/module-hooks/test-module-hooks-resolve-builtin-on-disk-require.js",
    "test/module-hooks/test-module-hooks-resolve-load-builtin-override-both-prefix.js",
    "test/module-hooks/test-module-hooks-resolve-load-builtin-override-both.js",
    "test/module-hooks/test-module-hooks-resolve-load-builtin-redirect-prefix.js",
    "test/module-hooks/test-module-hooks-resolve-load-builtin-redirect.js",
    "test/module-hooks/test-module-hooks-resolve-require-resolve-loaded-with-source.js",
    "test/parallel/test-util-callbackify.js",
    "test/parallel/test-util-inspect-regexp.js",
];

fn esm_module_loader_promoted_fixture_paths(groups: &[&[&str]]) -> Vec<String> {
    groups
        .iter()
        .flat_map(|group| group.iter().copied())
        .map(str::to_string)
        .collect()
}

#[test]
fn node22_supported_lane_executes_esm_module_loader_promoted_batch_fixture() {
    let fixture_paths = esm_module_loader_promoted_fixture_paths(&[
        ESM_MODULE_LOADER_PROMOTED_COMMON_PATHS,
        ESM_MODULE_LOADER_PROMOTED_NODE22_ONLY_PATHS,
    ]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-esm-module-loader-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_esm_module_loader_promoted_batch_fixture() {
    let fixture_paths = esm_module_loader_promoted_fixture_paths(&[
        ESM_MODULE_LOADER_PROMOTED_COMMON_PATHS,
        ESM_MODULE_LOADER_PROMOTED_NODE24_ONLY_PATHS,
    ]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-esm-module-loader-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_esm_module_loader_promoted_batch_fixture() {
    let fixture_paths = esm_module_loader_promoted_fixture_paths(&[
        ESM_MODULE_LOADER_PROMOTED_NODE26_PATHS,
        ESM_MODULE_LOADER_NODE26_CYCLE21_PROMOTED_PATHS,
    ]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-esm-module-loader-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

const NDS3_FORK_CYCLE8_PROMOTED_COMMON_PATHS: &[&str] =
    &["test/es-module/test-esm-import-meta-main.mjs"];

#[test]
fn node22_supported_lane_executes_nds3_fork_cycle8_promoted_batch_fixture() {
    let fixture_paths = NDS3_FORK_CYCLE8_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-nds3-fork-cycle8-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_nds3_fork_cycle8_promoted_batch_fixture() {
    let fixture_paths = NDS3_FORK_CYCLE8_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-nds3-fork-cycle8-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked ESM/module-loader required-gap inventory; keep ignored until root-cause clusters are fixed or precisely classified"]
fn node22_supported_lane_esm_module_loader_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node22,
        esm_module_loader_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-esm-module-loader-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked ESM/module-loader required-gap inventory; keep ignored until root-cause clusters are fixed or precisely classified"]
fn node24_default_lane_esm_module_loader_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node24,
        esm_module_loader_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-esm-module-loader-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 diagnostic: refined ESM/package-semantics inventory; keep ignored until a real module-loader implementation wave is selected"]
fn node22_supported_lane_esm_inprocess_module_loader_watchpoint() {
    let fixture_paths =
        esm_inprocess_module_loader_required_gap_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-esm-inprocess-module-loader-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 diagnostic: refined ESM/package-semantics inventory; keep ignored until a real module-loader implementation wave is selected"]
fn node24_default_lane_esm_inprocess_module_loader_watchpoint() {
    let fixture_paths =
        esm_inprocess_module_loader_required_gap_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-esm-inprocess-module-loader-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 focused root-cause closure: data URL ESM imports; promote only after the broad ESM batch confirms the cluster is green"]
fn node22_supported_lane_esm_data_url_cluster_watchpoint() {
    let fixture_paths: Vec<String> = ESM_DATA_URL_CLUSTER_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-esm-data-url-cluster-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 focused root-cause closure: data URL ESM imports; promote only after the broad ESM batch confirms the cluster is green"]
fn node24_default_lane_esm_data_url_cluster_watchpoint() {
    let fixture_paths: Vec<String> = ESM_DATA_URL_CLUSTER_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-esm-data-url-cluster-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 focused pre-run: JSON/data URL/import-attributes required-surface slice; promote only after the broad module-loader batch confirms the pass delta"]
fn node22_supported_lane_esm_json_data_import_attributes_required_surface_watchpoint() {
    let fixture_paths =
        module_loader_json_data_import_attributes_required_surface_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-esm-json-data-import-attributes-required-surface-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 focused pre-run: JSON/data URL/import-attributes required-surface slice; promote only after the broad module-loader batch confirms the pass delta"]
fn node24_default_lane_esm_json_data_import_attributes_required_surface_watchpoint() {
    let fixture_paths =
        module_loader_json_data_import_attributes_required_surface_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-esm-json-data-import-attributes-required-surface-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

const ASYNC_HOOKS_REQUIRED_GAP_EXTRA_RUNTIME_FILES: &[&str] =
    &["test/fixtures/person-large.jpg"];

const ASYNC_HOOKS_REQUIRED_GAP_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/async-hooks",
    "test/fixtures/keys",
];

const ASYNC_HOOKS_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/async-hooks/test-async-local-storage-args.js",
    "test/async-hooks/test-async-local-storage-async-await.js",
    "test/async-hooks/test-async-local-storage-enable-disable.js",
    "test/async-hooks/test-async-local-storage-enter-with.js",
    "test/async-hooks/test-async-local-storage-http-agent.js",
    "test/async-hooks/test-async-local-storage-http.js",
    "test/async-hooks/test-async-local-storage-misc-stores.js",
    "test/async-hooks/test-async-local-storage-nested.js",
    "test/async-hooks/test-async-local-storage-no-mix-contexts.js",
    "test/async-hooks/test-async-local-storage-promises.js",
    "test/async-hooks/test-async-local-storage-thenable.js",
    "test/async-hooks/test-embedder.api.async-resource-no-type.js",
    "test/async-hooks/test-embedder.api.async-resource.runInAsyncScope.js",
    "test/async-hooks/test-emit-after-on-destroyed.js",
    "test/async-hooks/test-emit-before-after.js",
    "test/async-hooks/test-emit-before-on-destroyed.js",
    "test/async-hooks/test-filehandle-no-reuse.js",
    "test/async-hooks/test-fseventwrap.js",
    "test/async-hooks/test-fsreqcallback-access.js",
    "test/async-hooks/test-graph.fsreq-readFile.js",
    "test/async-hooks/test-graph.intervals.js",
    "test/async-hooks/test-graph.statwatcher.js",
    "test/async-hooks/test-graph.timeouts.js",
    "test/async-hooks/test-immediate.js",
    "test/async-hooks/test-improper-order.js",
    "test/async-hooks/test-improper-unwind.js",
    "test/async-hooks/test-no-assert-when-disabled.js",
    "test/async-hooks/test-promise.chain-promise-before-init-hooks.js",
    "test/async-hooks/test-timers.setTimeout.js",
    "test/async-hooks/test-unhandled-exception-valid-ids.js",
    "test/parallel/test-async-hooks-close-during-destroy.js",
    "test/parallel/test-async-hooks-destroy-on-gc.js",
    "test/parallel/test-async-hooks-disable-gc-tracking.js",
    "test/parallel/test-async-hooks-http-agent-destroy.js",
    "test/parallel/test-async-hooks-http-agent.js",
    "test/parallel/test-async-hooks-prevent-double-destroy.js",
    "test/parallel/test-async-hooks-run-in-async-scope-caught-exception.js",
    "test/parallel/test-async-hooks-vm-gc.js",
    // NDS3 v2.8.2-nimbus.4 promotion wave (ground-truth verified on the repinned
    // fork plus the op_nimbus_runtime_test_force_gc `reentrant` fix that unblocks
    // GC-destroy hooks creating async ids). Each path below passed the node22 AND
    // node24 nonblocking async-hooks required-gap watchpoint.
    "test/async-hooks/test-async-await.js",
    "test/async-hooks/test-async-exec-resource-match.js",
    "test/async-hooks/test-async-local-storage-async-functions.js",
    "test/async-hooks/test-async-local-storage-errors.js",
    "test/async-hooks/test-async-local-storage-gcable.js",
    "test/async-hooks/test-async-wrap-providers.js",
    "test/async-hooks/test-callback-error.js",
    "test/async-hooks/test-destroy-not-blocked.js",
    "test/async-hooks/test-disable-in-init.js",
    "test/async-hooks/test-embedder.api.async-resource.js",
    "test/async-hooks/test-emit-init.js",
    "test/async-hooks/test-enable-disable.js",
    "test/async-hooks/test-enable-in-init.js",
    "test/async-hooks/test-fsreqcallback-readFile.js",
    "test/async-hooks/test-getaddrinforeqwrap.js",
    "test/async-hooks/test-getnameinforeqwrap.js",
    "test/async-hooks/test-late-hook-enable.js",
    "test/async-hooks/test-nexttick-default-trigger.js",
    "test/async-hooks/test-promise.js",
    "test/async-hooks/test-promise.promise-before-init-hooks.js",
    "test/async-hooks/test-querywrap.js",
    "test/async-hooks/test-queue-microtask.js",
    "test/async-hooks/test-shutdownwrap.js",
    "test/async-hooks/test-statwatcher.js",
    "test/async-hooks/test-timers.setInterval.js",
    "test/async-hooks/test-unhandled-rejection-context.js",
    "test/async-hooks/test-writewrap.js",
    "test/parallel/test-async-hooks-fatal-error.js",
    "test/parallel/test-async-hooks-stack-overflow-nested-async.js",
    "test/parallel/test-async-hooks-stack-overflow.js",
    "test/parallel/test-async-hooks-top-level-clearimmediate.js",
    // NDS3 cycle 3b broken-chunk recovery: this required-gap fixture lost its
    // census result to a chunk-level timeout (rc 124/133 discarded ~19 fast
    // neighbors). Re-run individually under an OS-level gtimeout it passes
    // cleanly on both lanes (node22 + node24) with the async-hooks batch
    // staging; ground-truth verified in the non-ignored batch below.
    "test/async-hooks/test-async-exec-resource-http-32060.js",
];

const ASYNC_HOOKS_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-async-hooks-enabledhooksexits.js",
    // NDS3 v2.8.2-nimbus.4 promotion wave: Node24-line promise-tracking surface,
    // ground-truth verified on the node24 nonblocking async-hooks watchpoint.
    "test/async-hooks/test-track-promises-default.js",
    "test/async-hooks/test-track-promises-false-check.js",
    "test/async-hooks/test-track-promises-false.js",
    "test/async-hooks/test-track-promises-true.js",
    "test/async-hooks/test-track-promises-validation.js",
];

const ASYNC_HOOKS_PROMOTED_NODE26_PATHS: &[&str] = &[
    "test/async-hooks/test-async-await.js",
    "test/async-hooks/test-async-exec-resource-match.js",
    "test/async-hooks/test-async-local-storage-args.js",
    "test/async-hooks/test-async-local-storage-async-await.js",
    "test/async-hooks/test-async-local-storage-async-functions.js",
    "test/async-hooks/test-async-local-storage-enable-disable.js",
    "test/async-hooks/test-async-local-storage-enter-with.js",
    "test/async-hooks/test-async-local-storage-errors.js",
    "test/async-hooks/test-async-local-storage-gcable.js",
    "test/async-hooks/test-async-local-storage-http-agent.js",
    "test/async-hooks/test-async-local-storage-http.js",
    "test/async-hooks/test-async-local-storage-misc-stores.js",
    "test/async-hooks/test-async-local-storage-nested.js",
    "test/async-hooks/test-async-local-storage-no-mix-contexts.js",
    "test/async-hooks/test-async-local-storage-promises.js",
    "test/async-hooks/test-async-local-storage-stream-finished.js",
    "test/async-hooks/test-async-local-storage-thenable.js",
    "test/async-hooks/test-async-wrap-providers.js",
    "test/async-hooks/test-callback-error.js",
    "test/async-hooks/test-destroy-not-blocked.js",
    "test/async-hooks/test-disable-in-init.js",
    "test/async-hooks/test-embedder.api.async-resource-no-type.js",
    "test/async-hooks/test-embedder.api.async-resource.js",
    "test/async-hooks/test-embedder.api.async-resource.runInAsyncScope.js",
    "test/async-hooks/test-emit-after-on-destroyed.js",
    "test/async-hooks/test-emit-before-after.js",
    "test/async-hooks/test-emit-before-on-destroyed.js",
    "test/async-hooks/test-emit-init.js",
    "test/async-hooks/test-enable-disable.js",
    "test/async-hooks/test-enable-in-init.js",
    "test/async-hooks/test-filehandle-no-reuse.js",
    "test/async-hooks/test-fseventwrap.js",
    "test/async-hooks/test-fsreqcallback-access.js",
    "test/async-hooks/test-fsreqcallback-readFile.js",
    "test/async-hooks/test-graph.fsreq-readFile.js",
    "test/async-hooks/test-graph.intervals.js",
    "test/async-hooks/test-graph.statwatcher.js",
    "test/async-hooks/test-graph.timeouts.js",
    "test/async-hooks/test-immediate.js",
    "test/async-hooks/test-improper-order.js",
    "test/async-hooks/test-improper-unwind.js",
    "test/async-hooks/test-late-hook-enable.js",
    "test/async-hooks/test-nexttick-default-trigger.js",
    "test/async-hooks/test-no-assert-when-disabled.js",
    "test/async-hooks/test-promise.chain-promise-before-init-hooks.js",
    "test/async-hooks/test-promise.js",
    "test/async-hooks/test-promise.promise-before-init-hooks.js",
    "test/async-hooks/test-queue-microtask.js",
    "test/async-hooks/test-shutdownwrap.js",
    "test/async-hooks/test-statwatcher.js",
    "test/async-hooks/test-timers.setInterval.js",
    "test/async-hooks/test-timers.setTimeout.js",
    "test/async-hooks/test-track-promises-default.js",
    "test/async-hooks/test-track-promises-false-check.js",
    "test/async-hooks/test-track-promises-false.js",
    "test/async-hooks/test-track-promises-true.js",
    "test/async-hooks/test-track-promises-validation.js",
    "test/async-hooks/test-unhandled-exception-valid-ids.js",
    "test/async-hooks/test-unhandled-rejection-context.js",
    "test/async-hooks/test-writewrap.js",
    "test/parallel/test-async-hooks-close-during-destroy.js",
    "test/parallel/test-async-hooks-destroy-on-gc.js",
    "test/parallel/test-async-hooks-disable-gc-tracking.js",
    "test/parallel/test-async-hooks-enable-recursive.js",
    "test/parallel/test-async-hooks-enabledhooksexits.js",
    "test/parallel/test-async-hooks-http-agent-destroy.js",
    "test/parallel/test-async-hooks-http-agent.js",
    "test/parallel/test-async-hooks-prevent-double-destroy.js",
    "test/parallel/test-async-hooks-run-in-async-scope-caught-exception.js",
    "test/parallel/test-async-hooks-stack-overflow-nested-async.js",
    "test/parallel/test-async-hooks-stack-overflow.js",
    "test/parallel/test-async-hooks-top-level-clearimmediate.js",
    "test/parallel/test-async-hooks-vm-gc.js",
];

#[test]
fn node22_async_hooks_enable_recursive_fsreqcallback_regression() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-async-hooks-enable-recursive-fsreqcallback-regression.js",
        "regression/async-hooks/test-async-hooks-enable-recursive-fsreqcallback.js",
        &[],
        NodeCompatLane::Node22,
    );
}

#[test]
fn node24_async_hooks_enable_recursive_fsreqcallback_regression() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-async-hooks-enable-recursive-fsreqcallback-regression.js",
        "regression/async-hooks/test-async-hooks-enable-recursive-fsreqcallback.js",
        &[],
        NodeCompatLane::Node24,
    );
}

#[test]
fn node22_supported_lane_executes_async_hooks_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = ASYNC_HOOKS_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-async-hooks-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_RUNTIME_FILES,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_async_hooks_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = ASYNC_HOOKS_PROMOTED_COMMON_PATHS
        .iter()
        .chain(ASYNC_HOOKS_PROMOTED_NODE24_ONLY_PATHS.iter())
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-async-hooks-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_RUNTIME_FILES,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_async_hooks_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = ASYNC_HOOKS_PROMOTED_NODE26_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-async-hooks-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_RUNTIME_FILES,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_DIRS,
    );
}

// NDS3 census promotion wave (workflow-independent empirical census of every
// node22 + node24 required-gap path, run one-fixture-per-process under a hard
// timeout with cluster-appropriate support dirs). Each path below produced a
// clean PASS verdict in process-isolated census AND is re-verified here as an
// in-batch pass by the non-ignored batch tests, so it is promoted from a
// required gap to measured default-lane support. Split common (passes on both
// lines) vs lane-only to mirror the census's per-lane verdicts exactly.
const NDS3_CENSUS_PROMOTED_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/async-hooks",
    "test/es-module",
    "test/fixtures/es-modules",
    "test/fixtures/keys",
];

const NDS3_CENSUS_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/async-hooks/test-async-exec-resource-http-agent.js",
    "test/async-hooks/test-async-exec-resource-http.js",
    "test/es-module/test-dynamic-import-script-lifetime.js",
    "test/parallel/test-async-local-storage-isolation.js",
    "test/parallel/test-async-wrap-promise-after-enabled.js",
    "test/parallel/test-async-wrap-trigger-id.js",
    "test/parallel/test-console-diagnostics-channels.js",
    "test/parallel/test-console-with-frozen-intrinsics.js",
    "test/parallel/test-diagnostic-channel-http-request-created.js",
    "test/parallel/test-diagnostic-channel-http-response-created.js",
    "test/parallel/test-dns-channel-cancel-promise.js",
    "test/parallel/test-dns-lookup-promises-options-deprecated.js",
    "test/parallel/test-dns-lookup-promises.js",
    "test/parallel/test-dns-perf_hooks.js",
    "test/parallel/test-dns-promises-exists.js",
    "test/parallel/test-fs-sir-writes-alot.js",
    "test/parallel/test-fs-write-buffer-large.js",
    "test/parallel/test-gc-tls-external-memory.js",
    "test/parallel/test-process-getactiveresources-track-interval-lifetime.js",
    "test/parallel/test-process-getactiveresources.js",
    "test/parallel/test-v8-collect-gc-profile-exit-before-stop.js",
    "test/parallel/test-v8-collect-gc-profile.js",
    "test/parallel/test-v8-getheapsnapshot-twice.js",
    "test/parallel/test-v8-global-setter.js",
    "test/parallel/test-vm-module-dynamic-import-promise.js",
    "test/parallel/test-vm-module-dynamic-namespace.js",
    "test/parallel/test-vm-module-evaluate-source-text-module.js",
    "test/parallel/test-vm-module-evaluate-synthethic-module-rejection.js",
    "test/parallel/test-vm-module-evaluate-synthethic-module.js",
    "test/parallel/test-vm-module-instantiate.js",
    "test/parallel/test-vm-module-link-shared-deps.js",
    "test/parallel/test-vm-module-link.js",
    "test/parallel/test-vm-module-linkmodulerequests-circular.js",
    "test/parallel/test-vm-module-linkmodulerequests-deep.js",
    "test/parallel/test-vm-module-linkmodulerequests.js",
    "test/parallel/test-vm-module-reevaluate.js",
    "test/parallel/test-vm-no-dynamic-import-callback.js",
];

const NDS3_CENSUS_PROMOTED_NODE22_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-dgram-cluster-close-in-listening.js",
    "test/parallel/test-dgram-unref-in-cluster.js",
    "test/parallel/test-performance-gc.js",
    "test/parallel/test-webcrypto-encrypt-decrypt.js",
    "test/parallel/test-whatwg-encoding-encodeinto-large.js",
];

const NDS3_CENSUS_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-async-local-storage-http-parser-leak.js",
    "test/parallel/test-console.js",
    "test/parallel/test-fs-promises-watch-ignore-invalid.mjs",
    "test/parallel/test-fs-promises-watch.js",
    "test/parallel/test-v8-collect-gc-profile-using.js",
    "test/parallel/test-vm-module-modulerequests.js",
];

#[test]
fn node22_supported_lane_executes_nds3_census_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NDS3_CENSUS_PROMOTED_COMMON_PATHS
        .iter()
        .chain(NDS3_CENSUS_PROMOTED_NODE22_ONLY_PATHS.iter())
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-nds3-census-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CENSUS_PROMOTED_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_nds3_census_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NDS3_CENSUS_PROMOTED_COMMON_PATHS
        .iter()
        .chain(NDS3_CENSUS_PROMOTED_NODE24_ONLY_PATHS.iter())
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-nds3-census-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CENSUS_PROMOTED_EXTRA_DIRS,
    );
}

// NDS3 deno_node fork-method promotion wave (nimbus/deno v2.8.2-nimbus.5). Each
// fixture below is a genuine Application-API required gap whose only blocker was
// a missing `ext/node/polyfills` method, ported faithfully against upstream Node
// semantics in the fork (process.assert -> DEP0100 ERR_ASSERTION; process.ref/
// unref -> Symbol.for("nodejs.ref"/"nodejs.unref")-then-method dispatch matching
// lib/internal/process/per_thread.js; util.getCallSite -> ExperimentalWarning
// rename alias for util.getCallSites). With that fork tag pinned, each fixture
// executes green in-isolate, so it is promoted from a required gap to measured
// default-lane support and re-verified here as a non-ignored in-batch pass.
const NDS3_FORK_METHOD_PROMOTED_EXTRA_DIRS: &[&str] = &["test/common"];

const NDS3_FORK_METHOD_PROMOTED_NODE22_PATHS: &[&str] = &[
    "test/parallel/test-process-assert.js",
    "test/parallel/test-process-ref-unref.js",
    "test/parallel/test-util-getcallsite.js",
];

const NDS3_FORK_METHOD_PROMOTED_NODE24_PATHS: &[&str] =
    &["test/parallel/test-process-ref-unref.js"];

#[test]
fn node22_supported_lane_executes_nds3_fork_method_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NDS3_FORK_METHOD_PROMOTED_NODE22_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-nds3-fork-method-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_FORK_METHOD_PROMOTED_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_nds3_fork_method_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NDS3_FORK_METHOD_PROMOTED_NODE24_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-nds3-fork-method-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_FORK_METHOD_PROMOTED_EXTRA_DIRS,
    );
}

// NDS3 cycle-3 domain fork promotion wave (nimbus/deno v2.8.2-nimbus.8). The
// `node:domain` polyfill diverged from Node lib/domain.js in two ways that
// corrupted the domains-stack/active-domain bookkeeping a domain's error handler
// observes (synchronously and from a nextTick scheduled inside it):
//   1. Domain.exit() set the active domain to `null` on an empty stack, where
//      Node yields `undefined` (stack[-1]); Node reserves `null` for the active
//      domain only in synchronous emit-error routing and the top-level uncaught
//      handler.
//   2. The post-error-handler restore rebound the `stack` local to the saved
//      array, leaving the exported `_stack` reference pointing at the emptied
//      original array (Node's `exports._stack` is a live binding, so its raw
//      reassignment is safe; ours is not).
// Both are fixed in the fork polyfill (exit() -> undefined-on-empty; in-place
// truncate+repush restore that preserves `_stack` identity). With that fork tag
// pinned, the fixture executes green in-isolate on BOTH lanes (process-isolated
// census + the non-ignored in-batch pass below), so it is promoted from a
// v8-isolate-required gap to measured default-lane support.
const NDS3_DOMAIN_FORK_PROMOTED_EXTRA_DIRS: &[&str] = &["test/common"];

const NDS3_DOMAIN_FORK_PROMOTED_COMMON_PATHS: &[&str] =
    &["test/parallel/test-domain-emit-error-handler-stack.js"];

#[test]
fn node22_supported_lane_executes_nds3_domain_fork_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NDS3_DOMAIN_FORK_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-nds3-domain-fork-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_DOMAIN_FORK_PROMOTED_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_nds3_domain_fork_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NDS3_DOMAIN_FORK_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-nds3-domain-fork-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_DOMAIN_FORK_PROMOTED_EXTRA_DIRS,
    );
}

// NDS3 event-loop-lifecycle promotion wave. Each fixture below is a genuine
// Application-API required gap whose only blocker was that the Nimbus runtime
// driver never natively emits the Node `beforeExit`/`exit` process lifecycle (or
// honors a `process.exit()` short-circuit) at the end of a fixture module. The
// harness compensates with per-fixture lifecycle preludes/postludes
// (process_exit_sentinel / process_before_exit_reentry /
// process_before_exit_throw_to_exit / process_lifecycle_drain) assigned in
// default_prelude_behavior_for_fixture / default_postlude_behavior_for_fixture.
// These are pure nimbus test-crate mechanisms with no fork dependency; with them
// each fixture executes green in-isolate on BOTH lanes (verified individually
// under a hard external timeout), so it is promoted from a required gap to
// measured default-lane support and re-verified here as a non-ignored in-batch
// pass. upstream Deno passes each of these in tests/node_compat/config.jsonc,
// confirming the gap was a Nimbus-harness divergence, not a Deno-level gap.
const NDS3_LIFECYCLE_PROMOTED_EXTRA_DIRS: &[&str] = &["test/common"];

const NDS3_LIFECYCLE_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-beforeexit-event-exit.js",
    "test/parallel/test-process-beforeexit-throw-exit.js",
    "test/parallel/test-process-exit-from-before-exit.js",
    "test/parallel/test-process-exit-recursive.js",
    "test/parallel/test-timers-unrefed-in-beforeexit.js",
];

#[test]
fn node22_supported_lane_executes_nds3_lifecycle_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NDS3_LIFECYCLE_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-nds3-lifecycle-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_LIFECYCLE_PROMOTED_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_nds3_lifecycle_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NDS3_LIFECYCLE_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-nds3-lifecycle-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_LIFECYCLE_PROMOTED_EXTRA_DIRS,
    );
}

// NDS3 fork-cycle-1 promotion wave (nimbus/deno v2.8.2-nimbus.6). The fork tag
// adds process.getActiveResourcesInfo() request/handle/timer tracking with Node
// provider names (process.ts + internal/process/active_resources.ts + net.ts +
// internal/timers.mjs), legacy timers enroll/unenroll/active/_unrefActive with
// DEP0095/0096/0126/0127 via lazy util.deprecate (timers.ts), the process.binding
// util allowlist (process.ts + 01_require.js), process.emit ReflectApply/unshift
// hardening, and a <Revoked Proxy> console inspect path (ext/web/01_console.js).
// Each fixture below is a genuine Application-API required gap that produced a
// clean PASS in the process-isolated fork-cycle-1 census on the repinned tag AND
// is re-verified here as a non-ignored in-batch pass, so it is promoted from a
// required gap to measured default-lane support. Common = passes on both lines;
// node22-only = legacy-timers fixtures that exist (and pass) only on the node22
// line (the node24 corpus does not vendor them).
const NDS3_FORK_CYCLE1_PROMOTED_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/async-hooks",
    "test/es-module",
    "test/fixtures/es-modules",
    "test/fixtures/keys",
];

const NDS3_FORK_CYCLE1_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-console-issue-43095.js",
    "test/parallel/test-internal-process-binding.js",
    "test/parallel/test-process-binding-util.js",
    "test/parallel/test-process-emit.js",
    "test/parallel/test-process-getactiveresources-track-active-handles.js",
    "test/parallel/test-process-getactiveresources-track-active-requests.js",
    "test/parallel/test-process-getactiveresources-track-multiple-timers.js",
    "test/parallel/test-process-getactiveresources-track-timer-lifetime.js",
];

const NDS3_FORK_CYCLE1_PROMOTED_NODE22_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-timers-active.js",
    "test/parallel/test-timers-enroll-invalid-msecs.js",
    "test/parallel/test-timers-enroll-second-time.js",
    "test/parallel/test-timers-max-duration-warning.js",
    "test/parallel/test-timers-unenroll-unref-interval.js",
    "test/parallel/test-timers-unref-active.js",
    "test/parallel/test-timers-unref-remove-other-unref-timers-only-one-fires.js",
    "test/parallel/test-timers-unref-remove-other-unref-timers.js",
];

#[test]
fn node22_supported_lane_executes_nds3_fork_cycle1_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NDS3_FORK_CYCLE1_PROMOTED_COMMON_PATHS
        .iter()
        .chain(NDS3_FORK_CYCLE1_PROMOTED_NODE22_ONLY_PATHS.iter())
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-nds3-fork-cycle1-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_FORK_CYCLE1_PROMOTED_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_nds3_fork_cycle1_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NDS3_FORK_CYCLE1_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-nds3-fork-cycle1-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_FORK_CYCLE1_PROMOTED_EXTRA_DIRS,
    );
}

// NDS3 fork cycle 2 (fork tag v2.8.2-nimbus.7). Each path below produced a clean
// PASS in the process-isolated cycle-2 census on the repinned tag AND is
// re-verified here as a non-ignored in-batch pass, which removes it from
// `requires_unpromoted_node_surface` on the next `classifications.py sync`. The
// fork edits behind these promotions: node:v8 `promiseHooks` registry (append-only
// core.setPromiseHooks trampolines + per-type splice-able user lists), node:url
// `URLPattern` re-export of the WHATWG URLPattern, the Nimbus-local
// `snapshotFsStreamOptions` rewrite (for-in inherited keys, non-object passthrough
// to the builtin, default-only `fs` injection), and the Nimbus-local
// `module_type_from_path` reverse import-attribute guard (a non-JSON file requested
// `with { type: 'json' }` now rejects). Common = passes on both lines; node24-only
// = urlpattern fixtures the node22 corpus does not vendor.
const NDS3_FORK_CYCLE2_PROMOTED_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/es-module",
    "test/fixtures/es-modules",
];

const NDS3_FORK_CYCLE2_PROMOTED_EXTRA_RUNTIME_FILES: &[&str] = &[
    "test/fixtures/elipses.txt",
    "test/fixtures/x.txt",
    "test/fixtures/empty.js",
    "test/fixtures/empty.json",
];

// Promoted = confirmed green on a per-fixture census against the repinned fork.
// The node:v8 `promiseHooks` registry greens the two hooks whose assertions run
// synchronously at promise creation (`on-init`, `on-before`); the `create-hook`,
// `on-after`, `on-resolve`, and `exceptions` fixtures assert during tick-drain,
// where the Nimbus bundle harness's own bootstrap/tick promises are observable to
// the global v8 promise-hook surface and over-count against bare-fixture
// expectations -- a harness-observability gap deferred to a dedicated suppression
// cycle, NOT a `promiseHooks` API defect (see nds3-fork-cycle2-result.md). The two
// `test-fs-read-stream{,-pos}.js` fixtures stay genuine structural gaps: the former
// needs an mkfifo named pipe the isolate does not provide, the latter calls
// `process.exit` -> `Deno.exit`, deliberately absent in a multi-tenant isolate.
const NDS3_FORK_CYCLE2_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-promise-hook-on-init.js",
    "test/parallel/test-promise-hook-on-before.js",
    "test/parallel/test-fs-read-stream-inherit.js",
    "test/parallel/test-fs-read-stream-throw-type-error.js",
    "test/es-module/test-esm-dynamic-import-attribute.js",
    "test/es-module/test-esm-dynamic-import-attribute.mjs",
];

const NDS3_FORK_CYCLE2_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-urlpattern-invalidthis.js",
    "test/parallel/test-urlpattern.js",
];

#[test]
fn node22_supported_lane_executes_nds3_fork_cycle2_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NDS3_FORK_CYCLE2_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-nds3-fork-cycle2-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        NDS3_FORK_CYCLE2_PROMOTED_EXTRA_RUNTIME_FILES,
        NDS3_FORK_CYCLE2_PROMOTED_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_nds3_fork_cycle2_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NDS3_FORK_CYCLE2_PROMOTED_COMMON_PATHS
        .iter()
        .chain(NDS3_FORK_CYCLE2_PROMOTED_NODE24_ONLY_PATHS.iter())
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-nds3-fork-cycle2-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        NDS3_FORK_CYCLE2_PROMOTED_EXTRA_RUNTIME_FILES,
        NDS3_FORK_CYCLE2_PROMOTED_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked async_hooks required-gap inventory; classify AsyncLocalStorage, promise hooks, provider lifecycle, graph/network, timer/task, GC, and host-owned failures after the first wide run"]
fn node22_supported_lane_async_hooks_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node22,
        async_hooks_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-async-hooks-required-gap-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_RUNTIME_FILES,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked async_hooks required-gap inventory; classify AsyncLocalStorage, promise hooks, provider lifecycle, graph/network, timer/task, GC, and host-owned failures after the first wide run"]
fn node24_default_lane_async_hooks_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node24,
        async_hooks_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-async-hooks-required-gap-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_RUNTIME_FILES,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 P3 measure: async_hooks required-gap fixtures with the socket-bind networking subset excluded, so the batch completes and writes an honest summary on the v2.8.2-nimbus.2 promise-lifecycle wiring; the excluded networking fixtures are the structural-networking tension owned separately"]
fn node22_supported_lane_async_hooks_nonblocking_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node22,
        async_hooks_nonblocking_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-async-hooks-nonblocking-required-gap-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_RUNTIME_FILES,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 P3 measure: async_hooks required-gap fixtures with the socket-bind networking subset excluded, so the batch completes and writes an honest summary on the v2.8.2-nimbus.2 promise-lifecycle wiring; the excluded networking fixtures are the structural-networking tension owned separately"]
fn node24_default_lane_async_hooks_nonblocking_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node24,
        async_hooks_nonblocking_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-async-hooks-nonblocking-required-gap-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_RUNTIME_FILES,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 node26 broad pre-run: async_hooks required-gap fixtures with socket-bind networking excluded; promote only dynamically green lifecycle fixtures"]
fn node26_current_lane_async_hooks_nonblocking_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node26,
        async_hooks_nonblocking_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-async-hooks-nonblocking-required-gap-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_RUNTIME_FILES,
        ASYNC_HOOKS_REQUIRED_GAP_EXTRA_DIRS,
    );
}

const WEBCRYPTO_REQUIRED_GAP_COMMON_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures/crypto",
    "test/fixtures/keys",
];

const WEBCRYPTO_REQUIRED_GAP_NODE24_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures/crypto",
    "test/fixtures/keys",
    "test/fixtures/webcrypto",
];

const WEBCRYPTO_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-webcrypto-constructors.js",
    "test/parallel/test-webcrypto-derivebits.js",
    "test/parallel/test-webcrypto-digest.js",
    "test/parallel/test-webcrypto-getRandomValues.js",
    "test/parallel/test-webcrypto-random.js",
    // Cycle-9 free promotion: ECDSA and HMAC sign/verify pass dynamically on
    // both lanes against the cycle-8 fork crypto baseline (verified by the
    // webcrypto green-guard batch; HMAC was previously Node24-only).
    "test/parallel/test-webcrypto-sign-verify-ecdsa.js",
    "test/parallel/test-webcrypto-sign-verify-hmac.js",
];

// Wave 19 broad diagnostics proved this path only on the Node22 lane. Keep it
// lane-local until Node24's newer KMAC/raw-secret tails are fixed.
const WEBCRYPTO_PROMOTED_NODE22_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-webcrypto-derivekey.js",
];

// Wave 19 broad diagnostics proved these Node24 paths after Deno-owned
// WebCrypto key-view, prototype-self-call, derive, digest, and HMAC fixes.
const WEBCRYPTO_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-webcrypto-derivebits-cfrg.js",
    "test/parallel/test-webcrypto-derivebits-ecdh.js",
    "test/parallel/test-webcrypto-derivekey-cfrg.js",
    "test/parallel/test-webcrypto-derivekey-ecdh.js",
    "test/parallel/test-webcrypto-encrypt-decrypt-rsa.js",
    "test/parallel/test-webcrypto-encrypt-decrypt.js",
    "test/parallel/test-webcrypto-get-public-key.mjs",
    "test/parallel/test-webcrypto-internal-slots.mjs",
    "test/parallel/test-webcrypto-sign-verify-eddsa.js",
];

const WEBCRYPTO_PROMOTED_NODE26_PATHS: &[&str] = &[
    "test/parallel/test-webcrypto-aead-decrypt-detached-buffer.js",
    "test/parallel/test-webcrypto-deduplicate-usages.js",
    "test/parallel/test-webcrypto-derivebits.js",
    "test/parallel/test-webcrypto-derivekey.js",
    "test/parallel/test-webcrypto-encrypt-decrypt-aes.js",
    "test/parallel/test-webcrypto-encrypt-decrypt-rsa.js",
    "test/parallel/test-webcrypto-encrypt-decrypt.js",
    "test/parallel/test-webcrypto-export-import.js",
    "test/parallel/test-webcrypto-getRandomValues.js",
    "test/parallel/test-webcrypto-keygen-kmac.js",
    "test/parallel/test-webcrypto-keygen.js",
    "test/parallel/test-webcrypto-random.js",
    "test/parallel/test-webcrypto-sign-verify-kmac.js",
    "test/parallel/test-webcrypto-sign-verify.js",
    "test/parallel/test-webcrypto-wrap-unwrap.js",
];

#[test]
fn node22_supported_lane_executes_webcrypto_promoted_batch_fixture() {
    let fixture_paths = WEBCRYPTO_PROMOTED_COMMON_PATHS
        .iter()
        .chain(WEBCRYPTO_PROMOTED_NODE22_ONLY_PATHS.iter())
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-webcrypto-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        WEBCRYPTO_REQUIRED_GAP_COMMON_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_webcrypto_promoted_batch_fixture() {
    let fixture_paths = WEBCRYPTO_PROMOTED_COMMON_PATHS
        .iter()
        .chain(WEBCRYPTO_PROMOTED_NODE24_ONLY_PATHS.iter())
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-webcrypto-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        WEBCRYPTO_REQUIRED_GAP_NODE24_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_webcrypto_promoted_batch_fixture() {
    let fixture_paths = WEBCRYPTO_PROMOTED_NODE26_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-webcrypto-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        WEBCRYPTO_REQUIRED_GAP_NODE24_EXTRA_DIRS,
    );
}

// AES-GCM/RSA-OAEP encrypt/decrypt fixtures promoted on the Node22 supported
// lane only. The green path depends on two fork/runtime fixes that landed for
// the v2.8.2-nimbus.13 pin: the DOMException op-error builders registered in
// `98_global_scope_shared.js` (so OperationError surfaces with the right name)
// and the AES-GCM truncated authentication-tag decrypt support in the fork's
// `ext/crypto/decrypt.rs`. Node24's version-conditional encrypt/decrypt
// dispatch remains wave-B, so these stay out of the shared common batch.
const WEBCRYPTO_PROMOTED_NODE22_ENCRYPT_DECRYPT_PATHS: &[&str] = &[
    "test/parallel/test-webcrypto-encrypt-decrypt-aes.js",
    "test/parallel/test-webcrypto-encrypt-decrypt-rsa.js",
];

#[test]
fn node22_supported_lane_executes_webcrypto_encrypt_decrypt_batch_fixture() {
    let fixture_paths = WEBCRYPTO_PROMOTED_NODE22_ENCRYPT_DECRYPT_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-webcrypto-encrypt-decrypt-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        WEBCRYPTO_REQUIRED_GAP_COMMON_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked WebCrypto required-gap inventory; classify algorithms, key import/export, error shape, host-policy, and Node24-only fixtures after the first wide run"]
fn node22_supported_lane_webcrypto_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node22,
        webcrypto_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-webcrypto-required-gap-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        WEBCRYPTO_REQUIRED_GAP_COMMON_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked WebCrypto required-gap inventory; classify algorithms, key import/export, error shape, host-policy, and Node24-only fixtures after the first wide run"]
fn node24_default_lane_webcrypto_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node24,
        webcrypto_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-webcrypto-required-gap-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        WEBCRYPTO_REQUIRED_GAP_NODE24_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 node26 broad pre-run: ROI-ranked WebCrypto required-gap inventory; promote only dynamically green Current-lane fixtures"]
fn node26_current_lane_webcrypto_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node26,
        webcrypto_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-webcrypto-required-gap-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        WEBCRYPTO_REQUIRED_GAP_NODE24_EXTRA_DIRS,
    );
}

// Cycle-9 free promotion outside the WebCrypto cluster. This fixture passed a
// dynamic green-guard run against the cycle-8 fork baseline with exactly the
// staging declared here (vendored `test/common` plus the `test/fixtures/syntax`
// module it imports), so it transfers from the v8_isolate_required gap set to
// manifested-green without any further fork change. The broader cycle-9 census
// candidates (vm dynamic-import error-code, WHATWG byte-stream validation, vm
// module referrer-realm, util.styleText) were rejected by the same green-guard
// as false greens — they self-skip or assert-mismatch dynamically — so they
// stay in the gap set pending real fork fixes.

// `test-esm-error-cache` re-imports a deliberately broken module and asserts the
// cached SyntaxError identity is preserved across the second dynamic import. It
// needs the shared `test/common` plus the `fixtures/syntax/bad_syntax.mjs`
// module it imports by relative specifier.
const ESM_ERROR_CACHE_EXTRA_DIRS: &[&str] = &["test/common", "test/fixtures/syntax"];

const ESM_ERROR_CACHE_PROMOTED_COMMON_PATHS: &[&str] =
    &["test/es-module/test-esm-error-cache.js"];

#[test]
fn node22_supported_lane_executes_esm_error_cache_promoted_batch_fixture() {
    let fixture_paths = ESM_ERROR_CACHE_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-esm-error-cache-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        ESM_ERROR_CACHE_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_esm_error_cache_promoted_batch_fixture() {
    let fixture_paths = ESM_ERROR_CACHE_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-esm-error-cache-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        ESM_ERROR_CACHE_EXTRA_DIRS,
    );
}

const EVENT_REQUIRED_GAP_EXTRA_DIRS: &[&str] = &["test/common"];

const EVENT_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-event-capture-rejections.js",
    "test/parallel/test-event-emitter-add-listeners.js",
    "test/parallel/test-event-emitter-check-listener-leaks.js",
    "test/parallel/test-event-emitter-emit-context.js",
    "test/parallel/test-event-emitter-error-monitor.js",
    "test/parallel/test-event-emitter-errors.js",
    "test/parallel/test-event-emitter-get-max-listeners.js",
    "test/parallel/test-event-emitter-invalid-listener.js",
    "test/parallel/test-event-emitter-listener-count.js",
    "test/parallel/test-event-emitter-listeners-side-effects.js",
    "test/parallel/test-event-emitter-listeners.js",
    "test/parallel/test-event-emitter-max-listeners-warning-for-null.js",
    "test/parallel/test-event-emitter-max-listeners-warning-for-symbol.js",
    "test/parallel/test-event-emitter-max-listeners-warning.js",
    "test/parallel/test-event-emitter-max-listeners.js",
    "test/parallel/test-event-emitter-method-names.js",
    "test/parallel/test-event-emitter-modify-in-emit.js",
    "test/parallel/test-event-emitter-no-error-provided-to-error-event.js",
    "test/parallel/test-event-emitter-num-args.js",
    "test/parallel/test-event-emitter-once.js",
    "test/parallel/test-event-emitter-prepend.js",
    "test/parallel/test-event-emitter-remove-all-listeners.js",
    "test/parallel/test-event-emitter-remove-listeners.js",
    "test/parallel/test-event-emitter-set-max-listeners-side-effects.js",
    "test/parallel/test-event-emitter-special-event-names.js",
    "test/parallel/test-event-emitter-subclass.js",
    "test/parallel/test-event-emitter-symbols.js",
    "test/parallel/test-event-target.js",
    "test/parallel/test-events-customevent.js",
    "test/parallel/test-events-on-async-iterator.js",
    "test/parallel/test-eventsource.js",
    "test/parallel/test-eventtarget-brandcheck.js",
    "test/parallel/test-eventtarget-custom-inspect-does-not-throw.js",
    "test/parallel/test-eventtarget-once-twice.js",
];

#[test]
fn node22_supported_lane_executes_event_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = EVENT_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-event-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        EVENT_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_event_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = EVENT_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-event-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        EVENT_REQUIRED_GAP_EXTRA_DIRS,
    );
}

const EVENT_PROMOTED_NODE26_PATHS: &[&str] = &[
    "test/parallel/test-event-capture-rejections.js",
    "test/parallel/test-event-emitter-add-listeners.js",
    "test/parallel/test-event-emitter-check-listener-leaks.js",
    "test/parallel/test-event-emitter-emit-context.js",
    "test/parallel/test-event-emitter-error-monitor.js",
    "test/parallel/test-event-emitter-errors.js",
    "test/parallel/test-event-emitter-get-max-listeners.js",
    "test/parallel/test-event-emitter-invalid-listener.js",
    "test/parallel/test-event-emitter-listener-count.js",
    "test/parallel/test-event-emitter-listeners-side-effects.js",
    "test/parallel/test-event-emitter-listeners.js",
    "test/parallel/test-event-emitter-max-listeners-warning-for-null.js",
    "test/parallel/test-event-emitter-max-listeners-warning-for-symbol.js",
    "test/parallel/test-event-emitter-max-listeners-warning.js",
    "test/parallel/test-event-emitter-max-listeners.js",
    "test/parallel/test-event-emitter-method-names.js",
    "test/parallel/test-event-emitter-modify-in-emit.js",
    "test/parallel/test-event-emitter-no-error-provided-to-error-event.js",
    "test/parallel/test-event-emitter-num-args.js",
    "test/parallel/test-event-emitter-once.js",
    "test/parallel/test-event-emitter-prepend.js",
    "test/parallel/test-event-emitter-remove-all-listeners.js",
    "test/parallel/test-event-emitter-remove-listeners.js",
    "test/parallel/test-event-emitter-set-max-listeners-side-effects.js",
    "test/parallel/test-event-emitter-special-event-names.js",
    "test/parallel/test-event-emitter-subclass.js",
    "test/parallel/test-event-emitter-symbols.js",
    "test/parallel/test-event-target.js",
    "test/parallel/test-eventemitter-asyncresource.js",
    "test/parallel/test-events-customevent.js",
    "test/parallel/test-events-on-async-iterator.js",
    "test/parallel/test-events-uncaught-exception-stack.js",
    "test/parallel/test-eventsource-disabled.js",
    "test/parallel/test-eventtarget-brandcheck.js",
    "test/parallel/test-eventtarget-custom-inspect-does-not-throw.js",
    "test/parallel/test-eventtarget-once-twice.js",
];

#[test]
fn node26_current_lane_executes_event_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = EVENT_PROMOTED_NODE26_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-event-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        EVENT_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked EventEmitter/EventTarget/EventSource required-gap inventory; classify clean event semantics, async resource context, web event targets, and host-only EventSource cases after the first wide run"]
fn node22_supported_lane_event_required_gap_watchpoint() {
    let fixture_paths =
        node_compat_required_gap_paths_for_selector(NodeCompatLane::Node22, event_required_gap_path);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-event-required-gap-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        EVENT_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked EventEmitter/EventTarget/EventSource required-gap inventory; classify clean event semantics, async resource context, web event targets, and host-only EventSource cases after the first wide run"]
fn node24_default_lane_event_required_gap_watchpoint() {
    let fixture_paths =
        node_compat_required_gap_paths_for_selector(NodeCompatLane::Node24, event_required_gap_path);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-event-required-gap-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        EVENT_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 node26 broad pre-run: ROI-ranked EventEmitter/EventTarget/EventSource required-gap inventory; promote only dynamically green Current-lane fixtures"]
fn node26_current_lane_event_required_gap_watchpoint() {
    let fixture_paths =
        node_compat_required_gap_paths_for_selector(NodeCompatLane::Node26, event_required_gap_path);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-event-required-gap-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        EVENT_REQUIRED_GAP_EXTRA_DIRS,
    );
}

const NETWORKING_CRYPTO_REQUIRED_GAP_EXTRA_RUNTIME_FILES: &[&str] =
    &["test/fixtures/aead-vectors.js"];

const NETWORKING_CRYPTO_REQUIRED_GAP_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures/crypto",
    "test/fixtures/keys",
];

const NETWORKING_CRYPTO_PROMOTED_COMMON_PATHS: &[&str] =
    &["test/parallel/test-crypto-authenticated-stream.js"];

const NETWORKING_CRYPTO_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-crypto-dh-stateless.js",
    "test/parallel/test-crypto-scrypt.js",
];

#[test]
fn node22_supported_lane_executes_networking_crypto_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NETWORKING_CRYPTO_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-networking-crypto-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        NETWORKING_CRYPTO_REQUIRED_GAP_EXTRA_RUNTIME_FILES,
        NETWORKING_CRYPTO_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_networking_crypto_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = NETWORKING_CRYPTO_PROMOTED_COMMON_PATHS
        .iter()
        .chain(NETWORKING_CRYPTO_PROMOTED_NODE24_ONLY_PATHS.iter())
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-networking-crypto-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        NETWORKING_CRYPTO_REQUIRED_GAP_EXTRA_RUNTIME_FILES,
        NETWORKING_CRYPTO_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked networking/crypto required-gap inventory; classify clean crypto semantics, host-provider boundaries, Node24-only algorithms, async callbacks, worker/messageport behavior, and error-shape failures after the first wide run"]
fn node22_supported_lane_networking_crypto_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node22,
        networking_crypto_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-networking-crypto-required-gap-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        NETWORKING_CRYPTO_REQUIRED_GAP_EXTRA_RUNTIME_FILES,
        NETWORKING_CRYPTO_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked networking/crypto required-gap inventory; classify clean crypto semantics, host-provider boundaries, Node24-only algorithms, async callbacks, worker/messageport behavior, and error-shape failures after the first wide run"]
fn node24_default_lane_networking_crypto_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node24,
        networking_crypto_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-networking-crypto-required-gap-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        NETWORKING_CRYPTO_REQUIRED_GAP_EXTRA_RUNTIME_FILES,
        NETWORKING_CRYPTO_REQUIRED_GAP_EXTRA_DIRS,
    );
}

const HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS: &[&str] = &["test/common"];

const HTTP_CLIENT_AGENT_DIAGNOSTIC_LOW_ROI_PATHS: &[&str] = &[
    "test/parallel/test-http-agent-domain-reused-gc.js",
    "test/parallel/test-http-agent-reuse-drained-socket-only.js",
    "test/parallel/test-http-client-leaky-with-double-response.js",
];

fn http_client_agent_diagnostic_path(path: &str) -> bool {
    path.starts_with("test/parallel/test-http-client")
        || path.starts_with("test/parallel/test-http-agent")
}

fn http_client_agent_diagnostic_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths = node_compat_posture_paths_for_selector(lane, |entry| {
        entry["support_denominator"] == "diagnostic_only_non_isolate"
            && entry["owner"] == "networking/http"
            && entry["test_path"]
                .as_str()
                .is_some_and(http_client_agent_diagnostic_path)
    });
    fixture_paths.retain(|path| {
        !HTTP_CLIENT_AGENT_DIAGNOSTIC_LOW_ROI_PATHS
            .iter()
            .any(|low_roi_path| path == low_roi_path)
    });
    assert!(
        (50..=100).contains(&fixture_paths.len()),
        "HTTP client/agent diagnostic selector should stay reviewable; selected {} fixtures",
        fixture_paths.len()
    );
    fixture_paths
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked diagnostic-only HTTP client/agent core inventory; leak/GC lifecycle paths are excluded by the kill rule and remain diagnostics"]
fn node22_supported_lane_http_client_agent_diagnostic_watchpoint() {
    let fixture_paths = http_client_agent_diagnostic_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-http-client-agent-diagnostic-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked diagnostic-only HTTP client/agent core inventory; leak/GC lifecycle paths are excluded by the kill rule and remain diagnostics"]
fn node24_default_lane_http_client_agent_diagnostic_watchpoint() {
    let fixture_paths = http_client_agent_diagnostic_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-http-client-agent-diagnostic-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

fn http_server_request_response_diagnostic_path(path: &str) -> bool {
    path.starts_with("test/parallel/test-http-server")
        || path.starts_with("test/parallel/test-http-request")
        || path.starts_with("test/parallel/test-http-res-")
        || path.starts_with("test/parallel/test-http-incoming")
        || path.starts_with("test/parallel/test-http-outgoing")
        || path == "test/parallel/test-http-allow-content-length-304.js"
        || path == "test/parallel/test-http-allow-req-after-204-res.js"
        || path == "test/parallel/test-http-extra-response.js"
        || path == "test/parallel/test-http-flush-response-headers.js"
        || path == "test/parallel/test-http-full-response.js"
        || path == "test/parallel/test-http-information-headers.js"
        || path == "test/parallel/test-http-information-processing.js"
}

const HTTP_SERVER_REQUEST_RESPONSE_DIAGNOSTIC_LOW_ROI_PATHS: &[&str] = &[
    "test/parallel/test-http-server-connections-checking-leak.js",
    "test/parallel/test-http-server-drop-connections-in-cluster.js",
    "test/parallel/test-http-server-keepalive-req-gc.js",
];

fn http_server_request_response_diagnostic_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths = node_compat_posture_paths_for_selector(lane, |entry| {
        entry["support_denominator"] == "diagnostic_only_non_isolate"
            && entry["owner"] == "networking/http"
            && entry["test_path"]
                .as_str()
                .is_some_and(http_server_request_response_diagnostic_path)
    });
    fixture_paths.retain(|path| {
        !HTTP_SERVER_REQUEST_RESPONSE_DIAGNOSTIC_LOW_ROI_PATHS
            .iter()
            .any(|low_roi_path| path == low_roi_path)
    });
    assert!(
        (50..=150).contains(&fixture_paths.len()),
        "HTTP server/request/response diagnostic selector should stay reviewable; selected {} fixtures",
        fixture_paths.len()
    );
    fixture_paths
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked diagnostic-only HTTP server/request/response inventory; cluster, leak, and GC lifecycle paths are excluded by the kill rule"]
fn node22_supported_lane_http_server_request_response_diagnostic_watchpoint() {
    let fixture_paths = http_server_request_response_diagnostic_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-http-server-request-response-diagnostic-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked diagnostic-only HTTP server/request/response inventory; cluster, leak, and GC lifecycle paths are excluded by the kill rule"]
fn node24_default_lane_http_server_request_response_diagnostic_watchpoint() {
    let fixture_paths = http_server_request_response_diagnostic_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-http-server-request-response-diagnostic-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

const HTTP_SERVER_REQUEST_RESPONSE_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-http-allow-content-length-304.js",
    "test/parallel/test-http-allow-req-after-204-res.js",
    "test/parallel/test-http-extra-response.js",
    "test/parallel/test-http-flush-response-headers.js",
    "test/parallel/test-http-incoming-matchKnownFields.js",
    "test/parallel/test-http-incoming-message-connection-setter.js",
    "test/parallel/test-http-incoming-message-destroy.js",
    "test/parallel/test-http-incoming-message-options.js",
    "test/parallel/test-http-incoming-pipelined-socket-destroy.js",
    "test/parallel/test-http-information-headers.js",
    "test/parallel/test-http-information-processing.js",
    "test/parallel/test-http-outgoing-buffer.js",
    "test/parallel/test-http-outgoing-destroy.js",
    "test/parallel/test-http-outgoing-end-cork.js",
    "test/parallel/test-http-outgoing-end-types.js",
    "test/parallel/test-http-outgoing-finish-writable.js",
    "test/parallel/test-http-outgoing-finish.js",
    "test/parallel/test-http-outgoing-finished.js",
    "test/parallel/test-http-outgoing-first-chunk-singlebyte-encoding.js",
    "test/parallel/test-http-outgoing-message-capture-rejection.js",
    "test/parallel/test-http-outgoing-message-inheritance.js",
    "test/parallel/test-http-outgoing-message-write-callback.js",
    "test/parallel/test-http-outgoing-properties.js",
    "test/parallel/test-http-outgoing-proto.js",
    "test/parallel/test-http-outgoing-renderHeaders.js",
    "test/parallel/test-http-outgoing-settimeout.js",
    "test/parallel/test-http-outgoing-writableFinished.js",
    "test/parallel/test-http-outgoing-write-types.js",
    "test/parallel/test-http-request-arguments.js",
    "test/parallel/test-http-request-dont-override-options.js",
    "test/parallel/test-http-request-end-twice.js",
    "test/parallel/test-http-request-end.js",
    "test/parallel/test-http-request-host-header.js",
    "test/parallel/test-http-request-invalid-method-error.js",
    "test/parallel/test-http-request-join-authorization-headers.js",
    "test/parallel/test-http-request-large-payload.js",
    "test/parallel/test-http-request-method-delete-payload.js",
    "test/parallel/test-http-request-methods.js",
    "test/parallel/test-http-request-smuggling-content-length.js",
    "test/parallel/test-http-res-write-after-end.js",
    "test/parallel/test-http-res-write-end-dont-take-array.js",
    "test/parallel/test-http-server-async-dispose.js",
    "test/parallel/test-http-server-clear-timer.js",
    "test/parallel/test-http-server-close-all.js",
    "test/parallel/test-http-server-close-destroy-timeout.js",
    "test/parallel/test-http-server-close-idle-wait-response.js",
    "test/parallel/test-http-server-close-idle.js",
    "test/parallel/test-http-server-connection-list-when-close.js",
    "test/parallel/test-http-server-consumed-timeout.js",
    "test/parallel/test-http-server-de-chunked-trailer.js",
    "test/parallel/test-http-server-delete-parser.js",
    "test/parallel/test-http-server-headers-timeout-delayed-headers.js",
    "test/parallel/test-http-server-headers-timeout-interrupted-headers.js",
    "test/parallel/test-http-server-headers-timeout-pipelining.js",
    "test/parallel/test-http-server-incomingmessage-destroy.js",
    "test/parallel/test-http-server-keep-alive-defaults.js",
    "test/parallel/test-http-server-keep-alive-max-requests-null.js",
    "test/parallel/test-http-server-keep-alive-timeout.js",
    "test/parallel/test-http-server-method.query.js",
    "test/parallel/test-http-server-multiheaders.js",
    "test/parallel/test-http-server-multiheaders2.js",
    "test/parallel/test-http-server-multiple-client-error.js",
    "test/parallel/test-http-server-options-highwatermark.js",
    "test/parallel/test-http-server-reject-chunked-with-content-length.js",
    "test/parallel/test-http-server-reject-cr-no-lf.js",
    "test/parallel/test-http-server-request-timeout-delayed-body.js",
    "test/parallel/test-http-server-request-timeout-delayed-headers.js",
    "test/parallel/test-http-server-request-timeout-interrupted-body.js",
    "test/parallel/test-http-server-request-timeout-interrupted-headers.js",
    "test/parallel/test-http-server-request-timeout-pipelining.js",
    "test/parallel/test-http-server-request-timeout-upgrade.js",
    "test/parallel/test-http-server-response-standalone.js",
    "test/parallel/test-http-server-timeouts-validation.js",
    "test/parallel/test-http-server-unconsume-consume.js",
    "test/parallel/test-http-server-write-after-end.js",
    "test/parallel/test-http-server-write-end-after-end.js",
    "test/parallel/test-http-server.js",
];

#[test]
fn node22_supported_lane_executes_http_server_request_response_promoted_batch_fixture() {
    let fixture_paths = HTTP_SERVER_REQUEST_RESPONSE_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-http-server-request-response-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_http_server_request_response_promoted_batch_fixture() {
    let fixture_paths = HTTP_SERVER_REQUEST_RESPONSE_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-http-server-request-response-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

fn http_remaining_diagnostic_candidate_path(path: &str) -> bool {
    path == "test/abort/test-http-parser-consume.js"
        || path.starts_with("test/parallel/test-http-")
}

const HTTP_REMAINING_DIAGNOSTIC_EXCLUDED_PATHS: &[&str] = &[
    "test/parallel/test-http-agent-domain-reused-gc.js",
    "test/parallel/test-http-agent-reuse-drained-socket-only.js",
    "test/parallel/test-http-client-leaky-with-double-response.js",
    "test/parallel/test-http-client-null-prototype-options.js",
    "test/parallel/test-http-client-request-listeners-leak.js",
    "test/parallel/test-http-full-response.js",
    "test/parallel/test-http-outgoing-destroyed.js",
    "test/parallel/test-http-outgoing-drain-writable-length.js",
    "test/parallel/test-http-outgoing-end-multiple.js",
    "test/parallel/test-http-request-agent.js",
    "test/parallel/test-http-request-signal.js",
    "test/parallel/test-http-server-capture-rejections.js",
    "test/parallel/test-http-server-client-error.js",
    "test/parallel/test-http-server-connections-checking-leak.js",
    "test/parallel/test-http-server-destroy-socket-on-client-error.js",
    "test/parallel/test-http-server-drop-connections-in-cluster.js",
    "test/parallel/test-http-server-headers-timeout-keepalive.js",
    "test/parallel/test-http-server-keepalive-end.js",
    "test/parallel/test-http-server-keepalive-req-gc.js",
    "test/parallel/test-http-server-non-utf8-header.js",
    "test/parallel/test-http-server-request-timeout-keepalive.js",
    "test/parallel/test-http-server-stale-close.js",
    "test/parallel/test-http-server-unconsume.js",
];

fn http_remaining_diagnostic_excluded_path(path: &str) -> bool {
    path.contains("client-proxy/")
        || path.contains("internet/")
        || path.contains("https")
        || path.contains("proxy")
        || path.contains("unix-socket")
        || path.contains("test-http-client")
        || path.contains("test-http-agent")
        || path.contains("cluster")
        || path.contains("leak")
        || path.contains("gc")
        || HTTP_REMAINING_DIAGNOSTIC_EXCLUDED_PATHS.contains(&path)
}

fn http_remaining_diagnostic_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths = node_compat_posture_paths_for_selector(lane, |entry| {
        entry["support_denominator"] == "diagnostic_only_non_isolate"
            && entry["owner"] == "networking/http"
            && entry["test_path"]
                .as_str()
                .is_some_and(http_remaining_diagnostic_candidate_path)
    });
    fixture_paths.retain(|path| !http_remaining_diagnostic_excluded_path(path));
    assert!(
        (50..=200).contains(&fixture_paths.len()),
        "remaining HTTP diagnostic selector should stay broad but reviewable; selected {} fixtures",
        fixture_paths.len()
    );
    fixture_paths
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked remaining HTTP diagnostic core inventory; prior client/agent/server residuals and proxy/internet/TLS/Unix/cluster/leak/GC paths are excluded"]
fn node22_supported_lane_http_remaining_diagnostic_watchpoint() {
    let fixture_paths = http_remaining_diagnostic_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-http-remaining-diagnostic-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked remaining HTTP diagnostic core inventory; prior client/agent/server residuals and proxy/internet/TLS/Unix/cluster/leak/GC paths are excluded"]
fn node24_default_lane_http_remaining_diagnostic_watchpoint() {
    let fixture_paths = http_remaining_diagnostic_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-http-remaining-diagnostic-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

const HTTP_REMAINING_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-http-1.0-keep-alive.js",
    "test/parallel/test-http-1.0.js",
    "test/parallel/test-http-abort-before-end.js",
    "test/parallel/test-http-abort-client.js",
    "test/parallel/test-http-abort-queued.js",
    "test/parallel/test-http-abort-stream-end.js",
    "test/parallel/test-http-aborted.js",
    "test/parallel/test-http-addrequest-localaddress.js",
    "test/parallel/test-http-after-connect.js",
    "test/parallel/test-http-bind-twice.js",
    "test/parallel/test-http-blank-header.js",
    "test/parallel/test-http-buffer-sanity.js",
    "test/parallel/test-http-byteswritten.js",
    "test/parallel/test-http-catch-uncaughtexception.js",
    "test/parallel/test-http-chunked-304.js",
    "test/parallel/test-http-chunked-smuggling.js",
    "test/parallel/test-http-chunked.js",
    "test/parallel/test-http-common.js",
    "test/parallel/test-http-conn-reset.js",
    "test/parallel/test-http-connect-req-res.js",
    "test/parallel/test-http-connect.js",
    "test/parallel/test-http-content-length-mismatch.js",
    "test/parallel/test-http-content-length.js",
    "test/parallel/test-http-correct-hostname.js",
    "test/parallel/test-http-createConnection.js",
    "test/parallel/test-http-date-header.js",
    "test/parallel/test-http-decoded-auth.js",
    "test/parallel/test-http-default-encoding.js",
    "test/parallel/test-http-destroyed-socket-write2.js",
    "test/parallel/test-http-dns-error.js",
    "test/parallel/test-http-dont-set-default-headers-with-set-header.js",
    "test/parallel/test-http-dont-set-default-headers-with-setHost.js",
    "test/parallel/test-http-dont-set-default-headers.js",
    "test/parallel/test-http-dummy-characters-smuggling.js",
    "test/parallel/test-http-early-hints.js",
    "test/parallel/test-http-end-throw-socket-handling.js",
    "test/parallel/test-http-eof-on-connect.js",
    "test/parallel/test-http-expect-continue-reuse-race.js",
    "test/parallel/test-http-expect-continue.js",
    "test/parallel/test-http-expect-handling.js",
    "test/parallel/test-http-flush-headers.js",
    "test/parallel/test-http-header-badrequest.js",
    "test/parallel/test-http-header-obstext.js",
    "test/parallel/test-http-header-owstext.js",
    "test/parallel/test-http-header-read.js",
    "test/parallel/test-http-header-validators.js",
    "test/parallel/test-http-headers-distinct-proto.js",
    "test/parallel/test-http-hex-write.js",
    "test/parallel/test-http-highwatermark.js",
    "test/parallel/test-http-host-header-ipv6-fail.js",
    "test/parallel/test-http-host-headers.js",
    "test/parallel/test-http-hostname-typechecking.js",
    "test/parallel/test-http-insecure-parser-per-stream.js",
    "test/parallel/test-http-invalid-path-chars.js",
    "test/parallel/test-http-invalid-te.js",
    "test/parallel/test-http-invalid-urls.js",
    "test/parallel/test-http-invalidheaderfield.js",
    "test/parallel/test-http-invalidheaderfield2.js",
    "test/parallel/test-http-keep-alive-close-on-header.js",
    "test/parallel/test-http-keep-alive-drop-requests.js",
    "test/parallel/test-http-keep-alive-empty-line.mjs",
    "test/parallel/test-http-keep-alive-max-requests.js",
    "test/parallel/test-http-keep-alive-pipeline-max-requests.js",
    "test/parallel/test-http-keep-alive-timeout-buffer.js",
    "test/parallel/test-http-keep-alive-timeout-custom.js",
    "test/parallel/test-http-keep-alive-timeout-race-condition.js",
    "test/parallel/test-http-keep-alive-timeout.js",
    "test/parallel/test-http-keep-alive.js",
    "test/parallel/test-http-keepalive-client.js",
    "test/parallel/test-http-keepalive-free.js",
    "test/parallel/test-http-keepalive-override.js",
    "test/parallel/test-http-keepalive-request.js",
    "test/parallel/test-http-listening.js",
    "test/parallel/test-http-malformed-request.js",
    "test/parallel/test-http-many-ended-pipelines.js",
    "test/parallel/test-http-max-header-size-per-stream.js",
    "test/parallel/test-http-max-headers-count.js",
    "test/parallel/test-http-max-http-headers.js",
    "test/parallel/test-http-max-sockets.js",
    "test/parallel/test-http-methods.js",
    "test/parallel/test-http-missing-header-separator-cr.js",
    "test/parallel/test-http-missing-header-separator-lf.js",
    "test/parallel/test-http-multi-line-headers.js",
    "test/parallel/test-http-mutable-headers.js",
    "test/parallel/test-http-no-content-length.js",
    "test/parallel/test-http-nodelay.js",
    "test/parallel/test-http-parser-bad-ref.js",
    "test/parallel/test-http-parser-free.js",
    "test/parallel/test-http-parser-freed-before-upgrade.js",
    "test/parallel/test-http-parser-multiple-execute.js",
    "test/parallel/test-http-parser-timeout-reset.js",
    "test/parallel/test-http-parser.js",
    "test/parallel/test-http-pause-no-dump.js",
    "test/parallel/test-http-pause-resume-one-end.js",
    "test/parallel/test-http-pause.js",
    "test/parallel/test-http-pipe-fs.js",
    "test/parallel/test-http-pipeline-assertionerror-finish.js",
    "test/parallel/test-http-pipeline-socket-parser-typeerror.js",
    "test/parallel/test-http-raw-headers.js",
    "test/parallel/test-http-rawheaders-limit.js",
    "test/parallel/test-http-readable-data-event.js",
    "test/parallel/test-http-remove-connection-header-persists-connection.js",
    "test/parallel/test-http-remove-header-stays-removed.js",
    "test/parallel/test-http-req-close-robust-from-tampering.js",
    "test/parallel/test-http-req-res-close.js",
    "test/parallel/test-http-same-map.js",
    "test/parallel/test-http-set-cookies.js",
    "test/parallel/test-http-set-header-chain.js",
    "test/parallel/test-http-set-max-idle-http-parser.js",
    "test/parallel/test-http-set-timeout-server.js",
    "test/parallel/test-http-set-trailers.js",
    "test/parallel/test-http-should-keep-alive.js",
    "test/parallel/test-http-socket-encoding-error.js",
    "test/parallel/test-http-sync-write-error-during-continue.js",
    "test/parallel/test-http-timeout-client-warning.js",
    "test/parallel/test-http-timeout-overflow.js",
    "test/parallel/test-http-timeout.js",
    "test/parallel/test-http-transfer-encoding-repeated-chunked.js",
    "test/parallel/test-http-transfer-encoding-smuggling.js",
    "test/parallel/test-http-uncaught-from-request-callback.js",
    "test/parallel/test-http-upgrade-advertise.js",
    "test/parallel/test-http-upgrade-agent.js",
    "test/parallel/test-http-upgrade-binary.js",
    "test/parallel/test-http-upgrade-client.js",
    "test/parallel/test-http-upgrade-client2.js",
    "test/parallel/test-http-upgrade-reconsume-stream.js",
    "test/parallel/test-http-upgrade-server.js",
    "test/parallel/test-http-url.parse-auth-with-header-in-request.js",
    "test/parallel/test-http-url.parse-auth.js",
    "test/parallel/test-http-url.parse-basic.js",
    "test/parallel/test-http-url.parse-path.js",
    "test/parallel/test-http-url.parse-post.js",
    "test/parallel/test-http-url.parse-search.js",
    "test/parallel/test-http-wget.js",
    "test/parallel/test-http-writable-true-after-close.js",
    "test/parallel/test-http-write-callbacks.js",
    "test/parallel/test-http-write-empty-string.js",
    "test/parallel/test-http-zero-length-write.js",
    "test/parallel/test-http-zerolengthbuffer.js",
];

#[test]
fn node22_supported_lane_executes_http_remaining_promoted_batch_fixture() {
    let fixture_paths = HTTP_REMAINING_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-http-remaining-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_http_remaining_promoted_batch_fixture() {
    let fixture_paths = HTTP_REMAINING_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-http-remaining-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

fn http2_diagnostic_core_candidate_path(path: &str) -> bool {
    path.starts_with("test/parallel/test-http2-")
}

fn http2_diagnostic_core_excluded_path(path: &str) -> bool {
    path.contains("internet/")
        || path.contains("https")
        || path.contains("tls")
        || path.contains("secure")
        || path.contains("alpn")
        || path.contains("fallback")
        || path.contains("allow-http1")
        || path.contains("http1")
        || path.contains("respond-file")
        || path.contains("respond-with-file")
        || path.contains("serve-file")
        || path.contains("sendfile")
        || path.contains("filehandle")
        || path.contains("respond-with-fd")
        || path.contains("fd-")
        || path.contains("fd.js")
        || path.contains("large")
        || path.contains("leak")
        || path.contains("gc")
        || path.contains("heapdump")
        || path.contains("flood")
        || path.contains("info-headers")
        || path.contains("pack-end-stream-flag")
        || path.contains("pipe-named-pipe")
        || path.contains("debug")
        || path.contains("proxy")
        || path.contains("port-80")
        || path.contains("ip-address-host")
        || path.contains("autoselect")
        || path.contains("worker")
        || path.contains("benchmark")
        || path.contains("sequential")
        || path.contains("pummel")
        || path.contains("wpt")
}

fn http2_diagnostic_core_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths = node_compat_posture_paths_for_selector(lane, |entry| {
        entry["support_denominator"] == "diagnostic_only_non_isolate"
            && entry["owner"] == "networking/http2"
            && entry["test_path"]
                .as_str()
                .is_some_and(http2_diagnostic_core_candidate_path)
    });
    fixture_paths.retain(|path| !http2_diagnostic_core_excluded_path(path));
    assert!(
        (50..=200).contains(&fixture_paths.len()),
        "HTTP/2 diagnostic-core selector should stay broad but reviewable; selected {} fixtures",
        fixture_paths.len()
    );
    fixture_paths
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked HTTP/2 diagnostic core inventory; internet/TLS/secure/file-serving/leak/stress/host-topology paths are excluded"]
fn node22_supported_lane_http2_diagnostic_core_watchpoint() {
    let fixture_paths = http2_diagnostic_core_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-http2-diagnostic-core-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked HTTP/2 diagnostic core inventory; internet/TLS/secure/file-serving/leak/stress/host-topology paths are excluded"]
fn node24_default_lane_http2_diagnostic_core_watchpoint() {
    let fixture_paths = http2_diagnostic_core_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-http2-diagnostic-core-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

const HTTP2_DIAGNOSTIC_CORE_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-http2-altsvc.js",
    "test/parallel/test-http2-async-local-storage.js",
    "test/parallel/test-http2-backpressure.js",
    "test/parallel/test-http2-binding.js",
    "test/parallel/test-http2-buffersize.js",
    "test/parallel/test-http2-byteswritten-server.js",
    "test/parallel/test-http2-clean-output.js",
    "test/parallel/test-http2-client-data-end.js",
    "test/parallel/test-http2-client-destroy.js",
    "test/parallel/test-http2-client-onconnect-errors.js",
    "test/parallel/test-http2-client-priority-before-connect.js",
    "test/parallel/test-http2-client-promisify-connect-error.js",
    "test/parallel/test-http2-client-promisify-connect.js",
    "test/parallel/test-http2-client-request-listeners-warning.js",
    "test/parallel/test-http2-client-request-options-errors.js",
    "test/parallel/test-http2-client-rststream-before-connect.js",
    "test/parallel/test-http2-client-setLocalWindowSize.js",
    "test/parallel/test-http2-client-setNextStreamID-errors.js",
    "test/parallel/test-http2-client-settings-before-connect.js",
    "test/parallel/test-http2-client-shutdown-before-connect.js",
    "test/parallel/test-http2-client-socket-destroy.js",
    "test/parallel/test-http2-client-stream-destroy-before-connect.js",
    "test/parallel/test-http2-client-unescaped-path.js",
    "test/parallel/test-http2-client-write-before-connect.js",
    "test/parallel/test-http2-client-write-empty-string.js",
    "test/parallel/test-http2-compat-write-head-after-close.js",
    "test/parallel/test-http2-connect-method-extended-cant-turn-off.js",
    "test/parallel/test-http2-connect-method-extended.js",
    "test/parallel/test-http2-connect-method.js",
    "test/parallel/test-http2-cookies.js",
    "test/parallel/test-http2-create-client-session.js",
    "test/parallel/test-http2-createserver-options.js",
    "test/parallel/test-http2-createwritereq.js",
    "test/parallel/test-http2-date-header.js",
    "test/parallel/test-http2-destroy-after-write.js",
    "test/parallel/test-http2-dont-lose-data.js",
    "test/parallel/test-http2-dont-override.js",
    "test/parallel/test-http2-endafterheaders.js",
    "test/parallel/test-http2-error-order.js",
    "test/parallel/test-http2-exceeds-server-trailer-size.js",
    "test/parallel/test-http2-forget-closed-streams.js",
    "test/parallel/test-http2-generic-streams.js",
    "test/parallel/test-http2-goaway-delayed-request.js",
    "test/parallel/test-http2-goaway-opaquedata.js",
    "test/parallel/test-http2-head-request.js",
    "test/parallel/test-http2-invalid-last-stream-id.js",
    "test/parallel/test-http2-invalidargtypes-errors.js",
    "test/parallel/test-http2-invalidheaderfield.js",
    "test/parallel/test-http2-invalidheaderfields-client.js",
    "test/parallel/test-http2-malformed-altsvc.js",
    "test/parallel/test-http2-many-writes-and-destroy.js",
    "test/parallel/test-http2-max-concurrent-streams.js",
    "test/parallel/test-http2-max-invalid-frames.js",
    "test/parallel/test-http2-max-settings.js",
    "test/parallel/test-http2-methods.js",
    "test/parallel/test-http2-misbehaving-flow-control-paused.js",
    "test/parallel/test-http2-misbehaving-flow-control.js",
    "test/parallel/test-http2-misbehaving-multiplex.js",
    "test/parallel/test-http2-misused-pseudoheaders.js",
    "test/parallel/test-http2-multiplex.js",
    "test/parallel/test-http2-no-more-streams.js",
    "test/parallel/test-http2-no-wanttrailers-listener.js",
    "test/parallel/test-http2-onping.js",
    "test/parallel/test-http2-options-max-headers-block-length.js",
    "test/parallel/test-http2-options-max-headers-exceeds-nghttp2.js",
    "test/parallel/test-http2-options-max-reserved-streams.js",
    "test/parallel/test-http2-padding-aligned.js",
    "test/parallel/test-http2-perform-server-handshake.js",
    "test/parallel/test-http2-ping-unsolicited-ack.js",
    "test/parallel/test-http2-ping.js",
    "test/parallel/test-http2-premature-close.js",
    "test/parallel/test-http2-priority-cycle-.js",
    "test/parallel/test-http2-propagate-session-destroy-code.js",
    "test/parallel/test-http2-raw-headers-defaults.js",
    "test/parallel/test-http2-raw-headers.js",
    "test/parallel/test-http2-removed-header-stays-removed.js",
    "test/parallel/test-http2-request-remove-connect-listener.js",
    "test/parallel/test-http2-request-response-proto.js",
    "test/parallel/test-http2-res-corked.js",
    "test/parallel/test-http2-respond-errors.js",
    "test/parallel/test-http2-respond-nghttperrors.js",
    "test/parallel/test-http2-respond-no-data.js",
    "test/parallel/test-http2-sensitive-headers.js",
    "test/parallel/test-http2-sent-headers.js",
    "test/parallel/test-http2-server-async-dispose.js",
    "test/parallel/test-http2-server-close-callback.js",
    "test/parallel/test-http2-server-errors.js",
    "test/parallel/test-http2-server-push-disabled.js",
    "test/parallel/test-http2-server-push-stream-errors-args.js",
    "test/parallel/test-http2-server-push-stream-errors.js",
    "test/parallel/test-http2-server-push-stream-head.js",
    "test/parallel/test-http2-server-push-stream.js",
    "test/parallel/test-http2-server-rfc-9113-client.js",
    "test/parallel/test-http2-server-rfc-9113-server.js",
    "test/parallel/test-http2-server-rst-before-respond.js",
    "test/parallel/test-http2-server-rst-stream.js",
    "test/parallel/test-http2-server-session-destroy.js",
    "test/parallel/test-http2-server-sessionerror.js",
    "test/parallel/test-http2-server-set-header.js",
    "test/parallel/test-http2-server-setLocalWindowSize.js",
    "test/parallel/test-http2-server-settimeout-no-callback.js",
    "test/parallel/test-http2-server-shutdown-before-respond.js",
    "test/parallel/test-http2-server-shutdown-options-errors.js",
    "test/parallel/test-http2-server-shutdown-redundant.js",
    "test/parallel/test-http2-server-socket-destroy.js",
    "test/parallel/test-http2-server-stream-session-destroy.js",
    "test/parallel/test-http2-server-timeout.js",
    "test/parallel/test-http2-session-graceful-close.js",
    "test/parallel/test-http2-session-settings.js",
    "test/parallel/test-http2-session-stream-state.js",
    "test/parallel/test-http2-session-timeout.js",
    "test/parallel/test-http2-settings-unsolicited-ack.js",
    "test/parallel/test-http2-short-stream-client-server.js",
    "test/parallel/test-http2-stream-client.js",
    "test/parallel/test-http2-stream-destroy-event-order.js",
    "test/parallel/test-http2-stream-removelisteners-after-close.js",
    "test/parallel/test-http2-timeouts.js",
    "test/parallel/test-http2-too-many-headers.js",
    "test/parallel/test-http2-too-many-settings.js",
    "test/parallel/test-http2-too-many-streams.js",
    "test/parallel/test-http2-trailers-after-session-close.js",
    "test/parallel/test-http2-trailers.js",
    "test/parallel/test-http2-update-settings.js",
    "test/parallel/test-http2-window-size.js",
    "test/parallel/test-http2-window-update-overflow.js",
    "test/parallel/test-http2-write-callbacks.js",
    "test/parallel/test-http2-write-empty-string.js",
    "test/parallel/test-http2-write-finishes-after-stream-destroy.js",
    "test/parallel/test-http2-zero-length-write.js",
];

#[test]
fn node22_supported_lane_executes_http2_diagnostic_core_promoted_batch_fixture() {
    let fixture_paths = HTTP2_DIAGNOSTIC_CORE_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-http2-diagnostic-core-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_http2_diagnostic_core_promoted_batch_fixture() {
    let fixture_paths = HTTP2_DIAGNOSTIC_CORE_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-http2-diagnostic-core-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

fn node26_current_http2_promoted_residual_paths() -> &'static [&'static str] {
    &[
        "test/parallel/test-http2-misbehaving-flow-control-paused.js",
        "test/parallel/test-http2-misbehaving-flow-control.js",
        "test/parallel/test-http2-options-max-headers-exceeds-nghttp2.js",
    ]
}

#[test]
fn node26_current_lane_executes_http2_diagnostic_core_promoted_batch_fixture() {
    let mut fixture_paths = HTTP2_DIAGNOSTIC_CORE_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    fixture_paths.retain(|path| {
        !node26_current_http2_promoted_residual_paths().contains(&path.as_str())
    });
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-http2-diagnostic-core-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

fn net_diagnostic_core_candidate_path(path: &str) -> bool {
    path.starts_with("test/parallel/test-net-")
}

fn net_diagnostic_core_excluded_path(path: &str) -> bool {
    path.contains("internet/")
        || path.contains("async-hooks/")
        || path.contains("autoselect")
        || path.contains("child-process")
        || path.contains("cluster")
        || path.contains("fd")
        || path.contains("pipe")
        || path.contains("path")
        || path.contains("ipv6")
        || path.contains("large")
        || path.contains("memleak")
        || path.contains("reuseport")
        || path.ends_with("test-net-connect-keepalive.js")
        || path.ends_with("test-net-server-keepalive.js")
        || path.ends_with("test-net-server-nodelay.js")
        || path.contains("perf_hooks")
        || path.contains("stdin")
        || path.contains("exclusive-random")
        || path.contains("try-ports")
        || path.contains("remote-address")
        || path.contains("local-address")
        || path.contains("eaddrinuse")
        || path.contains("dns")
        || path.contains("lookup")
        || path.contains("blocklist")
        || path.contains("listen-handle")
        || path.contains("ipc")
        || path.contains("simultaneous")
        || path.contains("deprecated")
        || path.contains("tos")
        || path.contains("drop-connections")
        || path.contains("max-connections")
}

fn net_diagnostic_core_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths = node_compat_posture_paths_for_selector(lane, |entry| {
        entry["support_denominator"] == "diagnostic_only_non_isolate"
            && entry["owner"] == "networking/net"
            && entry["test_path"]
                .as_str()
                .is_some_and(net_diagnostic_core_candidate_path)
    });
    fixture_paths.retain(|path| !net_diagnostic_core_excluded_path(path));
    assert!(
        (50..=120).contains(&fixture_paths.len()),
        "net diagnostic-core selector should stay broad but reviewable; selected {} fixtures",
        fixture_paths.len()
    );
    fixture_paths
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked net diagnostic core inventory; internet/autoselect/DNS/cluster/fd/path/pipe/multi-address/large/stress host-topology paths are excluded"]
fn node22_supported_lane_net_diagnostic_core_watchpoint() {
    let fixture_paths = net_diagnostic_core_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-net-diagnostic-core-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked net diagnostic core inventory; internet/autoselect/DNS/cluster/fd/path/pipe/multi-address/large/stress host-topology paths are excluded"]
fn node24_default_lane_net_diagnostic_core_watchpoint() {
    let fixture_paths = net_diagnostic_core_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-net-diagnostic-core-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

const NET_DIAGNOSTIC_CORE_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-net-access-byteswritten.js",
    "test/parallel/test-net-allow-half-open.js",
    "test/parallel/test-net-better-error-messages-port-hostname.js",
    "test/parallel/test-net-binary.js",
    "test/parallel/test-net-bind-twice.js",
    "test/parallel/test-net-buffersize.js",
    "test/parallel/test-net-bytes-stats.js",
    "test/parallel/test-net-client-bind-twice.js",
    "test/parallel/test-net-connect-after-destroy.js",
    "test/parallel/test-net-connect-buffer.js",
    "test/parallel/test-net-connect-buffer2.js",
    "test/parallel/test-net-connect-call-socket-connect.js",
    "test/parallel/test-net-connect-destroy.js",
    "test/parallel/test-net-connect-immediate-destroy.js",
    "test/parallel/test-net-connect-immediate-finish.js",
    "test/parallel/test-net-connect-nodelay.js",
    "test/parallel/test-net-connect-options-allowhalfopen.js",
    "test/parallel/test-net-connect-options-port.js",
    "test/parallel/test-net-connect-paused-connection.js",
    "test/parallel/test-net-connect-reset-after-destroy.js",
    "test/parallel/test-net-connect-reset-before-connected.js",
    "test/parallel/test-net-connect-reset-until-connected.js",
    "test/parallel/test-net-connect-reset.js",
    "test/parallel/test-net-end-close.js",
    "test/parallel/test-net-end-destroyed.js",
    "test/parallel/test-net-error-twice.js",
    "test/parallel/test-net-keepalive.js",
    "test/parallel/test-net-listen-close-server-callback-is-not-function.js",
    "test/parallel/test-net-listen-invalid-port.js",
    "test/parallel/test-net-listen-twice.js",
    "test/parallel/test-net-localerror.js",
    "test/parallel/test-net-normalize-args.js",
    "test/parallel/test-net-onread-static-buffer.js",
    "test/parallel/test-net-pause-resume-connecting.js",
    "test/parallel/test-net-persistent-keepalive.js",
    "test/parallel/test-net-persistent-nodelay.js",
    "test/parallel/test-net-persistent-ref-unref.js",
    "test/parallel/test-net-pingpong.js",
    "test/parallel/test-net-reconnect.js",
    "test/parallel/test-net-server-capture-rejection.js",
    "test/parallel/test-net-server-listen-remove-callback.js",
    "test/parallel/test-net-server-options.js",
    "test/parallel/test-net-server-pause-on-connect.js",
    "test/parallel/test-net-server-reset.js",
    "test/parallel/test-net-server-unref.js",
    "test/parallel/test-net-socket-byteswritten.js",
    "test/parallel/test-net-socket-connect-without-cb.js",
    "test/parallel/test-net-socket-destroy-send.js",
    "test/parallel/test-net-socket-destroy-twice.js",
    "test/parallel/test-net-socket-end-before-connect.js",
    "test/parallel/test-net-socket-end-callback.js",
    "test/parallel/test-net-socket-no-halfopen-enforcer.js",
    "test/parallel/test-net-socket-ready-without-cb.js",
    "test/parallel/test-net-socket-reset-send.js",
    "test/parallel/test-net-socket-reset-twice.js",
    "test/parallel/test-net-socket-setnodelay.js",
    "test/parallel/test-net-socket-timeout-unref.js",
    "test/parallel/test-net-socket-timeout.js",
    "test/parallel/test-net-socket-write-after-close.js",
    "test/parallel/test-net-socket-write-error.js",
    "test/parallel/test-net-stream.js",
    "test/parallel/test-net-sync-cork.js",
    "test/parallel/test-net-throttle.js",
    "test/parallel/test-net-timeout-no-handle.js",
    "test/parallel/test-net-writable.js",
    "test/parallel/test-net-write-after-close.js",
    "test/parallel/test-net-write-after-end-nt.js",
    "test/parallel/test-net-write-arguments.js",
    "test/parallel/test-net-write-cb-on-destroy-before-connect.js",
    "test/parallel/test-net-write-connect-write.js",
    "test/parallel/test-net-write-fully-async-buffer.js",
    "test/parallel/test-net-write-fully-async-hex-string.js",
    "test/parallel/test-net-write-slow.js",
];

#[test]
fn node22_supported_lane_executes_net_diagnostic_core_promoted_batch_fixture() {
    let fixture_paths = NET_DIAGNOSTIC_CORE_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-net-diagnostic-core-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_net_diagnostic_core_promoted_batch_fixture() {
    let fixture_paths = NET_DIAGNOSTIC_CORE_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-net-diagnostic-core-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

const HTTP_CLIENT_AGENT_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-http-agent-keep-alive-timeout-buffer.js",
    "test/parallel/test-http-agent.js",
    "test/parallel/test-http-client-abort-destroy.js",
    "test/parallel/test-http-client-abort-event.js",
    "test/parallel/test-http-client-abort-keep-alive-destroy-res.js",
    "test/parallel/test-http-client-abort-keep-alive-queued-tcp-socket.js",
    "test/parallel/test-http-client-abort-keep-alive-queued-unix-socket.js",
    "test/parallel/test-http-client-abort-no-agent.js",
    "test/parallel/test-http-client-abort-response-event.js",
    "test/parallel/test-http-client-abort-unix-socket.js",
    "test/parallel/test-http-client-abort.js",
    "test/parallel/test-http-client-abort2.js",
    "test/parallel/test-http-client-abort3.js",
    "test/parallel/test-http-client-aborted-event.js",
    "test/parallel/test-http-client-agent-abort-close-event.js",
    "test/parallel/test-http-client-agent-end-close-event.js",
    "test/parallel/test-http-client-agent.js",
    "test/parallel/test-http-client-check-http-token.js",
    "test/parallel/test-http-client-close-with-default-agent.js",
    "test/parallel/test-http-client-default-headers-exist.js",
    "test/parallel/test-http-client-encoding.js",
    "test/parallel/test-http-client-error-rawbytes.js",
    "test/parallel/test-http-client-finished.js",
    "test/parallel/test-http-client-headers-array.js",
    "test/parallel/test-http-client-headers-host-array.js",
    "test/parallel/test-http-client-immediate-error.js",
    "test/parallel/test-http-client-incomingmessage-destroy.js",
    "test/parallel/test-http-client-input-function.js",
    "test/parallel/test-http-client-insecure-http-parser-error.js",
    "test/parallel/test-http-client-invalid-path.js",
    "test/parallel/test-http-client-keep-alive-hint.js",
    "test/parallel/test-http-client-keep-alive-release-before-finish.js",
    "test/parallel/test-http-client-override-global-agent.js",
    "test/parallel/test-http-client-parse-error.js",
    "test/parallel/test-http-client-pipe-end.js",
    "test/parallel/test-http-client-race-2.js",
    "test/parallel/test-http-client-race.js",
    "test/parallel/test-http-client-read-in-error.js",
    "test/parallel/test-http-client-readable.js",
    "test/parallel/test-http-client-reject-chunked-with-content-length.js",
    "test/parallel/test-http-client-reject-cr-no-lf.js",
    "test/parallel/test-http-client-reject-unexpected-agent.js",
    "test/parallel/test-http-client-req-error-dont-double-fire.js",
    "test/parallel/test-http-client-res-destroyed.js",
    "test/parallel/test-http-client-response-domain.js",
    "test/parallel/test-http-client-set-timeout-after-end.js",
    "test/parallel/test-http-client-spurious-aborted.js",
    "test/parallel/test-http-client-timeout-agent.js",
    "test/parallel/test-http-client-timeout-connect-listener.js",
    "test/parallel/test-http-client-timeout-event.js",
    "test/parallel/test-http-client-timeout-on-connect.js",
    "test/parallel/test-http-client-timeout-option-listeners.js",
    "test/parallel/test-http-client-timeout-option-with-agent.js",
    "test/parallel/test-http-client-timeout-with-data.js",
    "test/parallel/test-http-client-timeout.js",
    "test/parallel/test-http-client-unescaped-path.js",
    "test/parallel/test-http-client-with-create-connection.js",
];

#[test]
fn node22_supported_lane_executes_http_client_agent_promoted_batch_fixture() {
    let fixture_paths = HTTP_CLIENT_AGENT_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-http-client-agent-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_http_client_agent_promoted_batch_fixture() {
    let fixture_paths = HTTP_CLIENT_AGENT_PROMOTED_COMMON_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-http-client-agent-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        HTTP_CLIENT_AGENT_DIAGNOSTIC_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked module-hooks required-gap inventory; classify Node24-only hooks, shared loader-hook semantics, error-shape failures, and harness topology after the first wide run"]
fn node22_supported_lane_module_hooks_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node22,
        module_hooks_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-module-hooks-required-gap-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked module-hooks required-gap inventory; classify Node24-only hooks, shared loader-hook semantics, error-shape failures, and harness topology after the first wide run"]
fn node24_default_lane_module_hooks_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node24,
        module_hooks_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-module-hooks-required-gap-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 required-surface implementation wave: exact module-loader blocker inventory; keep ignored until broad root-cause clusters are fixed or precisely classified"]
fn node22_supported_lane_module_loader_required_surface_blocker_watchpoint() {
    let fixture_paths = module_loader_required_surface_blocker_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-module-loader-required-surface-blocker-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 required-surface implementation wave: exact module-loader blocker inventory; keep ignored until broad root-cause clusters are fixed or precisely classified"]
fn node24_default_lane_module_loader_required_surface_blocker_watchpoint() {
    let fixture_paths = module_loader_required_surface_blocker_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-module-loader-required-surface-blocker-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 node26 broad pre-run: exact module-loader blocker inventory; promote only dynamically green Current-lane fixtures"]
fn node26_current_lane_module_loader_required_surface_blocker_watchpoint() {
    let fixture_paths = module_loader_required_surface_blocker_paths(NodeCompatLane::Node26);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-module-loader-required-surface-blocker-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 focused implementation wave: package/CJS/ESM loader core; run after exact broad pre-run and before focused fixes"]
fn node22_supported_lane_module_loader_package_core_watchpoint() {
    let fixture_paths = module_loader_package_core_required_surface_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-module-loader-package-core-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 focused implementation wave: package/CJS/ESM loader core; run after exact broad pre-run and before focused fixes"]
fn node24_default_lane_module_loader_package_core_watchpoint() {
    let fixture_paths = module_loader_package_core_required_surface_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-module-loader-package-core-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        ESM_MODULE_LOADER_EXTRA_RUNTIME_FILES,
        ESM_MODULE_LOADER_EXTRA_DIRS,
    );
}

const PARALLEL_JS_PLATFORM_REQUIRED_GAP_EXTRA_DIRS: &[&str] = &["test/common"];

const PARALLEL_JS_PLATFORM_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-abort-controller-any-timeout.js",
    "test/parallel/test-error-aggregateTwoErrors.js",
    "test/parallel/test-errors-aborterror.js",
    "test/parallel/test-errors-hide-stack-frames.js",
    "test/parallel/test-errors-systemerror-frozen-intrinsics.js",
    "test/parallel/test-errors-systemerror-stackTraceLimit-custom-setter.js",
    "test/parallel/test-errors-systemerror-stackTraceLimit-deleted-and-Error-sealed.js",
    "test/parallel/test-errors-systemerror-stackTraceLimit-deleted.js",
    "test/parallel/test-errors-systemerror-stackTraceLimit-has-only-a-getter.js",
    "test/parallel/test-errors-systemerror-stackTraceLimit-not-writable.js",
    "test/parallel/test-global-console-exists.js",
    "test/parallel/test-global-encoder.js",
    "test/parallel/test-global-webcrypto.js",
    "test/parallel/test-performance-function-async.js",
    "test/parallel/test-performance-global.js",
    "test/parallel/test-performance-measure-detail.js",
    "test/parallel/test-performance-measure.js",
    "test/parallel/test-performance-nodetiming.js",
    "test/parallel/test-performanceobserver-gc.js",
    "test/parallel/test-performanceobserver.js",
    "test/parallel/test-promise-handled-rejection-no-warning.js",
    "test/parallel/test-promise-unhandled-default.js",
    "test/parallel/test-promise-unhandled-issue-43655.js",
    "test/parallel/test-promise-unhandled-silent.js",
    "test/parallel/test-promise-unhandled-throw-handler.js",
    "test/parallel/test-promise-unhandled-throw.js",
    "test/parallel/test-util-emit-experimental-warning.js",
    "test/parallel/test-util-getcallsites-preparestacktrace.js",
    "test/parallel/test-util-inspect-getters-accessing-this.js",
    "test/parallel/test-util-inspect-namespace.js",
    "test/parallel/test-util-isDeepStrictEqual.js",
    "test/parallel/test-util-primordial-monkeypatching.js",
    "test/parallel/test-util-stripvtcontrolcharacters.js",
];

const PARALLEL_JS_PLATFORM_PROMOTED_NODE22_EXTRA_PATHS: &[&str] =
    &["test/parallel/test-performance-function.js"];

const PARALLEL_JS_PLATFORM_PROMOTED_NODE26_PATHS: &[&str] = &[
    "test/parallel/test-abort-controller-any-timeout.js",
    "test/parallel/test-abortcontroller.js",
    "test/parallel/test-error-aggregateTwoErrors.js",
    "test/parallel/test-error-prepare-stack-trace.js",
    "test/parallel/test-errors-aborterror.js",
    "test/parallel/test-errors-hide-stack-frames.js",
    "test/parallel/test-errors-systemerror-frozen-intrinsics.js",
    "test/parallel/test-errors-systemerror-stackTraceLimit-custom-setter.js",
    "test/parallel/test-errors-systemerror-stackTraceLimit-deleted-and-Error-sealed.js",
    "test/parallel/test-errors-systemerror-stackTraceLimit-deleted.js",
    "test/parallel/test-errors-systemerror-stackTraceLimit-has-only-a-getter.js",
    "test/parallel/test-errors-systemerror-stackTraceLimit-not-writable.js",
    "test/parallel/test-global-console-exists.js",
    "test/parallel/test-global-domexception.js",
    "test/parallel/test-global-encoder.js",
    "test/parallel/test-global-setters.js",
    "test/parallel/test-global-webcrypto.js",
    "test/parallel/test-performance-function-async.js",
    "test/parallel/test-performance-gc.js",
    "test/parallel/test-performance-global.js",
    "test/parallel/test-performance-measure-detail.js",
    "test/parallel/test-performance-measure.js",
    "test/parallel/test-performance-nodetiming.js",
    "test/parallel/test-performance-resourcetimingbufferfull.js",
    "test/parallel/test-performance-timeline.mjs",
    "test/parallel/test-performanceobserver-gc.js",
    "test/parallel/test-performanceobserver.js",
    "test/parallel/test-promise-handled-rejection-no-warning.js",
    "test/parallel/test-promise-hook-create-hook.js",
    "test/parallel/test-promise-hook-exceptions.js",
    "test/parallel/test-promise-hook-on-after.js",
    "test/parallel/test-promise-hook-on-before.js",
    "test/parallel/test-promise-hook-on-init.js",
    "test/parallel/test-promise-hook-on-resolve.js",
    "test/parallel/test-promise-race-memory-leak.js",
    "test/parallel/test-promise-unhandled-default.js",
    "test/parallel/test-promise-unhandled-error-with-reading-file.js",
    "test/parallel/test-promise-unhandled-error.js",
    "test/parallel/test-promise-unhandled-issue-43655.js",
    "test/parallel/test-promise-unhandled-silent-no-hook.js",
    "test/parallel/test-promise-unhandled-silent.js",
    "test/parallel/test-promise-unhandled-throw-handler.js",
    "test/parallel/test-promise-unhandled-throw.js",
    "test/parallel/test-promise-unhandled-warn-no-hook.js",
    "test/parallel/test-promise-unhandled-warn.js",
    "test/parallel/test-promises-unhandled-proxy-rejections.js",
    "test/parallel/test-promises-unhandled-rejections.js",
    "test/parallel/test-promises-unhandled-symbol-rejections.js",
    "test/parallel/test-promises-warning-on-unhandled-rejection.js",
];

#[test]
fn node22_supported_lane_executes_parallel_js_platform_promoted_batch_fixture() {
    let mut fixture_paths: Vec<String> = PARALLEL_JS_PLATFORM_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    fixture_paths.extend(
        PARALLEL_JS_PLATFORM_PROMOTED_NODE22_EXTRA_PATHS
            .iter()
            .map(|path| (*path).to_string()),
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-parallel-js-platform-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        PARALLEL_JS_PLATFORM_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_parallel_js_platform_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = PARALLEL_JS_PLATFORM_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-parallel-js-platform-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        PARALLEL_JS_PLATFORM_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_parallel_js_platform_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = PARALLEL_JS_PLATFORM_PROMOTED_NODE26_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-parallel-js-platform-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        PARALLEL_JS_PLATFORM_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked parallel JS platform required-gap inventory; classify util, global, errors, promises, performance, abort, and EventTarget failures before focused fixes"]
fn node22_supported_lane_parallel_js_platform_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node22,
        parallel_js_platform_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-parallel-js-platform-required-gap-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        PARALLEL_JS_PLATFORM_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked parallel JS platform required-gap inventory; classify util, global, errors, promises, performance, abort, and EventTarget failures before focused fixes"]
fn node24_default_lane_parallel_js_platform_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node24,
        parallel_js_platform_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-parallel-js-platform-required-gap-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        PARALLEL_JS_PLATFORM_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 node26 broad pre-run: ROI-ranked parallel JS platform required-gap inventory; promote only dynamically green fixtures"]
fn node26_current_lane_parallel_js_platform_required_gap_watchpoint() {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        NodeCompatLane::Node26,
        parallel_js_platform_required_gap_path,
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-parallel-js-platform-required-gap-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        PARALLEL_JS_PLATFORM_REQUIRED_GAP_EXTRA_DIRS,
    );
}

const UNPROMOTED_PARALLEL_DISCOVERY_EXTRA_DIRS: &[&str] = &["test/common", "test/fixtures"];

const UNPROMOTED_PARALLEL_DISCOVERY_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-async-local-storage-http-agent.js",
    "test/parallel/test-async-local-storage-http-multiclients.js",
    "test/parallel/test-async-wrap-constructor.js",
    "test/parallel/test-handle-wrap-close-abort.js",
    "test/parallel/test-icu-stringwidth.js",
    "test/parallel/test-internal-modules.js",
    "test/parallel/test-internal-util-normalizeencoding.js",
    "test/parallel/test-internal-util-weakreference.js",
    "test/parallel/test-next-tick-domain.js",
    "test/parallel/test-nodeeventtarget.js",
    "test/parallel/test-queue-microtask-uncaught-asynchooks.js",
    "test/parallel/test-stringbytes-external.js",
];

const UNPROMOTED_PARALLEL_DISCOVERY_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-async-local-storage-enter-with.js",
    "test/parallel/test-async-local-storage-weak-asyncwrap-leak.js",
];

const UNPROMOTED_PARALLEL_DISCOVERY_PROMOTED_NODE26_PATHS: &[&str] = &[
    "test/parallel/test-async-local-storage-enter-with.js",
    "test/parallel/test-async-local-storage-http-agent.js",
    "test/parallel/test-async-local-storage-http-multiclients.js",
    "test/parallel/test-async-local-storage-http-parser-leak.js",
    "test/parallel/test-async-local-storage-isolation.js",
    "test/parallel/test-async-local-storage-run-scope.js",
    "test/parallel/test-async-wrap-constructor.js",
    "test/parallel/test-async-wrap-promise-after-enabled.js",
    "test/parallel/test-async-wrap-trigger-id.js",
    "test/parallel/test-async-wrap-uncaughtexception.js",
    "test/parallel/test-asyncresource-bind.js",
    "test/parallel/test-beforeexit-event-exit.js",
    "test/parallel/test-binding-constants.js",
    "test/parallel/test-constants.js",
    "test/parallel/test-handle-wrap-close-abort.js",
    "test/parallel/test-icu-stringwidth.js",
    "test/parallel/test-internal-modules.js",
    "test/parallel/test-internal-process-binding.js",
    "test/parallel/test-internal-util-normalizeencoding.js",
    "test/parallel/test-internal-util-weakreference.js",
    "test/parallel/test-messageevent-brandcheck.js",
    "test/parallel/test-next-tick-domain.js",
    "test/parallel/test-nodeeventtarget.js",
    "test/parallel/test-queue-microtask-uncaught-asynchooks.js",
    "test/parallel/test-require-process.js",
    "test/parallel/test-require-resolve-invalid-paths.js",
    "test/parallel/test-require-resolve-opts-paths-relative.js",
    "test/parallel/test-source-map-invalid-url.js",
    "test/parallel/test-stringbytes-external.js",
    "test/parallel/test-tojson-perf_hooks.js",
    "test/parallel/test-warn-tls-common-deprecation.js",
    "test/parallel/test-warn-tls-wrap-deprecation.js",
];

#[test]
fn node22_supported_lane_executes_unpromoted_parallel_discovery_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = UNPROMOTED_PARALLEL_DISCOVERY_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-unpromoted-parallel-discovery-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        UNPROMOTED_PARALLEL_DISCOVERY_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_unpromoted_parallel_discovery_promoted_batch_fixture() {
    let mut fixture_paths: Vec<String> = UNPROMOTED_PARALLEL_DISCOVERY_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    fixture_paths.extend(
        UNPROMOTED_PARALLEL_DISCOVERY_PROMOTED_NODE24_ONLY_PATHS
            .iter()
            .map(|path| (*path).to_string()),
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-unpromoted-parallel-discovery-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        UNPROMOTED_PARALLEL_DISCOVERY_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_unpromoted_parallel_discovery_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = UNPROMOTED_PARALLEL_DISCOVERY_PROMOTED_NODE26_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-unpromoted-parallel-discovery-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        UNPROMOTED_PARALLEL_DISCOVERY_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: generated-posture discovery for remaining unpromoted test/parallel required gaps after excluding already killed host/native/CLI/stress/fatal families"]
fn node22_supported_lane_unpromoted_parallel_discovery_watchpoint() {
    let fixture_paths = unpromoted_parallel_discovery_fixture_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-unpromoted-parallel-discovery-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        UNPROMOTED_PARALLEL_DISCOVERY_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: generated-posture discovery for remaining unpromoted test/parallel required gaps after excluding already killed host/native/CLI/stress/fatal families"]
fn node24_default_lane_unpromoted_parallel_discovery_watchpoint() {
    let fixture_paths = unpromoted_parallel_discovery_fixture_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-unpromoted-parallel-discovery-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        UNPROMOTED_PARALLEL_DISCOVERY_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 node26 broad pre-run: generated-posture discovery for remaining unpromoted test/parallel required gaps after excluding already killed host/native/CLI/stress/fatal families"]
fn node26_current_lane_unpromoted_parallel_discovery_watchpoint() {
    let fixture_paths = unpromoted_parallel_discovery_fixture_paths(NodeCompatLane::Node26);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-unpromoted-parallel-discovery-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        UNPROMOTED_PARALLEL_DISCOVERY_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 post-2000 broad pre-run: internal helper/module/process/WebIDL slice from node-compat/unpromoted-surface; classify internal helper failures before focused fixes"]
fn node22_supported_lane_unpromoted_internal_helper_watchpoint() {
    let fixture_paths = unpromoted_internal_helper_required_gap_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-unpromoted-internal-helper-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        UNPROMOTED_PARALLEL_DISCOVERY_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 post-2000 broad pre-run: internal helper/module/process/WebIDL slice from node-compat/unpromoted-surface; classify internal helper failures before focused fixes"]
fn node24_default_lane_unpromoted_internal_helper_watchpoint() {
    let fixture_paths = unpromoted_internal_helper_required_gap_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-unpromoted-internal-helper-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        UNPROMOTED_PARALLEL_DISCOVERY_EXTRA_DIRS,
    );
}

const CORE_SEMANTICS_UTIL_REQUIRED_GAP_EXTRA_DIRS: &[&str] = &["test/common", "test/fixtures"];

const CORE_SEMANTICS_UTIL_REQUIRED_GAP_OWNERS: &[&str] = &[
    "core-semantics/assert",
    "core-semantics/buffer",
    "core-semantics/path",
    "core-semantics/url",
    "loader-context/util",
];

fn core_semantics_util_required_gap_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths: Vec<String> = CORE_SEMANTICS_UTIL_REQUIRED_GAP_OWNERS
        .iter()
        .flat_map(|owner| node_compat_required_gap_paths_for_owner(lane, owner))
        .collect();
    fixture_paths.sort();
    fixture_paths.dedup();
    fixture_paths
}

const CORE_SEMANTICS_UTIL_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-assert-class.js",
    "test/parallel/test-assert-esm-cjs-message-verify.js",
    "test/parallel/test-assert-myers-diff.js",
    "test/parallel/test-assert-partial-deep-equal.js",
    "test/parallel/test-buffer-constructor-outside-node-modules.js",
    "test/parallel/test-buffer-isascii.js",
    "test/parallel/test-buffer-isutf8.js",
    "test/parallel/test-buffer-pool-untransferable.js",
    "test/parallel/test-buffer-tostring-4gb.js",
    "test/parallel/test-buffer-zero-fill-cli.js",
    "test/parallel/test-buffer-zero-fill-reset.js",
    "test/parallel/test-buffer-zero-fill.js",
    "test/parallel/test-path-resolve.js",
    "test/parallel/test-url-is-url-internal.js",
];

const CORE_SEMANTICS_UTIL_PROMOTED_NODE22_EXTRA_PATHS: &[&str] = &[
    "test/parallel/test-path-makelong.js",
    "test/parallel/test-path-normalize.js",
    "test/parallel/test-util-log.js",
];

const CORE_SEMANTICS_UTIL_PROMOTED_NODE24_EXTRA_PATHS: &[&str] = &[
    "test/parallel/test-assert.js",
    "test/parallel/test-buffer-generic-methods.js",
    "test/parallel/test-url-parse-deprecation.js",
    "test/parallel/test-util.js",
    "test/parallel/test-util-styletext-hex.js",
];

const CORE_SEMANTICS_UTIL_PROMOTED_NODE26_PATHS: &[&str] = &[
    "test/parallel/test-assert-class.js",
    "test/parallel/test-assert-esm-cjs-message-verify.js",
    "test/parallel/test-assert-myers-diff.js",
    "test/parallel/test-assert-partial-deep-equal.js",
    "test/parallel/test-buffer-constructor-outside-node-modules.js",
    "test/parallel/test-buffer-generic-methods.js",
    "test/parallel/test-buffer-isascii.js",
    "test/parallel/test-buffer-isutf8.js",
    "test/parallel/test-buffer-pool-untransferable.js",
    "test/parallel/test-buffer-tostring-4gb.js",
    "test/parallel/test-buffer-zero-fill-cli.js",
    "test/parallel/test-buffer-zero-fill-reset.js",
    "test/parallel/test-buffer-zero-fill.js",
    "test/parallel/test-path-resolve.js",
    "test/parallel/test-url-is-url-internal.js",
    "test/parallel/test-util-emit-experimental-warning.js",
    "test/parallel/test-util-getcallsites-preparestacktrace.js",
    "test/parallel/test-util-inspect-getters-accessing-this.js",
    "test/parallel/test-util-inspect-namespace.js",
    "test/parallel/test-util-isDeepStrictEqual.js",
    "test/parallel/test-util-primordial-monkeypatching.js",
    "test/parallel/test-util-promisify-custom-names.mjs",
    "test/parallel/test-util-stripvtcontrolcharacters.js",
    "test/parallel/test-util-styletext-hex.js",
];

#[test]
fn node22_supported_lane_executes_core_semantics_util_promoted_batch_fixture() {
    let mut fixture_paths: Vec<String> = CORE_SEMANTICS_UTIL_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    fixture_paths.extend(
        CORE_SEMANTICS_UTIL_PROMOTED_NODE22_EXTRA_PATHS
            .iter()
            .map(|path| (*path).to_string()),
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-core-semantics-util-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        CORE_SEMANTICS_UTIL_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_core_semantics_util_promoted_batch_fixture() {
    let mut fixture_paths: Vec<String> = CORE_SEMANTICS_UTIL_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    fixture_paths.extend(
        CORE_SEMANTICS_UTIL_PROMOTED_NODE24_EXTRA_PATHS
            .iter()
            .map(|path| (*path).to_string()),
    );
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-core-semantics-util-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        CORE_SEMANTICS_UTIL_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_core_semantics_util_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = CORE_SEMANTICS_UTIL_PROMOTED_NODE26_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-core-semantics-util-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        CORE_SEMANTICS_UTIL_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked core semantics and util required-gap inventory; classify assert, buffer, path, URL, and util failures before focused fixes"]
fn node22_supported_lane_core_semantics_util_required_gap_watchpoint() {
    let fixture_paths = core_semantics_util_required_gap_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-core-semantics-util-required-gap-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        CORE_SEMANTICS_UTIL_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked core semantics and util required-gap inventory; classify assert, buffer, path, URL, and util failures before focused fixes"]
fn node24_default_lane_core_semantics_util_required_gap_watchpoint() {
    let fixture_paths = core_semantics_util_required_gap_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-core-semantics-util-required-gap-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        CORE_SEMANTICS_UTIL_REQUIRED_GAP_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 node26 broad pre-run: ROI-ranked core semantics and util required-gap inventory; promote only dynamically green Current-lane fixtures"]
fn node26_current_lane_core_semantics_util_required_gap_watchpoint() {
    let fixture_paths = core_semantics_util_required_gap_paths(NodeCompatLane::Node26);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-core-semantics-util-required-gap-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        CORE_SEMANTICS_UTIL_REQUIRED_GAP_EXTRA_DIRS,
    );
}

const FS_HOST_IO_EXTRA_RUNTIME_FILES: &[&str] = &[
    "test/fixtures/a.js",
    "test/fixtures/baz.js",
    "test/fixtures/empty.js",
    "test/fixtures/x.txt",
    "test/fixtures/empty.txt",
    "test/fixtures/elipses.txt",
    "test/fixtures/loop.js",
    "test/fixtures/utf8_test_text.txt",
];

const FS_HOST_IO_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures/copy", "test/fixtures/cycles"];

const FS_HOST_IO_LOW_ROI_PATHS: &[&str] = &[
    "test/parallel/test-fs-existssync-memleak-longpath.js",
    "test/parallel/test-fs-sir-writes-alot.js",
    "test/parallel/test-fs-write-buffer-large.js",
    "test/parallel/test-fs-write-sigxfsz.js",
];

const FS_HOST_IO_LOW_ROI_PREFIXES: &[&str] = &["test/parallel/test-fs-promises-watch"];

fn fs_host_io_runnable_fixture_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths =
        node_compat_required_gap_paths_for_owner(lane, "streams-local-io/fs-host-io");
    fixture_paths.retain(|path| {
        !FS_HOST_IO_LOW_ROI_PATHS
            .iter()
            .any(|low_roi_path| path == low_roi_path)
            && !FS_HOST_IO_LOW_ROI_PREFIXES
                .iter()
                .any(|low_roi_prefix| path.starts_with(low_roi_prefix))
    });
    fixture_paths
}

const FS_HOST_IO_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-fs-chown-negative-one.js",
    "test/parallel/test-fs-fchown-negative-one.js",
    "test/parallel/test-fs-filehandle-use-after-close.js",
    "test/parallel/test-fs-fmap.js",
    "test/parallel/test-fs-internal-assertencoding.js",
    "test/parallel/test-fs-lchmod.js",
    "test/parallel/test-fs-lchown-negative-one.js",
    "test/parallel/test-fs-make-callback.js",
    "test/parallel/test-fs-makeStatsCallback.js",
    "test/parallel/test-fs-mkdir-recursive-eaccess.js",
    "test/parallel/test-fs-promises-statfs-validate-path.js",
    "test/parallel/test-fs-read-stream-concurrent-reads.js",
    "test/parallel/test-fs-read-stream-err.js",
    "test/parallel/test-fs-read-stream-fd-leak.js",
    "test/parallel/test-fs-read-stream-patch-open.js",
    // The fixture exits only after its positioned read assertions complete and
    // the stream close handler runs. Route that code-0 teardown through the
    // harness sentinel; do not grant a host-process exit primitive.
    "test/parallel/test-fs-read-stream-pos.js",
    "test/parallel/test-fs-read-stream-resume.js",
    "test/parallel/test-fs-readdir-recursive.js",
    "test/parallel/test-fs-readdir-stack-overflow.js",
    "test/parallel/test-fs-readdir-types-symlinks.js",
    "test/parallel/test-fs-stat-bigint.js",
    "test/parallel/test-fs-stream-destroy-emit-error.js",
    "test/parallel/test-fs-stream-double-close.js",
    // These stream subclass fixtures call fs.ReadStream or fs.WriteStream with
    // a user-created receiver and expect an overridden open() path to run. The
    // Nimbus fs wrapper preserves that receiver while still snapshotting
    // sandboxed fs options.
    "test/parallel/test-fs-stream-construct-compat-error-write.js",
    "test/parallel/test-fs-stream-construct-compat-graceful-fs.js",
    "test/parallel/test-fs-stream-construct-compat-old-node.js",
    "test/parallel/test-fs-stream-fs-options.js",
    "test/parallel/test-fs-stream-options.js",
    "test/parallel/test-fs-symlink-dir.js",
    "test/parallel/test-fs-symlink-longpath.js",
    "test/parallel/test-fs-write-reuse-callback.js",
    "test/parallel/test-fs-write-stream-change-open.js",
    "test/parallel/test-fs-write-stream-close-without-callback.js",
    "test/parallel/test-fs-write-stream-err.js",
    "test/parallel/test-fs-write-stream-file-handle-2.js",
    "test/parallel/test-fs-write-stream-fs.js",
    "test/parallel/test-fs-write-stream-throw-type-error.js",
    "test/parallel/test-fs-writestream-open-write.js",
];

const FS_HOST_IO_PROMOTED_NODE22_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-fs-read-position-validation.mjs",
    "test/parallel/test-fs-read-promises-position-validation.mjs",
    "test/parallel/test-fs-readSync-position-validation.mjs",
    "test/parallel/test-fs-utils-get-dirents.js",
];

const FS_HOST_IO_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-fastutf8stream-sync.js",
    "test/parallel/test-fs-glob-throw.mjs",
    // fs.mkdtempDisposableSync / fs.promises.mkdtempDisposable are Node 24+
    // disposable temp-dir APIs (no node22 variant exists). Each returns an
    // object with .path, .remove(), and Symbol.dispose/Symbol.asyncDispose that
    // captures the directory at creation so a later process.chdir() does not
    // break removal. The disposable wrappers shipped earlier, but the
    // underlying mkdtemp/mkdtempSync resolved a relative prefix against the
    // sandbox base rather than the per-isolate virtual process.cwd(), so the
    // chdirDoesNotAffectRemoval sub-test failed. Greened by nimbus/deno
    // v2.8.2-nimbus.16, which resolves a relative prefix against process.cwd()
    // before the op and relativizes the return for Node's relative-in/
    // relative-out contract.
    "test/parallel/test-fs-mkdtempDisposableSync.js",
    "test/parallel/test-fs-promises-mkdtempDisposable.js",
    // The node24 variants of these position-validation fixtures add an
    // empty-buffer + zero-length + invalid-position block (absent from the
    // node22 variants promoted in the node22-only group above), which requires
    // validating position before the zero-length short-circuit. Greened by
    // nimbus/deno v2.8.2-nimbus.15 (fs read/readSync position order parity).
    "test/parallel/test-fs-read-position-validation.mjs",
    "test/parallel/test-fs-read-promises-position-validation.mjs",
    "test/parallel/test-fs-readSync-position-validation.mjs",
    "test/parallel/test-fs-rmSync-special-char.js",
    // fs.stat honors an already-aborted AbortSignal (settles the callback with
    // an AbortError before issuing the stat). Greened by the same fork tag.
    "test/parallel/test-fs-stat-abort-test.js",
    "test/parallel/test-fs-write-stream.js",
    // NDS3 cycle 10: module_fs_modules.js wraps fsModule.writeSync to route a
    // thrown error through denoErrorToNodeError so a poisoned Object.prototype
    // errno setter fires and propagates an empty-message Error, matching Node's
    // test-fs-writesync-crash.js crash-path contract. No host-process exit or
    // signal primitive is granted; this runs in the fs-host-io promoted batch.
    "test/parallel/test-fs-writesync-crash.js",
];

const FS_HOST_IO_PROMOTED_NODE26_PATHS: &[&str] = &[
    "test/parallel/test-fs-chown-negative-one.js",
    "test/parallel/test-fs-constants.js",
    "test/parallel/test-fs-cp-async-async-filter-function.mjs",
    "test/parallel/test-fs-cp-async-copy-non-directory-symlink.mjs",
    "test/parallel/test-fs-cp-async-dereference-force-false-silent-fail.mjs",
    "test/parallel/test-fs-cp-async-dereference-symlink.mjs",
    "test/parallel/test-fs-cp-async-dest-symlink-points-to-src-error.mjs",
    "test/parallel/test-fs-cp-async-dir-exists-error-on-exist.mjs",
    "test/parallel/test-fs-cp-async-dir-to-file.mjs",
    "test/parallel/test-fs-cp-async-error-on-exist.mjs",
    "test/parallel/test-fs-cp-async-file-to-dir.mjs",
    "test/parallel/test-fs-cp-async-file-to-file.mjs",
    "test/parallel/test-fs-cp-async-file-url.mjs",
    "test/parallel/test-fs-cp-async-filter-child-folder.mjs",
    "test/parallel/test-fs-cp-async-filter-function.mjs",
    "test/parallel/test-fs-cp-async-identical-src-dest.mjs",
    "test/parallel/test-fs-cp-async-invalid-mode-range.mjs",
    "test/parallel/test-fs-cp-async-invalid-options-type.mjs",
    "test/parallel/test-fs-cp-async-nested-files-folders.mjs",
    "test/parallel/test-fs-cp-async-no-errors-force-false.mjs",
    "test/parallel/test-fs-cp-async-no-recursive.mjs",
    "test/parallel/test-fs-cp-async-overwrites-force-true.mjs",
    "test/parallel/test-fs-cp-async-preserve-timestamps-readonly-file.mjs",
    "test/parallel/test-fs-cp-async-preserve-timestamps.mjs",
    "test/parallel/test-fs-cp-async-same-dir-twice.mjs",
    "test/parallel/test-fs-cp-async-skip-validation-when-filtered.mjs",
    "test/parallel/test-fs-cp-async-subdirectory-of-self.mjs",
    "test/parallel/test-fs-cp-async-symlink-dest-points-to-src.mjs",
    "test/parallel/test-fs-cp-async-symlink-over-file.mjs",
    "test/parallel/test-fs-cp-async-symlink-points-to-dest.mjs",
    "test/parallel/test-fs-cp-async-with-mode-flags.mjs",
    "test/parallel/test-fs-cp-promises-async-error.mjs",
    "test/parallel/test-fs-cp-promises-file-url.mjs",
    "test/parallel/test-fs-cp-promises-invalid-mode.mjs",
    "test/parallel/test-fs-cp-promises-mode-flags.mjs",
    "test/parallel/test-fs-cp-promises-nested-folder-recursive.mjs",
    "test/parallel/test-fs-cp-promises-options-validation.mjs",
    "test/parallel/test-fs-cp-sync-apply-filter-function.mjs",
    "test/parallel/test-fs-cp-sync-async-filter-error.mjs",
    "test/parallel/test-fs-cp-sync-copy-directory-to-file-error.mjs",
    "test/parallel/test-fs-cp-sync-copy-directory-without-recursive-error.mjs",
    "test/parallel/test-fs-cp-sync-copy-file-to-directory-error.mjs",
    "test/parallel/test-fs-cp-sync-copy-file-to-file-path.mjs",
    "test/parallel/test-fs-cp-sync-copy-symlink-not-pointing-to-folder.mjs",
    "test/parallel/test-fs-cp-sync-copy-symlink-over-file-error.mjs",
    "test/parallel/test-fs-cp-sync-copy-symlinks-to-existing-symlinks.mjs",
    "test/parallel/test-fs-cp-sync-copy-to-subdirectory-error.mjs",
    "test/parallel/test-fs-cp-sync-dereference.js",
    "test/parallel/test-fs-cp-sync-dereference-directory.mjs",
    "test/parallel/test-fs-cp-sync-dereference-file.mjs",
    "test/parallel/test-fs-cp-sync-dereference-twice.mjs",
    "test/parallel/test-fs-cp-sync-dest-name-prefix-match.mjs",
    "test/parallel/test-fs-cp-sync-dest-parent-name-prefix-match.mjs",
    "test/parallel/test-fs-cp-sync-directory-not-exist-error.mjs",
    "test/parallel/test-fs-cp-sync-error-on-exist.mjs",
    "test/parallel/test-fs-cp-sync-file-url.mjs",
    "test/parallel/test-fs-cp-sync-filename-too-long-error.mjs",
    "test/parallel/test-fs-cp-sync-incompatible-options-error.mjs",
    "test/parallel/test-fs-cp-sync-mode-invalid.mjs",
    "test/parallel/test-fs-cp-sync-mode-flags.mjs",
    "test/parallel/test-fs-cp-sync-nested-files-folders.mjs",
    "test/parallel/test-fs-cp-sync-no-overwrite-force-false.mjs",
    "test/parallel/test-fs-cp-sync-options-invalid-type-error.mjs",
    "test/parallel/test-fs-cp-sync-overwrite-force-true.mjs",
    "test/parallel/test-fs-cp-sync-parent-symlink-dest-points-to-src-error.mjs",
    "test/parallel/test-fs-cp-sync-preserve-timestamps-readonly.mjs",
    "test/parallel/test-fs-cp-sync-preserve-timestamps.mjs",
    "test/parallel/test-fs-cp-sync-resolve-relative-symlinks-default.mjs",
    "test/parallel/test-fs-cp-sync-resolve-relative-symlinks-false.mjs",
    "test/parallel/test-fs-cp-sync-src-dest-identical-error.mjs",
    "test/parallel/test-fs-cp-sync-src-parent-of-dest-error.mjs",
    "test/parallel/test-fs-cp-sync-symlink-dest-points-to-src-error.mjs",
    "test/parallel/test-fs-cp-sync-symlink-points-to-dest-error.mjs",
    "test/parallel/test-fs-cp-sync-unicode-dest.mjs",
    "test/parallel/test-fs-cp-sync-unicode-folder-names.mjs",
    "test/parallel/test-fs-cp-sync-verbatim-symlinks-true.mjs",
    "test/parallel/test-fs-cp-sync-verbatim-symlinks-invalid.mjs",
    "test/parallel/test-fs-fchown-negative-one.js",
    "test/parallel/test-fs-filehandle-use-after-close.js",
    "test/parallel/test-fs-fmap.js",
    "test/parallel/test-fs-glob-throw.mjs",
    "test/parallel/test-fs-internal-assertencoding.js",
    "test/parallel/test-fs-lchmod.js",
    "test/parallel/test-fs-lchown-negative-one.js",
    "test/parallel/test-fs-make-callback.js",
    "test/parallel/test-fs-makeStatsCallback.js",
    "test/parallel/test-fs-mkdir-recursive-eaccess.js",
    "test/parallel/test-fs-mkdtempDisposableSync.js",
    "test/parallel/test-fs-open-flags.js",
    "test/parallel/test-fs-promises-file-handle-pull.js",
    "test/parallel/test-fs-promises-file-handle-pullsync.js",
    "test/parallel/test-fs-promises-file-handle-writer.js",
    "test/parallel/test-fs-promises-mkdtempDisposable.js",
    "test/parallel/test-fs-promises-statfs-validate-path.js",
    "test/parallel/test-fs-promises.js",
    "test/parallel/test-fs-read-position-validation.mjs",
    "test/parallel/test-fs-read-promises-position-validation.mjs",
    "test/parallel/test-fs-read-stream-concurrent-reads.js",
    "test/parallel/test-fs-read-stream-err.js",
    "test/parallel/test-fs-read-stream-fd-leak.js",
    "test/parallel/test-fs-read-stream-inherit.js",
    "test/parallel/test-fs-read-stream-patch-open.js",
    "test/parallel/test-fs-read-stream-pos.js",
    "test/parallel/test-fs-read-stream-resume.js",
    "test/parallel/test-fs-read-stream-throw-type-error.js",
    "test/parallel/test-fs-readSync-position-validation.mjs",
    "test/parallel/test-fs-readdir-recursive.js",
    "test/parallel/test-fs-readdir-stack-overflow.js",
    "test/parallel/test-fs-readdir-types-symlinks.js",
    "test/parallel/test-fs-rmSync-special-char.js",
    "test/parallel/test-fs-rmdir-recursive-error.js",
    "test/parallel/test-fs-rmdir-throws-not-found.js",
    "test/parallel/test-fs-rmdir-throws-on-file.js",
    "test/parallel/test-fs-stat-abort-test.js",
    "test/parallel/test-fs-stat-bigint.js",
    "test/parallel/test-fs-stat-date.mjs",
    "test/parallel/test-fs-stream-construct-compat-error-write.js",
    "test/parallel/test-fs-stream-construct-compat-graceful-fs.js",
    "test/parallel/test-fs-stream-construct-compat-old-node.js",
    "test/parallel/test-fs-stream-destroy-emit-error.js",
    "test/parallel/test-fs-stream-double-close.js",
    "test/parallel/test-fs-stream-fs-options.js",
    "test/parallel/test-fs-stream-options.js",
    "test/parallel/test-fs-symlink-dir-junction.js",
    "test/parallel/test-fs-symlink-dir-junction-relative.js",
    "test/parallel/test-fs-symlink-dir.js",
    "test/parallel/test-fs-symlink-longpath.js",
    "test/parallel/test-fs-truncate-sync.js",
    "test/parallel/test-fs-truncate.js",
    "test/parallel/test-fs-write-file-sync.js",
    "test/parallel/test-fs-write-reuse-callback.js",
    "test/parallel/test-fs-write-stream-change-open.js",
    "test/parallel/test-fs-write-stream-close-without-callback.js",
    "test/parallel/test-fs-write-stream-eagain.mjs",
    "test/parallel/test-fs-write-stream-err.js",
    "test/parallel/test-fs-write-stream-file-handle-2.js",
    "test/parallel/test-fs-write-stream-flush.js",
    "test/parallel/test-fs-write-stream-fs.js",
    "test/parallel/test-fs-write-stream-throw-type-error.js",
    "test/parallel/test-fs-write-stream.js",
    "test/parallel/test-fs-writestream-open-write.js",
    "test/parallel/test-fs-writesync-crash.js",
];

fn fs_host_io_promoted_fixture_paths(groups: &[&[&str]]) -> Vec<String> {
    groups
        .iter()
        .flat_map(|group| group.iter().copied())
        .map(str::to_string)
        .collect()
}

#[test]
fn node22_supported_lane_executes_fs_host_io_promoted_batch_fixture() {
    let fixture_paths = fs_host_io_promoted_fixture_paths(&[
        FS_HOST_IO_PROMOTED_COMMON_PATHS,
        FS_HOST_IO_PROMOTED_NODE22_ONLY_PATHS,
    ]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-fs-host-io-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        FS_HOST_IO_EXTRA_RUNTIME_FILES,
        FS_HOST_IO_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_fs_host_io_promoted_batch_fixture() {
    let fixture_paths = fs_host_io_promoted_fixture_paths(&[
        FS_HOST_IO_PROMOTED_COMMON_PATHS,
        FS_HOST_IO_PROMOTED_NODE24_ONLY_PATHS,
    ]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-fs-host-io-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        FS_HOST_IO_EXTRA_RUNTIME_FILES,
        FS_HOST_IO_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_fs_host_io_promoted_batch_fixture() {
    let fixture_paths =
        fs_host_io_promoted_fixture_paths(&[FS_HOST_IO_PROMOTED_NODE26_PATHS]);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-fs-host-io-promoted-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        FS_HOST_IO_EXTRA_RUNTIME_FILES,
        FS_HOST_IO_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked fs-host-io required-gap inventory; watch/stress/crash paths are excluded by the kill rule and remain gaps"]
fn node22_supported_lane_fs_host_io_watchpoint() {
    let fixture_paths = fs_host_io_runnable_fixture_paths(NodeCompatLane::Node22);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-fs-host-io-watchpoint",
        NodeCompatLane::Node22,
        &fixture_paths,
        FS_HOST_IO_EXTRA_RUNTIME_FILES,
        FS_HOST_IO_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 broad pre-run: ROI-ranked fs-host-io required-gap inventory; watch/stress/crash paths are excluded by the kill rule and remain gaps"]
fn node24_default_lane_fs_host_io_watchpoint() {
    let fixture_paths = fs_host_io_runnable_fixture_paths(NodeCompatLane::Node24);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-fs-host-io-watchpoint",
        NodeCompatLane::Node24,
        &fixture_paths,
        FS_HOST_IO_EXTRA_RUNTIME_FILES,
        FS_HOST_IO_EXTRA_DIRS,
    );
}

#[test]
#[ignore = "NDS3 node26 broad pre-run: ROI-ranked fs-host-io required-gap inventory; watch/stress/crash paths are excluded by the kill rule and remain gaps"]
fn node26_current_lane_fs_host_io_watchpoint() {
    let fixture_paths = fs_host_io_runnable_fixture_paths(NodeCompatLane::Node26);
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-fs-host-io-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        FS_HOST_IO_EXTRA_RUNTIME_FILES,
        FS_HOST_IO_EXTRA_DIRS,
    );
}

#[test]
fn node22_fs_rmdir_recursive_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-rmdir-recursive.js",
        "node22/test/parallel/test-fs-rmdir-recursive.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 test-fs-stat.js still requires the older JSON.stringify(Stats) field shape that the current runtime no longer preserves while matching the newer Node22/Node24 file contract"]
fn node20_fs_stat_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-stat.js",
        "node20/test/parallel/test-fs-stat.js",
        &[],
    );
}

// Greened by NDS3 cycle 11: module_fs_modules.js now restores Node 24's
// DEP0176 runtime-deprecation shape for the fs.F_OK/R_OK/W_OK/X_OK aliases.
// deno_node drops the deprecated aliases; Nimbus redefines them as configurable,
// non-enumerable, accessor-only getters (assigning throws TypeError in strict
// mode) where the first read across the four emits a single DEP0176
// DeprecationWarning naming the first-read key, returning the matching
// fs.constants value. Now a passing dynamic green-guard, no longer an ignored
// watchpoint.
#[test]
fn node24_fs_constants_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-constants.js",
        "node24/test/parallel/test-fs-constants.js",
        &[],
    );
}

// Greened by NDS3 cycle 10: module_fs_helpers.js wrapDirHandle now defines
// Dir[Symbol.asyncDispose]/[Symbol.dispose] (idempotent close, no double-close),
// matching Node 24's opendir disposal semantics. Now a passing dynamic
// green-guard, no longer an ignored watchpoint.
#[test]
fn node24_fs_promises_file_handle_dispose_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-promises-file-handle-dispose.js",
        "node24/test/parallel/test-fs-promises-file-handle-dispose.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 default-lane divergence: official v24.15.0 test-fs-write-stream.js now also requires fs.close() to be observed when destroying WriteStream directly, while the current older Node22-compatible contract still follows the older file semantics"]
fn node24_fs_write_stream_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-write-stream.js",
        "node24/test/parallel/test-fs-write-stream.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 default-lane divergence: official v24.15.0 test-fs-write-stream-autoclose-option.js now also asserts ERR_INVALID_THIS when probing WriteStream.prototype.autoClose, while the current older Node22-compatible contract still follows the older surface"]
fn node24_fs_write_stream_autoclose_option_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-write-stream-autoclose-option.js",
        "node24/test/parallel/test-fs-write-stream-autoclose-option.js",
        &[],
    );
}

#[test]
#[ignore = "Superseded by NDS3 cycle 53: node24 test-fs-symlink.js is now dynamically green-guarded by node24_default_lane_executes_cycle53_fs_symlink_batch"]
fn node24_fs_symlink_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-symlink.js",
        "node24/test/parallel/test-fs-symlink.js",
        CYCLE_FIXTURES_EXTRA_FILES,
    );
}

// Greened by NDS3 cycle 10: module_fs_helpers.js ensureDirPathInvalidThisGetter
// now wraps Dir.prototype.path with an ERR_INVALID_THIS receiver guard, matching
// Node 24's newer Dir handle receiver checks. Now a passing dynamic green-guard,
// no longer an ignored watchpoint.
#[test]
fn node24_fs_opendir_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-opendir.js",
        "node24/test/parallel/test-fs-opendir.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 default-lane divergence: official v24.15.0 test-fs-promises-watch.js adds maxQueue and overflow option validation that Nimbus has not adopted into the default lane yet"]
fn node24_fs_promises_watch_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-promises-watch.js",
        "node24/test/parallel/test-fs-promises-watch.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared runtime gap: structuredClone transfer currently leaves ArrayBuffer usable in the embedded runtime, so test-buffer-isascii.js does not raise ERR_INVALID_STATE on detached buffers"]
fn node22_buffer_isascii_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-buffer-isascii.js",
        "node20/test/parallel/test-buffer-isascii.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared runtime gap: structuredClone transfer currently leaves ArrayBuffer usable in the embedded runtime, so test-buffer-isascii.js does not raise ERR_INVALID_STATE on detached buffers"]
fn node20_buffer_isascii_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-buffer-isascii.js",
        "node20/test/parallel/test-buffer-isascii.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared runtime gap: structuredClone transfer currently leaves ArrayBuffer usable in the embedded runtime, so test-buffer-isutf8.js does not raise ERR_INVALID_STATE on detached buffers"]
fn node22_buffer_isutf8_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-buffer-isutf8.js",
        "node20/test/parallel/test-buffer-isutf8.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared runtime gap: structuredClone transfer currently leaves ArrayBuffer usable in the embedded runtime, so test-buffer-isutf8.js does not raise ERR_INVALID_STATE on detached buffers"]
fn node20_buffer_isutf8_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-buffer-isutf8.js",
        "node20/test/parallel/test-buffer-isutf8.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 test-buffer-slow.js still exercises SlowBuffer(buffer.kMaxLength), and the embedded runtime hits its 128 MB heap ceiling before Node-style range semantics"]
fn node20_buffer_slow_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-buffer-slow.js",
        "node20/test/parallel/test-buffer-slow.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node22-only path gap: official v22.15.0 expects the post-CVE path.win32.normalize() semantics that preserve the test segment in \\\\? and \\\\. device paths"]
fn node22_path_normalize_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-path-normalize.js",
        "node22/test/parallel/test-path-normalize.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node22 path gap: official v22.15.0 expects path.win32.toNamespacedPath('\\\\?\\\\foo') to retain the trailing slash, but the current runtime still returns the older Node20 shape"]
fn node22_path_makelong_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-path-makelong.js",
        "node22/test/parallel/test-path-makelong.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared path gap: official Node20/Node22 test-path-resolve.js currently fails because win32.resolve rejects drive-letter-less inputs without a CWD"]
fn node22_path_resolve_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-path-resolve.js",
        "node22/test/parallel/test-path-resolve.js",
        PATH_RESOLVE_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned shared path gap: official Node20/Node22 test-path-resolve.js currently fails because win32.resolve rejects drive-letter-less inputs without a CWD"]
fn node20_path_resolve_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-path-resolve.js",
        "node20/test/parallel/test-path-resolve.js",
        PATH_RESOLVE_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned vendored fixture tracks post-22 url.parse deprecation semantics; official Node22 v22.15.0 has no counterpart"]
fn node22_url_parse_deprecation_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-url-parse-deprecation.js",
        "test/parallel/test-url-parse-deprecation.js",
        URL_PARSE_DEPRECATION_EXTRA_FILES,
    );
}

#[test]
fn node20_legacy_lane_executes_official_core_semantics_subset() {
    run_manifested_subset_for_lane(
        "core-semantics",
        NodeCompatLane::Node20,
        CORE_SEMANTICS_BATCH,
    );
}

#[test]
fn node22_supported_lane_executes_manifested_core_semantics_subset() {
    run_manifested_subset_for_lane(
        "core-semantics",
        NodeCompatLane::Node22,
        CORE_SEMANTICS_BATCH,
    );
}

#[test]
fn node24_default_lane_executes_core_semantics_subset() {
    run_manifested_subset_for_lane(
        "core-semantics",
        NodeCompatLane::Node24,
        CORE_SEMANTICS_BATCH,
    );
}

fn node26_current_broad_residual_paths() -> &'static [&'static str] {
    &[
        "test/parallel/test-buffer-indexof.js",
        "test/parallel/test-buffer-tostring-rangeerror.js",
        "test/parallel/test-crypto-default-shake-lengths-oneshot.js",
        "test/parallel/test-crypto-dh-group-setters.js",
        "test/parallel/test-crypto-dh-modp2-views.js",
        "test/parallel/test-crypto-dh-modp2.js",
        "test/parallel/test-crypto-dh.js",
        "test/parallel/test-crypto-gcm-implicit-short-tag.js",
        "test/parallel/test-crypto-oneshot-hash-xof.js",
        "test/parallel/test-crypto-scrypt.js",
        "test/parallel/test-fs-glob.mjs",
        "test/parallel/test-fs-opendir.js",
        "test/parallel/test-fs-promises-file-handle-dispose.js",
        "test/parallel/test-fs-promises-file-handle-readLines.mjs",
        "test/parallel/test-fs-symlink.js",
        "test/parallel/test-fs-write-stream-autoclose-option.js",
        "test/parallel/test-https-agent-session-reuse.js",
        "test/parallel/test-module-multi-extensions.js",
        "test/parallel/test-process-load-env-file.js",
        "test/parallel/test-readline-promises-csi.mjs",
        "test/parallel/test-runner-get-test-context.js",
        "test/parallel/test-stream-compose.js",
        "test/parallel/test-stream-duplex.js",
        "test/parallel/test-stream-pipeline.js",
        "test/parallel/test-stream-readable-emittedReadable.js",
        "test/parallel/test-stream-readable-infinite-read.js",
        "test/parallel/test-stream-typedarray.js",
        "test/parallel/test-stream-uint8array.js",
        "test/parallel/test-trace-events-dynamic-enable.js",
        "test/parallel/test-url-parse-invalid-input.js",
        "test/parallel/test-util-parse-env.js",
    ]
}

#[test]
fn node26_current_lane_executes_manifested_core_semantics_subset() {
    run_manifested_subset_for_lane_excluding(
        "core-semantics",
        NodeCompatLane::Node26,
        CORE_SEMANTICS_BATCH,
        node26_current_broad_residual_paths(),
    );
}

#[test]
fn node20_legacy_lane_executes_official_process_and_timing_subset() {
    run_manifested_subset_for_lane(
        "process-and-timing",
        NodeCompatLane::Node20,
        PROCESS_AND_TIMING_BATCH,
    );
}

#[test]
fn node22_supported_lane_executes_manifested_process_and_timing_subset() {
    run_manifested_subset_for_lane(
        "process-and-timing",
        NodeCompatLane::Node22,
        PROCESS_AND_TIMING_BATCH,
    );
}

#[test]
fn node24_default_lane_executes_process_and_timing_subset() {
    run_manifested_subset_for_lane(
        "process-and-timing",
        NodeCompatLane::Node24,
        PROCESS_AND_TIMING_BATCH,
    );
}

#[test]
fn node26_current_lane_executes_manifested_process_and_timing_subset() {
    run_manifested_subset_for_lane_excluding(
        "process-and-timing",
        NodeCompatLane::Node26,
        PROCESS_AND_TIMING_BATCH,
        node26_current_broad_residual_paths(),
    );
}

#[test]
fn node20_legacy_lane_executes_official_streams_and_local_io_subset() {
    run_manifested_subset_for_lane(
        "streams-and-local-io",
        NodeCompatLane::Node20,
        STREAMS_AND_LOCAL_IO_BATCH,
    );
}

#[test]
fn node22_supported_lane_executes_manifested_streams_and_local_io_subset() {
    run_manifested_subset_for_lane(
        "streams-and-local-io",
        NodeCompatLane::Node22,
        STREAMS_AND_LOCAL_IO_BATCH,
    );
}

#[test]
fn node24_stream_duplex_from_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-stream-duplex-from.js",
        "node24/test/parallel/test-stream-duplex-from.js",
        &[],
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_fs_append_file_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-fs-append-file.js",
        "node24/test/parallel/test-fs-append-file.js",
        &[],
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_fs_readfile_flags_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-fs-readfile-flags.js",
        "node24/test/parallel/test-fs-readfile-flags.js",
        &[],
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_fs_whatwg_url_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-fs-whatwg-url.js",
        "node24/test/parallel/test-fs-whatwg-url.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES,
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_fs_mkdir_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-fs-mkdir.js",
        "node24/test/parallel/test-fs-mkdir.js",
        &[],
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_fs_statfs_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-fs-statfs.js",
        "node24/test/parallel/test-fs-statfs.js",
        &[],
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_fs_truncate_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-fs-truncate.js",
        "node24/test/parallel/test-fs-truncate.js",
        &[],
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_fs_watch_enoent_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-fs-watch-enoent.js",
        "node24/test/parallel/test-fs-watch-enoent.js",
        &[],
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_fs_watch_encoding_fixture() {
    run_node_compat_watchpoint_for_lane(
        "test/parallel/test-fs-watch-encoding.js",
        "node24/test/parallel/test-fs-watch-encoding.js",
        &[],
        NodeCompatLane::Node24,
    );
}

#[test]
fn node24_default_lane_executes_streams_and_local_io_subset() {
    run_manifested_subset_for_lane(
        "streams-and-local-io",
        NodeCompatLane::Node24,
        STREAMS_AND_LOCAL_IO_BATCH,
    );
}

#[test]
fn node26_current_lane_executes_manifested_streams_and_local_io_subset() {
    run_manifested_subset_for_lane_excluding(
        "streams-and-local-io",
        NodeCompatLane::Node26,
        STREAMS_AND_LOCAL_IO_BATCH,
        node26_current_broad_residual_paths(),
    );
}

#[test]
fn node20_legacy_lane_executes_official_networking_subset() {
    run_manifested_subset_for_lane("networking", NodeCompatLane::Node20, NETWORKING_BATCH);
}

#[test]
fn node22_supported_lane_executes_manifested_networking_subset() {
    run_manifested_subset_for_lane("networking", NodeCompatLane::Node22, NETWORKING_BATCH);
}

#[test]
fn node24_default_lane_networking_watchpoint() {
    run_manifested_subset_for_lane("networking", NodeCompatLane::Node24, NETWORKING_BATCH);
}

#[test]
fn node26_current_lane_executes_manifested_networking_subset() {
    run_manifested_subset_for_lane_excluding(
        "networking",
        NodeCompatLane::Node26,
        NETWORKING_BATCH,
        node26_current_broad_residual_paths(),
    );
}

#[test]
fn node20_legacy_lane_executes_official_loader_context_subset() {
    run_manifested_subset_for_lane(
        "loader-context",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_BATCH,
    );
}

#[test]
fn node22_supported_lane_executes_manifested_loader_context_subset() {
    run_manifested_subset_for_lane(
        "loader-context",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_BATCH,
    );
}

#[test]
fn node24_default_lane_executes_loader_context_subset() {
    run_manifested_subset_for_lane(
        "loader-context",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_BATCH,
    );
}

#[test]
fn node26_current_lane_executes_manifested_loader_context_subset() {
    run_manifested_subset_for_lane_excluding(
        "loader-context",
        NodeCompatLane::Node26,
        LOADER_CONTEXT_BATCH,
        node26_current_broad_residual_paths(),
    );
}

#[test]
fn node_compat_supplementary_builtin_completeness_node20() {
    run_manifested_subset_for_lane(
        "loader-context-supplementary",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_SUPPLEMENTARY_BATCH,
    );
}

#[test]
fn node_compat_supplementary_builtin_completeness_node22() {
    run_manifested_subset_for_lane(
        "loader-context-supplementary",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_SUPPLEMENTARY_BATCH,
    );
}

#[test]
fn node_compat_supplementary_builtin_completeness_node24() {
    run_manifested_subset_for_lane(
        "loader-context-supplementary",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_SUPPLEMENTARY_BATCH,
    );
}

#[test]
fn node_compat_supplementary_module_bridge_node20() {
    run_manifested_subset_for_lane(
        "loader-context-supplementary-module-bridge",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH,
    );
}

#[test]
fn node_compat_supplementary_module_bridge_node22() {
    run_manifested_subset_for_lane(
        "loader-context-supplementary-module-bridge",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH,
    );
}

#[test]
fn node_compat_supplementary_module_bridge_node24() {
    run_manifested_subset_for_lane(
        "loader-context-supplementary-module-bridge",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH,
    );
}

#[test]
fn node_compat_supplementary_global_injection_node20() {
    run_manifested_subset_for_lane(
        "loader-context-supplementary-global-injection",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH,
    );
}

#[test]
fn node_compat_supplementary_global_injection_node22() {
    run_manifested_subset_for_lane(
        "loader-context-supplementary-global-injection",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH,
    );
}

#[test]
fn node_compat_supplementary_global_injection_node24() {
    run_manifested_subset_for_lane(
        "loader-context-supplementary-global-injection",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH,
    );
}

#[test]
fn node_compat_supplementary_process_shape_node20() {
    let outcome =
        observe_seeded_fixture_runtime_outcome("node20", "supplementary/process-release-shape.js")
            .expect("supplementary process release shape node20 outcome should resolve");
    assert_eq!(
        outcome.state,
        node_compat_manifest_report::NodeCompatObservedFixtureState::Pass,
        "node20 supplementary process shape detail: {:?}",
        outcome.detail
    );
    assert!(
        outcome.detail.is_none(),
        "node20 supplementary process shape should pass without failure detail: {:?}",
        outcome.detail
    );
}

#[test]
fn node_compat_supplementary_process_shape_node22() {
    let outcome =
        observe_seeded_fixture_runtime_outcome("node22", "supplementary/process-release-shape.js")
            .expect("supplementary process release shape node22 outcome should resolve");
    assert_eq!(
        outcome.state,
        node_compat_manifest_report::NodeCompatObservedFixtureState::Pass,
        "node22 supplementary process shape detail: {:?}",
        outcome.detail
    );
    assert!(
        outcome.detail.is_none(),
        "node22 supplementary process shape should pass without failure detail: {:?}",
        outcome.detail
    );
}

#[test]
fn node_compat_supplementary_process_shape_node24() {
    let outcome =
        observe_seeded_fixture_runtime_outcome("node24", "supplementary/process-release-shape.js")
            .expect("supplementary process release shape node24 outcome should resolve");
    assert_eq!(
        outcome.state,
        node_compat_manifest_report::NodeCompatObservedFixtureState::Pass,
        "node24 supplementary process shape detail: {:?}",
        outcome.detail
    );
    assert!(
        outcome.detail.is_none(),
        "node24 supplementary process shape should pass without failure detail: {:?}",
        outcome.detail
    );
}

#[test]
fn node_compat_supplementary_runtime_node20() {
    run_manifested_subset_for_lane(
        "runtime-supplementary",
        NodeCompatLane::Node20,
        RUNTIME_SUPPLEMENTARY_BATCH,
    );
}

#[test]
fn node_compat_supplementary_runtime_node22() {
    run_manifested_subset_for_lane(
        "runtime-supplementary",
        NodeCompatLane::Node22,
        RUNTIME_SUPPLEMENTARY_BATCH,
    );
}

#[test]
fn node_compat_supplementary_runtime_node24() {
    run_manifested_subset_for_lane(
        "runtime-supplementary",
        NodeCompatLane::Node24,
        RUNTIME_SUPPLEMENTARY_BATCH,
    );
}

fn assert_signal_lifecycle_watchpoint(lane: &str) {
    let outcome =
        observe_seeded_fixture_runtime_outcome(lane, "supplementary/signal-listener-lifecycle.mjs")
            .expect("supplementary signal lifecycle outcome should resolve");
    assert_eq!(
        outcome.state,
        node_compat_manifest_report::NodeCompatObservedFixtureState::Fail
    );
    let detail = outcome
        .detail
        .expect("supplementary signal lifecycle failure should record detail");
    assert!(
        detail.contains("Deno.addSignalListener is not a function"),
        "signal lifecycle watchpoint should record missing Deno.addSignalListener: {detail}",
    );
}

#[test]
fn node_compat_supplementary_signal_lifecycle_watchpoint_node20() {
    assert_signal_lifecycle_watchpoint("node20");
}

#[test]
fn node_compat_supplementary_signal_lifecycle_watchpoint_node22() {
    assert_signal_lifecycle_watchpoint("node22");
}

#[test]
fn node_compat_supplementary_signal_lifecycle_watchpoint_node24() {
    assert_signal_lifecycle_watchpoint("node24");
}

// NDS3 wave-23 incidental free-pass promotion. Each fixture below is a
// v8-isolate-required gap that already executes green in-isolate against the
// pinned fork (v2.8.2-nimbus tag .24); the full required-gap harvest surfaced
// them as incidental passes needing no fork change. They are promoted here
// from required gaps to measured default-lane support and re-verified as
// non-ignored in-batch passes. The block is self-contained (its own staged
// dirs/files) so it neither depends on diagnostic harvest scaffolding nor
// shifts any existing watchpoint.

const WAVE23_HARVEST_PROMOTED_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/async-hooks",
    "test/es-module",
    "test/module-hooks",
    "test/fixtures/wpt",
    "test/fixtures/crypto",
    "test/fixtures/keys",
    "test/fixtures/cycles",
    "test/fixtures/es-module-url",
    "test/fixtures/es-module-loaders",
    "test/fixtures/es-module-require-cache",
    "test/fixtures/es-module-specifiers",
    "test/fixtures/es-modules",
    "test/fixtures/import-require-cycle",
    "test/fixtures/module-hooks",
    "test/fixtures/module-require-symlink",
    "test/fixtures/node_modules",
    "test/fixtures/packages",
    "test/fixtures/snapshot",
    "test/fixtures/test-module-loading-globalpaths",
    "test/fixtures/typescript",
    "test/fixtures/uncaught-exceptions",
];

const WAVE23_HARVEST_PROMOTED_EXTRA_RUNTIME_FILES: &[&str] = &[
    "test/fixtures/person-large.jpg",
    "test/fixtures/aead-vectors.js",
    "test/fixtures/a.js",
    "test/fixtures/baz.js",
    "test/fixtures/empty.js",
    "test/fixtures/empty.cjs",
    "test/fixtures/empty.json",
    "test/fixtures/empty.txt",
    "test/fixtures/x.txt",
    "test/fixtures/elipses.txt",
    "test/fixtures/loop.js",
    "test/fixtures/utf8_test_text.txt",
    "test/fixtures/experimental.json",
    "test/fixtures/invalid.json",
    "test/fixtures/is-object.js",
    "test/fixtures/module-loading-error.node",
    "test/fixtures/out-of-bound.wasm",
    "test/fixtures/pkgexports.mjs",
    "test/fixtures/printA.js",
    "test/fixtures/primitive-42.json",
    "test/fixtures/recursive-a.cjs",
    "test/fixtures/recursive-b.cjs",
    "test/fixtures/simple.wasm",
];

const WAVE23_HARVEST_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/es-module/test-cjs-prototype-pollution.js",
    "test/es-module/test-esm-data-urls.js",
    "test/es-module/test-esm-exports.mjs",
    "test/es-module/test-esm-import-attributes-errors.mjs",
    "test/es-module/test-esm-import-attributes-identity.mjs",
    "test/es-module/test-esm-import-meta-resolve-hooks.mjs",
    "test/es-module/test-esm-import-meta.mjs",
    "test/es-module/test-esm-imports.mjs",
    "test/es-module/test-esm-invalid-data-urls.js",
    "test/es-module/test-esm-json-cache.mjs",
    "test/es-module/test-esm-live-binding.mjs",
    "test/es-module/test-esm-main-lookup.mjs",
    "test/es-module/test-esm-pkgname.mjs",
    "test/es-module/test-esm-process.mjs",
    "test/es-module/test-esm-prototype-pollution.mjs",
    "test/es-module/test-esm-undefined-cjs-global-like-variables.js",
    "test/es-module/test-require-module-error-catching.js",
    "test/parallel/test-module-setsourcemapssupport.js",
    "test/parallel/test-require-process.js",
    "test/parallel/test-util-callbackify.js",
    "test/parallel/test-util-promisify-custom-names.mjs",
];

// NOTE: test/parallel/test-webcrypto-sign-verify.js was a harvest free-pass
// candidate but exceeds the wall-clock timeout under the non-ignored batch
// green-guard (heavy RSA/ECDSA sign+verify across the full algorithm matrix),
// so it is intentionally NOT promoted here — it stays a required gap and a
// candidate for the webcrypto fork-fix cluster, not a measured pass.
const WAVE23_HARVEST_PROMOTED_NODE22_ONLY_PATHS: &[&str] =
    &["test/parallel/test-eventemitter-asyncresource.js"];

const WAVE23_HARVEST_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/es-module/test-esm-import-attributes-errors.js",
];

#[test]
fn node22_supported_lane_executes_wave23_harvest_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = WAVE23_HARVEST_PROMOTED_COMMON_PATHS
        .iter()
        .chain(WAVE23_HARVEST_PROMOTED_NODE22_ONLY_PATHS.iter())
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-wave23-harvest-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        WAVE23_HARVEST_PROMOTED_EXTRA_RUNTIME_FILES,
        WAVE23_HARVEST_PROMOTED_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_wave23_harvest_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = WAVE23_HARVEST_PROMOTED_COMMON_PATHS
        .iter()
        .chain(WAVE23_HARVEST_PROMOTED_NODE24_ONLY_PATHS.iter())
        .map(|path| (*path).to_string())
        .collect();
    // The Node24 vendored tree additionally carries the webcrypto fixture subtree.
    let mut extra_dirs: Vec<&str> = WAVE23_HARVEST_PROMOTED_EXTRA_DIRS.to_vec();
    extra_dirs.push("test/fixtures/webcrypto");
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-wave23-harvest-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        WAVE23_HARVEST_PROMOTED_EXTRA_RUNTIME_FILES,
        &extra_dirs,
    );
}

// NDS3 wave-24 fork-fix promotion block. Self-contained green guards for the two
// fixtures the v2.8.2-nimbus.25 fork bump turned green on both lanes:
//   - test-process-cpuUsage.js  (deno_node process.cpuUsage() now reads the
//     current-thread CPU op directly instead of the stripped Deno namespace)
//   - test-global-webstreams.js (bootstrap now seeds TextEncoder/DecoderStream
//     and Compression/DecompressionStream from the same deno_web ext modules the
//     stream/web polyfill uses, so the globals are identity-equal to require's)
// Carries its own extra-dirs so it does not depend on any other batch's scaffolding.
const WAVE24_FORK_FIX_PROMOTED_EXTRA_DIRS: &[&str] = &["test/common", "test/fixtures"];

const WAVE24_FORK_FIX_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/parallel/test-global-webstreams.js",
    "test/parallel/test-process-cpuUsage.js",
];

#[test]
fn node22_supported_lane_executes_wave24_fork_fix_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = WAVE24_FORK_FIX_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| path.to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-wave24-fork-fix-promoted-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        WAVE24_FORK_FIX_PROMOTED_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_wave24_fork_fix_promoted_batch_fixture() {
    let fixture_paths: Vec<String> = WAVE24_FORK_FIX_PROMOTED_COMMON_PATHS
        .iter()
        .map(|path| path.to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-wave24-fork-fix-promoted-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        WAVE24_FORK_FIX_PROMOTED_EXTRA_DIRS,
    );
}
