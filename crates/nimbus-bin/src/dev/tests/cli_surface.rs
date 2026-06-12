use super::*;

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
    let error = Cli::try_parse_from(["nimbus", "dev", "--help"]).expect_err("help should render");
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
