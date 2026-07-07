//! Shared byte-encoding helpers.
//!
//! Content-addressed hashes and digests get rendered as hex in several
//! crates (blob content addresses, engine consistency digests). This module
//! is the single place that owns the lowercase-hex encoding so those crates
//! do not each hand-roll the same nibble loop.
//!
//! CO10: base64 audit — convex, crypto, firebase, mongodb, and proxy each
//! instantiated their own `base64::engine::general_purpose::STANDARD` or
//! `URL_SAFE_NO_PAD` engine to encode/decode the same two RFC 4648 variants.
//! This module owns both so those crates call a named function instead of
//! re-selecting an engine. `nimbus-runtime` (zero workspace deps) and the
//! `nimbus-artifacts` secret-detection heuristics (character classification,
//! not codec use) are unrelated and stay outside this module.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};

/// Encodes `bytes` as a lowercase hex string.
pub fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    hex::encode(bytes)
}

/// Encodes `bytes` with the padded, standard-alphabet base64 variant (RFC 4648 §4).
pub fn base64_encode_standard(bytes: impl AsRef<[u8]>) -> String {
    STANDARD.encode(bytes)
}

/// Decodes a padded, standard-alphabet base64 string (RFC 4648 §4).
pub fn base64_decode_standard(value: impl AsRef<[u8]>) -> Result<Vec<u8>, base64::DecodeError> {
    STANDARD.decode(value)
}

/// Encodes `bytes` with the unpadded, URL-safe base64 variant (RFC 4648 §5) used
/// for opaque tokens embedded in URLs (JWT segments, pagination tokens, nonces).
pub fn base64_encode_url_safe_no_pad(bytes: impl AsRef<[u8]>) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Decodes an unpadded, URL-safe base64 string (RFC 4648 §5).
pub fn base64_decode_url_safe_no_pad(
    value: impl AsRef<[u8]>,
) -> Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD.decode(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_encode_renders_lowercase_nibbles() {
        assert_eq!(hex_encode([0x0f, 0xa0, 0xff]), "0fa0ff");
    }

    #[test]
    fn hex_encode_handles_empty_input() {
        assert_eq!(hex_encode([]), "");
    }

    #[test]
    fn base64_standard_round_trips_and_pads() {
        let encoded = base64_encode_standard([1_u8, 2, 3]);
        assert_eq!(encoded, "AQID");
        assert_eq!(base64_decode_standard(&encoded).unwrap(), vec![1, 2, 3]);

        // One byte pads to two `=` under the standard variant.
        assert_eq!(base64_encode_standard([0xffu8]), "/w==");
    }

    #[test]
    fn base64_standard_rejects_invalid_input() {
        assert!(base64_decode_standard("!not-base64!").is_err());
    }

    #[test]
    fn base64_url_safe_no_pad_round_trips_without_padding() {
        // 0xFF encodes to a `-`-bearing char under the url-safe alphabet and
        // carries no trailing `=` padding.
        let encoded = base64_encode_url_safe_no_pad([0xffu8]);
        assert_eq!(encoded, "_w");
        assert_eq!(base64_decode_url_safe_no_pad(&encoded).unwrap(), vec![0xff]);
    }

    #[test]
    fn base64_url_safe_no_pad_rejects_standard_padding() {
        // A padded standard-alphabet string must not decode as url-safe/no-pad.
        assert!(base64_decode_url_safe_no_pad("/w==").is_err());
    }
}
