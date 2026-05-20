use std::error::Error;
use std::fmt;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;

use clap::{Args, Subcommand};
use nimbus_server::{
    LocalServerPaths, load_local_admin_token, read_live_server_discovery,
    rotate_local_admin_token_offline,
};

use crate::credentials::{
    self, ConnectionEntry, CredentialsFile, default_credentials_path, find_connection, mask_bearer,
    read_credentials_file, remove_connection, upsert_connection, write_credentials_file,
};
use crate::local_server_client::LocalServerHttpClient;

#[derive(Debug, Subcommand)]
pub(crate) enum AuthCommand {
    /// Mint a single-use launch URL for the local operator console.
    Url(AuthUrlCommand),
    /// Print (or copy) the local admin token from the on-disk token file.
    Token(AuthTokenCommand),
    /// Store a deploy bearer for a remote Nimbus daemon.
    Login(AuthLoginCommand),
    /// List configured deploy connections.
    Status(AuthStatusCommand),
    /// Remove a stored deploy bearer.
    Logout(AuthLogoutCommand),
    /// Rotate the local admin token (required before `nimbus start --allow-network`).
    #[command(name = "rotate-admin")]
    RotateAdmin(AuthRotateAdminCommand),
}

#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = crate::cli_ux::AUTH_URL_HELP_EXAMPLES,
)]
pub(crate) struct AuthUrlCommand {
    /// Copy the launch URL to the OS clipboard in addition to printing it.
    #[arg(long)]
    pub(crate) copy: bool,
    /// Open the launch URL in the default browser in addition to printing it.
    #[arg(long)]
    pub(crate) open: bool,
}

#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = crate::cli_ux::AUTH_TOKEN_HELP_EXAMPLES,
)]
pub(crate) struct AuthTokenCommand {
    /// Copy the token to the OS clipboard in addition to printing it.
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

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct AuthRotateAdminCommand {}

pub(crate) enum AuthUrlError {
    ServerNotRunning,
    Io(io::Error),
    Mint(nimbus::Error),
    Clipboard(String),
    OpenBrowser(io::Error),
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
            AuthUrlError::OpenBrowser(error) => {
                write!(f, "failed to open launch URL in browser: {error}")
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
            AuthUrlError::OpenBrowser(error) => Some(error),
        }
    }
}

pub(crate) enum AuthTokenError {
    Io(io::Error),
    Clipboard(String),
}

impl fmt::Debug for AuthTokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for AuthTokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthTokenError::Io(error) => {
                write!(f, "failed to read local admin token: {error}")
            }
            AuthTokenError::Clipboard(message) => {
                write!(f, "failed to copy token to clipboard: {message}")
            }
        }
    }
}

impl Error for AuthTokenError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            AuthTokenError::Io(error) => Some(error),
            AuthTokenError::Clipboard(_) => None,
        }
    }
}

pub(crate) async fn run_auth_command(command: AuthCommand) -> Result<(), Box<dyn Error>> {
    match command {
        AuthCommand::Url(command) => run_auth_url_command(command).await,
        AuthCommand::Token(command) => run_auth_token_command(command),
        AuthCommand::Login(command) => run_auth_login_command(command),
        AuthCommand::Status(command) => run_auth_status_command(command),
        AuthCommand::Logout(command) => run_auth_logout_command(command),
        AuthCommand::RotateAdmin(command) => run_auth_rotate_admin_command(command),
    }
}

fn run_auth_rotate_admin_command(_command: AuthRotateAdminCommand) -> Result<(), Box<dyn Error>> {
    let paths = LocalServerPaths::resolve_for_current_platform()?;
    let rotated = rotate_local_admin_token_offline(&paths)?;
    println!(
        "Rotated local admin token (generation {} → token file: {}).",
        rotated.generation,
        paths.auth_token_path.display()
    );
    if let Some(rotated_at) = rotated.rotated_at.as_deref() {
        println!("rotated_at: {rotated_at}");
    }
    println!(
        "The new token is on disk. Any running `nimbus start` daemon keeps its in-memory token until restart — restart it to invalidate existing sign-in sessions and launch tickets, then run `nimbus auth url` to mint a fresh launch URL."
    );
    Ok(())
}

fn run_auth_login_command(command: AuthLoginCommand) -> Result<(), Box<dyn Error>> {
    let path = default_credentials_path()?;
    let bearer = resolve_login_bearer(command.bearer.as_deref(), &mut io::stdin().lock())?;
    let mut file = read_credentials_file(&path)?;
    upsert_connection(&mut file, &command.url, bearer.clone(), None, false)?;
    write_credentials_file(&path, &file)?;
    let normalized = credentials::normalize_url(&command.url);
    println!(
        "Stored bearer for {normalized} (mask: {})",
        mask_bearer(&bearer)
    );
    println!("Credentials file: {}", path.display());
    Ok(())
}

fn run_auth_status_command(_command: AuthStatusCommand) -> Result<(), Box<dyn Error>> {
    let path = default_credentials_path()?;
    let file = read_credentials_file(&path)?;
    print_auth_status(&file, &path, &mut io::stdout().lock())?;
    Ok(())
}

fn run_auth_logout_command(command: AuthLogoutCommand) -> Result<(), Box<dyn Error>> {
    let path = default_credentials_path()?;
    let mut file = read_credentials_file(&path)?;
    let normalized = credentials::normalize_url(&command.url);
    if !remove_connection(&mut file, &command.url) {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no stored bearer for {normalized}; nothing to remove"),
        )));
    }
    write_credentials_file(&path, &file)?;
    println!("Removed bearer for {normalized}");
    Ok(())
}

fn resolve_login_bearer(
    explicit: Option<&str>,
    stdin: &mut impl BufRead,
) -> Result<String, Box<dyn Error>> {
    if let Some(bearer) = explicit {
        let trimmed = bearer.trim();
        if trimmed.is_empty() {
            return Err(Box::new(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--bearer value must not be empty",
            )));
        }
        return Ok(trimmed.to_string());
    }
    if io::stdin().is_terminal() {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no --bearer flag and stdin is a TTY — re-run with `--bearer <value>` or pipe the bearer in",
        )));
    }
    let mut buffer = String::new();
    stdin.read_to_string(&mut buffer)?;
    let trimmed = buffer.trim();
    if trimmed.is_empty() {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::InvalidInput,
            "stdin bearer was empty",
        )));
    }
    Ok(trimmed.to_string())
}

fn print_auth_status(
    file: &CredentialsFile,
    path: &std::path::Path,
    out: &mut impl Write,
) -> io::Result<()> {
    writeln!(out, "Credentials file: {}", path.display())?;
    if file.connection.is_empty() {
        writeln!(
            out,
            "No stored deploy bearers. Use `nimbus auth login --url <daemon> --bearer <value>` to add one."
        )?;
        return Ok(());
    }
    writeln!(out)?;
    for (url, entry) in &file.connection {
        writeln!(out, "  {url}")?;
        writeln!(out, "    bearer:       {}", mask_bearer(&entry.bearer))?;
        if let Some(expires) = &entry.expires_at {
            writeln!(out, "    expires_at:   {expires}")?;
        }
        if let Some(last_used) = &entry.last_used_at {
            writeln!(out, "    last_used_at: {last_used}")?;
        }
    }
    Ok(())
}

/// Read the credentials file and return the bearer + path used (if any).
/// Used by `nimbus deploy` to fall back to the credentials file when the
/// `NIMBUS_DEPLOY_TOKEN` env var is unset.
pub(crate) fn lookup_credentials_bearer(url: &str) -> io::Result<Option<(String, PathBuf)>> {
    let path = default_credentials_path()?;
    let file = read_credentials_file(&path)?;
    Ok(find_connection(&file, url).map(|entry: &ConnectionEntry| (entry.bearer.clone(), path)))
}

async fn run_auth_url_command(command: AuthUrlCommand) -> Result<(), Box<dyn Error>> {
    let paths = LocalServerPaths::resolve_for_current_platform()?;
    let url = resolve_launch_url(&paths).await?;
    println!("{url}");
    if command.copy {
        copy_to_clipboard(&url).map_err(AuthUrlError::Clipboard)?;
        eprintln!("(launch URL copied to clipboard)");
    }
    if command.open {
        open_in_browser(&url).map_err(AuthUrlError::OpenBrowser)?;
        eprintln!("(launch URL opened in browser)");
    }
    Ok(())
}

fn run_auth_token_command(command: AuthTokenCommand) -> Result<(), Box<dyn Error>> {
    let paths = LocalServerPaths::resolve_for_current_platform()?;
    let record = load_local_admin_token(&paths).map_err(AuthTokenError::Io)?;
    println!("{}", record.token);
    if command.copy {
        copy_to_clipboard(&record.token).map_err(AuthTokenError::Clipboard)?;
        eprintln!("(local admin token copied to clipboard)");
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

#[cfg(not(test))]
fn open_in_browser(url: &str) -> io::Result<()> {
    open::that(url)
}

#[cfg(test)]
fn open_in_browser(_url: &str) -> io::Result<()> {
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
                assert!(!command.open, "url default should not open the browser");
            }
            other => panic!("expected auth url command, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_auth_url_copy_flag() {
        let cli = Cli::parse_from(["nimbus", "auth", "url", "--copy"]);
        match cli.command {
            Command::Auth(AuthCommand::Url(command)) => {
                assert!(command.copy);
                assert!(!command.open);
            }
            other => panic!("expected auth url --copy, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_auth_url_open_flag() {
        let cli = Cli::parse_from(["nimbus", "auth", "url", "--open"]);
        match cli.command {
            Command::Auth(AuthCommand::Url(command)) => {
                assert!(command.open);
                assert!(!command.copy);
            }
            other => panic!("expected auth url --open, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_auth_url_open_and_copy_flags() {
        let cli = Cli::parse_from(["nimbus", "auth", "url", "--copy", "--open"]);
        match cli.command {
            Command::Auth(AuthCommand::Url(command)) => {
                assert!(command.copy, "--copy should compose with --open");
                assert!(command.open, "--open should compose with --copy");
            }
            other => panic!("expected auth url --copy --open, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_auth_token_subcommand() {
        let cli = Cli::parse_from(["nimbus", "auth", "token"]);
        match cli.command {
            Command::Auth(AuthCommand::Token(command)) => {
                assert!(!command.copy, "token default should not copy");
            }
            other => panic!("expected auth token command, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_auth_token_copy_flag() {
        let cli = Cli::parse_from(["nimbus", "auth", "token", "--copy"]);
        match cli.command {
            Command::Auth(AuthCommand::Token(command)) => assert!(command.copy),
            other => panic!("expected auth token --copy, got {other:?}"),
        }
    }

    #[test]
    fn auth_token_reads_token_from_admin_file() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let minted =
            load_or_create_local_admin_token(&paths).expect("admin token should initialize");
        let loaded = load_local_admin_token(&paths).expect("loaded token should round-trip");
        assert_eq!(
            loaded.token, minted.token,
            "auth token must read the same value that load_or_create wrote"
        );
        assert!(
            loaded.token.starts_with("nimbus_at_"),
            "token must carry the local-admin prefix, got: {}",
            loaded.token
        );
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

    #[test]
    fn cli_parses_auth_rotate_admin_subcommand() {
        let cli = Cli::parse_from(["nimbus", "auth", "rotate-admin"]);
        assert!(
            matches!(cli.command, Command::Auth(AuthCommand::RotateAdmin(_))),
            "expected auth rotate-admin command, got {:?}",
            cli.command
        );
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

    #[test]
    fn resolve_login_bearer_uses_explicit_flag_when_present() {
        let mut stdin: &[u8] = b"";
        let bearer = resolve_login_bearer(Some("deploy_tok_abc"), &mut stdin)
            .expect("explicit bearer should resolve");
        assert_eq!(bearer, "deploy_tok_abc");
    }

    #[test]
    fn resolve_login_bearer_trims_explicit_flag_value() {
        let mut stdin: &[u8] = b"";
        let bearer = resolve_login_bearer(Some("  deploy_tok_abc  "), &mut stdin)
            .expect("explicit bearer should resolve");
        assert_eq!(bearer, "deploy_tok_abc");
    }

    #[test]
    fn resolve_login_bearer_rejects_empty_explicit_value() {
        let mut stdin: &[u8] = b"";
        let error = resolve_login_bearer(Some("   "), &mut stdin)
            .expect_err("empty bearer should be rejected");
        assert!(
            error.to_string().contains("must not be empty"),
            "error should explain emptiness, got: {error}"
        );
    }

    #[test]
    fn print_auth_status_with_empty_file_prompts_for_login() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let path = temp.path().join("credentials");
        let mut buffer = Vec::new();
        print_auth_status(&CredentialsFile::default(), &path, &mut buffer)
            .expect("status should render");
        let rendered = String::from_utf8(buffer).expect("output is utf-8");
        assert!(
            rendered.contains("Credentials file:"),
            "output should label the credentials path: {rendered}"
        );
        assert!(
            rendered.contains("nimbus auth login"),
            "empty-state should recommend `nimbus auth login`: {rendered}"
        );
    }

    #[test]
    fn print_auth_status_masks_bearer_and_lists_metadata() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let path = temp.path().join("credentials");
        let mut file = CredentialsFile::default();
        upsert_connection(
            &mut file,
            "https://nimbus.example.com",
            "deploy_tok_abcdef0123456789".to_string(),
            Some("2026-12-01T00:00:00Z".to_string()),
            true,
        )
        .expect("upsert should succeed");
        let mut buffer = Vec::new();
        print_auth_status(&file, &path, &mut buffer).expect("status should render");
        let rendered = String::from_utf8(buffer).expect("output is utf-8");
        assert!(
            rendered.contains("https://nimbus.example.com"),
            "status should list the daemon URL: {rendered}"
        );
        assert!(
            rendered.contains("deploy_tok_…6789"),
            "status should mask the bearer to prefix + last four, got: {rendered}"
        );
        assert!(
            !rendered.contains("deploy_tok_abcdef0123456789"),
            "status must not leak the full bearer in cleartext, got: {rendered}"
        );
        assert!(
            rendered.contains("2026-12-01T00:00:00Z"),
            "status should show expires_at, got: {rendered}"
        );
        assert!(
            rendered.contains("last_used_at"),
            "status should show last_used_at, got: {rendered}"
        );
    }

    /// End-to-end: login → status → deploy-token-lookup → logout against a
    /// tempdir credentials path. The DEP1-DEP4 verifier from the plan.
    #[test]
    fn auth_login_status_deploy_logout_roundtrip_uses_credentials_file() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let path = temp.path().join("credentials");

        let mut file = read_credentials_file(&path).expect("missing file should read as empty");
        assert!(
            file.connection.is_empty(),
            "fresh tempdir should have no connections"
        );

        upsert_connection(
            &mut file,
            "http://localhost:3210",
            "deploy_tok_abcdef0123456789".to_string(),
            None,
            false,
        )
        .expect("login should upsert the entry");
        write_credentials_file(&path, &file).expect("login should write the file");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("credentials file should exist")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "credentials file must be mode 0600");
        }

        let loaded = read_credentials_file(&path).expect("status read should succeed");
        assert_eq!(loaded.connection.len(), 1);
        let entry =
            find_connection(&loaded, "http://localhost:3210").expect("entry should be present");
        assert_eq!(entry.bearer, "deploy_tok_abcdef0123456789");

        // Deploy lookup behavior: a daemon-URL lookup against the same file
        // must surface the stored bearer + the file path. This is the
        // contract `crate::deploy::resolve_deploy_token` relies on.
        let mut file_for_logout = read_credentials_file(&path).expect("logout read should succeed");
        assert!(
            remove_connection(&mut file_for_logout, "http://localhost:3210"),
            "logout should report that an entry was removed"
        );
        write_credentials_file(&path, &file_for_logout).expect("logout should rewrite the file");

        let after_logout = read_credentials_file(&path).expect("post-logout read should succeed");
        assert!(
            after_logout.connection.is_empty(),
            "logout should leave the credentials file empty"
        );
    }
}
