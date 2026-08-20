use super::*;

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
        nimbus_tenant::TenantIsolationMode::LocalDevelopment,
        "dev should preserve Node-compatible localhost grants explicitly"
    );
}

#[test]
fn dev_plan_keeps_explicit_data_and_control_roots_distinct() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");

    let plan = resolve_dev_plan(
        parse_dev([
            "nimbus",
            "dev",
            "--data-dir",
            "./tenant-data",
            "--control-data-dir",
            "./control-data",
        ]),
        temp.path(),
    )
    .expect("dev plan should resolve distinct persistence roots");

    assert_eq!(
        plan.start_command.data_dir,
        Some(temp.path().join("tenant-data"))
    );
    assert_eq!(
        plan.start_command.control_data_dir,
        Some(temp.path().join("control-data"))
    );
    assert!(
        crate::wire_credentials::wire_credentials_path(&temp.path().join("control-data")).exists(),
        "dev and start must share the control-root wire credential store"
    );
    assert!(
        !crate::wire_credentials::wire_credentials_path(&temp.path().join("tenant-data")).exists(),
        "tenant data must not gain a duplicate wire credential authority"
    );
}

#[test]
fn dev_plan_retains_every_advertised_wire_socket_until_start_handoff() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");

    let mut plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");
    let advertised_ports = [
        plan.wire.mongodb_port.port,
        plan.wire.dynamodb_port.port,
        plan.wire.s3_port.port,
    ];
    assert_eq!(plan.start_command.mongodb_port, Some(advertised_ports[0]));
    assert_eq!(plan.start_command.dynamodb_port, Some(advertised_ports[1]));
    assert_eq!(plan.start_command.s3_port, Some(advertised_ports[2]));
    for port in advertised_ports {
        let competing = std::net::TcpListener::bind(("127.0.0.1", port));
        assert!(
            matches!(
                competing,
                Err(ref error) if error.kind() == std::io::ErrorKind::AddrInUse
            ),
            "advertised dev wire port {port} must remain held by its exact handoff socket"
        );
    }

    let listeners = plan
        .start_command
        .prebound_wire_listeners
        .take()
        .expect("dev start command should carry its retained listener bundle");
    listeners
        .close_and_settle()
        .expect("discarded dev plan should close and settle every listener");
    // These ports are the kernel's, not the test's: an undetected wire surface
    // takes a provider-assigned port, so there is nothing here for a
    // `PortWindow` to claim in advance. Holding each re-bound socket until the
    // loop ends at least proves all three are free at the same moment rather
    // than one at a time.
    let _released: Vec<std::net::TcpListener> = advertised_ports
        .into_iter()
        .map(|port| {
            std::net::TcpListener::bind(("127.0.0.1", port))
                .expect("explicit plan cancellation should release the advertised socket")
        })
        .collect();
}

#[test]
fn dev_start_command_inherits_conservative_runtime_host_budget() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");
    let start_defaults = StartCommand::default();

    assert_eq!(
        plan.start_command.runtime_host_millicpus, start_defaults.runtime_host_millicpus,
        "dev must inherit the same host capacity default as nimbus start"
    );
    assert_eq!(
        plan.start_command.runtime_system_reserve_millicpus,
        start_defaults.runtime_system_reserve_millicpus,
        "dev must preserve the start-side host OS reserve"
    );
    assert_eq!(
        plan.start_command.runtime_control_plane_reserve_millicpus,
        start_defaults.runtime_control_plane_reserve_millicpus,
        "dev must preserve the start-side Nimbus control-plane reserve"
    );
    assert_eq!(
        plan.start_command.runtime_hard_ceiling_millicpus,
        start_defaults.runtime_hard_ceiling_millicpus,
        "dev must not invent a separate runtime hard-ceiling policy"
    );
    assert_eq!(
        plan.start_command.runtime_seat_millicpus, start_defaults.runtime_seat_millicpus,
        "dev must use the same runtime dispatch seat size as nimbus start"
    );
    assert!(
        plan.start_command.runtime_system_reserve_millicpus > 0
            && plan.start_command.runtime_control_plane_reserve_millicpus > 0,
        "dev must carry a conservative non-runtime CPU reserve"
    );
}

#[test]
fn dev_start_command_inherits_disabled_runtime_adaptive_controller() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");
    let start_defaults = StartCommand::default();

    assert_eq!(
        plan.start_command.runtime_adaptive_mode, start_defaults.runtime_adaptive_mode,
        "dev must inherit start's adaptive-controller mode instead of inventing a dev speed knob"
    );
    assert_eq!(
        plan.start_command.runtime_adaptive_canary_percent,
        start_defaults.runtime_adaptive_canary_percent,
        "dev must not canary live adaptive actuation by default"
    );
    assert_eq!(
        plan.start_command.runtime_adaptive_rollback, start_defaults.runtime_adaptive_rollback,
        "dev must use the same rollback default as nimbus start"
    );
}

#[test]
fn firestore_client_plan_maps_discovered_project_to_auto_tenant() {
    let temp = tempdir().expect("tempdir should build");
    // Real apps live in git repos; the boundary keeps the app-dir
    // walk-up from escaping the fixture.
    fs::create_dir_all(temp.path().join(".git")).expect(".git boundary should create");
    fs::write(
        temp.path().join("package.json"),
        r#"{"dependencies": {"firebase": "^11.0.0"}}"#,
    )
    .expect("package.json should write");
    fs::write(
        temp.path().join(".firebaserc"),
        r#"{"projects": {"default": "acme-staging"}}"#,
    )
    .expect(".firebaserc should write");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");

    assert_eq!(plan.adapter, Some(DevAdapter::FirestoreClient));
    assert_eq!(
        plan.start_command.auto_tenant,
        Some("acme-staging".to_string()),
        "the auto-created tenant must be the tenant the app's requests resolve to"
    );
    let mapping = plan
        .firestore_tenant
        .as_ref()
        .expect("firestore client plan should carry the tenant mapping");
    assert_eq!(mapping.tenant, "acme-staging");
    assert_eq!(
        mapping.source,
        firebase_project::ProjectTenantSource::FirebaseRc
    );
    assert!(
        dev_banner_lines(&plan)
            .iter()
            .any(|line| line == "Tenant:     acme-staging (.firebaserc default project)"),
        "banner must name the mapped tenant and its source"
    );
}

#[test]
fn firestore_client_plan_falls_back_to_demo_tenant() {
    let temp = tempdir().expect("tempdir should build");
    fs::create_dir_all(temp.path().join(".git")).expect(".git boundary should create");
    fs::write(
        temp.path().join("package.json"),
        r#"{"dependencies": {"firebase": "^11.0.0"}}"#,
    )
    .expect("package.json should write");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");

    assert_eq!(plan.start_command.auto_tenant, Some("demo".to_string()));
    assert_eq!(
        plan.firestore_tenant
            .as_ref()
            .expect("firestore client plan should carry the tenant mapping")
            .source,
        firebase_project::ProjectTenantSource::DemoFallback
    );
    assert!(
        dev_banner_lines(&plan)
            .iter()
            .any(|line| line == "Tenant:     demo (no Firebase project id found)"),
        "banner must state the demo fallback"
    );
}

#[test]
fn firebaserc_marks_the_app_root_for_nested_cwd() {
    let temp = tempdir().expect("tempdir should build");
    fs::create_dir_all(temp.path().join(".git")).expect(".git boundary should create");
    fs::write(
        temp.path().join("package.json"),
        r#"{"dependencies": {"firebase": "^11.0.0"}}"#,
    )
    .expect("package.json should write");
    fs::write(
        temp.path().join(".firebaserc"),
        r#"{"projects": {"default": "acme-staging"}}"#,
    )
    .expect(".firebaserc should write");
    let nested = temp.path().join("src").join("components");
    fs::create_dir_all(&nested).expect("nested cwd should create");

    let plan =
        resolve_dev_plan(parse_dev(["nimbus", "dev"]), &nested).expect("dev plan should resolve");

    assert_eq!(
        plan.app_dir,
        temp.path()
            .canonicalize()
            .expect("app dir should canonicalize"),
        ".firebaserc must mark the app root from a nested cwd"
    );
    assert_eq!(plan.adapter, Some(DevAdapter::FirestoreClient));
    assert_eq!(
        plan.start_command.auto_tenant,
        Some("acme-staging".to_string())
    );
}

#[test]
fn non_firestore_apps_keep_demo_tenant_despite_project_signals() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");
    fs::write(
        temp.path().join(".firebaserc"),
        r#"{"projects": {"default": "acme-staging"}}"#,
    )
    .expect(".firebaserc should write");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");

    assert!(matches!(plan.adapter, Some(DevAdapter::Convex { .. })));
    assert_eq!(
        plan.start_command.auto_tenant,
        Some("demo".to_string()),
        "projectId mapping applies only to the Firestore client adapter"
    );
    assert!(plan.firestore_tenant.is_none());
}

fn firestore_client_fixture(temp: &Path) {
    fs::create_dir_all(temp.join(".git")).expect(".git boundary should create");
    fs::write(
        temp.join("package.json"),
        r#"{"dependencies": {"firebase": "^11.0.0"}}"#,
    )
    .expect("package.json should write");
    fs::write(
        temp.join(".firebaserc"),
        r#"{"projects": {"default": "acme-staging"}}"#,
    )
    .expect(".firebaserc should write");
}

#[test]
fn firestore_client_banner_states_endpoint_and_omits_watch_line() {
    let temp = tempdir().expect("tempdir should build");
    firestore_client_fixture(temp.path());

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");
    let lines = dev_banner_lines(&plan);
    assert!(
        lines
            .iter()
            .any(|line| line == "Adapter:    firestore-client"),
        "banner must state the adapter: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "Tenant:     acme-staging (.firebaserc default project)"),
        "banner must state the mapped tenant: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line
            == "Firestore:  http://localhost:3210/v1/projects/acme-staging/databases/(default)/documents"),
        "banner must state the Firestore endpoint: {lines:?}"
    );
    assert!(
        lines.iter().all(|line| !line.starts_with("Watch:")),
        "a client app has no server sources; the banner must not claim watching: {lines:?}"
    );

    let once_plan = resolve_dev_plan(parse_dev(["nimbus", "dev", "--once"]), temp.path())
        .expect("dev plan should resolve");
    assert!(
        dev_banner_lines(&once_plan)
            .iter()
            .all(|line| !line.starts_with("Watch:")),
        "--once must not reintroduce a Watch line for client apps"
    );
}

#[test]
fn firestore_client_start_command_omits_app_dir_so_start_accepts_it() {
    // Start rejects explicit app dirs without a Convex or Cloud
    // Functions surface, and its codegen preflight only acts on a
    // resolved app dir — so handing start no app dir is both what
    // makes `nimbus dev` boot for a client app and what guarantees
    // no codegen ever writes `_generated/` into it.
    let temp = tempdir().expect("tempdir should build");
    firestore_client_fixture(temp.path());

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");
    assert!(
        plan.start_command.app_dir.is_none(),
        "client apps must hand start no app dir"
    );
    let resolved = crate::start::resolve_start_app_dir(&plan.start_command)
        .expect("start must accept a firestore client start command");
    assert!(
        resolved.is_none(),
        "no resolved app dir means the codegen preflight and registry loads stay off"
    );
    assert!(
        !temp.path().join("_generated").exists(),
        "plan resolution must not create codegen artifacts in the app dir"
    );
}

#[tokio::test]
async fn firestore_client_long_running_watch_loop_stays_idle() {
    // The dev loop races the server against the watch loop in a
    // select!. If the empty-roots branch ever completed instead of
    // idling, the select! would resolve and the long-running server
    // would exit at startup for every client app.
    let temp = tempdir().expect("tempdir should build");
    firestore_client_fixture(temp.path());

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");
    let watch_plan = plan.watch_plan();
    assert!(
        plan.initial_watch_roots().is_empty(),
        "client apps have no server sources to watch"
    );
    let (_roots_tx, roots_rx) = tokio::sync::watch::channel(plan.initial_watch_roots());
    let outcome = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        run_dev_watch_loop(watch_plan, roots_rx),
    )
    .await;
    assert!(
        outcome.is_err(),
        "the watch loop must idle forever with no source roots"
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
fn compose_discovery_defaults_to_enabled() {
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
        dev_selection
            .files
            .iter()
            .map(|path| fs::canonicalize(path).unwrap())
            .collect::<Vec<_>>()
    );
}

#[test]
fn compose_discovery_opt_out_performs_no_discovery() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");
    fs::write(
        temp.path().join("docker-compose.yaml"),
        "services:\n  first:\n    image: busybox:latest\n",
    )
    .expect("first ambiguous Compose fixture should write");
    fs::write(
        temp.path().join("docker-compose.yml"),
        "services:\n  second:\n    image: redis:7\n",
    )
    .expect("second ambiguous Compose fixture should write");
    let nested_cwd = temp.path().join("convex");

    let cli = Cli::try_parse_from(["nimbus", "dev", "--no-compose-discovery"])
        .expect("the explicit Compose-discovery opt-out should parse");
    let Command::Dev(command) = cli.command else {
        panic!("dev subcommand should parse");
    };
    let plan = resolve_dev_plan(*command, &nested_cwd)
        .expect("the opt-out must not inspect the ambiguous Compose project");

    assert_eq!(plan.compose_selection, None);
    assert_eq!(plan.start_command.compose_file, Vec::<PathBuf>::new());
}

#[test]
fn dev_builds_ordered_desired_intents_for_compose_services() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");
    fs::write(
        temp.path().join("compose.yaml"),
        r#"
name: Demo Stack
services:
  api:
    image: ghcr.io/acme/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  db:
    image: docker.io/library/postgres@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
"#,
    )
    .expect("compose fixture should write");
    let nested_cwd = temp.path().join("convex");

    let dev_plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), &nested_cwd)
        .expect("dev plan should resolve");
    assert_eq!(
        dev_plan.start_command.tenant_isolation_mode,
        nimbus_tenant::TenantIsolationMode::LocalDevelopment
    );
    assert_eq!(
        dev_plan.start_command.compose_file,
        vec![temp.path().join("compose.yaml")]
    );

    let selection = dev_plan
        .compose_selection
        .as_ref()
        .expect("dev should auto-discover compose");
    let workload_plan = crate::workload_boot::plan_compose_services(
        selection,
        &dev_plan.data_dir,
        dev_plan.start_command.tenant_isolation_mode,
        &crate::workload_boot::default_local_node_capacity().expect("local node should build"),
    )
    .expect("dev workload-control plan should resolve");
    let workload_ids = workload_plan
        .desired_workloads()
        .iter()
        .map(|workload| workload.workload_id().to_owned())
        .collect::<Vec<_>>();

    assert_eq!(
        workload_plan.compose_files(),
        &[temp.path().join("compose.yaml")]
    );
    assert_eq!(workload_ids, ["service:api", "service:db"]);
}

#[test]
fn explicit_compose_file_still_loads() {
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
