use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::SystemTime;

use clap::{Args, ValueEnum};
use rand::RngCore;

use crate::cli_ux;
use crate::codegen::run_codegen_for_app_dir;
use crate::deploy::{DeployRequest, post_deploy_request};
use crate::start::{CliTenantProvider, StartCommand, run_start_command};

const DEFAULT_DEV_PORT: u16 = 3210;
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const WATCH_DEBOUNCE_DELAY: Duration = Duration::from_millis(300);

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

    /// App directory containing a neovex/ or convex/ source root.
    #[arg(long)]
    pub(crate) app_dir: Option<PathBuf>,

    /// Optional Compose file that declares local service dependencies.
    #[arg(long)]
    pub(crate) compose_file: Option<PathBuf>,

    /// Run startup only, without the watched codegen loop.
    #[arg(long, default_value_t = false)]
    pub(crate) once: bool,

    /// Skip initial codegen before starting the local server. Watched reruns still use codegen.
    #[arg(long, default_value_t = false)]
    pub(crate) skip_codegen: bool,

    /// Runtime log tailing mode. Log multiplexing is pending runtime log plumbing.
    #[arg(long, value_enum, default_value_t = DevTailLogsMode::PauseOnSync)]
    pub(crate) tail_logs: DevTailLogsMode,

    /// Shared local dev persistence root for tenant data and control state.
    #[arg(long)]
    pub(crate) data_dir: Option<PathBuf>,
}

pub(crate) async fn run_dev_command(command: DevCommand) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    let plan = resolve_dev_plan(command, &cwd)?;
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
    local_url: String,
    source_root: Option<PathBuf>,
    once: bool,
    tail_logs: DevTailLogsMode,
    start_command: StartCommand,
}

#[derive(Debug, Clone)]
struct DevWatchPlan {
    app_dir: PathBuf,
    source_root: Option<PathBuf>,
    tail_logs: DevTailLogsMode,
    local_url: String,
    deploy_admin_token: String,
}

impl DevPlan {
    fn watch_plan(&self) -> DevWatchPlan {
        DevWatchPlan {
            app_dir: self.app_dir.clone(),
            source_root: self.source_root.clone(),
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
    let source_root = detect_source_root(&app_dir);
    let data_dir = command
        .data_dir
        .as_deref()
        .map(|path| resolve_unchecked_path(path, cwd))
        .unwrap_or_else(|| app_dir.join(".neovex").join("dev"));
    let local_url = format!("http://localhost:{}/", command.port);
    let deploy_admin_token = generate_dev_deploy_token();
    let start_command = StartCommand {
        port: command.port,
        data_dir: Some(data_dir.clone()),
        control_data_dir: Some(data_dir.clone()),
        tenant_provider: Some(CliTenantProvider::Sqlite),
        app_dir: Some(app_dir.clone()),
        skip_codegen: command.skip_codegen,
        compose_file: command.compose_file,
        deploy_admin_token: Some(deploy_admin_token),
        ..StartCommand::default()
    };

    Ok(DevPlan {
        app_dir,
        data_dir,
        local_url,
        source_root,
        once: command.once,
        tail_logs: command.tail_logs,
        start_command,
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
        if candidate.join("neovex").is_dir()
            || candidate.join("convex").is_dir()
            || candidate
                .join(".neovex")
                .join("convex")
                .join("functions.json")
                .is_file()
        {
            return candidate.to_path_buf();
        }
    }
    cwd.to_path_buf()
}

fn detect_source_root(app_dir: &Path) -> Option<PathBuf> {
    let neovex_root = app_dir.join("neovex");
    if neovex_root.is_dir() {
        return Some(neovex_root);
    }

    let convex_root = app_dir.join("convex");
    if convex_root.is_dir() {
        return Some(convex_root);
    }

    None
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
    cli_ux::write_stderr_line("Neovex dev ready to start")?;
    cli_ux::write_stderr_line(&format!("Local:   {}", plan.local_url))?;
    cli_ux::write_stderr_line(&format!("App dir: {}", plan.app_dir.display()))?;
    cli_ux::write_stderr_line(&format!("Data:    {}", plan.data_dir.display()))?;
    match &plan.source_root {
        Some(source_root) if plan.once => cli_ux::write_stderr_line(&format!(
            "Watch:   disabled by --once; detected {}",
            source_root.display()
        ))?,
        Some(source_root) => {
            cli_ux::write_stderr_line(&format!("Watch:   {}", source_root.display()))?;
        }
        None if plan.once => {
            cli_ux::write_stderr_line("Watch:   disabled by --once; no source root detected")?;
        }
        None => {
            cli_ux::write_stderr_line("Watch:   disabled; no neovex/ or convex/ root detected")?;
        }
    }
    cli_ux::write_stderr_line(&format!("Logs:    {}", plan.tail_logs.as_str()))?;
    cli_ux::write_stderr_line(
        "Note: watched codegen activates regenerated artifacts locally after validation; runtime log multiplexing is still pending.",
    )
}

async fn run_dev_watch_loop(plan: DevWatchPlan) -> Result<(), Box<dyn std::error::Error>> {
    let Some(source_root) = plan.source_root.as_deref() else {
        std::future::pending::<()>().await;
        return Ok(());
    };

    emit_dev_info(format!(
        "watching {} for codegen changes",
        source_root.display()
    ));
    emit_log_tail_note(plan.tail_logs);

    let mut snapshot = match collect_source_snapshot(source_root) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            emit_dev_warning(format!(
                "could not snapshot {}: {error}",
                source_root.display()
            ));
            SourceSnapshot::default()
        }
    };

    loop {
        tokio::time::sleep(WATCH_POLL_INTERVAL).await;
        let changed = match collect_source_snapshot(source_root) {
            Ok(next) if next != snapshot => true,
            Ok(_) => false,
            Err(error) => {
                emit_dev_warning(format!(
                    "could not rescan {}: {error}",
                    source_root.display()
                ));
                false
            }
        };

        if !changed {
            continue;
        }

        tokio::time::sleep(WATCH_DEBOUNCE_DELAY).await;
        let next_snapshot = match collect_source_snapshot(source_root) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                emit_dev_warning(format!(
                    "could not rescan {} after debounce: {error}",
                    source_root.display()
                ));
                continue;
            }
        };

        if next_snapshot == snapshot {
            continue;
        }
        snapshot = next_snapshot;

        emit_dev_info("source change detected; running codegen");
        match run_codegen_for_app_dir(&plan.app_dir).await {
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

fn collect_source_snapshot(source_root: &Path) -> io::Result<SourceSnapshot> {
    let mut files = std::collections::BTreeMap::new();
    collect_source_snapshot_recursive(source_root, source_root, &mut files)?;
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

fn should_skip_watch_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    matches!(
        name,
        "_generated" | "node_modules" | ".git" | ".neovex" | ".next" | "dist" | "build"
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use clap::{Parser, error::ErrorKind};
    use tempfile::tempdir;

    use super::*;
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
        let command = parse_dev(["neovex", "dev"]);
        assert_eq!(command.port, DEFAULT_DEV_PORT);
        assert_eq!(command.app_dir, None);
        assert_eq!(command.data_dir, None);
        assert_eq!(command.compose_file, None);
        assert!(!command.once);
        assert!(!command.skip_codegen);
        assert_eq!(command.tail_logs, DevTailLogsMode::PauseOnSync);
    }

    #[test]
    fn cli_parses_dev_overrides() {
        let command = parse_dev([
            "neovex",
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
            "--tail-logs",
            "disable",
        ]);
        assert_eq!(command.port, 4567);
        assert_eq!(command.app_dir, Some(PathBuf::from("./demo")));
        assert_eq!(command.data_dir, Some(PathBuf::from("./state")));
        assert_eq!(command.compose_file, Some(PathBuf::from("./compose.yaml")));
        assert!(command.once);
        assert!(command.skip_codegen);
        assert_eq!(command.tail_logs, DevTailLogsMode::Disable);
    }

    #[test]
    fn dev_help_is_honest_about_watch_scope() {
        let error =
            Cli::try_parse_from(["neovex", "dev", "--help"]).expect_err("help should render");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(rendered.contains("--app-dir"));
        assert!(rendered.contains("--skip-codegen"));
        assert!(rendered.contains("--data-dir"));
        assert!(rendered.contains("--once"));
        assert!(rendered.contains("--tail-logs"));
        assert!(rendered.contains("debounced codegen reruns"));
        assert!(rendered.contains("locally activates"));
        assert!(rendered.contains("runtime log multiplexing"));
    }

    #[test]
    fn dev_plan_uses_project_local_persistence_root() {
        let temp = tempdir().expect("tempdir should build");
        create_source_root(temp.path(), "convex");

        let plan = resolve_dev_plan(parse_dev(["neovex", "dev"]), temp.path())
            .expect("dev plan should resolve");
        let app_dir = temp
            .path()
            .canonicalize()
            .expect("app dir should canonicalize");
        let expected_data_dir = app_dir.join(".neovex").join("dev");

        assert_eq!(plan.app_dir, app_dir);
        assert_eq!(plan.data_dir, expected_data_dir);
        assert_eq!(plan.local_url, "http://localhost:3210/");
        assert_eq!(plan.source_root, Some(plan.app_dir.join("convex")));
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
    }

    #[test]
    fn dev_plan_detects_parent_app_from_source_root() {
        let temp = tempdir().expect("tempdir should build");
        create_source_root(temp.path(), "neovex");
        let source_root = temp.path().join("neovex");

        let plan = resolve_dev_plan(parse_dev(["neovex", "dev"]), &source_root)
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
                "neovex",
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
    }

    #[test]
    fn dev_plan_prefers_native_source_root_for_watch_when_both_exist() {
        let temp = tempdir().expect("tempdir should build");
        create_source_root(temp.path(), "convex");
        create_source_root(temp.path(), "neovex");

        let plan = resolve_dev_plan(parse_dev(["neovex", "dev"]), temp.path())
            .expect("dev plan should resolve");

        assert_eq!(plan.source_root, Some(plan.app_dir.join("neovex")));
    }

    #[test]
    fn source_snapshot_detects_source_file_changes() {
        let temp = tempdir().expect("tempdir should build");
        let root = temp.path().join("convex");
        fs::create_dir_all(&root).expect("source root should build");
        fs::write(root.join("messages.ts"), "export const list = 1;\n")
            .expect("source file should write");

        let before = collect_source_snapshot(&root).expect("snapshot should collect");
        fs::write(root.join("messages.ts"), "export const list = 12345;\n")
            .expect("source file should update");
        let after = collect_source_snapshot(&root).expect("snapshot should recollect");

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

        let before = collect_source_snapshot(&root).expect("snapshot should collect");
        fs::write(
            root.join("_generated").join("api.ts"),
            "second and longer\n",
        )
        .expect("generated file should update");
        let after = collect_source_snapshot(&root).expect("snapshot should recollect");

        assert_eq!(before, after);
    }
}
