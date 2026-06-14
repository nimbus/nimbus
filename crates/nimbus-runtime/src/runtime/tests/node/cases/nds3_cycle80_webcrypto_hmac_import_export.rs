// NDS3 cycle-80 WebCrypto HMAC import/export parity promotion.
//
// Deno fork tag v2.8.3-nimbus.29 aligns SubtleCrypto.importKey() argument
// error codes and HMAC import error messages with the node22 fixture. The
// node24 copy of this fixture still reaches the KMAC native-provider gap and is
// intentionally not promoted here.

const NDS3_CYCLE80_WEBCRYPTO_HMAC_IMPORT_EXPORT_NODE22_PATHS: &[&str] =
    &["test/parallel/test-webcrypto-export-import.js"];

const NDS3_CYCLE80_WEBCRYPTO_HMAC_IMPORT_EXPORT_NODE22_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures",
    "test/fixtures/crypto",
    "test/fixtures/keys",
];

#[test]
fn node22_supported_lane_executes_cycle80_webcrypto_hmac_import_export_batch() {
    let fixture_paths = NDS3_CYCLE80_WEBCRYPTO_HMAC_IMPORT_EXPORT_NODE22_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle80-webcrypto-hmac-import-export-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE80_WEBCRYPTO_HMAC_IMPORT_EXPORT_NODE22_EXTRA_DIRS,
    );
}
