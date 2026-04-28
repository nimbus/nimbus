use std::fs;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use clap::{Parser, error::ErrorKind};
use neovex::RuntimeLimits;
use serde_json::json;

use super::config::{
    CliTenantProvider, PersistenceEnv, PersistenceFileConfig, load_runtime_config_file,
    persistence_config_from_sources,
};
use super::*;
use crate::codegen::CodegenCommand;
use crate::test_support::with_current_dir;
use crate::{Cli, Command};

use std::env;
#[cfg(target_os = "linux")]
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
#[cfg(target_os = "linux")]
use std::time::Duration;
#[cfg(target_os = "linux")]
use std::time::Instant;

#[cfg(target_os = "linux")]
use neovex::{ConvexRegistry, RuntimeBundle, SandboxCatalog};
#[cfg(target_os = "linux")]
use neovex_sandbox::backends::krun::{KrunLaunchMode, KrunSandboxBackend};
#[cfg(target_os = "linux")]
use neovex_server::build_router_with_convex_and_sandbox_service_manager;
#[cfg(target_os = "linux")]
use neovex_testing::{
    HttpApiFixture, ServerFixture, ServiceFixture, run_to_completion_snapshot_runtime_test_limits,
    wait_for_condition,
};
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

fn parse_start<I, T>(args: I) -> StartCommand
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    let Command::Start(command) = cli.command else {
        panic!("start subcommand should parse");
    };
    *command
}

fn parse_codegen<I, T>(args: I) -> CodegenCommand
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    let Command::Codegen(command) = cli.command else {
        panic!("codegen subcommand should parse");
    };
    command
}

#[test]
fn cli_defaults_to_embedded_sqlite() {
    let cli = parse_start(["neovex", "start"]);
    let config = persistence_config_from_sources(
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
fn start_command_default_has_no_auto_tenant() {
    let command = StartCommand::default();
    assert!(
        command.auto_tenant.is_none(),
        "start should not auto-create a tenant by default"
    );
}

#[test]
fn cli_requires_explicit_start_subcommand_for_server_flags() {
    assert!(Cli::try_parse_from(["neovex"]).is_err());
    assert!(Cli::try_parse_from(["neovex", "--compose-file", "./compose.dev.yaml"]).is_err());
}

#[test]
fn legacy_serve_namespace_is_not_supported() {
    let error = Cli::try_parse_from(["neovex", "serve", "--help"])
        .expect_err("legacy serve namespace should not parse");
    assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
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
fn cli_help_describes_codegen_machine_and_compose_surface() {
    let error = Cli::try_parse_from(["neovex", "--help"]).expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains(
        "Convex-compatible reactive backend with local development and Compose-backed services"
    ));
    assert!(rendered.contains("Usage:"));
    assert!(rendered.contains("Available Commands:"));
    assert!(rendered.contains("Examples:"));
    assert!(rendered.contains("neovex start"));
    assert!(rendered.contains("neovex dev"));
    assert!(rendered.contains("neovex codegen --app ./demos/convex/html"));
    assert!(rendered.contains("neovex token rotate"));
    assert!(rendered.contains("neovex machine start"));
    assert!(rendered.contains("neovex compose up"));
    assert!(rendered.contains("start"));
    assert!(rendered.contains("dev"));
    assert!(rendered.contains("codegen"));
    assert!(rendered.contains("token"));
    assert!(rendered.contains("machine     Manage local developer machines"));
    assert!(rendered.contains("compose"));
}

#[test]
fn cli_parses_start_command_with_optional_compose_file() {
    let cli = parse_start(["neovex", "start", "--compose-file", "./compose.dev.yaml"]);
    assert_eq!(cli.compose_file, vec![PathBuf::from("./compose.dev.yaml")]);
}

#[test]
fn cli_parses_start_command_with_multiple_compose_files_in_order() {
    let cli = parse_start([
        "neovex",
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
    let cli = parse_start(["neovex", "start", "--app-dir", "./demos/convex/html"]);
    assert_eq!(cli.app_dir, Some(PathBuf::from("./demos/convex/html")));
}

#[test]
fn cli_defaults_start_host_to_loopback_and_accepts_explicit_host() {
    let default_cli = parse_start(["neovex", "start"]);
    assert_eq!(default_cli.host, "127.0.0.1");

    let explicit_cli = parse_start(["neovex", "start", "--host", "0.0.0.0"]);
    assert_eq!(explicit_cli.host, "0.0.0.0");
}

#[test]
fn cli_parses_start_command_with_skip_codegen() {
    let cli = parse_start([
        "neovex",
        "start",
        "--app-dir",
        "./demos/convex/html",
        "--skip-codegen",
    ]);
    assert_eq!(cli.app_dir, Some(PathBuf::from("./demos/convex/html")));
    assert!(cli.skip_codegen);
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
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        true,
    );

    assert!(
        lines
            .iter()
            .any(|line| line == "Neovex server listening at http://localhost:3210/")
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
            .any(|line| line == "compose file: ./compose.yaml")
    );
    assert!(lines.iter().any(|line| line == "deploy admin API: enabled"));
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
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        false,
    );

    assert!(lines.iter().any(|line| {
        line == "compose file: auto-discovered /workspace/compose.yaml (+ compose.override.yaml)"
    }));
}

#[test]
fn start_startup_summary_reports_auto_detected_app_dir() {
    let command = StartCommand::default();
    let lines = super::boot::start_startup_summary_lines(
        &command,
        Some(&super::boot::ResolvedStartAppDir::AutoDetected(
            PathBuf::from("/workspace/functions"),
        )),
        None,
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        false,
    );

    assert!(
        lines
            .iter()
            .any(|line| line == "app dir: auto-detected /workspace/functions")
    );
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
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3210)),
        false,
    );

    assert!(lines.iter().any(|line| {
        line == "compose file: COMPOSE_FILE=./compose.yaml (+ 1 extra Compose files)"
    }));
}

#[test]
fn start_compose_selection_discovers_from_current_dir_not_app_dir() {
    let temp = tempfile::tempdir().expect("tempdir should build");
    let project_root = temp.path().join("workspace");
    let nested_cwd = project_root.join("apps").join("web");
    let app_dir = temp.path().join("separate-app");
    fs::create_dir_all(&nested_cwd).expect("nested cwd should build");
    fs::create_dir_all(app_dir.join("convex")).expect("app dir should build");
    let compose_path = project_root.join("compose.yaml");
    fs::write(
        &compose_path,
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
    .expect("compose selection should resolve")
    .expect("compose selection should be discovered");

    assert_eq!(
        fs::canonicalize(selection.primary_file()).unwrap(),
        fs::canonicalize(&compose_path).unwrap()
    );
    assert_eq!(
        fs::canonicalize(&selection.project_root).unwrap(),
        fs::canonicalize(&project_root).unwrap()
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
    let cli = parse_codegen(["neovex", "codegen"]);
    assert_eq!(cli.app, PathBuf::from("."));
}

#[test]
fn cli_parses_codegen_command_with_explicit_app_dir() {
    let cli = parse_codegen(["neovex", "codegen", "--app", "./demos/convex/html"]);
    assert_eq!(cli.app, PathBuf::from("./demos/convex/html"));
}

#[test]
fn cli_rejects_legacy_convex_app_dir_flag() {
    let error = Cli::try_parse_from(["neovex", "start", "--convex-app-dir", "./demo"])
        .expect_err("legacy app-dir flag should be removed");
    assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    assert!(error.to_string().contains("--convex-app-dir"));
}

#[test]
fn start_help_shows_app_dir_flag_name() {
    let error =
        Cli::try_parse_from(["neovex", "start", "--help"]).expect_err("help should short-circuit");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let rendered = error.to_string();
    assert!(rendered.contains("--host"));
    assert!(rendered.contains("--app-dir"));
    assert!(rendered.contains("--skip-codegen"));
    assert!(rendered.contains("neovex start --app-dir ./demos/convex/html"));
    assert!(rendered.contains("neovex start --app-dir ./demos/convex/html --skip-codegen"));
    assert!(rendered.contains("COMPOSE_FILE"));
    assert!(!rendered.contains("--convex-app-dir"));
}

#[test]
fn start_missing_functions_manifest_reports_actionable_error() {
    let temp = tempdir_in_repo_target();
    let app_dir = temp.path().to_path_buf();
    let command = StartCommand {
        app_dir: Some(app_dir.clone()),
        skip_codegen: true,
        ..StartCommand::default()
    };

    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    let error = super::boot::load_convex_registry(
        &command,
        resolved_app_dir.as_ref(),
        &RuntimeLimits::default(),
    )
    .expect_err("missing functions manifest should fail registry loading");
    let rendered = error.to_string();
    let functions_path = app_dir
        .join(".neovex")
        .join("convex")
        .join("functions.json");
    assert!(
        rendered.contains(&format!(
            "No generated function manifest found at {}.",
            functions_path.display()
        )),
        "error should point at the missing manifest: {rendered}"
    );
    assert!(
        rendered.contains(&format!("neovex codegen --app {}", app_dir.display())),
        "error should include the exact codegen command: {rendered}"
    );
    assert!(
        rendered.contains("--skip-codegen"),
        "error should explain the skip-codegen escape hatch: {rendered}"
    );
}

#[test]
fn load_convex_registry_accepts_manifest_only_app_dir_without_bundle() {
    let temp = tempdir_in_repo_target();
    let convex_dir = temp.path().join(".neovex").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [{
                "name": "messages:list",
                "kind": "query",
                "plan": {
                    "type": "limit",
                    "source": { "type": "scan", "table": "messages" },
                    "limit": 20
                }
            }]
        }))
        .expect("manifest json should serialize"),
    )
    .expect("manifest should write");

    let command = StartCommand {
        app_dir: Some(temp.path().to_path_buf()),
        skip_codegen: true,
        ..StartCommand::default()
    };
    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    let registry = super::boot::load_convex_registry(
        &command,
        resolved_app_dir.as_ref(),
        &RuntimeLimits::default(),
    )
    .expect("manifest-only app dir should load");
    assert!(
        registry.is_some(),
        "manifest-only app dir should still load a registry without bundle.mjs"
    );
}

#[test]
fn load_cloud_functions_registry_accepts_generated_app_dir() {
    let temp = tempdir_in_repo_target();
    write_generated_cloud_functions_artifacts(temp.path());

    let command = StartCommand {
        app_dir: Some(temp.path().to_path_buf()),
        skip_codegen: true,
        ..StartCommand::default()
    };
    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    let registry = super::boot::load_cloud_functions_registry(
        &command,
        resolved_app_dir.as_ref(),
        &RuntimeLimits::default(),
    )
    .expect("generated cloud functions app dir should load");
    assert!(
        registry.is_some(),
        "generated cloud functions app dir should load a registry"
    );
}

#[test]
fn resolve_start_app_dir_auto_detects_firebase_project_root_from_nested_child() {
    let temp = tempdir_in_repo_target();
    write_firebase_cloud_functions_fixture(temp.path());
    let nested_child = temp.path().join("functions").join("src");

    let resolved = with_current_dir(&nested_child, || {
        super::boot::resolve_start_app_dir(&StartCommand::default())
    })
    .expect("start app dir should resolve")
    .expect("start app dir should auto-detect");

    assert_eq!(
        resolved,
        super::boot::ResolvedStartAppDir::AutoDetected(
            temp.path()
                .canonicalize()
                .expect("tempdir should canonicalize")
        )
    );
}

#[test]
fn load_cloud_functions_registry_auto_detects_generated_app_dir_from_nested_child() {
    let temp = tempdir_in_repo_target();
    write_firebase_cloud_functions_fixture(temp.path());
    write_generated_cloud_functions_artifacts(temp.path());
    let nested_child = temp.path().join("functions").join("src");

    let registry = with_current_dir(&nested_child, || {
        let command = StartCommand {
            skip_codegen: true,
            ..StartCommand::default()
        };
        let resolved = super::boot::resolve_start_app_dir(&command)
            .expect("start app dir should resolve")
            .expect("start app dir should auto-detect");
        super::boot::load_cloud_functions_registry(
            &command,
            Some(&resolved),
            &RuntimeLimits::default(),
        )
        .expect("generated cloud functions app dir should load")
    });

    assert!(registry.is_some(), "auto-detected Firebase app should load");
}

#[test]
fn load_cloud_functions_registry_honors_explicit_override_for_nested_framework_package() {
    let temp = tempdir_in_repo_target();
    write_firebase_cloud_functions_fixture(temp.path());
    write_generated_cloud_functions_artifacts(temp.path());

    let nested_framework = temp.path().join("packages").join("functions");
    fs::create_dir_all(&nested_framework).expect("nested framework dir should create");
    write_framework_cloud_functions_fixture(&nested_framework);
    write_generated_cloud_functions_artifacts(&nested_framework);

    let registry = with_current_dir(temp.path(), || {
        let command = StartCommand {
            app_dir: Some(nested_framework.clone()),
            skip_codegen: true,
            ..StartCommand::default()
        };
        let resolved = super::boot::resolve_start_app_dir(&command)
            .expect("explicit app dir should resolve")
            .expect("explicit app dir should persist");
        super::boot::load_cloud_functions_registry(
            &command,
            Some(&resolved),
            &RuntimeLimits::default(),
        )
        .expect("explicit framework app dir should load")
        .expect("explicit framework app dir should produce a registry")
    });

    assert_eq!(
        registry.artifact_dir(),
        nested_framework
            .join(".neovex")
            .join("firebase")
            .canonicalize()
            .expect("framework artifact dir should canonicalize")
    );
}

#[tokio::test]
async fn start_codegen_preflight_generates_runtime_artifacts() {
    if !workspace_codegen_dependencies_available() {
        eprintln!(
            "skipping codegen preflight integration test; workspace JS dependencies are unavailable"
        );
        return;
    }

    let temp = tempdir_in_repo_target();
    write_codegen_source_fixture(temp.path());

    let command = StartCommand {
        app_dir: Some(temp.path().to_path_buf()),
        ..StartCommand::default()
    };

    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    super::boot::run_codegen_preflight(&command, resolved_app_dir.as_ref())
        .await
        .expect("codegen preflight should succeed");

    let convex_dir = temp.path().join(".neovex").join("convex");
    assert!(
        convex_dir.join("functions.json").is_file(),
        "functions manifest should be generated"
    );
    assert!(
        convex_dir.join("bundle.mjs").is_file(),
        "runtime bundle should be generated"
    );
    assert!(
        temp.path()
            .join("convex")
            .join("_generated")
            .join("api.ts")
            .is_file(),
        "_generated api file should be generated"
    );
}

#[tokio::test]
async fn start_codegen_preflight_generates_cloud_functions_artifacts() {
    if !workspace_codegen_dependencies_available() {
        eprintln!(
            "skipping cloud functions codegen preflight integration test; workspace JS dependencies are unavailable"
        );
        return;
    }

    let temp = tempdir_in_repo_target();
    write_firebase_cloud_functions_fixture(temp.path());

    let command = StartCommand {
        app_dir: Some(temp.path().to_path_buf()),
        ..StartCommand::default()
    };

    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    super::boot::run_codegen_preflight(&command, resolved_app_dir.as_ref())
        .await
        .expect("cloud functions codegen preflight should succeed");

    let firebase_dir = temp.path().join(".neovex").join("firebase");
    assert!(
        firebase_dir.join("artifact.json").is_file(),
        "cloud functions artifact manifest should be generated"
    );
    assert!(
        firebase_dir.join("targets.json").is_file(),
        "cloud functions targets manifest should be generated"
    );
    assert!(
        firebase_dir.join("bundle.mjs").is_file(),
        "cloud functions runtime bundle should be generated"
    );
}

#[tokio::test]
async fn start_codegen_preflight_generates_framework_cloud_functions_artifacts() {
    if !workspace_codegen_dependencies_available() {
        eprintln!(
            "skipping framework cloud functions codegen preflight integration test; workspace JS dependencies are unavailable"
        );
        return;
    }

    let temp = tempdir_in_repo_target();
    write_framework_cloud_functions_fixture(temp.path());

    let command = StartCommand {
        app_dir: Some(temp.path().to_path_buf()),
        ..StartCommand::default()
    };

    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    super::boot::run_codegen_preflight(&command, resolved_app_dir.as_ref())
        .await
        .expect("framework cloud functions codegen preflight should succeed");

    let firebase_dir = temp.path().join(".neovex").join("firebase");
    assert!(
        firebase_dir.join("artifact.json").is_file(),
        "cloud functions artifact manifest should be generated"
    );
    assert!(
        firebase_dir.join("targets.json").is_file(),
        "framework targets manifest should be preserved and normalized"
    );
    assert!(
        firebase_dir.join("bundle.mjs").is_file(),
        "cloud functions runtime bundle should be generated"
    );
}

#[tokio::test]
async fn start_codegen_preflight_honors_skip_codegen() {
    let temp = tempdir_in_repo_target();
    write_codegen_source_fixture(temp.path());

    let command = StartCommand {
        app_dir: Some(temp.path().to_path_buf()),
        skip_codegen: true,
        ..StartCommand::default()
    };

    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    super::boot::run_codegen_preflight(&command, resolved_app_dir.as_ref())
        .await
        .expect("skip-codegen should bypass preflight");

    let convex_dir = temp.path().join(".neovex").join("convex");
    assert!(
        !convex_dir.join("functions.json").exists(),
        "skip-codegen should leave manifests untouched"
    );
    assert!(
        !temp
            .path()
            .join("convex")
            .join("_generated")
            .join("api.ts")
            .exists(),
        "skip-codegen should leave generated source untouched"
    );
}

#[test]
fn cli_builds_postgres_typed_config_with_overrides() {
    let cli = parse_start([
        "neovex",
        "start",
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
    let config = persistence_config_from_sources(
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
    let cli = parse_start(["neovex", "start"]);
    let env = PersistenceEnv {
        tenant_provider: Some(CliTenantProvider::Postgres),
        control_data_dir: Some(PathBuf::from("./control-from-env")),
        postgres_url: Some("host=/tmp user=jack dbname=postgres".to_string()),
        postgres_min_connections: Some(3),
        postgres_max_connections: Some(9),
        ..PersistenceEnv::default()
    };

    let config = persistence_config_from_sources(&cli, &PersistenceFileConfig::default(), &env)
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
    let cli = parse_start([
        "neovex",
        "start",
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
    let config = persistence_config_from_sources(
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
    let cli = parse_start(["neovex", "start"]);
    let env = PersistenceEnv {
        tenant_provider: Some(CliTenantProvider::LibsqlReplica),
        control_data_dir: Some(PathBuf::from("./control-from-env")),
        libsql_url: Some("libsql://127.0.0.1:8080".to_string()),
        libsql_admin_url: Some("http://127.0.0.1:8081".to_string()),
        libsql_replica_cache_dir: Some(PathBuf::from("./replica-cache-from-env")),
        ..PersistenceEnv::default()
    };

    let config = persistence_config_from_sources(&cli, &PersistenceFileConfig::default(), &env)
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
    let cli = parse_start([
        "neovex",
        "start",
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
    let config = persistence_config_from_sources(
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
    let cli = parse_start(["neovex", "start"]);
    let env = PersistenceEnv {
        tenant_provider: Some(CliTenantProvider::Mysql),
        control_data_dir: Some(PathBuf::from("./control-from-env")),
        mysql_url: Some("mysql://root:password@127.0.0.1:3306/neovex".to_string()),
        mysql_min_connections: Some(3),
        mysql_max_connections: Some(9),
        ..PersistenceEnv::default()
    };

    let config = persistence_config_from_sources(&cli, &PersistenceFileConfig::default(), &env)
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
    let cli = parse_start(["neovex", "start", "--config", path.to_str().unwrap()]);
    let file_config =
        load_runtime_config_file(Some(path.as_path())).expect("config file should load");

    let config =
        persistence_config_from_sources(&cli, &file_config.persistence, &PersistenceEnv::default())
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
    let cli = parse_start([
        "neovex",
        "start",
        "--config",
        path.to_str().unwrap(),
        "--postgres-max-connections",
        "8",
    ]);
    let file_config =
        load_runtime_config_file(Some(path.as_path())).expect("config file should load");

    let config =
        persistence_config_from_sources(&cli, &file_config.persistence, &PersistenceEnv::default())
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
    let context = crate::compose::load_compose_project_context(&compose_path, &control_data_dir)
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
        crate::compose::load_sandbox_service_manager(
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

fn tempdir_in_repo_target() -> tempfile::TempDir {
    let repo_root = repo_root();
    let target_dir = repo_root.join("target");
    fs::create_dir_all(&target_dir).expect("repo target dir should exist");
    tempfile::tempdir_in(&target_dir).expect("tempdir in repo target should create")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate manifest dir should have repo root")
        .to_path_buf()
}

fn workspace_codegen_dependencies_available() -> bool {
    let repo_root = repo_root();
    repo_root.join("packages/codegen/src/main.mjs").is_file()
        && (repo_root.join("node_modules/esbuild").is_dir()
            || repo_root
                .join("packages/codegen/node_modules/esbuild")
                .is_dir())
}

fn write_codegen_source_fixture(app_dir: &Path) {
    let convex_dir = app_dir.join("convex");
    fs::create_dir_all(&convex_dir).expect("convex source dir should create");
    fs::write(
        convex_dir.join("messages.ts"),
        r#"
import { query } from "./_generated/server";

export const list = query({
  args: {},
  handler: async () => [],
});
"#,
    )
    .expect("convex source fixture should write");
}

fn write_firebase_cloud_functions_fixture(app_dir: &Path) {
    let functions_dir = app_dir.join("functions");
    let source_dir = functions_dir.join("src");
    fs::create_dir_all(&source_dir).expect("firebase functions source dir should create");
    fs::write(
        app_dir.join("firebase.json"),
        r#"{
  "functions": { "source": "functions" }
}
"#,
    )
    .expect("firebase.json should write");
    fs::write(
        functions_dir.join("package.json"),
        r#"{
  "main": "lib/index.js"
}
"#,
    )
    .expect("functions package.json should write");
    fs::write(
        source_dir.join("index.ts"),
        r#"
import { onDocumentCreated } from "firebase-functions/v2/firestore";

export const syncUser = onDocumentCreated("users/{userId}", async (event) => event);
"#,
    )
    .expect("firebase source fixture should write");
}

fn write_generated_cloud_functions_artifacts(app_dir: &Path) {
    let firebase_dir = app_dir.join(".neovex").join("firebase");
    fs::create_dir_all(&firebase_dir).expect("firebase manifest directory should build");
    fs::write(
        firebase_dir.join("artifact.json"),
        r#"{"version":1,"family":"cloud_functions","runtime_bundle":{"entry_file":"bundle.mjs","sha256_file":"bundle.sha256"},"targets_manifest":"targets.json","import_resolution":{"strategy":"deploy_alias_layer","covered_specifiers":["@google-cloud/functions-framework","firebase-admin/app","firebase-admin/firestore","firebase-functions/v2","firebase-functions/v2/firestore","firebase-functions/v2/https"]}}"#,
    )
    .expect("artifact manifest should write");
    fs::write(
        firebase_dir.join("targets.json"),
        r#"{"version":1,"targets":[]}"#,
    )
    .expect("targets should write");
    fs::write(firebase_dir.join("bundle.mjs"), "export const value = 1;\n")
        .expect("bundle should write");
    fs::write(firebase_dir.join("bundle.sha256"), "a".repeat(64)).expect("bundle sha should write");
}

fn write_framework_cloud_functions_fixture(app_dir: &Path) {
    let source_dir = app_dir.join("src");
    let generated_dir = app_dir.join(".neovex").join("firebase");
    fs::create_dir_all(&source_dir).expect("framework source dir should create");
    fs::create_dir_all(&generated_dir).expect("framework generated dir should create");
    fs::write(
        app_dir.join("package.json"),
        r#"{
  "main": "dist/index.js",
  "dependencies": {
    "@google-cloud/functions-framework": "^3.4.5"
  }
}
"#,
    )
    .expect("framework package.json should write");
    fs::write(
        generated_dir.join("targets.json"),
        r#"{
  "version": 1,
  "targets": [
    {
      "name": "syncUser",
      "entrypoint": "registry.syncUser",
      "authoring_surface": "functions_framework",
      "signature_type": "cloud_event",
      "binding": {
        "binding_kind": "firestore_document",
        "event_type": "google.cloud.firestore.document.v1.written",
        "database": "(default)",
        "document": "users/{userId}",
        "execution": "service"
      }
    }
  ]
}
"#,
    )
    .expect("framework targets manifest should write");
    fs::write(
        source_dir.join("index.ts"),
        r#"
import functions from "@google-cloud/functions-framework";

functions.cloudEvent("syncUser", async (event) => event);
"#,
    )
    .expect("framework source fixture should write");
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
    let cli = parse_start(["neovex", "start"]);
    let config = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("default config should build");
    assert!(!config.local_encryption.is_enabled());
}

#[test]
fn cli_builds_master_key_file_encryption_config() {
    let cli = parse_start([
        "neovex",
        "start",
        "--encryption-key-provider",
        "master-key-file",
        "--encryption-master-key-file",
        "/secure/neovex.key",
    ]);
    let config = persistence_config_from_sources(
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
    let cli = parse_start([
        "neovex",
        "start",
        "--encryption-key-provider",
        "key-dir",
        "--encryption-key-dir",
        "/secure/keys",
    ]);
    let config = persistence_config_from_sources(
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
    let cli = parse_start([
        "neovex",
        "start",
        "--encryption-key-provider",
        "aws-kms",
        "--encryption-aws-kms-key-id",
        "arn:aws:kms:us-east-1:123456789:key/example-key-id",
        "--encryption-aws-region",
        "us-east-1",
    ]);
    let config = persistence_config_from_sources(
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
    let cli = parse_start([
        "neovex",
        "start",
        "--encryption-master-key-file",
        "/secure/neovex.key",
    ]);
    let result = persistence_config_from_sources(
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
    let cli = parse_start([
        "neovex",
        "start",
        "--encryption-key-provider",
        "master-key-file",
        "--encryption-master-key-file",
        "/secure/neovex.key",
        "--encryption-aws-kms-key-id",
        "arn:aws:kms:us-east-1:123456789:key/example-key-id",
    ]);
    let result = persistence_config_from_sources(
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
    let cli = parse_start([
        "neovex",
        "start",
        "--encryption-key-provider",
        "master-key-file",
    ]);
    let result = persistence_config_from_sources(
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
    let cli = parse_start(["neovex", "start", "--encryption-key-provider", "aws-kms"]);
    let result = persistence_config_from_sources(
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

// -------------------------------------------------------------------------
// License path resolution tests
// -------------------------------------------------------------------------

fn license_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct LicenseEnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl LicenseEnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = env::var_os(key);
        unsafe { env::set_var(key, value) };
        Self { key, previous }
    }

    fn clear(key: &'static str) -> Self {
        let previous = env::var_os(key);
        unsafe { env::remove_var(key) };
        Self { key, previous }
    }
}

impl Drop for LicenseEnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe { env::set_var(self.key, value) },
            None => unsafe { env::remove_var(self.key) },
        }
    }
}

#[test]
fn resolve_license_path_returns_explicit_path() {
    let path = Path::new("/explicit/license.json");
    let result = super::boot::resolve_license_path(Some(path));
    assert_eq!(result, Some(PathBuf::from("/explicit/license.json")));
}

#[test]
fn resolve_license_path_defers_to_env_when_set() {
    let _lock = license_env_lock()
        .lock()
        .expect("license env lock should not be poisoned");
    let _guard = LicenseEnvGuard::set(neovex::LICENSE_FILE_ENV, "/env/license.json");
    let result = super::boot::resolve_license_path(None);
    assert!(
        result.is_none(),
        "should return None so LicenseState::load handles the env var, got: {result:?}"
    );
}

#[test]
fn resolve_license_path_returns_xdg_default_when_file_exists() {
    let _lock = license_env_lock()
        .lock()
        .expect("license env lock should not be poisoned");
    let _guard = LicenseEnvGuard::clear(neovex::LICENSE_FILE_ENV);
    let temp = tempfile::tempdir().expect("tempdir should build");
    let config_dir = temp.path().join("neovex");
    fs::create_dir_all(&config_dir).expect("config dir should build");
    fs::write(config_dir.join("license.json"), "{}").expect("license file should write");
    let _xdg_guard = LicenseEnvGuard::set("XDG_CONFIG_HOME", temp.path().to_str().unwrap());
    let result = super::boot::resolve_license_path(None);
    assert_eq!(
        result,
        Some(config_dir.join("license.json")),
        "should return the XDG default path when the file exists"
    );
}

#[test]
fn resolve_license_path_returns_none_when_no_xdg_default() {
    let _lock = license_env_lock()
        .lock()
        .expect("license env lock should not be poisoned");
    let _guard = LicenseEnvGuard::clear(neovex::LICENSE_FILE_ENV);
    let temp = tempfile::tempdir().expect("tempdir should build");
    let _xdg_guard = LicenseEnvGuard::set("XDG_CONFIG_HOME", temp.path().to_str().unwrap());
    let result = super::boot::resolve_license_path(None);
    assert!(
        result.is_none(),
        "should return None when XDG license file does not exist, got: {result:?}"
    );
}
