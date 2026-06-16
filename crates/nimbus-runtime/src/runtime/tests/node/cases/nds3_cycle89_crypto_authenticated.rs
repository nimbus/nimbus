// NDS3 cycle-89 authenticated crypto promotion.
//
// Deno fork tag v2.8.3-nimbus.37 adds AES-CCM authenticated cipher support and
// aligns authenticated-cipher error metadata / DataView input handling with
// Node. The official authenticated-crypto fixture dynamically greened on the
// published tag in both required lanes.

const NDS3_CYCLE89_CRYPTO_AUTHENTICATED_PATHS: &[&str] =
    &["test/parallel/test-crypto-authenticated.js"];

const NDS3_CYCLE89_CRYPTO_AUTHENTICATED_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures"];

#[test]
fn node22_supported_lane_executes_cycle89_crypto_authenticated_batch() {
    let fixture_paths = NDS3_CYCLE89_CRYPTO_AUTHENTICATED_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle89-crypto-authenticated-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE89_CRYPTO_AUTHENTICATED_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle89_crypto_authenticated_batch() {
    let fixture_paths = NDS3_CYCLE89_CRYPTO_AUTHENTICATED_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle89-crypto-authenticated-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE89_CRYPTO_AUTHENTICATED_EXTRA_DIRS,
    );
}
