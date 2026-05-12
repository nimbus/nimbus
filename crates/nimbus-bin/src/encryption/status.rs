//! Encryption status inspection command.

use clap::Args;
use nimbus::{EncryptionConfigDescriptor, LocalPersistenceFamily, ServicePersistenceConfig};

/// Inspect encryption coverage and status.
#[derive(Debug, Args)]
pub(crate) struct StatusCommand {
    /// Output format: text or json.
    #[arg(long, default_value = "text")]
    format: OutputFormat,
}

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
enum OutputFormat {
    #[default]
    Text,
    Json,
}

/// Run the encryption status command.
pub(crate) async fn run_status_command(
    command: StatusCommand,
    config: &ServicePersistenceConfig,
) -> nimbus::Result<()> {
    let enabled = config.local_encryption.is_enabled();
    let encryptable_families = config.encryptable_families();
    let descriptor = config.local_encryption.descriptor();

    match command.format {
        OutputFormat::Text => {
            print_text_status(enabled, &encryptable_families, &descriptor);
        }
        OutputFormat::Json => {
            print_json_status(enabled, &encryptable_families, &descriptor)?;
        }
    }

    Ok(())
}

fn print_text_status(
    enabled: bool,
    encryptable_families: &[LocalPersistenceFamily],
    descriptor: &EncryptionConfigDescriptor,
) {
    println!("Encryption Status");
    println!("=================");
    println!();

    if enabled {
        println!("Status: ENABLED");
        println!();

        // Print key provider info
        match descriptor {
            EncryptionConfigDescriptor::Disabled => {
                // Should not happen when enabled is true
            }
            EncryptionConfigDescriptor::Enabled(key_provider) => {
                println!("Key Provider: {key_provider}");
            }
        }
        println!();

        // Print coverage
        println!("Encryptable Families:");
        for family in encryptable_families {
            let family_name = match family {
                LocalPersistenceFamily::EmbeddedSqlite => "Embedded SQLite tenant databases",
                LocalPersistenceFamily::EmbeddedRedb => "Embedded redb tenant databases",
                LocalPersistenceFamily::ControlPlaneRedb => "Control plane redb database",
                LocalPersistenceFamily::LibsqlReplicaCache => "libsql replica cache files",
            };
            let status = if family.is_tenant_data() || family.is_control_plane() {
                "encrypted"
            } else {
                "not applicable"
            };
            println!("  - {family_name}: {status}");
        }
    } else {
        println!("Status: DISABLED");
        println!();
        println!("Local encryption is not configured.");
        println!("To enable, set --encryption-key-provider or NIMBUS_ENCRYPTION_KEY_PROVIDER.");
    }
}

fn print_json_status(
    enabled: bool,
    encryptable_families: &[LocalPersistenceFamily],
    descriptor: &EncryptionConfigDescriptor,
) -> nimbus::Result<()> {
    #[derive(serde::Serialize)]
    struct JsonStatus {
        enabled: bool,
        descriptor: EncryptionConfigDescriptor,
        encryptable_families: Vec<LocalPersistenceFamily>,
    }

    let status = JsonStatus {
        enabled,
        descriptor: descriptor.clone(),
        encryptable_families: encryptable_families.to_vec(),
    };

    let json = serde_json::to_string_pretty(&status).map_err(|e| {
        nimbus::Error::Internal(format!("failed to serialize encryption status: {e}"))
    })?;
    println!("{json}");
    Ok(())
}
