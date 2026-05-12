//! Cross-module encryption tests.
//!
//! These tests verify end-to-end encryption workflows across multiple
//! modules in the encryption subsystem.

use tempfile::tempdir;

use super::*;
use nimbus_core::TenantId;

/// Helper to create a test header for a given subject and provider.
fn make_header(
    subject: &LocalKeySubject,
    provider: &dyn provider::LocalKeyProvider,
    cipher: ManifestCipher,
) -> manifest::KeyManifestHeader {
    manifest::KeyManifestHeader {
        version: manifest::MANIFEST_VERSION,
        cipher,
        subject_descriptor: subject.descriptor(),
        key_provider: provider.kind(),
        created_at: 1000,
        rotated_at: 1000,
    }
}

/// Tests that manifests can round-trip through write and read.
#[test]
fn manifest_file_round_trip() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("test.sqlite3.nimbus-enc");

    let original = KeyManifest::new(
        ManifestCipher::SqlCipher,
        "db:sqlite:tenant:demo:demo.sqlite3".to_string(),
        provider::KeyProviderKind::MasterKeyFile {
            path: "/secure/master.key".to_string(),
        },
        key::WrappedDatabaseKey::new(
            key::WrappingCipher::Aes256GcmSiv,
            vec![1, 2, 3, 4, 5, 6, 7, 8],
        ),
    );

    original
        .write(&manifest_path)
        .expect("write should succeed");
    let loaded = KeyManifest::read(&manifest_path).expect("read should succeed");

    assert_eq!(loaded.header.cipher, original.header.cipher);
    assert_eq!(
        loaded.header.subject_descriptor,
        original.header.subject_descriptor
    );
    assert_eq!(loaded.wrapped_key, original.wrapped_key);
}

/// Tests that manifest writes create the sidecar parent directory when needed.
#[test]
fn manifest_write_creates_missing_parent_directory() {
    let dir = tempdir().expect("tempdir should create");
    let protected_path = dir.path().join("nested").join("tenant.sqlite3");
    let manifest = KeyManifest::new(
        ManifestCipher::SqlCipher,
        "db:sqlite:tenant:demo:tenant.sqlite3".to_string(),
        provider::KeyProviderKind::MasterKeyFile {
            path: "/secure/master.key".to_string(),
        },
        key::WrappedDatabaseKey::new(
            key::WrappingCipher::Aes256GcmSiv,
            vec![9, 8, 7, 6, 5, 4, 3, 2],
        ),
    );

    manifest
        .write_for(&protected_path)
        .expect("write should succeed even when the protected path parent does not exist");

    let manifest_path = KeyManifest::manifest_path(&protected_path);
    assert!(manifest_path.exists(), "manifest should be created");
    assert!(
        protected_path
            .parent()
            .is_some_and(|parent| parent.exists()),
        "manifest write should create the sidecar parent directory"
    );
}

/// Tests that MasterKeyFileProvider can generate keys and create valid manifests.
#[test]
fn master_key_file_provider_manifest_integration() {
    let dir = tempdir().expect("tempdir should create");

    // Create master key file
    let key_path = dir.path().join("master.key");
    std::fs::write(&key_path, [0x42u8; 32]).expect("key should write");

    let provider = MasterKeyFileProvider::new(key_path.clone()).expect("provider should create");

    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let subject = LocalKeySubject::sqlite_tenant(tenant_id, "demo.sqlite3");

    // Build the manifest header first — the same header must be used for
    // both wrapping (generate) and unwrapping (read-back).
    let manifest_header = manifest::KeyManifestHeader {
        version: manifest::MANIFEST_VERSION,
        cipher: ManifestCipher::SqlCipher,
        subject_descriptor: subject.descriptor(),
        key_provider: provider.kind(),
        created_at: 1000,
        rotated_at: 1000,
    };

    // Generate a key using this header for AAD binding
    let generated = provider
        .generate_database_key(&subject, &manifest_header)
        .expect("key should generate");

    // Store the plaintext before consuming into_wrapped
    let expected_plaintext = *generated.plaintext();

    // Build the manifest using the same header
    let manifest = KeyManifest {
        header: manifest_header.clone(),
        wrapped_key: generated.into_wrapped(),
    };

    // Write and read the manifest
    let protected_path = dir.path().join("demo.sqlite3");
    manifest
        .write_for(&protected_path)
        .expect("write should succeed");

    let loaded = KeyManifest::read_for(&protected_path).expect("read should succeed");

    // Unwrap the key using the provider — the loaded header must match
    let unwrapped = provider
        .unwrap_database_key(&subject, &loaded.wrapped_key, &loaded.header)
        .expect("unwrap should succeed");

    assert_eq!(loaded.header.cipher, ManifestCipher::SqlCipher);
    assert_eq!(loaded.header.subject_descriptor, subject.descriptor());
    assert_eq!(unwrapped, expected_plaintext);
}

/// Tests that KeyDirectoryProvider can generate keys and create valid manifests.
#[test]
fn key_directory_provider_manifest_integration() {
    let dir = tempdir().expect("tempdir should create");

    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let subject = LocalKeySubject::sqlite_tenant(tenant_id, "demo.sqlite3");

    // Create the provider first so we can use key_file_path
    let provider =
        KeyDirectoryProvider::new(dir.path().to_path_buf()).expect("provider should create");

    // Write the subject's key file
    let key_file_path = provider.key_file_path(&subject);
    std::fs::write(&key_file_path, [0x42u8; 32]).expect("key should write");

    // Build the manifest header first for consistent AAD binding
    let manifest_header = manifest::KeyManifestHeader {
        version: manifest::MANIFEST_VERSION,
        cipher: ManifestCipher::SqlCipher,
        subject_descriptor: subject.descriptor(),
        key_provider: provider.kind(),
        created_at: 1000,
        rotated_at: 1000,
    };

    // Generate a key using this header
    let generated = provider
        .generate_database_key(&subject, &manifest_header)
        .expect("key should generate");

    let expected_plaintext = *generated.plaintext();

    // Build the manifest with the same header
    let manifest = KeyManifest {
        header: manifest_header.clone(),
        wrapped_key: generated.into_wrapped(),
    };

    // Write and read the manifest
    let protected_path = dir.path().join("demo.sqlite3");
    manifest
        .write_for(&protected_path)
        .expect("write should succeed");

    let loaded = KeyManifest::read_for(&protected_path).expect("read should succeed");

    // Unwrap the key using the provider
    let unwrapped = provider
        .unwrap_database_key(&subject, &loaded.wrapped_key, &loaded.header)
        .expect("unwrap should succeed");

    assert_eq!(loaded.header.cipher, ManifestCipher::SqlCipher);
    assert_eq!(loaded.header.subject_descriptor, subject.descriptor());
    assert_eq!(unwrapped, expected_plaintext);
}

/// Tests that providers generate unique DEKs for each call.
#[test]
fn providers_generate_unique_deks() {
    let dir = tempdir().expect("tempdir should create");

    // Create master key file
    let key_path = dir.path().join("master.key");
    std::fs::write(&key_path, [0x42u8; 32]).expect("key should write");

    let provider = MasterKeyFileProvider::new(key_path).expect("provider should create");

    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let subject = LocalKeySubject::sqlite_tenant(tenant_id, "demo.sqlite3");
    let header = make_header(&subject, &provider, ManifestCipher::SqlCipher);

    // Generate multiple keys for the same subject
    let key1 = provider
        .generate_database_key(&subject, &header)
        .expect("key1 should generate");
    let key2 = provider
        .generate_database_key(&subject, &header)
        .expect("key2 should generate");

    // Plaintext keys should be different (random)
    assert_ne!(key1.plaintext(), key2.plaintext());

    // Wrapped keys should also be different (different plaintext + random nonce)
    assert_ne!(key1.wrapped().ciphertext, key2.wrapped().ciphertext);
}

/// Tests that different subjects derive different wrapping keys (for MasterKeyFileProvider).
#[test]
fn master_key_derives_different_wrapping_keys_per_subject() {
    let dir = tempdir().expect("tempdir should create");

    // Create master key file
    let key_path = dir.path().join("master.key");
    std::fs::write(&key_path, [0x42u8; 32]).expect("key should write");

    let provider = MasterKeyFileProvider::new(key_path).expect("provider should create");

    let tenant1 = TenantId::new("tenant1").expect("tenant id should build");
    let tenant2 = TenantId::new("tenant2").expect("tenant id should build");
    let subject1 = LocalKeySubject::sqlite_tenant(tenant1, "tenant1.sqlite3");
    let subject2 = LocalKeySubject::sqlite_tenant(tenant2, "tenant2.sqlite3");
    let header1 = make_header(&subject1, &provider, ManifestCipher::SqlCipher);
    let header2 = make_header(&subject2, &provider, ManifestCipher::SqlCipher);

    // Generate keys for both subjects
    let key1 = provider
        .generate_database_key(&subject1, &header1)
        .expect("key1 should generate");
    let key2 = provider
        .generate_database_key(&subject2, &header2)
        .expect("key2 should generate");

    // Cross-subject unwrap should fail (different HKDF derivation path)
    let result = provider.unwrap_database_key(&subject2, key1.wrapped(), &header1);
    assert!(result.is_err());

    let result = provider.unwrap_database_key(&subject1, key2.wrapped(), &header2);
    assert!(result.is_err());
}

/// Tests that control plane subjects work correctly.
#[test]
fn control_plane_subject_encryption() {
    let dir = tempdir().expect("tempdir should create");

    // Create master key file
    let key_path = dir.path().join("master.key");
    std::fs::write(&key_path, [0x42u8; 32]).expect("key should write");

    let provider = MasterKeyFileProvider::new(key_path).expect("provider should create");

    let subject = LocalKeySubject::control_plane("nimbus-control.db");
    let header = make_header(&subject, &provider, ManifestCipher::RedbAes256GcmSiv);

    // Generate a key
    let generated = provider
        .generate_database_key(&subject, &header)
        .expect("key should generate");

    // Create a manifest (using redb cipher for control plane)
    let manifest = KeyManifest::new(
        ManifestCipher::RedbAes256GcmSiv,
        subject.descriptor(),
        provider.kind(),
        generated.into_wrapped(),
    );

    let summary = manifest.summary();
    assert_eq!(summary.cipher, ManifestCipher::RedbAes256GcmSiv);
    assert!(summary.subject_descriptor.contains("control"));
}

/// Tests that artifact subjects work correctly.
#[test]
fn artifact_subject_encryption() {
    let dir = tempdir().expect("tempdir should create");

    // Create master key file
    let key_path = dir.path().join("master.key");
    std::fs::write(&key_path, [0x42u8; 32]).expect("key should write");

    let provider = MasterKeyFileProvider::new(key_path).expect("provider should create");

    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let subject = LocalKeySubject::migration_copy(Some(tenant_id), "demo.sqlite3.migration");
    let header = make_header(&subject, &provider, ManifestCipher::SqlCipher);

    // Generate a key
    let generated = provider
        .generate_database_key(&subject, &header)
        .expect("key should generate");

    // Unwrap should work
    let unwrapped = provider
        .unwrap_database_key(&subject, generated.wrapped(), &header)
        .expect("unwrap should succeed");

    assert_eq!(generated.plaintext(), &unwrapped);
}

/// Tests that manifest tampering is detected.
#[test]
fn manifest_tampering_detected() {
    let dir = tempdir().expect("tempdir should create");

    // Create master key file
    let key_path = dir.path().join("master.key");
    std::fs::write(&key_path, [0x42u8; 32]).expect("key should write");

    let provider = MasterKeyFileProvider::new(key_path).expect("provider should create");

    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let subject = LocalKeySubject::sqlite_tenant(tenant_id, "demo.sqlite3");
    let header = make_header(&subject, &provider, ManifestCipher::SqlCipher);

    // Generate a key
    let generated = provider
        .generate_database_key(&subject, &header)
        .expect("key should generate");

    // Create a manifest
    let manifest = KeyManifest::new(
        ManifestCipher::SqlCipher,
        subject.descriptor(),
        provider.kind(),
        generated.into_wrapped(),
    );

    // Write the manifest
    let protected_path = dir.path().join("demo.sqlite3");
    manifest
        .write_for(&protected_path)
        .expect("write should succeed");

    // Tamper with the manifest by modifying the subject descriptor
    let tampered_subject = LocalKeySubject::sqlite_tenant(
        TenantId::new("attacker").expect("tenant id should build"),
        "attacker.sqlite3",
    );

    // Try to unwrap with the tampered subject - should fail due to HKDF derivation mismatch
    let loaded = KeyManifest::read_for(&protected_path).expect("read should succeed");
    let result =
        provider.unwrap_database_key(&tampered_subject, &loaded.wrapped_key, &loaded.header);
    assert!(result.is_err());
}

/// Tests libsql cache subject type.
#[test]
fn libsql_cache_subject_encryption() {
    let dir = tempdir().expect("tempdir should create");

    // Create master key file
    let key_path = dir.path().join("master.key");
    std::fs::write(&key_path, [0x42u8; 32]).expect("key should write");

    let provider = MasterKeyFileProvider::new(key_path).expect("provider should create");

    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let subject = LocalKeySubject::libsql_cache(tenant_id, "demo.db");
    let header = make_header(&subject, &provider, ManifestCipher::LibsqlAes256Cbc);

    // Generate a key
    let generated = provider
        .generate_database_key(&subject, &header)
        .expect("key should generate");

    // Create manifest with libsql cipher
    let manifest = KeyManifest::new(
        ManifestCipher::LibsqlAes256Cbc,
        subject.descriptor(),
        provider.kind(),
        generated.into_wrapped(),
    );

    let summary = manifest.summary();
    assert_eq!(summary.cipher, ManifestCipher::LibsqlAes256Cbc);
    assert!(summary.subject_descriptor.contains("libsql"));
}
