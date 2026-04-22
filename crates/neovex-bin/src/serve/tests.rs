use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use clap::{Parser, error::ErrorKind};

use super::config::{
    CliTenantProvider, PersistenceEnv, PersistenceFileConfig, load_runtime_config_file,
    service_persistence_config_from_sources,
};
use super::*;
use crate::{Cli, Command};

#[cfg(target_os = "linux")]
use std::env;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::time::Duration;
#[cfg(target_os = "linux")]
use std::time::Instant;

#[cfg(target_os = "linux")]
use neovex::{
    ConvexRegistry, RuntimeBundle, SandboxCatalog,
    build_router_with_convex_and_sandbox_service_manager,
};
#[cfg(target_os = "linux")]
use neovex_sandbox::backends::krun::{KrunLaunchMode, KrunSandboxBackend};
#[cfg(target_os = "linux")]
use neovex_testing::{
    HttpApiFixture, ServerFixture, ServiceFixture, run_to_completion_snapshot_runtime_test_limits,
    wait_for_condition,
};
#[cfg(target_os = "linux")]
use serde_json::json;
#[cfg(target_os = "linux")]
use tempfile::tempdir;

static TEST_CONFIG_COUNTER: AtomicU64 = AtomicU64::new(0);

fn write_test_config(contents: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "neovex-bin-config-{}-{}.json",
        std::process::id(),
        TEST_CONFIG_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, contents).expect("test config file should write");
    path
}

fn parse_serve<I, T>(args: I) -> ServeCommand
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    let Command::Serve(command) = cli.command else {
        panic!("serve subcommand should parse");
    };
    *command
}

#[test]
fn cli_defaults_to_embedded_sqlite() {
    let cli = parse_serve(["neovex", "serve"]);
    let config = service_persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("default sqlite config should build");
    assert_eq!(
        config,
        neovex::ServicePersistenceConfig::embedded("./data", neovex::EmbeddedProviderKind::Sqlite)
    );
}

#[test]
fn cli_requires_explicit_serve_subcommand_for_server_flags() {
    assert!(Cli::try_parse_from(["neovex"]).is_err());
    assert!(Cli::try_parse_from(["neovex", "--compose-file", "./compose.dev.yaml"]).is_err());
}

#[test]
fn cli_supports_top_level_version_flag() {
    let error = Cli::try_parse_from(["neovex", "--version"])
        .expect_err("top-level version flag should short-circuit with display output");
    assert_eq!(error.kind(), ErrorKind::DisplayVersion);
    assert_eq!(
        error.to_string(),
        format!("neovex {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn cli_help_describes_machine_and_service_surface() {
    let error = Cli::try_parse_from(["neovex", "--help"]).expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("Reactive document database with machine and service orchestration"));
    assert!(rendered.contains("Usage:"));
    assert!(rendered.contains("Available Commands:"));
    assert!(rendered.contains("Examples:"));
    assert!(rendered.contains("neovex serve"));
    assert!(rendered.contains("neovex machine start"));
    assert!(rendered.contains("neovex service up"));
    assert!(rendered.contains("serve"));
    assert!(rendered.contains("machine"));
    assert!(rendered.contains("service"));
}

#[test]
fn cli_parses_serve_command_with_optional_compose_file() {
    let cli = parse_serve(["neovex", "serve", "--compose-file", "./compose.dev.yaml"]);
    assert_eq!(cli.compose_file, Some(PathBuf::from("./compose.dev.yaml")));
}

#[test]
fn cli_builds_postgres_typed_config_with_overrides() {
    let cli = parse_serve([
        "neovex",
        "serve",
        "--tenant-provider",
        "postgres",
        "--control-data-dir",
        "./control",
        "--data-dir",
        "./ignored-for-postgres",
        "--postgres-url",
        "host=/tmp user=jack dbname=postgres",
        "--postgres-metadata-schema",
        "provider_meta",
        "--postgres-tenant-schema-prefix",
        "tenant_pg_",
        "--postgres-min-connections",
        "2",
        "--postgres-max-connections",
        "8",
    ]);
    let config = service_persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("postgres config should build");
    assert_eq!(
        config.control_plane,
        neovex::ControlPlaneConfig::embedded_redb("./control")
    );
    assert_eq!(
        config.tenant_provider.dialect,
        neovex::PersistenceDialect::Postgres
    );
    assert_eq!(
        config.tenant_provider.topology,
        neovex::PersistenceTopology::ExternalPrimary
    );
    assert_eq!(
        config.tenant_provider.credentials,
        neovex::ProviderCredentials::ConnectionString(
            "host=/tmp user=jack dbname=postgres".to_string()
        )
    );
    assert_eq!(config.tenant_provider.pool.min_connections, Some(2));
    assert_eq!(config.tenant_provider.pool.max_connections, Some(8));
    assert_eq!(
        config.tenant_provider.routing,
        neovex::TenantRoutingConfig::SchemaPerTenant {
            metadata_schema: "provider_meta".to_string(),
            tenant_schema_prefix: "tenant_pg_".to_string(),
        }
    );
}

#[test]
fn env_builds_postgres_typed_config_with_generic_resource_name() {
    let cli = parse_serve(["neovex", "serve"]);
    let env = PersistenceEnv {
        tenant_provider: Some(CliTenantProvider::Postgres),
        control_data_dir: Some(PathBuf::from("./control-from-env")),
        postgres_url: Some("host=/tmp user=jack dbname=postgres".to_string()),
        postgres_min_connections: Some(3),
        postgres_max_connections: Some(9),
        ..PersistenceEnv::default()
    };

    let config =
        service_persistence_config_from_sources(&cli, &PersistenceFileConfig::default(), &env)
            .expect("env-backed postgres config should build");

    assert_eq!(
        config.control_plane,
        neovex::ControlPlaneConfig::embedded_redb("./control-from-env")
    );
    assert_eq!(
        config.tenant_provider.credentials,
        neovex::ProviderCredentials::ConnectionString(
            "host=/tmp user=jack dbname=postgres".to_string()
        )
    );
    assert_eq!(config.tenant_provider.pool.min_connections, Some(3));
    assert_eq!(config.tenant_provider.pool.max_connections, Some(9));
}

#[test]
fn cli_builds_libsql_replica_typed_config_with_overrides() {
    let cli = parse_serve([
        "neovex",
        "serve",
        "--tenant-provider",
        "libsql-replica",
        "--control-data-dir",
        "./control",
        "--libsql-url",
        "libsql://127.0.0.1:8080",
        "--libsql-auth-token",
        "replica-secret",
        "--libsql-admin-url",
        "http://127.0.0.1:8081",
        "--libsql-admin-auth-header",
        "Bearer replica-admin",
        "--libsql-metadata-namespace",
        "provider_meta",
        "--libsql-tenant-namespace-prefix",
        "tenant_sqlite_",
        "--libsql-replica-cache-dir",
        "./replica-cache",
    ]);
    let config = service_persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("libsql replica config should build");
    assert_eq!(
        config.control_plane,
        neovex::ControlPlaneConfig::embedded_redb("./control")
    );
    assert_eq!(
        config.tenant_provider.dialect,
        neovex::PersistenceDialect::Sqlite
    );
    assert_eq!(
        config.tenant_provider.topology,
        neovex::PersistenceTopology::ExternalPrimaryWithReplicas
    );
    assert_eq!(
        config.tenant_provider.credentials,
        neovex::ProviderCredentials::LibsqlReplica {
            primary_url: "libsql://127.0.0.1:8080".to_string(),
            auth_token: Some("replica-secret".to_string()),
            admin_api_url: "http://127.0.0.1:8081".to_string(),
            admin_auth_header: Some("Bearer replica-admin".to_string()),
        }
    );
    assert_eq!(
        config.tenant_provider.routing,
        neovex::TenantRoutingConfig::NamespacePerTenant {
            metadata_namespace: "provider_meta".to_string(),
            tenant_namespace_prefix: "tenant_sqlite_".to_string(),
            replica_cache_dir: PathBuf::from("./replica-cache"),
        }
    );
}

#[test]
fn env_builds_libsql_replica_typed_config_with_generic_resource_name() {
    let cli = parse_serve(["neovex", "serve"]);
    let env = PersistenceEnv {
        tenant_provider: Some(CliTenantProvider::LibsqlReplica),
        control_data_dir: Some(PathBuf::from("./control-from-env")),
        libsql_url: Some("libsql://127.0.0.1:8080".to_string()),
        libsql_admin_url: Some("http://127.0.0.1:8081".to_string()),
        libsql_replica_cache_dir: Some(PathBuf::from("./replica-cache-from-env")),
        ..PersistenceEnv::default()
    };

    let config =
        service_persistence_config_from_sources(&cli, &PersistenceFileConfig::default(), &env)
            .expect("env-backed libsql replica config should build");

    assert_eq!(
        config.control_plane,
        neovex::ControlPlaneConfig::embedded_redb("./control-from-env")
    );
    assert_eq!(
        config.tenant_provider.credentials,
        neovex::ProviderCredentials::LibsqlReplica {
            primary_url: "libsql://127.0.0.1:8080".to_string(),
            auth_token: None,
            admin_api_url: "http://127.0.0.1:8081".to_string(),
            admin_auth_header: None,
        }
    );
    assert_eq!(
        config.tenant_provider.routing,
        neovex::TenantRoutingConfig::NamespacePerTenant {
            metadata_namespace: "neovex_provider".to_string(),
            tenant_namespace_prefix: "tenant_".to_string(),
            replica_cache_dir: PathBuf::from("./replica-cache-from-env"),
        }
    );
}

#[test]
fn cli_builds_mysql_typed_config_with_overrides() {
    let cli = parse_serve([
        "neovex",
        "serve",
        "--tenant-provider",
        "mysql",
        "--control-data-dir",
        "./control",
        "--data-dir",
        "./ignored-for-mysql",
        "--mysql-url",
        "mysql://root:password@127.0.0.1:3306/neovex",
        "--mysql-metadata-database",
        "provider_meta",
        "--mysql-tenant-database-prefix",
        "tenant_mysql_",
        "--mysql-min-connections",
        "2",
        "--mysql-max-connections",
        "8",
    ]);
    let config = service_persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("mysql config should build");
    assert_eq!(
        config.control_plane,
        neovex::ControlPlaneConfig::embedded_redb("./control")
    );
    assert_eq!(
        config.tenant_provider.dialect,
        neovex::PersistenceDialect::MySql
    );
    assert_eq!(
        config.tenant_provider.topology,
        neovex::PersistenceTopology::ExternalPrimary
    );
    assert_eq!(
        config.tenant_provider.credentials,
        neovex::ProviderCredentials::ConnectionString(
            "mysql://root:password@127.0.0.1:3306/neovex".to_string()
        )
    );
    assert_eq!(config.tenant_provider.pool.min_connections, Some(2));
    assert_eq!(config.tenant_provider.pool.max_connections, Some(8));
    assert_eq!(
        config.tenant_provider.routing,
        neovex::TenantRoutingConfig::DatabasePerTenant {
            metadata_database: "provider_meta".to_string(),
            tenant_database_prefix: "tenant_mysql_".to_string(),
        }
    );
}

#[test]
fn env_builds_mysql_typed_config_with_generic_resource_name() {
    let cli = parse_serve(["neovex", "serve"]);
    let env = PersistenceEnv {
        tenant_provider: Some(CliTenantProvider::Mysql),
        control_data_dir: Some(PathBuf::from("./control-from-env")),
        mysql_url: Some("mysql://root:password@127.0.0.1:3306/neovex".to_string()),
        mysql_min_connections: Some(3),
        mysql_max_connections: Some(9),
        ..PersistenceEnv::default()
    };

    let config =
        service_persistence_config_from_sources(&cli, &PersistenceFileConfig::default(), &env)
            .expect("env-backed mysql config should build");

    assert_eq!(
        config.control_plane,
        neovex::ControlPlaneConfig::embedded_redb("./control-from-env")
    );
    assert_eq!(
        config.tenant_provider.credentials,
        neovex::ProviderCredentials::ConnectionString(
            "mysql://root:password@127.0.0.1:3306/neovex".to_string()
        )
    );
    assert_eq!(config.tenant_provider.pool.min_connections, Some(3));
    assert_eq!(config.tenant_provider.pool.max_connections, Some(9));
}

#[test]
fn config_file_builds_split_embedded_sqlite_config() {
    let path = write_test_config(
        r#"{
  "persistence": {
    "tenant_provider": "sqlite",
    "data_dir": "./tenant-data",
    "control_data_dir": "./control-data"
  }
}"#,
    );
    let cli = parse_serve(["neovex", "serve", "--config", path.to_str().unwrap()]);
    let file_config =
        load_runtime_config_file(Some(path.as_path())).expect("config file should load");

    let config = service_persistence_config_from_sources(
        &cli,
        &file_config.persistence,
        &PersistenceEnv::default(),
    )
    .expect("config-backed sqlite config should build");

    assert_eq!(
        config.tenant_provider,
        neovex::TenantProviderConfig::embedded(
            "./tenant-data",
            neovex::EmbeddedProviderKind::Sqlite
        )
    );
    assert_eq!(
        config.control_plane,
        neovex::ControlPlaneConfig::embedded_redb("./control-data")
    );
}

#[test]
fn cli_overrides_config_file_postgres_pool_settings() {
    let path = write_test_config(
        r#"{
  "persistence": {
    "tenant_provider": "postgres",
    "control_data_dir": "./control",
    "postgres_url": "host=/tmp user=jack dbname=postgres",
    "postgres_min_connections": 2,
    "postgres_max_connections": 4
  }
}"#,
    );
    let cli = parse_serve([
        "neovex",
        "serve",
        "--config",
        path.to_str().unwrap(),
        "--postgres-max-connections",
        "8",
    ]);
    let file_config =
        load_runtime_config_file(Some(path.as_path())).expect("config file should load");

    let config = service_persistence_config_from_sources(
        &cli,
        &file_config.persistence,
        &PersistenceEnv::default(),
    )
    .expect("config + cli postgres config should build");

    assert_eq!(config.tenant_provider.pool.min_connections, Some(2));
    assert_eq!(config.tenant_provider.pool.max_connections, Some(8));
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires Linux KVM host with krun toolchain"]
async fn convex_runtime_query_starts_real_krun_service_from_compose_file_and_tears_it_down() {
    let tempdir = tempdir().expect("compose + convex tempdir should build");
    let tenant_id = neovex::TenantId::new("demo").expect("tenant id should be valid");
    let host_port = env_u16("NEOVEX_KRUN_SMOKE_M5_HOST_PORT").unwrap_or(18091);
    let guest_port = env_u16("NEOVEX_KRUN_SMOKE_M5_GUEST_PORT").unwrap_or(8091);
    let compose_path = write_compose_smoke_fixture(tempdir.path(), host_port, guest_port);
    let registry = write_convex_service_query_fixture(tempdir.path());

    let base_dir = env_path("NEOVEX_KRUN_SMOKE_WORKDIR");
    let control_data_dir = base_dir.join("m5-compose-control");
    let context = crate::service::load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");
    if let Some(metadata_path) = env::var_os("NEOVEX_KRUN_SMOKE_M5_METADATA_FILE") {
        let metadata_path = PathBuf::from(metadata_path);
        if let Some(parent) = metadata_path.parent() {
            fs::create_dir_all(parent).expect("metadata parent should build");
        }
        fs::write(
            &metadata_path,
            serde_json::to_vec_pretty(&json!({
                "project_root": context.control_plane.project_root,
                "project_key": context.control_plane.project_key,
            }))
            .expect("metadata json should serialize"),
        )
        .expect("metadata file should write");
    }
    println!(
        "M5_PROJECT_ROOT={}",
        context.control_plane.project_root.display()
    );
    println!("M5_PROJECT_KEY={}", context.control_plane.project_key);
    let mut config = context.control_plane.krun_backend_config();
    config.launch_mode = KrunLaunchMode::Execute;
    if let Some(runtime_path) = env::var_os("NEOVEX_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NEOVEX_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NEOVEX_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    let sandbox_service_manager = Arc::new(
        crate::service::load_sandbox_service_manager(
            &compose_path,
            Arc::new(KrunSandboxBackend::new(config)),
        )
        .expect("compose-backed sandbox service manager should load")
        .with_activation_poll_interval(Duration::from_millis(50))
        .with_activation_timeout(Duration::from_secs(30)),
    );
    let fixture = ServiceFixture::new(|path| neovex::Service::new(path));
    let server = ServerFixture::start(build_router_with_convex_and_sandbox_service_manager(
        fixture.service(),
        registry,
        sandbox_service_manager.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        reqwest::StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "services:activate", json!({}))
        .await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let port = response
        .json::<serde_json::Value>()
        .await
        .expect("activation response should parse")
        .as_u64()
        .expect("port should be numeric");
    assert_eq!(port, u64::from(host_port));

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15)).await;
    assert!(
        http_response.starts_with("HTTP/1.") || http_response.contains("404"),
        "expected HTTP response from compose-backed krun service, got: {http_response}"
    );
    assert!(
        sandbox_service_manager
            .sandboxes_for_tenant(&tenant_id)
            .contains_key("db"),
        "compose-backed manager should expose the declared db binding"
    );

    let delete = api.delete_tenant("demo").await;
    assert_eq!(delete.status(), reqwest::StatusCode::NO_CONTENT);
    wait_for_condition(
        "compose-backed krun service should disappear after tenant deletion",
        Duration::from_secs(10),
        Duration::from_millis(100),
        || async {
            reqwest::get(format!("http://127.0.0.1:{host_port}/"))
                .await
                .is_err()
                && sandbox_service_manager
                    .sandboxes_for_tenant(&tenant_id)
                    .is_empty()
        },
    )
    .await;
}

#[cfg(target_os = "linux")]
fn write_compose_smoke_fixture(root: &Path, host_port: u16, guest_port: u16) -> PathBuf {
    let compose_path = root.join("compose.yaml");
    fs::write(
        &compose_path,
        format!(
            r#"
name: Smoke App
services:
  db:
    image: busybox:latest
    ports:
      - "{host_port}:{guest_port}"
    command:
      - /bin/busybox
      - httpd
      - -f
      - -p
      - "{guest_port}"
    stop_grace_period: 5s
"#
        ),
    )
    .expect("compose smoke fixture should write");
    compose_path
}

#[cfg(target_os = "linux")]
fn write_convex_service_query_fixture(app_dir: &Path) -> ConvexRegistry {
    let convex_dir = app_dir.join(".neovex").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [{
                "name": "services:activate",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx) => ctx.services.db.port"
            }]
        }))
        .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex routes json should serialize"),
    )
    .expect("convex routes manifest should write");

    let bundle_path = convex_dir.join("bundle.mjs");
    fs::write(
        &bundle_path,
        r#"
const definitions = new Map([
  ["services:activate", {
    name: "services:activate",
    kind: "query",
    runtime_handler: "async (ctx) => ctx.services.db.port",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__neovexInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__neovexCreateContext({
          request,
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#,
    )
    .expect("convex runtime bundle should write");
    let bundle_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    fs::write(
        bundle_path.with_extension("sha256"),
        format!("{bundle_sha256}\n"),
    )
    .expect("convex runtime bundle hash should write");

    ConvexRegistry::from_app_dir(app_dir)
        .expect("convex registry should load")
        .with_runtime_limits(run_to_completion_snapshot_runtime_test_limits())
}

#[cfg(target_os = "linux")]
fn env_path(name: &str) -> PathBuf {
    PathBuf::from(env::var_os(name).unwrap_or_else(|| panic!("missing env var {name}")))
}

#[cfg(target_os = "linux")]
fn env_u16(name: &str) -> Option<u16> {
    env::var(name).ok().map(|value| {
        value
            .parse::<u16>()
            .unwrap_or_else(|error| panic!("invalid {name} value {value:?}: {error}"))
    })
}

#[cfg(target_os = "linux")]
async fn wait_for_http_response(host_port: u16, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(response) = reqwest::get(format!("http://127.0.0.1:{host_port}/")).await {
            let status = response.status();
            if let Ok(body) = response.text().await {
                return format!("HTTP/1.1 {status}\n{body}");
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for HTTP response on port {host_port}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

// -------------------------------------------------------------------------
// Encryption config tests
// -------------------------------------------------------------------------

#[test]
fn cli_defaults_to_encryption_disabled() {
    let cli = parse_serve(["neovex", "serve"]);
    let config = service_persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("default config should build");
    assert!(!config.local_encryption.is_enabled());
}

#[test]
fn cli_builds_master_key_file_encryption_config() {
    let cli = parse_serve([
        "neovex",
        "serve",
        "--encryption-key-provider",
        "master-key-file",
        "--encryption-master-key-file",
        "/secure/neovex.key",
    ]);
    let config = service_persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("master-key-file encryption config should build");
    assert!(config.local_encryption.is_enabled());
    let descriptor = config.local_encryption.descriptor();
    assert!(matches!(
        descriptor,
        neovex::EncryptionConfigDescriptor::Enabled(
            neovex::KeyProviderDescriptor::MasterKeyFile { .. }
        )
    ));
}

#[test]
fn cli_builds_key_dir_encryption_config() {
    let cli = parse_serve([
        "neovex",
        "serve",
        "--encryption-key-provider",
        "key-dir",
        "--encryption-key-dir",
        "/secure/keys",
    ]);
    let config = service_persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("key-dir encryption config should build");
    assert!(config.local_encryption.is_enabled());
    let descriptor = config.local_encryption.descriptor();
    assert!(matches!(
        descriptor,
        neovex::EncryptionConfigDescriptor::Enabled(
            neovex::KeyProviderDescriptor::KeyDirectory { .. }
        )
    ));
}

#[test]
fn cli_builds_aws_kms_encryption_config() {
    let cli = parse_serve([
        "neovex",
        "serve",
        "--encryption-key-provider",
        "aws-kms",
        "--encryption-aws-kms-key-id",
        "arn:aws:kms:us-east-1:123456789:key/example-key-id",
        "--encryption-aws-region",
        "us-east-1",
    ]);
    let config = service_persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("aws-kms encryption config should build");
    assert!(config.local_encryption.is_enabled());
    let descriptor = config.local_encryption.descriptor();
    assert!(matches!(
        descriptor,
        neovex::EncryptionConfigDescriptor::Enabled(neovex::KeyProviderDescriptor::AwsKms { .. })
    ));
}

#[test]
fn cli_rejects_orphaned_encryption_options() {
    let cli = parse_serve([
        "neovex",
        "serve",
        "--encryption-master-key-file",
        "/secure/neovex.key",
    ]);
    let result = service_persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    );
    assert!(result.is_err());
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("require"),
        "error should mention requirement: {error}"
    );
}

#[test]
fn cli_rejects_mismatched_encryption_provider_options() {
    let cli = parse_serve([
        "neovex",
        "serve",
        "--encryption-key-provider",
        "master-key-file",
        "--encryption-master-key-file",
        "/secure/neovex.key",
        "--encryption-aws-kms-key-id",
        "arn:aws:kms:us-east-1:123456789:key/example-key-id",
    ]);
    let result = service_persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    );
    assert!(result.is_err());
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("aws-kms"),
        "error should mention aws-kms: {error}"
    );
}

#[test]
fn cli_requires_master_key_file_path() {
    let cli = parse_serve([
        "neovex",
        "serve",
        "--encryption-key-provider",
        "master-key-file",
    ]);
    let result = service_persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    );
    assert!(result.is_err());
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("encryption-master-key-file"),
        "error should mention missing file: {error}"
    );
}

#[test]
fn cli_requires_aws_kms_key_id() {
    let cli = parse_serve(["neovex", "serve", "--encryption-key-provider", "aws-kms"]);
    let result = service_persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    );
    assert!(result.is_err());
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("aws-kms-key-id"),
        "error should mention missing key id: {error}"
    );
}
