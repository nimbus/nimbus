use std::env;
use std::io;
use std::path::PathBuf;

use clap::{Args, ValueEnum};
use nimbus_operator::LocalNodeNetworkRoot;

use crate::cli_ux;
use crate::network_composition::StagedLocalNetworkComposition;
use crate::node_runtime;
use crate::provision;
use crate::start::{
    StartCommand, network_root_from_start_command, run_start_command_with_prepared_network,
};

mod adapter;
mod banner;
mod env_file;
mod firebase;
mod firebase_project;
mod firebase_scan;
mod launch;
mod plan;
mod redetect;
mod surfaces;
mod watch;
mod wire;

use adapter::DevAdapter;
use banner::emit_dev_banner;
use env_file::write_env_local_deployment;
use launch::{announce_launch_url_when_ready, operator_console_url};
#[cfg(test)]
use plan::resolve_dev_plan;
use plan::resolve_dev_plan_with_staged_network;
use watch::run_dev_watch_loop;

const DEFAULT_DEV_PORT: u16 = 3210;
/// Start a local development server with watched codegen and dev defaults.
#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = crate::cli_ux::DEV_HELP_EXAMPLES
)]
pub(crate) struct DevCommand {
    /// Port to listen on.
    #[arg(long, default_value_t = DEFAULT_DEV_PORT)]
    pub(crate) port: u16,

    /// App directory containing an adapter source root.
    #[arg(long)]
    pub(crate) app_dir: Option<PathBuf>,

    /// Optional ordered Compose file list that declares local service
    /// dependencies. Repeat `--compose-file` to merge overlays. When omitted,
    /// Nimbus uses `COMPOSE_FILE` when set, then discovers from the current
    /// directory and parent directories.
    #[arg(long)]
    pub(crate) compose_file: Vec<PathBuf>,

    /// Skip `COMPOSE_FILE` and walk-up Compose discovery for this dev session.
    /// Do not combine this option with `--compose-file`.
    #[arg(long, default_value_t = false, conflicts_with = "compose_file")]
    pub(crate) no_compose_discovery: bool,

    /// Run startup only, without the watched codegen loop.
    #[arg(long, default_value_t = false)]
    pub(crate) once: bool,

    /// Skip initial codegen before starting the local server. Watched reruns still use codegen.
    #[arg(long, default_value_t = false)]
    pub(crate) skip_codegen: bool,

    /// Diagnose Node.js builtin imports that should move behind "use node".
    #[arg(long, default_value_t = false)]
    pub(crate) debug_node_apis: bool,

    /// Runtime log tailing mode. Log multiplexing is pending runtime log plumbing.
    #[arg(long, value_enum, default_value_t = DevTailLogsMode::PauseOnSync)]
    pub(crate) tail_logs: DevTailLogsMode,

    /// Local dev persistence root for tenant data.
    #[arg(long)]
    pub(crate) data_dir: Option<PathBuf>,

    /// Optional local dev control-plane persistence root. Defaults to the
    /// tenant data root when omitted.
    #[arg(long)]
    pub(crate) control_data_dir: Option<PathBuf>,

    /// Stable OS-node root for host-global network allocation authority.
    ///
    /// This is independent of the app and local dev persistence roots.
    #[arg(long)]
    pub(crate) network_state_dir: Option<PathBuf>,

    /// Suppress the default browser auto-open and print a one-line launch
    /// URL banner instead. Auto-open is also suppressed automatically in
    /// non-interactive environments (when `$CI` or `$NO_BROWSER` is set,
    /// or stdout is not a TTY); in those cases the same banner is printed.
    #[arg(long, default_value_t = false)]
    pub(crate) no_open: bool,
}

fn resolve_dev_network_root(command: &DevCommand) -> nimbus::Result<LocalNodeNetworkRoot> {
    network_root_from_start_command(&StartCommand {
        network_state_dir: command.network_state_dir.clone(),
        ..StartCommand::default()
    })
}

pub(crate) async fn run_dev_command(command: DevCommand) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    let skip_codegen = command.skip_codegen;

    if let Some(app_dir) = command.app_dir.as_deref() {
        let resolved = if app_dir.is_absolute() {
            app_dir.to_path_buf()
        } else {
            cwd.join(app_dir)
        };
        if !resolved.exists() {
            std::fs::create_dir_all(&resolved).map_err(|e| {
                io::Error::other(format!(
                    "failed to create --app-dir {}: {e}",
                    resolved.display()
                ))
            })?;
        }
    }

    let network_root = resolve_dev_network_root(&command)?;
    let staged_network = StagedLocalNetworkComposition::claim(&network_root)?;
    let (plan, prepared_network) =
        resolve_dev_plan_with_staged_network(command, &cwd, staged_network)?;
    tracing::debug!(
        mongodb = plan.wire_surfaces.mongodb,
        dynamodb = plan.wire_surfaces.dynamodb,
        aws_sdk_v2_hint = plan.wire_surfaces.aws_sdk_v2_hint,
        "detected wire surfaces"
    );

    // D8: no detected adapter is guidance, not an exit. The session serves
    // anyway — detection never gates serving — so an adapter added to the
    // app is adopted live by the manifest watch loop below.
    if plan.adapter.is_none() && !skip_codegen {
        cli_ux::write_stderr_line("")?;
        cli_ux::write_stderr_line("No compatible adapter detected.")?;
        cli_ux::write_stderr_line("")?;
        cli_ux::write_stderr_line("To get started:")?;
        cli_ux::write_stderr_line("  nimbus init convex          # Convex adapter")?;
        cli_ux::write_stderr_line("  nimbus init cloud-functions # Cloud Functions adapter")?;
        cli_ux::write_stderr_line("")?;
        cli_ux::write_stderr_line(
            "The dev server is starting anyway; an adapter added to this app is adopted live.",
        )?;
    }

    // The Firestore client path is the only dev flow that mutates the app
    // (`package.json` is rewired to the drop-in `firebase` package), so it
    // runs first and fail-closed: a refusal happens before any mutation —
    // including the `.env.local` write below.
    if matches!(plan.adapter, Some(DevAdapter::FirestoreClient)) && !skip_codegen {
        firebase::wire_firestore_client_app(&plan.app_dir)?;
    }

    write_env_local_deployment(&plan.app_dir, &plan.deployment_slug)?;
    // Detected wire surfaces get their resolved endpoints + generated
    // credentials as Nimbus-owned keys; user-owned keys are never touched.
    env_file::write_env_local_nimbus_keys(
        &plan.app_dir,
        &plan.wire.env_local_entries(plan.wire_surfaces),
    )?;
    for notice in wire::port_fallback_notices(&plan.wire, plan.wire_surfaces) {
        cli_ux::write_stderr_line(&notice)?;
    }

    if let Some(adapter) = &plan.adapter
        && !skip_codegen
        && adapter.needs_node_dependencies()
    {
        // Provision the adapter's embedded packages before installing so the
        // `file:` specifiers resolve — on a fresh clone `.nimbus/` is gitignored
        // and absent, and after a binary upgrade the payload must be refreshed
        // (which also forces a Node dependency reinstall so copies can't go stale).
        if let Some(target) = adapter.provision_target() {
            let selection = provision::Selection::parse(target)
                .expect("adapter provision target must be a known selection");
            provision::ensure(&plan.app_dir, &selection)?;
        }
        for install_dir in adapter.npm_install_dirs(&plan.app_dir) {
            node_runtime::auto_install_node_dependencies(&install_dir).await?;
        }
    }

    emit_dev_banner(&plan)?;
    let console_url = operator_console_url(&plan.local_url);
    let auto_open_decision = plan.auto_open_decision.clone();
    tokio::spawn(announce_launch_url_when_ready(
        console_url,
        auto_open_decision,
    ));
    if plan.once {
        return run_start_command_with_prepared_network(plan.start_command, prepared_network).await;
    }

    let watch_plan = plan.watch_plan();
    let boot_auto_tenant = plan
        .start_command
        .auto_tenant
        .clone()
        .expect("dev plan should configure an auto tenant");
    // The codegen watch roots are live state shared between the loops:
    // seeded from the boot-time adapter, re-registered by the manifest
    // watch loop when an adapter is adopted or removed mid-session.
    let (watch_roots_tx, watch_roots_rx) = tokio::sync::watch::channel(plan.initial_watch_roots());
    // Moving `plan.start_command` into the first arm while the manifest
    // watch borrows `plan.app_dir` / `plan.wire` is a disjoint partial
    // move — `DevPlan` has no `Drop`, so the borrow checker allows it.
    tokio::select! {
        result = run_start_command_with_prepared_network(plan.start_command, prepared_network) => result,
        result = run_dev_watch_loop(watch_plan, watch_roots_rx) => result,
        result = redetect::run_manifest_watch_loop(redetect::ManifestWatch {
            app_dir: &plan.app_dir,
            wire: &plan.wire,
            initial_surfaces: plan.wire_surfaces,
            initial_adapter: plan.adapter,
            boot_auto_tenant,
            watch_roots: &watch_roots_tx,
        }) => result,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(crate) enum DevTailLogsMode {
    Always,
    #[default]
    PauseOnSync,
    Disable,
}

impl DevTailLogsMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::PauseOnSync => "pause-on-sync",
            Self::Disable => "disable",
        }
    }
}

#[cfg(test)]
mod tests;
