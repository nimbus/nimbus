// NDS3 cycle-85 WebCrypto AES encrypt/decrypt Node24 parity promotion.
//
// Deno fork tag v2.8.3-nimbus.34 aligns Node24's AES encrypt/decrypt
// DOMException names/messages and AES-GCM nonce handling enough to dynamically
// green the official Node24 AES encrypt/decrypt fixture. Node22's AES
// encrypt/decrypt fixture was already covered by the older supported-lane
// WebCrypto encrypt/decrypt batch in watchpoints_extended.rs.

const NDS3_CYCLE85_WEBCRYPTO_AES_ENCRYPT_DECRYPT_PATHS: &[&str] =
    &["test/parallel/test-webcrypto-encrypt-decrypt-aes.js"];

const NDS3_CYCLE85_WEBCRYPTO_AES_ENCRYPT_DECRYPT_NODE24_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures",
    "test/fixtures/crypto",
    "test/fixtures/keys",
    "test/fixtures/webcrypto",
];

#[test]
fn node24_default_lane_executes_cycle85_webcrypto_aes_encrypt_decrypt_batch() {
    let fixture_paths = NDS3_CYCLE85_WEBCRYPTO_AES_ENCRYPT_DECRYPT_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle85-webcrypto-aes-encrypt-decrypt-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE85_WEBCRYPTO_AES_ENCRYPT_DECRYPT_NODE24_EXTRA_DIRS,
    );
}
