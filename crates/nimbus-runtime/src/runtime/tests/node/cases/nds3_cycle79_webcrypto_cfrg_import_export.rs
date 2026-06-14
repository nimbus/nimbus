// NDS3 cycle-79 WebCrypto CFRG import/export parity promotion.
//
// Deno fork tag v2.8.3-nimbus.28 adds Ed448 import/export support and fixes
// X448 public-key derivation to use RFC 7748 raw clamped scalar bits instead of
// reducing the scalar through Ed448 group arithmetic.

const NDS3_CYCLE79_WEBCRYPTO_CFRG_IMPORT_EXPORT_PATHS: &[&str] =
    &["test/parallel/test-webcrypto-export-import-cfrg.js"];

const NDS3_CYCLE79_WEBCRYPTO_CFRG_IMPORT_EXPORT_NODE22_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures",
    "test/fixtures/crypto",
    "test/fixtures/keys",
];

const NDS3_CYCLE79_WEBCRYPTO_CFRG_IMPORT_EXPORT_NODE24_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures",
    "test/fixtures/crypto",
    "test/fixtures/keys",
    "test/fixtures/webcrypto",
];

#[test]
fn node22_supported_lane_executes_cycle79_webcrypto_cfrg_import_export_batch() {
    let fixture_paths = NDS3_CYCLE79_WEBCRYPTO_CFRG_IMPORT_EXPORT_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle79-webcrypto-cfrg-import-export-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE79_WEBCRYPTO_CFRG_IMPORT_EXPORT_NODE22_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle79_webcrypto_cfrg_import_export_batch() {
    let fixture_paths = NDS3_CYCLE79_WEBCRYPTO_CFRG_IMPORT_EXPORT_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle79-webcrypto-cfrg-import-export-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE79_WEBCRYPTO_CFRG_IMPORT_EXPORT_NODE24_EXTRA_DIRS,
    );
}
