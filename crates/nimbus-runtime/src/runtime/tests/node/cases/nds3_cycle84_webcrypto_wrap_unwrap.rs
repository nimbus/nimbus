// NDS3 cycle-84 WebCrypto wrapKey/unwrapKey parity promotion.
//
// Deno fork tag v2.8.3-nimbus.33 aligns Node's wrap/unwrap key validation
// messages, EC exportKey format/type errors, and AES-KW JWK padding behavior
// enough to dynamically green the official wrap/unwrap fixture in both
// required lanes. The broad fixture matrix uses the fixture-specific finite
// slow budget configured in the harness.

const NDS3_CYCLE84_WEBCRYPTO_WRAP_UNWRAP_PATHS: &[&str] =
    &["test/parallel/test-webcrypto-wrap-unwrap.js"];

const NDS3_CYCLE84_WEBCRYPTO_WRAP_UNWRAP_NODE22_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures/crypto", "test/fixtures/keys"];

const NDS3_CYCLE84_WEBCRYPTO_WRAP_UNWRAP_NODE24_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures/crypto",
    "test/fixtures/keys",
    "test/fixtures/webcrypto",
];

#[test]
fn node22_supported_lane_executes_cycle84_webcrypto_wrap_unwrap_batch() {
    let fixture_paths = NDS3_CYCLE84_WEBCRYPTO_WRAP_UNWRAP_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle84-webcrypto-wrap-unwrap-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE84_WEBCRYPTO_WRAP_UNWRAP_NODE22_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle84_webcrypto_wrap_unwrap_batch() {
    let fixture_paths = NDS3_CYCLE84_WEBCRYPTO_WRAP_UNWRAP_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle84-webcrypto-wrap-unwrap-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE84_WEBCRYPTO_WRAP_UNWRAP_NODE24_EXTRA_DIRS,
    );
}
