// NDS3 cycle-82 WebCrypto HKDF deriveBits/deriveKey parity promotion.
//
// Deno fork tag v2.8.3-nimbus.31 aligns HKDF zero-length deriveBits(), HKDF
// missing-option error codes, KDF length error text, deriveBits/deriveKey
// key-usage and key-algorithm mismatch text, and AES-OCB derived-key length
// registration enough to dynamically green both required lanes.

const NDS3_CYCLE82_WEBCRYPTO_HKDF_DERIVEBITS_PATHS: &[&str] =
    &["test/parallel/test-webcrypto-derivebits-hkdf.js"];

const NDS3_CYCLE82_WEBCRYPTO_HKDF_DERIVEBITS_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle82_webcrypto_hkdf_derivebits_batch() {
    let fixture_paths = NDS3_CYCLE82_WEBCRYPTO_HKDF_DERIVEBITS_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle82-webcrypto-hkdf-derivebits-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE82_WEBCRYPTO_HKDF_DERIVEBITS_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle82_webcrypto_hkdf_derivebits_batch() {
    let fixture_paths = NDS3_CYCLE82_WEBCRYPTO_HKDF_DERIVEBITS_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle82-webcrypto-hkdf-derivebits-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE82_WEBCRYPTO_HKDF_DERIVEBITS_EXTRA_DIRS,
    );
}
