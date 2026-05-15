use std::error::Error;
use std::io;

use clap::{Args, Subcommand};
use nimbus_server::{
    LocalAdminTokenRecord, LocalServerPaths, ServerDiscoveryRecord, load_local_admin_token,
    read_live_server_discovery, rotate_local_admin_token_offline,
};
use serde::Deserialize;

use crate::local_server_client::normalize_loopback_connect_address;

#[derive(Debug, Subcommand)]
pub(crate) enum TokenCommand {
    /// Rotate the local admin token used for localhost server access.
    Rotate(RotateTokenCommand),
}

#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = crate::cli_ux::TOKEN_ROTATE_HELP_EXAMPLES
)]
pub(crate) struct RotateTokenCommand {}

#[derive(Debug)]
enum RotateMode {
    Live { discovery: ServerDiscoveryRecord },
    Offline,
}

#[derive(Debug)]
enum RotateOutcome {
    Live { generation: u64, address: String },
    Offline { generation: u64 },
}

#[derive(Debug, Deserialize)]
struct RotateLocalAdminTokenResponse {
    generation: u64,
}

pub(crate) async fn run_token_command(command: TokenCommand) -> Result<(), Box<dyn Error>> {
    match command {
        TokenCommand::Rotate(command) => run_rotate_token_command(command).await,
    }
}

async fn run_rotate_token_command(
    _command: RotateTokenCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let paths = LocalServerPaths::resolve_for_current_platform()?;
    let client = reqwest::Client::new();
    let outcome = rotate_local_admin_token(&paths, &client).await?;
    match outcome {
        RotateOutcome::Live {
            generation,
            address,
        } => {
            println!(
                "Rotated local admin token through the running server at {address} (generation {generation})."
            );
        }
        RotateOutcome::Offline { generation } => {
            println!("Rotated local admin token offline (generation {generation}).");
        }
    }
    Ok(())
}

async fn rotate_local_admin_token(
    paths: &LocalServerPaths,
    client: &reqwest::Client,
) -> Result<RotateOutcome, Box<dyn Error>> {
    match rotation_mode(paths)? {
        RotateMode::Live { discovery } => {
            rotate_local_admin_token_live(paths, &discovery, client).await
        }
        RotateMode::Offline => {
            let rotated = rotate_offline_if_no_live_server(paths, None)?;
            Ok(RotateOutcome::Offline {
                generation: rotated.generation,
            })
        }
    }
}

fn rotation_mode(paths: &LocalServerPaths) -> io::Result<RotateMode> {
    match read_live_server_discovery(paths)? {
        Some(discovery) => Ok(RotateMode::Live { discovery }),
        None => Ok(RotateMode::Offline),
    }
}

fn rotate_offline_if_no_live_server(
    paths: &LocalServerPaths,
    live_discovery: Option<&ServerDiscoveryRecord>,
) -> io::Result<LocalAdminTokenRecord> {
    if let Some(discovery) = live_discovery {
        return Err(io::Error::other(format!(
            "refusing offline rotation while a live server is discoverable at {}; use the live rotation path instead",
            discovery.address
        )));
    }
    rotate_local_admin_token_offline(paths)
}

async fn rotate_local_admin_token_live(
    paths: &LocalServerPaths,
    discovery: &ServerDiscoveryRecord,
    client: &reqwest::Client,
) -> Result<RotateOutcome, Box<dyn Error>> {
    let current = load_local_admin_token(paths)?;
    let connect_address = normalize_loopback_connect_address(&discovery.address)?;
    let response = client
        .post(format!("http://{connect_address}/api/system/token/rotate"))
        .bearer_auth(&current.token)
        .send()
        .await?;
    let response = response.error_for_status()?;
    let body = response.json::<RotateLocalAdminTokenResponse>().await?;
    let rotated = load_local_admin_token(paths)?;
    if rotated.generation != body.generation {
        return Err(io::Error::other(format!(
            "token generation mismatch after live rotation: server reported {}, file now contains {}",
            body.generation, rotated.generation
        ))
        .into());
    }
    Ok(RotateOutcome::Live {
        generation: body.generation,
        address: connect_address,
    })
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::sync::Arc;
    use std::time::Duration;

    use clap::Parser;
    use nimbus::Service;
    use nimbus_server::{
        LocalServerPaths, LocalServerSecurityState, ServeOptions, ServerDiscoveryRecord,
        load_local_admin_token, load_or_create_local_admin_token, serve_with_options,
    };
    use nimbus_testing::wait_for_condition;

    use super::*;
    use crate::{Cli, Command};

    fn sample_paths(root: &std::path::Path) -> LocalServerPaths {
        LocalServerPaths {
            auth_token_path: root.join("auth").join("token"),
            server_discovery_path: root.join("run").join("server.json"),
            audit_log_path: root.join("logs").join("access.jsonl"),
        }
    }

    #[test]
    fn cli_parses_token_rotate_subcommand() {
        let cli = Cli::parse_from(["nimbus", "token", "rotate"]);
        match cli.command {
            Command::Token(TokenCommand::Rotate(_)) => {}
            other => panic!("expected token rotate command, got {other:?}"),
        }
    }

    #[test]
    fn offline_rotation_refuses_when_live_server_is_discoverable() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let _record = load_or_create_local_admin_token(&paths).expect("token should exist");
        let live = ServerDiscoveryRecord {
            pid: std::process::id(),
            address: "127.0.0.1:3210".to_string(),
            started_at: "2026-04-23T00:00:00Z".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_versions: vec!["nimbus.v1".to_string()],
        };

        let error = rotate_offline_if_no_live_server(&paths, Some(&live))
            .expect_err("offline rotation should refuse to race a live server");

        assert!(error.to_string().contains("refusing offline rotation"));
    }

    #[test]
    fn live_rotation_normalizes_wildcard_discovery_addresses_to_loopback() {
        let normalized =
            normalize_loopback_connect_address("0.0.0.0:3210").expect("address should normalize");
        assert_eq!(normalized, "127.0.0.1:3210");

        let ipv6 = normalize_loopback_connect_address("[::]:3210")
            .expect("ipv6 wildcard address should normalize");
        assert_eq!(ipv6, "[::1]:3210");
    }

    #[tokio::test]
    async fn live_rotation_calls_running_server_and_updates_token_file() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let current = load_or_create_local_admin_token(&paths).expect("token should exist");
        let local_server_security = Arc::new(LocalServerSecurityState::new(
            paths.clone(),
            current.clone(),
        ));
        let service =
            Arc::new(Service::new(temp.path().join("data")).expect("service should initialize"));
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("listener should bind");
        let address = listener
            .local_addr()
            .expect("listener address should resolve");
        let server_task = tokio::spawn(serve_with_options(
            listener,
            service.clone(),
            ServeOptions::default().with_local_server_security(local_server_security),
        ));
        let client = reqwest::Client::new();
        wait_for_condition(
            "local admin rotate test server should answer health checks",
            Duration::from_secs(5),
            Duration::from_millis(50),
            || async {
                client
                    .get(format!("http://{address}/health"))
                    .send()
                    .await
                    .map(|response| response.status().is_success())
                    .unwrap_or(false)
            },
        )
        .await;

        let discovery = ServerDiscoveryRecord {
            pid: std::process::id(),
            address: address.to_string(),
            started_at: "2026-04-23T00:00:00Z".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_versions: vec!["nimbus.v1".to_string()],
        };
        let outcome = rotate_local_admin_token_live(&paths, &discovery, &client)
            .await
            .expect("live rotation should succeed");

        match outcome {
            RotateOutcome::Live { generation, .. } => {
                assert_eq!(generation, current.generation + 1);
            }
            RotateOutcome::Offline { .. } => panic!("expected live rotation outcome"),
        }
        assert_eq!(
            load_local_admin_token(&paths)
                .expect("rotated token should load")
                .generation,
            current.generation + 1
        );

        server_task.abort();
        let _ = server_task.await;
        service.quiesce().await;
    }
}
