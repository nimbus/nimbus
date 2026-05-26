use std::path::{Path, PathBuf};
use std::time::Duration;

use nimbus_server::LocalServerPaths;

use crate::cli_ux;
use crate::local_server_client::LocalServerHttpClient;

const FIRST_BOOT_STAMP_NAME: &str = ".nimbus-init-stamp";
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Returns true when `<stamp_dir>/.nimbus-init-stamp` is absent. The
/// absence is a durable "data dir was just created" signal — we write
/// the stamp only after the H5 banner has fired, so a Ctrl-C before
/// the stamp lands still surfaces the banner on the next boot.
pub(super) fn is_first_boot(stamp_dir: &Path) -> bool {
    !stamp_dir.join(FIRST_BOOT_STAMP_NAME).exists()
}

pub(super) fn write_first_boot_stamp(stamp_dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(stamp_dir)?;
    let stamp_path = stamp_dir.join(FIRST_BOOT_STAMP_NAME);
    std::fs::write(&stamp_path, "")?;
    Ok(stamp_path)
}

/// Banner text emitted to stderr when the data dir is being initialized
/// for the first time. The format is intentionally close to dev's H6
/// fallback so users who already know the dev shape can read it without
/// re-learning.
pub(super) fn first_boot_banner_lines(launch_url: &str) -> Vec<String> {
    vec![
        "Welcome to Nimbus — this looks like a first boot.".to_string(),
        format!("Open this URL to sign in: {launch_url}"),
        "(this hint shows once; later starts stay quiet)".to_string(),
    ]
}

/// Spawn the first-boot announce task. It waits for the operator console
/// to start answering, mints a launch ticket, prints the banner, and only
/// then writes the stamp file. Best-effort: a probe timeout or mint
/// failure is logged but never brings the daemon down.
pub(super) fn spawn_first_boot_announce(
    console_url: String,
    local_server_paths: LocalServerPaths,
    stamp_dir: PathBuf,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(error) = announce_first_boot(&console_url, &local_server_paths, &stamp_dir).await
        {
            tracing::warn!("first-boot banner: {error}");
            let _ = cli_ux::write_stderr_line(&format!("warning: first-boot banner: {error}"));
        }
    })
}

async fn announce_first_boot(
    console_url: &str,
    local_server_paths: &LocalServerPaths,
    stamp_dir: &Path,
) -> Result<(), String> {
    wait_for_console_ready(console_url).await?;
    let launch_url = mint_first_boot_launch_url(console_url, local_server_paths).await;
    for line in first_boot_banner_lines(&launch_url) {
        let _ = cli_ux::write_stderr_line(&line);
    }
    write_first_boot_stamp(stamp_dir)
        .map(|_| ())
        .map_err(|error| format!("failed to write first-boot stamp: {error}"))
}

async fn wait_for_console_ready(console_url: &str) -> Result<(), String> {
    let deadline = std::time::Instant::now() + PROBE_TIMEOUT;
    let probe_url = format!("{console_url}auth");
    let client = reqwest::Client::new();
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "operator console did not become ready at {console_url} within {}s",
                PROBE_TIMEOUT.as_secs()
            ));
        }
        let probed = client
            .get(&probe_url)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map(|response| response.status().is_success() || response.status().is_redirection())
            .unwrap_or(false);
        if probed {
            return Ok(());
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn mint_first_boot_launch_url(
    console_url: &str,
    local_server_paths: &LocalServerPaths,
) -> String {
    let client = match LocalServerHttpClient::discover(local_server_paths, reqwest::Client::new()) {
        Ok(Some(client)) => client,
        Ok(None) => return console_url.to_string(),
        Err(error) => {
            tracing::warn!(
                "first-boot launch ticket: client unavailable ({error}); falling back to unauthenticated /ui/"
            );
            return console_url.to_string();
        }
    };
    match client.mint_ui_launch_ticket().await {
        Ok(minted) => format!("{}{}", client.base_url(), minted.url),
        Err(error) => {
            tracing::warn!(
                "first-boot launch ticket: mint failed ({error}); falling back to unauthenticated /ui/"
            );
            console_url.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::sync::Arc;

    use nimbus::Service;
    use nimbus_server::{
        LocalServerSecurityState, ServeOptions, ServerDiscoveryLease,
        load_or_create_local_admin_token, serve,
    };
    use nimbus_testing::wait_for_condition;
    use tempfile::tempdir;

    use super::*;

    fn sample_paths(root: &std::path::Path) -> LocalServerPaths {
        LocalServerPaths {
            auth_token_path: root.join("auth").join("token"),
            server_discovery_path: root.join("run").join("server.json"),
            audit_log_path: root.join("logs").join("access.jsonl"),
        }
    }

    #[test]
    fn first_boot_true_when_stamp_absent() {
        let temp = tempdir().expect("tempdir should build");
        assert!(
            is_first_boot(temp.path()),
            "fresh data dir should report first-boot"
        );
    }

    #[test]
    fn first_boot_false_after_stamp_write() {
        let temp = tempdir().expect("tempdir should build");
        assert!(is_first_boot(temp.path()));
        write_first_boot_stamp(temp.path()).expect("stamp should write");
        assert!(
            !is_first_boot(temp.path()),
            "writing the stamp should flip first-boot back to false"
        );
    }

    #[test]
    fn first_boot_stamp_path_uses_dotfile_marker() {
        let temp = tempdir().expect("tempdir should build");
        let written = write_first_boot_stamp(temp.path()).expect("stamp should write");
        assert_eq!(
            written.file_name().and_then(|name| name.to_str()),
            Some(".nimbus-init-stamp"),
            "the stamp file should use the documented dotfile name"
        );
    }

    #[test]
    fn write_first_boot_stamp_creates_missing_parent_dirs() {
        let temp = tempdir().expect("tempdir should build");
        let nested = temp.path().join("does").join("not").join("exist");
        assert!(!nested.exists(), "nested parent should not yet exist");
        write_first_boot_stamp(&nested).expect("stamp should write into freshly-built parent");
        assert!(
            nested.join(".nimbus-init-stamp").exists(),
            "stamp file should be present"
        );
    }

    #[test]
    fn banner_lines_include_launch_url_and_setup_hint() {
        let lines = first_boot_banner_lines("http://127.0.0.1:8080/ui/launch?lt=nimbus_lt_demo");
        assert_eq!(lines.len(), 3);
        assert!(
            lines[0].contains("first boot"),
            "first line should call out the first-boot context: {:?}",
            lines[0]
        );
        assert!(
            lines[1].starts_with("Open this URL to sign in: ")
                && lines[1].contains("nimbus_lt_demo"),
            "second line should hand the user the launch URL: {:?}",
            lines[1]
        );
        assert!(
            lines[2].contains("shows once"),
            "third line should warn the hint is one-shot: {:?}",
            lines[2]
        );
    }

    /// End-to-end: spawn a live local server, run `announce_first_boot`
    /// against it, then assert that the stamp file lands and a second
    /// invocation would observe `is_first_boot == false`. This is the
    /// "second boot stays quiet" promise from the DA4 spec.
    #[tokio::test]
    async fn announce_first_boot_writes_stamp_after_minting_against_live_server() {
        let temp = tempdir().expect("tempdir should build");
        let stamp_dir = temp.path().join("data");
        std::fs::create_dir_all(&stamp_dir).expect("stamp dir should build");
        let paths_root = temp.path().join("paths");
        let paths = sample_paths(&paths_root);

        let token =
            load_or_create_local_admin_token(&paths).expect("local admin token should initialize");
        let local_server_security = Arc::new(LocalServerSecurityState::new(paths.clone(), token));
        let service =
            Arc::new(Service::new(temp.path().join("service")).expect("service should initialize"));
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("listener should bind");
        let address = listener
            .local_addr()
            .expect("listener address should resolve");
        let server_task = tokio::spawn(serve(
            listener,
            ServeOptions::new(service.clone()).with_local_server_security(local_server_security),
        ));
        let client = reqwest::Client::new();
        wait_for_condition(
            "first-boot test server should answer /health",
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

        assert!(
            is_first_boot(&stamp_dir),
            "stamp file should be absent before announce runs"
        );
        let console_url = format!("http://{address}/ui/");
        announce_first_boot(&console_url, &paths, &stamp_dir)
            .await
            .expect("first-boot announce should succeed against a live server");
        assert!(
            !is_first_boot(&stamp_dir),
            "stamp file should exist after a successful announce — second boot should stay quiet"
        );

        drop(lease);
        server_task.abort();
        let _ = server_task.await;
        service.quiesce().await;
    }
}
