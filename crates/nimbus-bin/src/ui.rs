use std::error::Error;
use std::fmt;
use std::io;

use clap::Args;
use nimbus_server::{LocalServerPaths, ServerDiscoveryRecord, read_live_server_discovery};

use crate::local_server_client::normalize_loopback_connect_address;

#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = crate::cli_ux::UI_HELP_EXAMPLES,
)]
pub(crate) struct UiCommand {}

pub(crate) enum UiError {
    ServerNotRunning,
    Io(io::Error),
    Address(io::Error),
    Open(io::Error),
}

impl fmt::Debug for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UiError::ServerNotRunning => write!(
                f,
                "Nimbus server is not running. Start one with `nimbus start` (in another terminal) for production-shaped startup, or `nimbus dev --open` for a dev loop that opens the operator console for you."
            ),
            UiError::Io(error) => write!(f, "failed to read server discovery state: {error}"),
            UiError::Address(error) => write!(f, "server discovery address invalid: {error}"),
            UiError::Open(error) => write!(f, "failed to open browser: {error}"),
        }
    }
}

impl Error for UiError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            UiError::ServerNotRunning => None,
            UiError::Io(error) | UiError::Address(error) | UiError::Open(error) => Some(error),
        }
    }
}

pub(crate) async fn run_ui_command(_command: UiCommand) -> Result<(), Box<dyn Error>> {
    let paths = LocalServerPaths::resolve_for_current_platform()?;
    let discovery = resolve_discovery(&paths).await?;
    let url = build_ui_url(&discovery)?;
    let opened_with = open_in_preferred_browser(&url)?;
    match opened_with {
        OpenedBrowser::Chromium(label) => {
            println!("Opening Nimbus UI in {label} at {url}");
        }
        OpenedBrowser::SystemDefault => {
            println!("Opening Nimbus UI at {url}");
        }
    }
    Ok(())
}

#[derive(Debug)]
enum OpenedBrowser {
    Chromium(&'static str),
    SystemDefault,
}

fn open_in_preferred_browser(url: &str) -> Result<OpenedBrowser, UiError> {
    for candidate in CHROMIUM_CANDIDATES {
        if open::with(url, candidate.app).is_ok() {
            return Ok(OpenedBrowser::Chromium(candidate.label));
        }
    }
    open::that(url).map_err(UiError::Open)?;
    Ok(OpenedBrowser::SystemDefault)
}

struct ChromiumCandidate {
    label: &'static str,
    app: &'static str,
}

#[cfg(target_os = "macos")]
const CHROMIUM_CANDIDATES: &[ChromiumCandidate] = &[
    ChromiumCandidate {
        label: "Google Chrome",
        app: "Google Chrome",
    },
    ChromiumCandidate {
        label: "Chromium",
        app: "Chromium",
    },
    ChromiumCandidate {
        label: "Microsoft Edge",
        app: "Microsoft Edge",
    },
];

#[cfg(target_os = "linux")]
const CHROMIUM_CANDIDATES: &[ChromiumCandidate] = &[
    ChromiumCandidate {
        label: "Google Chrome",
        app: "google-chrome",
    },
    ChromiumCandidate {
        label: "Google Chrome",
        app: "google-chrome-stable",
    },
    ChromiumCandidate {
        label: "Chromium",
        app: "chromium",
    },
    ChromiumCandidate {
        label: "Chromium",
        app: "chromium-browser",
    },
    ChromiumCandidate {
        label: "Microsoft Edge",
        app: "microsoft-edge",
    },
];

#[cfg(target_os = "windows")]
const CHROMIUM_CANDIDATES: &[ChromiumCandidate] = &[
    ChromiumCandidate {
        label: "Google Chrome",
        app: "chrome",
    },
    ChromiumCandidate {
        label: "Microsoft Edge",
        app: "msedge",
    },
];

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
const CHROMIUM_CANDIDATES: &[ChromiumCandidate] = &[];

async fn resolve_discovery(paths: &LocalServerPaths) -> Result<ServerDiscoveryRecord, UiError> {
    if let Some(record) = read_live_server_discovery(paths).map_err(UiError::Io)? {
        return Ok(record);
    }
    Err(UiError::ServerNotRunning)
}

fn build_ui_url(record: &ServerDiscoveryRecord) -> Result<String, UiError> {
    let address = normalize_loopback_connect_address(&record.address).map_err(UiError::Address)?;
    Ok(format!("http://{address}/ui/"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nimbus::Service;
    use nimbus_server::{
        LocalServerPaths, LocalServerSecurityState, ServeOptions, load_or_create_local_admin_token,
        serve_with_options,
    };
    use nimbus_testing::wait_for_condition;
    use std::net::Ipv4Addr;
    use std::time::Duration;

    use super::*;

    fn sample_paths(root: &std::path::Path) -> LocalServerPaths {
        LocalServerPaths {
            auth_token_path: root.join("auth").join("token"),
            server_discovery_path: root.join("run").join("server.json"),
            audit_log_path: root.join("logs").join("access.jsonl"),
        }
    }

    #[tokio::test]
    async fn ui_command_without_running_server_returns_actionable_error() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let error = resolve_discovery(&paths)
            .await
            .expect_err("missing server should produce error");
        let message = error.to_string();
        assert!(
            matches!(error, UiError::ServerNotRunning),
            "expected ServerNotRunning, got {error}"
        );
        assert!(
            !message.contains("--ensure"),
            "post-CD5 error must not reference the removed --ensure flag, got: {message}"
        );
        assert!(
            message.contains("nimbus start"),
            "error should mention `nimbus start`, got: {message}"
        );
        assert!(
            message.contains("nimbus dev --open"),
            "error should point at `nimbus dev --open` as the spawn-and-open shortcut, got: {message}"
        );
    }

    #[tokio::test]
    async fn ui_command_resolves_live_discovery_record() {
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
            "ui resolver test server should answer health checks",
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

        let lease = nimbus_server::ServerDiscoveryLease::acquire(&paths, address)
            .expect("discovery lease should write");

        let resolved = resolve_discovery(&paths)
            .await
            .expect("live server should resolve");
        assert_eq!(resolved.address, address.to_string());

        let url = build_ui_url(&resolved).expect("url should build");
        assert!(url.starts_with("http://127.0.0.1:"), "url was: {url}");
        assert!(url.ends_with("/ui/"), "url was: {url}");

        drop(lease);
        server_task.abort();
        let _ = server_task.await;
        service.quiesce().await;
    }

    #[test]
    fn build_ui_url_normalizes_wildcard_address() {
        let record = ServerDiscoveryRecord {
            pid: std::process::id(),
            address: "0.0.0.0:8080".to_string(),
            started_at: "2026-05-15T00:00:00Z".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_versions: vec!["nimbus.v2".to_string()],
        };
        let url = build_ui_url(&record).expect("url should build");
        assert_eq!(url, "http://127.0.0.1:8080/ui/");
    }
}
