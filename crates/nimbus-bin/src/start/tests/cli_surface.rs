use super::*;

#[test]
fn cli_defaults_to_embedded_sqlite() {
    let cli = parse_start(["nimbus", "start"]);
    let config = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("default sqlite config should build");
    assert_eq!(
        config,
        nimbus::EnginePersistenceConfig::embedded("./data", nimbus::EmbeddedProviderKind::Sqlite)
    );
}

#[test]
fn cors_allow_origin_flag_repeats_and_normalizes() {
    let command = parse_start([
        "nimbus",
        "start",
        "--cors-allow-origin",
        "https://App.Example.com/",
        "--cors-allow-origin",
        "http://app.example.com:8080",
    ]);
    assert_eq!(
        command.cors_allow_origin,
        vec![
            "https://app.example.com".to_string(),
            "http://app.example.com:8080".to_string(),
        ],
        "flag values should be normalized at parse time"
    );
    assert!(
        StartCommand::default().cors_allow_origin.is_empty(),
        "no extra CORS origins by default"
    );
}

#[test]
fn cors_allow_origin_flag_rejects_wildcards_and_bare_hosts() {
    for bad in ["*", "https://*.example.com", "app.example.com"] {
        let error = Cli::try_parse_from(["nimbus", "start", "--cors-allow-origin", bad])
            .expect_err(&format!("origin `{bad}` should be rejected at parse time"));
        assert_eq!(error.kind(), ErrorKind::ValueValidation);
    }
}

#[test]
fn cors_env_fallback_applies_only_without_flags() {
    // Flag wins: the env var is not consulted when flags are present.
    let with_flag = parse_start([
        "nimbus",
        "start",
        "--cors-allow-origin",
        "https://app.example.com",
    ]);
    assert_eq!(
        super::boot::resolve_cors_allowed_origins(&with_flag)
            .expect("flag-provided origins should resolve"),
        vec!["https://app.example.com".to_string()],
    );
    // Without flag or env, no extra origins. (Env-set behavior is covered
    // by normalization tests; mutating process env in tests races other
    // threads, so the env path is exercised through the pure normalizer.)
    let without_flag = parse_start(["nimbus", "start"]);
    if std::env::var_os("NIMBUS_CORS_ALLOW_ORIGINS").is_none() {
        assert_eq!(
            super::boot::resolve_cors_allowed_origins(&without_flag)
                .expect("empty configuration should resolve"),
            Vec::<String>::new(),
        );
    }
}

#[test]
fn tls_flags_are_both_or_neither() {
    let command = parse_start([
        "nimbus",
        "start",
        "--tls-cert",
        "/etc/nimbus/cert.pem",
        "--tls-key",
        "/etc/nimbus/key.pem",
    ]);
    assert!(command.tls_cert.is_some() && command.tls_key.is_some());

    for partial in [
        vec!["nimbus", "start", "--tls-cert", "/etc/nimbus/cert.pem"],
        vec!["nimbus", "start", "--tls-key", "/etc/nimbus/key.pem"],
    ] {
        let error = Cli::try_parse_from(partial)
            .expect_err("a lone TLS flag must be rejected at parse time");
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
    }
    assert!(
        StartCommand::default().tls_cert.is_none(),
        "TLS stays off by default"
    );
}

#[test]
fn start_command_default_has_no_auto_tenant() {
    let command = StartCommand::default();
    assert!(
        command.auto_tenant.is_none(),
        "start should not auto-create a tenant by default"
    );
}

#[test]
fn start_command_default_uses_production_tenant_isolation() {
    let command = StartCommand::default();
    assert_eq!(
        command.tenant_isolation_mode,
        nimbus_server::TenantIsolationMode::Production,
        "start is the production-oriented server entrypoint; dev opts out explicitly"
    );
}

#[test]
fn production_start_compose_manager_rejects_tag_only_image_before_backend_setup() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = tempdir.path().join("compose.yaml");
    fs::write(&compose, "services:\n  api:\n    image: busybox:latest\n")
        .expect("compose fixture should write");
    let selection = crate::compose::discovery::ResolvedComposeSelection::explicit(compose);

    let error = match super::boot::load_service_manager(
        Some(&selection),
        &tempdir.path().join("control"),
        nimbus_server::TenantIsolationMode::Production,
    ) {
        Ok(_) => panic!("production compose manager should reject tag-only images"),
        Err(error) => error,
    };

    assert!(
        error.to_string().contains("digest-pinned OCI image"),
        "expected production image admission error, got: {error}"
    );
}

#[test]
fn cli_requires_explicit_start_subcommand_for_server_flags() {
    assert!(Cli::try_parse_from(["nimbus"]).is_err());
    assert!(Cli::try_parse_from(["nimbus", "--compose-file", "./compose.dev.yaml"]).is_err());
}

#[test]
fn retired_serve_namespace_is_not_supported() {
    let error = Cli::try_parse_from(["nimbus", "serve", "--help"])
        .expect_err("retired serve namespace should not parse");
    assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
}

#[test]
fn cli_supports_top_level_version_flag() {
    let error = Cli::try_parse_from(["nimbus", "--version"])
        .expect_err("top-level version flag should short-circuit with display output");
    assert_eq!(error.kind(), ErrorKind::DisplayVersion);
    assert_eq!(
        error.to_string(),
        format!("nimbus {}\n", env!("CARGO_PKG_VERSION"))
    );
}

fn root_help_lists_command(rendered: &str, command: &str, description: &str) -> bool {
    rendered.lines().any(|line| {
        let line = line.trim_start();
        line.strip_prefix(command).is_some_and(|rest| {
            rest.split_whitespace().collect::<Vec<_>>().join(" ") == description
        })
    })
}

#[test]
fn cli_help_describes_codegen_machine_and_compose_surface() {
    let error = Cli::try_parse_from(["nimbus", "--help"]).expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains(
        "Convex-compatible reactive backend with local development and Compose-backed services"
    ));
    assert!(rendered.contains("Usage:"));
    assert!(rendered.contains("Available Commands:"));
    assert!(rendered.contains("Examples:"));
    assert!(rendered.contains("nimbus start"));
    assert!(rendered.contains("nimbus dev"));
    assert!(rendered.contains("nimbus run"));
    assert!(rendered.contains("nimbus codegen --app ./demos/convex/html"));
    assert!(rendered.contains("nimbus token rotate"));
    assert!(rendered.contains("nimbus machine start"));
    assert!(rendered.contains("nimbus compose up"));
    assert!(rendered.contains("start"));
    assert!(rendered.contains("dev"));
    assert!(rendered.contains("run"));
    assert!(rendered.contains("sandbox"));
    assert!(rendered.contains("codegen"));
    assert!(rendered.contains("token"));
    assert!(
        root_help_lists_command(
            &rendered,
            "object-storage",
            "Manage Nimbus object-storage placement, backup, restore, and GC"
        ),
        "root help should describe the object-storage command:\n{rendered}"
    );
    assert!(
        root_help_lists_command(&rendered, "machine", "Manage local developer machines"),
        "root help should describe the machine command:\n{rendered}"
    );
    assert!(rendered.contains("compose"));
    assert!(!rendered.contains("node-workload-executor"));
    assert!(!rendered.contains("sandbox-supervisor"));
}

#[test]
fn cli_parses_start_command_with_optional_compose_file() {
    let cli = parse_start(["nimbus", "start", "--compose-file", "./compose.dev.yaml"]);
    assert_eq!(cli.compose_file, vec![PathBuf::from("./compose.dev.yaml")]);
}

#[test]
fn cli_parses_start_command_with_multiple_compose_files_in_order() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--compose-file",
        "./compose.yaml",
        "--compose-file",
        "./compose.dev.yaml",
    ]);
    assert_eq!(
        cli.compose_file,
        vec![
            PathBuf::from("./compose.yaml"),
            PathBuf::from("./compose.dev.yaml")
        ]
    );
}

#[test]
fn cli_parses_start_command_with_app_dir() {
    let cli = parse_start(["nimbus", "start", "--app-dir", "./demos/convex/html"]);
    assert_eq!(cli.app_dir, Some(PathBuf::from("./demos/convex/html")));
}

#[test]
fn cli_defaults_start_host_to_loopback_and_accepts_explicit_host() {
    let default_cli = parse_start(["nimbus", "start"]);
    assert_eq!(default_cli.host, "127.0.0.1");

    let explicit_cli = parse_start(["nimbus", "start", "--host", "0.0.0.0"]);
    assert_eq!(explicit_cli.host, "0.0.0.0");
}

#[test]
fn cli_parses_start_command_with_skip_codegen() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--app-dir",
        "./demos/convex/html",
        "--skip-codegen",
    ]);
    assert_eq!(cli.app_dir, Some(PathBuf::from("./demos/convex/html")));
    assert!(cli.skip_codegen);
}

#[test]
fn cli_parses_per_tenant_runtime_budget_flags() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--runtime-max-active-per-tenant",
        "2",
        "--runtime-max-in-flight-per-tenant",
        "4",
        "--runtime-max-queued-per-tenant",
        "8",
    ]);

    assert_eq!(cli.runtime_max_active_per_tenant, 2);
    assert_eq!(cli.runtime_max_in_flight_per_tenant, 4);
    assert_eq!(cli.runtime_max_queued_per_tenant, 8);
}

#[test]
fn cli_parses_runtime_host_budget_policy_flags() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--runtime-host-millicpus",
        "8000",
        "--runtime-system-reserve-millicpus",
        "1000",
        "--runtime-control-plane-reserve-millicpus",
        "1500",
        "--runtime-hard-ceiling-millicpus",
        "2500",
        "--runtime-seat-millicpus",
        "500",
    ]);

    assert_eq!(cli.runtime_host_millicpus, 8000);
    assert_eq!(cli.runtime_system_reserve_millicpus, 1000);
    assert_eq!(cli.runtime_control_plane_reserve_millicpus, 1500);
    assert_eq!(cli.runtime_hard_ceiling_millicpus, Some(2500));
    assert_eq!(cli.runtime_seat_millicpus, 500);
}

#[test]
fn cli_parses_start_operator_policy_path() {
    let cli = parse_start(["nimbus", "start", "--policy", "./nimbus.policy.yaml"]);

    assert_eq!(cli.policy, Some(PathBuf::from("./nimbus.policy.yaml")));
}

#[test]
fn cli_parses_runtime_adaptive_operator_controls() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--runtime-adaptive-mode",
        "canary",
        "--runtime-adaptive-canary-percent",
        "5",
        "--runtime-adaptive-rollback",
    ]);

    assert_eq!(cli.runtime_adaptive_mode, CliRuntimeAdaptiveMode::Canary);
    assert_eq!(cli.runtime_adaptive_canary_percent, 5);
    assert!(cli.runtime_adaptive_rollback);
}

#[test]
fn cli_rejects_runtime_adaptive_canary_percent_above_one_hundred() {
    let error = Cli::try_parse_from([
        "nimbus",
        "start",
        "--runtime-adaptive-canary-percent",
        "101",
    ])
    .expect_err("adaptive canary percent must be bounded");
    assert_eq!(error.kind(), ErrorKind::ValueValidation);
}

#[test]
fn cli_rejects_zero_runtime_host_capacity_or_seat() {
    for flag in ["--runtime-host-millicpus", "--runtime-seat-millicpus"] {
        let error = Cli::try_parse_from(["nimbus", "start", flag, "0"])
            .expect_err(&format!("{flag}=0 must be rejected at parse time"));
        assert_eq!(error.kind(), ErrorKind::ValueValidation);
    }
}

#[test]
fn start_does_not_accept_runtime_efficiency_profile_knobs() {
    for flag in [
        "--runtime-profile",
        "--runtime-pool-kind",
        "--runtime-execution-model",
        "--runtime-reset-strategy",
    ] {
        let error = Cli::try_parse_from(["nimbus", "start", flag, "node_full"])
            .expect_err(&format!("{flag} must not parse as a start flag"));
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }
}

#[test]
fn runtime_limits_from_command_applies_per_tenant_runtime_budgets() {
    let command = StartCommand {
        runtime_max_instances: 8,
        runtime_worker_threads: 16,
        runtime_max_active_per_tenant: 3,
        runtime_max_in_flight_per_tenant: 5,
        runtime_max_queued_per_tenant: 7,
        ..StartCommand::default()
    };

    let limits = super::super::runtime_limits::runtime_limits_from_command(&command).normalized();

    assert_eq!(limits.max_active_top_level_invocations_per_tenant, 3);
    assert_eq!(limits.max_in_flight_top_level_invocations_per_tenant, 5);
    assert_eq!(limits.max_queued_top_level_invocations_per_tenant, 7);
}

#[test]
fn runtime_host_resource_budget_from_command_applies_operator_policy() {
    let command = StartCommand {
        runtime_max_instances: 16,
        runtime_host_millicpus: 8000,
        runtime_system_reserve_millicpus: 1000,
        runtime_control_plane_reserve_millicpus: 1500,
        runtime_hard_ceiling_millicpus: Some(2500),
        runtime_seat_millicpus: 500,
        ..StartCommand::default()
    };

    let budget = super::super::runtime_limits::runtime_host_resource_budget_from_command(&command);

    assert_eq!(budget.host_millicpus, 8000);
    assert_eq!(budget.system_reserved_millicpus, 1000);
    assert_eq!(budget.nimbus_control_plane_reserved_millicpus, 1500);
    assert_eq!(budget.runtime_hard_ceiling_millicpus, Some(2500));
    assert_eq!(budget.runtime_seat_millicpus.get(), 500);
    assert_eq!(budget.runtime_allocatable_millicpus(), 2500);
    assert_eq!(
        budget.nominal_dispatch_seats(command.runtime_max_instances),
        5
    );
}

#[test]
fn start_function_scaling_admission_keeps_selector_overrides() {
    let command = StartCommand {
        runtime_host_millicpus: 2_000,
        runtime_system_reserve_millicpus: 500,
        runtime_control_plane_reserve_millicpus: 0,
        runtime_seat_millicpus: 250,
        ..StartCommand::default()
    };
    let runtime_config: RuntimeConfigFile = serde_yaml::from_str(
        r#"
functions:
  scaling:
    overrides:
      "messages:send":
        preset: latency
        reason: "primary write path"
"#,
    )
    .expect("runtime config should parse");
    let runtime_limits = super::super::runtime_limits::runtime_limits_from_command(&command);
    let runtime_host_budget =
        super::super::runtime_limits::runtime_host_resource_budget_from_command(&command);

    let admission = super::boot::admit_start_function_scaling_plans(
        &command,
        &runtime_config,
        &runtime_limits,
        runtime_host_budget,
        None,
    )
    .expect("start function scaling should admit");

    assert_eq!(admission.plans.default_plan().function, "__default__");
    assert_eq!(admission.plans.default_plan().effective.min_warm, 0);
    assert_eq!(admission.plans.function_override_count(), 1);
    let hot_plan = admission.plans.plan_for_function("messages:send");
    assert_eq!(hot_plan.function, "messages:send");
    assert_eq!(hot_plan.effective.min_warm, 1);
    assert_eq!(hot_plan.effective.max_warm, 4);
    assert!(hot_plan.effective.autoscaling);
    assert_eq!(
        admission
            .plans
            .plan_for_function("messages:list")
            .effective
            .min_warm,
        admission.plans.default_plan().effective.min_warm
    );
}

#[test]
fn start_function_scaling_admission_uses_explicit_operator_policy() {
    let command = StartCommand::default();
    let runtime_config: RuntimeConfigFile = serde_yaml::from_str(
        r#"
functions:
  scaling:
    overrides:
      "messages:send":
        min_warm: 1
        max_warm: 4
        reason: "hot path"
"#,
    )
    .expect("runtime config should parse");
    let operator_policy: nimbus_server::OperatorPolicyDocument = serde_yaml::from_str(
        r#"
schema_version: 1
tenant: tenant-a
defaults:
  runtime_resources:
    cpu_millicpus: 1000
    memory_bytes: 536870912
    storage_bytes: 10737418240
    host_cpu_reserve_millicpus: 250
    host_memory_reserve_bytes: 134217728
  runtime_safety:
    max_warm_per_function: 2
workloads:
  - kind: runtime_function
    name: messages:send
"#,
    )
    .expect("operator policy should parse");
    let runtime_limits = super::super::runtime_limits::runtime_limits_from_command(&command);
    let runtime_host_budget =
        super::super::runtime_limits::runtime_host_resource_budget_from_command(&command);

    let error = super::boot::admit_start_function_scaling_plans(
        &command,
        &runtime_config,
        &runtime_limits,
        runtime_host_budget,
        Some(&operator_policy),
    )
    .expect_err("explicit operator policy should reject over-limit max_warm");

    assert!(error.to_string().contains("requested max_warm=4"));
    assert!(
        error
            .to_string()
            .contains("operator effective max_warm_per_function=2")
    );
}

#[test]
fn runtime_adaptive_controller_settings_from_command_applies_operator_policy() {
    let command = StartCommand {
        runtime_adaptive_mode: CliRuntimeAdaptiveMode::Canary,
        runtime_adaptive_canary_percent: 5,
        runtime_adaptive_rollback: true,
        ..StartCommand::default()
    };

    let settings =
        super::super::runtime_limits::runtime_adaptive_controller_settings_from_command(&command);

    assert_eq!(
        settings.mode(),
        nimbus::RuntimeAdaptiveControllerMode::Canary
    );
    assert!(settings.live_adaptive_defaults_enabled());
    assert_eq!(settings.canary_policy().admitted_remainders, 5);
    assert!(settings.rollback_to_static_defaults());
}

#[test]
fn start_command_default_has_conservative_runtime_host_budget() {
    let command = StartCommand::default();
    let budget = super::super::runtime_limits::runtime_host_resource_budget_from_command(&command);

    assert_eq!(budget.host_millicpus, command.runtime_host_millicpus);
    assert!(
        budget.system_reserved_millicpus > 0,
        "default host budget should leave CPU for the host OS"
    );
    assert!(
        budget.nimbus_control_plane_reserved_millicpus > 0,
        "default host budget should leave CPU for Nimbus control-plane work"
    );
    assert_eq!(budget.runtime_hard_ceiling_millicpus, None);
    assert_eq!(budget.runtime_seat_millicpus.get(), 1000);
    assert!(
        budget.runtime_allocatable_millicpus() < budget.host_millicpus,
        "default allocatable runtime CPU should reserve non-runtime host capacity"
    );
}

/// Summary-shape tests pass an opted-out enablement; the resolution
/// behind it (and the populated status lines) is covered by the
/// `adapters` module's own tests.
fn adapterless_enablement() -> crate::start::adapters::AdapterEnablement {
    crate::start::adapters::AdapterEnablement {
        firebase: None,
        cloudflare: None,
        mongodb: None,
        dynamodb: None,
        s3: None,
    }
}

#[test]
fn start_startup_summary_mentions_url_app_codegen_and_deploy_api() {
    let command = StartCommand {
        port: 0,
        app_dir: Some(PathBuf::from("./app")),
        skip_codegen: true,
        compose_file: vec![PathBuf::from("./compose.yaml")],
        deploy_admin_token: Some("dev-token".to_string()),
        ..StartCommand::default()
    };

    let lines = super::boot::start_startup_summary_lines(
        &command,
        Some(&super::boot::ResolvedStartAppDir::Explicit(PathBuf::from(
            "./app",
        ))),
        Some(
            &crate::compose::discovery::ResolvedComposeSelection::explicit(PathBuf::from(
                "./compose.yaml",
            )),
        ),
        &adapterless_enablement(),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        true,
    );

    assert!(
        lines
            .iter()
            .any(|line| line == "Nimbus server listening at http://localhost:3210/")
    );
    assert!(lines.iter().any(|line| line == "app dir: ./app"));
    assert!(
        lines
            .iter()
            .any(|line| line == "codegen preflight: skipped by --skip-codegen")
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "tenant isolation:\tproduction")
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("runtime host budget:\t"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "compose file: ./compose.yaml")
    );
    assert!(lines.iter().any(|line| line == "deploy admin API: enabled"));
    // The adapter status lines flow into the banner — one per surface,
    // honest about an opted-out boot.
    assert!(lines.iter().any(|line| line == "firestore routes:\toff"));
    assert!(lines.iter().any(|line| line == "mongodb listener:\toff"));
    assert!(lines.iter().any(|line| line == "dynamodb listener:\toff"));
}

#[test]
fn start_startup_summary_mentions_runtime_host_budget() {
    let command = StartCommand {
        runtime_host_millicpus: 8000,
        runtime_system_reserve_millicpus: 1000,
        runtime_control_plane_reserve_millicpus: 1000,
        runtime_hard_ceiling_millicpus: Some(2500),
        runtime_seat_millicpus: 500,
        ..StartCommand::default()
    };

    let lines = super::boot::start_startup_summary_lines(
        &command,
        None,
        None,
        &adapterless_enablement(),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        false,
    );

    assert!(lines.iter().any(|line| {
        line == "runtime host budget:\t2500m allocatable CPU (8000m host - 1000m system reserve - 1000m Nimbus control-plane reserve; hard ceiling 2500m; seat 500m)"
    }));
}

#[test]
fn start_startup_summary_mentions_baked_function_scaling_defaults() {
    let start_lines = super::boot::start_startup_summary_lines(
        &StartCommand::default(),
        None,
        None,
        &adapterless_enablement(),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        false,
    );
    assert!(start_lines.iter().any(|line| {
        line
            == "Function scaling: start defaults, min_warm=0, max_warm=auto, scale_down_delay=600s, autoscaling inferred=true. Run nimbus explain functions <name>."
    }));

    let dev_command = StartCommand {
        tenant_isolation_mode: nimbus_server::TenantIsolationMode::LocalDevelopment,
        ..StartCommand::default()
    };
    let dev_lines = super::boot::start_startup_summary_lines(
        &dev_command,
        None,
        None,
        &adapterless_enablement(),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        false,
    );
    assert!(dev_lines.iter().any(|line| {
        line
            == "Function scaling: dev defaults, min_warm=0, max_warm=auto, scale_down_delay=120s, autoscaling inferred=true. Run nimbus explain functions <name>."
    }));
}

#[test]
fn start_startup_summary_reports_auto_discovered_override_companion() {
    let command = StartCommand::default();
    let selection = crate::compose::discovery::ResolvedComposeSelection {
        origin: crate::compose::discovery::ComposeSelectionOrigin::AutoDiscovered,
        project_root: PathBuf::from("/workspace"),
        files: vec![
            PathBuf::from("/workspace/compose.yaml"),
            PathBuf::from("/workspace/compose.override.yaml"),
        ],
        display_files: vec![
            PathBuf::from("/workspace/compose.yaml"),
            PathBuf::from("/workspace/compose.override.yaml"),
        ],
    };

    let lines = super::boot::start_startup_summary_lines(
        &command,
        None,
        Some(&selection),
        &adapterless_enablement(),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        false,
    );

    assert!(lines.iter().any(|line| {
        line == "compose file: auto-discovered /workspace/compose.yaml (+ compose.override.yaml)"
    }));
}

#[test]
fn start_startup_summary_emits_operator_console_url_line() {
    // CD7(h) — the operator-console banner is the contract that
    // documentation, the install card, and Electron all hang off of. A
    // missing `/ui/` suffix or a missing `operator console:` label would
    // silently regress that contract, so this test asserts both literally.
    // See cli-daemon-canonicalization plan, CD3 / CD7(h).
    let command = StartCommand {
        port: 0,
        ..StartCommand::default()
    };

    let lines = super::boot::start_startup_summary_lines(
        &command,
        None,
        None,
        &adapterless_enablement(),
        SocketAddr::from((Ipv4Addr::LOCALHOST, 4711)),
        false,
    );

    let console_line = lines
        .iter()
        .find(|line| line.starts_with("operator console:"))
        .expect("startup banner must contain an `operator console:` line");
    assert!(
        console_line.contains("/ui/"),
        "operator-console URL must end at /ui/, got: {console_line}"
    );
    assert!(
        console_line.contains("127.0.0.1:4711"),
        "operator-console URL must reflect the listener address, got: {console_line}"
    );
}

#[test]
fn start_startup_summary_reports_no_app_dir_when_none_resolved() {
    // Post-CD1: `nimbus start` returns Ok(None) when no `--app-dir`
    // is passed (no source-tree walk-up). The banner must clearly
    // state that Convex-compatible routes wait for deploy activation
    // rather than implying an autodetect happened.
    let command = StartCommand::default();
    let lines = super::boot::start_startup_summary_lines(
        &command,
        None,
        None,
        &adapterless_enablement(),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        false,
    );

    assert!(lines.iter().any(|line| {
        line == "app dir: none; Convex-compatible routes wait for deploy activation"
    }));
}

#[test]
fn start_startup_summary_reports_compose_file_environment_selection() {
    let command = StartCommand::default();
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

    let lines = super::boot::start_startup_summary_lines(
        &command,
        None,
        Some(&selection),
        &adapterless_enablement(),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        false,
    );

    assert!(lines.iter().any(|line| {
        line == "compose file: COMPOSE_FILE=./compose.yaml (+ 1 extra Compose files)"
    }));
}

#[test]
fn start_does_not_auto_admit_ambient_compose() {
    let temp = tempfile::tempdir().expect("tempdir should build");
    let project_root = temp.path().join("workspace");
    let nested_cwd = project_root.join("apps").join("web");
    let app_dir = temp.path().join("separate-app");
    fs::create_dir_all(&nested_cwd).expect("nested cwd should build");
    fs::create_dir_all(app_dir.join("convex")).expect("app dir should build");
    fs::write(
        project_root.join("compose.yaml"),
        "name: demo\nservices:\n  db:\n    image: busybox:latest\n",
    )
    .expect("compose fixture should write");
    let command = StartCommand {
        app_dir: Some(app_dir),
        compose_file: Vec::new(),
        ..StartCommand::default()
    };

    let selection = with_current_dir(&nested_cwd, || {
        super::boot::resolve_optional_compose_selection(&command)
    })
    .expect("compose selection should resolve");

    assert!(
        selection.is_none(),
        "nimbus start must not auto-admit an ambient Compose project from cwd/app-dir"
    );
}

#[test]
fn start_compose_selection_prefers_explicit_flag_over_auto_discovery() {
    let temp = tempfile::tempdir().expect("tempdir should build");
    let nested_cwd = temp.path().join("apps").join("web");
    fs::create_dir_all(&nested_cwd).expect("nested cwd should build");
    fs::write(
        temp.path().join("compose.yaml"),
        "name: auto\nservices:\n  db:\n    image: busybox:latest\n",
    )
    .expect("auto compose fixture should write");
    let explicit_path = nested_cwd.join("compose.custom.yaml");
    fs::write(
        &explicit_path,
        "name: explicit\nservices:\n  db:\n    image: redis:7\n",
    )
    .expect("explicit compose fixture should write");
    let command = StartCommand {
        compose_file: vec![PathBuf::from("./compose.custom.yaml")],
        ..StartCommand::default()
    };

    let selection = with_current_dir(&nested_cwd, || {
        super::boot::resolve_optional_compose_selection(&command)
    })
    .expect("compose selection should resolve")
    .expect("compose selection should exist");

    assert_eq!(
        fs::canonicalize(selection.primary_file()).unwrap(),
        fs::canonicalize(&explicit_path).unwrap()
    );
    assert_eq!(selection.files.len(), 1);
}

#[test]
fn cli_parses_codegen_command_with_default_app_dir() {
    let cli = parse_codegen(["nimbus", "codegen"]);
    assert_eq!(cli.app, PathBuf::from("."));
    assert!(!cli.debug_node_apis);
}

#[test]
fn cli_parses_codegen_command_with_explicit_app_dir() {
    let cli = parse_codegen([
        "nimbus",
        "codegen",
        "--app",
        "./demos/convex/html",
        "--debug-node-apis",
    ]);
    assert_eq!(cli.app, PathBuf::from("./demos/convex/html"));
    assert!(cli.debug_node_apis);
}

#[test]
fn cli_rejects_removed_convex_app_dir_flag() {
    let error = Cli::try_parse_from(["nimbus", "start", "--convex-app-dir", "./demo"])
        .expect_err("removed app-dir flag should be removed");
    assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    assert!(error.to_string().contains("--convex-app-dir"));
}

#[test]
fn start_help_shows_app_dir_flag_name() {
    let error =
        Cli::try_parse_from(["nimbus", "start", "--help"]).expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("--host"));
    assert!(rendered.contains("--app-dir"));
    assert!(rendered.contains("--skip-codegen"));
    assert!(rendered.contains("nimbus start --app-dir ./demos/convex/html"));
    assert!(rendered.contains("nimbus start --app-dir ./demos/convex/html --skip-codegen"));
    assert!(rendered.contains("COMPOSE_FILE"));
    assert!(!rendered.contains("--convex-app-dir"));
}
