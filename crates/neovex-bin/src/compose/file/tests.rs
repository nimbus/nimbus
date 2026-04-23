use super::parse::compose_lifecycle_spec;
use super::*;
use crate::compose::discovery::resolve_compose_selection;

fn write_compose_fixture(tempdir: &tempfile::TempDir, name: &str, contents: &str) -> PathBuf {
    let path = tempdir.path().join(name);
    fs::write(&path, contents).expect("fixture file should write");
    path
}

#[test]
fn compose_project_resolves_image_and_build_services() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    write_compose_fixture(
        &tempdir,
        "db.env",
        "FROM_ENV=from-file\nOVERRIDE_ME=from-env-file\n",
    );
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
name: Demo App
services:
  db:
    image: postgres:16
    env_file:
      - ./db.env
    environment:
      POSTGRES_PASSWORD: secret
      OVERRIDE_ME: inline
    ports:
      - "5432:5432"
      - "127.0.0.1:15433:5433/tcp"
    deploy:
      resources:
        limits:
          cpus: "1.0"
          memory: 256M
    restart: on-failure:3
    depends_on:
      cache:
        condition: service_healthy
    healthcheck:
      test:
        - CMD
        - pg_isready
        - -U
        - postgres
      interval: 10s
    stop_grace_period: 30s
    labels:
      app.role: database
    x_neovex:
      snapshot: true
  api:
    build:
      context: .
      dockerfile: Dockerfile.api
    command: ["./server"]
    entrypoint: ["/bin/sh", "-lc"]
    working_dir: /workspace
    user: "1000:1000"
    deploy:
      resources:
        limits:
          cpus: "0.5"
          memory: 128M
volumes:
  pgdata: {}
"#,
    );

    let project = ComposeProjectPlan::load(&compose).expect("compose file should resolve");
    assert_eq!(project.project_name, "demo-app");
    assert_eq!(project.volumes, vec!["pgdata".to_owned()]);

    let db = project.services.get("db").expect("db service should exist");
    assert_eq!(db.backend, SandboxBackendKind::Krun);
    assert_eq!(
        db.source,
        ComposeLaunchPlan::Image {
            image_reference: "postgres:16".to_owned(),
        }
    );
    assert_eq!(
        db.process.environment.get("FROM_ENV"),
        Some(&"from-file".to_owned())
    );
    assert_eq!(
        db.process.environment.get("OVERRIDE_ME"),
        Some(&"inline".to_owned())
    );
    assert_eq!(db.ports.len(), 2);
    assert_eq!(db.ports[0].name, "default");
    assert_eq!(db.ports[0].host_port, 5432);
    assert_eq!(db.ports[0].guest_port, 5432);
    assert_eq!(db.resources.cpu_count, Some(1));
    assert_eq!(db.resources.memory_limit_bytes, Some(256 * 1024 * 1024));
    assert_eq!(
        db.restart.policy,
        SandboxRestartPolicy::OnFailure { max_restarts: 3 }
    );
    assert_eq!(
        db.depends_on.get("cache"),
        Some(&ComposeDependencyCondition::ServiceHealthy)
    );
    assert_eq!(
        db.healthcheck
            .as_ref()
            .and_then(|healthcheck| healthcheck.interval.as_deref()),
        Some("10s")
    );
    assert_eq!(db.stop_grace_period.as_deref(), Some("30s"));
    assert_eq!(db.labels.get("app.role"), Some(&"database".to_owned()));
    assert_eq!(
        db.x_neovex
            .as_ref()
            .and_then(|extensions| extensions.snapshot),
        Some(true)
    );

    let api = project
        .services
        .get("api")
        .expect("api service should exist");
    assert_eq!(
        api.source,
        ComposeLaunchPlan::Build {
            image_name: "neovex-demo-app-api".to_owned(),
            dockerfile_path: tempdir.path().join("Dockerfile.api"),
            context_path: tempdir.path().to_path_buf(),
        }
    );
    assert_eq!(api.process.user.as_deref(), Some("1000:1000"));
    assert_eq!(
        api.process.working_dir.as_ref(),
        Some(&PathBuf::from("/workspace"))
    );
    assert_eq!(
        api.process.command.as_ref(),
        Some(&ComposeCommandPlan::List(vec!["./server".to_owned()]))
    );
    assert_eq!(api.resources.cpu_count, Some(1));
    assert!(
        api.warnings
            .iter()
            .any(|warning| warning.contains("rounded 0.5 up to 1 vCPU")),
        "expected fractional CPU rounding warning, got {:?}",
        api.warnings
    );
}

#[test]
fn compose_project_reports_ignored_fields() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  db:
    image: postgres:16
    networks:
      - default
    privileged: true
    logging:
      driver: json-file
"#,
    );

    let project = ComposeProjectPlan::load(&compose).expect("compose file should resolve");
    let db = project.services.get("db").expect("db service should exist");
    assert!(
        db.warnings
            .iter()
            .any(|warning| warning.contains("networks")),
        "expected network warning, got {:?}",
        db.warnings
    );
    assert!(
        db.warnings
            .iter()
            .any(|warning| warning.contains("privileged")),
        "expected privileged warning, got {:?}",
        db.warnings
    );
    assert!(
        db.warnings
            .iter()
            .any(|warning| warning.contains("logging")),
        "expected logging warning, got {:?}",
        db.warnings
    );
}

#[test]
fn compose_project_allows_backend_selection_through_x_neovex() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    image: busybox:latest
    x_neovex:
      backend: container
"#,
    );

    let project = ComposeProjectPlan::load(&compose).expect("compose file should resolve");
    let api = project
        .services
        .get("api")
        .expect("api service should exist");

    assert_eq!(api.backend, SandboxBackendKind::Container);
    assert_eq!(
        api.x_neovex
            .as_ref()
            .and_then(|extensions| extensions.backend),
        Some(SandboxBackendKind::Container)
    );
}

#[test]
fn compose_project_rejects_invalid_memory_values() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  db:
    image: postgres:16
    deploy:
      resources:
        limits:
          memory: abc
"#,
    );

    let error = ComposeProjectPlan::load(&compose).expect_err("invalid memory should fail");
    assert!(
        error
            .to_string()
            .contains("Expected format: 256M, 1G, etc."),
        "expected actionable memory error, got: {error}"
    );
}

#[test]
fn render_compose_project_services_lists_names_and_warnings() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  cache:
    image: redis:7
  db:
    image: postgres:16
    networks:
      - default
"#,
    );

    let rendered = render_compose_project(&compose, true).expect("service names should render");
    assert_eq!(rendered.stdout, "cache\ndb\n");
    assert!(
        rendered
            .warnings
            .iter()
            .any(|warning| warning.contains("services.db")),
        "expected service warning to surface in list mode, got {:?}",
        rendered.warnings
    );
}

#[test]
fn compose_project_load_selection_merges_auto_discovered_override_files() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let base = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
name: Demo App
services:
  api:
    image: busybox:latest
    command: ["./base"]
    environment:
      BASE_ONLY: from-base
      OVERRIDE_ME: base
    ports:
      - "8080:80"
    labels:
      layer: base
    x_neovex:
      snapshot: true
volumes:
  shared: {}
"#,
    );
    write_compose_fixture(
        &tempdir,
        "compose.override.yaml",
        r#"
services:
  api:
    command: ["./override"]
    environment:
      OVERRIDE_ME: override
      OVERRIDE_ONLY: from-override
    ports:
      - "8081:81"
    labels:
      role: api
    x_neovex:
      idle_timeout: 30s
  worker:
    image: redis:7
"#,
    );
    let selection = resolve_compose_selection(&[], tempdir.path())
        .expect("selection should resolve")
        .expect("selection should exist");

    let project =
        ComposeProjectPlan::load_selection(&selection).expect("merged selection should load");

    assert_eq!(project.source_file, base);
    assert_eq!(project.project_name, "demo-app");
    assert_eq!(project.volumes, vec!["shared".to_owned()]);

    let api = project
        .services
        .get("api")
        .expect("api service should exist");
    assert_eq!(
        api.process.command,
        Some(ComposeCommandPlan::List(vec!["./override".to_owned()]))
    );
    assert_eq!(
        api.process.environment.get("BASE_ONLY"),
        Some(&"from-base".to_owned())
    );
    assert_eq!(
        api.process.environment.get("OVERRIDE_ME"),
        Some(&"override".to_owned())
    );
    assert_eq!(
        api.process.environment.get("OVERRIDE_ONLY"),
        Some(&"from-override".to_owned())
    );
    assert_eq!(api.ports.len(), 2);
    assert_eq!(api.labels.get("layer"), Some(&"base".to_owned()));
    assert_eq!(api.labels.get("role"), Some(&"api".to_owned()));
    assert_eq!(
        api.x_neovex
            .as_ref()
            .and_then(|extensions| extensions.snapshot),
        Some(true)
    );
    assert_eq!(
        api.x_neovex
            .as_ref()
            .and_then(|extensions| extensions.idle_timeout.as_deref()),
        Some("30s")
    );
    assert!(
        project.services.contains_key("worker"),
        "override service should merge into the project"
    );
}

#[test]
fn render_compose_project_selection_renders_merged_services() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    write_compose_fixture(
        &tempdir,
        "compose.yaml",
        "services:\n  api:\n    image: busybox:latest\n",
    );
    write_compose_fixture(
        &tempdir,
        "compose.override.yaml",
        "services:\n  worker:\n    image: redis:7\n",
    );
    let selection = resolve_compose_selection(&[], tempdir.path())
        .expect("selection should resolve")
        .expect("selection should exist");

    let rendered = render_compose_project_selection(&selection, false)
        .expect("rendered compose config should resolve");

    assert!(rendered.stdout.contains("source_file:"));
    assert!(rendered.stdout.contains("compose.yaml"));
    assert!(rendered.stdout.contains("api:"));
    assert!(rendered.stdout.contains("worker:"));
}

#[test]
fn compose_process_plan_lowers_to_image_process_overrides() {
    let process = ComposeProcessPlan {
        entrypoint: Some(ComposeCommandPlan::List(vec![
            "/bin/sh".to_owned(),
            "-lc".to_owned(),
        ])),
        command: Some(ComposeCommandPlan::String(
            "exec ./server --port 8080".to_owned(),
        )),
        environment: BTreeMap::from([
            ("APP_ENV".to_owned(), "dev".to_owned()),
            ("LOG_LEVEL".to_owned(), "debug".to_owned()),
        ]),
        working_dir: Some(PathBuf::from("/workspace")),
        user: Some("1000:1000".to_owned()),
    };

    let overrides = process
        .to_image_process_overrides()
        .expect("compose process should lower");

    assert_eq!(
        overrides.entrypoint,
        Some(vec!["/bin/sh".to_owned(), "-lc".to_owned()])
    );
    assert_eq!(
        overrides.cmd,
        Some(vec![
            "exec".to_owned(),
            "./server".to_owned(),
            "--port".to_owned(),
            "8080".to_owned()
        ])
    );
    assert_eq!(
        overrides.env,
        vec!["APP_ENV=dev".to_owned(), "LOG_LEVEL=debug".to_owned(),]
    );
    assert_eq!(overrides.cwd, Some(PathBuf::from("/workspace")));
    assert_eq!(overrides.user.as_deref(), Some("1000:1000"));
}

#[test]
fn compose_process_plan_rejects_empty_override_commands() {
    let process = ComposeProcessPlan {
        entrypoint: None,
        command: Some(ComposeCommandPlan::List(Vec::new())),
        environment: BTreeMap::new(),
        working_dir: None,
        user: None,
    };

    let error = process
        .to_image_process_overrides()
        .expect_err("empty command override should be rejected");
    assert!(
        error
            .to_string()
            .contains("empty command/entrypoint overrides"),
        "expected actionable empty override error, got: {error}"
    );
}

#[test]
fn compose_service_plan_lowers_stop_grace_period_into_sandbox_lifecycle() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  db:
    image: postgres:16
    restart: on-failure:3
    stop_grace_period: 1m30s
"#,
    );

    let project = ComposeProjectPlan::load(&compose).expect("compose file should resolve");
    let service = project.services.get("db").expect("db service should exist");
    let lifecycle = compose_lifecycle_spec(
        &service.restart,
        service.stop_grace_period.as_deref(),
        "services.db.stop_grace_period",
    )
    .expect("compose lifecycle should lower");

    assert_eq!(
        lifecycle.restart_policy,
        SandboxRestartPolicy::OnFailure { max_restarts: 3 }
    );
    assert_eq!(lifecycle.stop_timeout, Some(Duration::from_secs(90)));
}

#[test]
fn compose_project_rejects_invalid_stop_grace_period() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  db:
    image: postgres:16
    stop_grace_period: later
"#,
    );

    let error = ComposeProjectPlan::load(&compose).expect_err("invalid stop grace should fail");
    assert!(
        error.to_string().contains("services.db.stop_grace_period"),
        "expected field-scoped stop_grace_period error, got: {error}"
    );
}

#[test]
fn compose_project_lowers_into_sandbox_service_catalog() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
name: Demo App
services:
  db:
    image: postgres:16
    ports:
      - "5432:5432"
    deploy:
      resources:
        limits:
          cpus: "1.0"
          memory: 256M
    restart: on-failure:3
    stop_grace_period: 30s
  api:
    build:
      context: .
      dockerfile: Dockerfile.api
    command: ["./server"]
    entrypoint: ["/bin/sh", "-lc"]
    working_dir: /workspace
    user: "1000:1000"
"#,
    );
    std::fs::write(tempdir.path().join("Dockerfile.api"), "FROM scratch\n")
        .expect("dockerfile fixture should be writable");

    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    let catalog = ComposeProjectPlan::load(&compose)
        .expect("compose file should resolve")
        .into_service_catalog()
        .expect("compose project should lower into a service catalog");

    assert_eq!(catalog.project.project_name, "demo-app");

    let db = catalog
        .sandbox_service_for_tenant(&tenant_id, "db")
        .expect("db launch should exist");
    match db {
        SandboxServiceLaunch::Image(launch) => {
            assert_eq!(launch.image_reference, "postgres:16");
            assert_eq!(launch.spec.tenant_id, tenant_id);
            assert_eq!(launch.spec.name, "db");
            assert_eq!(launch.spec.resources.cpu_count, Some(1));
            assert_eq!(
                launch.spec.resources.memory_limit_bytes,
                Some(256 * 1024 * 1024)
            );
            assert_eq!(
                launch.spec.lifecycle.restart_policy,
                SandboxRestartPolicy::OnFailure { max_restarts: 3 }
            );
            assert_eq!(
                launch.spec.lifecycle.stop_timeout,
                Some(Duration::from_secs(30))
            );
            assert_eq!(launch.spec.port_bindings.len(), 1);
            assert_eq!(launch.spec.port_bindings[0].host_port, 5432);
            assert_eq!(launch.spec.port_bindings[0].guest_port, 5432);
        }
        SandboxServiceLaunch::Build(_) => panic!("db should lower as an image-backed launch"),
    }

    let api = catalog
        .sandbox_service_for_tenant(&tenant_id, "api")
        .expect("api launch should exist");
    match api {
        SandboxServiceLaunch::Build(launch) => {
            assert_eq!(launch.image_name, "neovex-demo-app-api");
            assert_eq!(
                launch.dockerfile_path,
                tempdir.path().join("Dockerfile.api")
            );
            assert_eq!(launch.context_path, tempdir.path());
            assert_eq!(
                launch.process_overrides.entrypoint,
                Some(vec!["/bin/sh".to_owned(), "-lc".to_owned()])
            );
            assert_eq!(
                launch.process_overrides.cmd,
                Some(vec!["./server".to_owned()])
            );
            assert_eq!(
                launch.process_overrides.cwd,
                Some(PathBuf::from("/workspace"))
            );
            assert_eq!(launch.process_overrides.user.as_deref(), Some("1000:1000"));
        }
        SandboxServiceLaunch::Image(_) => panic!("api should lower as a build-backed launch"),
    }

    let other_tenant = TenantId::new("other").expect("tenant id should be valid");
    let other_db = catalog
        .sandbox_service_for_tenant(&other_tenant, "db")
        .expect("catalog should lower the same service plan for another tenant");
    match other_db {
        SandboxServiceLaunch::Image(launch) => {
            assert_eq!(launch.spec.tenant_id, other_tenant);
            assert_eq!(launch.spec.name, "db");
        }
        SandboxServiceLaunch::Build(_) => panic!("db should stay image-backed across tenants"),
    }
}
