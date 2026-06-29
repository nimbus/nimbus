//! Content-address and cluster-leg identifier types for the blob store.
//!
//! Blobs are addressed by their BLAKE3 digest over the *stored* bytes (per
//! spec §17 D1, the content address of an encrypted blob is over the framed
//! ciphertext, so the same [`BlobHash`] type is used at every layer).

use std::fmt;

use nimbus_core::{Error, Result};

/// Length of a BLAKE3 digest in bytes.
pub const BLAKE3_HASH_LEN: usize = 32;

/// Content address of a blob: its BLAKE3 digest over the stored bytes.
///
/// A newtype over the raw 32-byte digest rather than `blake3::Hash` so the
/// public seam does not leak the `blake3` crate's type into callers and so the
/// value is trivially `Copy`/`Hash`/`Ord` for use as a map key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlobHash([u8; BLAKE3_HASH_LEN]);

impl BlobHash {
    /// Computes the content address of `bytes` (BLAKE3).
    pub fn of(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }

    /// Wraps an existing 32-byte digest.
    pub fn from_bytes(bytes: [u8; BLAKE3_HASH_LEN]) -> Self {
        Self(bytes)
    }

    /// Borrows the raw digest bytes.
    pub fn as_bytes(&self) -> &[u8; BLAKE3_HASH_LEN] {
        &self.0
    }

    /// Lowercase hex rendering of the digest.
    pub fn to_hex(&self) -> String {
        let mut out = String::with_capacity(BLAKE3_HASH_LEN * 2);
        for byte in self.0 {
            out.push(char::from_digit((byte >> 4) as u32, 16).expect("nibble is < 16"));
            out.push(char::from_digit((byte & 0x0f) as u32, 16).expect("nibble is < 16"));
        }
        out
    }

    /// Parses a 64-char lowercase-or-uppercase hex string into a [`BlobHash`].
    ///
    /// Inverse of [`to_hex`](Self::to_hex). Rejects any string that is not
    /// exactly `BLAKE3_HASH_LEN * 2` ASCII hex digits.
    pub fn from_hex(hex: &str) -> Result<Self> {
        let bytes = hex.as_bytes();
        if bytes.len() != BLAKE3_HASH_LEN * 2 {
            return Err(Error::InvalidInput(format!(
                "blob hash hex must be {} chars, got {}",
                BLAKE3_HASH_LEN * 2,
                bytes.len()
            )));
        }
        let mut out = [0u8; BLAKE3_HASH_LEN];
        for (i, slot) in out.iter_mut().enumerate() {
            let hi = hex_nibble(bytes[i * 2])?;
            let lo = hex_nibble(bytes[i * 2 + 1])?;
            *slot = (hi << 4) | lo;
        }
        Ok(Self(out))
    }
}

/// Decodes a single ASCII hex digit into its 4-bit value.
fn hex_nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        other => Err(Error::InvalidInput(format!(
            "invalid hex digit {:?} in blob hash",
            other as char
        ))),
    }
}

impl fmt::Debug for BlobHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlobHash({})", self.to_hex())
    }
}

impl fmt::Display for BlobHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Cluster-leg fetch ticket announcing where a blob can be retrieved.
///
/// Gated behind the `cluster` feature ([`ReplicatingBlobStore`] seam).
/// The current seam type is an opaque locator. The NOS-A8 cluster leg replaces
/// the locator payload with iroh blob/node addressing once that leg lands.
//
// [`ReplicatingBlobStore`]: crate::ReplicatingBlobStore
#[cfg(feature = "cluster")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobTicket {
    /// Content address the ticket resolves to.
    pub hash: BlobHash,
    /// Opaque locator bytes (iroh ticket encoding in the real impl).
    pub locator: Vec<u8>,
}

#[cfg(feature = "cluster")]
impl BlobTicket {
    /// Builds a ticket for `hash` with an opaque `locator`.
    pub fn new(hash: BlobHash, locator: Vec<u8>) -> Self {
        Self { hash, locator }
    }
}

/// Address of a cluster peer that can serve a [`BlobTicket`].
///
/// Gated behind the `cluster` feature ([`ReplicatingBlobStore`] seam).
/// The current seam type is an opaque peer locator. The NOS-A8 cluster leg maps
/// it to the iroh endpoint identity named by the cluster substrate.
//
// [`ReplicatingBlobStore`]: crate::ReplicatingBlobStore
#[cfg(feature = "cluster")]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PeerAddr(pub String);

#[cfg(feature = "cluster")]
impl PeerAddr {
    /// Wraps a peer locator string.
    pub fn new(addr: impl Into<String>) -> Self {
        Self(addr.into())
    }

    /// Borrows the peer locator.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic_and_distinguishes_inputs() {
        assert_eq!(BlobHash::of(b"hello"), BlobHash::of(b"hello"));
        assert_ne!(BlobHash::of(b"hello"), BlobHash::of(b"world"));
    }

    #[test]
    fn hex_round_trips_through_from_bytes() {
        let hash = BlobHash::of(b"payload");
        let copy = BlobHash::from_bytes(*hash.as_bytes());
        assert_eq!(hash, copy);
        assert_eq!(hash.to_hex().len(), BLAKE3_HASH_LEN * 2);
        assert!(hash.to_hex().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn to_hex_from_hex_round_trips() {
        let hash = BlobHash::of(b"round trip me");
        let hex = hash.to_hex();
        let parsed = BlobHash::from_hex(&hex).expect("valid hex parses");
        assert_eq!(hash, parsed, "from_hex is the exact inverse of to_hex");
    }

    #[test]
    fn from_hex_accepts_uppercase() {
        let hash = BlobHash::of(b"case insensitive");
        let upper = hash.to_hex().to_uppercase();
        assert_eq!(BlobHash::from_hex(&upper).unwrap(), hash);
    }

    #[test]
    fn from_hex_rejects_bad_length() {
        let err = BlobHash::from_hex("abcd").unwrap_err();
        assert!(matches!(err, nimbus_core::Error::InvalidInput(_)));
    }

    #[test]
    fn from_hex_rejects_non_hex_digit() {
        let bad = "g".repeat(BLAKE3_HASH_LEN * 2);
        let err = BlobHash::from_hex(&bad).unwrap_err();
        assert!(matches!(err, nimbus_core::Error::InvalidInput(_)));
    }
}
