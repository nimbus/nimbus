// NDS3 cycle-88 WebCrypto EdDSA sign/verify promotion.
//
// Deno fork tag v2.8.3-nimbus.36 adds WebCrypto Ed448 sign/verify support and
// aligns EdDSA wrong-key/wrong-algorithm error messages with Node. The official
// Node22 EdDSA fixture dynamically greened on the published tag.

const NDS3_CYCLE88_WEBCRYPTO_EDDSA_NODE22_PATHS: &[&str] =
    &["test/parallel/test-webcrypto-sign-verify-eddsa.js"];

const NDS3_CYCLE88_WEBCRYPTO_EDDSA_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures",
    "test/fixtures/crypto",
    "test/fixtures/keys",
];

#[test]
fn node22_supported_lane_executes_cycle88_webcrypto_eddsa_batch() {
    let fixture_paths = NDS3_CYCLE88_WEBCRYPTO_EDDSA_NODE22_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle88-webcrypto-eddsa-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE88_WEBCRYPTO_EDDSA_EXTRA_DIRS,
    );
}
