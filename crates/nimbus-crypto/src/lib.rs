//! Local encryption contracts for Nimbus-owned persistence.
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
mod framed;
mod key;
mod key_directory;
mod manifest;
mod master_key_file;
mod materials;
mod provider;
mod rotation;
mod runtime;
mod signing;
mod subject;

#[cfg(feature = "aws-kms")]
pub use aws_kms::AwsKmsKeyProvider;
pub use framed::{
    Aes256GcmSivFramedAead, FRAME_PLAINTEXT_LEN, FRAMED_HEADER_LEN, FramedAead, FramedAeadSecurity,
    FramedAlgorithmSuite, FramedBlobHeader, FramedBlobKey, FramedBlobSeed, FramedOpenSession,
    FramedSealSession, FramedSeedKind, KEY_SEED_LEN, MAX_PLAINTEXT_BYTES, MAX_WRAPPED_DATA_KEYS,
    NONCE_LEN, framed_span_for_plaintext_range, open_framed_blob, open_framed_blob_range,
    open_framed_span, random_framed_salt, seal_framed_blob,
};
pub use key::{DataEncryptionKey, GeneratedDataKey, WrappedDataKey, WrappingCipher};
pub use key_directory::KeyDirectoryProvider;
pub use manifest::{
    KeyManifest, KeyManifestHeader, ManifestCipher, ManifestError, ManifestReadError,
    ManifestWriteError,
};
pub use master_key_file::MasterKeyFileProvider;
pub use materials::{
    AlgorithmSuite, CommitmentMetadata, CryptoMaterials, CryptoShredError, CryptoShredOutcome,
    CryptoShredRegistry, DekTemplate, EnvelopeKeyring, KeyringTrace, KeyringTraceEvent,
    ProviderFailureKind, ProviderFamily, ProviderIdentity, RevocableDataKey, crypto_shred_subject,
    shred_tombstone_path,
};
pub use provider::{LocalKeyProvider, LocalKeyProviderError};
pub use rotation::{
    commit_staged_dek_rotation, dek_rotation_data_stage_path, dek_rotation_manifest_stage_path,
    recover_interrupted_dek_rotation,
};
pub use runtime::{generate_key_manifest, resolve_subject_encryption_key, unwrap_key_manifest};
pub use signing::{
    FileBackedIdentitySigner, IdentityPublicKey, IdentitySignature, IdentitySigner,
    IdentitySignerKind, OpenMode, SigningError, SigningResult,
};
pub use subject::{LocalArtifactRole, LocalDatabaseRole, LocalKeySubject, LocalKeySubjectKind};

#[cfg(test)]
mod tests;
