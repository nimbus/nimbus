use std::error::Error;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use clap::Args;
use nimbus::TenantId;
use nimbus_kv::{
    CredentialRegistry, NimbusKvConfig, NimbusKvListenerConfig, NimbusKvStore, TieringConfig,
    run_listener,
};

#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = "Examples:\n  nimbus kv --tenant demo\n  nimbus kv --bind 127.0.0.1:6380 --tenant demo --password local-secret\n"
)]
pub(crate) struct KvCommand {
    /// Address for the RESP listener. Must be loopback.
    #[arg(long, default_value = "127.0.0.1:6380")]
    bind: SocketAddr,
    /// Tenant id bound to the generated or supplied dev credential.
    #[arg(long, default_value = "demo")]
    tenant: String,
    /// Username for AUTH. Defaults to the tenant id.
    #[arg(long)]
    username: Option<String>,
    /// Password for AUTH. Defaults to NIMBUS_KV_PASSWORD or a generated value.
    #[arg(long)]
    password: Option<String>,
    /// redb tenant file for durable mode.
    #[arg(long)]
    data: Option<PathBuf>,
    /// Shared control-plane directory containing host-global network authority.
    #[arg(long)]
    control_data_dir: Option<PathBuf>,
    /// Keep state in memory only.
    #[arg(long)]
    no_disk: bool,
    /// Disable the read-through cache.
    #[arg(long)]
    no_cache: bool,
    /// Approximate cache byte budget.
    #[arg(long)]
    maxmemory: Option<usize>,
}

pub(crate) async fn run_kv_command(command: KvCommand) -> Result<(), Box<dyn Error>> {
    let tenant = TenantId::new(command.tenant.clone())?;
    let username = command
        .username
        .clone()
        .unwrap_or_else(|| tenant.as_str().to_owned());
    let password = command
        .password
        .clone()
        .or_else(|| std::env::var("NIMBUS_KV_PASSWORD").ok());

    let (credentials, password) = match password {
        Some(password) => (
            CredentialRegistry::new().bind(username.clone(), password.clone(), tenant.clone()),
            password,
        ),
        None => {
            let (credentials, generated) =
                CredentialRegistry::generated_dev_for(username.clone(), tenant.clone());
            (credentials, generated.password)
        }
    };

    println!("Nimbus KV listening on {}", command.bind);
    println!(
        "Dev credential: AUTH {} {} (tenant {})",
        username,
        password,
        tenant.as_str()
    );

    let store = kv_store_for_command(&command, &tenant)?;
    let listener = NimbusKvListenerConfig::new(kv_network_state_root(&command));
    let config = NimbusKvConfig::new(command.bind, credentials, listener).with_store(store);
    run_listener(config).await?;
    Ok(())
}

fn kv_store_for_command(
    command: &KvCommand,
    tenant: &TenantId,
) -> Result<NimbusKvStore, Box<dyn Error>> {
    if command.no_disk && command.no_cache {
        return Err("nimbus kv cannot combine --no-disk and --no-cache".into());
    }

    let tiering = match (command.no_disk, command.no_cache) {
        (true, false) => TieringConfig::no_disk(),
        (false, true) => TieringConfig::no_cache(),
        (false, false) => TieringConfig::durable(),
        (true, true) => unreachable!("validated above"),
    };
    let tiering = match command.maxmemory {
        Some(maxmemory) => tiering.with_maxmemory(maxmemory),
        None => tiering,
    };

    if command.no_disk {
        return Ok(NimbusKvStore::no_disk(tiering)?);
    }

    let data = command
        .data
        .clone()
        .unwrap_or_else(|| default_data_path(tenant));
    if let Some(parent) = data.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(NimbusKvStore::durable_at(data, tiering)?)
}

fn default_data_path(tenant: &TenantId) -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".nimbus")
        .join("kv")
        .join(format!("{}.redb", tenant.as_str()))
}

fn kv_network_state_root(command: &KvCommand) -> PathBuf {
    kv_network_state_root_from(command, std::env::var_os("NIMBUS_CONTROL_DATA_DIR"))
}

fn kv_network_state_root_from(
    command: &KvCommand,
    environment_root: Option<std::ffi::OsString>,
) -> PathBuf {
    command
        .control_data_dir
        .clone()
        .or_else(|| environment_root.map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("./data"))
}

impl Default for KvCommand {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 6380)),
            tenant: "demo".to_owned(),
            username: None,
            password: None,
            data: None,
            control_data_dir: None,
            no_disk: false,
            no_cache: false,
            maxmemory: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    #[test]
    fn network_state_root_precedence_is_flag_then_environment_then_default() {
        let command = KvCommand {
            control_data_dir: Some(PathBuf::from("/flag-root")),
            ..KvCommand::default()
        };
        assert_eq!(
            kv_network_state_root_from(&command, Some(OsString::from("/environment-root"))),
            PathBuf::from("/flag-root")
        );

        let command = KvCommand::default();
        assert_eq!(
            kv_network_state_root_from(&command, Some(OsString::from("/environment-root"))),
            PathBuf::from("/environment-root")
        );
        assert_eq!(
            kv_network_state_root_from(&command, None),
            PathBuf::from("./data")
        );
    }
}
