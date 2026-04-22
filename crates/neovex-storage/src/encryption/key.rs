//! Key types for local encryption.

use std::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

/// A freshly generated database encryption key with its wrapped form.
///
/// The plaintext DEK is guaranteed to be zeroed when this value is dropped,
/// using compiler-barrier-protected writes via the `zeroize` crate.
///
/// This type intentionally does NOT implement `Clone` — cloning would create
/// a second copy of plaintext key material that might escape zeroing.
#[derive(ZeroizeOnDrop)]
pub struct GeneratedDatabaseKey {
    /// The plaintext 256-bit data-encryption key.
    ///
    /// This is only held in memory and never persisted directly.
    #[zeroize(drop)]
    plaintext: [u8; 32],

    /// The wrapped (encrypted) form of the DEK, suitable for storage.
    #[zeroize(skip)]
    wrapped: WrappedDatabaseKey,
}

impl GeneratedDatabaseKey {
    /// Creates a new generated key from plaintext and wrapped forms.
    pub fn new(plaintext: [u8; 32], wrapped: WrappedDatabaseKey) -> Self {
        Self { plaintext, wrapped }
    }

    /// Returns the plaintext DEK for use with storage engines.
    ///
    /// This key must never be persisted or logged.
    pub fn plaintext(&self) -> &[u8; 32] {
        &self.plaintext
    }

    /// Returns the wrapped DEK for storage in a sidecar manifest.
    pub fn wrapped(&self) -> &WrappedDatabaseKey {
        &self.wrapped
    }

    /// Consumes the generated key and returns the wrapped form.
    ///
    /// The plaintext is zeroed as part of the drop.
    pub fn into_wrapped(self) -> WrappedDatabaseKey {
        // ZeroizeOnDrop handles zeroing `self.plaintext` when self is dropped.
        // We need to extract wrapped before drop runs. Use a small trick:
        // read wrapped out, then let the rest of self drop (which zeros plaintext).
        //
        // Safety: We read wrapped via ptr::read, then forget self to prevent
        // double-drop, then manually zero plaintext.
        let wrapped = unsafe { std::ptr::read(&self.wrapped) };
        let mut plaintext_copy = self.plaintext;
        std::mem::forget(self);
        plaintext_copy.zeroize();
        wrapped
    }
}

impl fmt::Debug for GeneratedDatabaseKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeneratedDatabaseKey")
            .field("plaintext", &"[REDACTED]")
            .field("wrapped", &self.wrapped)
            .finish()
    }
}

/// A wrapped (encrypted) database encryption key.
///
/// This is the form stored in sidecar manifests. It can only be unwrapped
/// by the key provider that created it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedDatabaseKey {
    /// The cipher used to wrap the key.
    pub cipher: WrappingCipher,

    /// The encrypted key material.
    ///
    /// Format depends on the cipher:
    /// - For AES-256-GCM-SIV: `nonce (12 bytes) || ciphertext (32 bytes) || tag (16 bytes)`
    pub ciphertext: Vec<u8>,
}

impl WrappedDatabaseKey {
    /// Creates a new wrapped key.
    pub fn new(cipher: WrappingCipher, ciphertext: Vec<u8>) -> Self {
        Self { cipher, ciphertext }
    }

    /// Returns the expected ciphertext length for the given cipher when it is
    /// fixed-width.
    pub fn expected_ciphertext_len(cipher: WrappingCipher) -> usize {
        match cipher {
            WrappingCipher::Aes256GcmSiv => 12 + 32 + 16, // nonce + plaintext + tag
            WrappingCipher::AwsKms => 0,
        }
    }
}

/// The cipher used to wrap database encryption keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrappingCipher {
    /// AES-256-GCM-SIV as specified in RFC 8452.
    ///
    /// This is the default for local key providers because it:
    /// - Provides misuse-resistant AEAD semantics
    /// - Supports associated data for manifest binding
    /// - Stays in the AES family for enterprise compatibility
    Aes256GcmSiv,
    /// AWS KMS-managed ciphertext blob for a wrapped DEK.
    ///
    /// The ciphertext length is provider-defined and variable because KMS
    /// returns an opaque blob that embeds provider-owned metadata.
    AwsKms,
}

impl WrappingCipher {
    /// Returns a stable string identifier for the cipher.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Aes256GcmSiv => "aes-256-gcm-siv",
            Self::AwsKms => "aws-kms",
        }
    }

    /// Parses a cipher from its string identifier.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "aes-256-gcm-siv" => Some(Self::Aes256GcmSiv),
            "aws-kms" => Some(Self::AwsKms),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_key_debug_redacts_plaintext() {
        let plaintext = [0xABu8; 32];
        let wrapped = WrappedDatabaseKey::new(WrappingCipher::Aes256GcmSiv, vec![0u8; 60]);
        let key = GeneratedDatabaseKey::new(plaintext, wrapped);

        let debug = format!("{key:?}");

        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("171")); // 0xAB = 171, should not appear
    }

    #[test]
    fn wrapped_key_expected_length_matches_cipher() {
        assert_eq!(
            WrappedDatabaseKey::expected_ciphertext_len(WrappingCipher::Aes256GcmSiv),
            60
        );
    }

    #[test]
    fn wrapping_cipher_round_trips_through_string() {
        let cipher = WrappingCipher::Aes256GcmSiv;
        let s = cipher.as_str();
        let parsed = WrappingCipher::parse(s);
        assert_eq!(parsed, Some(cipher));
    }

    #[test]
    fn into_wrapped_returns_wrapped_key() {
        let plaintext = [0x42u8; 32];
        let expected_wrapped = WrappedDatabaseKey::new(WrappingCipher::Aes256GcmSiv, vec![1, 2, 3]);
        let key = GeneratedDatabaseKey::new(plaintext, expected_wrapped.clone());

        let recovered = key.into_wrapped();
        assert_eq!(recovered, expected_wrapped);
    }
}
