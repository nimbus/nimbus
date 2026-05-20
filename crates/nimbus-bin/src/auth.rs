use std::error::Error;
use std::fmt;
use std::io;

use clap::{Args, Subcommand};
use nimbus_server::{LocalServerPaths, read_live_server_discovery};

use crate::local_server_client::LocalServerHttpClient;

#[derive(Debug, Subcommand)]
pub(crate) enum AuthCommand {
    /// Mint a single-use launch URL for the local operator console.
    Url(AuthUrlCommand),
    /// Store a deploy bearer for a remote Nimbus daemon.
    Login(AuthLoginCommand),
    /// List configured deploy connections.
    Status(AuthStatusCommand),
    /// Remove a stored deploy bearer.
    Logout(AuthLogoutCommand),
}

#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = crate::cli_ux::AUTH_URL_HELP_EXAMPLES,
)]
pub(crate) struct AuthUrlCommand {
    /// Copy the launch URL to the OS clipboard instead of (only) printing it.
    #[arg(long)]
    pub(crate) copy: bool,
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct AuthLoginCommand {
    /// Daemon URL to authenticate against (e.g. https://nimbus.example.com).
    #[arg(long)]
    pub(crate) url: String,
    /// Deploy bearer token. If omitted, read from stdin.
    #[arg(long)]
    pub(crate) bearer: Option<String>,
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct AuthStatusCommand {}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct AuthLogoutCommand {
    /// Daemon URL whose stored bearer should be removed.
    #[arg(long)]
    pub(crate) url: String,
}

pub(crate) enum AuthUrlError {
    ServerNotRunning,
    Io(io::Error),
    Mint(nimbus::Error),
    Clipboard(String),
}

impl fmt::Debug for AuthUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for AuthUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthUrlError::ServerNotRunning => write!(
                f,
                "Nimbus server is not running. Start one with `nimbus start` (or `nimbus dev` for a watched dev loop) and re-run `nimbus auth url`."
            ),
            AuthUrlError::Io(error) => {
                write!(f, "failed to read server discovery state: {error}")
            }
            AuthUrlError::Mint(error) => {
                write!(f, "failed to mint launch ticket: {error}")
            }
            AuthUrlError::Clipboard(message) => {
                write!(f, "failed to copy launch URL to clipboard: {message}")
            }
        }
    }
}

impl Error for AuthUrlError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            AuthUrlError::ServerNotRunning | AuthUrlError::Clipboard(_) => None,
            AuthUrlError::Io(error) => Some(error),
            AuthUrlError::Mint(error) => Some(error),
        }
    }
}

pub(crate) async fn run_auth_command(command: AuthCommand) -> Result<(), Box<dyn Error>> {
    match command {
        AuthCommand::Url(command) => run_auth_url_command(command).await,
        AuthCommand::Login(_) | AuthCommand::Status(_) | AuthCommand::Logout(_) => {
            Err(Box::new(io::Error::other(
                "nimbus auth login/status/logout ships in DA8 (deploy auth credentials file); \
             not yet wired up — see docs/plans/desktop-auth-dx-plan.md DEP1-DEP4.",
            )))
        }
    }
}

async fn run_auth_url_command(command: AuthUrlCommand) -> Result<(), Box<dyn Error>> {
    let paths = LocalServerPaths::resolve_for_current_platform()?;
    let url = resolve_launch_url(&paths).await?;
    println!("{url}");
    if command.copy {
        copy_to_clipboard(&url).map_err(AuthUrlError::Clipboard)?;
        eprintln!("(launch URL copied to clipboard)");
    }
    Ok(())
}

pub(crate) async fn resolve_launch_url(paths: &LocalServerPaths) -> Result<String, AuthUrlError> {
    if read_live_server_discovery(paths)
        .map_err(AuthUrlError::Io)?
        .is_none()
    {
        return Err(AuthUrlError::ServerNotRunning);
    }
    let client = LocalServerHttpClient::discover(paths, reqwest::Client::new())
        .map_err(AuthUrlError::Mint)?
        .ok_or(AuthUrlError::ServerNotRunning)?;
    let minted = client
        .mint_ui_launch_ticket()
        .await
        .map_err(AuthUrlError::Mint)?;
    Ok(format!("{}{}", client.base_url(), minted.url))
}

#[cfg(not(test))]
fn copy_to_clipboard(value: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let (program, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("pbcopy", &[])
    } else if cfg!(target_os = "linux") {
        ("xclip", &["-selection", "clipboard"])
    } else if cfg!(target_os = "windows") {
        ("clip", &[])
    } else {
        return Err(format!(
            "no clipboard helper known for {} — paste manually",
            std::env::consts::OS
        ));
    };

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not spawn {program}: {error}"))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| format!("could not open stdin for {program}"))?;
        stdin
            .write_all(value.as_bytes())
            .map_err(|error| format!("could not write to {program}: {error}"))?;
    }
    let status = child
        .wait()
        .map_err(|error| format!("{program} did not exit cleanly: {error}"))?;
    if !status.success() {
        return Err(format!("{program} exited with status {status}"));
    }
    Ok(())
}

#[cfg(test)]
fn copy_to_clipboard(_value: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::sync::Arc;
    use std::time::Duration;

    use clap::Parser;
    use nimbus::Service;
    use nimbus_server::{
        LocalServerPaths, LocalServerSecurityState, ServeOptions, ServerDiscoveryLease,
        load_or_create_local_admin_token, serve_with_options,
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
    fn cli_parses_auth_url_subcommand() {
        let cli = Cli::parse_from(["nimbus", "auth", "url"]);
        match cli.command {
            Command::Auth(AuthCommand::Url(command)) => {
                assert!(!command.copy, "url default should not copy");
            }
            other => panic!("expected auth url command, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_auth_url_copy_flag() {
        let cli = Cli::parse_from(["nimbus", "auth", "url", "--copy"]);
        match cli.command {
            Command::Auth(AuthCommand::Url(command)) => assert!(command.copy),
            other => panic!("expected auth url --copy, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_auth_login_with_url_and_bearer() {
        let cli = Cli::parse_from([
            "nimbus",
            "auth",
            "login",
            "--url",
            "https://nimbus.example.com",
            "--bearer",
            "deploy_tok_abc",
        ]);
        match cli.command {
            Command::Auth(AuthCommand::Login(command)) => {
                assert_eq!(command.url, "https://nimbus.example.com");
                assert_eq!(command.bearer.as_deref(), Some("deploy_tok_abc"));
            }
            other => panic!("expected auth login command, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_auth_status_and_logout() {
        let status = Cli::parse_from(["nimbus", "auth", "status"]);
        assert!(matches!(
            status.command,
            Command::Auth(AuthCommand::Status(_))
        ));

        let logout = Cli::parse_from([
            "nimbus",
            "auth",
            "logout",
            "--url",
            "https://nimbus.example.com",
        ]);
        match logout.command {
            Command::Auth(AuthCommand::Logout(command)) => {
                assert_eq!(command.url, "https://nimbus.example.com");
            }
            other => panic!("expected auth logout command, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn auth_url_without_running_server_returns_actionable_error() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let error = resolve_launch_url(&paths)
            .await
            .expect_err("missing server should produce error");
        assert!(
            matches!(error, AuthUrlError::ServerNotRunning),
            "expected ServerNotRunning, got {error}"
        );
        let message = error.to_string();
        assert!(
            message.contains("nimbus start"),
            "error should mention `nimbus start`, got: {message}"
        );
        assert!(
            message.contains("nimbus dev"),
            "error should point at `nimbus dev`, got: {message}"
        );
    }

    #[tokio::test]
    async fn auth_url_against_live_server_returns_launch_url() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let token =
            load_or_create_local_admin_token(&paths).expect("local admin token should initialize");
        let local_server_security = Arc::new(LocalServerSecurityState::new(paths.clone(), token));
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
            "auth url test server should answer health checks",
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

        let lease =
            ServerDiscoveryLease::acquire(&paths, address).expect("discovery lease should write");

        let url = resolve_launch_url(&paths)
            .await
            .expect("auth url should mint against the live server");
        assert!(
            url.starts_with("http://127.0.0.1:"),
            "url should be a loopback http URL, got: {url}"
        );
        assert!(
            url.contains("/ui/launch?lt=nimbus_lt_"),
            "url should embed a launch ticket of the nimbus_lt_ shape, got: {url}"
        );

        drop(lease);
        server_task.abort();
        let _ = server_task.await;
        service.quiesce().await;
    }
}
