// NDS3 cycle-87 WebCrypto KMAC and usage canonicalization promotion.
//
// Deno fork tag v2.8.3-nimbus.35 adds WebCrypto KMAC128/KMAC256 sign/verify,
// import/export, and key generation support, aligns canonical CryptoKey usage
// ordering/deduplication, and closes small WebCrypto error/metadata parity gaps
// exposed by the aggregate keygen and deriveKey fixtures. The broader
// `test-webcrypto-sign-verify.js` fixture remained over the harness wall-clock
// on the published tag and is intentionally not promoted here.

const NDS3_CYCLE87_WEBCRYPTO_NODE24_PATHS: &[&str] = &[
    "test/parallel/test-webcrypto-deduplicate-usages.js",
    "test/parallel/test-webcrypto-derivekey.js",
    "test/parallel/test-webcrypto-export-import.js",
    "test/parallel/test-webcrypto-keygen-kmac.js",
    "test/parallel/test-webcrypto-keygen.js",
    "test/parallel/test-webcrypto-sign-verify-kmac.js",
];

const NDS3_CYCLE87_WEBCRYPTO_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures",
    "test/fixtures/crypto",
    "test/fixtures/keys",
    "test/fixtures/webcrypto",
];

#[test]
fn node24_default_lane_executes_cycle87_webcrypto_batch() {
    let fixture_paths = NDS3_CYCLE87_WEBCRYPTO_NODE24_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle87-webcrypto-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE87_WEBCRYPTO_EXTRA_DIRS,
    );
}
