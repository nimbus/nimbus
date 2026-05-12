//! Key directory provider implementation.
//!
//! This provider reads per-subject wrapping keys from a directory structure.
//! Each subject has its own 32-byte key file, providing explicit per-subject
//! key isolation for advanced deployments.

use std::fs;
use std::path::PathBuf;

use aes_gcm_siv::aead::{Aead, KeyInit};
use aes_gcm_siv::{Aes256GcmSiv, Nonce};
use rand::RngCore;
use rand::rngs::OsRng;

use super::key::{GeneratedDatabaseKey, WrappedDatabaseKey, WrappingCipher};
use super::manifest::KeyManifestHeader;
use super::provider::{
    KeyProviderKind, KeyProviderResult, LocalKeyProvider, LocalKeyProviderError,
};
use super::subject::LocalKeySubject;

/// Key provider that reads per-subject wrapping keys from a directory.
///
/// # Security Model
///
/// - Each subject has its own key file in the directory
/// - Key files contain exactly 32 bytes of raw wrapping key material
/// - AES-256-GCM-SIV is used to wrap DEKs with manifest metadata as AAD
/// - The key directory should have restrictive permissions (e.g., 0700)
/// - Individual key files should have restrictive permissions (e.g., 0600)
///
/// # File Naming
///
/// Subject key files are named by sanitizing the subject descriptor:
/// - Replace `:` with `_`
/// - Replace `/` with `_`
/// - Append `.key` extension
///
/// For example, `db:sqlite:tenant:demo:demo.sqlite3` becomes
/// `db_sqlite_tenant_demo_demo.sqlite3.key`.
pub struct KeyDirectoryProvider {
    /// Path to the key directory.
    path: PathBuf,
}

impl KeyDirectoryProvider {
    /// Creates a new provider using the given key directory.
    ///
    /// The directory must exist and be readable.
    pub fn new(path: PathBuf) -> KeyProviderResult<Self> {
        // Verify the directory exists and is readable
        fs::read_dir(&path).map_err(|source| LocalKeyProviderError::KeyDirectoryReadError {
            path: path.clone(),
            source,
        })?;

        Ok(Self { path })
    }

    /// Returns the key file path for a given subject.
    pub fn key_file_path(&self, subject: &LocalKeySubject) -> PathBuf {
        let descriptor = subject.descriptor();
        let sanitized = descriptor.replace([':', '/'], "_");
        self.path.join(format!("{sanitized}.key"))
    }

    /// Reads the wrapping key for a subject from its key file.
    fn read_wrapping_key(&self, subject: &LocalKeySubject) -> KeyProviderResult<[u8; 32]> {
        let key_path = self.key_file_path(subject);

        let bytes = fs::read(&key_path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                LocalKeyProviderError::KeyFileNotFound { path: key_path }
            } else {
                LocalKeyProviderError::KeyDirectoryReadError {
                    path: key_path,
                    source,
                }
            }
        })?;

        if bytes.len() != 32 {
            return Err(LocalKeyProviderError::InvalidKeyFileSize {
                path: self.key_file_path(subject),
                actual: bytes.len(),
            });
        }

        let mut wrapping_key = [0u8; 32];
        wrapping_key.copy_from_slice(&bytes);
        Ok(wrapping_key)
    }

    /// Wraps a DEK using the subject's wrapping key and header AAD.
    fn wrap_key(
        &self,
        subject: &LocalKeySubject,
        plaintext: &[u8; 32],
        header: &KeyManifestHeader,
    ) -> KeyProviderResult<WrappedDatabaseKey> {
        let wrapping_key = self.read_wrapping_key(subject)?;
        let cipher = Aes256GcmSiv::new_from_slice(&wrapping_key).map_err(|e| {
            LocalKeyProviderError::WrapError {
                message: format!("failed to create cipher: {e}"),
            }
        })?;

        // Generate a random 12-byte nonce
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Use header AAD for authenticated encryption
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

        // Prepend nonce to ciphertext
        let mut full_ciphertext = nonce_bytes.to_vec();
        full_ciphertext.extend_from_slice(&ciphertext);

        Ok(WrappedDatabaseKey::new(
            WrappingCipher::Aes256GcmSiv,
            full_ciphertext,
        ))
    }

    /// Unwraps a DEK using the subject's wrapping key and header AAD.
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

        let wrapping_key = self.read_wrapping_key(subject)?;
        let cipher = Aes256GcmSiv::new_from_slice(&wrapping_key).map_err(|e| {
            LocalKeyProviderError::UnwrapError {
                message: format!("failed to create cipher: {e}"),
            }
        })?;

        // Extract nonce from the beginning of ciphertext
        let nonce = Nonce::from_slice(&wrapped.ciphertext[..12]);
        let ciphertext = &wrapped.ciphertext[12..];

        // Use header AAD for authenticated decryption
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

impl LocalKeyProvider for KeyDirectoryProvider {
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
        KeyProviderKind::KeyDirectory {
            path: self.path.display().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::encryption::manifest::{MANIFEST_VERSION, ManifestCipher};
    use nimbus_core::TenantId;

    fn write_test_key(dir: &std::path::Path, subject: &LocalKeySubject) -> PathBuf {
        let provider =
            KeyDirectoryProvider::new(dir.to_path_buf()).expect("provider should create");
        let key_path = provider.key_file_path(subject);
        let key = [0x42u8; 32];
        fs::write(&key_path, key).expect("test key should write");
        key_path
    }

    fn test_header(
        subject: &LocalKeySubject,
        provider: &KeyDirectoryProvider,
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
    fn key_file_path_sanitizes_descriptor() {
        let dir = tempdir().expect("tempdir should create");
        let provider =
            KeyDirectoryProvider::new(dir.path().to_path_buf()).expect("provider should create");

        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        let subject = LocalKeySubject::sqlite_tenant(tenant_id, "demo.sqlite3");

        let key_path = provider.key_file_path(&subject);
        let filename = key_path.file_name().unwrap().to_str().unwrap();

        // Should not contain colons
        assert!(!filename.contains(':'));
        assert!(filename.ends_with(".key"));
    }

    #[test]
    fn provider_rejects_missing_directory() {
        let result = KeyDirectoryProvider::new(PathBuf::from("/nonexistent/path"));
        assert!(matches!(
            result,
            Err(LocalKeyProviderError::KeyDirectoryReadError { .. })
        ));
    }

    #[test]
    fn provider_rejects_wrong_key_size() {
        let dir = tempdir().expect("tempdir should create");
        let tenant_id = TenantId::new("test").expect("tenant id should build");
        let subject = LocalKeySubject::sqlite_tenant(tenant_id, "test.sqlite3");

        // Write a key file with wrong size
        let provider =
            KeyDirectoryProvider::new(dir.path().to_path_buf()).expect("provider should create");
        let key_path = provider.key_file_path(&subject);
        fs::write(&key_path, [0u8; 16]).expect("bad key should write");

        let header = test_header(&subject, &provider);
        let result = provider.generate_database_key(&subject, &header);
        assert!(matches!(
            result,
            Err(LocalKeyProviderError::InvalidKeyFileSize { actual: 16, .. })
        ));
    }

    #[test]
    fn provider_rejects_missing_key_file() {
        let dir = tempdir().expect("tempdir should create");
        let provider =
            KeyDirectoryProvider::new(dir.path().to_path_buf()).expect("provider should create");

        let tenant_id = TenantId::new("missing").expect("tenant id should build");
        let subject = LocalKeySubject::sqlite_tenant(tenant_id, "missing.sqlite3");

        let header = test_header(&subject, &provider);
        let result = provider.generate_database_key(&subject, &header);
        assert!(matches!(
            result,
            Err(LocalKeyProviderError::KeyFileNotFound { .. })
        ));
    }

    #[test]
    fn provider_generates_and_unwraps_key() {
        let dir = tempdir().expect("tempdir should create");
        let tenant_id = TenantId::new("test").expect("tenant id should build");
        let subject = LocalKeySubject::sqlite_tenant(tenant_id, "test.sqlite3");
        write_test_key(dir.path(), &subject);

        let provider =
            KeyDirectoryProvider::new(dir.path().to_path_buf()).expect("provider should create");
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
    fn different_subjects_need_different_key_files() {
        let dir = tempdir().expect("tempdir should create");
        let tenant1 = TenantId::new("tenant1").expect("tenant id should build");
        let tenant2 = TenantId::new("tenant2").expect("tenant id should build");
        let subject1 = LocalKeySubject::sqlite_tenant(tenant1, "tenant1.sqlite3");
        let subject2 = LocalKeySubject::sqlite_tenant(tenant2, "tenant2.sqlite3");

        // Only write key for subject1
        write_test_key(dir.path(), &subject1);

        let provider =
            KeyDirectoryProvider::new(dir.path().to_path_buf()).expect("provider should create");
        let header1 = test_header(&subject1, &provider);
        let header2 = test_header(&subject2, &provider);

        // subject1 should work
        let result1 = provider.generate_database_key(&subject1, &header1);
        assert!(result1.is_ok());

        // subject2 should fail (no key file)
        let result2 = provider.generate_database_key(&subject2, &header2);
        assert!(matches!(
            result2,
            Err(LocalKeyProviderError::KeyFileNotFound { .. })
        ));
    }

    #[test]
    fn rewrap_produces_valid_wrapped_key() {
        let dir = tempdir().expect("tempdir should create");
        let tenant_id = TenantId::new("test").expect("tenant id should build");
        let subject = LocalKeySubject::sqlite_tenant(tenant_id, "test.sqlite3");
        write_test_key(dir.path(), &subject);

        let provider =
            KeyDirectoryProvider::new(dir.path().to_path_buf()).expect("provider should create");
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
    fn wrong_key_file_fails_unwrap() {
        let dir = tempdir().expect("tempdir should create");
        let tenant_id = TenantId::new("test").expect("tenant id should build");
        let subject = LocalKeySubject::sqlite_tenant(tenant_id, "test.sqlite3");
        write_test_key(dir.path(), &subject);

        let provider =
            KeyDirectoryProvider::new(dir.path().to_path_buf()).expect("provider should create");
        let header = test_header(&subject, &provider);

        // Generate a key
        let generated = provider
            .generate_database_key(&subject, &header)
            .expect("key should generate");

        // Overwrite the key file with different content
        let key_path = provider.key_file_path(&subject);
        fs::write(&key_path, [0xABu8; 32]).expect("new key should write");

        // Unwrap should fail
        let result = provider.unwrap_database_key(&subject, generated.wrapped(), &header);
        assert!(matches!(
            result,
            Err(LocalKeyProviderError::UnwrapError { .. })
        ));
    }
}
