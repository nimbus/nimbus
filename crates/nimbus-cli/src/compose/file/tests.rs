use super::parse::{compose_lifecycle_spec, parse_compose_duration};
use super::*;
use crate::compose::discovery::{ResolvedComposeSelection, resolve_compose_selection};

fn write_compose_fixture(tempdir: &tempfile::TempDir, name: &str, contents: &str) -> PathBuf {
    let path = tempdir.path().join(name);
    fs::write(&path, contents).expect("fixture file should write");
    path
}

fn sandbox_spec_from_backend(service_backend: ServiceBackend) -> SandboxSpec {
    match service_backend {
        ServiceBackend::Sandbox(spec) => *spec,
        other => panic!("service should lower as sandbox-backed, got {other:?}"),
    }
}

fn image_reference_from_spec(spec: &SandboxSpec) -> &str {
    match &spec.root {
        SandboxRootSpec::OciImage(image) => match &image.source {
            SandboxOciImageSource::Reference(reference) => reference.reference.as_str(),
            SandboxOciImageSource::Build(_) => {
                panic!("service should lower as image-reference-backed")
            }
        },
        SandboxRootSpec::Rootfs(_) => panic!("service should lower as OCI-image-backed"),
    }
}

fn build_source_from_spec(spec: &SandboxSpec) -> &SandboxOciBuildSpec {
    match &spec.root {
        SandboxRootSpec::OciImage(image) => match &image.source {
            SandboxOciImageSource::Build(build) => build,
            SandboxOciImageSource::Reference(_) => panic!("service should lower as build-backed"),
        },
        SandboxRootSpec::Rootfs(_) => panic!("service should lower as OCI-image-backed"),
    }
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
    x_nimbus:
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
        db.x_nimbus
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
            image_name: "nimbus-demo-app-api".to_owned(),
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
fn compose_project_allows_backend_selection_through_x_nimbus() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    image: busybox:latest
    x_nimbus:
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
        api.x_nimbus
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
fn compose_project_lowers_x_nimbus_disk_and_log_limits_into_sandbox_resources() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  db:
    image: postgres:16
    x-nimbus:
      disk_limit: 2G
      log_limit: 32M
"#,
    );

    let tenant_id = TenantId::new("tenant-a").expect("tenant id should be valid");
    let catalog = ComposeProjectPlan::load(&compose)
        .expect("compose file should resolve")
        .into_service_catalog()
        .expect("compose project should lower into a service catalog");
    let spec = sandbox_spec_from_backend(
        catalog
            .service_definition_for_tenant(&tenant_id, "db")
            .expect("db definition should exist")
            .backend,
    );
    assert_eq!(image_reference_from_spec(&spec), "postgres:16");

    assert_eq!(
        spec.resources.disk_limit_bytes,
        Some(2 * 1024 * 1024 * 1024)
    );
    assert_eq!(spec.resources.log_limit_bytes, Some(32 * 1024 * 1024));
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
    x_nimbus:
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
    x_nimbus:
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
        api.x_nimbus
            .as_ref()
            .and_then(|extensions| extensions.snapshot),
        Some(true)
    );
    assert_eq!(
        api.x_nimbus
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
fn compose_process_plan_lowers_to_sandbox_process_spec() {
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

    let process = process
        .to_process_spec()
        .expect("compose process should lower");

    assert_eq!(
        process.entrypoint,
        Some(vec!["/bin/sh".to_owned(), "-lc".to_owned()])
    );
    assert_eq!(
        process.command,
        Some(vec![
            "exec".to_owned(),
            "./server".to_owned(),
            "--port".to_owned(),
            "8080".to_owned()
        ])
    );
    assert_eq!(
        process.env,
        vec!["APP_ENV=dev".to_owned(), "LOG_LEVEL=debug".to_owned(),]
    );
    assert_eq!(process.cwd, PathBuf::from("/workspace"));
    assert_eq!(process.user.as_deref(), Some("1000:1000"));
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
        .to_process_spec()
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
fn compose_duration_parser_accepts_every_supported_unit() {
    let cases = [
        ("1ns", Duration::from_nanos(1)),
        ("1us", Duration::from_micros(1)),
        ("1µs", Duration::from_micros(1)),
        ("1μs", Duration::from_micros(1)),
        ("1ms", Duration::from_millis(1)),
        ("1s", Duration::from_secs(1)),
        ("1m", Duration::from_secs(60)),
        ("1h", Duration::from_secs(60 * 60)),
        ("1m30s", Duration::from_secs(90)),
    ];

    for (value, expected) in cases {
        assert_eq!(
            parse_compose_duration("services.db.stop_grace_period", value)
                .expect("supported duration unit should parse"),
            expected,
            "duration literal {value:?} should parse through the shared unit table"
        );
    }
}

#[test]
fn compose_duration_parser_rejects_unknown_units_without_panicking() {
    let error = parse_compose_duration("services.db.stop_grace_period", "1d")
        .expect_err("unknown duration unit should return a parse error");

    assert!(
        error
            .to_string()
            .contains("Supported units are ns, us, ms, s, m, h"),
        "unknown unit should be reported as validation error, got: {error}"
    );
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
fn compose_project_rejects_non_loopback_port_exposure_without_policy() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    image: busybox:latest
    ports:
      - "0.0.0.0:8080:80"
"#,
    );

    let error =
        ComposeProjectPlan::load(&compose).expect_err("non-loopback exposure should fail closed");
    assert!(
        error.to_string().contains("non-loopback host address"),
        "expected non-loopback policy error, got: {error}"
    );
    assert!(
        error
            .to_string()
            .contains("operator network exposure policy"),
        "expected operator-policy guidance, got: {error}"
    );
}

#[test]
fn compose_project_rejects_host_bind_mounts_without_policy() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    image: busybox:latest
    volumes:
      - ./data:/data
"#,
    );

    let error = ComposeProjectPlan::load(&compose).expect_err("bind mount should fail closed");
    assert!(
        error.to_string().contains("host bind mounts are denied"),
        "expected bind-mount policy error, got: {error}"
    );
    assert!(
        error.to_string().contains("tenant-owned storage"),
        "expected tenant-owned storage guidance, got: {error}"
    );
}

#[test]
fn compose_project_rejects_anonymous_volume_mounts_without_policy() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    image: busybox:latest
    volumes:
      - /cache
"#,
    );

    let error = ComposeProjectPlan::load(&compose).expect_err("anonymous mount should fail closed");
    assert!(
        error
            .to_string()
            .contains("anonymous volumes are not admitted"),
        "expected anonymous volume policy error, got: {error}"
    );
}

#[test]
fn compose_project_lowers_named_volumes_into_sandbox_mounts() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
name: Demo App
services:
  db:
    image: postgres:16
    volumes:
      - pgdata:/var/lib/postgresql/data
      - type: volume
        source: logs
        target: /var/log/postgres
        read_only: true
volumes:
  pgdata: {}
  logs: {}
"#,
    );

    let tenant_id = TenantId::new("tenant-a").expect("tenant id should be valid");
    let catalog = ComposeProjectPlan::load(&compose)
        .expect("compose file should resolve")
        .into_service_catalog()
        .expect("compose project should lower into a service catalog");
    let spec = sandbox_spec_from_backend(
        catalog
            .service_definition_for_tenant(&tenant_id, "db")
            .expect("db definition should exist")
            .backend,
    );
    let volume_policy = catalog.service_volume_policy_for_tenant(&tenant_id, "db");
    assert_eq!(image_reference_from_spec(&spec), "postgres:16");
    assert_eq!(
        volume_policy.named_volumes(),
        &["pgdata".to_owned(), "logs".to_owned()],
        "compose catalog should expose admitted service volumes independently from the launch spec"
    );

    assert_eq!(spec.mounts.len(), 2);
    assert_eq!(spec.mounts[0].tenant_volume_name(), Some("pgdata"));
    assert_eq!(
        spec.mounts[0].destination,
        PathBuf::from("/var/lib/postgresql/data")
    );
    assert!(!spec.mounts[0].read_only);
    assert_eq!(spec.mounts[1].tenant_volume_name(), Some("logs"));
    assert_eq!(
        spec.mounts[1].destination,
        PathBuf::from("/var/log/postgres")
    );
    assert!(spec.mounts[1].read_only);
}

#[test]
fn compose_project_rejects_undeclared_named_volume_mounts() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    image: busybox:latest
    volumes:
      - cache:/cache
"#,
    );

    let error = ComposeProjectPlan::load(&compose).expect_err("undeclared volume should fail");
    assert!(
        error
            .to_string()
            .contains("must be declared at top-level volumes"),
        "expected declared-volume policy error, got: {error}"
    );
}

#[test]
fn compose_project_rejects_unsupported_top_level_volume_options() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    image: busybox:latest
volumes:
  cache:
    driver: local
"#,
    );

    let error = ComposeProjectPlan::load(&compose).expect_err("volume driver should fail closed");
    assert!(
        error.to_string().contains("unsupported volume options"),
        "expected top-level volume option error, got: {error}"
    );
}

#[test]
fn production_compose_admission_rejects_tag_only_images() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    image: busybox:latest
"#,
    );

    let error = ComposeProjectPlan::load_selection_with_admission(
        &ResolvedComposeSelection::explicit(compose),
        ComposeAdmissionMode::Production,
    )
    .expect_err("tag-only image should fail production admission");
    assert!(
        error.to_string().contains("digest-pinned OCI image"),
        "expected digest-pinned image guidance, got: {error}"
    );
}

#[test]
fn production_compose_admission_accepts_digest_pinned_images() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let digest = "1111111111111111111111111111111111111111111111111111111111111111";
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        &format!(
            r#"
services:
  api:
    image: docker.io/library/busybox@sha256:{digest}
"#
        ),
    );

    let project = ComposeProjectPlan::load_selection_with_admission(
        &ResolvedComposeSelection::explicit(compose),
        ComposeAdmissionMode::Production,
    )
    .expect("digest-pinned image should pass production admission");
    let api = project.services.get("api").expect("api should load");
    assert_eq!(
        api.source,
        ComposeLaunchPlan::Image {
            image_reference: format!("docker.io/library/busybox@sha256:{digest}"),
        }
    );
}

#[test]
fn production_compose_admission_accepts_docker_hub_short_digest_references() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let digest = "1111111111111111111111111111111111111111111111111111111111111111";
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        &format!(
            r#"
services:
  api:
    image: busybox@sha256:{digest}
"#
        ),
    );

    let project = ComposeProjectPlan::load_selection_with_admission(
        &ResolvedComposeSelection::explicit(compose),
        ComposeAdmissionMode::Production,
    )
    .expect("Docker Hub short digest image should pass production admission");
    let api = project.services.get("api").expect("api should load");
    assert_eq!(
        api.source,
        ComposeLaunchPlan::Image {
            image_reference: format!("busybox@sha256:{digest}"),
        }
    );
}

#[test]
fn production_compose_admission_rejects_invalid_oci_references() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    image: ":justtag"
"#,
    );

    let error = ComposeProjectPlan::load_selection_with_admission(
        &ResolvedComposeSelection::explicit(compose),
        ComposeAdmissionMode::Production,
    )
    .expect_err("invalid OCI reference should fail production admission");
    assert!(
        error.to_string().contains("invalid OCI image reference"),
        "expected parser failure, got: {error}"
    );
}

#[test]
fn compose_project_lowers_x_nimbus_egress_policy_into_sandbox_spec() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    image: registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
    x-nimbus:
      egress:
        allow:
          - name: stripe-api
            protocol: https
            host: api.stripe.com
            port: 443
            methods:
              - POST
            path_prefixes:
              - /v1/
"#,
    );

    let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
    let catalog = ComposeProjectPlan::load(&compose)
        .expect("compose file should resolve")
        .into_service_catalog()
        .expect("compose project should lower into a service catalog");
    let spec = sandbox_spec_from_backend(
        catalog
            .service_definition_for_tenant(&tenant_id, "api")
            .expect("api definition should exist")
            .backend,
    );
    assert_eq!(
        image_reference_from_spec(&spec),
        "registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(spec.egress.rules().len(), 1);
    let rule = &spec.egress.rules()[0];
    assert_eq!(rule.name, "stripe-api");
    assert_eq!(rule.host, "api.stripe.com");
    assert_eq!(rule.methods, vec!["POST".to_string()]);
    assert_eq!(rule.path_prefixes, vec!["/v1/".to_string()]);
}

#[test]
fn compose_project_rejects_invalid_x_nimbus_egress_policy() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    image: registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
    x-nimbus:
      egress:
        allow:
          - name: all
            protocol: https
            host: "*.example.com"
            port: 443
"#,
    );

    let error = ComposeProjectPlan::load(&compose)
        .expect_err("wildcard egress policy should fail during compose admission");
    assert!(
        error.to_string().contains("x-nimbus.egress") && error.to_string().contains("wildcards"),
        "error should name the invalid egress shape: {error}"
    );
}

#[test]
fn compose_project_rejects_runtime_only_websocket_egress_protocol() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    image: registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
    x-nimbus:
      egress:
        allow:
          - name: events-websocket
            protocol: wss
            host: events.example.com
            port: 443
"#,
    );

    let error = ComposeProjectPlan::load(&compose)
        .expect_err("Compose must reject a protocol the supervisor proxy cannot observe");
    assert!(
        error.to_string().contains("observable runtime gateway"),
        "error should explain where ws/wss policy is enforceable: {error}"
    );
}

#[test]
fn compose_project_rejects_unknown_x_nimbus_egress_fields() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    image: registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
    x-nimbus:
      egress:
        allow:
          - name: stripe-api
            protocol: https
            host: api.stripe.com
            port: 443
            method:
              - POST
"#,
    );

    let error = ComposeProjectPlan::load(&compose)
        .expect_err("unknown egress field should fail closed during compose admission");
    assert!(
        error.to_string().contains("unknown field") && error.to_string().contains("method"),
        "error should name the unknown egress field: {error}"
    );
}

#[test]
fn production_compose_admission_rejects_local_builds_without_provenance_policy() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
services:
  api:
    build:
      context: .
"#,
    );

    let error = ComposeProjectPlan::load_selection_with_admission(
        &ResolvedComposeSelection::explicit(compose),
        ComposeAdmissionMode::Production,
    )
    .expect_err("production build should fail without operator provenance policy");
    assert!(
        error
            .to_string()
            .contains("image provenance/signature policy"),
        "expected provenance/signature guidance, got: {error}"
    );
}

#[test]
fn production_compose_admission_rejects_raw_compose_secrets() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let top_level = write_compose_fixture(
        &tempdir,
        "compose-top.yaml",
        r#"
services:
  api:
    image: docker.io/library/busybox@sha256:1111111111111111111111111111111111111111111111111111111111111111
secrets:
  db_password:
    file: ./db_password.txt
"#,
    );
    let service_level = write_compose_fixture(
        &tempdir,
        "compose-service.yaml",
        r#"
services:
  api:
    image: docker.io/library/busybox@sha256:1111111111111111111111111111111111111111111111111111111111111111
    secrets:
      - db_password
"#,
    );

    let top_error = ComposeProjectPlan::load_selection_with_admission(
        &ResolvedComposeSelection::explicit(top_level),
        ComposeAdmissionMode::Production,
    )
    .expect_err("top-level raw secret should fail production admission");
    assert!(
        top_error
            .to_string()
            .contains("secret handles/capabilities"),
        "expected secret handles guidance, got: {top_error}"
    );

    let service_error = ComposeProjectPlan::load_selection_with_admission(
        &ResolvedComposeSelection::explicit(service_level),
        ComposeAdmissionMode::Production,
    )
    .expect_err("service raw secret should fail production admission");
    assert!(
        service_error
            .to_string()
            .contains("secret handles/capabilities"),
        "expected secret handles guidance, got: {service_error}"
    );
}

#[test]
fn compose_project_lowers_into_service_definition_catalog() {
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

    let db = sandbox_spec_from_backend(
        catalog
            .service_definition_for_tenant(&tenant_id, "db")
            .expect("db definition should exist")
            .backend,
    );
    assert_eq!(image_reference_from_spec(&db), "postgres:16");
    assert_eq!(db.tenant_id, tenant_id);
    assert_eq!(db.service_name(), Some("db"));
    assert_eq!(db.resources.cpu_count, Some(1));
    assert_eq!(db.resources.memory_limit_bytes, Some(256 * 1024 * 1024));
    assert_eq!(
        db.lifecycle.restart_policy,
        SandboxRestartPolicy::OnFailure { max_restarts: 3 }
    );
    assert_eq!(db.lifecycle.stop_timeout, Some(Duration::from_secs(30)));
    assert_eq!(db.port_bindings.len(), 1);
    assert_eq!(db.port_bindings[0].host_port, 5432);
    assert_eq!(db.port_bindings[0].guest_port, 5432);

    let api = sandbox_spec_from_backend(
        catalog
            .service_definition_for_tenant(&tenant_id, "api")
            .expect("api definition should exist")
            .backend,
    );
    let api_build = build_source_from_spec(&api);
    assert_eq!(api_build.image_name, "nimbus-demo-app-api");
    assert_eq!(
        api_build.dockerfile_path,
        tempdir.path().join("Dockerfile.api")
    );
    assert_eq!(api_build.context_path, tempdir.path());
    assert_eq!(
        api.process.entrypoint,
        Some(vec!["/bin/sh".to_owned(), "-lc".to_owned()])
    );
    assert_eq!(api.process.command, Some(vec!["./server".to_owned()]));
    assert_eq!(api.process.cwd, PathBuf::from("/workspace"));
    assert_eq!(api.process.user.as_deref(), Some("1000:1000"));

    let other_tenant = TenantId::new("other").expect("tenant id should be valid");
    let other_db = sandbox_spec_from_backend(
        catalog
            .service_definition_for_tenant(&other_tenant, "db")
            .expect("catalog should lower the same service plan for another tenant")
            .backend,
    );
    assert_eq!(image_reference_from_spec(&other_db), "postgres:16");
    assert_eq!(other_db.tenant_id, other_tenant);
    assert_eq!(other_db.service_name(), Some("db"));
}

#[test]
fn compose_service_catalog_stamps_cached_backends_for_each_tenant() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
name: Demo App
services:
  db:
    image: postgres:16
  api:
    image: ghcr.io/example/api:latest
"#,
    );
    let catalog = ComposeProjectPlan::load(&compose)
        .expect("compose file should resolve")
        .into_service_catalog()
        .expect("compose project should lower into a service catalog");

    let tenant_a = TenantId::new("tenant-a").expect("tenant id should be valid");
    let tenant_b = TenantId::new("tenant-b").expect("tenant id should be valid");
    let tenant_a_definitions = catalog.service_definitions_for_tenant(&tenant_a);
    let tenant_b_definitions = catalog.service_definitions_for_tenant(&tenant_b);

    assert_eq!(
        tenant_a_definitions.keys().cloned().collect::<Vec<_>>(),
        vec!["api".to_owned(), "db".to_owned()]
    );
    assert_eq!(
        tenant_b_definitions.keys().cloned().collect::<Vec<_>>(),
        vec!["api".to_owned(), "db".to_owned()]
    );

    let tenant_a_db = sandbox_spec_from_backend(
        tenant_a_definitions
            .get("db")
            .expect("db definition should exist")
            .backend
            .clone(),
    );
    let tenant_b_db = sandbox_spec_from_backend(
        tenant_b_definitions
            .get("db")
            .expect("db definition should exist")
            .backend
            .clone(),
    );

    assert_eq!(tenant_a_db.tenant_id, tenant_a);
    assert_eq!(tenant_b_db.tenant_id, tenant_b);
    assert_eq!(tenant_a_db.service_name(), Some("db"));
    assert_eq!(tenant_b_db.service_name(), Some("db"));
    assert_eq!(image_reference_from_spec(&tenant_a_db), "postgres:16");
    assert_eq!(image_reference_from_spec(&tenant_b_db), "postgres:16");
}

#[test]
fn compose_catalog_reload_preserves_complete_definition_and_executable_digest() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let compose = write_compose_fixture(
        &tempdir,
        "compose.yaml",
        r#"
name: Stable App
services:
  api:
    image: ghcr.io/example/api@sha256:8b7c8f245b3a78327cf14a44aeeea7e69f2cccf13d1f338f4132c3ee445758b8
    command: ["./server", "--port", "8080"]
    labels:
      nimbus.test.role: api
"#,
    );
    let tenant_id = TenantId::new("tenant-stable").expect("tenant id should be valid");

    let load = || {
        ComposeProjectPlan::load(&compose)
            .expect("compose file should resolve")
            .into_service_catalog()
            .expect("compose project should lower into a service catalog")
            .service_definition_for_tenant(&tenant_id, "api")
            .expect("api definition should exist")
    };
    let first = load();
    let reloaded = load();

    assert_eq!(first.generation, 1);
    assert_eq!(first, reloaded);
    assert!(first.resource_version.starts_with("sha256:"));
    assert_eq!(
        first.labels.get("nimbus.test.role").map(String::as_str),
        Some("api")
    );

    let first_spec = first
        .backend
        .sandbox_spec()
        .expect("api definition should remain sandbox-backed");
    let reloaded_spec = reloaded
        .backend
        .sandbox_spec()
        .expect("reloaded api definition should remain sandbox-backed");
    let first_executable = nimbus_compute::workload_executable::encode_sandbox_spec(first_spec)
        .expect("first sandbox spec should encode canonically");
    let reloaded_executable =
        nimbus_compute::workload_executable::encode_sandbox_spec(reloaded_spec)
            .expect("reloaded sandbox spec should encode canonically");
    assert_eq!(
        first_executable.content_digest(),
        reloaded_executable.content_digest()
    );
    assert_eq!(
        first_executable.canonical_content(),
        reloaded_executable.canonical_content()
    );
}
