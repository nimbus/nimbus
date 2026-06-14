// NDS3 cycle-78 WebCrypto EC/RSA import/export parity promotion.
//
// Deno fork tag v2.8.3-nimbus.27 aligns WebCrypto import/export error
// semantics with Node for EC/RSA JWK/DER validation, key extractability,
// requested key usages, and EC PKCS#8 private scalar validation.

const NDS3_CYCLE78_WEBCRYPTO_IMPORT_EXPORT_PATHS: &[&str] = &[
    "test/parallel/test-webcrypto-export-import-ec.js",
    "test/parallel/test-webcrypto-export-import-rsa.js",
];

const NDS3_CYCLE78_WEBCRYPTO_IMPORT_EXPORT_NODE22_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures",
    "test/fixtures/crypto",
    "test/fixtures/keys",
];

const NDS3_CYCLE78_WEBCRYPTO_IMPORT_EXPORT_NODE24_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures",
    "test/fixtures/crypto",
    "test/fixtures/keys",
    "test/fixtures/webcrypto",
];

#[test]
fn node22_supported_lane_executes_cycle78_webcrypto_import_export_batch() {
    let fixture_paths = NDS3_CYCLE78_WEBCRYPTO_IMPORT_EXPORT_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle78-webcrypto-import-export-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE78_WEBCRYPTO_IMPORT_EXPORT_NODE22_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle78_webcrypto_import_export_batch() {
    let fixture_paths = NDS3_CYCLE78_WEBCRYPTO_IMPORT_EXPORT_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle78-webcrypto-import-export-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE78_WEBCRYPTO_IMPORT_EXPORT_NODE24_EXTRA_DIRS,
    );
}
