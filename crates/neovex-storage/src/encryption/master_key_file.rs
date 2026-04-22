//! Master key file provider implementation.
//!
//! This provider reads a 32-byte master key from a file and uses HKDF to derive
//! per-subject wrapping keys. The wrapped DEKs are encrypted using AES-256-GCM-SIV.

use std::fs;
use std::path::PathBuf;

use aes_gcm_siv::aead::{Aead, KeyInit};
use aes_gcm_siv::{Aes256GcmSiv, Nonce};
use hkdf::Hkdf;
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::key::{GeneratedDatabaseKey, WrappedDatabaseKey, WrappingCipher};
use super::manifest::KeyManifestHeader;
use super::provider::{
    KeyProviderKind, KeyProviderResult, LocalKeyProvider, LocalKeyProviderError,
};
use super::subject::LocalKeySubject;

/// Key provider that derives per-subject keys from a master key file.
///
/// # Security Model
///
/// - The master key file contains exactly 32 bytes of key material
/// - HKDF-SHA256 is used to derive per-subject wrapping keys
/// - AES-256-GCM-SIV is used to wrap DEKs with manifest metadata as AAD
/// - The master key file should be stored outside the data directory
/// - The master key file should have restrictive permissions (e.g., 0400)
/// - The master key is zeroed from memory on drop via the `zeroize` crate
#[derive(ZeroizeOnDrop)]
pub struct MasterKeyFileProvider {
    /// Path to the master key file.
    #[zeroize(skip)]
    path: PathBuf,

    /// The loaded master key (32 bytes). Zeroed on drop.
    master_key: [u8; 32],
}

impl MasterKeyFileProvider {
    /// Creates a new provider by loading the master key from the given path.
    pub fn new(path: PathBuf) -> KeyProviderResult<Self> {
        let bytes =
            fs::read(&path).map_err(|source| LocalKeyProviderError::MasterKeyReadError {
                path: path.clone(),
                source,
            })?;

        if bytes.len() != 32 {
            return Err(LocalKeyProviderError::InvalidMasterKeySize {
                path,
                actual: bytes.len(),
            });
        }

        let mut master_key = [0u8; 32];
        master_key.copy_from_slice(&bytes);

        // Zero the intermediate Vec
        let mut bytes = bytes;
        bytes.zeroize();

        Ok(Self { path, master_key })
    }

    /// Derives a per-subject wrapping key using HKDF-SHA256.
    ///
    /// The derivation context includes the subject's kind tag, tenant ID, and
    /// logical name, ensuring each subject gets a unique wrapping key.
    fn derive_wrapping_key(&self, subject: &LocalKeySubject) -> [u8; 32] {
        let hkdf = Hkdf::<Sha256>::new(None, &self.master_key);
        let info = subject.derivation_context();
        let mut wrapping_key = [0u8; 32];
        hkdf.expand(&info, &mut wrapping_key)
            .expect("32 bytes is a valid HKDF output length");
        wrapping_key
    }

    /// Wraps a DEK using the per-subject wrapping key and header AAD.
    fn wrap_key(
        &self,
        subject: &LocalKeySubject,
        plaintext: &[u8; 32],
        header: &KeyManifestHeader,
    ) -> KeyProviderResult<WrappedDatabaseKey> {
        let mut wrapping_key = self.derive_wrapping_key(subject);
        let cipher = Aes256GcmSiv::new_from_slice(&wrapping_key).map_err(|e| {
            wrapping_key.zeroize();
            LocalKeyProviderError::WrapError {
                message: format!("failed to create cipher: {e}"),
            }
        })?;
        wrapping_key.zeroize();

        // Generate a random 12-byte nonce from OS entropy
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Use header AAD for authenticated encryption — this binds the wrapped
        // DEK to the manifest metadata, preventing substitution attacks.
        let aad = header.to_aad();

        let ciphertext = cipher
            .encrypt(
                nonce,
                aes_gcm_siv::aead::Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|e| LocalKeyProviderError::WrapError {
                message: format!("encryption failed: {e}"),
            })?;

        // Prepend nonce to ciphertext: nonce || ciphertext || tag
        let mut full_ciphertext = nonce_bytes.to_vec();
        full_ciphertext.extend_from_slice(&ciphertext);

        Ok(WrappedDatabaseKey::new(
            WrappingCipher::Aes256GcmSiv,
            full_ciphertext,
        ))
    }

    /// Unwraps a DEK using the per-subject wrapping key and header AAD.
    fn unwrap_key(
        &self,
        subject: &LocalKeySubject,
        wrapped: &WrappedDatabaseKey,
        header: &KeyManifestHeader,
    ) -> KeyProviderResult<[u8; 32]> {
        if wrapped.cipher != WrappingCipher::Aes256GcmSiv {
            return Err(LocalKeyProviderError::UnsupportedCipher {
                cipher: format!("{:?}", wrapped.cipher),
            });
        }

        let expected_len =
            WrappedDatabaseKey::expected_ciphertext_len(WrappingCipher::Aes256GcmSiv);
        if wrapped.ciphertext.len() != expected_len {
            return Err(LocalKeyProviderError::UnwrapError {
                message: format!(
                    "invalid ciphertext length: expected {expected_len}, got {}",
                    wrapped.ciphertext.len()
                ),
            });
        }

        let mut wrapping_key = self.derive_wrapping_key(subject);
        let cipher = Aes256GcmSiv::new_from_slice(&wrapping_key).map_err(|e| {
            wrapping_key.zeroize();
            LocalKeyProviderError::UnwrapError {
                message: format!("failed to create cipher: {e}"),
            }
        })?;
        wrapping_key.zeroize();

        // Extract nonce from the beginning of ciphertext
        let nonce = Nonce::from_slice(&wrapped.ciphertext[..12]);
        let ciphertext = &wrapped.ciphertext[12..];

        // Use header AAD for authenticated decryption — this verifies that
        // the manifest metadata hasn't been tampered with.
        let aad = header.to_aad();

        let plaintext = cipher
            .decrypt(
                nonce,
                aes_gcm_siv::aead::Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| LocalKeyProviderError::UnwrapError {
                message: "decryption failed (wrong key or corrupted ciphertext)".to_string(),
            })?;

        if plaintext.len() != 32 {
            return Err(LocalKeyProviderError::UnwrapError {
                message: format!(
                    "decrypted key has wrong length: expected 32, got {}",
                    plaintext.len()
                ),
            });
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(&plaintext);
        Ok(key)
    }
}

impl LocalKeyProvider for MasterKeyFileProvider {
    fn generate_database_key(
        &self,
        subject: &LocalKeySubject,
        header: &KeyManifestHeader,
    ) -> KeyProviderResult<GeneratedDatabaseKey> {
        // Generate a random 256-bit DEK from OS entropy
        let mut plaintext = [0u8; 32];
        OsRng.fill_bytes(&mut plaintext);

        let wrapped = self.wrap_key(subject, &plaintext, header)?;

        Ok(GeneratedDatabaseKey::new(plaintext, wrapped))
    }

    fn unwrap_database_key(
        &self,
        subject: &LocalKeySubject,
        wrapped: &WrappedDatabaseKey,
        header: &KeyManifestHeader,
    ) -> KeyProviderResult<[u8; 32]> {
        self.unwrap_key(subject, wrapped, header)
    }

    fn rewrap_database_key(
        &self,
        subject: &LocalKeySubject,
        plaintext: &[u8; 32],
        header: &KeyManifestHeader,
    ) -> KeyProviderResult<WrappedDatabaseKey> {
        self.wrap_key(subject, plaintext, header)
    }

    fn kind(&self) -> KeyProviderKind {
        KeyProviderKind::MasterKeyFile {
            path: self.path.display().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::encryption::manifest::{MANIFEST_VERSION, ManifestCipher};
    use neovex_core::TenantId;

    fn write_test_key(path: &std::path::Path) {
        let key = [0x42u8; 32];
        fs::write(path, key).expect("test key should write");
    }

    fn test_header(
        subject: &LocalKeySubject,
        provider: &MasterKeyFileProvider,
    ) -> KeyManifestHeader {
        KeyManifestHeader {
            version: MANIFEST_VERSION,
            cipher: ManifestCipher::SqlCipher,
            subject_descriptor: subject.descriptor(),
            key_provider: provider.kind(),
            created_at: 1000,
            rotated_at: 1000,
        }
    }

    #[test]
    fn provider_rejects_wrong_key_size() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("bad.key");
        fs::write(&path, [0u8; 16]).expect("bad key should write");

        let result = MasterKeyFileProvider::new(path);
        assert!(matches!(
            result,
            Err(LocalKeyProviderError::InvalidMasterKeySize { actual: 16, .. })
        ));
    }

    #[test]
    fn provider_generates_and_unwraps_key() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("master.key");
        write_test_key(&path);

        let provider = MasterKeyFileProvider::new(path).expect("provider should create");
        let tenant_id = TenantId::new("test").expect("tenant id should build");
        let subject = LocalKeySubject::sqlite_tenant(tenant_id, "test.sqlite3");
        let header = test_header(&subject, &provider);

        let generated = provider
            .generate_database_key(&subject, &header)
            .expect("key should generate");

        let unwrapped = provider
            .unwrap_database_key(&subject, generated.wrapped(), &header)
            .expect("key should unwrap");

        assert_eq!(generated.plaintext(), &unwrapped);
    }

    #[test]
    fn different_subjects_get_different_wrapped_keys() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("master.key");
        write_test_key(&path);

        let provider = MasterKeyFileProvider::new(path).expect("provider should create");
        let tenant1 = TenantId::new("tenant1").expect("tenant id should build");
        let tenant2 = TenantId::new("tenant2").expect("tenant id should build");
        let subject1 = LocalKeySubject::sqlite_tenant(tenant1, "tenant1.sqlite3");
        let subject2 = LocalKeySubject::sqlite_tenant(tenant2, "tenant2.sqlite3");
        let header1 = test_header(&subject1, &provider);
        let header2 = test_header(&subject2, &provider);

        let key1 = provider
            .generate_database_key(&subject1, &header1)
            .expect("key1 should generate");
        let key2 = provider
            .generate_database_key(&subject2, &header2)
            .expect("key2 should generate");

        // Wrapped keys should be different (different nonces and different wrapping keys)
        assert_ne!(key1.wrapped().ciphertext, key2.wrapped().ciphertext);
    }

    #[test]
    fn rewrap_produces_valid_wrapped_key() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("master.key");
        write_test_key(&path);

        let provider = MasterKeyFileProvider::new(path).expect("provider should create");
        let tenant_id = TenantId::new("test").expect("tenant id should build");
        let subject = LocalKeySubject::sqlite_tenant(tenant_id, "test.sqlite3");
        let header = test_header(&subject, &provider);

        // Generate a key and extract the plaintext
        let generated = provider
            .generate_database_key(&subject, &header)
            .expect("key should generate");
        let plaintext = *generated.plaintext();

        // Rewrap the same plaintext
        let rewrapped = provider
            .rewrap_database_key(&subject, &plaintext, &header)
            .expect("rewrap should succeed");

        // Unwrap and verify
        let unwrapped = provider
            .unwrap_database_key(&subject, &rewrapped, &header)
            .expect("unwrap should succeed");

        assert_eq!(plaintext, unwrapped);
    }

    #[test]
    fn tampered_header_rejects_unwrap() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("master.key");
        write_test_key(&path);

        let provider = MasterKeyFileProvider::new(path).expect("provider should create");
        let tenant_id = TenantId::new("test").expect("tenant id should build");
        let subject = LocalKeySubject::sqlite_tenant(tenant_id, "test.sqlite3");
        let header = test_header(&subject, &provider);

        let generated = provider
            .generate_database_key(&subject, &header)
            .expect("key should generate");

        // Tamper with the header (change timestamp)
        let tampered_header = KeyManifestHeader {
            rotated_at: 9999,
            ..header
        };

        // Unwrap with tampered header should fail (AAD mismatch)
        let result = provider.unwrap_database_key(&subject, generated.wrapped(), &tampered_header);
        assert!(result.is_err());
    }
}
