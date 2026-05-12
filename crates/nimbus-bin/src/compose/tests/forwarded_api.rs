use super::*;

#[test]
fn require_krun_backend_rejects_container_only_projects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture_with_body(
        temp_dir.path(),
        r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_nimbus:
      backend: container
"#,
    );
    let control_data_dir = temp_dir.path().join("control");
    let context = load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");

    let error = require_krun_backend_for_service_operation(
        &context,
        None,
        "load a compose-backed sandbox manager",
    )
    .expect_err("container-only project should fail fast");

    assert_eq!(
        error.to_string(),
        "invalid input: compose project demo-app selects sandbox backend container, but nimbus load a compose-backed sandbox manager only supports the krun backend today"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_loader_accepts_container_projects_with_ready_forwarded_machine_api_on_macos() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture_with_body(
        temp_dir.path(),
        r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_nimbus:
      backend: container
"#,
    );
    let control_data_dir = temp_dir.path().join("control");
    let context = load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");
    let socket_path = temp_dir.path().join("default-api.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("machine-control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
        helper_binary_dirs: default_guest_helper_binary_dirs(),
        service_backend: Some(Arc::new(StubMachineApiSandboxBackend)),
        machine_port_forwarder: None,
    };
    write_fake_runtime_binaries(temp_dir.path());
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    let client = MachineApiClient::new(socket_path.clone());

    wait_for_machine_api_health(&client);
    let _manager = load_host_backed_sandbox_service_manager_for_platform(
        &compose_path,
        &control_data_dir,
        ServiceHostPlatform::Macos,
        Some(client.clone()),
    )
    .expect("host loader should accept ready container backend");
    let backend =
        load_host_backed_project_backend(&context, ServiceHostPlatform::Macos, Some(client))
            .expect("project backend should load");
    assert_eq!(backend.kind(), SandboxBackendKind::Container);

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_loader_accepts_default_projects_with_ready_forwarded_machine_api_on_macos() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture(temp_dir.path());
    let control_data_dir = temp_dir.path().join("control");
    let context = load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");
    let socket_path = temp_dir.path().join("default-api.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("machine-control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
        helper_binary_dirs: default_guest_helper_binary_dirs(),
        service_backend: Some(Arc::new(StubMachineApiSandboxBackend)),
        machine_port_forwarder: None,
    };
    write_fake_runtime_binaries(temp_dir.path());
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    let client = MachineApiClient::new(socket_path);

    wait_for_machine_api_health(&client);
    let backend =
        load_host_backed_project_backend(&context, ServiceHostPlatform::Macos, Some(client))
            .expect("host loader should accept default macOS service backend");
    assert_eq!(backend.kind(), SandboxBackendKind::Container);

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_loader_reports_machine_api_readiness_blockers_for_container_projects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture_with_body(
        temp_dir.path(),
        r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_nimbus:
      backend: container
"#,
    );
    let control_data_dir = temp_dir.path().join("control");
    let context = load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");
    let socket_path = temp_dir.path().join("default-api.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("machine-control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
        helper_binary_dirs: default_guest_helper_binary_dirs(),
        service_backend: None,
        machine_port_forwarder: None,
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    let client = MachineApiClient::new(socket_path);

    wait_for_machine_api_health(&client);
    let error = match load_host_backed_project_backend(
        &context,
        ServiceHostPlatform::Macos,
        Some(client),
    ) {
        Ok(_) => panic!("container backend should reject unready machine API"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("not ready for container-backed service execution"),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .contains("guest machine API does not yet expose service lifecycle operations"),
        "{error}"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn macos_service_up_uses_forwarded_machine_api_for_container_projects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture_with_body(
        temp_dir.path(),
        r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_nimbus:
      backend: container
"#,
    );
    let control_data_dir = temp_dir.path().join("control");
    let socket_path = temp_dir.path().join("default-api.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("machine-control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
        helper_binary_dirs: default_guest_helper_binary_dirs(),
        service_backend: Some(Arc::new(StubMachineApiSandboxBackend)),
        machine_port_forwarder: None,
    };
    write_fake_runtime_binaries(temp_dir.path());
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    let client = MachineApiClient::new(socket_path);
    wait_for_machine_api_health(&client);

    let rendered_up = render_service_up_for_platform(
        &ComposeUpCommand {
            service: Some("db".to_owned()),
            file: vec![compose_path],
            tenant: None,
        },
        &control_data_dir,
        ServiceHostPlatform::Macos,
        Some(client),
    )
    .await
    .expect("compose up should render");
    assert!(
        rendered_up.contains("Compose up completed for project demo-app"),
        "{rendered_up}"
    );
    assert!(
        rendered_up.contains("db: started (sandbox db-01stub, status ready)"),
        "{rendered_up}"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn macos_service_up_uses_forwarded_machine_api_for_default_projects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture(temp_dir.path());
    let control_data_dir = temp_dir.path().join("control");
    let socket_path = temp_dir.path().join("default-api.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let state = MachineApiState {
        control_data_dir: temp_dir.path().join("machine-control"),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
        helper_binary_dirs: default_guest_helper_binary_dirs(),
        service_backend: Some(Arc::new(StubMachineApiSandboxBackend)),
        machine_port_forwarder: None,
    };
    write_fake_runtime_binaries(temp_dir.path());
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    let client = MachineApiClient::new(socket_path);
    wait_for_machine_api_health(&client);

    let rendered_up = render_service_up_for_platform(
        &ComposeUpCommand {
            service: Some("db".to_owned()),
            file: vec![compose_path],
            tenant: None,
        },
        &control_data_dir,
        ServiceHostPlatform::Macos,
        Some(client),
    )
    .await
    .expect("compose up should render for default macOS backend");
    assert!(
        rendered_up.contains("Compose up completed for project demo-app"),
        "{rendered_up}"
    );
    assert!(
        rendered_up.contains("db: started (sandbox db-01stub, status ready)"),
        "{rendered_up}"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

#[test]
fn macos_effective_backend_preserves_explicit_krun_selection() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture_with_body(
        temp_dir.path(),
        r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_nimbus:
      backend: krun
"#,
    );
    let control_data_dir = temp_dir.path().join("control");
    let context = load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");

    let surface = resolve_service_execution_surface(
        &context,
        Some("db"),
        "compose up",
        ServiceHostPlatform::Macos,
        None,
    )
    .expect("explicit krun selection should remain local");

    assert!(
        matches!(surface, ServiceExecutionSurface::Krun { .. }),
        "explicit macOS krun selection should not be rewritten to container"
    );
}

#[test]
fn macos_host_loader_auto_starts_default_machine_only_for_container_projects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let container_compose_path = write_compose_fixture_with_body(
        temp_dir.path(),
        r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_nimbus:
      backend: container
"#,
    );
    let krun_compose_path = temp_dir.path().join("compose-krun.yaml");
    fs::write(
        &krun_compose_path,
        r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_nimbus:
      backend: krun
"#,
    )
    .expect("krun compose fixture should write");
    let control_data_dir = temp_dir.path().join("control");
    let container_context =
        load_compose_project_context(&container_compose_path, &control_data_dir)
            .expect("container compose project context should load");
    let krun_context = load_compose_project_context(&krun_compose_path, &control_data_dir)
        .expect("krun compose project context should load");

    assert!(
        should_auto_start_default_machine_for_host_loader(
            &container_context,
            ServiceHostPlatform::Macos,
        )
        .expect("container compose project should evaluate"),
        "container-backed macOS start should auto-start the default machine"
    );
    assert!(
        !should_auto_start_default_machine_for_host_loader(
            &krun_context,
            ServiceHostPlatform::Macos,
        )
        .expect("krun compose project should evaluate"),
        "krun-backed macOS start should stay on the local backend"
    );
    assert!(
        !should_auto_start_default_machine_for_host_loader(
            &container_context,
            ServiceHostPlatform::Linux,
        )
        .expect("linux compose project should evaluate"),
        "non-macOS start should not auto-start the default machine"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn macos_service_commands_use_forwarded_machine_api_for_container_projects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture_with_body(
        temp_dir.path(),
        r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_nimbus:
      backend: container
"#,
    );
    let control_data_dir = temp_dir.path().join("control");
    let context = load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");
    let machine_control_data_dir = temp_dir.path().join("machine-control");
    let socket_path = temp_dir.path().join("default-api.sock");
    let listener = bind_direct_listener(&socket_path).expect("listener should bind");
    let mut backend_config = ContainerSandboxBackendConfig::under_root(
        machine_control_data_dir
            .join("service-sandboxes")
            .join("container"),
    );
    backend_config.launch_mode = ContainerLaunchMode::PlanOnly;
    let state = MachineApiState {
        control_data_dir: machine_control_data_dir.clone(),
        listen_mode: MachineApiListenMode::DirectSocket,
        binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
        helper_binary_dirs: default_guest_helper_binary_dirs(),
        service_backend: Some(Arc::new(ContainerSandboxBackend::new(backend_config))),
        machine_port_forwarder: None,
    };
    write_fake_runtime_binaries(temp_dir.path());
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_machine_api(listener, state, async move {
        let _ = shutdown_rx.await;
    }));
    let client = MachineApiClient::new(socket_path);
    wait_for_machine_api_health(&client);
    let container_dir = write_container_machine_manifest(
        &machine_control_data_dir
            .join("service-sandboxes")
            .join("container")
            .join("state"),
        "db-01aaa",
        context.control_plane.local_tenant_id.as_str(),
        "db",
        SandboxStatus::Ready,
    );
    fs::write(container_dir.join("ctr.log"), "guest log line\n")
        .expect("guest ctr.log should write");
    fs::write(container_dir.join("pidfile"), "2002\n").expect("pidfile should write");
    fs::write(container_dir.join("conmon.pid"), "1001\n").expect("conmon pidfile should write");

    let rendered_list = render_service_list_for_platform(
        &ComposePsCommand {
            file: vec![compose_path.clone()],
            format: ComposePsOutputFormat::Table,
            no_heading: false,
            all_tenants: false,
        },
        &control_data_dir,
        ServiceHostPlatform::Macos,
        Some(client.clone()),
    )
    .expect("compose ps should render");
    assert!(
        rendered_list.contains(context.control_plane.local_tenant_id.as_str()),
        "{rendered_list}"
    );
    assert!(rendered_list.contains("SERVICE"), "{rendered_list}");
    assert!(rendered_list.contains("db"), "{rendered_list}");

    let rendered_inspect = render_service_inspect_for_platform(
        &ComposeInspectCommand {
            service: "db".to_owned(),
            file: vec![compose_path.clone()],
            tenant: None,
            format: ComposeInspectOutputFormat::Json,
        },
        &control_data_dir,
        ServiceHostPlatform::Macos,
        Some(client.clone()),
    )
    .expect("compose inspect should render");
    assert!(
        rendered_inspect.contains("\"service_name\": \"db\""),
        "{rendered_inspect}"
    );
    assert!(rendered_inspect.contains("ctr.log"), "{rendered_inspect}");

    let current = client
        .inspect_current_service_sandbox(&context.control_plane.local_tenant_id, "db")
        .expect("current sandbox lookup should succeed")
        .details
        .expect("current sandbox should exist");
    fs::write(&current.log_paths.ctr_log, "guest log line\n").expect("guest ctr.log should write");
    fs::write(current.state_dir.join("pidfile"), "4294967294\n").expect("pidfile should write");
    fs::write(current.state_dir.join("conmon.pid"), "4294967295\n")
        .expect("conmon pidfile should write");

    let rendered_top = render_compose_top_for_platform(
        &ComposeTopCommand {
            service: "db".to_owned(),
            file: vec![compose_path.clone()],
            tenant: None,
            format: ComposeTopOutputFormat::Table,
            no_heading: false,
        },
        &control_data_dir,
        ServiceHostPlatform::Macos,
        Some(client.clone()),
    )
    .expect("compose top should render");
    assert!(
        rendered_top.contains("runtime pid: 4294967294"),
        "{rendered_top}"
    );
    assert!(
        rendered_top.contains("conmon pid: 4294967295"),
        "{rendered_top}"
    );
    assert!(
        rendered_top.contains("tracked processes: none"),
        "{rendered_top}"
    );

    let rendered_down = render_service_down_for_platform(
        &ComposeDownCommand {
            service: Some("db".to_owned()),
            file: vec![compose_path],
            tenant: None,
        },
        &control_data_dir,
        ServiceHostPlatform::Macos,
        Some(client),
    )
    .await
    .expect("compose down should render");
    assert!(
        rendered_down.contains("Compose down completed for project demo-app"),
        "{rendered_down}"
    );
    assert!(
        rendered_down.contains("db: stopped (sandbox db-01aaa, status stopped)"),
        "{rendered_down}"
    );

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("machine API server task should join")
        .expect("machine API server should shut down cleanly");
}

#[test]
fn project_wide_service_operations_reject_mixed_backend_projects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture_with_body(
        temp_dir.path(),
        r#"
name: Demo App
services:
  api:
    image: busybox:latest
  db:
    image: busybox:latest
    x_nimbus:
      backend: container
"#,
    );
    let control_data_dir = temp_dir.path().join("control");
    let context = load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");

    let error = require_krun_backend_for_service_operation(&context, None, "compose up")
        .expect_err("mixed backend project should fail fast");

    assert_eq!(
        error.to_string(),
        "invalid input: compose project demo-app mixes sandbox backends across services (api=krun, db=container); nimbus compose up currently requires one backend family per project-wide operation"
    );
}

#[test]
fn service_scoped_operations_allow_mixed_projects_when_requested_service_is_krun() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture_with_body(
        temp_dir.path(),
        r#"
name: Demo App
services:
  api:
    image: busybox:latest
  db:
    image: busybox:latest
    x_nimbus:
      backend: container
"#,
    );
    let control_data_dir = temp_dir.path().join("control");
    let context = load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");

    require_krun_backend_for_service_operation(&context, Some("api"), "compose up")
        .expect("krun service in mixed project should remain allowed");

    let error = require_krun_backend_for_service_operation(&context, Some("db"), "compose up")
        .expect_err("container service should fail fast");
    assert_eq!(
        error.to_string(),
        "invalid input: service db in compose project demo-app selects sandbox backend container, but nimbus compose up only supports the krun backend today"
    );
}
