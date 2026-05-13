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
#[ignore = "Pinned node-tools node:test/reporters watchpoint: test-runner-run-files-undefined.mjs is now narrowed to the missing node:test/reporters builtin family rather than the earlier eval/input-type harness gap"]
fn node22_node_tools_test_runner_reporters_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-runner-run-files-undefined.mjs",
        "test/parallel/test-runner-run-files-undefined.mjs",
        COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES,
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
#[ignore = "Pinned Node24 supported-lane divergence: test-crypto-scrypt.js expects ERR_INCOMPATIBLE_OPTION_PAIR for duplicate short/long option pairs, while the current runtime still throws the older ERR_CRYPTO_SCRYPT_INVALID_PARAMETER shape used by the verified Node22 baseline"]
fn node24_loader_context_crypto_scrypt_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-crypto-scrypt.js",
        "node24/test/parallel/test-crypto-scrypt.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES,
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
#[ignore = "Pinned Node24 supported-lane drift: test-crypto-dh-stateless.js still expects ERR_OSSL_FAILED_DURING_DERIVATION on the invalid X25519 public-key case"]
fn node24_loader_context_crypto_dh_stateless_supported_watchpoint_batch() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-crypto-dh-stateless-supported-watchpoints",
        NodeCompatLane::Node24,
        NODE24_LOADER_CONTEXT_CRYPTO_DH_STATELESS_SUPPORTED_WATCHPOINT_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node20 supported-lane divergence: test-crypto-dh.js still expects the older OpenSSL invalid-secret message while the verified Node22 baseline now returns the newer unspecified-validation shape"]
fn node20_loader_context_crypto_dh_supported_watchpoint_batch() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-crypto-dh-supported-watchpoints",
        NodeCompatLane::Node20,
        NODE20_LOADER_CONTEXT_CRYPTO_DH_SUPPORTED_WATCHPOINT_BATCH,
    );
}

#[test]
fn node22_loader_context_crypto_authenticated_and_aes_wrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-loader-context-crypto-authenticated-and-aes-wrap-batch",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_CRYPTO_AUTHENTICATED_AND_AES_WRAP_BATCH,
    );
}

#[test]
fn node20_loader_context_crypto_authenticated_and_aes_wrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-crypto-authenticated-and-aes-wrap-batch",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_CRYPTO_AUTHENTICATED_AND_AES_WRAP_BATCH,
    );
}

#[test]
fn node24_loader_context_crypto_authenticated_and_aes_wrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-crypto-authenticated-and-aes-wrap-batch",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_CRYPTO_AUTHENTICATED_AND_AES_WRAP_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node20 supported-lane divergence: test-crypto-authenticated.js still expects the older deprecation-warning ordering without DEP0182"]
fn node20_loader_context_crypto_authenticated_supported_watchpoint_batch() {
    run_node_compat_watchpoint_entry_batch(
        "node20-loader-context-crypto-authenticated-supported-watchpoints",
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
#[ignore = "Pinned Node20 supported-lane divergence: test-https-hwm.js still times out on the current Node20 lane while the Node22/Node24 official files complete"]
fn node20_https_hwm_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-https-hwm.js",
        "node20/test/parallel/test-https-hwm.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned Node20 supported-lane divergence: test-tls-connect-hwm-option.js still times out on the current Node20 lane while the Node22/Node24 official files complete"]
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
#[ignore = "Pinned Node24 supported-lane divergence: test-stream-pipeline.js currently returns an AbortError-style 'The operation was aborted' message where the staged Node24 fixture still expects the inner 'Boom!' pipeline error message"]
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

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-constants.js expects a newer constant-surface TypeError gate that Nimbus has not adopted into the current Node22 contract"]
fn node24_fs_constants_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-constants.js",
        "node24/test/parallel/test-fs-constants.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-promises-file-handle-dispose.js now also asserts opendir Dir[Symbol.asyncDispose]() close semantics that the current runtime does not yet match"]
fn node24_fs_promises_file_handle_dispose_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-promises-file-handle-dispose.js",
        "node24/test/parallel/test-fs-promises-file-handle-dispose.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-write-stream.js now also requires fs.close() to be observed when destroying WriteStream directly, while the current Node22 contract still follows the older file semantics"]
fn node24_fs_write_stream_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-write-stream.js",
        "node24/test/parallel/test-fs-write-stream.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-write-stream-autoclose-option.js now also asserts ERR_INVALID_THIS when probing WriteStream.prototype.autoClose, while the current Node22 contract still follows the older surface"]
fn node24_fs_write_stream_autoclose_option_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-write-stream-autoclose-option.js",
        "node24/test/parallel/test-fs-write-stream-autoclose-option.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-symlink.js still expects the newer invalid-type ERR_INVALID_ARG_VALUE contract, while the current runtime intentionally keeps the Node22 ERR_FS_INVALID_SYMLINK_TYPE behavior"]
fn node24_fs_symlink_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-symlink.js",
        "node24/test/parallel/test-fs-symlink.js",
        CYCLE_FIXTURES_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-opendir.js now also asserts ERR_INVALID_THIS for newer Dir handle receiver checks, while the current runtime intentionally keeps the Node22 directory-handle surface"]
fn node24_fs_opendir_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-opendir.js",
        "node24/test/parallel/test-fs-opendir.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-promises-watch.js adds maxQueue and overflow option validation that Nimbus has not adopted into the current Node22-based fs.watch contract"]
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
fn node20_supported_lane_executes_official_core_semantics_subset() {
    run_manifested_subset_for_lane(
        "core-semantics",
        NodeCompatLane::Node20,
        CORE_SEMANTICS_BATCH,
    );
}

#[test]
fn node22_default_lane_executes_manifested_core_semantics_subset() {
    run_manifested_subset_for_lane(
        "core-semantics",
        NodeCompatLane::Node22,
        CORE_SEMANTICS_BATCH,
    );
}

#[test]
#[ignore = "Node24 supported lane watchpoint: the broad core semantics batch includes known newer-console-clear drift and remains classified until that fixture is promoted green"]
fn node24_supported_lane_core_semantics_watchpoint() {
    run_manifested_subset_for_lane(
        "core-semantics",
        NodeCompatLane::Node24,
        CORE_SEMANTICS_BATCH,
    );
}

#[test]
fn node20_supported_lane_executes_official_process_and_timing_subset() {
    run_manifested_subset_for_lane(
        "process-and-timing",
        NodeCompatLane::Node20,
        PROCESS_AND_TIMING_BATCH,
    );
}

#[test]
fn node22_default_lane_executes_manifested_process_and_timing_subset() {
    run_manifested_subset_for_lane(
        "process-and-timing",
        NodeCompatLane::Node22,
        PROCESS_AND_TIMING_BATCH,
    );
}

#[test]
#[ignore = "Node24 supported lane watchpoint: the broad process/timing batch is classified until each carried fixture is replayed and promoted under the supported-lane gate"]
fn node24_supported_lane_process_and_timing_watchpoint() {
    run_manifested_subset_for_lane(
        "process-and-timing",
        NodeCompatLane::Node24,
        PROCESS_AND_TIMING_BATCH,
    );
}

#[test]
fn node20_supported_lane_executes_official_streams_and_local_io_subset() {
    run_manifested_subset_for_lane(
        "streams-and-local-io",
        NodeCompatLane::Node20,
        STREAMS_AND_LOCAL_IO_BATCH,
    );
}

#[test]
fn node22_default_lane_executes_manifested_streams_and_local_io_subset() {
    run_manifested_subset_for_lane(
        "streams-and-local-io",
        NodeCompatLane::Node22,
        STREAMS_AND_LOCAL_IO_BATCH,
    );
}

#[test]
#[ignore = "Node24 supported lane watchpoint: the broad streams/local-I/O batch is classified until each carried fixture is replayed and promoted under the supported-lane gate"]
fn node24_supported_lane_streams_and_local_io_watchpoint() {
    run_manifested_subset_for_lane(
        "streams-and-local-io",
        NodeCompatLane::Node24,
        STREAMS_AND_LOCAL_IO_BATCH,
    );
}

#[test]
fn node20_supported_lane_executes_official_networking_subset() {
    run_manifested_subset_for_lane("networking", NodeCompatLane::Node20, NETWORKING_BATCH);
}

#[test]
fn node22_default_lane_executes_manifested_networking_subset() {
    run_manifested_subset_for_lane("networking", NodeCompatLane::Node22, NETWORKING_BATCH);
}

#[test]
#[ignore = "Node24 supported lane watchpoint: the broad networking batch is classified until each carried fixture is replayed and promoted under the supported-lane gate"]
fn node24_supported_lane_networking_watchpoint() {
    run_manifested_subset_for_lane("networking", NodeCompatLane::Node24, NETWORKING_BATCH);
}

#[test]
fn node20_supported_lane_executes_official_loader_context_subset() {
    run_manifested_subset_for_lane(
        "loader-context",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_BATCH,
    );
}

#[test]
fn node22_default_lane_executes_manifested_loader_context_subset() {
    run_manifested_subset_for_lane(
        "loader-context",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_BATCH,
    );
}

#[test]
#[ignore = "Node24 supported lane watchpoint: the broad loader/context batch is classified until each carried fixture is replayed and promoted under the supported-lane gate"]
fn node24_supported_lane_loader_context_watchpoint() {
    run_manifested_subset_for_lane(
        "loader-context",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_BATCH,
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
        node_compat_manifest_report::NodeCompatObservedFixtureState::Fail
    );
    let detail = outcome
        .detail
        .expect("node20 supplementary process shape failure should record detail");
    assert!(
        detail.contains("v22.0.0-nimbus") && detail.contains("/^v20\\./"),
        "node20 supplementary process shape should record the cross-lane version drift: {detail}",
    );
}

#[test]
fn node_compat_supplementary_process_shape_node22() {
    let outcome =
        observe_seeded_fixture_runtime_outcome("node22", "supplementary/process-release-shape.js")
            .expect("supplementary process release shape node22 outcome should resolve");
    assert_eq!(
        outcome.state,
        node_compat_manifest_report::NodeCompatObservedFixtureState::Fail
    );
    let detail = outcome
        .detail
        .expect("node22 supplementary process shape failure should record detail");
    assert!(
        detail.contains("undefined !== 'Jod'"),
        "node22 supplementary process shape should record the missing LTS label: {detail}",
    );
}

#[test]
fn node_compat_supplementary_process_shape_node24() {
    let outcome =
        observe_seeded_fixture_runtime_outcome("node24", "supplementary/process-release-shape.js")
            .expect("supplementary process release shape node24 outcome should resolve");
    assert_eq!(
        outcome.state,
        node_compat_manifest_report::NodeCompatObservedFixtureState::Fail
    );
    let detail = outcome
        .detail
        .expect("node24 supplementary process shape failure should record detail");
    assert!(
        detail.contains("v22.0.0-nimbus") && detail.contains("/^v24\\./"),
        "node24 supplementary process shape should record the supported-lane version drift: {detail}",
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
