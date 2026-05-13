use super::*;

#[test]
fn cli_defaults_to_encryption_disabled() {
    let cli = parse_start(["nimbus", "start"]);
    let config = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("default config should build");
    assert!(!config.local_encryption.is_enabled());
}

#[test]
fn cli_builds_master_key_file_encryption_config() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--encryption-key-provider",
        "master-key-file",
        "--encryption-master-key-file",
        "/secure/nimbus.key",
    ]);
    let config = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("master-key-file encryption config should build");
    assert!(config.local_encryption.is_enabled());
    let descriptor = config.local_encryption.descriptor();
    assert!(matches!(
        descriptor,
        nimbus::EncryptionConfigDescriptor::Enabled(
            nimbus::KeyProviderDescriptor::MasterKeyFile { .. }
        )
    ));
}

#[test]
fn cli_builds_key_dir_encryption_config() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--encryption-key-provider",
        "key-dir",
        "--encryption-key-dir",
        "/secure/keys",
    ]);
    let config = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("key-dir encryption config should build");
    assert!(config.local_encryption.is_enabled());
    let descriptor = config.local_encryption.descriptor();
    assert!(matches!(
        descriptor,
        nimbus::EncryptionConfigDescriptor::Enabled(
            nimbus::KeyProviderDescriptor::KeyDirectory { .. }
        )
    ));
}

#[test]
fn cli_builds_aws_kms_encryption_config() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--encryption-key-provider",
        "aws-kms",
        "--encryption-aws-kms-key-id",
        "arn:aws:kms:us-east-1:123456789:key/example-key-id",
        "--encryption-aws-region",
        "us-east-1",
    ]);
    let config = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("aws-kms encryption config should build");
    assert!(config.local_encryption.is_enabled());
    let descriptor = config.local_encryption.descriptor();
    assert!(matches!(
        descriptor,
        nimbus::EncryptionConfigDescriptor::Enabled(nimbus::KeyProviderDescriptor::AwsKms { .. })
    ));
}

#[test]
fn cli_rejects_orphaned_encryption_options() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--encryption-master-key-file",
        "/secure/nimbus.key",
    ]);
    let result = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    );
    assert!(result.is_err());
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("require"),
        "error should mention requirement: {error}"
    );
}

#[test]
fn cli_rejects_mismatched_encryption_provider_options() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--encryption-key-provider",
        "master-key-file",
        "--encryption-master-key-file",
        "/secure/nimbus.key",
        "--encryption-aws-kms-key-id",
        "arn:aws:kms:us-east-1:123456789:key/example-key-id",
    ]);
    let result = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    );
    assert!(result.is_err());
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("aws-kms"),
        "error should mention aws-kms: {error}"
    );
}

#[test]
fn cli_requires_master_key_file_path() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--encryption-key-provider",
        "master-key-file",
    ]);
    let result = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    );
    assert!(result.is_err());
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("encryption-master-key-file"),
        "error should mention missing file: {error}"
    );
}

#[test]
fn cli_requires_aws_kms_key_id() {
    let cli = parse_start(["nimbus", "start", "--encryption-key-provider", "aws-kms"]);
    let result = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    );
    assert!(result.is_err());
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("aws-kms-key-id"),
        "error should mention missing key id: {error}"
    );
}

// -------------------------------------------------------------------------
// License path resolution tests
// -------------------------------------------------------------------------
