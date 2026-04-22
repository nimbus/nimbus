//! Local encryption contracts for Neovex-owned persistence.
//!
//! This module provides the cross-provider key-management model for encrypting
//! local databases and persisted artifacts. The design keeps key-management
//! semantics uniform while allowing provider-specific data encryption.
//!
//! # Architecture
//!
//! - Every protected local database and artifact gets its own random 256-bit DEK
//! - DEKs are wrapped by a configured key provider and stored in sidecar manifests
//! - Manifest metadata is authenticated through AEAD AAD (local) or EncryptionContext (KMS)
//! - KEK rotation rewraps manifests only; DEK rotation is provider-specific

#[cfg(feature = "aws-kms")]
mod aws_kms;
mod key;
mod key_directory;
mod manifest;
mod master_key_file;
mod provider;
mod runtime;
mod subject;

#[cfg(feature = "aws-kms")]
pub use aws_kms::AwsKmsKeyProvider;
pub use key::{GeneratedDatabaseKey, WrappedDatabaseKey};
pub use key_directory::KeyDirectoryProvider;
pub use manifest::{
    KeyManifest, KeyManifestHeader, ManifestCipher, ManifestError, ManifestReadError,
    ManifestWriteError,
};
pub use master_key_file::MasterKeyFileProvider;
pub use provider::{LocalKeyProvider, LocalKeyProviderError};
pub use runtime::{
    generate_database_manifest, resolve_database_encryption_key, unwrap_database_manifest_key,
};
pub use subject::{LocalArtifactRole, LocalDatabaseRole, LocalKeySubject, LocalKeySubjectKind};

#[cfg(test)]
mod tests;
