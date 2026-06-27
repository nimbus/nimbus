use std::error::Error;
use std::net::{Ipv4Addr, SocketAddr};

use clap::Args;
use nimbus::TenantId;
use nimbus_kv::{CredentialRegistry, NimbusKvConfig, run_listener};

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
}

pub(crate) async fn run_kv_command(command: KvCommand) -> Result<(), Box<dyn Error>> {
    let tenant = TenantId::new(command.tenant)?;
    let username = command
        .username
        .unwrap_or_else(|| tenant.as_str().to_owned());
    let password = command
        .password
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

    let config = NimbusKvConfig::new(command.bind, credentials);
    run_listener(config).await?;
    Ok(())
}

impl Default for KvCommand {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 6380)),
            tenant: "demo".to_owned(),
            username: None,
            password: None,
        }
    }
}
