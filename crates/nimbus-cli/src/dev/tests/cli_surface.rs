use super::*;

#[test]
fn cli_parses_dev_defaults() {
    let command = parse_dev(["nimbus", "dev"]);
    assert_eq!(command.port, DEFAULT_DEV_PORT);
    assert_eq!(command.app_dir, None);
    assert_eq!(command.data_dir, None);
    assert_eq!(command.network_state_dir, None);
    assert_eq!(command.compose_file, Vec::<PathBuf>::new());
    assert!(!command.no_compose_discovery);
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
        "--network-state-dir",
        "/var/lib/nimbus/network",
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
    assert_eq!(
        command.network_state_dir,
        Some(PathBuf::from("/var/lib/nimbus/network"))
    );
    assert_eq!(command.compose_file, vec![PathBuf::from("./compose.yaml")]);
    assert!(command.once);
    assert!(command.skip_codegen);
    assert!(command.debug_node_apis);
    assert_eq!(command.tail_logs, DevTailLogsMode::Disable);
    assert!(command.no_open, "--no-open should opt out of auto-open");
}

#[test]
#[serial_test::serial]
fn dev_network_root_honors_discovered_start_config_before_claim() {
    let workspace = tempdir().expect("temporary workspace should exist");
    let configured_root = workspace.path().join("configured-network");
    fs::write(
        workspace.path().join("nimbus.yaml"),
        format!("network:\n  state_dir: {}\n", configured_root.display()),
    )
    .expect("network config should write");

    with_current_dir(workspace.path(), || {
        let command = parse_dev(["nimbus", "dev"]);
        let resolved = resolve_dev_network_root(&command).expect("dev network root should resolve");
        assert_eq!(resolved.as_path(), configured_root);
        assert!(
            !configured_root.exists(),
            "config resolution must not mutate the network root"
        );
    });
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
fn cli_parses_compose_discovery_opt_out() {
    let command = parse_dev(["nimbus", "dev", "--no-compose-discovery"]);

    assert!(command.no_compose_discovery);
    assert_eq!(command.compose_file, Vec::<PathBuf>::new());
}

#[test]
fn compose_discovery_opt_out_conflicts_with_explicit_file() {
    let error = Cli::try_parse_from([
        "nimbus",
        "dev",
        "--no-compose-discovery",
        "--compose-file",
        "./compose.yaml",
    ])
    .expect_err("the opt-out and an explicit Compose file must conflict");

    assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
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
    assert!(rendered.contains("--network-state-dir"));
    assert!(rendered.contains("--once"));
    assert!(rendered.contains("--tail-logs"));
    assert!(rendered.contains("debounced codegen reruns"));
    assert!(rendered.contains("locally activates"));
    assert!(rendered.contains("runtime log multiplexing"));
    assert!(rendered.contains("COMPOSE_FILE"));
    assert!(rendered.contains("--no-compose-discovery"));
}
