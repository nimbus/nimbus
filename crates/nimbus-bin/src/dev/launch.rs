use std::env;
use std::time::Duration;

use crate::cli_ux;

/// Result of the M1 smart-detect ladder. Either auto-open is on, or it
/// is off with a reason worth surfacing under the H6 banner so the user
/// knows why the browser didn't pop.
#[derive(Debug, Clone)]
pub(super) struct AutoOpenDecision {
    pub(super) auto_open: bool,
    pub(super) reason: Option<String>,
}

impl AutoOpenDecision {
    pub(super) fn open() -> Self {
        Self {
            auto_open: true,
            reason: None,
        }
    }

    pub(super) fn suppressed(reason: impl Into<String>) -> Self {
        Self {
            auto_open: false,
            reason: Some(reason.into()),
        }
    }
}

/// Smart-detect M1: only auto-open when stdout is a TTY, `$CI` is
/// unset, `$NO_BROWSER` is unset, and the user did not pass `--no-open`.
/// The detector takes its inputs explicitly so tests can drive every
/// branch without poking process env / file descriptors.
pub(super) fn resolve_auto_open(
    no_open: bool,
    stdout_is_tty: bool,
    env: &dyn EnvLookup,
) -> AutoOpenDecision {
    if no_open {
        return AutoOpenDecision::suppressed("--no-open");
    }
    if env.get("CI").is_some() {
        return AutoOpenDecision::suppressed("$CI is set");
    }
    if env.get("NO_BROWSER").is_some() {
        return AutoOpenDecision::suppressed("$NO_BROWSER is set");
    }
    if !stdout_is_tty {
        return AutoOpenDecision::suppressed("stdout is not a TTY");
    }
    AutoOpenDecision::open()
}

pub(super) trait EnvLookup {
    fn get(&self, key: &str) -> Option<String>;
}

pub(super) struct ProcessEnv;

impl EnvLookup for ProcessEnv {
    fn get(&self, key: &str) -> Option<String> {
        env::var(key).ok().filter(|value| !value.is_empty())
    }
}

/// Wait for the operator console to start answering at `<url>auth`, then
/// mint a single-use launch ticket from the local server. If auto-open
/// is allowed (interactive TTY, `$CI` and `$NO_BROWSER` unset, user did
/// not pass `--no-open`), spawn the OS browser at the launch URL so the
/// user lands already signed in. Otherwise print the H6 banner with the
/// launch URL so the user can copy/paste it. Best-effort: a launcher
/// failure or a probe timeout is reported via `tracing::error!` plus a
/// stderr line and does not bring the daemon down.
pub(super) async fn announce_launch_url_when_ready(
    console_url: String,
    decision: AutoOpenDecision,
) {
    const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
    const POLL_INTERVAL: Duration = Duration::from_millis(200);
    let deadline = std::time::Instant::now() + PROBE_TIMEOUT;
    let probe_url = format!("{}auth", console_url);
    let client = reqwest::Client::new();
    loop {
        if std::time::Instant::now() >= deadline {
            let message = format!(
                "operator console did not become ready at {console_url} within {}s; daemon is reachable at {console_url}",
                PROBE_TIMEOUT.as_secs()
            );
            tracing::error!("{message}");
            let _ = cli_ux::write_stderr_line(&format!("error: {message}"));
            return;
        }
        let probed = client
            .get(&probe_url)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map(|response| response.status().is_success() || response.status().is_redirection())
            .unwrap_or(false);
        if probed {
            break;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    let launch_url = mint_launch_url_or_fallback(&console_url).await;
    if decision.auto_open {
        if let Err(error) = open::that(&launch_url) {
            tracing::error!("browser launcher failed: {error}; falling back to printed URL");
            let _ = cli_ux::write_stderr_line(&format!("error: browser launcher failed: {error}"));
            let _ = cli_ux::write_stderr_line(&format!("Open this URL to sign in: {launch_url}"));
        }
        return;
    }
    let _ = cli_ux::write_stderr_line(&format!("Open this URL to sign in: {launch_url}"));
    if let Some(reason) = decision.reason {
        let _ = cli_ux::write_stderr_line(&format!("(auto-open suppressed: {reason})"));
    }
}

async fn mint_launch_url_or_fallback(console_url: &str) -> String {
    let paths = match nimbus_server::LocalServerPaths::resolve_for_current_platform() {
        Ok(paths) => paths,
        Err(error) => {
            tracing::warn!(
                "launch ticket: unable to resolve local server paths for mint ({error}); falling back to unauthenticated /ui/"
            );
            return console_url.to_string();
        }
    };
    let client = match crate::local_server_client::LocalServerHttpClient::discover(
        &paths,
        reqwest::Client::new(),
    ) {
        Ok(Some(client)) => client,
        Ok(None) => return console_url.to_string(),
        Err(error) => {
            tracing::warn!(
                "launch ticket: client unavailable ({error}); falling back to unauthenticated /ui/"
            );
            return console_url.to_string();
        }
    };
    match client.mint_ui_launch_ticket().await {
        Ok(minted) => format!("{}{}", client.base_url(), minted.url),
        Err(error) => {
            tracing::warn!(
                "launch ticket: mint failed ({error}); falling back to unauthenticated /ui/"
            );
            console_url.to_string()
        }
    }
}

/// Append `/ui/` to the daemon's base URL so the dev banner can advertise the
/// operator console without the caller having to assemble paths. Mirrors
/// the CockroachDB precedent (`webui:\t<url>`) — see CD3 in
/// `docs/private/plans/cli-daemon-canonicalization-plan.md`.
pub(super) fn operator_console_url(local_url: &str) -> String {
    let trimmed = local_url.trim_end_matches('/');
    format!("{trimmed}/ui/")
}
