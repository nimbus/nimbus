use std::path::PathBuf;

use clap::Args;

mod boot;
mod config;
mod runtime_limits;
#[cfg(test)]
mod tests;

pub(crate) use self::boot::run_start_command;
pub(crate) use self::config::persistence_config_from_start_command;
pub(crate) use self::config::{CliKeyProvider, CliTenantProvider};
use self::runtime_limits::{
    default_runtime_heap_mb, default_runtime_initial_heap_mb, default_runtime_max_instances,
    default_runtime_max_nested_calls, default_runtime_timeout_secs, default_runtime_worker_threads,
};

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

    /// Optional app directory with generated .neovex/convex/ runtime artifacts.
    #[arg(long)]
    pub(crate) app_dir: Option<PathBuf>,

    /// Skip automatic codegen before startup. Use when manifests are
    /// pre-built by a separate build step.
    #[arg(long, default_value_t = false)]
    pub(crate) skip_codegen: bool,

    /// Optional Compose file that declares sandbox-backed services for
    /// `ctx.services.*` activation.
    #[arg(long)]
    pub(crate) compose_file: Option<PathBuf>,

    /// Optional path to a Neovex license file. Defaults to ./.neovex/license.json when present.
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

    /// Number of runtime worker threads.
    #[arg(long, default_value_t = default_runtime_worker_threads())]
    pub(crate) runtime_worker_threads: usize,

    /// Maximum number of nested runtime ctx.run* invocations allowed per request tree.
    #[arg(long, default_value_t = default_runtime_max_nested_calls())]
    pub(crate) runtime_max_nested_calls: usize,

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
}

impl Default for StartCommand {
    fn default() -> Self {
        Self {
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
            app_dir: None,
            skip_codegen: false,
            compose_file: None,
            license_file: None,
            runtime_heap_mb: default_runtime_heap_mb(),
            runtime_initial_heap_mb: default_runtime_initial_heap_mb(),
            runtime_timeout_secs: default_runtime_timeout_secs(),
            runtime_max_instances: default_runtime_max_instances(),
            runtime_worker_threads: default_runtime_worker_threads(),
            runtime_max_nested_calls: default_runtime_max_nested_calls(),
            encryption_key_provider: None,
            encryption_master_key_file: None,
            encryption_key_dir: None,
            encryption_aws_kms_key_id: None,
            encryption_aws_region: None,
            encryption_aws_endpoint_url: None,
            deploy_admin_token: None,
        }
    }
}
