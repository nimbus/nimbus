#[cfg(feature = "js-packages")]
use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 of `bytes`.
#[cfg(feature = "js-packages")]
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
