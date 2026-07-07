//! Shared byte-encoding helpers.
//!
//! Content-addressed hashes and digests get rendered as hex in several
//! crates (blob content addresses, engine consistency digests). This module
//! is the single place that owns the lowercase-hex encoding so those crates
//! do not each hand-roll the same nibble loop.

/// Encodes `bytes` as a lowercase hex string.
pub fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    hex::encode(bytes)
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
}
