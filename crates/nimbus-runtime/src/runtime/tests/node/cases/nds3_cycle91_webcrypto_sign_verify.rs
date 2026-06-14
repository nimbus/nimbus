// NDS3 cycle-91 WebCrypto sign/verify promotion.
//
// Deno fork tags through v2.8.3-nimbus.38 provide the required RSA, ECDSA,
// HMAC, Ed25519, and Ed448 primitives. The official fixture is a broad
// sign/verify matrix, so the harness gives it the same finite slow-fixture
// evidence budget used for the broad WebCrypto wrap/unwrap matrix.

const NDS3_CYCLE91_WEBCRYPTO_SIGN_VERIFY_PATHS: &[&str] =
    &["test/parallel/test-webcrypto-sign-verify.js"];

const NDS3_CYCLE91_WEBCRYPTO_SIGN_VERIFY_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle91_webcrypto_sign_verify_batch() {
    let fixture_paths = NDS3_CYCLE91_WEBCRYPTO_SIGN_VERIFY_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle91-webcrypto-sign-verify-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE91_WEBCRYPTO_SIGN_VERIFY_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle91_webcrypto_sign_verify_batch() {
    let fixture_paths = NDS3_CYCLE91_WEBCRYPTO_SIGN_VERIFY_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle91-webcrypto-sign-verify-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE91_WEBCRYPTO_SIGN_VERIFY_EXTRA_DIRS,
    );
}
