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
    EmbeddedProviderKind, Engine, EnginePersistenceConfig, PointInTimeRestoreArchive, TenantId,
};
use serde::{Deserialize, Serialize};

use crate::cli_ux;

const BACKUP_FORMAT_VERSION: u16 = 1;

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

pub(crate) async fn run_backup_command(command: BackupCommand) -> Result<(), Box<dyn Error>> {
    match command {
        BackupCommand::Create(command) => run_backup_create(command).await,
        BackupCommand::Restore(command) => run_backup_restore(command).await,
    }
}

async fn run_backup_create(command: BackupCreateCommand) -> Result<(), Box<dyn Error>> {
    if command.out.exists() {
        return Err(format!(
            "backup target {} already exists; refusing to overwrite",
            command.out.display()
        )
        .into());
    }
    let engine = open_engine(&command.data_dir, command.provider).await?;
    let mut tenants = BTreeMap::new();
    for tenant_id in engine.list_tenants()? {
        let archive = engine.export_latest_point_in_time_restore_archive(&tenant_id)?;
        tenants.insert(tenant_id.as_str().to_string(), archive);
    }
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
    engine.quiesce().await;
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
    let backup: BackupFile = serde_json::from_slice(&raw).map_err(|error| {
        format!(
            "backup file {} is not a valid nimbus backup: {error}",
            command.input.display()
        )
    })?;
    if backup.format_version != BACKUP_FORMAT_VERSION {
        return Err(format!(
            "backup file {} has unsupported format version {} (this binary supports {})",
            command.input.display(),
            backup.format_version,
            BACKUP_FORMAT_VERSION,
        )
        .into());
    }
    let engine = open_engine(&command.data_dir, command.provider).await?;
    for (tenant_name, archive) in &backup.tenants {
        let tenant_id = TenantId::new(tenant_name)?;
        engine.create_tenant(tenant_id.clone())?; // tenant-lifecycle: embedded-only
        engine.import_point_in_time_restore_archive(&tenant_id, archive)?;
    }
    engine.quiesce().await;
    emit_backup_info(format!(
        "restored {} tenant(s) from {} into {}",
        backup.tenants.len(),
        command.input.display(),
        command.data_dir.display(),
    ));
    Ok(())
}

async fn open_engine(
    data_dir: &Path,
    provider: BackupProvider,
) -> Result<std::sync::Arc<Engine>, Box<dyn Error>> {
    let config = EnginePersistenceConfig::embedded(data_dir, provider.embedded_kind());
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
    use nimbus::TableName;
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
            "--out",
            "b.json",
        ]);
        let Command::Backup(BackupCommand::Create(create)) = cli.command else {
            panic!("backup create should parse");
        };
        assert_eq!(create.provider, BackupProvider::Sqlite);

        let cli = Cli::parse_from([
            "nimbus",
            "backup",
            "restore",
            "--in",
            "b.json",
            "--provider",
            "redb",
        ]);
        let Command::Backup(BackupCommand::Restore(restore)) = cli.command else {
            panic!("backup restore should parse");
        };
        assert_eq!(restore.provider, BackupProvider::Redb);
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
            let engine = open_engine(&source_dir, BackupProvider::Sqlite)
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
            provider: BackupProvider::Sqlite,
            out: backup_path.clone(),
        })
        .await
        .expect("backup create should succeed");

        // Second create against the same path must refuse to overwrite.
        let clobber = run_backup_create(BackupCreateCommand {
            data_dir: source_dir.clone(),
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
            provider: BackupProvider::Sqlite,
        })
        .await
        .expect("backup restore should succeed");

        let restored_engine = open_engine(&restore_dir, BackupProvider::Sqlite)
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
}
