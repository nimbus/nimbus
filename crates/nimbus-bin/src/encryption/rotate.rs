//! Key rotation commands.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Args, ValueEnum};
use rand::RngCore;

use nimbus::{
    AwsKmsConfig, Error, InitializedKeyProvider, KeyDirectoryConfig, KeyManifest,
    LOGICAL_PAGE_SIZE, LocalArtifactRole, LocalKeyProvider, LocalKeyProviderConfig,
    LocalKeySubject, LocalKeySubjectKind, ManifestCipher, MasterKeyFileConfig, PHYSICAL_PAGE_SIZE,
    Result, ServicePersistenceConfig, TenantId, checkpoint_encrypted_database_at_path,
    unwrap_database_manifest_key,
};

use super::migrate::{ProviderFamily, database_subject};

/// Rotate key-encryption keys (rewrap manifests without rewriting data).
#[derive(Debug, Args)]
pub(crate) struct RotateKekCommand {
    /// Path to the database or data directory.
    #[arg(long)]
    path: PathBuf,

    /// Provider family: sqlite, redb, or libsql-cache.
    #[arg(long)]
    provider: Option<ProviderFamily>,

    /// Replacement key provider for the rotated manifests.
    #[arg(long, value_enum)]
    new_key_provider: Option<RotateKeyProvider>,

    /// Path to the new master key file when rotating to `master-key-file`.
    #[arg(long)]
    new_master_key_file: Option<PathBuf>,

    /// Path to the new key directory when rotating to `key-dir`.
    #[arg(long)]
    new_key_dir: Option<PathBuf>,

    /// AWS KMS key ID or alias when rotating to `aws-kms`.
    #[arg(long)]
    new_aws_kms_key_id: Option<String>,

    /// AWS region override when rotating to `aws-kms`.
    #[arg(long)]
    new_aws_region: Option<String>,

    /// AWS endpoint override when rotating to `aws-kms`.
    #[arg(long)]
    new_aws_endpoint_url: Option<String>,

    /// Rotate all manifests in the directory.
    #[arg(long, default_value = "false")]
    all: bool,
}

/// Rotate data-encryption keys (provider-specific, may rewrite data).
#[derive(Debug, Args)]
pub(crate) struct RotateDekCommand {
    /// Path to the encrypted database.
    #[arg(long)]
    path: PathBuf,

    /// Provider family: sqlite, redb, or libsql-cache.
    #[arg(long)]
    provider: ProviderFamily,

    /// Tenant ID for tenant databases.
    #[arg(long)]
    tenant_id: Option<String>,

    /// Skip backup before rotation.
    #[arg(long, default_value = "false")]
    skip_backup: bool,
}

pub(crate) async fn run_rotate_kek_command(
    command: RotateKekCommand,
    config: &ServicePersistenceConfig,
) -> Result<()> {
    if !config.local_encryption.is_enabled() {
        return Err(Error::InvalidInput(
            "encryption must be enabled to rotate keys; configure --encryption-key-provider"
                .to_string(),
        ));
    }

    println!("Rotating key-encryption key (KEK)...");
    println!();

    let current_provider = build_current_provider(config)?;
    let new_provider = build_new_provider(&command)?;

    if command.all {
        println!("Scanning for manifests in: {}", command.path.display());
        rotate_all_manifests(
            &command.path,
            current_provider.as_ref(),
            new_provider.as_ref(),
        )?;
    } else {
        let manifest_path = KeyManifest::manifest_path(&command.path);
        if !manifest_path.exists() {
            return Err(Error::InvalidInput(format!(
                "manifest not found: {}",
                manifest_path.display()
            )));
        }
        rotate_manifest(
            &manifest_path,
            current_provider.as_ref(),
            new_provider.as_ref(),
        )?;
    }

    println!();
    println!("KEK rotation complete.");
    println!();
    println!("NOTE: KEK rotation only rewraps the manifest. Database pages are unchanged.");
    println!("      The new KEK must be used for all subsequent operations.");

    Ok(())
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RotateKeyProvider {
    MasterKeyFile,
    KeyDir,
    AwsKms,
}

pub(crate) async fn run_rotate_dek_command(
    command: RotateDekCommand,
    config: &ServicePersistenceConfig,
) -> Result<()> {
    if !config.local_encryption.is_enabled() {
        return Err(Error::InvalidInput(
            "encryption must be enabled to rotate keys; configure --encryption-key-provider"
                .to_string(),
        ));
    }

    println!("Rotating data-encryption key (DEK)...");
    println!();

    match command.provider {
        ProviderFamily::Sqlite => rotate_sqlite_dek(&command.path, config, &command)?,
        ProviderFamily::Redb => rotate_redb_dek(&command.path, config, &command)?,
        ProviderFamily::LibsqlCache => rotate_libsql_cache_dek(&command.path, config, &command)?,
    }

    Ok(())
}

fn build_current_provider(config: &ServicePersistenceConfig) -> Result<Arc<dyn LocalKeyProvider>> {
    let key_provider_config = config
        .local_encryption
        .key_provider()
        .ok_or_else(|| Error::InvalidInput("encryption key provider not configured".to_string()))?;
    Ok(InitializedKeyProvider::from_config(key_provider_config)?.provider())
}

fn rotate_all_manifests(
    dir: &Path,
    current_provider: &dyn LocalKeyProvider,
    new_provider: &dyn LocalKeyProvider,
) -> Result<()> {
    let mut found_count = 0u32;
    let mut rotated_count = 0u32;

    for entry in std::fs::read_dir(dir)
        .map_err(|e| Error::Internal(format!("failed to read directory: {e}")))?
    {
        let entry =
            entry.map_err(|e| Error::Internal(format!("failed to read directory entry: {e}")))?;
        let path = entry.path();

        if path.extension().map(|e| e == "nimbus-enc").unwrap_or(false) {
            found_count += 1;
            rotate_manifest(&path, current_provider, new_provider)?;
            rotated_count += 1;
        }
    }

    println!();
    println!("Found {found_count} manifests, rotated {rotated_count}.");

    Ok(())
}

fn rotate_manifest(
    manifest_path: &Path,
    current_provider: &dyn LocalKeyProvider,
    new_provider: &dyn LocalKeyProvider,
) -> Result<()> {
    println!("  Rotating: {}", manifest_path.display());

    let manifest = KeyManifest::read(manifest_path)
        .map_err(|e| Error::Internal(format!("failed to read manifest: {e}")))?;
    let subject = subject_from_descriptor(&manifest.header.subject_descriptor)?;

    let mut new_header = manifest.header.clone();
    new_header.rotated_at = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    new_header.key_provider = new_provider.kind();

    let new_wrapped = match new_provider
        .rewrap_wrapped_database_key(
            &subject,
            &manifest.wrapped_key,
            &manifest.header,
            &new_header,
        )
        .map_err(|e| Error::Internal(format!("failed to rewrap DEK: {e}")))?
    {
        Some(wrapped) => wrapped,
        None => {
            let plaintext_dek = current_provider
                .unwrap_database_key(&subject, &manifest.wrapped_key, &manifest.header)
                .map_err(|e| Error::Internal(format!("failed to unwrap DEK: {e}")))?;
            new_provider
                .rewrap_database_key(&subject, &plaintext_dek, &new_header)
                .map_err(|e| Error::Internal(format!("failed to rewrap DEK: {e}")))?
        }
    };

    let new_manifest = KeyManifest {
        header: new_header,
        wrapped_key: new_wrapped,
    };
    new_manifest
        .write(manifest_path)
        .map_err(|e| Error::Internal(format!("failed to write rotated manifest: {e}")))?;

    Ok(())
}

fn build_new_provider(command: &RotateKekCommand) -> Result<Arc<dyn LocalKeyProvider>> {
    let inferred_provider = infer_new_provider(command)?;
    let config = match inferred_provider {
        RotateKeyProvider::MasterKeyFile => {
            let path = command.new_master_key_file.clone().ok_or_else(|| {
                Error::InvalidInput(
                    "--new-master-key-file is required when --new-key-provider=master-key-file"
                        .to_string(),
                )
            })?;
            LocalKeyProviderConfig::MasterKeyFile(MasterKeyFileConfig { path })
        }
        RotateKeyProvider::KeyDir => {
            let path = command.new_key_dir.clone().ok_or_else(|| {
                Error::InvalidInput(
                    "--new-key-dir is required when --new-key-provider=key-dir".to_string(),
                )
            })?;
            LocalKeyProviderConfig::KeyDirectory(KeyDirectoryConfig { path })
        }
        RotateKeyProvider::AwsKms => {
            let key_id = command.new_aws_kms_key_id.clone().ok_or_else(|| {
                Error::InvalidInput(
                    "--new-aws-kms-key-id is required when --new-key-provider=aws-kms".to_string(),
                )
            })?;
            LocalKeyProviderConfig::AwsKms(AwsKmsConfig {
                key_id,
                region: command.new_aws_region.clone(),
                endpoint_url: command.new_aws_endpoint_url.clone(),
            })
        }
    };
    Ok(InitializedKeyProvider::from_config(&config)?.provider())
}

fn infer_new_provider(command: &RotateKekCommand) -> Result<RotateKeyProvider> {
    let provider = if let Some(provider) = command.new_key_provider {
        provider
    } else {
        let mut inferred = Vec::new();
        if command.new_master_key_file.is_some() {
            inferred.push(RotateKeyProvider::MasterKeyFile);
        }
        if command.new_key_dir.is_some() {
            inferred.push(RotateKeyProvider::KeyDir);
        }
        if command.new_aws_kms_key_id.is_some() {
            inferred.push(RotateKeyProvider::AwsKms);
        }

        match inferred.as_slice() {
            [provider] => *provider,
            [] => {
                return Err(Error::InvalidInput(
                    "a replacement provider is required; pass --new-key-provider or one of --new-master-key-file, --new-key-dir, or --new-aws-kms-key-id"
                        .to_string(),
                ));
            }
            _ => {
                return Err(Error::InvalidInput(
                    "multiple replacement provider inputs were supplied; choose exactly one target provider"
                        .to_string(),
                ));
            }
        }
    };

    validate_new_provider_inputs(command, provider)?;
    Ok(provider)
}

fn validate_new_provider_inputs(
    command: &RotateKekCommand,
    provider: RotateKeyProvider,
) -> Result<()> {
    let mut conflicting_flags = Vec::new();
    match provider {
        RotateKeyProvider::MasterKeyFile => {
            if command.new_key_dir.is_some() {
                conflicting_flags.push("--new-key-dir");
            }
            if command.new_aws_kms_key_id.is_some() {
                conflicting_flags.push("--new-aws-kms-key-id");
            }
            if command.new_aws_region.is_some() {
                conflicting_flags.push("--new-aws-region");
            }
            if command.new_aws_endpoint_url.is_some() {
                conflicting_flags.push("--new-aws-endpoint-url");
            }
        }
        RotateKeyProvider::KeyDir => {
            if command.new_master_key_file.is_some() {
                conflicting_flags.push("--new-master-key-file");
            }
            if command.new_aws_kms_key_id.is_some() {
                conflicting_flags.push("--new-aws-kms-key-id");
            }
            if command.new_aws_region.is_some() {
                conflicting_flags.push("--new-aws-region");
            }
            if command.new_aws_endpoint_url.is_some() {
                conflicting_flags.push("--new-aws-endpoint-url");
            }
        }
        RotateKeyProvider::AwsKms => {
            if command.new_master_key_file.is_some() {
                conflicting_flags.push("--new-master-key-file");
            }
            if command.new_key_dir.is_some() {
                conflicting_flags.push("--new-key-dir");
            }
        }
    }

    if conflicting_flags.is_empty() {
        Ok(())
    } else {
        Err(Error::InvalidInput(format!(
            "flags {} do not apply to --new-key-provider={}; choose exactly one replacement provider configuration",
            conflicting_flags.join(", "),
            provider.as_kebab_case(),
        )))
    }
}

impl RotateKeyProvider {
    fn as_kebab_case(self) -> &'static str {
        match self {
            Self::MasterKeyFile => "master-key-file",
            Self::KeyDir => "key-dir",
            Self::AwsKms => "aws-kms",
        }
    }
}

fn rotate_sqlite_dek(
    path: &Path,
    config: &ServicePersistenceConfig,
    command: &RotateDekCommand,
) -> Result<()> {
    println!("Rotating SQLite DEK: {}", path.display());

    if !path.exists() {
        return Err(Error::InvalidInput(format!(
            "database does not exist: {}",
            path.display()
        )));
    }

    let provider = build_current_provider(config)?;
    let subject = database_subject(ProviderFamily::Sqlite, command.tenant_id.as_deref(), path)?;
    let manifest = KeyManifest::read_for(path)
        .map_err(|e| Error::InvalidInput(format!("failed to read encryption manifest: {e}")))?;
    let current_dek = unwrap_database_manifest_key(
        &manifest,
        provider.as_ref(),
        &subject,
        ManifestCipher::SqlCipher,
        path,
    )?;

    checkpoint_encrypted_database_at_path(path, &current_dek)?;

    let backups = if command.skip_backup {
        Vec::new()
    } else {
        backup_sqlite_artifacts(path)?
    };

    let mut new_dek = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut new_dek);

    println!("  Executing PRAGMA rekey...");
    if let Err(error) = nimbus::rekey_encrypted_database_at_path(path, &current_dek, &new_dek) {
        restore_sqlite_artifacts(path, &backups)?;
        return Err(error);
    }

    if let Err(error) =
        write_rotated_manifest(&manifest, provider.as_ref(), &subject, &new_dek, path)
    {
        restore_sqlite_artifacts(path, &backups)?;
        return Err(error);
    }

    println!("  SQLite DEK rotation complete.");
    Ok(())
}

fn rotate_redb_dek(
    path: &Path,
    config: &ServicePersistenceConfig,
    command: &RotateDekCommand,
) -> Result<()> {
    println!("Rotating redb DEK: {}", path.display());

    if !path.exists() {
        return Err(Error::InvalidInput(format!(
            "database does not exist: {}",
            path.display()
        )));
    }

    let provider = build_current_provider(config)?;
    let subject = database_subject(ProviderFamily::Redb, command.tenant_id.as_deref(), path)?;
    let manifest = KeyManifest::read_for(path)
        .map_err(|e| Error::InvalidInput(format!("failed to read encryption manifest: {e}")))?;
    let current_dek = unwrap_database_manifest_key(
        &manifest,
        provider.as_ref(),
        &subject,
        ManifestCipher::RedbAes256GcmSiv,
        path,
    )?;

    let backup_path = if command.skip_backup {
        None
    } else {
        Some(backup_single_file(path)?)
    };

    let mut new_dek = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut new_dek);

    println!("  Re-encrypting pages...");
    let temp_path = append_suffix(path, ".rotating");
    if let Err(error) = reencrypt_redb_pages(path, &temp_path, &current_dek, &new_dek) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }

    std::fs::rename(&temp_path, path)
        .map_err(|e| Error::Internal(format!("failed to replace database: {e}")))?;

    if let Err(error) =
        write_rotated_manifest(&manifest, provider.as_ref(), &subject, &new_dek, path)
    {
        if let Some(backup_path) = &backup_path {
            std::fs::copy(backup_path, path).map_err(|copy_error| {
                Error::Internal(format!(
                    "failed to restore redb backup after manifest write failure: {copy_error}"
                ))
            })?;
        }
        return Err(error);
    }

    println!("  redb DEK rotation complete.");
    Ok(())
}

fn rotate_libsql_cache_dek(
    path: &Path,
    config: &ServicePersistenceConfig,
    command: &RotateDekCommand,
) -> Result<()> {
    println!("Rotating libsql replica cache DEK: {}", path.display());

    let provider = build_current_provider(config)?;
    let subject = database_subject(
        ProviderFamily::LibsqlCache,
        command.tenant_id.as_deref(),
        path,
    )?;
    let manifest = KeyManifest::read_for(path)
        .map_err(|e| Error::InvalidInput(format!("failed to read encryption manifest: {e}")))?;

    if !command.skip_backup {
        let _ = backup_sqlite_artifacts(path)?;
    }

    let mut new_dek = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut new_dek);

    write_rotated_manifest(&manifest, provider.as_ref(), &subject, &new_dek, path)?;
    retire_sqlite_database_artifacts(path)?;

    println!("  libsql replica cache manifest updated.");
    println!("  Remove any running service instance, then restart to rebuild the local cache.");
    Ok(())
}

fn write_rotated_manifest(
    manifest: &KeyManifest,
    provider: &dyn LocalKeyProvider,
    subject: &LocalKeySubject,
    new_dek: &[u8; 32],
    protected_path: &Path,
) -> Result<()> {
    let mut new_header = manifest.header.clone();
    new_header.rotated_at = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    new_header.key_provider = provider.kind();

    let new_wrapped = provider
        .rewrap_database_key(subject, new_dek, &new_header)
        .map_err(|e| Error::Internal(format!("failed to rewrap DEK: {e}")))?;

    let new_manifest = KeyManifest {
        header: new_header,
        wrapped_key: new_wrapped,
    };
    new_manifest
        .write_for(protected_path)
        .map_err(|e| Error::Internal(format!("failed to write rotated manifest: {e}")))?;
    Ok(())
}

fn backup_single_file(path: &Path) -> Result<PathBuf> {
    let backup_path = append_suffix(path, ".bak");
    println!("  Creating backup: {}", backup_path.display());
    std::fs::copy(path, &backup_path)
        .map_err(|e| Error::Internal(format!("failed to create backup: {e}")))?;
    Ok(backup_path)
}

fn backup_sqlite_artifacts(path: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut backups = Vec::new();
    for original in sqlite_artifact_paths(path) {
        if original.exists() {
            let backup = append_suffix(&original, ".bak");
            println!("  Creating backup: {}", backup.display());
            std::fs::copy(&original, &backup).map_err(|e| {
                Error::Internal(format!("failed to create backup {}: {e}", backup.display()))
            })?;
            backups.push((original, backup));
        }
    }
    Ok(backups)
}

fn restore_sqlite_artifacts(path: &Path, backups: &[(PathBuf, PathBuf)]) -> Result<()> {
    let backed_up_paths = backups
        .iter()
        .map(|(original, _)| original.as_path())
        .collect::<std::collections::HashSet<_>>();
    for artifact in sqlite_artifact_paths(path) {
        if !backed_up_paths.contains(artifact.as_path()) && artifact.exists() {
            std::fs::remove_file(&artifact).map_err(|e| {
                Error::Internal(format!(
                    "failed to remove stray artifact {} during restore: {e}",
                    artifact.display()
                ))
            })?;
        }
    }
    for (original, backup) in backups {
        if original.exists() {
            std::fs::remove_file(original).map_err(|e| {
                Error::Internal(format!(
                    "failed to remove damaged artifact {}: {e}",
                    original.display()
                ))
            })?;
        }
        std::fs::copy(backup, original).map_err(|e| {
            Error::Internal(format!(
                "failed to restore backup {}: {e}",
                backup.display()
            ))
        })?;
    }
    Ok(())
}

fn retire_sqlite_database_artifacts(path: &Path) -> Result<()> {
    for artifact in sqlite_artifact_paths(path) {
        if artifact.exists() {
            std::fs::remove_file(&artifact).map_err(|e| {
                Error::Internal(format!(
                    "failed to remove retired SQLite artifact {}: {e}",
                    artifact.display()
                ))
            })?;
        }
    }
    Ok(())
}

fn sqlite_artifact_paths(path: &Path) -> Vec<PathBuf> {
    vec![
        path.to_path_buf(),
        append_suffix(path, "-wal"),
        append_suffix(path, "-shm"),
        append_suffix(path, "-journal"),
    ]
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(suffix);
    PathBuf::from(value)
}

/// Re-encrypts all pages from a source redb file to a target with a new DEK.
fn reencrypt_redb_pages(
    source: &Path,
    target: &Path,
    old_dek: &[u8; 32],
    new_dek: &[u8; 32],
) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};

    use aes_gcm_siv::aead::{Aead, KeyInit, Payload};
    use aes_gcm_siv::{Aes256GcmSiv, Nonce};

    let mut source_file = std::fs::File::open(source)
        .map_err(|e| Error::Internal(format!("failed to open source: {e}")))?;
    let source_len = source_file
        .metadata()
        .map_err(|e| Error::Internal(format!("failed to get source metadata: {e}")))?
        .len();

    if source_len == 0 {
        std::fs::copy(source, target)
            .map_err(|e| Error::Internal(format!("failed to copy empty file: {e}")))?;
        return Ok(());
    }

    let page_count = source_len / PHYSICAL_PAGE_SIZE as u64;
    if source_len % PHYSICAL_PAGE_SIZE as u64 != 0 {
        return Err(Error::Internal(format!(
            "source file size {} is not a multiple of physical page size {}",
            source_len, PHYSICAL_PAGE_SIZE
        )));
    }

    let old_cipher = Aes256GcmSiv::new_from_slice(old_dek)
        .map_err(|e| Error::Internal(format!("failed to create old cipher: {e}")))?;
    let new_cipher = Aes256GcmSiv::new_from_slice(new_dek)
        .map_err(|e| Error::Internal(format!("failed to create new cipher: {e}")))?;

    let mut target_file = std::fs::File::create(target)
        .map_err(|e| Error::Internal(format!("failed to create target: {e}")))?;

    let mut page_buf = vec![0u8; PHYSICAL_PAGE_SIZE];
    for page_idx in 0..page_count {
        let physical_offset = page_idx * PHYSICAL_PAGE_SIZE as u64;
        source_file
            .seek(SeekFrom::Start(physical_offset))
            .map_err(|e| Error::Internal(format!("seek failed: {e}")))?;
        source_file
            .read_exact(&mut page_buf)
            .map_err(|e| Error::Internal(format!("read failed at page {page_idx}: {e}")))?;

        let nonce = Nonce::from_slice(&page_buf[..12]);
        let ciphertext = &page_buf[12..];

        let mut aad = Vec::with_capacity(16);
        aad.extend_from_slice(&1u32.to_be_bytes());
        aad.extend_from_slice(&page_idx.to_be_bytes());
        aad.extend_from_slice(&(LOGICAL_PAGE_SIZE as u32).to_be_bytes());

        let plaintext = old_cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| {
                Error::Internal(format!(
                    "decryption failed at page {page_idx} (wrong key or corrupted data)"
                ))
            })?;

        let mut new_nonce_bytes = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut new_nonce_bytes);
        let new_nonce = Nonce::from_slice(&new_nonce_bytes);

        let new_ciphertext = new_cipher
            .encrypt(
                new_nonce,
                Payload {
                    msg: plaintext.as_slice(),
                    aad: &aad,
                },
            )
            .map_err(|e| Error::Internal(format!("encryption failed at page {page_idx}: {e}")))?;

        target_file
            .write_all(&new_nonce_bytes)
            .map_err(|e| Error::Internal(format!("write nonce failed: {e}")))?;
        target_file
            .write_all(&new_ciphertext)
            .map_err(|e| Error::Internal(format!("write ciphertext failed: {e}")))?;
    }

    target_file
        .sync_all()
        .map_err(|e| Error::Internal(format!("sync failed: {e}")))?;

    println!("  Re-encrypted {page_count} pages.");
    Ok(())
}

/// Reconstructs a `LocalKeySubject` from a manifest descriptor string.
fn subject_from_descriptor(descriptor: &str) -> Result<LocalKeySubject> {
    if let Some(remainder) = descriptor.strip_prefix("db:sqlite:tenant:") {
        let colon_pos = remainder.find(':').ok_or_else(|| {
            Error::InvalidInput(format!("missing logical name in descriptor: {descriptor}"))
        })?;
        let tid = &remainder[..colon_pos];
        let name = &remainder[colon_pos + 1..];
        Ok(LocalKeySubject::sqlite_tenant(
            TenantId::new(tid.to_string())?,
            name.to_string(),
        ))
    } else if let Some(remainder) = descriptor.strip_prefix("db:redb:tenant:") {
        let colon_pos = remainder.find(':').ok_or_else(|| {
            Error::InvalidInput(format!("missing logical name in descriptor: {descriptor}"))
        })?;
        let tid = &remainder[..colon_pos];
        let name = &remainder[colon_pos + 1..];
        Ok(LocalKeySubject::redb_tenant(
            TenantId::new(tid.to_string())?,
            name.to_string(),
        ))
    } else if let Some(remainder) = descriptor.strip_prefix("db:redb:control:") {
        Ok(LocalKeySubject::control_plane(remainder.to_string()))
    } else if let Some(remainder) = descriptor.strip_prefix("db:libsql:cache:") {
        let colon_pos = remainder.find(':').ok_or_else(|| {
            Error::InvalidInput(format!("missing logical name in descriptor: {descriptor}"))
        })?;
        let tid = &remainder[..colon_pos];
        let name = &remainder[colon_pos + 1..];
        Ok(LocalKeySubject::libsql_cache(
            TenantId::new(tid.to_string())?,
            name.to_string(),
        ))
    } else if let Some(remainder) = descriptor.strip_prefix("artifact:migration:") {
        parse_artifact_subject(remainder, LocalArtifactRole::MigrationCopy)
    } else if let Some(remainder) = descriptor.strip_prefix("artifact:rebuild:") {
        parse_artifact_subject(remainder, LocalArtifactRole::RebuildStaging)
    } else if let Some(remainder) = descriptor.strip_prefix("artifact:retired-cache:") {
        parse_artifact_subject(remainder, LocalArtifactRole::RetiredReplicaCache)
    } else if let Some(remainder) = descriptor.strip_prefix("artifact:snapshot:") {
        parse_artifact_subject(remainder, LocalArtifactRole::SnapshotExport)
    } else if let Some(remainder) = descriptor.strip_prefix("artifact:bootstrap:") {
        parse_artifact_subject(remainder, LocalArtifactRole::BootstrapBundle)
    } else {
        Err(Error::InvalidInput(format!(
            "unrecognized subject descriptor: {descriptor}"
        )))
    }
}

fn parse_artifact_subject(remainder: &str, role: LocalArtifactRole) -> Result<LocalKeySubject> {
    if let Some(colon_pos) = remainder.find(':') {
        let tenant_candidate = &remainder[..colon_pos];
        let logical_name = &remainder[colon_pos + 1..];
        let tenant_id = if tenant_candidate.is_empty() {
            None
        } else {
            Some(TenantId::new(tenant_candidate.to_string())?)
        };
        Ok(LocalKeySubject {
            kind: LocalKeySubjectKind::Artifact(role),
            tenant_id,
            logical_name: logical_name.to_string(),
        })
    } else {
        Ok(LocalKeySubject {
            kind: LocalKeySubjectKind::Artifact(role),
            tenant_id: None,
            logical_name: remainder.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_rotate_kek_command() -> RotateKekCommand {
        RotateKekCommand {
            path: PathBuf::from("/tmp/demo.sqlite3"),
            provider: None,
            new_key_provider: None,
            new_master_key_file: None,
            new_key_dir: None,
            new_aws_kms_key_id: None,
            new_aws_region: None,
            new_aws_endpoint_url: None,
            all: false,
        }
    }

    #[test]
    fn infer_new_provider_defaults_to_master_key_file_from_legacy_flag() {
        let mut command = base_rotate_kek_command();
        command.new_master_key_file = Some(PathBuf::from("/secure/new.key"));

        let provider = infer_new_provider(&command).expect("provider should infer");

        assert!(matches!(provider, RotateKeyProvider::MasterKeyFile));
    }

    #[test]
    fn infer_new_provider_detects_aws_kms_inputs() {
        let mut command = base_rotate_kek_command();
        command.new_aws_kms_key_id = Some("alias/nimbus-prod".to_string());
        command.new_aws_region = Some("us-east-1".to_string());
        command.new_aws_endpoint_url = Some("http://localhost:4566".to_string());

        let provider = infer_new_provider(&command).expect("provider should infer");

        assert!(matches!(provider, RotateKeyProvider::AwsKms));
    }

    #[test]
    fn infer_new_provider_rejects_multiple_targets_without_explicit_provider() {
        let mut command = base_rotate_kek_command();
        command.new_master_key_file = Some(PathBuf::from("/secure/new.key"));
        command.new_aws_kms_key_id = Some("alias/nimbus-prod".to_string());

        let error = infer_new_provider(&command).expect_err("provider inference should fail");

        assert!(
            error
                .to_string()
                .contains("multiple replacement provider inputs were supplied")
        );
    }

    #[test]
    fn infer_new_provider_rejects_conflicting_flags_for_explicit_provider() {
        let mut command = base_rotate_kek_command();
        command.new_key_provider = Some(RotateKeyProvider::AwsKms);
        command.new_aws_kms_key_id = Some("alias/nimbus-prod".to_string());
        command.new_master_key_file = Some(PathBuf::from("/secure/old-style.key"));

        let error = infer_new_provider(&command).expect_err("conflicting flags should fail");

        assert!(error.to_string().contains("--new-master-key-file"));
        assert!(error.to_string().contains("--new-key-provider=aws-kms"));
    }

    #[test]
    fn infer_new_provider_rejects_kms_aux_flags_for_non_kms_target() {
        let mut command = base_rotate_kek_command();
        command.new_key_provider = Some(RotateKeyProvider::MasterKeyFile);
        command.new_master_key_file = Some(PathBuf::from("/secure/new.key"));
        command.new_aws_region = Some("us-east-1".to_string());

        let error = infer_new_provider(&command).expect_err("conflicting kms flags should fail");

        assert!(error.to_string().contains("--new-aws-region"));
        assert!(error.to_string().contains("master-key-file"));
    }
}
