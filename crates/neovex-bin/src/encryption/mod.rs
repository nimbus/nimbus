//! Encryption admin CLI commands.
//!
//! This module provides administrative commands for encryption operations:
//! - `status`: Inspect encryption coverage
//! - `migrate`: Migrate plaintext databases to encrypted
//! - `export`: Export encrypted databases to plaintext (recovery)
//! - `rotate-kek`: Rotate key-encryption keys (rewrap manifests only)
//! - `rotate-dek`: Rotate data-encryption keys (provider-specific)

mod migrate;
mod rotate;
mod status;

use clap::Subcommand;
use neovex::ServicePersistenceConfig;

pub(crate) use migrate::{ExportCommand, MigrateCommand};
pub(crate) use rotate::{RotateDekCommand, RotateKekCommand};
pub(crate) use status::StatusCommand;

/// Encryption admin commands for managing local at-rest encryption.
#[derive(Debug, Subcommand)]
pub(crate) enum EncryptionCommand {
    /// Inspect encryption coverage and status.
    Status(StatusCommand),

    /// Migrate plaintext databases to encrypted.
    Migrate(MigrateCommand),

    /// Export encrypted databases to plaintext for recovery.
    Export(ExportCommand),

    /// Rotate key-encryption keys (rewrap manifests without rewriting data).
    RotateKek(RotateKekCommand),

    /// Rotate data-encryption keys (provider-specific, may rewrite data).
    RotateDek(RotateDekCommand),
}

/// Run an encryption admin command.
pub(crate) async fn run_encryption_command(
    command: EncryptionCommand,
    config: &ServicePersistenceConfig,
) -> neovex::Result<()> {
    match command {
        EncryptionCommand::Status(cmd) => status::run_status_command(cmd, config).await,
        EncryptionCommand::Migrate(cmd) => migrate::run_migrate_command(cmd, config).await,
        EncryptionCommand::Export(cmd) => migrate::run_export_command(cmd, config).await,
        EncryptionCommand::RotateKek(cmd) => rotate::run_rotate_kek_command(cmd, config).await,
        EncryptionCommand::RotateDek(cmd) => rotate::run_rotate_dek_command(cmd, config).await,
    }
}
