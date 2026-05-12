use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::SystemTime;

use clap::{Args, ValueEnum};
use rand::RngCore;
use tempfile::NamedTempFile;

use crate::cli_ux;
use crate::codegen::{CodegenOptions, run_codegen_for_app_dir_with_options};
use crate::compose::discovery::{
    ResolvedComposeSelection, compose_selection_summary, resolve_compose_selection,
};
use crate::deploy::{DeployRequest, post_deploy_request};
use crate::dirs;
use crate::node;
use crate::start::{CliTenantProvider, StartCommand, run_start_command};

const DEFAULT_DEV_PORT: u16 = 3210;
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const WATCH_DEBOUNCE_DELAY: Duration = Duration::from_millis(300);
const NIMBUS_DEPLOYMENT_KEY: &str = "NIMBUS_DEPLOYMENT";

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

    /// Shared local dev persistence root for tenant data and control state.
    #[arg(long)]
    pub(crate) data_dir: Option<PathBuf>,
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

    let plan = resolve_dev_plan(command, &cwd)?;

    if plan.adapter.is_none() && !skip_codegen {
        cli_ux::write_stderr_line("")?;
        cli_ux::write_stderr_line("No compatible adapter detected.")?;
        cli_ux::write_stderr_line("")?;
        cli_ux::write_stderr_line("To get started:")?;
        cli_ux::write_stderr_line("  nimbus init convex          # Convex adapter")?;
        cli_ux::write_stderr_line("  nimbus init cloud-functions # Cloud Functions adapter")?;
        cli_ux::write_stderr_line("  nimbus dev")?;
        return Ok(());
    }

    write_env_local_deployment(&plan.app_dir, &plan.deployment_slug)?;

    if let Some(adapter) = &plan.adapter
        && !skip_codegen
        && adapter.needs_node_dependencies()
    {
        for install_dir in adapter.npm_install_dirs(&plan.app_dir) {
            node::auto_install_node_dependencies(&install_dir).await?;
        }
    }

    emit_dev_banner(&plan)?;
    if plan.once {
        return run_start_command(plan.start_command).await;
    }

    let watch_plan = plan.watch_plan();
    tokio::select! {
        result = run_start_command(plan.start_command) => result,
        result = run_dev_watch_loop(watch_plan) => result,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(crate) enum DevTailLogsMode {
    Always,
    #[default]
    PauseOnSync,
    Disable,
}

#[derive(Debug)]
struct DevPlan {
    app_dir: PathBuf,
    data_dir: PathBuf,
    deployment_slug: String,
    compose_selection: Option<ResolvedComposeSelection>,
    local_url: String,
    adapter: Option<DevAdapter>,
    once: bool,
    tail_logs: DevTailLogsMode,
    start_command: StartCommand,
}

#[derive(Debug, Clone)]
struct DevWatchPlan {
    app_dir: PathBuf,
    source_roots: Vec<PathBuf>,
    debug_node_apis: bool,
    tail_logs: DevTailLogsMode,
    local_url: String,
    deploy_admin_token: String,
}

impl DevPlan {
    fn watch_plan(&self) -> DevWatchPlan {
        DevWatchPlan {
            app_dir: self.app_dir.clone(),
            source_roots: self
                .adapter
                .as_ref()
                .map(|adapter| adapter.source_roots().to_vec())
                .unwrap_or_default(),
            debug_node_apis: self.start_command.debug_node_apis,
            tail_logs: self.tail_logs,
            local_url: self.local_url.clone(),
            deploy_admin_token: self
                .start_command
                .deploy_admin_token
                .clone()
                .expect("dev plan should configure deploy activation token"),
        }
    }
}

fn resolve_dev_plan(command: DevCommand, cwd: &Path) -> io::Result<DevPlan> {
    let app_dir = resolve_app_dir(command.app_dir.as_deref(), cwd)?;
    let adapter = detect_dev_adapter(&app_dir)?;
    let deployment_slug =
        dirs::deployment_slug(&app_dir).map_err(|error| io::Error::other(error.to_string()))?;
    let explicit_compose_files = command.compose_file.as_slice();
    let compose_selection = resolve_compose_selection(explicit_compose_files, cwd)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let data_dir = command
        .data_dir
        .as_deref()
        .map(|path| resolve_unchecked_path(path, cwd))
        .unwrap_or_else(|| app_dir.join(".nimbus").join("dev"));
    let local_url = format!("http://localhost:{}/", command.port);
    let deploy_admin_token = generate_dev_deploy_token();
    let start_command = StartCommand {
        port: command.port,
        data_dir: Some(data_dir.clone()),
        control_data_dir: Some(data_dir.clone()),
        tenant_provider: Some(CliTenantProvider::Sqlite),
        app_dir: Some(app_dir.clone()),
        skip_codegen: command.skip_codegen,
        debug_node_apis: command.debug_node_apis,
        compose_file: command.compose_file,
        deploy_admin_token: Some(deploy_admin_token),
        auto_tenant: Some("demo".to_string()),
        ..StartCommand::default()
    };

    Ok(DevPlan {
        app_dir,
        data_dir,
        deployment_slug,
        compose_selection,
        local_url,
        adapter,
        once: command.once,
        tail_logs: command.tail_logs,
        start_command,
    })
}

fn write_env_local_deployment(app_dir: &Path, slug: &str) -> io::Result<()> {
    let env_path = app_dir.join(".env.local");
    let deployment_value = format!("local:{slug}");
    let target_line = format!("{NIMBUS_DEPLOYMENT_KEY}={deployment_value}");
    let key_prefix = format!("{NIMBUS_DEPLOYMENT_KEY}=");

    let content = match std::fs::read_to_string(&env_path) {
        Ok(existing) => normalize_env_local_content(&existing, &target_line, &key_prefix),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Some(format!("{target_line}\n")),
        Err(e) => return Err(e),
    };

    if let Some(content) = content {
        write_text_file_atomically(&env_path, &content)?;
    }
    Ok(())
}

fn normalize_env_local_content(
    existing: &str,
    target_line: &str,
    key_prefix: &str,
) -> Option<String> {
    let line_ending = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let had_trailing_newline = existing.ends_with('\n');
    let mut found = false;
    let mut updated = Vec::new();

    for line in existing.lines() {
        if line.starts_with(key_prefix) {
            if !found {
                updated.push(target_line.to_owned());
                found = true;
            }
        } else {
            updated.push(line.to_owned());
        }
    }

    let normalized = if found {
        let mut result = updated.join(line_ending);
        if had_trailing_newline {
            result.push_str(line_ending);
        }
        result
    } else {
        let mut result = existing.to_owned();
        if !result.ends_with('\n') && !result.is_empty() {
            result.push_str(line_ending);
        }
        result.push_str(target_line);
        result.push_str(line_ending);
        result
    };

    (normalized != existing).then_some(normalized)
}

fn write_text_file_atomically(path: &Path, content: &str) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path {} does not have a parent directory", path.display()),
        )
    })?;
    std::fs::create_dir_all(parent)?;

    let mut temp_file = NamedTempFile::new_in(parent)?;
    temp_file.write_all(content.as_bytes())?;
    temp_file.flush()?;
    temp_file.as_file().sync_all()?;
    temp_file.into_temp_path().persist(path).map_err(|error| {
        io::Error::other(format!(
            "failed to atomically replace {}: {}",
            path.display(),
            error.error
        ))
    })
}

fn resolve_app_dir(explicit_app_dir: Option<&Path>, cwd: &Path) -> io::Result<PathBuf> {
    let selected = explicit_app_dir
        .map(|path| resolve_unchecked_path(path, cwd))
        .unwrap_or_else(|| detect_app_dir(cwd));
    canonicalize_dir(&selected)
}

fn detect_app_dir(cwd: &Path) -> PathBuf {
    for candidate in cwd.ancestors() {
        if candidate.join("nimbus").is_dir()
            || candidate.join("convex").is_dir()
            || candidate
                .join(".nimbus")
                .join("convex")
                .join("functions.json")
                .is_file()
            || candidate.join("firebase.json").is_file()
        {
            return candidate.to_path_buf();
        }
    }
    cwd.to_path_buf()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DevAdapter {
    Convex { source_root: PathBuf },
    CloudFunctions { source_roots: Vec<PathBuf> },
}

impl DevAdapter {
    fn adapter(&self) -> node::Adapter {
        match self {
            Self::Convex { .. } => node::Adapter::Convex,
            Self::CloudFunctions { .. } => node::Adapter::CloudFunctions,
        }
    }

    fn name(&self) -> &'static str {
        self.adapter().name()
    }

    fn source_roots(&self) -> &[PathBuf] {
        match self {
            Self::Convex { source_root } => std::slice::from_ref(source_root),
            Self::CloudFunctions { source_roots } => source_roots,
        }
    }

    fn needs_node_dependencies(&self) -> bool {
        self.adapter().needs_node_dependencies()
    }

    fn npm_install_dirs(&self, app_dir: &Path) -> Vec<PathBuf> {
        match self {
            Self::Convex { .. } => vec![app_dir.to_path_buf()],
            Self::CloudFunctions { source_roots } => source_roots.clone(),
        }
    }
}

fn detect_dev_adapter(app_dir: &Path) -> io::Result<Option<DevAdapter>> {
    let nimbus_root = app_dir.join("nimbus");
    if nimbus_root.is_dir() {
        return Ok(Some(DevAdapter::Convex {
            source_root: nimbus_root,
        }));
    }

    let convex_root = app_dir.join("convex");
    if convex_root.is_dir() {
        return Ok(Some(DevAdapter::Convex {
            source_root: convex_root,
        }));
    }

    if let Some(adapter) = detect_cloud_functions_adapter(app_dir)? {
        return Ok(Some(adapter));
    }

    Ok(None)
}

fn detect_cloud_functions_adapter(app_dir: &Path) -> io::Result<Option<DevAdapter>> {
    if let Some(project) = node::firebase_functions_project(app_dir)? {
        return Ok(Some(DevAdapter::CloudFunctions {
            source_roots: project.source_dirs(),
        }));
    }

    if has_functions_framework_dependency(app_dir) {
        return Ok(Some(DevAdapter::CloudFunctions {
            source_roots: vec![app_dir.to_path_buf()],
        }));
    }

    Ok(None)
}

fn has_functions_framework_dependency(app_dir: &Path) -> bool {
    let package_json_path = app_dir.join("package.json");
    let Ok(content) = std::fs::read_to_string(&package_json_path) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let package_name = "@google-cloud/functions-framework";
    for key in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        if parsed[key].get(package_name).is_some() {
            return true;
        }
    }
    false
}

fn resolve_unchecked_path(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn canonicalize_dir(path: &Path) -> io::Result<PathBuf> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("app directory {} is not readable: {error}", path.display()),
        )
    })?;
    if !metadata.is_dir() {
        return Err(io::Error::other(format!(
            "app path {} is not a directory",
            path.display()
        )));
    }
    path.canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to resolve app directory {}: {error}",
                path.display()
            ),
        )
    })
}

fn emit_dev_banner(plan: &DevPlan) -> io::Result<()> {
    for line in dev_banner_lines(plan) {
        cli_ux::write_stderr_line(&line)?;
    }
    Ok(())
}

fn dev_banner_lines(plan: &DevPlan) -> Vec<String> {
    let mut lines = vec![
        "Nimbus dev ready to start".to_string(),
        format!("Local:      {}", plan.local_url),
        format!("Deployment: local:{}", plan.deployment_slug),
        format!("App dir:    {}", plan.app_dir.display()),
        format!("Data:       {}", plan.data_dir.display()),
    ];
    if let Some(adapter) = &plan.adapter {
        lines.push(format!("Adapter:    {}", adapter.name()));
    }
    if let Some(selection) = plan.compose_selection.as_ref() {
        lines.push(format!(
            "Compose:    {}",
            compose_selection_summary(selection)
        ));
    }
    match plan.adapter.as_ref() {
        Some(adapter) if plan.once => lines.push(format!(
            "Watch:      disabled by --once; detected {}",
            format_watch_roots(adapter.source_roots())
        )),
        Some(adapter) => {
            lines.push(format!(
                "Watch:      {}",
                format_watch_roots(adapter.source_roots())
            ));
        }
        None if plan.once => {
            lines.push("Watch:      disabled by --once; no adapter detected".to_string());
        }
        None => {
            lines.push("Watch:      disabled; no adapter detected".to_string());
        }
    }
    lines.push(format!("Logs:       {}", plan.tail_logs.as_str()));
    lines.push(
        "Note: watched codegen activates regenerated artifacts locally after validation; runtime log multiplexing is still pending.".to_string(),
    );
    lines
}

async fn run_dev_watch_loop(plan: DevWatchPlan) -> Result<(), Box<dyn std::error::Error>> {
    if plan.source_roots.is_empty() {
        std::future::pending::<()>().await;
        return Ok(());
    }

    emit_dev_info(format!(
        "watching {} for codegen changes",
        format_watch_roots(&plan.source_roots)
    ));
    emit_log_tail_note(plan.tail_logs);

    let mut snapshot = match collect_source_snapshot(&plan.app_dir, &plan.source_roots) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            emit_dev_warning(format!(
                "could not snapshot watched sources under {}: {error}",
                plan.app_dir.display()
            ));
            SourceSnapshot::default()
        }
    };

    loop {
        tokio::time::sleep(WATCH_POLL_INTERVAL).await;
        let changed = match collect_source_snapshot(&plan.app_dir, &plan.source_roots) {
            Ok(next) if next != snapshot => true,
            Ok(_) => false,
            Err(error) => {
                emit_dev_warning(format!(
                    "could not rescan watched sources under {}: {error}",
                    plan.app_dir.display()
                ));
                false
            }
        };

        if !changed {
            continue;
        }

        tokio::time::sleep(WATCH_DEBOUNCE_DELAY).await;
        let next_snapshot = match collect_source_snapshot(&plan.app_dir, &plan.source_roots) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                emit_dev_warning(format!(
                    "could not rescan watched sources under {} after debounce: {error}",
                    plan.app_dir.display()
                ));
                continue;
            }
        };

        if next_snapshot == snapshot {
            continue;
        }
        snapshot = next_snapshot;

        emit_dev_info("source change detected; running codegen");
        match run_codegen_for_app_dir_with_options(
            &plan.app_dir,
            CodegenOptions {
                debug_node_apis: plan.debug_node_apis,
            },
        )
        .await
        {
            Ok(()) => match activate_dev_generation(&plan).await {
                Ok(response) => {
                    let change_lines = response.diff.human_lines();
                    if response.activated {
                        emit_dev_info(format!(
                            "activated app generation {} after codegen (previous {}, {} changes)",
                            response.generation,
                            response.previous_generation,
                            change_lines.len()
                        ));
                    } else {
                        emit_dev_info(format!(
                            "validated app artifacts against generation {} without activation (dry_run={})",
                            response.generation, response.dry_run
                        ));
                    }
                    for line in change_lines.into_iter().take(8) {
                        emit_dev_info(format!("deploy diff: {line}"));
                    }
                }
                Err(error) => emit_dev_warning(format!(
                    "generated app artifacts, but local activation failed: {error}"
                )),
            },
            Err(error) => emit_dev_warning(format!("codegen failed: {error}")),
        }
    }
}

async fn activate_dev_generation(
    plan: &DevWatchPlan,
) -> Result<crate::deploy::DeployResponse, Box<dyn std::error::Error>> {
    let request = DeployRequest::from_app_dir(&plan.app_dir, false)?;
    post_deploy_request(&plan.local_url, &plan.deploy_admin_token, &request).await
}

fn emit_dev_info(message: impl AsRef<str>) {
    let _ = cli_ux::write_stderr_prefixed_line("info:", message.as_ref());
}

fn emit_dev_warning(message: impl AsRef<str>) {
    let _ = cli_ux::write_stderr_prefixed_line("warning:", message.as_ref());
}

fn emit_log_tail_note(mode: DevTailLogsMode) {
    match mode {
        DevTailLogsMode::Always | DevTailLogsMode::PauseOnSync => emit_dev_info(format!(
            "runtime log tail mode is {}; live multiplexing is pending runtime log plumbing",
            mode.as_str()
        )),
        DevTailLogsMode::Disable => emit_dev_info("runtime log tailing disabled"),
    }
}

fn generate_dev_deploy_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut token, "{byte:02x}");
    }
    token
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SourceSnapshot {
    files: std::collections::BTreeMap<PathBuf, FileFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

fn collect_source_snapshot(app_dir: &Path, source_roots: &[PathBuf]) -> io::Result<SourceSnapshot> {
    let mut files = std::collections::BTreeMap::new();
    for source_root in source_roots {
        collect_source_snapshot_recursive(app_dir, source_root, &mut files)?;
    }
    Ok(SourceSnapshot { files })
}

fn collect_source_snapshot_recursive(
    base: &Path,
    dir: &Path,
    files: &mut std::collections::BTreeMap<PathBuf, FileFingerprint>,
) -> io::Result<()> {
    let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            if should_skip_watch_dir(&path) {
                continue;
            }
            collect_source_snapshot_recursive(base, &path, files)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let relative_path = path.strip_prefix(base).unwrap_or(&path).to_path_buf();
        files.insert(
            relative_path,
            FileFingerprint {
                len: metadata.len(),
                modified: metadata.modified().ok(),
            },
        );
    }
    Ok(())
}

fn format_watch_roots(source_roots: &[PathBuf]) -> String {
    source_roots
        .iter()
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn should_skip_watch_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    matches!(
        name,
        "_generated" | "node_modules" | ".git" | ".nimbus" | ".next" | "dist" | "build"
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use clap::{Parser, error::ErrorKind};
    use tempfile::tempdir;

    use super::*;
    use crate::test_support::with_current_dir;
    use crate::{Cli, Command};

    fn parse_dev<I, T>(args: I) -> DevCommand
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let cli = Cli::parse_from(args);
        let Command::Dev(command) = cli.command else {
            panic!("dev subcommand should parse");
        };
        *command
    }

    fn create_source_root(app_dir: &Path, root: &str) {
        fs::create_dir_all(app_dir.join(root)).expect("source root should build");
    }

    #[test]
    fn cli_parses_dev_defaults() {
        let command = parse_dev(["nimbus", "dev"]);
        assert_eq!(command.port, DEFAULT_DEV_PORT);
        assert_eq!(command.app_dir, None);
        assert_eq!(command.data_dir, None);
        assert_eq!(command.compose_file, Vec::<PathBuf>::new());
        assert!(!command.once);
        assert!(!command.skip_codegen);
        assert!(!command.debug_node_apis);
        assert_eq!(command.tail_logs, DevTailLogsMode::PauseOnSync);
    }

    #[test]
    fn cli_parses_dev_overrides() {
        let command = parse_dev([
            "nimbus",
            "dev",
            "--port",
            "4567",
            "--app-dir",
            "./demo",
            "--data-dir",
            "./state",
            "--compose-file",
            "./compose.yaml",
            "--once",
            "--skip-codegen",
            "--debug-node-apis",
            "--tail-logs",
            "disable",
        ]);
        assert_eq!(command.port, 4567);
        assert_eq!(command.app_dir, Some(PathBuf::from("./demo")));
        assert_eq!(command.data_dir, Some(PathBuf::from("./state")));
        assert_eq!(command.compose_file, vec![PathBuf::from("./compose.yaml")]);
        assert!(command.once);
        assert!(command.skip_codegen);
        assert!(command.debug_node_apis);
        assert_eq!(command.tail_logs, DevTailLogsMode::Disable);
    }

    #[test]
    fn cli_parses_dev_multiple_compose_files_in_order() {
        let command = parse_dev([
            "nimbus",
            "dev",
            "--compose-file",
            "./compose.yaml",
            "--compose-file",
            "./compose.dev.yaml",
        ]);

        assert_eq!(
            command.compose_file,
            vec![
                PathBuf::from("./compose.yaml"),
                PathBuf::from("./compose.dev.yaml")
            ]
        );
    }

    #[test]
    fn dev_help_is_honest_about_watch_scope() {
        let error =
            Cli::try_parse_from(["nimbus", "dev", "--help"]).expect_err("help should render");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(rendered.contains("--app-dir"));
        assert!(rendered.contains("--skip-codegen"));
        assert!(rendered.contains("--debug-node-apis"));
        assert!(rendered.contains("--data-dir"));
        assert!(rendered.contains("--once"));
        assert!(rendered.contains("--tail-logs"));
        assert!(rendered.contains("debounced codegen reruns"));
        assert!(rendered.contains("locally activates"));
        assert!(rendered.contains("runtime log multiplexing"));
        assert!(rendered.contains("COMPOSE_FILE"));
    }

    #[test]
    fn dev_plan_uses_project_local_persistence_root() {
        let temp = tempdir().expect("tempdir should build");
        create_source_root(temp.path(), "convex");

        let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
            .expect("dev plan should resolve");
        let app_dir = temp
            .path()
            .canonicalize()
            .expect("app dir should canonicalize");
        let expected_data_dir = app_dir.join(".nimbus").join("dev");

        assert_eq!(plan.app_dir, app_dir);
        assert_eq!(plan.data_dir, expected_data_dir);
        assert_eq!(plan.local_url, "http://localhost:3210/");
        assert_eq!(
            plan.adapter,
            Some(DevAdapter::Convex {
                source_root: plan.app_dir.join("convex"),
            })
        );
        assert!(!plan.once);
        assert_eq!(plan.tail_logs, DevTailLogsMode::PauseOnSync);
        assert_eq!(plan.start_command.port, 3210);
        assert_eq!(plan.start_command.app_dir, Some(plan.app_dir.clone()));
        assert_eq!(plan.start_command.data_dir, Some(expected_data_dir.clone()));
        assert_eq!(plan.start_command.control_data_dir, Some(expected_data_dir));
        assert_eq!(
            plan.start_command.tenant_provider,
            Some(CliTenantProvider::Sqlite)
        );
        assert!(!plan.start_command.skip_codegen);
        assert!(
            plan.start_command
                .deploy_admin_token
                .as_deref()
                .is_some_and(|token| token.len() == 64)
        );
        assert_eq!(
            plan.start_command.auto_tenant,
            Some("demo".to_string()),
            "dev plan should auto-create the demo tenant"
        );
    }

    #[test]
    fn dev_plan_detects_parent_app_from_source_root() {
        let temp = tempdir().expect("tempdir should build");
        create_source_root(temp.path(), "nimbus");
        let source_root = temp.path().join("nimbus");

        let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), &source_root)
            .expect("dev plan should resolve from source root");

        assert_eq!(
            plan.app_dir,
            temp.path()
                .canonicalize()
                .expect("app dir should canonicalize")
        );
    }

    #[test]
    fn dev_plan_respects_explicit_app_and_data_dirs() {
        let temp = tempdir().expect("tempdir should build");
        let app_dir = temp.path().join("app");
        create_source_root(&app_dir, "convex");

        let plan = resolve_dev_plan(
            parse_dev([
                "nimbus",
                "dev",
                "--app-dir",
                "./app",
                "--data-dir",
                "./state",
                "--skip-codegen",
            ]),
            temp.path(),
        )
        .expect("dev plan should resolve");

        assert_eq!(
            plan.app_dir,
            app_dir.canonicalize().expect("app dir should canonicalize")
        );
        assert_eq!(plan.data_dir, temp.path().join("./state"));
        assert_eq!(
            plan.start_command.data_dir,
            Some(temp.path().join("./state"))
        );
        assert_eq!(
            plan.start_command.control_data_dir,
            Some(temp.path().join("./state"))
        );
        assert!(plan.start_command.skip_codegen);
        assert_eq!(plan.start_command.compose_file, Vec::<PathBuf>::new());
    }

    #[test]
    fn dev_banner_lines_report_explicit_compose_file() {
        let temp = tempdir().expect("tempdir should build");
        create_source_root(temp.path(), "convex");
        fs::write(
            temp.path().join("compose.custom.yaml"),
            "services:\n  db:\n    image: busybox:latest\n",
        )
        .expect("compose fixture should write");

        let plan = resolve_dev_plan(
            parse_dev(["nimbus", "dev", "--compose-file", "./compose.custom.yaml"]),
            temp.path(),
        )
        .expect("dev plan should resolve");

        let lines = dev_banner_lines(&plan);

        assert!(
            lines
                .iter()
                .any(|line| line == "Compose:    ./compose.custom.yaml")
        );
    }

    #[test]
    fn dev_banner_lines_report_auto_discovered_override_selection() {
        let temp = tempdir().expect("tempdir should build");
        create_source_root(temp.path(), "convex");
        fs::write(
            temp.path().join("compose.yaml"),
            "services:\n  db:\n    image: busybox:latest\n",
        )
        .expect("compose fixture should write");
        fs::write(
            temp.path().join("compose.override.yaml"),
            "services:\n  worker:\n    image: redis:7\n",
        )
        .expect("compose override fixture should write");

        let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
            .expect("dev plan should resolve");

        let lines = dev_banner_lines(&plan);
        let expected = format!(
            "Compose:    auto-discovered {} (+ compose.override.yaml)",
            temp.path().join("compose.yaml").display()
        );

        assert!(lines.iter().any(|line| line == &expected), "{lines:?}");
    }

    #[test]
    fn dev_banner_lines_report_compose_file_environment_selection() {
        let selection = crate::compose::discovery::ResolvedComposeSelection {
            origin: crate::compose::discovery::ComposeSelectionOrigin::ExplicitEnvironment,
            project_root: PathBuf::from("/workspace"),
            files: vec![
                PathBuf::from("/workspace/compose.yaml"),
                PathBuf::from("/workspace/compose.dev.yaml"),
            ],
            display_files: vec![
                PathBuf::from("./compose.yaml"),
                PathBuf::from("./compose.dev.yaml"),
            ],
        };
        let plan = DevPlan {
            app_dir: PathBuf::from("/workspace"),
            data_dir: PathBuf::from("/workspace/.nimbus/dev"),
            deployment_slug: "workspace-abcd1234".to_owned(),
            compose_selection: Some(selection),
            local_url: "http://localhost:3210/".to_owned(),
            adapter: None,
            once: false,
            tail_logs: DevTailLogsMode::PauseOnSync,
            start_command: StartCommand::default(),
        };

        let lines = dev_banner_lines(&plan);

        assert!(lines.iter().any(|line| {
            line == "Compose:    COMPOSE_FILE=./compose.yaml (+ 1 extra Compose files)"
        }));
    }

    #[test]
    fn dev_start_and_compose_resolve_same_project_from_same_cwd() {
        let temp = tempdir().expect("tempdir should build");
        create_source_root(temp.path(), "convex");
        fs::write(
            temp.path().join("compose.yaml"),
            "services:\n  db:\n    image: busybox:latest\n",
        )
        .expect("compose fixture should write");
        let nested_cwd = temp.path().join("convex");

        let compose_selection = with_current_dir(&nested_cwd, || {
            crate::compose::resolve_required_compose_selection(&[])
        })
        .expect("compose selection should resolve");
        let start_selection = with_current_dir(&nested_cwd, || {
            crate::start::resolve_optional_compose_selection(&StartCommand::default())
        })
        .expect("start selection should resolve")
        .expect("start selection should exist");
        let dev_plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), &nested_cwd)
            .expect("dev plan should resolve");
        let dev_selection = dev_plan
            .compose_selection
            .expect("dev selection should exist");

        assert_eq!(
            compose_selection
                .files
                .iter()
                .map(|path| fs::canonicalize(path).unwrap())
                .collect::<Vec<_>>(),
            start_selection
                .files
                .iter()
                .map(|path| fs::canonicalize(path).unwrap())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            compose_selection
                .files
                .iter()
                .map(|path| fs::canonicalize(path).unwrap())
                .collect::<Vec<_>>(),
            dev_selection
                .files
                .iter()
                .map(|path| fs::canonicalize(path).unwrap())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn dev_start_and_compose_explicit_paths_override_auto_discovery() {
        let temp = tempdir().expect("tempdir should build");
        create_source_root(temp.path(), "convex");
        fs::write(
            temp.path().join("compose.yaml"),
            "services:\n  db:\n    image: busybox:latest\n",
        )
        .expect("auto compose fixture should write");
        let nested_cwd = temp.path().join("convex");
        let explicit_path = nested_cwd.join("compose.custom.yaml");
        fs::write(&explicit_path, "services:\n  db:\n    image: redis:7\n")
            .expect("explicit compose fixture should write");
        let explicit_flag = Path::new("./compose.custom.yaml");

        let compose_selection = with_current_dir(&nested_cwd, || {
            crate::compose::resolve_required_compose_selection(&[explicit_flag.to_path_buf()])
        })
        .expect("compose selection should resolve");
        let start_selection = with_current_dir(&nested_cwd, || {
            crate::start::resolve_optional_compose_selection(&StartCommand {
                compose_file: vec![PathBuf::from("./compose.custom.yaml")],
                ..StartCommand::default()
            })
        })
        .expect("start selection should resolve")
        .expect("start selection should exist");
        let dev_plan = resolve_dev_plan(
            parse_dev(["nimbus", "dev", "--compose-file", "./compose.custom.yaml"]),
            &nested_cwd,
        )
        .expect("dev plan should resolve");
        let dev_selection = dev_plan
            .compose_selection
            .expect("dev selection should exist");

        assert_eq!(
            fs::canonicalize(compose_selection.primary_file()).unwrap(),
            fs::canonicalize(&explicit_path).unwrap()
        );
        assert_eq!(
            compose_selection
                .files
                .iter()
                .map(|path| fs::canonicalize(path).unwrap())
                .collect::<Vec<_>>(),
            start_selection
                .files
                .iter()
                .map(|path| fs::canonicalize(path).unwrap())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            compose_selection
                .files
                .iter()
                .map(|path| fs::canonicalize(path).unwrap())
                .collect::<Vec<_>>(),
            dev_selection
                .files
                .iter()
                .map(|path| fs::canonicalize(path).unwrap())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn dev_plan_prefers_native_source_root_for_watch_when_both_exist() {
        let temp = tempdir().expect("tempdir should build");
        create_source_root(temp.path(), "convex");
        create_source_root(temp.path(), "nimbus");

        let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
            .expect("dev plan should resolve");

        assert_eq!(
            plan.adapter,
            Some(DevAdapter::Convex {
                source_root: plan.app_dir.join("nimbus"),
            })
        );
    }

    #[test]
    fn source_snapshot_detects_source_file_changes() {
        let temp = tempdir().expect("tempdir should build");
        let root = temp.path().join("convex");
        fs::create_dir_all(&root).expect("source root should build");
        fs::write(root.join("messages.ts"), "export const list = 1;\n")
            .expect("source file should write");

        let before = collect_source_snapshot(temp.path(), std::slice::from_ref(&root))
            .expect("snapshot should collect");
        fs::write(root.join("messages.ts"), "export const list = 12345;\n")
            .expect("source file should update");
        let after = collect_source_snapshot(temp.path(), std::slice::from_ref(&root))
            .expect("snapshot should recollect");

        assert_ne!(before, after);
    }

    #[test]
    fn source_snapshot_ignores_generated_files() {
        let temp = tempdir().expect("tempdir should build");
        let root = temp.path().join("convex");
        fs::create_dir_all(root.join("_generated")).expect("generated root should build");
        fs::write(root.join("messages.ts"), "export const list = 1;\n")
            .expect("source file should write");
        fs::write(root.join("_generated").join("api.ts"), "first\n")
            .expect("generated file should write");

        let before = collect_source_snapshot(temp.path(), std::slice::from_ref(&root))
            .expect("snapshot should collect");
        fs::write(
            root.join("_generated").join("api.ts"),
            "second and longer\n",
        )
        .expect("generated file should update");
        let after = collect_source_snapshot(temp.path(), std::slice::from_ref(&root))
            .expect("snapshot should recollect");

        assert_eq!(before, after);
    }

    #[test]
    fn dev_plan_empty_dir_has_no_source_root() {
        let temp = tempdir().expect("tempdir should build");
        let app_dir_str = temp.path().to_str().unwrap();

        let plan = resolve_dev_plan(
            parse_dev(["nimbus", "dev", "--app-dir", app_dir_str]),
            temp.path(),
        )
        .expect("dev plan should resolve");
        assert!(
            plan.adapter.is_none(),
            "empty dir should have no source root"
        );
    }

    #[test]
    fn dev_plan_with_source_root_resolves() {
        let temp = tempdir().expect("tempdir should build");
        create_source_root(temp.path(), "convex");
        let app_dir_str = temp.path().to_str().unwrap();

        let plan = resolve_dev_plan(
            parse_dev(["nimbus", "dev", "--app-dir", app_dir_str]),
            temp.path(),
        )
        .expect("dev plan should resolve");
        assert!(
            plan.adapter.is_some(),
            "existing source root should be detected"
        );
    }

    #[test]
    fn dev_skip_codegen_allows_no_source_root() {
        let temp = tempdir().expect("tempdir should build");
        let app_dir_str = temp.path().to_str().unwrap();
        let command = parse_dev(["nimbus", "dev", "--skip-codegen", "--app-dir", app_dir_str]);
        assert!(command.skip_codegen);

        let plan = resolve_dev_plan(command, temp.path()).expect("dev plan should resolve");
        assert!(plan.adapter.is_none());
    }

    #[test]
    fn app_dir_nonexistent_errors_in_resolve() {
        let temp = tempdir().expect("tempdir should build");
        let new_dir = temp.path().join("new-project");
        let dir_str = new_dir.to_str().unwrap();

        let command = parse_dev(["nimbus", "dev", "--app-dir", dir_str]);
        assert!(!new_dir.exists());

        let plan = resolve_dev_plan(command, temp.path());
        assert!(
            plan.is_err(),
            "nonexistent --app-dir should error in resolve_dev_plan without pre-creation"
        );
    }

    #[test]
    fn app_dir_empty_has_no_source_root() {
        let temp = tempdir().expect("tempdir should build");
        let empty_dir = temp.path().join("empty");
        fs::create_dir_all(&empty_dir).unwrap();
        let dir_str = empty_dir.to_str().unwrap();

        let plan = resolve_dev_plan(
            parse_dev(["nimbus", "dev", "--app-dir", dir_str]),
            temp.path(),
        )
        .expect("dev plan should resolve for empty --app-dir");

        assert!(plan.adapter.is_none());
    }

    #[test]
    fn app_dir_nonempty_without_source_root_detected() {
        let temp = tempdir().expect("tempdir should build");
        let nonempty = temp.path().join("existing");
        fs::create_dir_all(&nonempty).unwrap();
        fs::write(nonempty.join("index.js"), "console.log('hi')").unwrap();
        let dir_str = nonempty.to_str().unwrap();

        let plan = resolve_dev_plan(
            parse_dev(["nimbus", "dev", "--app-dir", dir_str]),
            temp.path(),
        )
        .expect("dev plan should resolve");

        assert!(plan.adapter.is_none());
    }

    #[test]
    fn app_dir_with_source_root_skips_edge_case_check() {
        let temp = tempdir().expect("tempdir should build");
        let project = temp.path().join("project");
        fs::create_dir_all(project.join("convex")).unwrap();
        fs::write(project.join("index.js"), "console.log('hi')").unwrap();
        let dir_str = project.to_str().unwrap();

        let plan = resolve_dev_plan(
            parse_dev(["nimbus", "dev", "--app-dir", dir_str]),
            temp.path(),
        )
        .expect("dev plan should resolve");

        assert!(
            plan.adapter.is_some(),
            "should detect source root in non-empty dir"
        );
    }

    #[test]
    fn detect_cloud_functions_firebase_json() {
        let temp = tempdir().expect("tempdir should build");
        fs::create_dir_all(temp.path().join("functions")).unwrap();
        fs::write(
            temp.path().join("firebase.json"),
            r#"{"functions": {"source": "functions"}}"#,
        )
        .unwrap();

        let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
        assert_eq!(
            adapter,
            Some(DevAdapter::CloudFunctions {
                source_roots: vec![temp.path().join("functions").canonicalize().unwrap()],
            })
        );
    }

    #[test]
    fn detect_cloud_functions_firebase_json_custom_source() {
        let temp = tempdir().expect("tempdir should build");
        fs::create_dir_all(temp.path().join("backend")).unwrap();
        fs::write(
            temp.path().join("firebase.json"),
            r#"{"functions": {"source": "backend"}}"#,
        )
        .unwrap();

        let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
        assert_eq!(
            adapter,
            Some(DevAdapter::CloudFunctions {
                source_roots: vec![temp.path().join("backend").canonicalize().unwrap()],
            })
        );
    }

    #[test]
    fn detect_cloud_functions_firebase_json_array() {
        let temp = tempdir().expect("tempdir should build");
        fs::create_dir_all(temp.path().join("api")).unwrap();
        fs::write(
            temp.path().join("firebase.json"),
            r#"{"functions": [{"source": "api", "codebase": "api"}]}"#,
        )
        .unwrap();

        let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
        assert_eq!(
            adapter,
            Some(DevAdapter::CloudFunctions {
                source_roots: vec![temp.path().join("api").canonicalize().unwrap()],
            })
        );
    }

    #[test]
    fn detect_cloud_functions_firebase_json_multi_codebase_preserves_all_roots() {
        let temp = tempdir().expect("tempdir should build");
        fs::create_dir_all(temp.path().join("packages/app-functions")).unwrap();
        fs::create_dir_all(temp.path().join("packages/admin-functions")).unwrap();
        fs::write(
            temp.path().join("firebase.json"),
            r#"{"functions": [{"source": "packages/app-functions", "codebase": "app"}, {"source": "packages/admin-functions", "codebase": "admin"}]}"#,
        )
        .unwrap();

        let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
        assert_eq!(
            adapter,
            Some(DevAdapter::CloudFunctions {
                source_roots: vec![
                    temp.path()
                        .join("packages/app-functions")
                        .canonicalize()
                        .unwrap(),
                    temp.path()
                        .join("packages/admin-functions")
                        .canonicalize()
                        .unwrap(),
                ],
            })
        );
    }

    #[test]
    fn detect_cloud_functions_reports_missing_source_dir() {
        let temp = tempdir().expect("tempdir should build");
        fs::write(
            temp.path().join("firebase.json"),
            r#"{"functions": {"source": "functions"}}"#,
        )
        .unwrap();

        let error = detect_dev_adapter(temp.path()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not exist or is not readable"),
            "unexpected missing-source error: {error}"
        );
    }

    #[test]
    fn detect_cloud_functions_framework_package() {
        let temp = tempdir().expect("tempdir should build");
        fs::write(
            temp.path().join("package.json"),
            r#"{"dependencies": {"@google-cloud/functions-framework": "^3.0.0"}}"#,
        )
        .unwrap();

        let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
        assert_eq!(
            adapter,
            Some(DevAdapter::CloudFunctions {
                source_roots: vec![temp.path().to_path_buf()],
            })
        );
    }

    #[test]
    fn convex_adapter_takes_priority_over_cloud_functions() {
        let temp = tempdir().expect("tempdir should build");
        fs::create_dir_all(temp.path().join("convex")).unwrap();
        fs::write(temp.path().join("firebase.json"), "{}").unwrap();

        let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
        assert!(
            matches!(adapter, Some(DevAdapter::Convex { .. })),
            "convex should take priority over cloud-functions"
        );
    }

    #[test]
    fn cloud_functions_adapter_npm_install_dirs() {
        let adapter = DevAdapter::CloudFunctions {
            source_roots: vec![
                PathBuf::from("/project/functions"),
                PathBuf::from("/project/admin-functions"),
            ],
        };
        assert_eq!(
            adapter.npm_install_dirs(Path::new("/project")),
            vec![
                PathBuf::from("/project/functions"),
                PathBuf::from("/project/admin-functions"),
            ]
        );
    }

    #[test]
    fn convex_adapter_npm_install_dirs() {
        let adapter = DevAdapter::Convex {
            source_root: PathBuf::from("/project/convex"),
        };
        assert_eq!(
            adapter.npm_install_dirs(Path::new("/project")),
            vec![PathBuf::from("/project")]
        );
    }

    #[test]
    fn dev_plan_detects_cloud_functions_adapter() {
        let temp = tempdir().expect("tempdir should build");
        fs::create_dir_all(temp.path().join("functions")).unwrap();
        fs::write(
            temp.path().join("firebase.json"),
            r#"{"functions": {"source": "functions"}}"#,
        )
        .unwrap();
        let app_dir_str = temp.path().to_str().unwrap();

        let plan = resolve_dev_plan(
            parse_dev(["nimbus", "dev", "--app-dir", app_dir_str]),
            temp.path(),
        )
        .expect("dev plan should resolve");

        let canonical = temp.path().canonicalize().unwrap();
        assert_eq!(
            plan.adapter,
            Some(DevAdapter::CloudFunctions {
                source_roots: vec![canonical.join("functions")],
            })
        );
    }

    #[test]
    fn env_local_created_when_absent() {
        let temp = tempdir().expect("tempdir should build");
        write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();
        let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
        assert_eq!(content, "NIMBUS_DEPLOYMENT=local:myapp-abcd1234\n");
    }

    #[test]
    fn env_local_appends_when_no_deployment_var() {
        let temp = tempdir().expect("tempdir should build");
        fs::write(temp.path().join(".env.local"), "OTHER_VAR=hello\n").unwrap();
        write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();
        let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
        assert_eq!(
            content,
            "OTHER_VAR=hello\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\n"
        );
    }

    #[test]
    fn env_local_noop_when_correct_value() {
        let temp = tempdir().expect("tempdir should build");
        let original = "OTHER_VAR=hello\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\n";
        fs::write(temp.path().join(".env.local"), original).unwrap();
        write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();
        let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
        assert_eq!(
            content, original,
            "file must not be rewritten when already correct"
        );
    }

    #[test]
    fn env_local_overwrites_different_deployment_value() {
        let temp = tempdir().expect("tempdir should build");
        fs::write(
            temp.path().join(".env.local"),
            "OTHER_VAR=hello\nNIMBUS_DEPLOYMENT=local:old-slug-12345678\nANOTHER=world\n",
        )
        .unwrap();
        write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();
        let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
        assert_eq!(
            content,
            "OTHER_VAR=hello\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\nANOTHER=world\n"
        );
    }

    #[test]
    fn env_local_deduplicates_deployment_entries() {
        let temp = tempdir().expect("tempdir should build");
        fs::write(
            temp.path().join(".env.local"),
            "FIRST=1\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\nSECOND=2\nNIMBUS_DEPLOYMENT=local:old-slug-12345678\nTHIRD=3\n",
        )
        .unwrap();

        write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();

        let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
        assert_eq!(
            content,
            "FIRST=1\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\nSECOND=2\nTHIRD=3\n"
        );
    }

    #[test]
    fn env_local_preserves_other_content() {
        let temp = tempdir().expect("tempdir should build");
        fs::write(
            temp.path().join(".env.local"),
            "FIRST=1\nSECOND=2\nTHIRD=3\n",
        )
        .unwrap();
        write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();
        let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
        assert_eq!(
            content,
            "FIRST=1\nSECOND=2\nTHIRD=3\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\n"
        );
    }

    #[test]
    fn env_local_handles_file_without_trailing_newline() {
        let temp = tempdir().expect("tempdir should build");
        fs::write(temp.path().join(".env.local"), "OTHER=val").unwrap();
        write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();
        let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
        assert_eq!(
            content,
            "OTHER=val\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\n"
        );
    }

    #[test]
    fn env_local_preserves_crlf_when_rewriting() {
        let temp = tempdir().expect("tempdir should build");
        fs::write(
            temp.path().join(".env.local"),
            "FIRST=1\r\nNIMBUS_DEPLOYMENT=local:old-slug-12345678\r\nSECOND=2\r\n",
        )
        .unwrap();

        write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();

        let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
        assert_eq!(
            content,
            "FIRST=1\r\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\r\nSECOND=2\r\n"
        );
    }

    #[test]
    fn dev_banner_includes_deployment_line() {
        let temp = tempdir().expect("tempdir should build");
        create_source_root(temp.path(), "convex");
        let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
            .expect("dev plan should resolve");
        let lines = dev_banner_lines(&plan);
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("Deployment: local:")),
            "banner must include Deployment line, got: {lines:?}"
        );
    }
}
