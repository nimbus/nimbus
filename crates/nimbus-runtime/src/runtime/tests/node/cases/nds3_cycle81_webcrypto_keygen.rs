// NDS3 cycle-81 WebCrypto generateKey parity promotion.
//
// Deno fork tag v2.8.3-nimbus.30 aligns WebCrypto generateKey() RSA/AES
// validation, Ed448 key generation, and node:crypto utility parity enough to
// dynamically green the node22 fixture. The node24 copy still reaches the KMAC
// native-provider gap and is intentionally not promoted here.

const NDS3_CYCLE81_WEBCRYPTO_KEYGEN_NODE22_PATHS: &[&str] =
    &["test/parallel/test-webcrypto-keygen.js"];

const NDS3_CYCLE81_WEBCRYPTO_KEYGEN_NODE22_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle81_webcrypto_keygen_batch() {
    let fixture_paths = NDS3_CYCLE81_WEBCRYPTO_KEYGEN_NODE22_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle81-webcrypto-keygen-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE81_WEBCRYPTO_KEYGEN_NODE22_EXTRA_DIRS,
    );
}
