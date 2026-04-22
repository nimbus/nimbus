//! Key provider trait for local encryption.

use std::fmt;
use std::path::PathBuf;

use super::key::{GeneratedDatabaseKey, WrappedDatabaseKey};
use super::manifest::KeyManifestHeader;
use super::subject::LocalKeySubject;

/// Errors that can occur during key provider operations.
#[derive(Debug)]
pub enum LocalKeyProviderError {
    /// The master key file could not be read.
    MasterKeyReadError {
        path: PathBuf,
        source: std::io::Error,
    },

    /// The master key file has an invalid size.
    InvalidMasterKeySize { path: PathBuf, actual: usize },

    /// The key directory could not be read.
    KeyDirectoryReadError {
        path: PathBuf,
        source: std::io::Error,
    },

    /// A subject-specific key file could not be found.
    KeyFileNotFound { path: PathBuf },

    /// A subject-specific key file has an invalid size.
    InvalidKeyFileSize { path: PathBuf, actual: usize },

    /// Key unwrapping failed (wrong key or corrupted ciphertext).
    UnwrapError { message: String },

    /// Key wrapping failed.
    WrapError { message: String },

    /// The wrapped key has an unsupported cipher.
    UnsupportedCipher { cipher: String },

    /// Random number generation failed.
    RandomError { message: String },

    /// AWS KMS authentication failed.
    AwsKmsAuthError { message: String },

    /// AWS KMS denied access to the requested operation.
    AwsKmsPermissionDenied {
        operation: &'static str,
        message: String,
    },

    /// The configured AWS KMS key could not be found.
    AwsKmsKeyNotFound { key_id: String },

    /// AWS KMS configuration is invalid.
    AwsKmsConfigurationError { message: String },

    /// AWS KMS could not be reached.
    AwsKmsNetworkError {
        operation: &'static str,
        message: String,
    },

    /// AWS KMS returned an unexpected service error.
    AwsKmsOperationError {
        operation: &'static str,
        message: String,
    },
}

impl fmt::Display for LocalKeyProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MasterKeyReadError { path, source } => {
                write!(
                    f,
                    "failed to read master key from {}: {source}",
                    path.display()
                )
            }
            Self::InvalidMasterKeySize { path, actual } => {
                write!(
                    f,
                    "master key at {} has invalid size {actual} (expected 32 bytes)",
                    path.display()
                )
            }
            Self::KeyDirectoryReadError { path, source } => {
                write!(
                    f,
                    "failed to read key directory {}: {source}",
                    path.display()
                )
            }
            Self::KeyFileNotFound { path } => {
                write!(f, "key file not found at {}", path.display())
            }
            Self::InvalidKeyFileSize { path, actual } => {
                write!(
                    f,
                    "key file at {} has invalid size {actual} (expected 32 bytes)",
                    path.display()
                )
            }
            Self::UnwrapError { message } => {
                write!(f, "key unwrap failed: {message}")
            }
            Self::WrapError { message } => {
                write!(f, "key wrap failed: {message}")
            }
            Self::UnsupportedCipher { cipher } => {
                write!(f, "unsupported wrapping cipher: {cipher}")
            }
            Self::RandomError { message } => {
                write!(f, "random number generation failed: {message}")
            }
            Self::AwsKmsAuthError { message } => {
                write!(f, "aws kms authentication failed: {message}")
            }
            Self::AwsKmsPermissionDenied { operation, message } => {
                write!(f, "aws kms denied {operation}: {message}")
            }
            Self::AwsKmsKeyNotFound { key_id } => {
                write!(f, "aws kms key not found: {key_id}")
            }
            Self::AwsKmsConfigurationError { message } => {
                write!(f, "aws kms configuration error: {message}")
            }
            Self::AwsKmsNetworkError { operation, message } => {
                write!(f, "aws kms network error during {operation}: {message}")
            }
            Self::AwsKmsOperationError { operation, message } => {
                write!(f, "aws kms {operation} failed: {message}")
            }
        }
    }
}

impl std::error::Error for LocalKeyProviderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MasterKeyReadError { source, .. } => Some(source),
            Self::KeyDirectoryReadError { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// A diagnostics-safe descriptor for a key provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyProviderKind {
    MasterKeyFile {
        path: String,
    },
    KeyDirectory {
        path: String,
    },
    AwsKms {
        key_id: String,
        region: Option<String>,
    },
}

impl fmt::Display for KeyProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MasterKeyFile { path } => write!(f, "master-key-file:{path}"),
            Self::KeyDirectory { path } => write!(f, "key-dir:{path}"),
            Self::AwsKms { key_id, region } => {
                write!(f, "aws-kms:{key_id}")?;
                if let Some(region) = region {
                    write!(f, " (region={region})")?;
                }
                Ok(())
            }
        }
    }
}

/// Result type for key provider operations.
pub type KeyProviderResult<T> = Result<T, LocalKeyProviderError>;

/// Trait for local key providers.
///
/// A key provider manages the wrapping and unwrapping of per-subject data
/// encryption keys. It does not directly encrypt database pages; it provides
/// the DEKs that storage engines use.
///
/// # AAD Binding
///
/// All wrap/unwrap operations accept a `KeyManifestHeader` which is serialized
/// as Associated Authenticated Data (AAD). This cryptographically binds the
/// wrapped DEK to the manifest metadata: any tampering with the manifest
/// (cipher, subject, timestamps, provider descriptor) will cause unwrap to
/// fail with an authentication error.
pub trait LocalKeyProvider: Send + Sync + 'static {
    /// Generates a new random DEK for the given subject and returns both
    /// the plaintext key and its wrapped form.
    ///
    /// The `header` is used as AAD during wrapping to bind the DEK to its
    /// manifest metadata.
    fn generate_database_key(
        &self,
        subject: &LocalKeySubject,
        header: &KeyManifestHeader,
    ) -> KeyProviderResult<GeneratedDatabaseKey>;

    /// Unwraps a previously wrapped DEK for the given subject.
    ///
    /// Returns the plaintext DEK for use with storage engines.
    /// The `header` must match the one used during wrapping (AAD verification).
    fn unwrap_database_key(
        &self,
        subject: &LocalKeySubject,
        wrapped: &WrappedDatabaseKey,
        header: &KeyManifestHeader,
    ) -> KeyProviderResult<[u8; 32]>;

    /// Re-wraps a DEK under this provider's key.
    ///
    /// This is used during KEK rotation to update sidecar manifests without
    /// rewriting database pages. The `header` is the new manifest header that
    /// will be written after rewrapping.
    fn rewrap_database_key(
        &self,
        subject: &LocalKeySubject,
        plaintext: &[u8; 32],
        header: &KeyManifestHeader,
    ) -> KeyProviderResult<WrappedDatabaseKey>;

    /// Attempts a provider-native rewrap of an existing wrapped DEK.
    ///
    /// Providers that can rotate a wrapped DEK without exposing plaintext can
    /// override this hook and return `Some(wrapped)`. Providers that do not
    /// support this optimization should return `Ok(None)` and let callers fall
    /// back to unwrap-then-rewrap behavior.
    fn rewrap_wrapped_database_key(
        &self,
        _subject: &LocalKeySubject,
        _wrapped: &WrappedDatabaseKey,
        _current_header: &KeyManifestHeader,
        _new_header: &KeyManifestHeader,
    ) -> KeyProviderResult<Option<WrappedDatabaseKey>> {
        Ok(None)
    }

    /// Returns a diagnostics-safe descriptor for this provider.
    fn kind(&self) -> KeyProviderKind;
}
