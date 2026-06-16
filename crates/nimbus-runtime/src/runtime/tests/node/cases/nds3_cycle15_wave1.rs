// NDS3 cycle-15 wave-1: webcrypto support-file staging recovery promotions.
//
// These node24 webcrypto fixtures failed the cycle-13 census only because that
// census did not stage the webcrypto support fixtures (test/fixtures/crypto and
// test/fixtures/webcrypto). With those staged they execute cleanly on the
// published v2.8.2-nimbus.28 baseline: a one-process-per-fixture probe reported
// passed=1/skipped=0/failed=0 for each, re-confirmed by the enforced batch
// below. The batch helper re-executes every fixture and panics on any assertion
// mismatch or skip-to-empty -- the dynamic green-guard that keeps the promotion
// honest. No fork edit is required; this is pure support-file staging recovery,
// so each promotion drops node24's v8-isolate-required gap count by one.
//
// Held back as honest, still-open gaps (NOT promoted):
//   * test-webcrypto-encrypt-decrypt-aes.js hangs past the harness timeout
//     against this fork -- a separate fork defect deferred to a future cycle.
//   * export-import-{ec,cfrg,rsa,<generic>}, keygen[-kmac], sign-verify-{eddsa,
//     kmac,<generic>}, wrap-unwrap, derivekey, derivebits-hkdf,
//     deduplicate-usages, supports, and promise-prototype-pollution fail on
//     genuine behavioral parity (NotSupportedError / assertion mismatch) even
//     with the support fixtures staged.
//
// The #[test] calls run_node_compat_watchpoint_path_batch_with_lane_extra_dirs
// directly so the classifier's static execution-marker scan attributes each
// fixture to node24 and the batch helper enforces the dynamic pass.

const NDS3_CYCLE15_W1_WCRYPTO_NODE24_PATHS: &[&str] = &[
    "test/parallel/test-webcrypto-derivebits-argon2.js",
    "test/parallel/test-webcrypto-encap-decap-ml-kem.js",
    "test/parallel/test-webcrypto-encrypt-decrypt-chacha20-poly1305.js",
    "test/parallel/test-webcrypto-export-import-ml-dsa.js",
    "test/parallel/test-webcrypto-export-import-ml-kem.js",
    "test/parallel/test-webcrypto-sign-verify-ml-dsa.js",
    "test/parallel/test-webcrypto-sign-verify-rsa.js",
];

const NDS3_CYCLE15_W1_WCRYPTO_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures/crypto",
    "test/fixtures/webcrypto",
];

#[test]
fn node24_default_lane_executes_cycle15_w1_wcrypto_batch() {
    let fixture_paths = NDS3_CYCLE15_W1_WCRYPTO_NODE24_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle15-w1-wcrypto-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE15_W1_WCRYPTO_EXTRA_DIRS,
    );
}
