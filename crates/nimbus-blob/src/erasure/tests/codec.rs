use nimbus_core::hex_encode;

use super::stripe;

const NBLE1_PARITY_GOLDEN: &str =
    include_str!("fixtures/nble1-reed-solomon-simd-3.1.0-k3-m2-shard22.txt");

#[test]
fn nble1_parity_bytes_match_golden_vector() {
    const DATA_SHARDS: usize = 3;
    const PARITY_SHARDS: usize = 2;

    // 65 bytes produce three 22-byte data shards after one padding byte.
    // This deliberately covers the non-64-divisible size for which the
    // upstream codec does not promise cross-version compatibility.
    let payload = (0_u8..=64).collect::<Vec<_>>();
    let encoded = stripe::encode_stripe(&payload, DATA_SHARDS, PARITY_SHARDS).unwrap();

    assert_eq!(encoded.len(), DATA_SHARDS + PARITY_SHARDS);
    assert_eq!(encoded[0].len(), 22);
    assert_ne!(encoded[0].len() % 64, 0);

    let actual = encoded[DATA_SHARDS..]
        .iter()
        .map(hex_encode)
        .collect::<Vec<_>>();
    let expected = NBLE1_PARITY_GOLDEN
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect::<Vec<_>>();

    assert_eq!(
        actual, expected,
        "NBLE1 parity bytes changed; publish a deliberate NBLE2 format before changing the codec"
    );
}
