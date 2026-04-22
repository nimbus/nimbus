//! Migration commands for encryption.

use std::path::{Path, PathBuf};

use clap::Args;
use neovex::{
    Error, InitializedKeyProvider, KeyManifest, LOGICAL_PAGE_SIZE, LocalKeySubject, ManifestCipher,
    PHYSICAL_PAGE_SIZE, Result, ServicePersistenceConfig, TenantId, generate_database_manifest,
    migrate_encrypted_to_plaintext, migrate_plaintext_to_encrypted, unwrap_database_manifest_key,
};

/// Migrate plaintext databases to encrypted.
#[derive(Debug, Args)]
pub(crate) struct MigrateCommand {
    /// Path to the plaintext database to migrate.
    #[arg(long)]
    source: PathBuf,

    /// Path to the encrypted output database. Defaults to source path with .encrypted suffix.
    #[arg(long)]
    target: Option<PathBuf>,

    /// Provider family: sqlite, redb, or libsql-cache.
    #[arg(long)]
    provider: ProviderFamily,

    /// Tenant ID for tenant databases.
    #[arg(long)]
    tenant_id: Option<String>,

    /// Skip validation after migration.
    #[arg(long, default_value = "false")]
    skip_validation: bool,

    /// Remove source after successful migration.
    #[arg(long, default_value = "false")]
    retire_source: bool,
}

/// Export encrypted databases to plaintext for recovery.
#[derive(Debug, Args)]
pub(crate) struct ExportCommand {
    /// Path to the encrypted database to export.
    #[arg(long)]
    source: PathBuf,

    /// Path to the plaintext output database.
    #[arg(long)]
    target: PathBuf,

    /// Provider family: sqlite, redb, or libsql-cache.
    #[arg(long)]
    provider: ProviderFamily,

    /// Tenant ID for tenant databases.
    #[arg(long)]
    tenant_id: Option<String>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum ProviderFamily {
    Sqlite,
    Redb,
    LibsqlCache,
}

pub(crate) async fn run_migrate_command(
    command: MigrateCommand,
    config: &ServicePersistenceConfig,
) -> Result<()> {
    if !config.local_encryption.is_enabled() {
        return Err(Error::InvalidInput(
            "encryption must be enabled to migrate databases; configure --encryption-key-provider"
                .to_string(),
        ));
    }

    let target = command.target.clone().unwrap_or_else(|| {
        let mut target = command.source.clone();
        let ext = target
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();
        target.set_extension(format!("{ext}.encrypted"));
        target
    });

    match command.provider {
        ProviderFamily::Sqlite => migrate_sqlite(&command.source, &target, config, &command)?,
        ProviderFamily::Redb => migrate_redb(&command.source, &target, config, &command)?,
        ProviderFamily::LibsqlCache => {
            return Err(Error::InvalidInput(
                "libsql cache migration requires a running service; restart with encryption enabled so the cache can rebuild under the manifest-backed key"
                    .to_string(),
            ));
        }
    }

    if command.retire_source {
        println!("Retiring source: {}", command.source.display());
        retire_plaintext_artifact(&command.source)?;
    }

    Ok(())
}

pub(crate) async fn run_export_command(
    command: ExportCommand,
    config: &ServicePersistenceConfig,
) -> Result<()> {
    if !config.local_encryption.is_enabled() {
        return Err(Error::InvalidInput(
            "encryption must be enabled to export encrypted databases; configure --encryption-key-provider"
                .to_string(),
        ));
    }

    match command.provider {
        ProviderFamily::Sqlite => {
            export_sqlite(&command.source, &command.target, config, &command)?
        }
        ProviderFamily::Redb => export_redb(&command.source, &command.target, config, &command)?,
        ProviderFamily::LibsqlCache => {
            return Err(Error::InvalidInput(
                "libsql cache export is not supported; caches can be rebuilt from the remote primary"
                    .to_string(),
            ));
        }
    }

    Ok(())
}

fn migrate_sqlite(
    source: &Path,
    target: &Path,
    config: &ServicePersistenceConfig,
    command: &MigrateCommand,
) -> Result<()> {
    println!("Migrating SQLite database to encrypted format...");
    println!("  Source: {}", source.display());
    println!("  Target: {}", target.display());

    verify_source_exists(source)?;
    verify_target_absent(target)?;

    let key_provider = initialized_provider(config)?;
    let subject = database_subject(ProviderFamily::Sqlite, command.tenant_id.as_deref(), target)?;
    let (manifest, generated) =
        generate_database_manifest(key_provider.as_ref(), &subject, ManifestCipher::SqlCipher)?;

    let staging_target = staging_path(target, "encrypting")?;
    migrate_plaintext_to_encrypted(
        source,
        &staging_target,
        generated.plaintext(),
        !command.skip_validation,
    )?;

    publish_encrypted_target(&staging_target, target, &manifest)?;

    println!("Migration complete: {}", target.display());
    if !command.skip_validation {
        println!("Validation passed.");
    }
    Ok(())
}

fn migrate_redb(
    source: &Path,
    target: &Path,
    config: &ServicePersistenceConfig,
    command: &MigrateCommand,
) -> Result<()> {
    println!("Migrating redb database to encrypted format...");
    println!("  Source: {}", source.display());
    println!("  Target: {}", target.display());

    verify_source_exists(source)?;
    verify_target_absent(target)?;

    let key_provider = initialized_provider(config)?;
    let subject = database_subject(ProviderFamily::Redb, command.tenant_id.as_deref(), target)?;
    let (manifest, generated) = generate_database_manifest(
        key_provider.as_ref(),
        &subject,
        ManifestCipher::RedbAes256GcmSiv,
    )?;

    let staging_target = staging_path(target, "encrypting")?;
    encrypt_plaintext_redb(source, &staging_target, generated.plaintext())?;

    publish_encrypted_target(&staging_target, target, &manifest)?;

    println!("Migration complete: {}", target.display());
    Ok(())
}

fn export_sqlite(
    source: &Path,
    target: &Path,
    config: &ServicePersistenceConfig,
    command: &ExportCommand,
) -> Result<()> {
    println!("Exporting encrypted SQLite database to plaintext...");
    println!("  Source: {}", source.display());
    println!("  Target: {}", target.display());

    verify_source_exists(source)?;
    verify_target_absent(target)?;

    let key_provider = initialized_provider(config)?;
    let subject = database_subject(ProviderFamily::Sqlite, command.tenant_id.as_deref(), source)?;
    let manifest = KeyManifest::read_for(source).map_err(|error| {
        Error::InvalidInput(format!("failed to read encryption manifest: {error}"))
    })?;
    let dek = unwrap_database_manifest_key(
        &manifest,
        key_provider.as_ref(),
        &subject,
        ManifestCipher::SqlCipher,
        source,
    )?;

    let staging_target = staging_path(target, "decrypting")?;
    migrate_encrypted_to_plaintext(source, &staging_target, &dek)?;
    publish_plaintext_target(&staging_target, target)?;

    println!("Export complete: {}", target.display());
    Ok(())
}

fn export_redb(
    source: &Path,
    target: &Path,
    config: &ServicePersistenceConfig,
    command: &ExportCommand,
) -> Result<()> {
    println!("Exporting encrypted redb database to plaintext...");
    println!("  Source: {}", source.display());
    println!("  Target: {}", target.display());

    verify_source_exists(source)?;
    verify_target_absent(target)?;

    let key_provider = initialized_provider(config)?;
    let subject = database_subject(ProviderFamily::Redb, command.tenant_id.as_deref(), source)?;
    let manifest = KeyManifest::read_for(source).map_err(|error| {
        Error::InvalidInput(format!("failed to read encryption manifest: {error}"))
    })?;
    let dek = unwrap_database_manifest_key(
        &manifest,
        key_provider.as_ref(),
        &subject,
        ManifestCipher::RedbAes256GcmSiv,
        source,
    )?;

    let staging_target = staging_path(target, "decrypting")?;
    decrypt_redb_to_plaintext(source, &staging_target, &dek)?;
    publish_plaintext_target(&staging_target, target)?;

    println!("Export complete: {}", target.display());
    Ok(())
}

fn initialized_provider(
    config: &ServicePersistenceConfig,
) -> Result<std::sync::Arc<dyn neovex::LocalKeyProvider>> {
    let key_provider_config = config
        .local_encryption
        .key_provider()
        .ok_or_else(|| Error::InvalidInput("encryption key provider not configured".to_string()))?;
    Ok(InitializedKeyProvider::from_config(key_provider_config)?.provider())
}

fn verify_source_exists(source: &Path) -> Result<()> {
    if source.exists() {
        return Ok(());
    }
    Err(Error::InvalidInput(format!(
        "source database does not exist: {}",
        source.display()
    )))
}

fn verify_target_absent(target: &Path) -> Result<()> {
    if target.exists() {
        return Err(Error::InvalidInput(format!(
            "target already exists: {}; remove it first or specify a different target",
            target.display()
        )));
    }
    let manifest_path = KeyManifest::manifest_path(target);
    if manifest_path.exists() {
        return Err(Error::InvalidInput(format!(
            "target manifest already exists: {}; remove it first or specify a different target",
            manifest_path.display()
        )));
    }
    Ok(())
}

fn staging_path(target: &Path, suffix: &str) -> Result<PathBuf> {
    let file_name = target
        .file_name()
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "target path has no file name: {}",
                target.display()
            ))
        })?
        .to_string_lossy();
    let staging_name = format!("{file_name}.{suffix}.tmp");
    let parent = target.parent().ok_or_else(|| {
        Error::InvalidInput(format!("target path has no parent: {}", target.display()))
    })?;
    Ok(parent.join(staging_name))
}

fn publish_encrypted_target(
    staging_target: &Path,
    target: &Path,
    manifest: &KeyManifest,
) -> Result<()> {
    std::fs::rename(staging_target, target)
        .map_err(|error| Error::Internal(format!("failed to publish encrypted target: {error}")))?;

    if let Err(error) = manifest.write_for(target) {
        let _ = retire_plaintext_artifact(target);
        return Err(Error::Internal(format!(
            "failed to persist encryption manifest after migration: {error}"
        )));
    }
    Ok(())
}

fn publish_plaintext_target(staging_target: &Path, target: &Path) -> Result<()> {
    std::fs::rename(staging_target, target)
        .map_err(|error| Error::Internal(format!("failed to publish plaintext target: {error}")))
}

pub(crate) fn database_subject(
    provider: ProviderFamily,
    tenant_id: Option<&str>,
    path: &Path,
) -> Result<LocalKeySubject> {
    let logical_name = path
        .file_name()
        .ok_or_else(|| Error::InvalidInput(format!("path has no file name: {}", path.display())))?
        .to_string_lossy()
        .to_string();
    match provider {
        ProviderFamily::Sqlite => {
            let tenant_id = require_tenant_id(tenant_id, "sqlite tenant database")?;
            Ok(LocalKeySubject::sqlite_tenant(tenant_id, logical_name))
        }
        ProviderFamily::Redb => {
            if let Some(tenant_id) = tenant_id {
                Ok(LocalKeySubject::redb_tenant(
                    TenantId::new(tenant_id.to_string())?,
                    logical_name,
                ))
            } else if logical_name == "neovex-control.db" {
                Ok(LocalKeySubject::control_plane(logical_name))
            } else {
                Err(Error::InvalidInput(
                    "--tenant-id is required for redb tenant databases; omit it only for neovex-control.db"
                        .to_string(),
                ))
            }
        }
        ProviderFamily::LibsqlCache => {
            let tenant_id = require_tenant_id(tenant_id, "libsql replica cache")?;
            Ok(LocalKeySubject::libsql_cache(tenant_id, logical_name))
        }
    }
}

fn require_tenant_id(tenant_id: Option<&str>, subject_name: &str) -> Result<TenantId> {
    let tenant_id = tenant_id.ok_or_else(|| {
        Error::InvalidInput(format!(
            "--tenant-id is required for {subject_name} operations"
        ))
    })?;
    TenantId::new(tenant_id.to_string())
}

/// Encrypts a plaintext redb file into an encrypted target.
fn encrypt_plaintext_redb(source: &Path, target: &Path, dek: &[u8; 32]) -> Result<()> {
    use std::io::{Read, Write};

    use aes_gcm_siv::aead::{Aead, KeyInit, Payload};
    use aes_gcm_siv::{Aes256GcmSiv, Nonce};
    use rand::RngCore;

    let mut source_file = std::fs::File::open(source)
        .map_err(|e| Error::Internal(format!("failed to open source: {e}")))?;
    let source_len = source_file
        .metadata()
        .map_err(|e| Error::Internal(format!("failed to get source metadata: {e}")))?
        .len();

    if source_len == 0 {
        std::fs::File::create(target)
            .map_err(|e| Error::Internal(format!("failed to create empty target: {e}")))?;
        println!("  Source is empty, created empty target.");
        return Ok(());
    }

    let page_count = source_len.div_ceil(LOGICAL_PAGE_SIZE as u64);

    let cipher = Aes256GcmSiv::new_from_slice(dek)
        .map_err(|e| Error::Internal(format!("failed to create cipher: {e}")))?;

    let mut target_file = std::fs::File::create(target)
        .map_err(|e| Error::Internal(format!("failed to create target: {e}")))?;

    let mut page_buf = vec![0u8; LOGICAL_PAGE_SIZE];
    for page_idx in 0..page_count {
        let bytes_remaining = source_len - (page_idx * LOGICAL_PAGE_SIZE as u64);
        let read_size = std::cmp::min(bytes_remaining as usize, LOGICAL_PAGE_SIZE);

        page_buf.fill(0);
        source_file
            .read_exact(&mut page_buf[..read_size])
            .map_err(|e| Error::Internal(format!("read failed at page {page_idx}: {e}")))?;

        let mut nonce_bytes = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let mut aad = Vec::with_capacity(16);
        aad.extend_from_slice(&1u32.to_be_bytes());
        aad.extend_from_slice(&page_idx.to_be_bytes());
        aad.extend_from_slice(&(LOGICAL_PAGE_SIZE as u32).to_be_bytes());

        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: page_buf.as_slice(),
                    aad: &aad,
                },
            )
            .map_err(|e| Error::Internal(format!("encryption failed at page {page_idx}: {e}")))?;

        target_file
            .write_all(&nonce_bytes)
            .map_err(|e| Error::Internal(format!("write nonce failed: {e}")))?;
        target_file
            .write_all(&ciphertext)
            .map_err(|e| Error::Internal(format!("write ciphertext failed: {e}")))?;
    }

    target_file
        .sync_all()
        .map_err(|e| Error::Internal(format!("sync failed: {e}")))?;

    println!("  Encrypted {page_count} pages.");
    Ok(())
}

/// Decrypts an encrypted redb file to a plaintext target.
fn decrypt_redb_to_plaintext(source: &Path, target: &Path, dek: &[u8; 32]) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};

    let mut source_file = std::fs::File::open(source)
        .map_err(|e| Error::Internal(format!("failed to open source: {e}")))?;
    let source_len = source_file
        .metadata()
        .map_err(|e| Error::Internal(format!("failed to get source metadata: {e}")))?
        .len();

    if source_len == 0 {
        std::fs::File::create(target)
            .map_err(|e| Error::Internal(format!("failed to create empty target: {e}")))?;
        println!("  Source is empty, created empty target.");
        return Ok(());
    }

    let page_count = source_len / PHYSICAL_PAGE_SIZE as u64;
    if source_len % PHYSICAL_PAGE_SIZE as u64 != 0 {
        return Err(Error::Internal(format!(
            "source file size {} is not a multiple of physical page size {}",
            source_len, PHYSICAL_PAGE_SIZE
        )));
    }

    use aes_gcm_siv::aead::{Aead, KeyInit, Payload};
    use aes_gcm_siv::{Aes256GcmSiv, Nonce};

    let cipher = Aes256GcmSiv::new_from_slice(dek)
        .map_err(|e| Error::Internal(format!("failed to create cipher: {e}")))?;

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

        let plaintext = cipher
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

        target_file
            .write_all(&plaintext)
            .map_err(|e| Error::Internal(format!("write failed at page {page_idx}: {e}")))?;
    }

    target_file
        .sync_all()
        .map_err(|e| Error::Internal(format!("sync failed: {e}")))?;

    println!("  Decrypted {page_count} pages.");
    Ok(())
}

pub(crate) fn retire_plaintext_artifact(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path)
            .map_err(|e| Error::Internal(format!("failed to retire plaintext artifact: {e}")))?;
    }

    for sidecar in [
        path.with_extension("sqlite3-wal"),
        path.with_extension("sqlite3-shm"),
        path.with_extension("sqlite3-journal"),
    ] {
        if sidecar.exists() {
            std::fs::remove_file(&sidecar).map_err(|e| {
                Error::Internal(format!("failed to retire {}: {e}", sidecar.display()))
            })?;
        }
    }

    let manifest_path = KeyManifest::manifest_path(path);
    if manifest_path.exists() {
        let _ = std::fs::remove_file(manifest_path);
    }

    println!("Retired: {}", path.display());
    Ok(())
}
