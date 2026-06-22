use std::path::PathBuf;

use clap::{Args, ValueEnum};

pub(crate) mod adapters;
mod boot;
mod config;
mod first_boot;
mod network_bind;
mod runtime_limits;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use self::boot::resolve_optional_compose_selection;
#[cfg(test)]
pub(crate) use self::boot::resolve_start_app_dir;
pub(crate) use self::boot::run_start_command;
pub(crate) use self::config::persistence_config_from_start_command;
pub(crate) use self::config::{
    CliKeyProvider, CliTenantProvider, RuntimeConfigFile, runtime_config_from_start_command,
};
use self::runtime_limits::{
    default_runtime_control_plane_reserve_millicpus, default_runtime_heap_mb,
    default_runtime_host_millicpus, default_runtime_initial_heap_mb,
    default_runtime_max_active_per_tenant, default_runtime_max_in_flight_per_tenant,
    default_runtime_max_instances, default_runtime_max_nested_calls,
    default_runtime_max_queued_per_tenant, default_runtime_seat_millicpus,
    default_runtime_system_reserve_millicpus, default_runtime_timeout_secs,
    default_runtime_worker_threads,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum CliRuntimeAdaptiveMode {
    Disabled,
    Shadow,
    Canary,
    Live,
}

#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = crate::cli_ux::START_HELP_EXAMPLES
)]
pub(crate) struct StartCommand {
    /// Optional JSON config file. CLI flags override env and file values.
    #[arg(long)]
    pub(crate) config: Option<PathBuf>,

    /// Port to listen on.
    #[arg(long, default_value_t = 8080)]
    pub(crate) port: u16,

    /// Host interface to listen on. Defaults to loopback for local safety.
    #[arg(long, default_value = "127.0.0.1")]
    pub(crate) host: String,

    /// Opt-in to binding on a non-loopback interface. Without this flag,
    /// `nimbus start` refuses any `--host` that resolves outside the
    /// loopback range. With the flag set, the daemon additionally
    /// requires that the local admin token has been explicitly rotated
    /// at least once (`nimbus auth rotate-admin`); a rotation older than
    /// 30 days logs a warning but never blocks startup.
    #[arg(long, default_value_t = false)]
    pub(crate) allow_network: bool,

    /// Inherit a TCP listener from systemd socket activation.
    #[arg(long, default_value_t = false)]
    pub(crate) systemd_socket_activation: bool,

    /// Additional allowed browser origin for CORS (repeatable), e.g.
    /// `https://app.example.com`. Loopback origins are always allowed;
    /// wildcards are not supported. Defaults to NIMBUS_CORS_ALLOW_ORIGINS
    /// (comma-separated) when the flag is absent.
    #[arg(long = "cors-allow-origin", value_name = "ORIGIN", value_parser = parse_cors_origin)]
    pub(crate) cors_allow_origin: Vec<String>,

    /// PEM certificate chain for TLS termination on the main HTTP
    /// listener. Requires --tls-key; HTTPS and wss:// replace plain HTTP.
    #[arg(long, requires = "tls_key", value_name = "CERT_PEM")]
    pub(crate) tls_cert: Option<PathBuf>,

    /// PEM private key for TLS termination. Requires --tls-cert.
    #[arg(long, requires = "tls_cert", value_name = "KEY_PEM")]
    pub(crate) tls_key: Option<PathBuf>,

    /// Disable the Firestore-compatible routes (mounted on the main HTTP
    /// listener by default).
    #[arg(long = "no-firestore", action = clap::ArgAction::SetFalse, default_value_t = true)]
    pub(crate) firestore: bool,

    /// Disable the MongoDB wire-protocol listener (served by default on
    /// port 27017 when free, with SCRAM credentials from the data dir's
    /// wire-credential store unless operator credentials are provided).
    #[arg(long = "no-mongodb", action = clap::ArgAction::SetFalse, default_value_t = true)]
    pub(crate) mongodb: bool,

    /// Explicit port for the MongoDB wire-protocol listener. Without this
    /// flag the listener serves on 27017 when free and is skipped (with a
    /// warning) when busy; an explicit port fails loud instead.
    #[arg(long)]
    pub(crate) mongodb_port: Option<u16>,

    /// Host interface for the MongoDB listener. Non-loopback hosts
    /// require --allow-network.
    #[arg(long, default_value = "127.0.0.1")]
    pub(crate) mongodb_host: String,

    /// SCRAM username for the MongoDB listener. Defaults to
    /// NIMBUS_MONGODB_USERNAME. The password is env-only
    /// (NIMBUS_MONGODB_PASSWORD) so it never appears in process listings.
    /// With neither flag nor env set, the listener uses generated
    /// credentials persisted in the data dir's wire-credential store.
    #[arg(long)]
    pub(crate) mongodb_username: Option<String>,

    /// Disable the DynamoDB HTTP listener (served by default on port 8000
    /// when free, with the generated wire-credential store key bound to
    /// the `default` tenant unless operator bindings are provided).
    #[arg(long = "no-dynamodb", action = clap::ArgAction::SetFalse, default_value_t = true)]
    pub(crate) dynamodb: bool,

    /// Explicit port for the DynamoDB HTTP listener (DynamoDB Local
    /// convention is 8000). Without this flag the listener serves on 8000
    /// when free and is skipped (with a warning) when busy; an explicit
    /// port fails loud instead.
    #[arg(long)]
    pub(crate) dynamodb_port: Option<u16>,

    /// Host interface for the DynamoDB listener. Non-loopback hosts
    /// require --allow-network.
    #[arg(long, default_value = "127.0.0.1")]
    pub(crate) dynamodb_host: String,

    /// DynamoDB access-key binding as ACCESS_KEY_ID:SECRET:TENANT
    /// (repeatable). Defaults to NIMBUS_DYNAMODB_ACCESS_KEYS
    /// (comma-separated). Every request authenticates through these
    /// bindings; with none configured the listener rejects all requests.
    #[arg(long = "dynamodb-access-key", value_name = "KEY_ID:SECRET:TENANT")]
    pub(crate) dynamodb_access_key: Vec<String>,

    /// Local data directory used for embedded tenant databases and, by default,
    /// the local redb control plane.
    #[arg(long)]
    pub(crate) data_dir: Option<PathBuf>,

    /// Optional override for the local redb control-plane directory.
    #[arg(long)]
    pub(crate) control_data_dir: Option<PathBuf>,

    /// Tenant persistence provider mode.
    #[arg(long, value_enum)]
    pub(crate) tenant_provider: Option<CliTenantProvider>,

    /// Canonical libsql primary URL for tenant persistence when
    /// `--tenant-provider=libsql-replica`.
    #[arg(long)]
    pub(crate) libsql_url: Option<String>,

    /// Optional auth token for the libsql primary when
    /// `--tenant-provider=libsql-replica`.
    #[arg(long)]
    pub(crate) libsql_auth_token: Option<String>,

    /// Admin API URL used to provision libsql namespaces when
    /// `--tenant-provider=libsql-replica`.
    #[arg(long)]
    pub(crate) libsql_admin_url: Option<String>,

    /// Optional `Authorization` header value for the libsql admin API when
    /// `--tenant-provider=libsql-replica`.
    #[arg(long)]
    pub(crate) libsql_admin_auth_header: Option<String>,

    /// Provider metadata namespace for replica-connected SQLite tenant routing.
    #[arg(long)]
    pub(crate) libsql_metadata_namespace: Option<String>,

    /// Prefix used when deriving per-tenant libsql namespaces.
    #[arg(long)]
    pub(crate) libsql_tenant_namespace_prefix: Option<String>,

    /// Provider-owned local cache root for embedded replica files when
    /// `--tenant-provider=libsql-replica`.
    #[arg(long)]
    pub(crate) libsql_replica_cache_dir: Option<PathBuf>,

    /// Canonical Postgres resource URL for tenant persistence when
    /// `--tenant-provider=postgres`.
    #[arg(long)]
    pub(crate) postgres_url: Option<String>,

    /// Provider metadata schema for Postgres tenant routing.
    #[arg(long)]
    pub(crate) postgres_metadata_schema: Option<String>,

    /// Prefix used when deriving per-tenant Postgres schema names.
    #[arg(long)]
    pub(crate) postgres_tenant_schema_prefix: Option<String>,

    /// Minimum Postgres pool size.
    #[arg(long)]
    pub(crate) postgres_min_connections: Option<usize>,

    /// Maximum Postgres pool size.
    #[arg(long)]
    pub(crate) postgres_max_connections: Option<usize>,

    /// Canonical MySQL resource URL for tenant persistence when
    /// `--tenant-provider=mysql`.
    #[arg(long)]
    pub(crate) mysql_url: Option<String>,

    /// Provider metadata database for MySQL tenant routing.
    #[arg(long)]
    pub(crate) mysql_metadata_database: Option<String>,

    /// Prefix used when deriving per-tenant MySQL database names.
    #[arg(long)]
    pub(crate) mysql_tenant_database_prefix: Option<String>,

    /// Minimum MySQL pool size.
    #[arg(long)]
    pub(crate) mysql_min_connections: Option<usize>,

    /// Maximum MySQL pool size.
    #[arg(long)]
    pub(crate) mysql_max_connections: Option<usize>,

    /// Optional app directory with generated .nimbus/convex/ runtime artifacts.
    #[arg(long)]
    pub(crate) app_dir: Option<PathBuf>,

    /// Skip automatic codegen before startup. Use when manifests are
    /// pre-built by a separate build step.
    #[arg(long, default_value_t = false)]
    pub(crate) skip_codegen: bool,

    /// Diagnose Node.js builtin imports during automatic codegen preflight.
    #[arg(long, default_value_t = false)]
    pub(crate) debug_node_apis: bool,

    /// Optional ordered Compose file list that declares sandbox-backed services.
    /// Repeat `--compose-file` to merge overlays. When omitted, `nimbus start`
    /// uses COMPOSE_FILE when set and otherwise loads no Compose project. Use
    /// `nimbus dev` or `nimbus compose` for local Compose auto-discovery.
    #[arg(long)]
    pub(crate) compose_file: Vec<PathBuf>,

    /// Optional path to a Nimbus license file. Defaults to ~/.config/nimbus/license.json when present.
    #[arg(long)]
    pub(crate) license_file: Option<PathBuf>,

    /// V8 heap limit per runtime isolate in megabytes.
    #[arg(long, default_value_t = default_runtime_heap_mb())]
    pub(crate) runtime_heap_mb: usize,

    /// Initial V8 heap size per runtime isolate in megabytes.
    #[arg(long, default_value_t = default_runtime_initial_heap_mb())]
    pub(crate) runtime_initial_heap_mb: usize,

    /// Maximum wall-clock execution time for a runtime invocation in seconds.
    #[arg(long, default_value_t = default_runtime_timeout_secs())]
    pub(crate) runtime_timeout_secs: u64,

    /// Maximum number of concurrent top-level runtime instances.
    #[arg(long, default_value_t = default_runtime_max_instances())]
    pub(crate) runtime_max_instances: usize,

    /// Maximum active top-level runtime invocations per tenant.
    #[arg(long, default_value_t = default_runtime_max_active_per_tenant())]
    pub(crate) runtime_max_active_per_tenant: usize,

    /// Maximum active plus parked top-level runtime invocations per tenant.
    #[arg(long, default_value_t = default_runtime_max_in_flight_per_tenant())]
    pub(crate) runtime_max_in_flight_per_tenant: usize,

    /// Maximum queued top-level runtime invocations per tenant.
    #[arg(long, default_value_t = default_runtime_max_queued_per_tenant())]
    pub(crate) runtime_max_queued_per_tenant: usize,

    /// Number of runtime worker threads.
    #[arg(long, default_value_t = default_runtime_worker_threads())]
    pub(crate) runtime_worker_threads: usize,

    /// Maximum number of nested runtime ctx.run* invocations allowed per request tree.
    #[arg(long, default_value_t = default_runtime_max_nested_calls())]
    pub(crate) runtime_max_nested_calls: usize,

    /// Host CPU capacity available to the isolate runtime governor, in millicpus.
    #[arg(long = "runtime-host-millicpus", default_value_t = default_runtime_host_millicpus(), value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) runtime_host_millicpus: u32,

    /// CPU reserve kept outside isolate execution for the host OS, in millicpus.
    #[arg(long = "runtime-system-reserve-millicpus", default_value_t = default_runtime_system_reserve_millicpus())]
    pub(crate) runtime_system_reserve_millicpus: u32,

    /// CPU reserve kept for Nimbus control-plane work, in millicpus.
    #[arg(long = "runtime-control-plane-reserve-millicpus", default_value_t = default_runtime_control_plane_reserve_millicpus())]
    pub(crate) runtime_control_plane_reserve_millicpus: u32,

    /// Optional hard CPU ceiling for aggregate isolate execution, in millicpus.
    #[arg(long = "runtime-hard-ceiling-millicpus")]
    pub(crate) runtime_hard_ceiling_millicpus: Option<u32>,

    /// CPU represented by one nominal isolate dispatch seat, in millicpus.
    #[arg(long = "runtime-seat-millicpus", default_value_t = default_runtime_seat_millicpus(), value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) runtime_seat_millicpus: u32,

    /// Operator-only live adaptive warm-pool controller mode.
    #[arg(long = "runtime-adaptive-mode", value_enum, default_value_t = CliRuntimeAdaptiveMode::Disabled)]
    pub(crate) runtime_adaptive_mode: CliRuntimeAdaptiveMode,

    /// Percentage of authority keys admitted to adaptive actuation in canary mode.
    #[arg(long = "runtime-adaptive-canary-percent", default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=100))]
    pub(crate) runtime_adaptive_canary_percent: u8,

    /// Force live adaptive warm-pool control back to static measured defaults.
    #[arg(long = "runtime-adaptive-rollback", default_value_t = false)]
    pub(crate) runtime_adaptive_rollback: bool,

    // -------------------------------------------------------------------------
    // Local encryption options
    // -------------------------------------------------------------------------
    /// Local encryption key provider. One of: master-key-file, key-dir, aws-kms.
    ///
    /// `aws-kms` uses the same manifest-backed per-subject DEK contract as the
    /// local providers, but wraps those DEKs with AWS KMS `GenerateDataKey`,
    /// `Decrypt`, and `ReEncrypt`. If this flag is not specified, local
    /// encryption is disabled.
    #[arg(long, value_enum)]
    pub(crate) encryption_key_provider: Option<CliKeyProvider>,

    /// Path to the master key file when `--encryption-key-provider=master-key-file`.
    /// The file must contain exactly 32 bytes of key material.
    #[arg(long)]
    pub(crate) encryption_master_key_file: Option<PathBuf>,

    /// Path to the key directory when `--encryption-key-provider=key-dir`.
    #[arg(long)]
    pub(crate) encryption_key_dir: Option<PathBuf>,

    /// AWS KMS key ID (ARN or alias) when `--encryption-key-provider=aws-kms`.
    #[arg(long)]
    pub(crate) encryption_aws_kms_key_id: Option<String>,

    /// AWS region override when `--encryption-key-provider=aws-kms`.
    #[arg(long)]
    pub(crate) encryption_aws_region: Option<String>,

    /// AWS KMS endpoint URL override for testing or VPC endpoints.
    #[arg(long)]
    pub(crate) encryption_aws_endpoint_url: Option<String>,

    /// Internal bearer token used by development workflows to activate local app generations.
    #[arg(skip)]
    pub(crate) deploy_admin_token: Option<String>,

    /// Tenant to create automatically on startup (used by `nimbus dev`).
    #[arg(skip)]
    pub(crate) auto_tenant: Option<String>,

    /// Resolve MongoDB listener credentials from the shared wire-credential
    /// store only, ignoring operator flags/env. `nimbus dev` sets this: its
    /// `.env.local` advertises the store credentials, so ambient
    /// NIMBUS_MONGODB_* values in the developer's shell must not desync the
    /// listener from what the app's env file carries.
    #[arg(skip)]
    pub(crate) mongodb_credentials_from_store: bool,

    /// Tenant-isolation mode selected by the owning command.
    #[arg(skip = nimbus_server::TenantIsolationMode::Production)]
    pub(crate) tenant_isolation_mode: nimbus_server::TenantIsolationMode,
}

/// clap value parser for `--cors-allow-origin`: normalize-or-reject at
/// parse time so a bad origin fails the command instead of being silently
/// ignored at the CORS layer.
fn parse_cors_origin(value: &str) -> Result<String, String> {
    nimbus_server::normalize_cors_origin(value)
}

impl Default for StartCommand {
    fn default() -> Self {
        Self {
            config: None,
            port: 8080,
            host: "127.0.0.1".to_string(),
            allow_network: false,
            systemd_socket_activation: false,
            cors_allow_origin: Vec::new(),
            tls_cert: None,
            tls_key: None,
            firestore: true,
            mongodb: true,
            mongodb_port: None,
            mongodb_host: "127.0.0.1".to_string(),
            mongodb_username: None,
            dynamodb: true,
            dynamodb_port: None,
            dynamodb_host: "127.0.0.1".to_string(),
            dynamodb_access_key: Vec::new(),
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
            app_dir: None,
            skip_codegen: false,
            debug_node_apis: false,
            compose_file: Vec::new(),
            license_file: None,
            runtime_heap_mb: default_runtime_heap_mb(),
            runtime_initial_heap_mb: default_runtime_initial_heap_mb(),
            runtime_timeout_secs: default_runtime_timeout_secs(),
            runtime_max_instances: default_runtime_max_instances(),
            runtime_max_active_per_tenant: default_runtime_max_active_per_tenant(),
            runtime_max_in_flight_per_tenant: default_runtime_max_in_flight_per_tenant(),
            runtime_max_queued_per_tenant: default_runtime_max_queued_per_tenant(),
            runtime_worker_threads: default_runtime_worker_threads(),
            runtime_max_nested_calls: default_runtime_max_nested_calls(),
            runtime_host_millicpus: default_runtime_host_millicpus(),
            runtime_system_reserve_millicpus: default_runtime_system_reserve_millicpus(),
            runtime_control_plane_reserve_millicpus:
                default_runtime_control_plane_reserve_millicpus(),
            runtime_hard_ceiling_millicpus: None,
            runtime_seat_millicpus: default_runtime_seat_millicpus(),
            runtime_adaptive_mode: CliRuntimeAdaptiveMode::Disabled,
            runtime_adaptive_canary_percent: 0,
            runtime_adaptive_rollback: false,
            encryption_key_provider: None,
            encryption_master_key_file: None,
            encryption_key_dir: None,
            encryption_aws_kms_key_id: None,
            encryption_aws_region: None,
            encryption_aws_endpoint_url: None,
            deploy_admin_token: None,
            auto_tenant: None,
            mongodb_credentials_from_store: false,
            tenant_isolation_mode: nimbus_server::TenantIsolationMode::Production,
        }
    }
}
