use std::env;
use std::io;
use std::path::PathBuf;

use clap::{Args, ValueEnum};

use crate::cli_ux;
use crate::node;
use crate::start::run_start_command;

mod adapter;
mod banner;
mod env_file;
mod launch;
mod plan;
mod watch;

use banner::emit_dev_banner;
use env_file::write_env_local_deployment;
use launch::{announce_launch_url_when_ready, operator_console_url};
use plan::resolve_dev_plan;
use watch::run_dev_watch_loop;

#[cfg(test)]
use crate::start::{CliTenantProvider, StartCommand};
#[cfg(test)]
use adapter::{DevAdapter, detect_dev_adapter};
#[cfg(test)]
use banner::dev_banner_lines;
#[cfg(test)]
use launch::{AutoOpenDecision, EnvLookup, resolve_auto_open};
#[cfg(test)]
use plan::{DevPlan, detect_app_dir};
#[cfg(test)]
use watch::collect_source_snapshot;

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

    /// Suppress the default browser auto-open and print a one-line launch
    /// URL banner instead. Auto-open is also suppressed automatically in
    /// non-interactive environments (when `$CI` or `$NO_BROWSER` is set,
    /// or stdout is not a TTY); in those cases the same banner is printed.
    #[arg(long, default_value_t = false)]
    pub(crate) no_open: bool,
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
    let console_url = operator_console_url(&plan.local_url);
    let auto_open_decision = plan.auto_open_decision.clone();
    tokio::spawn(announce_launch_url_when_ready(
        console_url,
        auto_open_decision,
    ));
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
        assert!(
            !command.no_open,
            "nimbus dev should default to auto-opening the browser (no_open=false)"
        );
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
            "--no-open",
        ]);
        assert_eq!(command.port, 4567);
        assert_eq!(command.app_dir, Some(PathBuf::from("./demo")));
        assert_eq!(command.data_dir, Some(PathBuf::from("./state")));
        assert_eq!(command.compose_file, vec![PathBuf::from("./compose.yaml")]);
        assert!(command.once);
        assert!(command.skip_codegen);
        assert!(command.debug_node_apis);
        assert_eq!(command.tail_logs, DevTailLogsMode::Disable);
        assert!(command.no_open, "--no-open should opt out of auto-open");
    }

    /// Test stub for [`EnvLookup`] so smart-detect branches can be driven
    /// without poking process env.
    struct StubEnv {
        ci: Option<&'static str>,
        no_browser: Option<&'static str>,
    }

    impl EnvLookup for StubEnv {
        fn get(&self, key: &str) -> Option<String> {
            match key {
                "CI" => self.ci.map(str::to_owned),
                "NO_BROWSER" => self.no_browser.map(str::to_owned),
                _ => None,
            }
        }
    }

    const ENV_EMPTY: StubEnv = StubEnv {
        ci: None,
        no_browser: None,
    };

    #[test]
    fn smart_detect_opens_when_tty_and_env_clean() {
        let decision = resolve_auto_open(false, true, &ENV_EMPTY);
        assert!(
            decision.auto_open,
            "TTY + clean env + no --no-open should auto-open: {decision:?}"
        );
        assert!(decision.reason.is_none());
    }

    #[test]
    fn smart_detect_suppresses_on_no_open_flag() {
        let decision = resolve_auto_open(true, true, &ENV_EMPTY);
        assert!(!decision.auto_open);
        assert_eq!(decision.reason.as_deref(), Some("--no-open"));
    }

    #[test]
    fn smart_detect_suppresses_on_ci_env() {
        let env = StubEnv {
            ci: Some("true"),
            no_browser: None,
        };
        let decision = resolve_auto_open(false, true, &env);
        assert!(!decision.auto_open);
        assert_eq!(decision.reason.as_deref(), Some("$CI is set"));
    }

    #[test]
    fn smart_detect_suppresses_on_no_browser_env() {
        let env = StubEnv {
            ci: None,
            no_browser: Some("1"),
        };
        let decision = resolve_auto_open(false, true, &env);
        assert!(!decision.auto_open);
        assert_eq!(decision.reason.as_deref(), Some("$NO_BROWSER is set"));
    }

    #[test]
    fn smart_detect_suppresses_when_stdout_is_not_a_tty() {
        let decision = resolve_auto_open(false, false, &ENV_EMPTY);
        assert!(!decision.auto_open);
        assert_eq!(decision.reason.as_deref(), Some("stdout is not a TTY"));
    }

    #[test]
    fn smart_detect_prefers_no_open_reason_over_env_suppression() {
        // --no-open is the most user-explicit signal; surface it first.
        let env = StubEnv {
            ci: Some("true"),
            no_browser: Some("1"),
        };
        let decision = resolve_auto_open(true, false, &env);
        assert!(!decision.auto_open);
        assert_eq!(decision.reason.as_deref(), Some("--no-open"));
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
        assert_eq!(
            plan.start_command.tenant_isolation_mode,
            nimbus_server::TenantIsolationMode::LocalDevelopment,
            "dev should preserve Node-compatible localhost grants explicitly"
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
            auto_open_decision: AutoOpenDecision::open(),
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

    #[test]
    fn detect_app_dir_stops_at_git_boundary_when_marker_lives_outside() {
        // CD7(b) — `.git/` directory bounds the dev walker. A sibling
        // `nimbus/` outside the boundary is invisible; the walker falls
        // through and returns the original CWD, which is the "give up"
        // sentinel that dev later rejects via `detect_dev_adapter`.
        let temp = tempdir().expect("tempdir should build");
        fs::create_dir_all(temp.path().join("inner").join(".git")).expect(".git dir should create");
        let nested_cwd = temp.path().join("inner").join("sub");
        fs::create_dir_all(&nested_cwd).expect("nested CWD should create");
        fs::create_dir_all(temp.path().join("nimbus"))
            .expect("sibling nimbus outside boundary should create");

        let resolved = super::detect_app_dir(&nested_cwd);
        assert_eq!(
            resolved, nested_cwd,
            "walk-up must stop at the inner `.git` boundary and ignore the sibling nimbus/; got {resolved:?}"
        );
    }

    #[test]
    fn detect_app_dir_walks_multiple_levels_within_git_boundary() {
        // CD7(c) — multi-level discovery still works inside the boundary.
        // CWD is `<tmp>/app/src/components/`; `convex/` lives at
        // `<tmp>/app/convex/`; `.git/` is the outer repo at `<tmp>/.git/`.
        // The walker must find `<tmp>/app/` even though it has to climb
        // two ancestor levels to get there.
        let temp = tempdir().expect("tempdir should build");
        let app_dir = temp.path().join("app");
        let convex_dir = app_dir.join("convex");
        let components_cwd = app_dir.join("src").join("components");
        fs::create_dir_all(&convex_dir).expect("convex dir should create");
        fs::create_dir_all(&components_cwd).expect("components cwd should create");
        fs::create_dir_all(temp.path().join(".git")).expect("repo .git should create");

        let resolved = super::detect_app_dir(&components_cwd);
        assert_eq!(
            resolved, app_dir,
            "walker must find the parent `<tmp>/app/` (multi-level discovery inside boundary)"
        );
    }

    #[test]
    fn detect_app_dir_treats_dot_git_file_as_worktree_boundary() {
        // CD7(f) part 1 — agents in this repo work primarily out of `git
        // worktree` checkouts where `.git` is a *file*, not a directory.
        // The boundary helper uses `Path::exists()` precisely so this
        // shape stops the walker the same way a real `.git/` directory
        // would. Regressing to `is_dir()` would silently escape a
        // worktree and find unrelated parents.
        let temp = tempdir().expect("tempdir should build");
        let worktree_root = temp.path().join("wt");
        fs::create_dir_all(&worktree_root).expect("worktree root should create");
        fs::write(
            worktree_root.join(".git"),
            "gitdir: /fake/elsewhere/.git/worktrees/wt\n",
        )
        .expect(".git file should write");
        let nested_cwd = worktree_root.join("inner").join("sub");
        fs::create_dir_all(&nested_cwd).expect("nested cwd should create");
        fs::create_dir_all(temp.path().join("nimbus"))
            .expect("sibling nimbus outside worktree should create");

        let resolved = super::detect_app_dir(&nested_cwd);
        assert_eq!(
            resolved, nested_cwd,
            "walker must stop at the worktree's `.git` *file* and ignore parents; got {resolved:?}"
        );
    }
}
