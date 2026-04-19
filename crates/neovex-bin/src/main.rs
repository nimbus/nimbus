use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use neovex::{
    ConvexRegistry, EmbeddedProviderKind, Error, LicenseState, RuntimeLimits, SandboxCatalog,
    Service, ServicePersistenceConfig, run_scheduler, serve_with_convex_and_license,
    serve_with_convex_and_license_and_sandbox_service_manager, serve_with_license,
    serve_with_license_and_sandbox_catalog,
};
use serde::Deserialize;

mod cli_ux;
mod machine;
mod service;
#[cfg(test)]
mod test_support;

use crate::machine::{MachineCommand, run_machine_command};
use crate::service::{
    ServiceCommand, load_host_backed_sandbox_service_manager, run_service_command,
};

#[cfg(all(test, target_os = "linux"))]
use crate::service::load_compose_project_context;
#[cfg(all(test, target_os = "linux"))]
use neovex_sandbox::backends::krun::{KrunLaunchMode, KrunSandboxBackend};

const DEFAULT_DATA_DIR: &str = "./data";
const CONFIG_FILE_ENV: &str = "NEOVEX_CONFIG";
const DATA_DIR_ENV: &str = "NEOVEX_DATA_DIR";
const CONTROL_DATA_DIR_ENV: &str = "NEOVEX_CONTROL_DATA_DIR";
const TENANT_PROVIDER_ENV: &str = "NEOVEX_TENANT_PROVIDER";
const LIBSQL_URL_ENV: &str = "NEOVEX_LIBSQL_URL";
const LIBSQL_AUTH_TOKEN_ENV: &str = "NEOVEX_LIBSQL_AUTH_TOKEN";
const LIBSQL_ADMIN_URL_ENV: &str = "NEOVEX_LIBSQL_ADMIN_URL";
const LIBSQL_ADMIN_AUTH_HEADER_ENV: &str = "NEOVEX_LIBSQL_ADMIN_AUTH_HEADER";
const LIBSQL_METADATA_NAMESPACE_ENV: &str = "NEOVEX_LIBSQL_METADATA_NAMESPACE";
const LIBSQL_TENANT_NAMESPACE_PREFIX_ENV: &str = "NEOVEX_LIBSQL_TENANT_NAMESPACE_PREFIX";
const LIBSQL_REPLICA_CACHE_DIR_ENV: &str = "NEOVEX_LIBSQL_REPLICA_CACHE_DIR";
const POSTGRES_URL_ENV: &str = "NEOVEX_POSTGRES_URL";
const POSTGRES_METADATA_SCHEMA_ENV: &str = "NEOVEX_POSTGRES_METADATA_SCHEMA";
const POSTGRES_TENANT_SCHEMA_PREFIX_ENV: &str = "NEOVEX_POSTGRES_TENANT_SCHEMA_PREFIX";
const POSTGRES_MIN_CONNECTIONS_ENV: &str = "NEOVEX_POSTGRES_MIN_CONNECTIONS";
const POSTGRES_MAX_CONNECTIONS_ENV: &str = "NEOVEX_POSTGRES_MAX_CONNECTIONS";
const MYSQL_URL_ENV: &str = "NEOVEX_MYSQL_URL";
const MYSQL_METADATA_DATABASE_ENV: &str = "NEOVEX_MYSQL_METADATA_DATABASE";
const MYSQL_TENANT_DATABASE_PREFIX_ENV: &str = "NEOVEX_MYSQL_TENANT_DATABASE_PREFIX";
const MYSQL_MIN_CONNECTIONS_ENV: &str = "NEOVEX_MYSQL_MIN_CONNECTIONS";
const MYSQL_MAX_CONNECTIONS_ENV: &str = "NEOVEX_MYSQL_MAX_CONNECTIONS";

#[derive(Debug, Parser)]
#[command(
    name = "neovex",
    version,
    about = "Reactive document database with machine and service orchestration",
    help_template = cli_ux::ROOT_HELP_TEMPLATE,
    after_help = cli_ux::ROOT_HELP_EXAMPLES,
    subcommand_help_heading = "Available Commands"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVE_HELP_EXAMPLES
)]
struct ServeCommand {
    /// Optional JSON config file. CLI flags override env and file values.
    #[arg(long)]
    config: Option<PathBuf>,

    /// Port to listen on.
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Local data directory used for embedded tenant databases and, by default,
    /// the local redb control plane.
    #[arg(long)]
    data_dir: Option<PathBuf>,

    /// Optional override for the local redb control-plane directory.
    #[arg(long)]
    control_data_dir: Option<PathBuf>,

    /// Tenant persistence provider mode.
    #[arg(long, value_enum)]
    tenant_provider: Option<CliTenantProvider>,

    /// Canonical libsql primary URL for tenant persistence when
    /// `--tenant-provider=libsql-replica`.
    #[arg(long)]
    libsql_url: Option<String>,

    /// Optional auth token for the libsql primary when
    /// `--tenant-provider=libsql-replica`.
    #[arg(long)]
    libsql_auth_token: Option<String>,

    /// Admin API URL used to provision libsql namespaces when
    /// `--tenant-provider=libsql-replica`.
    #[arg(long)]
    libsql_admin_url: Option<String>,

    /// Optional `Authorization` header value for the libsql admin API when
    /// `--tenant-provider=libsql-replica`.
    #[arg(long)]
    libsql_admin_auth_header: Option<String>,

    /// Provider metadata namespace for replica-connected SQLite tenant routing.
    #[arg(long)]
    libsql_metadata_namespace: Option<String>,

    /// Prefix used when deriving per-tenant libsql namespaces.
    #[arg(long)]
    libsql_tenant_namespace_prefix: Option<String>,

    /// Provider-owned local cache root for embedded replica files when
    /// `--tenant-provider=libsql-replica`.
    #[arg(long)]
    libsql_replica_cache_dir: Option<PathBuf>,

    /// Canonical Postgres resource URL for tenant persistence when
    /// `--tenant-provider=postgres`.
    #[arg(long)]
    postgres_url: Option<String>,

    /// Provider metadata schema for Postgres tenant routing.
    #[arg(long)]
    postgres_metadata_schema: Option<String>,

    /// Prefix used when deriving per-tenant Postgres schema names.
    #[arg(long)]
    postgres_tenant_schema_prefix: Option<String>,

    /// Minimum Postgres pool size.
    #[arg(long)]
    postgres_min_connections: Option<usize>,

    /// Maximum Postgres pool size.
    #[arg(long)]
    postgres_max_connections: Option<usize>,

    /// Canonical MySQL resource URL for tenant persistence when
    /// `--tenant-provider=mysql`.
    #[arg(long)]
    mysql_url: Option<String>,

    /// Provider metadata database for MySQL tenant routing.
    #[arg(long)]
    mysql_metadata_database: Option<String>,

    /// Prefix used when deriving per-tenant MySQL database names.
    #[arg(long)]
    mysql_tenant_database_prefix: Option<String>,

    /// Minimum MySQL pool size.
    #[arg(long)]
    mysql_min_connections: Option<usize>,

    /// Maximum MySQL pool size.
    #[arg(long)]
    mysql_max_connections: Option<usize>,

    /// Optional app directory with a generated .neovex/convex/functions.json manifest.
    #[arg(long)]
    convex_app_dir: Option<PathBuf>,

    /// Optional Compose file that declares sandbox-backed services for
    /// `ctx.services.*` activation.
    #[arg(long)]
    compose_file: Option<PathBuf>,

    /// Optional path to a Neovex license file. Defaults to ./.neovex/license.json when present.
    #[arg(long)]
    license_file: Option<PathBuf>,

    /// V8 heap limit per runtime isolate in megabytes.
    #[arg(long, default_value_t = default_runtime_heap_mb())]
    runtime_heap_mb: usize,

    /// Initial V8 heap size per runtime isolate in megabytes.
    #[arg(long, default_value_t = default_runtime_initial_heap_mb())]
    runtime_initial_heap_mb: usize,

    /// Maximum wall-clock execution time for a runtime invocation in seconds.
    #[arg(long, default_value_t = default_runtime_timeout_secs())]
    runtime_timeout_secs: u64,

    /// Maximum number of concurrent top-level runtime instances.
    #[arg(long, default_value_t = default_runtime_max_instances())]
    runtime_max_instances: usize,

    /// Number of runtime worker threads.
    #[arg(long, default_value_t = default_runtime_worker_threads())]
    runtime_worker_threads: usize,

    /// Maximum number of nested runtime ctx.run* invocations allowed per request tree.
    #[arg(long, default_value_t = default_runtime_max_nested_calls())]
    runtime_max_nested_calls: usize,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve(Box<ServeCommand>),
    Machine(MachineCommand),
    Service(ServiceCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum CliTenantProvider {
    Sqlite,
    LibsqlReplica,
    Redb,
    Postgres,
    Mysql,
}

impl CliTenantProvider {
    fn parse_name(value: &str) -> neovex::Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "sqlite" => Ok(Self::Sqlite),
            "libsql-replica" | "libsql_replica" => Ok(Self::LibsqlReplica),
            "redb" => Ok(Self::Redb),
            "postgres" => Ok(Self::Postgres),
            "mysql" => Ok(Self::Mysql),
            other => Err(Error::InvalidInput(format!(
                "unsupported tenant provider '{other}'; expected sqlite, libsql-replica, redb, postgres, or mysql"
            ))),
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RuntimeConfigFile {
    persistence: PersistenceFileConfig,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PersistenceFileConfig {
    data_dir: Option<PathBuf>,
    control_data_dir: Option<PathBuf>,
    tenant_provider: Option<CliTenantProvider>,
    libsql_url: Option<String>,
    libsql_auth_token: Option<String>,
    libsql_admin_url: Option<String>,
    libsql_admin_auth_header: Option<String>,
    libsql_metadata_namespace: Option<String>,
    libsql_tenant_namespace_prefix: Option<String>,
    libsql_replica_cache_dir: Option<PathBuf>,
    postgres_url: Option<String>,
    postgres_metadata_schema: Option<String>,
    postgres_tenant_schema_prefix: Option<String>,
    postgres_min_connections: Option<usize>,
    postgres_max_connections: Option<usize>,
    mysql_url: Option<String>,
    mysql_metadata_database: Option<String>,
    mysql_tenant_database_prefix: Option<String>,
    mysql_min_connections: Option<usize>,
    mysql_max_connections: Option<usize>,
}

#[derive(Debug, Default, Clone)]
struct PersistenceEnv {
    data_dir: Option<PathBuf>,
    control_data_dir: Option<PathBuf>,
    tenant_provider: Option<CliTenantProvider>,
    libsql_url: Option<String>,
    libsql_auth_token: Option<String>,
    libsql_admin_url: Option<String>,
    libsql_admin_auth_header: Option<String>,
    libsql_metadata_namespace: Option<String>,
    libsql_tenant_namespace_prefix: Option<String>,
    libsql_replica_cache_dir: Option<PathBuf>,
    postgres_url: Option<String>,
    postgres_metadata_schema: Option<String>,
    postgres_tenant_schema_prefix: Option<String>,
    postgres_min_connections: Option<usize>,
    postgres_max_connections: Option<usize>,
    mysql_url: Option<String>,
    mysql_metadata_database: Option<String>,
    mysql_tenant_database_prefix: Option<String>,
    mysql_min_connections: Option<usize>,
    mysql_max_connections: Option<usize>,
}

impl PersistenceEnv {
    fn load() -> neovex::Result<Self> {
        Ok(Self {
            data_dir: std::env::var_os(DATA_DIR_ENV).map(PathBuf::from),
            control_data_dir: std::env::var_os(CONTROL_DATA_DIR_ENV).map(PathBuf::from),
            tenant_provider: optional_env_tenant_provider(TENANT_PROVIDER_ENV)?,
            libsql_url: std::env::var(LIBSQL_URL_ENV).ok(),
            libsql_auth_token: std::env::var(LIBSQL_AUTH_TOKEN_ENV).ok(),
            libsql_admin_url: std::env::var(LIBSQL_ADMIN_URL_ENV).ok(),
            libsql_admin_auth_header: std::env::var(LIBSQL_ADMIN_AUTH_HEADER_ENV).ok(),
            libsql_metadata_namespace: std::env::var(LIBSQL_METADATA_NAMESPACE_ENV).ok(),
            libsql_tenant_namespace_prefix: std::env::var(LIBSQL_TENANT_NAMESPACE_PREFIX_ENV).ok(),
            libsql_replica_cache_dir: std::env::var_os(LIBSQL_REPLICA_CACHE_DIR_ENV)
                .map(PathBuf::from),
            postgres_url: std::env::var(POSTGRES_URL_ENV).ok(),
            postgres_metadata_schema: std::env::var(POSTGRES_METADATA_SCHEMA_ENV).ok(),
            postgres_tenant_schema_prefix: std::env::var(POSTGRES_TENANT_SCHEMA_PREFIX_ENV).ok(),
            postgres_min_connections: optional_env_usize(POSTGRES_MIN_CONNECTIONS_ENV)?,
            postgres_max_connections: optional_env_usize(POSTGRES_MAX_CONNECTIONS_ENV)?,
            mysql_url: std::env::var(MYSQL_URL_ENV).ok(),
            mysql_metadata_database: std::env::var(MYSQL_METADATA_DATABASE_ENV).ok(),
            mysql_tenant_database_prefix: std::env::var(MYSQL_TENANT_DATABASE_PREFIX_ENV).ok(),
            mysql_min_connections: optional_env_usize(MYSQL_MIN_CONNECTIONS_ENV)?,
            mysql_max_connections: optional_env_usize(MYSQL_MAX_CONNECTIONS_ENV)?,
        })
    }
}

fn default_runtime_heap_mb() -> usize {
    RuntimeLimits::default().max_heap_mb
}

fn default_runtime_initial_heap_mb() -> usize {
    RuntimeLimits::default().initial_heap_mb
}

fn default_runtime_timeout_secs() -> u64 {
    RuntimeLimits::default().execution_timeout.as_secs()
}

fn default_runtime_max_instances() -> usize {
    RuntimeLimits::default().max_concurrent_runtime_instances
}

fn default_runtime_worker_threads() -> usize {
    RuntimeLimits::default().worker_threads
}

fn default_runtime_max_nested_calls() -> usize {
    RuntimeLimits::default().max_nested_runtime_invocations
}

fn default_serve_command() -> ServeCommand {
    ServeCommand {
        config: None,
        port: 8080,
        data_dir: None,
        control_data_dir: None,
        tenant_provider: None,
        libsql_url: None,
        libsql_auth_token: None,
        libsql_admin_url: None,
        libsql_admin_auth_header: None,
        libsql_metadata_namespace: None,
        libsql_tenant_namespace_prefix: None,
        libsql_replica_cache_dir: None,
        postgres_url: None,
        postgres_metadata_schema: None,
        postgres_tenant_schema_prefix: None,
        postgres_min_connections: None,
        postgres_max_connections: None,
        mysql_url: None,
        mysql_metadata_database: None,
        mysql_tenant_database_prefix: None,
        mysql_min_connections: None,
        mysql_max_connections: None,
        convex_app_dir: None,
        compose_file: None,
        license_file: None,
        runtime_heap_mb: default_runtime_heap_mb(),
        runtime_initial_heap_mb: default_runtime_initial_heap_mb(),
        runtime_timeout_secs: default_runtime_timeout_secs(),
        runtime_max_instances: default_runtime_max_instances(),
        runtime_worker_threads: default_runtime_worker_threads(),
        runtime_max_nested_calls: default_runtime_max_nested_calls(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    match cli.command {
        Command::Serve(command) => run_serve_command(*command).await?,
        Command::Machine(command) => {
            run_machine_command(command).await?;
        }
        Command::Service(command) => {
            let service_config =
                service_persistence_config_from_serve_command(&default_serve_command())?;
            run_service_command(command, &service_config).await?;
        }
    }
    Ok(())
}

async fn run_serve_command(command: ServeCommand) -> Result<(), Box<dyn std::error::Error>> {
    let service_config = service_persistence_config_from_serve_command(&command)?;
    let compose_control_data_dir =
        control_data_dir_from_service_config(&service_config).to_path_buf();
    let service = Arc::new(Service::new_with_persistence_config(service_config).await?);
    let shutdown_service = service.clone();
    service.recover_scheduled_work_on_startup_async().await?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let scheduler_service = service.clone();
    let scheduler_handle = tokio::spawn(async move {
        run_scheduler(scheduler_service, shutdown_rx).await;
    });
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", command.port)).await?;
    let runtime_limits = RuntimeLimits {
        max_heap_mb: command.runtime_heap_mb,
        initial_heap_mb: command.runtime_initial_heap_mb,
        execution_timeout: Duration::from_secs(command.runtime_timeout_secs),
        max_concurrent_runtime_instances: command.runtime_max_instances,
        worker_threads: command.runtime_worker_threads,
        max_nested_runtime_invocations: command.runtime_max_nested_calls,
        ..RuntimeLimits::default()
    };
    let license_state = LicenseState::load(command.license_file.as_deref())?;
    let license_snapshot = license_state.snapshot();
    let convex_registry = command
        .convex_app_dir
        .as_ref()
        .map(|path| {
            ConvexRegistry::from_app_dir(path)
                .map(|registry| registry.with_runtime_limits(runtime_limits.clone()))
        })
        .transpose()?;
    let sandbox_service_manager = command
        .compose_file
        .as_deref()
        .map(|path| load_host_backed_sandbox_service_manager(path, &compose_control_data_dir))
        .transpose()?;
    let sandbox_service_manager = sandbox_service_manager.map(Arc::new);

    tracing::info!(
        license_kind = ?license_snapshot.kind,
        license_status = ?license_snapshot.status,
        license_source = ?license_snapshot.source.kind,
        "loaded neovex license state"
    );
    for warning in &license_snapshot.warnings {
        tracing::warn!(license_warning = %warning, "neovex license warning");
    }

    tracing::info!("neovex listening on {}", listener.local_addr()?);
    let server_result = match (convex_registry, sandbox_service_manager) {
        (Some(registry), Some(manager)) => {
            serve_with_convex_and_license_and_sandbox_service_manager(
                listener,
                service,
                registry,
                license_state,
                manager,
            )
            .await
        }
        (Some(registry), None) => {
            serve_with_convex_and_license(listener, service, registry, license_state).await
        }
        (None, Some(manager)) => {
            let sandbox_catalog: Arc<dyn SandboxCatalog> = manager;
            serve_with_license_and_sandbox_catalog(
                listener,
                service,
                license_state,
                sandbox_catalog,
            )
            .await
        }
        (None, None) => serve_with_license(listener, service, license_state).await,
    };
    let _ = shutdown_tx.send(true);
    let _ = scheduler_handle.await;
    shutdown_service.quiesce().await;
    server_result?;
    Ok(())
}

fn service_persistence_config_from_serve_command(
    command: &ServeCommand,
) -> neovex::Result<ServicePersistenceConfig> {
    let config_path = command
        .config
        .clone()
        .or_else(|| std::env::var_os(CONFIG_FILE_ENV).map(PathBuf::from));
    let file_config = load_runtime_config_file(config_path.as_deref())?;
    let env = PersistenceEnv::load()?;
    service_persistence_config_from_sources(command, &file_config.persistence, &env)
}

fn service_persistence_config_from_sources(
    command: &ServeCommand,
    file: &PersistenceFileConfig,
    env: &PersistenceEnv,
) -> neovex::Result<ServicePersistenceConfig> {
    let data_dir = command
        .data_dir
        .clone()
        .or_else(|| env.data_dir.clone())
        .or_else(|| file.data_dir.clone())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR));
    let control_data_dir = command
        .control_data_dir
        .clone()
        .or_else(|| env.control_data_dir.clone())
        .or_else(|| file.control_data_dir.clone())
        .unwrap_or_else(|| data_dir.clone());
    let tenant_provider = command
        .tenant_provider
        .or(env.tenant_provider)
        .or(file.tenant_provider)
        .unwrap_or(CliTenantProvider::Sqlite);
    let libsql_url = command
        .libsql_url
        .clone()
        .or_else(|| env.libsql_url.clone())
        .or_else(|| file.libsql_url.clone());
    let libsql_auth_token = command
        .libsql_auth_token
        .clone()
        .or_else(|| env.libsql_auth_token.clone())
        .or_else(|| file.libsql_auth_token.clone());
    let libsql_admin_url = command
        .libsql_admin_url
        .clone()
        .or_else(|| env.libsql_admin_url.clone())
        .or_else(|| file.libsql_admin_url.clone());
    let libsql_admin_auth_header = command
        .libsql_admin_auth_header
        .clone()
        .or_else(|| env.libsql_admin_auth_header.clone())
        .or_else(|| file.libsql_admin_auth_header.clone());
    let libsql_metadata_namespace = command
        .libsql_metadata_namespace
        .clone()
        .or_else(|| env.libsql_metadata_namespace.clone())
        .or_else(|| file.libsql_metadata_namespace.clone());
    let libsql_tenant_namespace_prefix = command
        .libsql_tenant_namespace_prefix
        .clone()
        .or_else(|| env.libsql_tenant_namespace_prefix.clone())
        .or_else(|| file.libsql_tenant_namespace_prefix.clone());
    let libsql_replica_cache_dir = command
        .libsql_replica_cache_dir
        .clone()
        .or_else(|| env.libsql_replica_cache_dir.clone())
        .or_else(|| file.libsql_replica_cache_dir.clone());
    let postgres_url = command
        .postgres_url
        .clone()
        .or_else(|| env.postgres_url.clone())
        .or_else(|| file.postgres_url.clone());
    let postgres_metadata_schema = command
        .postgres_metadata_schema
        .clone()
        .or_else(|| env.postgres_metadata_schema.clone())
        .or_else(|| file.postgres_metadata_schema.clone());
    let postgres_tenant_schema_prefix = command
        .postgres_tenant_schema_prefix
        .clone()
        .or_else(|| env.postgres_tenant_schema_prefix.clone())
        .or_else(|| file.postgres_tenant_schema_prefix.clone());
    let postgres_min_connections = command
        .postgres_min_connections
        .or(env.postgres_min_connections)
        .or(file.postgres_min_connections);
    let postgres_max_connections = command
        .postgres_max_connections
        .or(env.postgres_max_connections)
        .or(file.postgres_max_connections);
    let mysql_url = command
        .mysql_url
        .clone()
        .or_else(|| env.mysql_url.clone())
        .or_else(|| file.mysql_url.clone());
    let mysql_metadata_database = command
        .mysql_metadata_database
        .clone()
        .or_else(|| env.mysql_metadata_database.clone())
        .or_else(|| file.mysql_metadata_database.clone());
    let mysql_tenant_database_prefix = command
        .mysql_tenant_database_prefix
        .clone()
        .or_else(|| env.mysql_tenant_database_prefix.clone())
        .or_else(|| file.mysql_tenant_database_prefix.clone());
    let mysql_min_connections = command
        .mysql_min_connections
        .or(env.mysql_min_connections)
        .or(file.mysql_min_connections);
    let mysql_max_connections = command
        .mysql_max_connections
        .or(env.mysql_max_connections)
        .or(file.mysql_max_connections);

    let libsql_replica_overrides_present = libsql_url.is_some()
        || libsql_auth_token.is_some()
        || libsql_admin_url.is_some()
        || libsql_admin_auth_header.is_some()
        || libsql_metadata_namespace.is_some()
        || libsql_tenant_namespace_prefix.is_some()
        || libsql_replica_cache_dir.is_some();
    let postgres_overrides_present = postgres_url.is_some()
        || postgres_metadata_schema.is_some()
        || postgres_tenant_schema_prefix.is_some()
        || postgres_min_connections.is_some()
        || postgres_max_connections.is_some();
    let mysql_overrides_present = mysql_url.is_some()
        || mysql_metadata_database.is_some()
        || mysql_tenant_database_prefix.is_some()
        || mysql_min_connections.is_some()
        || mysql_max_connections.is_some();

    match tenant_provider {
        CliTenantProvider::Sqlite => {
            if postgres_overrides_present
                || mysql_overrides_present
                || libsql_replica_overrides_present
            {
                return Err(Error::InvalidInput(
                    "External provider config requires --tenant-provider=libsql-replica, --tenant-provider=postgres, or --tenant-provider=mysql (or the equivalent env/config setting)"
                        .to_string(),
                ));
            }
            Ok(ServicePersistenceConfig {
                tenant_provider: neovex::TenantProviderConfig::embedded(
                    data_dir,
                    EmbeddedProviderKind::Sqlite,
                ),
                control_plane: neovex::ControlPlaneConfig::embedded_redb(control_data_dir),
            })
        }
        CliTenantProvider::Redb => {
            if postgres_overrides_present
                || mysql_overrides_present
                || libsql_replica_overrides_present
            {
                return Err(Error::InvalidInput(
                    "External provider config requires --tenant-provider=libsql-replica, --tenant-provider=postgres, or --tenant-provider=mysql (or the equivalent env/config setting)"
                        .to_string(),
                ));
            }
            Ok(ServicePersistenceConfig {
                tenant_provider: neovex::TenantProviderConfig::embedded(
                    data_dir,
                    EmbeddedProviderKind::Redb,
                ),
                control_plane: neovex::ControlPlaneConfig::embedded_redb(control_data_dir),
            })
        }
        CliTenantProvider::LibsqlReplica => {
            if postgres_overrides_present {
                return Err(Error::InvalidInput(
                    "Postgres config requires --tenant-provider=postgres or the equivalent env/config setting"
                        .to_string(),
                ));
            }
            if mysql_overrides_present {
                return Err(Error::InvalidInput(
                    "MySQL config requires --tenant-provider=mysql or the equivalent env/config setting"
                        .to_string(),
                ));
            }
            let libsql_url = libsql_url.ok_or_else(|| {
                Error::InvalidInput(
                    "--libsql-url, NEOVEX_LIBSQL_URL, or persistence.libsql_url is required when the tenant provider is libsql-replica"
                        .to_string(),
                )
            })?;
            let libsql_replica_cache_dir = libsql_replica_cache_dir.ok_or_else(|| {
                Error::InvalidInput(
                    "--libsql-replica-cache-dir, NEOVEX_LIBSQL_REPLICA_CACHE_DIR, or persistence.libsql_replica_cache_dir is required when the tenant provider is libsql-replica"
                        .to_string(),
                )
            })?;
            let libsql_admin_url = libsql_admin_url.ok_or_else(|| {
                Error::InvalidInput(
                    "--libsql-admin-url, NEOVEX_LIBSQL_ADMIN_URL, or persistence.libsql_admin_url is required when the tenant provider is libsql-replica"
                        .to_string(),
                )
            })?;
            let mut config = ServicePersistenceConfig::libsql_replica(
                control_data_dir,
                libsql_url,
                libsql_auth_token,
                libsql_admin_url,
                libsql_admin_auth_header,
                libsql_replica_cache_dir,
            );
            if let neovex::TenantRoutingConfig::NamespacePerTenant {
                metadata_namespace,
                tenant_namespace_prefix,
                ..
            } = &mut config.tenant_provider.routing
            {
                if let Some(value) = libsql_metadata_namespace {
                    *metadata_namespace = value;
                }
                if let Some(value) = libsql_tenant_namespace_prefix {
                    *tenant_namespace_prefix = value;
                }
            }
            Ok(config)
        }
        CliTenantProvider::Postgres => {
            if mysql_overrides_present {
                return Err(Error::InvalidInput(
                    "MySQL config requires --tenant-provider=mysql or the equivalent env/config setting"
                        .to_string(),
                ));
            }
            if libsql_replica_overrides_present {
                return Err(Error::InvalidInput(
                    "Replica-connected SQLite config requires --tenant-provider=libsql-replica or the equivalent env/config setting"
                        .to_string(),
                ));
            }
            let postgres_url = postgres_url.ok_or_else(|| {
                Error::InvalidInput(
                    "--postgres-url, NEOVEX_POSTGRES_URL, or persistence.postgres_url is required when the tenant provider is postgres"
                        .to_string(),
                )
            })?;
            let mut config = ServicePersistenceConfig::postgres(control_data_dir, postgres_url);
            if let neovex::TenantRoutingConfig::SchemaPerTenant {
                metadata_schema,
                tenant_schema_prefix,
            } = &mut config.tenant_provider.routing
            {
                if let Some(value) = postgres_metadata_schema {
                    *metadata_schema = value;
                }
                if let Some(value) = postgres_tenant_schema_prefix {
                    *tenant_schema_prefix = value;
                }
            }
            config.tenant_provider.pool.min_connections = postgres_min_connections;
            config.tenant_provider.pool.max_connections = postgres_max_connections;
            Ok(config)
        }
        CliTenantProvider::Mysql => {
            if postgres_overrides_present {
                return Err(Error::InvalidInput(
                    "Postgres config requires --tenant-provider=postgres or the equivalent env/config setting"
                        .to_string(),
                ));
            }
            if libsql_replica_overrides_present {
                return Err(Error::InvalidInput(
                    "Replica-connected SQLite config requires --tenant-provider=libsql-replica or the equivalent env/config setting"
                        .to_string(),
                ));
            }
            let mysql_url = mysql_url.ok_or_else(|| {
                Error::InvalidInput(
                    "--mysql-url, NEOVEX_MYSQL_URL, or persistence.mysql_url is required when the tenant provider is mysql"
                        .to_string(),
                )
            })?;
            let mut config = ServicePersistenceConfig::mysql(control_data_dir, mysql_url);
            if let neovex::TenantRoutingConfig::DatabasePerTenant {
                metadata_database,
                tenant_database_prefix,
            } = &mut config.tenant_provider.routing
            {
                if let Some(value) = mysql_metadata_database {
                    *metadata_database = value;
                }
                if let Some(value) = mysql_tenant_database_prefix {
                    *tenant_database_prefix = value;
                }
            }
            config.tenant_provider.pool.min_connections = mysql_min_connections;
            config.tenant_provider.pool.max_connections = mysql_max_connections;
            Ok(config)
        }
    }
}

fn control_data_dir_from_service_config(config: &ServicePersistenceConfig) -> &Path {
    match &config.control_plane {
        neovex::ControlPlaneConfig::EmbeddedRedb { data_dir } => data_dir.as_path(),
    }
}

fn load_runtime_config_file(path: Option<&Path>) -> neovex::Result<RuntimeConfigFile> {
    let Some(path) = path else {
        return Ok(RuntimeConfigFile::default());
    };
    let bytes = std::fs::read(path).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to read config file {}: {error}",
            path.display()
        ))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse config file {} as JSON: {error}",
            path.display()
        ))
    })
}

fn optional_env_tenant_provider(key: &str) -> neovex::Result<Option<CliTenantProvider>> {
    std::env::var(key)
        .ok()
        .map(|value| CliTenantProvider::parse_name(&value))
        .transpose()
}

fn optional_env_usize(key: &str) -> neovex::Result<Option<usize>> {
    std::env::var(key)
        .ok()
        .map(|value| {
            value.parse::<usize>().map_err(|error| {
                Error::InvalidInput(format!(
                    "failed to parse {key} as an unsigned integer: {error}"
                ))
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[cfg(target_os = "linux")]
    use std::env;
    #[cfg(target_os = "linux")]
    use std::time::Instant;

    #[cfg(target_os = "linux")]
    use neovex::{
        RuntimeBundle, SandboxCatalog, build_router_with_convex_and_sandbox_service_manager,
    };
    #[cfg(target_os = "linux")]
    #[cfg(target_os = "linux")]
    use neovex_testing::{
        HttpApiFixture, ServerFixture, ServiceFixture,
        run_to_completion_snapshot_runtime_test_limits, wait_for_condition,
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
            ServicePersistenceConfig::embedded("./data", EmbeddedProviderKind::Sqlite)
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
        let error =
            Cli::try_parse_from(["neovex", "--help"]).expect_err("help should short-circuit");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(
            rendered.contains("Reactive document database with machine and service orchestration")
        );
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
            neovex::TenantProviderConfig::embedded("./tenant-data", EmbeddedProviderKind::Sqlite)
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
        let context = load_compose_project_context(&compose_path, &control_data_dir)
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
        let fixture = ServiceFixture::new(|path| Service::new(path));
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
}
