//! LR9: `nimbus backup` — offline whole-deployment backup and restore for
//! the embedded providers, riding the storage layer's point-in-time
//! restore archives (SEQ8). One archive per tenant, one JSON file per
//! backup; restore fails closed unless the target tenant journal is empty
//! and every restored fingerprint matches its archive.
//!
//! External providers (Postgres, MySQL, libSQL) are deliberately out of
//! scope here: they ship their own first-class backup tooling, and this
//! command operates on the local data directory while the server is
//! stopped.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand, ValueEnum};
use nimbus::{
    ControlPlaneConfig, EmbeddedProviderKind, Engine, EnginePersistenceConfig,
    PointInTimeRestoreArchive, TenantId,
};
use serde::{Deserialize, Serialize};

use crate::cli_ux;
use crate::embedded_control_plane::require_existing_control_plane;

const BACKUP_FORMAT_VERSION: u16 = 3;

#[derive(Debug, Subcommand)]
pub(crate) enum BackupCommand {
    /// Write a point-in-time backup of every tenant to a single file.
    Create(BackupCreateCommand),
    /// Restore a backup file into an empty data directory.
    Restore(BackupRestoreCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum BackupProvider {
    Sqlite,
    Redb,
}

impl BackupProvider {
    fn embedded_kind(self) -> EmbeddedProviderKind {
        match self {
            Self::Sqlite => EmbeddedProviderKind::Sqlite,
            Self::Redb => EmbeddedProviderKind::Redb,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Redb => "redb",
        }
    }
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct BackupCreateCommand {
    /// Local data directory of the deployment (server must be stopped).
    #[arg(long, default_value = "./data")]
    pub(crate) data_dir: PathBuf,

    /// Control-plane directory used by the stopped deployment. Defaults to
    /// the data directory.
    #[arg(long)]
    pub(crate) control_data_dir: Option<PathBuf>,

    /// Embedded tenant persistence provider of the data directory.
    /// External providers (postgres, mysql, libsql) use their own native
    /// backup tooling.
    #[arg(long, value_enum, default_value_t = BackupProvider::Sqlite)]
    pub(crate) provider: BackupProvider,

    /// Output backup file. Refuses to overwrite an existing file.
    #[arg(long)]
    pub(crate) out: PathBuf,
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct BackupRestoreCommand {
    /// Backup file produced by `nimbus backup create`.
    #[arg(long = "in")]
    pub(crate) input: PathBuf,

    /// Target data directory. Every restored tenant must have an empty
    /// journal — restore into a fresh directory.
    #[arg(long, default_value = "./data")]
    pub(crate) data_dir: PathBuf,

    /// Control-plane directory for the restored deployment. Defaults to the
    /// target data directory.
    #[arg(long)]
    pub(crate) control_data_dir: Option<PathBuf>,

    /// Embedded tenant persistence provider for the restored deployment.
    #[arg(long, value_enum, default_value_t = BackupProvider::Sqlite)]
    pub(crate) provider: BackupProvider,
}

/// On-disk backup file: one point-in-time restore archive per tenant.
#[derive(Debug, Serialize, Deserialize)]
struct BackupFile {
    format_version: u16,
    provider: String,
    tenants: BTreeMap<String, PointInTimeRestoreArchive>,
}

#[derive(Deserialize)]
struct BackupFileHeader {
    format_version: u16,
}

#[derive(Deserialize)]
struct BackupFilePayload {
    format_version: u16,
    provider: String,
    tenants: BTreeMap<String, serde_json::Value>,
}

pub(crate) async fn run_backup_command(
    command: BackupCommand,
    persistence_config: &EnginePersistenceConfig,
) -> Result<(), Box<dyn Error>> {
    if persistence_config.local_encryption.is_enabled()
        || backup_command_has_encryption_marker(&command)?
    {
        return Err(
            "`nimbus backup` does not support encrypted data directories; stop Nimbus and cold-copy the complete data and control directories with every `.nimbus-enc` sidecar, then protect the key material separately"
                .into(),
        );
    }
    match command {
        BackupCommand::Create(command) => run_backup_create(command).await,
        BackupCommand::Restore(command) => run_backup_restore(command).await,
    }
}

fn backup_command_has_encryption_marker(command: &BackupCommand) -> Result<bool, Box<dyn Error>> {
    let (data_dir, control_data_dir) = match command {
        BackupCommand::Create(command) => (&command.data_dir, command.control_data_dir.as_deref()),
        BackupCommand::Restore(command) => (&command.data_dir, command.control_data_dir.as_deref()),
    };
    let control_data_dir = control_data_dir.unwrap_or(data_dir);
    root_has_encryption_marker(data_dir)
        .and_then(|found| Ok(found || root_has_encryption_marker(control_data_dir)?))
}

fn root_has_encryption_marker(root: &Path) -> Result<bool, Box<dyn Error>> {
    // Embedded tenant databases and the retained control database are direct
    // children of their selected roots. Their manifests are adjacent files,
    // so this bounded scan covers the persistence contract without walking
    // an unrelated object byte plane.
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "failed to inspect backup root {} for encryption sidecars: {error}",
                root.display()
            )
            .into());
        }
    };
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "failed to inspect backup root {} for encryption sidecars: {error}",
                root.display()
            )
        })?;
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == std::ffi::OsStr::new("nimbus-enc"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn run_backup_create(command: BackupCreateCommand) -> Result<(), Box<dyn Error>> {
    if command.out.exists() {
        return Err(format!(
            "backup target {} already exists; refusing to overwrite",
            command.out.display()
        )
        .into());
    }
    let control_data_dir = command
        .control_data_dir
        .as_deref()
        .unwrap_or(&command.data_dir);
    require_existing_control_plane(control_data_dir, "backup create")?;
    let engine = open_engine(
        &command.data_dir,
        command.control_data_dir.as_deref(),
        command.provider,
    )
    .await
    .map_err(|error| {
        format!(
            "failed to open backup source {}: {error}",
            command.data_dir.display()
        )
    })?;
    let tenants_result: Result<_, Box<dyn Error>> = (|| {
        let mut tenants = BTreeMap::new();
        for tenant_id in engine.list_tenants()? {
            let archive = engine
                .export_latest_point_in_time_restore_archive(&tenant_id)
                .map_err(|error| format!("failed to export tenant {tenant_id}: {error}"))?;
            tenants.insert(tenant_id.as_str().to_string(), archive);
        }
        Ok(tenants)
    })();
    engine.quiesce().await;
    let tenants = tenants_result?;
    let backup = BackupFile {
        format_version: BACKUP_FORMAT_VERSION,
        provider: command.provider.as_str().to_string(),
        tenants,
    };
    let encoded = serde_json::to_vec(&backup)?;
    if let Some(parent) = command.out.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&command.out, &encoded)?;
    emit_backup_info(format!(
        "backed up {} tenant(s) from {} to {} ({} bytes)",
        backup.tenants.len(),
        command.data_dir.display(),
        command.out.display(),
        encoded.len(),
    ));
    Ok(())
}

async fn run_backup_restore(command: BackupRestoreCommand) -> Result<(), Box<dyn Error>> {
    let raw = std::fs::read(&command.input).map_err(|error| {
        format!(
            "failed to read backup file {}: {error}",
            command.input.display()
        )
    })?;
    let backup = decode_backup_file(&raw, &command.input)?;
    let engine = open_engine(
        &command.data_dir,
        command.control_data_dir.as_deref(),
        command.provider,
    )
    .await
    .map_err(|error| {
        format!(
            "failed to open restore target {}: {error}",
            command.data_dir.display()
        )
    })?;
    let restore_result: Result<(), Box<dyn Error>> = (|| {
        for (tenant_name, archive) in &backup.tenants {
            let tenant_id = TenantId::new(tenant_name)?;
            engine.create_tenant(tenant_id.clone())?; // tenant-lifecycle: embedded-only
            engine
                .import_point_in_time_restore_archive(&tenant_id, archive)
                .map_err(|error| format!("failed to restore tenant {tenant_id}: {error}"))?;
        }
        Ok(())
    })();
    engine.quiesce().await;
    restore_result?;
    emit_backup_info(format!(
        "restored {} tenant(s) from {} into {}",
        backup.tenants.len(),
        command.input.display(),
        command.data_dir.display(),
    ));
    Ok(())
}

fn decode_backup_file(raw: &[u8], input: &Path) -> Result<BackupFile, Box<dyn Error>> {
    let header: BackupFileHeader = serde_json::from_slice(raw).map_err(|error| {
        format!(
            "backup file {} is not a valid nimbus backup: {error}",
            input.display()
        )
    })?;
    if header.format_version != BACKUP_FORMAT_VERSION {
        let codec_context = (header.format_version < BACKUP_FORMAT_VERSION).then_some(
            "; this backup predates materialized-position digest codec version 3 and must be recreated with a current Nimbus binary",
        );
        return Err(format!(
            "backup file {} has unsupported format version {} (this binary supports {}){}",
            input.display(),
            header.format_version,
            BACKUP_FORMAT_VERSION,
            codec_context.unwrap_or_default(),
        )
        .into());
    }

    let payload: BackupFilePayload = serde_json::from_slice(raw).map_err(|error| {
        format!(
            "backup file {} is not a valid nimbus backup: {error}",
            input.display()
        )
    })?;
    let mut tenants = BTreeMap::new();
    for (tenant, archive_json) in payload.tenants {
        let archive_bytes = serde_json::to_vec(&archive_json).map_err(|error| {
            format!(
                "backup file {} tenant {tenant} archive could not be decoded: {error}",
                input.display()
            )
        })?;
        let archive = PointInTimeRestoreArchive::decode_json(&archive_bytes).map_err(|error| {
            format!(
                "backup file {} tenant {tenant} has an invalid point-in-time restore archive: {error}",
                input.display()
            )
        })?;
        tenants.insert(tenant, archive);
    }
    Ok(BackupFile {
        format_version: payload.format_version,
        provider: payload.provider,
        tenants,
    })
}

async fn open_engine(
    data_dir: &Path,
    control_data_dir: Option<&Path>,
    provider: BackupProvider,
) -> Result<std::sync::Arc<Engine>, Box<dyn Error>> {
    let mut config = EnginePersistenceConfig::embedded(data_dir, provider.embedded_kind());
    if let Some(control_data_dir) = control_data_dir {
        config.control_plane = ControlPlaneConfig::embedded_redb(control_data_dir);
    }
    Ok(std::sync::Arc::new(
        Engine::new_with_persistence_config(config).await?,
    ))
}

fn emit_backup_info(message: impl AsRef<str>) {
    if cli_ux::info_output_enabled() {
        let _ = cli_ux::write_stderr_prefixed_line("info:", message.as_ref());
    } else {
        println!("{}", message.as_ref());
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use nimbus::{LocalEncryptionConfig, LocalKeyProviderConfig, MasterKeyFileConfig, TableName};
    use serde_json::json;

    use super::*;
    use crate::{Cli, Command};

    fn fields(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        value
            .as_object()
            .expect("test fields should be an object")
            .clone()
    }

    #[test]
    fn cli_parses_backup_create_and_restore() {
        let cli = Cli::parse_from([
            "nimbus",
            "backup",
            "create",
            "--data-dir",
            "./d",
            "--control-data-dir",
            "./c",
            "--out",
            "b.json",
        ]);
        let Command::Backup(BackupCommand::Create(create)) = cli.command else {
            panic!("backup create should parse");
        };
        assert_eq!(create.provider, BackupProvider::Sqlite);
        assert_eq!(create.control_data_dir, Some(PathBuf::from("./c")));

        let cli = Cli::parse_from([
            "nimbus",
            "backup",
            "restore",
            "--in",
            "b.json",
            "--provider",
            "redb",
            "--control-data-dir",
            "./restored-control",
        ]);
        let Command::Backup(BackupCommand::Restore(restore)) = cli.command else {
            panic!("backup restore should parse");
        };
        assert_eq!(restore.provider, BackupProvider::Redb);
        assert_eq!(
            restore.control_data_dir,
            Some(PathBuf::from("./restored-control"))
        );
    }

    #[test]
    fn backup_decode_reports_pre_digest_codec_format_before_tenants() {
        let legacy = br#"{
            "format_version": 1,
            "provider": "sqlite",
            "tenants": {
                "alpha": {
                    "version": 1,
                    "target_position": {
                        "version": 1,
                        "applied_sequence": 0,
                        "state_digest": "0000000000000000000000000000000000000000000000000000000000000000"
                    }
                }
            }
        }"#;

        let error = decode_backup_file(legacy, Path::new("legacy.json"))
            .expect_err("legacy backup must fail before tenant archive decoding");
        let message = error.to_string();
        assert!(
            message.contains("unsupported format version 1")
                && message.contains("materialized-position digest codec"),
            "legacy backup diagnostics must name the outer format and codec change: {message}"
        );
    }

    #[test]
    fn backup_decode_reports_tenant_archive_version_before_nested_position() {
        let legacy_tenant = br#"{
            "format_version": 3,
            "provider": "sqlite",
            "tenants": {
                "alpha": {
                    "version": 2,
                    "target_position": {
                        "version": 1,
                        "applied_sequence": 0,
                        "state_digest": "0000000000000000000000000000000000000000000000000000000000000000"
                    }
                }
            }
        }"#;

        let error = decode_backup_file(legacy_tenant, Path::new("relabeled.json"))
            .expect_err("current envelope must still validate each tenant archive header first");
        let message = error.to_string();
        assert!(
            message.contains("tenant alpha")
                && message.contains("unsupported point-in-time restore archive version 2")
                && message.contains("materialized-position digest codec"),
            "tenant archive diagnostics must name the tenant, archive, and codec change: {message}"
        );
    }

    #[tokio::test]
    async fn backup_restore_round_trip() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let source_dir = temp.path().join("source");
        let backup_path = temp.path().join("backups/deployment.json");
        let restore_dir = temp.path().join("restored");
        // Seed two tenants with distinct documents.
        let table = TableName::new("notes").expect("table should build");
        let alpha = TenantId::new("alpha").expect("tenant should build");
        let beta = TenantId::new("beta").expect("tenant should build");
        let mut expected = Vec::new();
        {
            let engine = open_engine(&source_dir, None, BackupProvider::Sqlite)
                .await
                .expect("source engine should open");
            for (tenant, body) in [(&alpha, "first"), (&beta, "second")] {
                engine
                    .create_tenant(tenant.clone())
                    .expect("tenant should create");
                let id = engine
                    .insert_document(
                        tenant,
                        table.clone(),
                        fields(json!({ "body": body, "rank": 7 })),
                    )
                    .expect("insert should succeed");
                let document = engine
                    .get_document(tenant, &table, id.clone())
                    .expect("document should read back");
                expected.push((tenant.clone(), id, document.into_json()));
            }
            engine.quiesce().await;
        }

        run_backup_create(BackupCreateCommand {
            data_dir: source_dir.clone(),
            control_data_dir: None,
            provider: BackupProvider::Sqlite,
            out: backup_path.clone(),
        })
        .await
        .expect("backup create should succeed");

        // Second create against the same path must refuse to overwrite.
        let clobber = run_backup_create(BackupCreateCommand {
            data_dir: source_dir.clone(),
            control_data_dir: None,
            provider: BackupProvider::Sqlite,
            out: backup_path.clone(),
        })
        .await
        .expect_err("existing backup file must not be overwritten");
        assert!(clobber.to_string().contains("refusing to overwrite"));

        // Restore into a fresh directory (the wipe-equivalent) and compare
        // every document byte-for-byte through its JSON form.
        run_backup_restore(BackupRestoreCommand {
            input: backup_path,
            data_dir: restore_dir.clone(),
            control_data_dir: None,
            provider: BackupProvider::Sqlite,
        })
        .await
        .expect("backup restore should succeed");

        let restored_engine = open_engine(&restore_dir, None, BackupProvider::Sqlite)
            .await
            .expect("restored engine should open");
        let mut tenant_ids = restored_engine
            .list_tenants()
            .expect("restored tenants should list");
        tenant_ids.sort();
        assert_eq!(
            tenant_ids,
            vec![alpha.clone(), beta.clone()],
            "both tenants should exist after restore"
        );
        for (tenant, id, expected_json) in &expected {
            let restored = restored_engine
                .get_document(tenant, &table, id.clone())
                .expect("restored document should read")
                .into_json();
            assert_eq!(
                &restored, expected_json,
                "restored document must match the original byte-for-byte"
            );
        }
        restored_engine.quiesce().await;
    }

    #[tokio::test]
    async fn backup_create_refuses_to_bootstrap_a_missing_control_plane() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let data_dir = temp.path().join("tenant-data");
        let control_data_dir = temp.path().join("missing-control");
        let backup_path = temp.path().join("deployment.json");

        let error = run_backup_create(BackupCreateCommand {
            data_dir,
            control_data_dir: Some(control_data_dir.clone()),
            provider: BackupProvider::Sqlite,
            out: backup_path.clone(),
        })
        .await
        .expect_err("a missing source control plane must fail before engine bootstrap");

        let message = error.to_string();
        assert!(
            message.contains("backup create requires an existing control-plane database")
                && message.contains("--control-data-dir"),
            "diagnostic must identify the missing source authority: {message}"
        );
        assert!(
            !backup_path.exists(),
            "rejected backup must not create an archive"
        );
        assert!(
            !control_data_dir
                .join(EmbeddedProviderKind::Redb.control_database_filename())
                .exists(),
            "rejected backup must not bootstrap an empty control plane"
        );
    }

    #[tokio::test]
    async fn redb_backup_uses_separate_control_roots() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let source_data = temp.path().join("source-data");
        let source_control = temp.path().join("source-control");
        let restore_data = temp.path().join("restore-data");
        let restore_control = temp.path().join("restore-control");
        let backup_path = temp.path().join("redb-backup.json");
        let system = TenantId::new("_nimbus").expect("system tenant should build");
        let application = TenantId::new("application").expect("application tenant should build");
        let table = TableName::new("notes").expect("table should build");
        let mut expected = Vec::new();

        {
            let engine = open_engine(&source_data, Some(&source_control), BackupProvider::Redb)
                .await
                .expect("source redb engine should open");
            for tenant in [&system, &application] {
                engine
                    .create_tenant(tenant.clone())
                    .expect("tenant should create");
                let document_id = engine
                    .insert_document(
                        tenant,
                        table.clone(),
                        fields(json!({ "owner": tenant.as_str() })),
                    )
                    .expect("document should insert");
                expected.push((tenant.clone(), document_id));
            }
            engine.quiesce().await;
        }

        run_backup_create(BackupCreateCommand {
            data_dir: source_data,
            control_data_dir: Some(source_control),
            provider: BackupProvider::Redb,
            out: backup_path.clone(),
        })
        .await
        .expect("redb backup should use the source incarnation authority");
        run_backup_restore(BackupRestoreCommand {
            input: backup_path,
            data_dir: restore_data.clone(),
            control_data_dir: Some(restore_control.clone()),
            provider: BackupProvider::Redb,
        })
        .await
        .expect("redb restore should create incarnations in the target control root");

        let restored = open_engine(&restore_data, Some(&restore_control), BackupProvider::Redb)
            .await
            .expect("restored redb engine should open");
        for (tenant, document_id) in expected {
            let document = restored
                .get_document(&tenant, &table, document_id)
                .expect("restored document should load");
            assert_eq!(
                document.fields.get("owner"),
                Some(&json!(tenant.as_str())),
                "restored tenant {tenant} must retain its document"
            );
        }
        restored.quiesce().await;
    }

    #[tokio::test]
    async fn backup_rejects_encrypted_data_with_cold_copy_guidance() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let key_path = temp.path().join("master.key");
        std::fs::write(&key_path, [0x42; 32]).expect("master key should write");
        let persistence_config = EnginePersistenceConfig::embedded_default(temp.path())
            .with_local_encryption(LocalEncryptionConfig::Enabled(
                LocalKeyProviderConfig::MasterKeyFile(MasterKeyFileConfig { path: key_path }),
            ));
        let output = temp.path().join("deployment.json");

        let error = run_backup_command(
            BackupCommand::Create(BackupCreateCommand {
                data_dir: temp.path().join("data"),
                control_data_dir: None,
                provider: BackupProvider::Sqlite,
                out: output.clone(),
            }),
            &persistence_config,
        )
        .await
        .expect_err("encrypted data must fail before the backup engine opens");

        let message = error.to_string();
        assert!(
            message.contains("does not support encrypted data directories")
                && message.contains("cold-copy")
                && message.contains(".nimbus-enc")
                && message.contains("key material"),
            "diagnostic must give the complete encrypted backup procedure: {message}"
        );
        assert!(
            !output.exists(),
            "rejected backup must not create an output"
        );
    }

    #[tokio::test]
    async fn backup_detects_encryption_from_the_selected_roots_without_ambient_config() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let data_dir = temp.path().join("data");
        let control_data_dir = temp.path().join("control");
        std::fs::create_dir_all(&data_dir).expect("data dir should build");
        std::fs::create_dir_all(&control_data_dir).expect("control dir should build");
        std::fs::write(
            control_data_dir.join("nimbus-control.db.nimbus-enc"),
            b"manifest",
        )
        .expect("encryption marker should write");
        let output = temp.path().join("deployment.json");

        let error = run_backup_command(
            BackupCommand::Create(BackupCreateCommand {
                data_dir,
                control_data_dir: Some(control_data_dir),
                provider: BackupProvider::Sqlite,
                out: output.clone(),
            }),
            &EnginePersistenceConfig::embedded_default(temp.path().join("unrelated-default")),
        )
        .await
        .expect_err("selected encrypted roots must fail without ambient encryption config");

        assert!(
            error
                .to_string()
                .contains("does not support encrypted data directories"),
            "selected-root detection must preserve the cold-copy diagnostic: {error}"
        );
        assert!(
            !output.exists(),
            "selected-root rejection must happen before output creation"
        );
    }
}
