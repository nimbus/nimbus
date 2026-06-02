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
#[ignore = "Pinned Node24 default-lane divergence: test-crypto-scrypt.js expects ERR_INCOMPATIBLE_OPTION_PAIR for duplicate short/long option pairs, while the current runtime still throws the older ERR_CRYPTO_SCRYPT_INVALID_PARAMETER shape used by the verified Node22 baseline"]
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
#[ignore = "Pinned Node24 default-lane drift: test-crypto-dh-stateless.js still expects ERR_OSSL_FAILED_DURING_DERIVATION on the invalid X25519 public-key case"]
fn node24_loader_context_crypto_dh_stateless_supported_watchpoint_batch() {
    run_node_compat_watchpoint_entry_batch(
        "node24-loader-context-crypto-dh-stateless-supported-watchpoints",
        NodeCompatLane::Node24,
        NODE24_LOADER_CONTEXT_CRYPTO_DH_STATELESS_SUPPORTED_WATCHPOINT_BATCH,
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
    "test/parallel/test-diagnostics-channel-http2-client-stream-error.js",
    "test/parallel/test-diagnostics-channel-http2-client-stream-finish.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-close-error.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-close.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-created-start-timing.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-created.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-error.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-finish.js",
    "test/parallel/test-diagnostics-channel-http2-server-stream-start.js",
    "test/parallel/test-diagnostics-channel-memory-leak.js",
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

const STREAMS_WEB_PLATFORM_EXTRA_DIRS: &[&str] = &["test/common"];

const STREAMS_WEB_PLATFORM_LOW_ROI_PATHS: &[&str] =
    &[
        "test/parallel/test-stream-base-typechecking.js",
        "test/parallel/test-webstreams-clone-unref.js",
        "test/parallel/test-whatwg-webstreams-transform-stream-members.js",
    ];

const STREAMS_WEB_PLATFORM_PROMOTED_COMMON_PATHS: &[&str] = &[
    "test/async-hooks/test-async-local-storage-stream-finished.js",
    "test/parallel/test-file-write-stream.js",
    "test/parallel/test-file-write-stream2.js",
    "test/parallel/test-file-write-stream3.js",
    "test/parallel/test-file-write-stream4.js",
    "test/parallel/test-filehandle-readablestream.js",
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
    "test/parallel/test-webstream-string-tag.js",
    "test/parallel/test-webstream-structured-clone-no-leftovers.mjs",
    "test/parallel/test-webstreams-compose.js",
    "test/parallel/test-webstreams-finished.js",
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
    "test/parallel/test-webstreams-adapters-sync-write-error.js",
    "test/parallel/test-webstreams-decompression-reject-trailing.js",
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
    "test/fixtures/primitive-42.json",
    "test/fixtures/recursive-a.cjs",
    "test/fixtures/recursive-b.cjs",
    "test/fixtures/simple.wasm",
];

const ESM_MODULE_LOADER_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/es-module",
    "test/fixtures/cycles",
    "test/fixtures/es-module-loaders",
    "test/fixtures/es-module-require-cache",
    "test/fixtures/es-module-specifiers",
    "test/fixtures/es-modules",
    "test/fixtures/import-require-cycle",
    "test/fixtures/module-hooks",
    "test/fixtures/module-require-symlink",
    "test/fixtures/node_modules",
    "test/fixtures/packages",
    "test/fixtures/test-module-loading-globalpaths",
    "test/fixtures/typescript",
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
    "test/es-module/test-esm-symlink.js",
    "test/es-module/test-esm-syntax-error.mjs",
    "test/es-module/test-esm-throw-undefined.mjs",
    "test/es-module/test-esm-tla.mjs",
    "test/es-module/test-esm-type-field.mjs",
    "test/es-module/test-esm-type-main.mjs",
    "test/es-module/test-esm-util-types.mjs",
    "test/es-module/test-esm-windows.js",
    "test/es-module/test-loaders-hidden-from-users.js",
    "test/es-module/test-require-as-esm-interop.mjs",
    "test/es-module/test-require-module-cached-tla.js",
    "test/es-module/test-require-module-conditional-exports.js",
    "test/es-module/test-require-module-cycle-cjs-esm-esm.js",
    "test/es-module/test-require-module-defined-esmodule.js",
    "test/es-module/test-require-module-detect-entry-point-aou.js",
    "test/es-module/test-require-module-detect-entry-point.js",
    "test/es-module/test-require-module-dont-detect-cjs.js",
    "test/es-module/test-require-module-dynamic-import-3.js",
    "test/es-module/test-require-module-dynamic-import-4.js",
    "test/es-module/test-require-module-instantiated.mjs",
    "test/es-module/test-require-module-retry-import-errored.js",
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
    "test/es-module/test-require-module-with-detection.js",
    "test/es-module/test-vm-compile-function-leak.js",
    "test/es-module/test-vm-compile-function-lineoffset.js",
    "test/es-module/test-vm-contextified-script-leak.js",
    "test/es-module/test-vm-source-text-module-leak.js",
    "test/es-module/test-vm-synthetic-module-leak.js",
    "test/es-module/test-wasm-memory-out-of-bound.js",
    "test/es-module/test-wasm-simple.js",
    "test/parallel/test-module-circular-dependency-warning.js",
    "test/parallel/test-module-circular-symlinks.js",
    "test/parallel/test-module-globalpaths-nodepath.js",
    "test/parallel/test-require-resolve-invalid-paths.js",
];

const ESM_MODULE_LOADER_PROMOTED_NODE22_ONLY_PATHS: &[&str] = &[
    "test/es-module/test-esm-preserve-symlinks.js",
    "test/es-module/test-esm-symlink-main.js",
    "test/es-module/test-require-module-twice.js",
    "test/es-module/test-require-module.js",
];

const ESM_MODULE_LOADER_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/es-module/test-esm-cjs-exports.js",
    "test/es-module/test-esm-wasm-escape-import-names.mjs",
    "test/es-module/test-esm-wasm-load-exports.mjs",
    "test/es-module/test-esm-wasm-source-phase-static.mjs",
    "test/parallel/test-require-resolve-opts-paths-relative.js",
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
    "test/async-hooks/test-embedder.api.async-resource.runInAsyncScope.js",
    "test/async-hooks/test-no-assert-when-disabled.js",
    "test/parallel/test-async-hooks-close-during-destroy.js",
    "test/parallel/test-async-hooks-destroy-on-gc.js",
    "test/parallel/test-async-hooks-disable-gc-tracking.js",
    "test/parallel/test-async-hooks-http-agent-destroy.js",
    "test/parallel/test-async-hooks-http-agent.js",
    "test/parallel/test-async-hooks-prevent-double-destroy.js",
    "test/parallel/test-async-hooks-run-in-async-scope-caught-exception.js",
    "test/parallel/test-async-hooks-vm-gc.js",
];

const ASYNC_HOOKS_PROMOTED_NODE24_ONLY_PATHS: &[&str] =
    &["test/parallel/test-async-hooks-enabledhooksexits.js"];

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

const NETWORKING_CRYPTO_REQUIRED_GAP_EXTRA_RUNTIME_FILES: &[&str] =
    &["test/fixtures/aead-vectors.js"];

const NETWORKING_CRYPTO_REQUIRED_GAP_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures/crypto",
    "test/fixtures/keys",
];

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

const FS_HOST_IO_EXTRA_DIRS: &[&str] = &["test/common"];

const FS_HOST_IO_LOW_ROI_PATHS: &[&str] = &[
    "test/parallel/test-fs-existssync-memleak-longpath.js",
    "test/parallel/test-fs-sir-writes-alot.js",
    "test/parallel/test-fs-write-buffer-large.js",
    "test/parallel/test-fs-write-sigxfsz.js",
    "test/parallel/test-fs-writesync-crash.js",
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
    "test/parallel/test-fs-fchown-negative-one.js",
    "test/parallel/test-fs-filehandle-use-after-close.js",
    "test/parallel/test-fs-fmap.js",
    "test/parallel/test-fs-internal-assertencoding.js",
    "test/parallel/test-fs-lchown-negative-one.js",
    "test/parallel/test-fs-make-callback.js",
    "test/parallel/test-fs-makeStatsCallback.js",
    "test/parallel/test-fs-mkdir-recursive-eaccess.js",
    "test/parallel/test-fs-promises-statfs-validate-path.js",
    "test/parallel/test-fs-read-stream-concurrent-reads.js",
    "test/parallel/test-fs-read-stream-err.js",
    "test/parallel/test-fs-read-stream-fd-leak.js",
    "test/parallel/test-fs-read-stream-patch-open.js",
    "test/parallel/test-fs-read-stream-resume.js",
    "test/parallel/test-fs-readdir-recursive.js",
    "test/parallel/test-fs-readdir-stack-overflow.js",
    "test/parallel/test-fs-readdir-types-symlinks.js",
    "test/parallel/test-fs-stat-bigint.js",
    "test/parallel/test-fs-stream-destroy-emit-error.js",
    "test/parallel/test-fs-stream-double-close.js",
    "test/parallel/test-fs-stream-options.js",
    "test/parallel/test-fs-symlink-dir.js",
    "test/parallel/test-fs-symlink-longpath.js",
    "test/parallel/test-fs-write-reuse-callback.js",
    "test/parallel/test-fs-write-stream-change-open.js",
    "test/parallel/test-fs-write-stream-close-without-callback.js",
    "test/parallel/test-fs-write-stream-err.js",
    "test/parallel/test-fs-write-stream-file-handle-2.js",
    "test/parallel/test-fs-writestream-open-write.js",
];

const FS_HOST_IO_PROMOTED_NODE22_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-fs-read-position-validation.mjs",
    "test/parallel/test-fs-read-promises-position-validation.mjs",
    "test/parallel/test-fs-readSync-position-validation.mjs",
    "test/parallel/test-fs-utils-get-dirents.js",
];

const FS_HOST_IO_PROMOTED_NODE24_ONLY_PATHS: &[&str] = &[
    "test/parallel/test-fs-glob-throw.mjs",
    "test/parallel/test-fs-rmSync-special-char.js",
    "test/parallel/test-fs-write-stream.js",
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
#[ignore = "Pinned Node24 default-lane divergence: official v24.15.0 test-fs-constants.js expects a newer constant-surface TypeError gate that Nimbus has not adopted into the default lane yet"]
fn node24_fs_constants_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-constants.js",
        "node24/test/parallel/test-fs-constants.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 default-lane divergence: official v24.15.0 test-fs-promises-file-handle-dispose.js now also asserts opendir Dir[Symbol.asyncDispose]() close semantics that the current runtime does not yet match"]
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
#[ignore = "Pinned Node24 default-lane divergence: official v24.15.0 test-fs-symlink.js still expects the newer invalid-type ERR_INVALID_ARG_VALUE contract, while the current runtime intentionally keeps the older Node22-compatible ERR_FS_INVALID_SYMLINK_TYPE behavior"]
fn node24_fs_symlink_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-symlink.js",
        "node24/test/parallel/test-fs-symlink.js",
        CYCLE_FIXTURES_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned Node24 default-lane divergence: official v24.15.0 test-fs-opendir.js now also asserts ERR_INVALID_THIS for newer Dir handle receiver checks, while the current runtime intentionally keeps the older Node22-compatible directory-handle surface"]
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
