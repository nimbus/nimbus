use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Context, Poll};

use hyper::client::HttpConnector;
use hyper::client::connect::{Connected, Connection as HyperConnection};
use libsql::{Builder, Connection, Database, Transaction, TransactionBehavior};
use native_tls::TlsConnector as NativeTlsConnector;
use neovex_core::{
    CommitEntry, CronJob, Document, DocumentId, DurableMutationRecord, Error, Result, ScheduledJob,
    ScheduledJobResult, Schema, SequenceNumber, StorageErrorKind, TableName, TableSchema, TenantId,
    Timestamp, WriteOp, WriteOpType,
};
use reqwest::Client as HttpClient;
use reqwest::header::AUTHORIZATION;
use rusqlite::{Connection as LocalSqliteConnection, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::runtime::Handle as TokioRuntimeHandle;
use tokio::sync::{Notify, Semaphore};
use tokio_native_tls::{TlsConnector as TokioTlsConnector, TlsStream};
use tower_service::Service;

use crate::async_storage::{TenantReadStorage, TenantWriteOutcome, TenantWriteStorage};
use crate::commit_log::{deserialize_durable_record, serialize_commit, serialize_durable_record};
use crate::simulation::{Clock, FaultInjector, NoopFaultInjector, SystemClock};
use crate::sqlite::{
    SQLITE_INIT_SQL, SqliteReadSnapshot, SqliteTenantStore,
    rebuild_sqlite_indexes_from_loaded_schema,
};
use crate::store::{
    APPLIED_SEQUENCE_KEY, DurableJournalBootstrap, DurableJournalPage, JournalProgress,
    MAX_DURABLE_JOURNAL_STREAM_LIMIT, NEXT_SEQUENCE_KEY, ResolvedScheduleOp, ResolvedWrite,
    TenantWriteCommit,
};

const LIBSQL_NAMESPACE_LIMIT: usize = 63;
const TARGET_TENANT_HASH_HEX_LEN: usize = 40;
const MIN_TENANT_HASH_HEX_LEN: usize = 16;
const LIBSQL_TENANT_READ_PARALLELISM: usize = 4;
const LIBSQL_TENANT_WRITE_PARALLELISM: usize = 1;
const LIBSQL_REPLICA_FILENAME: &str = "tenant.sqlite3";
const LIBSQL_DROP_TENANT_SQL: &str = r#"
DROP TABLE IF EXISTS documents;
DROP TABLE IF EXISTS schemas;
DROP TABLE IF EXISTS scheduled_jobs;
DROP TABLE IF EXISTS running_scheduled_jobs;
DROP TABLE IF EXISTS scheduled_job_results;
DROP TABLE IF EXISTS scheduled_job_executions;
DROP TABLE IF EXISTS cron_jobs;
DROP TABLE IF EXISTS commit_log;
DROP TABLE IF EXISTS metadata;
"#;

type LibsqlTransportError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[doc(hidden)]
#[derive(Clone)]
pub struct LibsqlTransportConnector {
    http: HttpConnector,
    tls: TokioTlsConnector,
}

#[doc(hidden)]
pub enum LibsqlTransportStream {
    Http(TcpStream),
    Https(TlsStream<TcpStream>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibsqlReplicaProviderConfig {
    pub primary_url: String,
    pub auth_token: Option<String>,
    pub admin_api_url: String,
    pub admin_auth_header: Option<String>,
    pub metadata_namespace: String,
    pub tenant_namespace_prefix: String,
    pub replica_cache_dir: PathBuf,
}

impl LibsqlReplicaProviderConfig {
    pub fn new(
        primary_url: impl Into<String>,
        admin_api_url: impl Into<String>,
        replica_cache_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            primary_url: primary_url.into(),
            auth_token: None,
            admin_api_url: admin_api_url.into(),
            admin_auth_header: None,
            metadata_namespace: "neovex_provider".to_string(),
            tenant_namespace_prefix: "tenant_".to_string(),
            replica_cache_dir: replica_cache_dir.into(),
        }
    }
}

impl LibsqlTransportConnector {
    fn new() -> Result<Self> {
        let mut http = HttpConnector::new();
        http.enforce_http(false);
        http.set_nodelay(true);
        let tls = NativeTlsConnector::builder()
            .build()
            .map(TokioTlsConnector::from)
            .map_err(|error| {
                Error::storage(
                    StorageErrorKind::Other,
                    format!("failed to build libsql TLS connector: {error}"),
                )
            })?;
        Ok(Self { http, tls })
    }
}

impl Service<hyper::http::Uri> for LibsqlTransportConnector {
    type Response = LibsqlTransportStream;
    type Error = LibsqlTransportError;
    type Future =
        Pin<Box<dyn Future<Output = std::result::Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        self.http.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, uri: hyper::http::Uri) -> Self::Future {
        let mut http = self.http.clone();
        let tls = self.tls.clone();
        Box::pin(async move {
            let scheme = uri.scheme_str().unwrap_or("https");
            let stream = http.call(uri.clone()).await?;
            if scheme.eq_ignore_ascii_case("http") {
                return Ok(LibsqlTransportStream::Http(stream));
            }
            if !scheme.eq_ignore_ascii_case("https") {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unsupported libsql URI scheme '{scheme}'"),
                )
                .into());
            }
            let host = uri.host().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "libsql URI is missing a host")
            })?;
            let tls_stream = tls.connect(host, stream).await?;
            Ok(LibsqlTransportStream::Https(tls_stream))
        })
    }
}

impl HyperConnection for LibsqlTransportStream {
    fn connected(&self) -> Connected {
        match self {
            Self::Http(stream) => stream.connected(),
            Self::Https(stream) => stream.get_ref().get_ref().get_ref().connected(),
        }
    }
}

impl AsyncRead for LibsqlTransportStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Http(stream) => Pin::new(stream).poll_read(cx, buf),
            Self::Https(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for LibsqlTransportStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Http(stream) => Pin::new(stream).poll_write(cx, buf),
            Self::Https(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Http(stream) => Pin::new(stream).poll_flush(cx),
            Self::Https(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Http(stream) => Pin::new(stream).poll_shutdown(cx),
            Self::Https(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

#[doc(hidden)]
pub fn libsql_transport_connector() -> Result<LibsqlTransportConnector> {
    LibsqlTransportConnector::new()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibsqlReplicaTenantRegistration {
    pub tenant_id: TenantId,
    pub namespace: String,
}

#[derive(Clone)]
pub struct LibsqlReplicaProvider {
    primary_url: String,
    auth_token: Option<String>,
    admin_api_url: String,
    admin_auth_header: Option<String>,
    metadata_namespace: String,
    tenant_namespace_prefix: String,
    replica_cache_dir: PathBuf,
    runtime_handle: TokioRuntimeHandle,
    clock: Arc<dyn Clock>,
    fault_injector: Arc<dyn FaultInjector>,
    tenant_read_parallelism: usize,
    metadata_database: Arc<Database>,
}

pub struct OpenedLibsqlReplicaTenant {
    pub store: Arc<LibsqlReplicaTenantStore>,
    pub read_storage: Arc<LibsqlReplicaTenantStorage>,
    tenant_id: TenantId,
    namespace: String,
    replica_path: PathBuf,
    primary_url: String,
}

#[derive(Clone)]
pub struct LibsqlReplicaTenantStore {
    provider: LibsqlReplicaProvider,
    tenant_id: TenantId,
    namespace: String,
    remote_database: Arc<Database>,
    active_cache: Arc<RwLock<ReplicaCacheHandle>>,
    retired_caches: Arc<Mutex<Vec<ReplicaCacheHandle>>>,
    next_cache_generation: Arc<AtomicU64>,
    refresh_needed: Arc<AtomicBool>,
    refresh_requested: Arc<AtomicBool>,
    refresh_inflight: Arc<AtomicBool>,
    refresh_complete: Arc<Notify>,
    required_cache_sequence: Arc<AtomicU64>,
}

#[derive(Clone)]
struct ReplicaCacheHandle {
    store: Arc<SqliteTenantStore>,
    replica_path: PathBuf,
}

#[derive(Clone)]
pub struct LibsqlReplicaTenantStorage {
    store: Arc<LibsqlReplicaTenantStore>,
    permits: Arc<Semaphore>,
    runtime_handle: TokioRuntimeHandle,
    write_executor: LibsqlReplicaBlockingWriteExecutor,
}

pub struct LibsqlReplicaWriteTransaction {
    store: LibsqlReplicaTenantStore,
    tx: Option<Transaction>,
    commit_writes: Vec<WriteOp>,
    check_cancel: Box<dyn Fn() -> Result<()> + Send>,
    refresh_cache_after_commit: bool,
}

#[derive(Clone)]
struct LibsqlReplicaBlockingWriteExecutor {
    store: Arc<LibsqlReplicaTenantStore>,
    permits: Arc<Semaphore>,
    runtime_handle: TokioRuntimeHandle,
}

impl LibsqlReplicaProvider {
    pub async fn connect(config: LibsqlReplicaProviderConfig) -> Result<Self> {
        Self::connect_with_simulation(
            config,
            TokioRuntimeHandle::current(),
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
        )
        .await
    }

    pub async fn connect_with_simulation(
        config: LibsqlReplicaProviderConfig,
        runtime_handle: TokioRuntimeHandle,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        validate_namespace_input(&config.metadata_namespace, "metadata namespace")?;
        validate_namespace_input(&config.tenant_namespace_prefix, "tenant namespace prefix")?;
        if config.admin_api_url.trim().is_empty() {
            return Err(Error::InvalidInput(
                "libsql admin API URL cannot be empty".to_string(),
            ));
        }
        std::fs::create_dir_all(&config.replica_cache_dir).map_err(storage_io_error)?;
        ensure_remote_namespace_exists(
            &config.admin_api_url,
            config.admin_auth_header.as_deref(),
            &config.metadata_namespace,
        )
        .await?;

        let metadata_database = Arc::new(
            open_remote_database(
                &config.primary_url,
                config.auth_token.as_deref(),
                &config.metadata_namespace,
            )
            .await?,
        );
        let provider = Self {
            primary_url: config.primary_url,
            auth_token: config.auth_token,
            admin_api_url: config.admin_api_url,
            admin_auth_header: config.admin_auth_header,
            metadata_namespace: config.metadata_namespace,
            tenant_namespace_prefix: config.tenant_namespace_prefix,
            replica_cache_dir: config.replica_cache_dir,
            runtime_handle,
            clock,
            fault_injector,
            tenant_read_parallelism: LIBSQL_TENANT_READ_PARALLELISM,
            metadata_database,
        };
        provider.ensure_metadata_namespace().await?;
        Ok(provider)
    }

    pub fn metadata_namespace(&self) -> &str {
        &self.metadata_namespace
    }

    pub fn tenant_namespace(&self, tenant_id: &TenantId) -> Result<String> {
        tenant_namespace_name(&self.tenant_namespace_prefix, tenant_id)
    }

    pub fn replica_cache_root(&self) -> &Path {
        &self.replica_cache_dir
    }

    pub fn replica_path_for_tenant(&self, tenant_id: &TenantId) -> PathBuf {
        self.replica_cache_dir
            .join(tenant_id.as_str())
            .join(LIBSQL_REPLICA_FILENAME)
    }

    pub fn read_storage_for_store(
        &self,
        store: Arc<LibsqlReplicaTenantStore>,
    ) -> Arc<LibsqlReplicaTenantStorage> {
        Arc::new(LibsqlReplicaTenantStorage::with_max_concurrent_reads(
            store,
            self.runtime_handle.clone(),
            self.tenant_read_parallelism,
        ))
    }

    pub async fn create_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<OpenedLibsqlReplicaTenant> {
        let registration = self.create_tenant(tenant_id).await?;
        self.open_registration(registration).await
    }

    pub async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedLibsqlReplicaTenant>> {
        let Some(registration) = self.open_existing_tenant(tenant_id).await? else {
            return Ok(None);
        };
        self.open_registration(registration).await.map(Some)
    }

    pub async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        let conn = self.metadata_connection()?;
        let mut rows = conn
            .query("SELECT tenant_id FROM tenants ORDER BY tenant_id", ())
            .await
            .map_err(map_libsql_error)?;
        let mut tenants = Vec::new();
        while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
            let tenant_id = row.get::<String>(0).map_err(map_libsql_error)?;
            tenants.push(TenantId::new(tenant_id)?);
        }
        Ok(tenants)
    }

    pub async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        let conn = self.metadata_connection()?;
        let mut rows = conn
            .query(
                "SELECT namespace FROM tenants WHERE tenant_id = ?",
                libsql::params![tenant_id.as_str()],
            )
            .await
            .map_err(map_libsql_error)?;
        Ok(rows.next().await.map_err(map_libsql_error)?.is_some())
    }

    pub async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<LibsqlReplicaTenantRegistration>> {
        let conn = self.metadata_connection()?;
        let mut rows = conn
            .query(
                "SELECT namespace FROM tenants WHERE tenant_id = ?",
                libsql::params![tenant_id.as_str()],
            )
            .await
            .map_err(map_libsql_error)?;
        let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
            return Ok(None);
        };
        let namespace = row.get::<String>(0).map_err(map_libsql_error)?;
        if !tenant_namespace_has_foundation(
            &self.primary_url,
            self.auth_token.as_deref(),
            &namespace,
        )
        .await?
        {
            return Err(Error::Internal(format!(
                "tenant registry points at missing libsql namespace '{namespace}'"
            )));
        }
        Ok(Some(LibsqlReplicaTenantRegistration {
            tenant_id: tenant_id.clone(),
            namespace,
        }))
    }

    pub async fn create_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<LibsqlReplicaTenantRegistration> {
        if self.tenant_exists(tenant_id).await? {
            return Err(Error::AlreadyExists(format!(
                "tenant '{}' already exists",
                tenant_id.as_str()
            )));
        }
        let namespace = self.tenant_namespace(tenant_id)?;
        ensure_remote_namespace_exists(
            &self.admin_api_url,
            self.admin_auth_header.as_deref(),
            &namespace,
        )
        .await?;
        bootstrap_tenant_namespace(&self.primary_url, self.auth_token.as_deref(), &namespace)
            .await?;
        let conn = self.metadata_connection()?;
        conn.execute(
            "INSERT INTO tenants (tenant_id, namespace) VALUES (?, ?)",
            libsql::params![tenant_id.as_str(), namespace.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
        Ok(LibsqlReplicaTenantRegistration {
            tenant_id: tenant_id.clone(),
            namespace,
        })
    }

    pub async fn refresh_tenant_snapshot(&self, tenant_id: &TenantId) -> Result<PathBuf> {
        let Some(registration) = self.open_existing_tenant(tenant_id).await? else {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        };
        self.sync_registration_snapshot(&registration).await
    }

    pub async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        let Some(registration) = self.open_existing_tenant(tenant_id).await? else {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        };
        clear_tenant_namespace(
            &self.primary_url,
            self.auth_token.as_deref(),
            &registration.namespace,
        )
        .await?;
        drop_remote_namespace(
            &self.admin_api_url,
            self.admin_auth_header.as_deref(),
            &registration.namespace,
        )
        .await?;
        let conn = self.metadata_connection()?;
        conn.execute(
            "DELETE FROM tenants WHERE tenant_id = ?",
            libsql::params![tenant_id.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
        let replica_dir = self.replica_dir_for_tenant(tenant_id);
        if replica_dir.exists() {
            std::fs::remove_dir_all(&replica_dir).map_err(storage_io_error)?;
        }
        Ok(())
    }

    async fn sync_registration_snapshot(
        &self,
        registration: &LibsqlReplicaTenantRegistration,
    ) -> Result<PathBuf> {
        let snapshot = fetch_remote_namespace_snapshot(
            &self.primary_url,
            self.auth_token.as_deref(),
            &registration.namespace,
        )
        .await?;
        let replica_path = self.replica_path_for_tenant(&registration.tenant_id);
        let path_for_publish = replica_path.clone();
        let replica_dir = self.replica_dir_for_tenant(&registration.tenant_id);
        self.runtime_handle
            .spawn_blocking(move || {
                materialize_snapshot_to_replica_cache(
                    replica_dir.as_path(),
                    path_for_publish.as_path(),
                    snapshot,
                )
            })
            .await
            .map_err(map_join_error)??;
        Ok(replica_path)
    }

    pub async fn drop_provider_namespaces_for_test(&self) -> Result<()> {
        let tenants = self.list_tenants().await?;
        for tenant_id in tenants {
            self.delete_tenant(&tenant_id).await?;
        }
        let conn = self.metadata_connection()?;
        conn.execute_batch("DROP TABLE IF EXISTS tenants")
            .await
            .map_err(map_libsql_error)?;
        let _ = drop_remote_namespace(
            &self.admin_api_url,
            self.admin_auth_header.as_deref(),
            &self.metadata_namespace,
        )
        .await;
        Ok(())
    }

    async fn open_registration(
        &self,
        registration: LibsqlReplicaTenantRegistration,
    ) -> Result<OpenedLibsqlReplicaTenant> {
        let replica_path = self.sync_registration_snapshot(&registration).await?;
        let remote_database = Arc::new(
            open_remote_database(
                &self.primary_url,
                self.auth_token.as_deref(),
                &registration.namespace,
            )
            .await?,
        );
        let clock = self.clock.clone();
        let fault_injector = self.fault_injector.clone();
        let path_for_open = replica_path.clone();
        let read_parallelism = self.tenant_read_parallelism;
        let local_store = self
            .runtime_handle
            .spawn_blocking(move || {
                SqliteTenantStore::open_with_simulation_and_max_read_connections(
                    path_for_open,
                    clock,
                    fault_injector,
                    read_parallelism,
                )
            })
            .await
            .map_err(map_join_error)??;
        let store = Arc::new(LibsqlReplicaTenantStore::new(
            self.clone(),
            registration.tenant_id.clone(),
            registration.namespace.clone(),
            remote_database,
            Arc::new(local_store),
            replica_path.clone(),
        ));
        let read_storage = self.read_storage_for_store(store.clone());
        Ok(OpenedLibsqlReplicaTenant {
            store,
            read_storage,
            tenant_id: registration.tenant_id,
            namespace: registration.namespace,
            replica_path,
            primary_url: self.primary_url.clone(),
        })
    }

    async fn ensure_metadata_namespace(&self) -> Result<()> {
        let conn = self.metadata_connection()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tenants (
                tenant_id TEXT NOT NULL PRIMARY KEY,
                namespace TEXT NOT NULL
            );",
        )
        .await
        .map_err(map_libsql_error)?;
        Ok(())
    }

    fn metadata_connection(&self) -> Result<Connection> {
        self.metadata_database.connect().map_err(map_libsql_error)
    }

    fn replica_dir_for_tenant(&self, tenant_id: &TenantId) -> PathBuf {
        self.replica_cache_dir.join(tenant_id.as_str())
    }
}

impl OpenedLibsqlReplicaTenant {
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn replica_path(&self) -> &Path {
        &self.replica_path
    }

    pub fn primary_url(&self) -> &str {
        &self.primary_url
    }
}

impl LibsqlReplicaTenantStore {
    fn new(
        provider: LibsqlReplicaProvider,
        tenant_id: TenantId,
        namespace: String,
        remote_database: Arc<Database>,
        local_store: Arc<SqliteTenantStore>,
        replica_path: PathBuf,
    ) -> Self {
        let initial_applied = local_store
            .applied_sequence()
            .map(|sequence| sequence.0)
            .unwrap_or(0);
        Self {
            provider,
            tenant_id,
            namespace,
            remote_database,
            active_cache: Arc::new(RwLock::new(ReplicaCacheHandle {
                store: local_store,
                replica_path,
            })),
            retired_caches: Arc::new(Mutex::new(Vec::new())),
            next_cache_generation: Arc::new(AtomicU64::new(1)),
            refresh_needed: Arc::new(AtomicBool::new(false)),
            refresh_requested: Arc::new(AtomicBool::new(false)),
            refresh_inflight: Arc::new(AtomicBool::new(false)),
            refresh_complete: Arc::new(Notify::new()),
            required_cache_sequence: Arc::new(AtomicU64::new(initial_applied)),
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn replica_path(&self) -> Result<PathBuf> {
        Ok(self.active_cache_handle()?.replica_path.clone())
    }

    pub fn primary_url(&self) -> &str {
        &self.provider.primary_url
    }

    pub fn check_fault(&self, point: crate::FaultPoint) -> Result<()> {
        self.provider.fault_injector.check(point)
    }

    pub fn read_snapshot(&self) -> Result<SqliteReadSnapshot> {
        let store = self.current_query_cache_store()?;
        store.read_snapshot()
    }

    pub fn load_schema(&self) -> Result<Schema> {
        let remote_schema = self.block_on(self.load_remote_schema())?;
        let local_schema = self.active_cache_store()?.load_schema()?;
        if local_schema != remote_schema {
            self.refresh_needed.store(true, Ordering::Release);
            self.schedule_background_refresh();
        }
        Ok(remote_schema)
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        self.block_on(self.load_remote_latest_sequence())
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        self.active_cache_store()?.applied_sequence()
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        Ok(JournalProgress {
            durable_head: self.latest_sequence()?,
            applied_head: self.applied_sequence()?,
        })
    }

    pub fn recover_durable_journal(&self) -> Result<JournalProgress> {
        let progress = self.journal_progress()?;
        if progress.applied_head.0 < progress.durable_head.0 {
            let next_sequence = SequenceNumber(progress.applied_head.0.saturating_add(1));
            let records = self.read_durable_journal_from(next_sequence)?;
            if !records.is_empty() {
                let applied_head =
                    self.block_on(self.apply_remote_durable_records_batch(records.as_slice()))?;
                self.note_required_cache_sequence(applied_head);
            } else {
                self.note_required_cache_sequence(progress.durable_head);
            }
        }
        if self.refresh_needed.load(Ordering::Acquire)
            || self.active_cache_store()?.applied_sequence()?.0
                < self.required_cache_sequence.load(Ordering::Acquire)
        {
            self.schedule_background_refresh();
            return self.refresh_local_cache();
        }
        self.journal_progress()
    }

    pub fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>> {
        Ok(self
            .read_durable_journal_from(sequence)?
            .into_iter()
            .map(|record| record.as_commit_entry())
            .collect())
    }

    pub fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        self.block_on(self.load_remote_durable_records_from(sequence))
    }

    pub fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        validate_durable_journal_stream_limit(limit)?;
        self.block_on(self.load_remote_durable_journal_page(after, limit))
    }

    pub fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        self.refresh_local_cache()?;
        self.active_cache_store()?
            .export_durable_journal_bootstrap()
    }

    pub fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
        let execution_id = execution_id.to_string();
        self.block_on(async move {
            let conn = self.remote_connection()?;
            let mut rows = conn
                .query(
                    "SELECT 1 FROM scheduled_job_executions WHERE execution_id = ?1",
                    libsql::params![execution_id],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(rows.next().await.map_err(map_libsql_error)?.is_some())
        })
    }

    pub fn get_scheduled_job_result(
        &self,
        job_id: &DocumentId,
    ) -> Result<Option<ScheduledJobResult>> {
        let job_id = job_id.to_string();
        self.block_on(async move {
            let conn = self.remote_connection()?;
            let mut rows = conn
                .query(
                    "SELECT data_json FROM scheduled_job_results WHERE job_id = ?1",
                    libsql::params![job_id],
                )
                .await
                .map_err(map_libsql_error)?;
            let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
                return Ok(None);
            };
            let json = row.get::<String>(0).map_err(map_libsql_error)?;
            Ok(Some(deserialize_json(json.as_str())?))
        })
    }

    pub fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        self.block_on(self.load_remote_scheduled_jobs("scheduled_jobs"))
    }

    pub fn load_cron_jobs(&self) -> Result<Vec<CronJob>> {
        self.block_on(self.load_remote_cron_jobs())
    }

    pub fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>> {
        let next_job_at = self.list_scheduled_jobs()?.first().map(|job| job.run_at);
        let next_cron_at = self
            .load_cron_jobs()?
            .into_iter()
            .filter(|cron| cron.enabled)
            .map(|cron| cron.next_run)
            .min();
        Ok(match (next_job_at, next_cron_at) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        })
    }

    pub fn has_scheduled_work(&self) -> Result<bool> {
        self.block_on(async move {
            let conn = self.remote_connection()?;
            Ok(table_has_entries_remote(&conn, "scheduled_jobs").await?
                || table_has_entries_remote(&conn, "running_scheduled_jobs").await?
                || table_has_entries_remote(&conn, "cron_jobs").await?)
        })
    }

    pub fn now(&self) -> Timestamp {
        self.provider.clock.now()
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        self.current_query_cache_store()?.get(table, id)
    }

    pub fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.current_query_cache_store()?
            .scan_table_matching_cancellable(table, check_cancel, include_document)
    }

    pub fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[neovex_core::Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.current_query_cache_store()?
            .scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            )
    }

    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.current_query_cache_store()?.index_scan_eq_cancellable(
            table,
            index_name,
            value,
            check_cancel,
        )
    }

    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.current_query_cache_store()?
            .index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.current_query_cache_store()?
            .index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.current_query_cache_store()?
            .index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            )
    }

    pub fn replace_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        let table_schema = table_schema.clone();
        self.execute_write(move |transaction| transaction.replace_table_schema(&table_schema))?;
        Ok(())
    }

    pub fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        let table = table.clone();
        self.execute_write(move |transaction| transaction.delete_table_schema(&table))?;
        Ok(())
    }

    pub fn append_durable_records_batch(&self, records: &[DurableMutationRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let records = records.to_vec();
        self.block_on(self.append_remote_durable_records_batch(records.as_slice()))?;
        Ok(())
    }

    pub fn apply_durable_records_batch(&self, records: &[DurableMutationRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let records = records.to_vec();
        let applied_head =
            self.block_on(self.apply_remote_durable_records_batch(records.as_slice()))?;
        self.note_required_cache_sequence(applied_head);
        Ok(())
    }

    pub fn insert_scheduled_job(&self, job: &ScheduledJob) -> Result<()> {
        let job = job.clone();
        self.execute_write(move |transaction| transaction.insert_scheduled_job(&job))?;
        Ok(())
    }

    pub fn claim_due_jobs(&self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        Ok(self
            .execute_write(move |transaction| transaction.claim_due_jobs(now))?
            .value)
    }

    pub fn complete_scheduled_job(&self, job_id: &DocumentId) -> Result<()> {
        let job_id = *job_id;
        self.execute_write(move |transaction| transaction.complete_scheduled_job(&job_id))?;
        Ok(())
    }

    pub fn cancel_scheduled_job(&self, job_id: &DocumentId) -> Result<bool> {
        let job_id = *job_id;
        Ok(self
            .execute_write(move |transaction| transaction.cancel_scheduled_job(&job_id))?
            .value)
    }

    pub fn record_scheduled_job_result(&self, result: &ScheduledJobResult) -> Result<()> {
        let result = result.clone();
        self.execute_write(move |transaction| transaction.record_scheduled_job_result(&result))?;
        Ok(())
    }

    pub fn save_cron_job(&self, cron: &CronJob) -> Result<()> {
        let cron = cron.clone();
        self.execute_write(move |transaction| transaction.save_cron_job(&cron))?;
        Ok(())
    }

    pub fn delete_cron_job(&self, name: &str) -> Result<()> {
        let name = name.to_string();
        self.execute_write(move |transaction| transaction.delete_cron_job(name.as_str()))?;
        Ok(())
    }

    pub fn recover_running_jobs(&self, now: Timestamp) -> Result<()> {
        self.execute_write(move |transaction| transaction.recover_running_jobs(now))?;
        Ok(())
    }

    pub fn insert(&self, document: &Document) -> Result<CommitEntry> {
        self.insert_once(document, None)?
            .ok_or_else(|| Error::Internal("non-deduplicated insert should commit".to_string()))
    }

    pub fn insert_with_indexes(
        &self,
        document: &Document,
        _indexes: &[neovex_core::IndexDefinition],
    ) -> Result<CommitEntry> {
        self.insert(document)
    }

    pub fn insert_once(
        &self,
        document: &Document,
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        let document = document.clone();
        let execution_id = execution_id.map(ToOwned::to_owned);
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(false);
            }
            transaction.insert_document(&document)?;
            Ok(true)
        })?;
        Ok(if committed.value {
            Some(expect_write_commit(
                committed.commit,
                "deduplicated insert should record a commit entry",
            )?)
        } else {
            None
        })
    }

    pub fn insert_with_indexes_once(
        &self,
        document: &Document,
        _indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        self.insert_once(document, execution_id)
    }

    pub fn update_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated_once(table, id, patch, None, validate)?
            .ok_or_else(|| Error::Internal("non-deduplicated update should commit".to_string()))
    }

    pub fn update_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        let table = table.clone();
        let id = *id;
        let patch = patch.clone();
        let execution_id = execution_id.map(ToOwned::to_owned);
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(false);
            }
            transaction.update_document_validated(&table, &id, &patch, validate)?;
            Ok(true)
        })?;
        Ok(if committed.value {
            Some(expect_write_commit(
                committed.commit,
                "deduplicated update should record a commit entry",
            )?)
        } else {
            None
        })
    }

    pub fn update_with_indexes_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        _indexes: &[neovex_core::IndexDefinition],
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated(table, id, patch, validate)
    }

    pub fn update_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        _indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated_once(table, id, patch, execution_id, validate)
    }

    pub fn delete_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_once(table, id, None, validate)?
            .ok_or_else(|| Error::Internal("non-deduplicated delete should commit".to_string()))
    }

    pub fn delete_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        let table = table.clone();
        let id = *id;
        let execution_id = execution_id.map(ToOwned::to_owned);
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(None);
            }
            let removed_document = transaction.delete_document_validated(&table, &id, validate)?;
            Ok(Some(removed_document))
        })?;
        Ok(if let Some(removed_document) = committed.value {
            Some((
                expect_write_commit(
                    committed.commit,
                    "deduplicated delete should record a commit entry",
                )?,
                removed_document,
            ))
        } else {
            None
        })
    }

    pub fn delete_with_indexes_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[neovex_core::IndexDefinition],
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_returning_document(table, id, validate)
    }

    pub fn delete_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_once(table, id, execution_id, validate)
    }

    pub fn apply_execution_unit_batch(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
    ) -> Result<Option<CommitEntry>> {
        if writes.is_empty() && schedule_ops.is_empty() {
            return Err(Error::Internal(
                "execution-unit batch must contain at least one change".to_string(),
            ));
        }
        let writes = writes.to_vec();
        let schedule_ops = schedule_ops.to_vec();
        let committed = self.execute_write(move |transaction| {
            for write in &writes {
                transaction.apply_resolved_write(write)?;
            }
            apply_schedule_ops_in_libsql_transaction(transaction, &schedule_ops)?;
            Ok(())
        })?;
        Ok(committed.commit)
    }

    pub fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut LibsqlReplicaWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.execute_write_cancellable(|| Ok(()), task)
    }

    pub fn execute_write_cancellable<T, Check, F>(
        &self,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut LibsqlReplicaWriteTransaction) -> Result<T> + Send + 'static,
    {
        if TokioRuntimeHandle::try_current().is_ok() {
            let store = self.clone();
            return std::thread::spawn(move || store.execute_write_cancellable(check_cancel, task))
                .join()
                .map_err(|_| {
                    Error::Internal("libsql replica write bridge thread panicked".to_string())
                })?;
        }

        let mut transaction = self.begin_write_transaction_cancellable(check_cancel)?;
        let value = match task(&mut transaction) {
            Ok(value) => value,
            Err(error) => {
                transaction.rollback();
                return Err(error);
            }
        };
        let commit = transaction.commit()?;
        Ok(TenantWriteCommit { value, commit })
    }

    fn begin_write_transaction_cancellable<Check>(
        &self,
        check_cancel: Check,
    ) -> Result<LibsqlReplicaWriteTransaction>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        LibsqlReplicaWriteTransaction::begin(self.clone(), check_cancel)
    }

    fn remote_connection(&self) -> Result<Connection> {
        self.remote_database.connect().map_err(map_libsql_error)
    }

    fn active_cache_handle(&self) -> Result<ReplicaCacheHandle> {
        self.active_cache
            .read()
            .map_err(|_| Error::Internal("libsql replica cache lock poisoned".to_string()))
            .map(|guard| guard.clone())
    }

    fn active_cache_store(&self) -> Result<Arc<SqliteTenantStore>> {
        Ok(self.active_cache_handle()?.store)
    }

    fn current_query_cache_store(&self) -> Result<Arc<SqliteTenantStore>> {
        self.ensure_local_cache_current()?;
        self.active_cache_store()
    }

    fn ensure_local_cache_current(&self) -> Result<()> {
        if self.local_cache_satisfies_requirements()? {
            return Ok(());
        }
        self.schedule_background_refresh();
        self.wait_for_background_refresh()?;
        if self.local_cache_satisfies_requirements()? {
            return Ok(());
        }
        self.refresh_local_cache()?;
        Ok(())
    }

    fn note_required_cache_sequence(&self, sequence: SequenceNumber) {
        let mut current = self.required_cache_sequence.load(Ordering::Acquire);
        while sequence.0 > current {
            match self.required_cache_sequence.compare_exchange(
                current,
                sequence.0,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(updated) => current = updated,
            }
        }
        self.schedule_background_refresh();
    }

    fn refresh_local_cache(&self) -> Result<JournalProgress> {
        if !self.refresh_needed.load(Ordering::Acquire) {
            return self.catch_up_local_cache_from_remote_durable_journal();
        }
        self.refresh_local_cache_from_snapshot()
    }

    fn refresh_local_cache_from_snapshot(&self) -> Result<JournalProgress> {
        let snapshot = self.block_on(fetch_remote_namespace_snapshot(
            &self.provider.primary_url,
            self.provider.auth_token.as_deref(),
            &self.namespace,
        ))?;
        let durable_head = snapshot
            .commit_log
            .last()
            .map(|record| SequenceNumber(record.sequence))
            .unwrap_or(SequenceNumber(0));
        let generation = self.next_cache_generation.fetch_add(1, Ordering::AcqRel);
        let replica_path = self
            .provider
            .replica_dir_for_tenant(&self.tenant_id)
            .join(format!("cache-{generation}.sqlite3"));
        let replica_dir = self.provider.replica_dir_for_tenant(&self.tenant_id);
        let path_for_materialize = replica_path.clone();
        let path_for_open = replica_path.clone();
        let clock = self.provider.clock.clone();
        let fault_injector = self.provider.fault_injector.clone();
        let read_parallelism = self.provider.tenant_read_parallelism;
        let next_store = {
            materialize_snapshot_to_replica_cache(
                replica_dir.as_path(),
                path_for_materialize.as_path(),
                snapshot,
            )?;
            SqliteTenantStore::open_with_simulation_and_max_read_connections(
                path_for_open,
                clock,
                fault_injector,
                read_parallelism,
            )?
        };
        let next_store = Arc::new(next_store);
        let next_handle = ReplicaCacheHandle {
            store: next_store.clone(),
            replica_path: replica_path.clone(),
        };
        let previous = {
            let mut guard = self
                .active_cache
                .write()
                .map_err(|_| Error::Internal("libsql replica cache lock poisoned".to_string()))?;
            let previous = guard.clone();
            *guard = next_handle;
            previous
        };
        self.retired_caches
            .lock()
            .map_err(|_| Error::Internal("libsql retired cache lock poisoned".to_string()))?
            .push(previous);
        self.refresh_needed.store(false, Ordering::Release);
        self.reap_retired_caches()?;
        Ok(JournalProgress {
            durable_head,
            applied_head: next_store.applied_sequence()?,
        })
    }

    fn catch_up_local_cache_from_remote_durable_journal(&self) -> Result<JournalProgress> {
        let store = self.active_cache_store()?;
        let local_progress = store.journal_progress()?;
        let required_sequence = self.required_cache_sequence.load(Ordering::Acquire);
        if local_progress.applied_head.0 >= required_sequence {
            return Ok(local_progress);
        }

        if local_progress.durable_head.0 < required_sequence {
            let next_sequence = SequenceNumber(local_progress.durable_head.0.saturating_add(1));
            let records = self.block_on(self.load_remote_durable_records_from(next_sequence))?;
            if !records.is_empty() {
                store.append_durable_records_batch(records.as_slice())?;
            }
        }

        let recovered = store.recover_durable_journal()?;
        if recovered.applied_head.0 >= required_sequence {
            return Ok(recovered);
        }

        self.refresh_needed.store(true, Ordering::Release);
        self.refresh_local_cache_from_snapshot()
    }

    fn local_cache_satisfies_requirements(&self) -> Result<bool> {
        let required = self.required_cache_sequence.load(Ordering::Acquire);
        let full_refresh_needed = self.refresh_needed.load(Ordering::Acquire);
        let local_applied = self.active_cache_store()?.applied_sequence()?.0;
        Ok(!full_refresh_needed && local_applied >= required)
    }

    fn schedule_background_refresh(&self) {
        self.refresh_requested.store(true, Ordering::Release);
        if self
            .refresh_inflight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let store = self.clone();
        let refresh_complete = self.refresh_complete.clone();
        let refresh_inflight = self.refresh_inflight.clone();
        self.provider.runtime_handle.spawn_blocking(move || {
            let refresh_result = store.run_background_refresh_loop();
            refresh_inflight.store(false, Ordering::Release);
            refresh_complete.notify_waiters();
            let should_reschedule = refresh_result.is_ok()
                && (store.refresh_requested.load(Ordering::Acquire)
                    || store
                        .local_cache_satisfies_requirements()
                        .map(|ready| !ready)
                        .unwrap_or(false));
            if should_reschedule {
                store.schedule_background_refresh();
            }
        });
    }

    fn run_background_refresh_loop(&self) -> Result<()> {
        loop {
            self.refresh_requested.store(false, Ordering::Release);
            self.refresh_local_cache()?;
            if !self.refresh_requested.load(Ordering::Acquire)
                && self.local_cache_satisfies_requirements()?
            {
                return Ok(());
            }
        }
    }

    fn wait_for_background_refresh(&self) -> Result<()> {
        while self.refresh_inflight.load(Ordering::Acquire) {
            let notified = self.refresh_complete.notified();
            if !self.refresh_inflight.load(Ordering::Acquire) {
                break;
            }
            self.block_on(async move {
                notified.await;
                Ok(())
            })?;
        }
        Ok(())
    }

    fn reap_retired_caches(&self) -> Result<()> {
        let mut retired = self
            .retired_caches
            .lock()
            .map_err(|_| Error::Internal("libsql retired cache lock poisoned".to_string()))?;
        let mut index = 0;
        while index < retired.len() {
            if Arc::strong_count(&retired[index].store) != 1 {
                index += 1;
                continue;
            }
            let handle = retired.swap_remove(index);
            drop(handle.store);
            remove_sqlite_artifacts(handle.replica_path.as_path())?;
        }
        Ok(())
    }

    fn block_on<T, Fut>(&self, future: Fut) -> Result<T>
    where
        T: Send,
        Fut: Future<Output = Result<T>>,
    {
        let handle = self.provider.runtime_handle.clone();
        if TokioRuntimeHandle::try_current().is_ok() {
            tokio::task::block_in_place(|| handle.block_on(future))
        } else {
            handle.block_on(future)
        }
    }

    async fn load_remote_schema(&self) -> Result<Schema> {
        let conn = self.remote_connection()?;
        let rows = query_remote_schema_rows(&conn).await?;
        let mut schema = Schema::default();
        for row in rows {
            let table_schema: TableSchema = deserialize_json(row.schema_json.as_str())?;
            schema
                .tables
                .insert(table_schema.table.clone(), table_schema);
        }
        Ok(schema)
    }

    async fn load_remote_latest_sequence(&self) -> Result<SequenceNumber> {
        Ok(SequenceNumber(
            load_next_sequence_from_session(&self.remote_connection()?)
                .await?
                .saturating_sub(1),
        ))
    }

    async fn load_remote_durable_records_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        let conn = self.remote_connection()?;
        let mut rows = conn
            .query(
                "SELECT record_blob FROM commit_log WHERE sequence >= ?1 ORDER BY sequence",
                libsql::params![i64_from_u64(sequence.0)?],
            )
            .await
            .map_err(map_libsql_error)?;
        let mut records = Vec::new();
        while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
            let payload = row.get::<Vec<u8>>(0).map_err(map_libsql_error)?;
            records.push(deserialize_durable_record(payload.as_slice())?);
        }
        Ok(records)
    }

    async fn load_remote_durable_journal_page(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        let latest_sequence = self.load_remote_latest_sequence().await?;
        let cursor_floor = self.load_remote_durable_journal_cursor_floor().await?;
        if after.0 < cursor_floor.0 {
            return Err(Error::InvalidInput(format!(
                "journal cursor {} is behind the retention floor {}",
                after.0, cursor_floor.0
            )));
        }
        if after.0 > latest_sequence.0 {
            return Err(Error::InvalidInput(format!(
                "journal cursor {} is ahead of the latest durable sequence {}",
                after.0, latest_sequence.0
            )));
        }

        let conn = self.remote_connection()?;
        let mut rows = conn
            .query(
                "SELECT record_blob FROM commit_log WHERE sequence > ?1 ORDER BY sequence LIMIT ?2",
                libsql::params![
                    i64_from_u64(after.0)?,
                    i64_from_u64(limit.saturating_add(1) as u64)?
                ],
            )
            .await
            .map_err(map_libsql_error)?;
        let mut records = Vec::with_capacity(limit);
        let mut has_more = false;
        while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
            let payload = row.get::<Vec<u8>>(0).map_err(map_libsql_error)?;
            if records.len() == limit {
                has_more = true;
                break;
            }
            records.push(deserialize_durable_record(payload.as_slice())?);
        }
        let next_cursor = records
            .last()
            .map(|record| record.sequence)
            .unwrap_or(after);
        Ok(DurableJournalPage {
            records,
            next_cursor,
            latest_sequence,
            cursor_floor,
            has_more,
        })
    }

    async fn load_remote_durable_journal_cursor_floor(&self) -> Result<SequenceNumber> {
        let conn = self.remote_connection()?;
        let mut rows = conn
            .query("SELECT MIN(sequence) FROM commit_log", ())
            .await
            .map_err(map_libsql_error)?;
        let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
            return Ok(SequenceNumber(0));
        };
        let min_sequence = row.get::<Option<i64>>(0).map_err(map_libsql_error)?;
        Ok(match min_sequence {
            Some(sequence) => SequenceNumber(sequence_from_i64(sequence)?.0.saturating_sub(1)),
            None => SequenceNumber(0),
        })
    }

    async fn load_remote_scheduled_jobs(&self, table: &str) -> Result<Vec<ScheduledJob>> {
        let conn = self.remote_connection()?;
        let sql = format!("SELECT data_json FROM {table}");
        let mut rows = conn
            .query(sql.as_str(), ())
            .await
            .map_err(map_libsql_error)?;
        let mut jobs = Vec::new();
        while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
            jobs.push(deserialize_json::<ScheduledJob>(
                row.get::<String>(0).map_err(map_libsql_error)?.as_str(),
            )?);
        }
        Ok(jobs)
    }

    async fn load_remote_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let conn = self.remote_connection()?;
        let mut rows = conn
            .query("SELECT data_json FROM cron_jobs ORDER BY name", ())
            .await
            .map_err(map_libsql_error)?;
        let mut crons = Vec::new();
        while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
            crons.push(deserialize_json::<CronJob>(
                row.get::<String>(0).map_err(map_libsql_error)?.as_str(),
            )?);
        }
        Ok(crons)
    }

    async fn append_remote_durable_records_batch(
        &self,
        records: &[DurableMutationRecord],
    ) -> Result<()> {
        let conn = self.remote_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(map_libsql_error)?;
        let mut next = load_next_sequence_from_session(&tx).await?;
        for record in records {
            if record.sequence.0 != next {
                return Err(Error::Internal(format!(
                    "durable journal append expected sequence {}, got {}",
                    next, record.sequence.0
                )));
            }
            tx.execute(
                "INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)",
                libsql::params![
                    i64_from_u64(record.sequence.0)?,
                    serialize_durable_record(record)?
                ],
            )
            .await
            .map_err(map_libsql_error)?;
            next = next.saturating_add(1);
        }
        put_remote_metadata_u64(&tx, NEXT_SEQUENCE_KEY, next).await?;
        tx.commit().await.map_err(map_libsql_error)?;
        Ok(())
    }

    async fn apply_remote_durable_records_batch(
        &self,
        records: &[DurableMutationRecord],
    ) -> Result<SequenceNumber> {
        let conn = self.remote_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(map_libsql_error)?;
        let mut applied_head = load_remote_metadata_u64(&tx, APPLIED_SEQUENCE_KEY)
            .await?
            .map(SequenceNumber)
            .unwrap_or(SequenceNumber(0));
        for record in records {
            if record.sequence.0 <= applied_head.0 {
                continue;
            }
            if record.sequence.0 != applied_head.0.saturating_add(1) {
                return Err(Error::Internal(format!(
                    "durable journal apply expected sequence {}, got {}",
                    applied_head.0.saturating_add(1),
                    record.sequence.0
                )));
            }
            apply_durable_record_in_remote_conn(&tx, record).await?;
            applied_head = record.sequence;
        }
        if applied_head.0 >= records[0].sequence.0 {
            put_remote_metadata_u64(&tx, APPLIED_SEQUENCE_KEY, applied_head.0).await?;
        }
        tx.commit().await.map_err(map_libsql_error)?;
        Ok(applied_head)
    }
}

impl LibsqlReplicaTenantStorage {
    pub fn new(store: Arc<LibsqlReplicaTenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self::with_max_concurrent_reads(store, runtime_handle, LIBSQL_TENANT_READ_PARALLELISM)
    }

    pub fn with_max_concurrent_reads(
        store: Arc<LibsqlReplicaTenantStore>,
        runtime_handle: TokioRuntimeHandle,
        max_concurrent_reads: usize,
    ) -> Self {
        Self {
            store: store.clone(),
            permits: Arc::new(Semaphore::new(max_concurrent_reads.max(1))),
            runtime_handle: runtime_handle.clone(),
            write_executor: LibsqlReplicaBlockingWriteExecutor::new(store, runtime_handle),
        }
    }

    pub fn store(&self) -> Arc<LibsqlReplicaTenantStore> {
        self.store.clone()
    }
}

impl TenantReadStorage for LibsqlReplicaTenantStorage {
    type Store = LibsqlReplicaTenantStore;

    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Self::Store>) -> Result<T> + Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(map_permit_error)?;
        let store = self.store.clone();
        self.runtime_handle
            .spawn_blocking(move || {
                let _permit = permit;
                task(store)
            })
            .await
            .map_err(map_join_error)?
    }

    async fn execute_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(Arc<Self::Store>, &mut dyn FnMut() -> Result<()>) -> Result<T> + Send + 'static,
    {
        tokio::pin!(cancel_wait);

        let permit = tokio::select! {
            _ = &mut cancel_wait => return Err(Error::Cancelled),
            permit = self.permits.clone().acquire_owned() => permit.map_err(map_permit_error)?,
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_for_task = cancelled.clone();
        let store = self.store.clone();
        let mut handle = self.runtime_handle.spawn_blocking(move || {
            let _permit = permit;
            let mut combined_cancel = || {
                if cancelled_for_task.load(Ordering::SeqCst) {
                    return Err(Error::Cancelled);
                }
                check_cancel()
            };
            task(store, &mut combined_cancel)
        });

        tokio::select! {
            _ = &mut cancel_wait => {
                cancelled.store(true, Ordering::SeqCst);
                handle.abort();
                Err(Error::Cancelled)
            }
            result = &mut handle => result.map_err(map_join_error)?,
        }
    }
}

impl TenantWriteStorage for LibsqlReplicaTenantStorage {
    type WriteTransaction = LibsqlReplicaWriteTransaction;

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut Self::WriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor.execute_write(task).await
    }

    async fn execute_write_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteOutcome<T>>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut Self::WriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor
            .execute_write_cancellable(cancel_wait, check_cancel, task)
            .await
    }
}

impl LibsqlReplicaBlockingWriteExecutor {
    fn new(store: Arc<LibsqlReplicaTenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self {
            store,
            permits: Arc::new(Semaphore::new(LIBSQL_TENANT_WRITE_PARALLELISM)),
            runtime_handle,
        }
    }

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut LibsqlReplicaWriteTransaction) -> Result<T> + Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(map_permit_error)?;
        let store = self.store.clone();
        self.runtime_handle
            .spawn_blocking(move || {
                let _permit = permit;
                store.execute_write(task)
            })
            .await
            .map_err(map_join_error)?
    }

    async fn execute_write_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteOutcome<T>>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut LibsqlReplicaWriteTransaction) -> Result<T> + Send + 'static,
    {
        tokio::pin!(cancel_wait);

        let permit = tokio::select! {
            _ = &mut cancel_wait => return Ok(TenantWriteOutcome::CancelledBeforeCommit),
            permit = self.permits.clone().acquire_owned() => permit.map_err(map_permit_error)?,
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_for_task = cancelled.clone();
        let store = self.store.clone();
        let mut handle = self.runtime_handle.spawn_blocking(move || {
            let _permit = permit;
            store.execute_write_cancellable(
                move || {
                    if cancelled_for_task.load(Ordering::SeqCst) {
                        return Err(Error::Cancelled);
                    }
                    check_cancel()
                },
                task,
            )
        });

        tokio::select! {
            result = &mut handle => map_write_result(result.map_err(map_join_error)?),
            _ = &mut cancel_wait => {
                cancelled.store(true, Ordering::SeqCst);
                map_write_result(handle.await.map_err(map_join_error)?)
            }
        }
    }
}

impl LibsqlReplicaWriteTransaction {
    fn begin<Check>(store: LibsqlReplicaTenantStore, check_cancel: Check) -> Result<Self>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let conn = store.remote_connection()?;
        let tx = store.block_on(async move {
            conn.transaction_with_behavior(TransactionBehavior::Immediate)
                .await
                .map_err(map_libsql_error)
        })?;
        Ok(Self {
            store,
            tx: Some(tx),
            commit_writes: Vec::new(),
            check_cancel: Box::new(check_cancel),
            refresh_cache_after_commit: false,
        })
    }

    pub fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.check_cancel()?;
        let schema_json = serialize_json(table_schema)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO schemas (table_name, schema_json) VALUES (?1, ?2)
                     ON CONFLICT(table_name) DO UPDATE SET schema_json = excluded.schema_json",
                    libsql::params![table_schema.table.as_str(), schema_json],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })?;
        self.refresh_cache_after_commit = true;
        Ok(())
    }

    pub fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.check_cancel()?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "DELETE FROM schemas WHERE table_name = ?1",
                    libsql::params![table.as_str()],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })?;
        self.refresh_cache_after_commit = true;
        Ok(())
    }

    pub fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool> {
        self.check_cancel()?;
        let Some(execution_id) = execution_id else {
            return Ok(true);
        };
        self.store.block_on(async {
            let changed = self
                .session()?
                .execute(
                    "INSERT OR IGNORE INTO scheduled_job_executions (execution_id) VALUES (?1)",
                    libsql::params![execution_id],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(changed == 1)
        })
    }

    pub fn insert_document(&mut self, document: &Document) -> Result<()> {
        self.check_cancel()?;
        let data_json = serialize_document_fields(document)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO documents (table_name, id, data_json, creation_time)
                     VALUES (?1, ?2, ?3, ?4)",
                    libsql::params![
                        document.table.as_str(),
                        document.id.to_string(),
                        data_json,
                        i64_from_u64(document.creation_time.0)?
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })?;
        self.record_commit_write(WriteOp {
            table: document.table.clone(),
            op_type: WriteOpType::Insert,
            doc_id: document.id,
            previous: None,
            current: Some(document.clone()),
        });
        Ok(())
    }

    pub fn update_document_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        validate: F,
    ) -> Result<()>
    where
        F: FnOnce(&Document, &Document) -> Result<()>,
    {
        self.check_cancel()?;
        let existing_document = self
            .load_document(table, id)?
            .ok_or(Error::DocumentNotFound(*id))?;
        let mut document = existing_document.clone();
        for (field, value) in patch {
            document.fields.insert(field.clone(), value.clone());
        }
        validate(&existing_document, &document)?;
        let data_json = serialize_document_fields(&document)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "UPDATE documents
                     SET data_json = ?3, creation_time = ?4
                     WHERE table_name = ?1 AND id = ?2",
                    libsql::params![
                        table.as_str(),
                        id.to_string(),
                        data_json,
                        i64_from_u64(document.creation_time.0)?
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })?;
        self.record_commit_write(WriteOp {
            table: table.clone(),
            op_type: WriteOpType::Update,
            doc_id: *id,
            previous: Some(existing_document),
            current: Some(document),
        });
        Ok(())
    }

    pub fn delete_document_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<Document>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        self.check_cancel()?;
        let removed_document = self
            .load_document(table, id)?
            .ok_or(Error::DocumentNotFound(*id))?;
        validate(&removed_document)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "DELETE FROM documents WHERE table_name = ?1 AND id = ?2",
                    libsql::params![table.as_str(), id.to_string()],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })?;
        self.record_commit_write(WriteOp {
            table: table.clone(),
            op_type: WriteOpType::Delete,
            doc_id: *id,
            previous: Some(removed_document.clone()),
            current: None,
        });
        Ok(removed_document)
    }

    pub fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.check_cancel()?;
        let data_json = serialize_json(job)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO scheduled_jobs (id, data_json) VALUES (?1, ?2)",
                    libsql::params![job.id.to_string(), data_json],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })
    }

    pub fn claim_due_jobs(&mut self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        self.check_cancel()?;
        let due = self
            .store
            .block_on(self.store.load_remote_scheduled_jobs("scheduled_jobs"))?;
        let due = due
            .into_iter()
            .filter(|job| job.run_at.0 <= now.0)
            .collect::<Vec<_>>();
        for job in &due {
            self.check_cancel()?;
            let data_json = serialize_json(job)?;
            self.store.block_on(async {
                self.session()?
                    .execute(
                        "DELETE FROM scheduled_jobs WHERE id = ?1",
                        libsql::params![job.id.to_string()],
                    )
                    .await
                    .map_err(map_libsql_error)?;
                self.session()?
                    .execute(
                        "INSERT INTO running_scheduled_jobs (id, data_json) VALUES (?1, ?2)",
                        libsql::params![job.id.to_string(), data_json],
                    )
                    .await
                    .map_err(map_libsql_error)?;
                Ok(())
            })?;
        }
        Ok(due)
    }

    pub fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.check_cancel()?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "DELETE FROM running_scheduled_jobs WHERE id = ?1",
                    libsql::params![job_id.to_string()],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })
    }

    pub fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.check_cancel()?;
        self.store.block_on(async {
            let affected = self
                .session()?
                .execute(
                    "DELETE FROM scheduled_jobs WHERE id = ?1",
                    libsql::params![job_id.to_string()],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(affected == 1)
        })
    }

    pub fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.check_cancel()?;
        let data_json = serialize_json(result)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO scheduled_job_results (job_id, data_json) VALUES (?1, ?2)
                     ON CONFLICT(job_id) DO UPDATE SET data_json = excluded.data_json",
                    libsql::params![result.id.to_string(), data_json],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })
    }

    pub fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.check_cancel()?;
        let data_json = serialize_json(cron)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO cron_jobs (name, data_json) VALUES (?1, ?2)
                     ON CONFLICT(name) DO UPDATE SET data_json = excluded.data_json",
                    libsql::params![cron.name.clone(), data_json],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })
    }

    pub fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.check_cancel()?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "DELETE FROM cron_jobs WHERE name = ?1",
                    libsql::params![name],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })
    }

    pub fn recover_running_jobs(&mut self, now: Timestamp) -> Result<()> {
        self.check_cancel()?;
        let running_jobs = self.store.block_on(
            self.store
                .load_remote_scheduled_jobs("running_scheduled_jobs"),
        )?;
        for mut job in running_jobs {
            self.check_cancel()?;
            job.run_at = now;
            let data_json = serialize_json(&job)?;
            self.store.block_on(async {
                self.session()?
                    .execute(
                        "INSERT INTO scheduled_jobs (id, data_json) VALUES (?1, ?2)",
                        libsql::params![job.id.to_string(), data_json],
                    )
                    .await
                    .map_err(map_libsql_error)?;
                self.session()?
                    .execute(
                        "DELETE FROM running_scheduled_jobs WHERE id = ?1",
                        libsql::params![job.id.to_string()],
                    )
                    .await
                    .map_err(map_libsql_error)?;
                Ok(())
            })?;
        }
        Ok(())
    }

    pub fn apply_resolved_write(&mut self, write: &ResolvedWrite) -> Result<()> {
        match write {
            ResolvedWrite::Insert { document, .. } => {
                self.check_cancel()?;
                if self.load_document(&document.table, &document.id)?.is_some() {
                    return Err(Error::Conflict(format!(
                        "document {} changed before transaction commit",
                        document.id
                    )));
                }
                self.insert_document(document)
            }
            ResolvedWrite::Update {
                previous, current, ..
            } => {
                self.check_cancel()?;
                let existing =
                    self.load_document(&current.table, &current.id)?
                        .ok_or(Error::Conflict(format!(
                            "document {} changed before transaction commit",
                            current.id
                        )))?;
                if existing != *previous {
                    return Err(Error::Conflict(format!(
                        "document {} changed before transaction commit",
                        current.id
                    )));
                }
                let data_json = serialize_document_fields(current)?;
                self.store.block_on(async {
                    self.session()?
                        .execute(
                            "UPDATE documents
                             SET data_json = ?3, creation_time = ?4
                             WHERE table_name = ?1 AND id = ?2",
                            libsql::params![
                                current.table.as_str(),
                                current.id.to_string(),
                                data_json,
                                i64_from_u64(current.creation_time.0)?
                            ],
                        )
                        .await
                        .map_err(map_libsql_error)?;
                    Ok(())
                })?;
                self.record_commit_write(WriteOp {
                    table: current.table.clone(),
                    op_type: WriteOpType::Update,
                    doc_id: current.id,
                    previous: Some(previous.clone()),
                    current: Some(current.clone()),
                });
                Ok(())
            }
            ResolvedWrite::Delete { previous, .. } => {
                self.check_cancel()?;
                let existing =
                    self.load_document(&previous.table, &previous.id)?
                        .ok_or(Error::Conflict(format!(
                            "document {} changed before transaction commit",
                            previous.id
                        )))?;
                if existing != *previous {
                    return Err(Error::Conflict(format!(
                        "document {} changed before transaction commit",
                        previous.id
                    )));
                }
                self.store.block_on(async {
                    self.session()?
                        .execute(
                            "DELETE FROM documents WHERE table_name = ?1 AND id = ?2",
                            libsql::params![previous.table.as_str(), previous.id.to_string()],
                        )
                        .await
                        .map_err(map_libsql_error)?;
                    Ok(())
                })?;
                self.record_commit_write(WriteOp {
                    table: previous.table.clone(),
                    op_type: WriteOpType::Delete,
                    doc_id: previous.id,
                    previous: Some(previous.clone()),
                    current: None,
                });
                Ok(())
            }
        }
    }

    pub fn commit(mut self) -> Result<Option<CommitEntry>> {
        self.check_cancel()?;
        let writes = std::mem::take(&mut self.commit_writes);
        let commit = if writes.is_empty() {
            None
        } else {
            Some(self.append_commit_entry(writes)?)
        };
        let tx = self.tx.take().ok_or_else(|| {
            Error::Internal("libsql replica write transaction already closed".to_string())
        })?;
        self.store.block_on(async move {
            tx.commit().await.map_err(map_libsql_error)?;
            Ok(())
        })?;
        if let Some(commit) = &commit {
            self.store.note_required_cache_sequence(commit.sequence);
        } else if self.refresh_cache_after_commit {
            self.store.refresh_needed.store(true, Ordering::Release);
            self.store.schedule_background_refresh();
        }
        Ok(commit)
    }

    pub fn rollback(mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = self.store.block_on(async move {
                tx.rollback().await.map_err(map_libsql_error)?;
                Ok(())
            });
        }
    }

    fn session(&self) -> Result<&Transaction> {
        self.tx.as_ref().ok_or_else(|| {
            Error::Internal("libsql replica write transaction already closed".to_string())
        })
    }

    fn check_cancel(&self) -> Result<()> {
        (self.check_cancel.as_ref())()
    }

    fn record_commit_write(&mut self, write: WriteOp) {
        self.commit_writes.push(write);
    }

    fn load_document(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        self.store.block_on(load_remote_document_from_session(
            self.session()?,
            table.clone(),
            *id,
        ))
    }

    fn append_commit_entry(&self, writes: Vec<WriteOp>) -> Result<CommitEntry> {
        let sequence = SequenceNumber(
            self.store
                .block_on(load_next_sequence_from_session(self.session()?))?,
        );
        let entry = CommitEntry {
            sequence,
            timestamp: self.store.provider.clock.now(),
            writes,
        };
        let payload = serialize_commit(&entry)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)",
                    libsql::params![i64_from_u64(sequence.0)?, payload],
                )
                .await
                .map_err(map_libsql_error)?;
            put_remote_metadata_u64(
                self.session()?,
                NEXT_SEQUENCE_KEY,
                sequence.0.saturating_add(1),
            )
            .await?;
            put_remote_metadata_u64(self.session()?, APPLIED_SEQUENCE_KEY, sequence.0).await?;
            Ok(())
        })?;
        Ok(entry)
    }
}

fn map_write_result<T>(result: Result<TenantWriteCommit<T>>) -> Result<TenantWriteOutcome<T>> {
    match result {
        Ok(committed) => Ok(TenantWriteOutcome::Committed(committed)),
        Err(Error::Cancelled) => Ok(TenantWriteOutcome::CancelledBeforeCommit),
        Err(error) => Err(error),
    }
}

fn expect_write_commit(commit: Option<CommitEntry>, expectation: &str) -> Result<CommitEntry> {
    commit.ok_or_else(|| Error::Internal(expectation.to_string()))
}

fn validate_durable_journal_stream_limit(limit: usize) -> Result<()> {
    if limit == 0 || limit > MAX_DURABLE_JOURNAL_STREAM_LIMIT {
        return Err(Error::InvalidInput(format!(
            "durable journal stream limit must be between 1 and {}",
            MAX_DURABLE_JOURNAL_STREAM_LIMIT
        )));
    }
    Ok(())
}

fn apply_schedule_ops_in_libsql_transaction(
    transaction: &mut LibsqlReplicaWriteTransaction,
    schedule_ops: &[ResolvedScheduleOp],
) -> Result<()> {
    for op in schedule_ops {
        match op {
            ResolvedScheduleOp::Insert { job } => transaction.insert_scheduled_job(job)?,
            ResolvedScheduleOp::Cancel { job_id } => {
                transaction.cancel_scheduled_job(job_id)?;
            }
        }
    }
    Ok(())
}

async fn table_has_entries_remote(conn: &Connection, table: &str) -> Result<bool> {
    let sql = format!("SELECT 1 FROM {table} LIMIT 1");
    let mut rows = conn
        .query(sql.as_str(), ())
        .await
        .map_err(map_libsql_error)?;
    Ok(rows.next().await.map_err(map_libsql_error)?.is_some())
}

async fn load_remote_document_from_session(
    conn: &Connection,
    table: TableName,
    id: DocumentId,
) -> Result<Option<Document>> {
    let mut rows = conn
        .query(
            "SELECT creation_time, data_json FROM documents WHERE table_name = ?1 AND id = ?2",
            libsql::params![table.as_str(), id.to_string()],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(None);
    };
    let creation_time = row.get::<i64>(0).map_err(map_libsql_error)?;
    let data_json = row.get::<String>(1).map_err(map_libsql_error)?;
    Ok(Some(row_to_document(
        &table,
        &id,
        creation_time,
        data_json.as_str(),
    )?))
}

async fn load_next_sequence_from_session(conn: &Connection) -> Result<u64> {
    if let Some(stored) = load_remote_metadata_u64(conn, NEXT_SEQUENCE_KEY).await? {
        return Ok(stored);
    }
    let mut rows = conn
        .query("SELECT MAX(sequence) FROM commit_log", ())
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(1);
    };
    let latest = row.get::<Option<i64>>(0).map_err(map_libsql_error)?;
    Ok(latest
        .map(sequence_from_i64)
        .transpose()?
        .unwrap_or(SequenceNumber(0))
        .0
        .saturating_add(1))
}

async fn load_remote_metadata_u64(conn: &Connection, key: &str) -> Result<Option<u64>> {
    let mut rows = conn
        .query(
            "SELECT value_blob FROM metadata WHERE key = ?1",
            libsql::params![key.to_string()],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(None);
    };
    let bytes = row.get::<Vec<u8>>(0).map_err(map_libsql_error)?;
    Ok(Some(decode_u64(bytes.as_slice())?))
}

async fn put_remote_metadata_u64(conn: &Connection, key: &str, value: u64) -> Result<()> {
    conn.execute(
        "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
        libsql::params![key.to_string(), encode_u64(value).to_vec()],
    )
    .await
    .map_err(map_libsql_error)?;
    Ok(())
}

async fn apply_durable_record_in_remote_conn(
    conn: &Connection,
    record: &DurableMutationRecord,
) -> Result<()> {
    if let Some(execution_id) = record.scheduled_execution_id.as_deref() {
        let _ = begin_scheduled_execution_remote(conn, Some(execution_id)).await?;
    }

    for write in &record.writes {
        match (&write.previous, &write.current) {
            (None, Some(current)) => {
                let existing =
                    load_remote_document_from_session(conn, write.table.clone(), write.doc_id)
                        .await?;
                match existing {
                    Some(existing) if existing == *current => continue,
                    Some(_) => {
                        return Err(Error::Conflict(format!(
                            "durable journal insert replay found conflicting state for document {}",
                            write.doc_id
                        )));
                    }
                    None => {
                        conn.execute(
                            "INSERT INTO documents (table_name, id, data_json, creation_time)
                             VALUES (?1, ?2, ?3, ?4)",
                            libsql::params![
                                write.table.as_str(),
                                write.doc_id.to_string(),
                                serialize_document_fields(current)?,
                                i64_from_u64(current.creation_time.0)?
                            ],
                        )
                        .await
                        .map_err(map_libsql_error)?;
                    }
                }
            }
            (Some(previous), Some(current)) => {
                let existing =
                    load_remote_document_from_session(conn, write.table.clone(), write.doc_id)
                        .await?
                        .ok_or(Error::Conflict(format!(
                            "durable journal update replay missing document {}",
                            write.doc_id
                        )))?;
                if existing == *current {
                    continue;
                }
                if existing != *previous {
                    return Err(Error::Conflict(format!(
                        "durable journal update replay found conflicting state for document {}",
                        write.doc_id
                    )));
                }
                conn.execute(
                    "UPDATE documents
                     SET data_json = ?3, creation_time = ?4
                     WHERE table_name = ?1 AND id = ?2",
                    libsql::params![
                        write.table.as_str(),
                        write.doc_id.to_string(),
                        serialize_document_fields(current)?,
                        i64_from_u64(current.creation_time.0)?
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
            }
            (Some(previous), None) => {
                match load_remote_document_from_session(conn, write.table.clone(), write.doc_id)
                    .await?
                {
                    Some(existing) if existing != *previous => {
                        return Err(Error::Conflict(format!(
                            "durable journal delete replay found conflicting state for document {}",
                            write.doc_id
                        )));
                    }
                    Some(_) => {
                        conn.execute(
                            "DELETE FROM documents WHERE table_name = ?1 AND id = ?2",
                            libsql::params![write.table.as_str(), write.doc_id.to_string()],
                        )
                        .await
                        .map_err(map_libsql_error)?;
                    }
                    None => continue,
                }
            }
            (None, None) => {
                return Err(Error::Internal(
                    "durable journal write must include a previous or current document".to_string(),
                ));
            }
        }
    }

    Ok(())
}

async fn begin_scheduled_execution_remote(
    conn: &Connection,
    execution_id: Option<&str>,
) -> Result<bool> {
    let Some(execution_id) = execution_id else {
        return Ok(true);
    };
    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO scheduled_job_executions (execution_id) VALUES (?1)",
            libsql::params![execution_id],
        )
        .await
        .map_err(map_libsql_error)?;
    Ok(inserted == 1)
}

fn serialize_json<T>(value: &T) -> Result<String>
where
    T: Serialize,
{
    serde_json::to_string(value).map_err(|error| Error::Serialization(error.to_string()))
}

fn deserialize_json<T>(json: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_str(json).map_err(|error| Error::Serialization(error.to_string()))
}

fn serialize_document_fields(document: &Document) -> Result<String> {
    serialize_json(&document.fields)
}

fn encode_u64(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

fn decode_u64(bytes: &[u8]) -> Result<u64> {
    <[u8; 8]>::try_from(bytes)
        .map(u64::from_be_bytes)
        .map_err(|_| Error::Serialization("invalid u64 encoding".to_string()))
}

fn row_to_document(
    table: &TableName,
    id: &DocumentId,
    creation_time: i64,
    data_json: &str,
) -> Result<Document> {
    Ok(Document {
        id: *id,
        table: table.clone(),
        creation_time: Timestamp(u64::try_from(creation_time).map_err(|_| {
            Error::storage(
                StorageErrorKind::Corruption,
                format!("negative creation_time in libsql row: {creation_time}"),
            )
        })?),
        fields: deserialize_json(data_json)?,
    })
}

fn sequence_from_i64(value: i64) -> Result<SequenceNumber> {
    Ok(SequenceNumber(u64::try_from(value).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!("negative libsql sequence value: {value}"),
        )
    })?))
}

fn i64_from_u64(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| Error::InvalidInput(format!("value {value} exceeds SQLite INTEGER")))
}

#[derive(Debug, Clone)]
struct RemoteNamespaceSnapshot {
    schemas: Vec<RemoteSchemaRow>,
    documents: Vec<RemoteDocumentRow>,
    scheduled_jobs: Vec<RemoteJsonRow>,
    running_scheduled_jobs: Vec<RemoteJsonRow>,
    scheduled_job_results: Vec<RemoteJsonRow>,
    scheduled_job_executions: Vec<String>,
    cron_jobs: Vec<RemoteNamedJsonRow>,
    commit_log: Vec<RemoteCommitLogRow>,
    metadata: Vec<RemoteMetadataRow>,
}

#[derive(Debug, Clone)]
struct RemoteSchemaRow {
    table_name: String,
    schema_json: String,
}

#[derive(Debug, Clone)]
struct RemoteDocumentRow {
    table_name: String,
    id: String,
    creation_time: u64,
    data_json: String,
}

#[derive(Debug, Clone)]
struct RemoteJsonRow {
    key: String,
    data_json: String,
}

#[derive(Debug, Clone)]
struct RemoteNamedJsonRow {
    name: String,
    data_json: String,
}

#[derive(Debug, Clone)]
struct RemoteCommitLogRow {
    sequence: u64,
    record_blob: Vec<u8>,
}

#[derive(Debug, Clone)]
struct RemoteMetadataRow {
    key: String,
    value_blob: Vec<u8>,
}

async fn fetch_remote_namespace_snapshot(
    primary_url: &str,
    auth_token: Option<&str>,
    namespace: &str,
) -> Result<RemoteNamespaceSnapshot> {
    let database = open_remote_database(primary_url, auth_token, namespace).await?;
    let conn = database.connect().map_err(map_libsql_error)?;
    conn.execute_batch("BEGIN")
        .await
        .map_err(map_libsql_error)?;
    let snapshot = async {
        Ok(RemoteNamespaceSnapshot {
            schemas: query_remote_schema_rows(&conn).await?,
            documents: query_remote_document_rows(&conn).await?,
            scheduled_jobs: query_remote_json_rows(&conn, "scheduled_jobs", "id").await?,
            running_scheduled_jobs: query_remote_json_rows(&conn, "running_scheduled_jobs", "id")
                .await?,
            scheduled_job_results: query_remote_json_rows(&conn, "scheduled_job_results", "job_id")
                .await?,
            scheduled_job_executions: query_remote_execution_ids(&conn).await?,
            cron_jobs: query_remote_named_json_rows(&conn, "cron_jobs", "name").await?,
            commit_log: query_remote_commit_log_rows(&conn).await?,
            metadata: query_remote_metadata_rows(&conn).await?,
        })
    }
    .await;
    let _ = conn.execute_batch("ROLLBACK").await;
    snapshot
}

async fn query_remote_schema_rows(conn: &Connection) -> Result<Vec<RemoteSchemaRow>> {
    let mut rows = conn
        .query(
            "SELECT table_name, schema_json FROM schemas ORDER BY table_name",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteSchemaRow {
            table_name: row.get::<String>(0).map_err(map_libsql_error)?,
            schema_json: row.get::<String>(1).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn query_remote_document_rows(conn: &Connection) -> Result<Vec<RemoteDocumentRow>> {
    let mut rows = conn
        .query(
            "SELECT table_name, id, creation_time, data_json
             FROM documents
             ORDER BY table_name, id",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        let creation_time = row.get::<i64>(2).map_err(map_libsql_error)?;
        result.push(RemoteDocumentRow {
            table_name: row.get::<String>(0).map_err(map_libsql_error)?,
            id: row.get::<String>(1).map_err(map_libsql_error)?,
            creation_time: u64::try_from(creation_time).map_err(|_| {
                Error::storage(StorageErrorKind::Corruption, format!(
                    "remote libsql creation_time {creation_time} is negative for namespace snapshot"
                ))
            })?,
            data_json: row.get::<String>(3).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn query_remote_json_rows(
    conn: &Connection,
    table: &str,
    key_column: &str,
) -> Result<Vec<RemoteJsonRow>> {
    let sql = format!("SELECT {key_column}, data_json FROM {table} ORDER BY {key_column}");
    let mut rows = conn
        .query(sql.as_str(), ())
        .await
        .map_err(map_libsql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteJsonRow {
            key: row.get::<String>(0).map_err(map_libsql_error)?,
            data_json: row.get::<String>(1).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn query_remote_named_json_rows(
    conn: &Connection,
    table: &str,
    name_column: &str,
) -> Result<Vec<RemoteNamedJsonRow>> {
    let sql = format!("SELECT {name_column}, data_json FROM {table} ORDER BY {name_column}");
    let mut rows = conn
        .query(sql.as_str(), ())
        .await
        .map_err(map_libsql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteNamedJsonRow {
            name: row.get::<String>(0).map_err(map_libsql_error)?,
            data_json: row.get::<String>(1).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn query_remote_execution_ids(conn: &Connection) -> Result<Vec<String>> {
    let mut rows = conn
        .query(
            "SELECT execution_id FROM scheduled_job_executions ORDER BY execution_id",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(row.get::<String>(0).map_err(map_libsql_error)?);
    }
    Ok(result)
}

async fn query_remote_commit_log_rows(conn: &Connection) -> Result<Vec<RemoteCommitLogRow>> {
    let mut rows = conn
        .query(
            "SELECT sequence, record_blob FROM commit_log ORDER BY sequence",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        let sequence = row.get::<i64>(0).map_err(map_libsql_error)?;
        result.push(RemoteCommitLogRow {
            sequence: u64::try_from(sequence).map_err(|_| {
                Error::storage(StorageErrorKind::Corruption, format!(
                    "remote libsql durable sequence {sequence} is negative for namespace snapshot"
                ))
            })?,
            record_blob: row.get::<Vec<u8>>(1).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn query_remote_metadata_rows(conn: &Connection) -> Result<Vec<RemoteMetadataRow>> {
    let mut rows = conn
        .query("SELECT key, value_blob FROM metadata ORDER BY key", ())
        .await
        .map_err(map_libsql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteMetadataRow {
            key: row.get::<String>(0).map_err(map_libsql_error)?,
            value_blob: row.get::<Vec<u8>>(1).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

fn materialize_snapshot_to_replica_cache(
    replica_dir: &Path,
    replica_path: &Path,
    snapshot: RemoteNamespaceSnapshot,
) -> Result<()> {
    std::fs::create_dir_all(replica_dir).map_err(storage_io_error)?;
    let staging_path = staged_replica_path(replica_path);
    remove_sqlite_artifacts(staging_path.as_path())?;

    let conn =
        LocalSqliteConnection::open(staging_path.as_path()).map_err(map_local_sqlite_error)?;
    initialize_local_replica_cache(&conn)?;
    let write_result = (|| {
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_local_sqlite_error)?;
        insert_snapshot_rows(&conn, &snapshot)?;
        conn.execute_batch("COMMIT")
            .map_err(map_local_sqlite_error)?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = conn.execute_batch("ROLLBACK");
        return Err(error);
    }
    rebuild_sqlite_indexes_from_loaded_schema(&conn)?;
    drop(conn);

    remove_sqlite_artifacts(replica_path)?;
    std::fs::rename(staging_path.as_path(), replica_path).map_err(storage_io_error)?;
    Ok(())
}

fn initialize_local_replica_cache(conn: &LocalSqliteConnection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(map_local_sqlite_error)?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(map_local_sqlite_error)?;
    conn.execute_batch(SQLITE_INIT_SQL)
        .map_err(map_local_sqlite_error)?;
    Ok(())
}

fn insert_snapshot_rows(
    conn: &LocalSqliteConnection,
    snapshot: &RemoteNamespaceSnapshot,
) -> Result<()> {
    {
        let mut statement = conn
            .prepare("INSERT INTO schemas (table_name, schema_json) VALUES (?1, ?2)")
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.schemas {
            statement
                .execute(params![row.table_name.as_str(), row.schema_json.as_str()])
                .map_err(map_local_sqlite_error)?;
        }
    }
    {
        let mut statement = conn
            .prepare(
                "INSERT INTO documents (table_name, id, data_json, creation_time)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.documents {
            statement
                .execute(params![
                    row.table_name.as_str(),
                    row.id.as_str(),
                    row.data_json.as_str(),
                    row.creation_time
                ])
                .map_err(map_local_sqlite_error)?;
        }
    }
    insert_json_rows(conn, "scheduled_jobs", "id", &snapshot.scheduled_jobs)?;
    insert_json_rows(
        conn,
        "running_scheduled_jobs",
        "id",
        &snapshot.running_scheduled_jobs,
    )?;
    insert_json_rows(
        conn,
        "scheduled_job_results",
        "job_id",
        &snapshot.scheduled_job_results,
    )?;
    {
        let mut statement = conn
            .prepare("INSERT INTO scheduled_job_executions (execution_id) VALUES (?1)")
            .map_err(map_local_sqlite_error)?;
        for execution_id in &snapshot.scheduled_job_executions {
            statement
                .execute(params![execution_id.as_str()])
                .map_err(map_local_sqlite_error)?;
        }
    }
    {
        let mut statement = conn
            .prepare("INSERT INTO cron_jobs (name, data_json) VALUES (?1, ?2)")
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.cron_jobs {
            statement
                .execute(params![row.name.as_str(), row.data_json.as_str()])
                .map_err(map_local_sqlite_error)?;
        }
    }
    {
        let mut statement = conn
            .prepare("INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)")
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.commit_log {
            statement
                .execute(params![row.sequence, row.record_blob.as_slice()])
                .map_err(map_local_sqlite_error)?;
        }
    }
    {
        let mut statement = conn
            .prepare("INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)")
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.metadata {
            statement
                .execute(params![row.key.as_str(), row.value_blob.as_slice()])
                .map_err(map_local_sqlite_error)?;
        }
    }
    Ok(())
}

fn insert_json_rows(
    conn: &LocalSqliteConnection,
    table: &str,
    key_column: &str,
    rows: &[RemoteJsonRow],
) -> Result<()> {
    let sql = format!("INSERT INTO {table} ({key_column}, data_json) VALUES (?1, ?2)");
    let mut statement = conn.prepare(sql.as_str()).map_err(map_local_sqlite_error)?;
    for row in rows {
        statement
            .execute(params![row.key.as_str(), row.data_json.as_str()])
            .map_err(map_local_sqlite_error)?;
    }
    Ok(())
}

fn staged_replica_path(replica_path: &Path) -> PathBuf {
    let file_name = replica_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| LIBSQL_REPLICA_FILENAME.to_string());
    replica_path.with_file_name(format!("{file_name}.staging"))
}

fn remove_sqlite_artifacts(path: &Path) -> Result<()> {
    remove_file_if_exists(path)?;
    remove_file_if_exists(sqlite_sidecar_path(path, "-wal").as_path())?;
    remove_file_if_exists(sqlite_sidecar_path(path, "-shm").as_path())?;
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_io_error(error)),
    }
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{}", path.display(), suffix))
}

async fn bootstrap_tenant_namespace(
    primary_url: &str,
    auth_token: Option<&str>,
    namespace: &str,
) -> Result<()> {
    let database = open_remote_database(primary_url, auth_token, namespace).await?;
    let conn = database.connect().map_err(map_libsql_error)?;
    conn.execute_batch(SQLITE_INIT_SQL)
        .await
        .map_err(map_libsql_error)?;
    Ok(())
}

async fn clear_tenant_namespace(
    primary_url: &str,
    auth_token: Option<&str>,
    namespace: &str,
) -> Result<()> {
    let database = open_remote_database(primary_url, auth_token, namespace).await?;
    let conn = database.connect().map_err(map_libsql_error)?;
    conn.execute_batch(LIBSQL_DROP_TENANT_SQL)
        .await
        .map_err(map_libsql_error)?;
    Ok(())
}

async fn tenant_namespace_has_foundation(
    primary_url: &str,
    auth_token: Option<&str>,
    namespace: &str,
) -> Result<bool> {
    let database = open_remote_database(primary_url, auth_token, namespace).await?;
    let conn = database.connect().map_err(map_libsql_error)?;
    let mut rows = conn
        .query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'metadata'",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    Ok(rows.next().await.map_err(map_libsql_error)?.is_some())
}

async fn open_remote_database(
    primary_url: &str,
    auth_token: Option<&str>,
    namespace: &str,
) -> Result<Database> {
    let builder = Builder::new_remote(
        primary_url.to_string(),
        auth_token.unwrap_or_default().to_string(),
    )
    .namespace(namespace.to_string())
    .connector(libsql_transport_connector()?);
    builder.build().await.map_err(map_libsql_error)
}

async fn ensure_remote_namespace_exists(
    admin_api_url: &str,
    admin_auth_header: Option<&str>,
    namespace: &str,
) -> Result<()> {
    let response = apply_admin_auth(
        HttpClient::new()
            .post(namespace_create_endpoint(admin_api_url, namespace))
            .json(&serde_json::json!({})),
        admin_auth_header,
    )
    .send()
    .await
    .map_err(map_admin_api_error)?;
    let status = response.status();
    let body = response.text().await.map_err(map_admin_api_error)?;
    if status.is_success() || (status.as_u16() == 400 && body.contains("already exists")) {
        return Ok(());
    }
    Err(Error::storage(
        StorageErrorKind::Unavailable,
        format!(
            "libsql admin namespace create failed for '{namespace}': status={status}, body={body}"
        ),
    ))
}

async fn drop_remote_namespace(
    admin_api_url: &str,
    admin_auth_header: Option<&str>,
    namespace: &str,
) -> Result<()> {
    let response = apply_admin_auth(
        HttpClient::new().delete(namespace_endpoint(admin_api_url, namespace)),
        admin_auth_header,
    )
    .send()
    .await
    .map_err(map_admin_api_error)?;
    let status = response.status();
    let body = response.text().await.map_err(map_admin_api_error)?;
    if status.is_success()
        || (status.as_u16() == 404 && body.contains("doesn't exist"))
        || (status.as_u16() == 500 && body.contains("Directory not empty"))
    {
        return Ok(());
    }
    Err(Error::storage(
        StorageErrorKind::Unavailable,
        format!(
            "libsql admin namespace delete failed for '{namespace}': status={status}, body={body}"
        ),
    ))
}

fn apply_admin_auth(
    request: reqwest::RequestBuilder,
    admin_auth_header: Option<&str>,
) -> reqwest::RequestBuilder {
    match admin_auth_header {
        Some(value) => request.header(AUTHORIZATION, value),
        None => request,
    }
}

fn namespace_create_endpoint(admin_api_url: &str, namespace: &str) -> String {
    format!(
        "{}/v1/namespaces/{namespace}/create",
        admin_api_url.trim_end_matches('/')
    )
}

fn namespace_endpoint(admin_api_url: &str, namespace: &str) -> String {
    format!(
        "{}/v1/namespaces/{namespace}",
        admin_api_url.trim_end_matches('/')
    )
}

fn tenant_namespace_name(prefix: &str, tenant_id: &TenantId) -> Result<String> {
    let mut candidate = format!("{prefix}{}", tenant_id.as_str().replace('-', "_"));
    if candidate.len() <= LIBSQL_NAMESPACE_LIMIT {
        validate_namespace_input(&candidate, "tenant namespace")?;
        return Ok(candidate);
    }

    let hash = hex_tenant_hash(tenant_id);
    let separator = if prefix.is_empty() { "" } else { "_" };
    let max_hash_len = TARGET_TENANT_HASH_HEX_LEN.min(hash.len());
    for hash_len in (MIN_TENANT_HASH_HEX_LEN..=max_hash_len).rev() {
        candidate = format!("{prefix}{separator}{}", &hash[..hash_len]);
        if candidate.len() <= LIBSQL_NAMESPACE_LIMIT {
            validate_namespace_input(&candidate, "tenant namespace")?;
            return Ok(candidate);
        }
    }

    Err(Error::InvalidInput(format!(
        "tenant namespace prefix '{prefix}' is too long to derive a libsql namespace"
    )))
}

fn hex_tenant_hash(tenant_id: &TenantId) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tenant_id.as_str().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn validate_namespace_input(value: &str, field: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidInput(format!("{field} cannot be empty")));
    }
    if value.len() > LIBSQL_NAMESPACE_LIMIT {
        return Err(Error::InvalidInput(format!(
            "{field} must be at most {LIBSQL_NAMESPACE_LIMIT} characters"
        )));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(Error::InvalidInput(format!(
            "{field} must contain only ASCII letters, digits, '_' or '-'"
        )));
    }
    Ok(())
}

fn map_libsql_error(error: libsql::Error) -> Error {
    let message = error.to_string();
    match error {
        libsql::Error::ConnectionFailed(_)
        | libsql::Error::Hrana(_)
        | libsql::Error::WriteDelegation(_)
        | libsql::Error::Replication(_)
        | libsql::Error::Sync(_)
        | libsql::Error::InvalidTlsConfiguration(_) => {
            Error::storage(StorageErrorKind::Unavailable, message)
        }
        libsql::Error::WalConflict => Error::storage(StorageErrorKind::Busy, message),
        libsql::Error::SqliteFailure(code, _) | libsql::Error::RemoteSqliteFailure(_, code, _) => {
            map_sqlite_result_code(code, message)
        }
        _ => Error::storage(StorageErrorKind::Other, message),
    }
}

fn map_local_sqlite_error(error: rusqlite::Error) -> Error {
    let message = error.to_string();
    match error {
        rusqlite::Error::SqliteFailure(code, _) => {
            map_sqlite_result_code(code.extended_code, message)
        }
        _ => Error::storage(StorageErrorKind::Other, message),
    }
}

fn map_admin_api_error(error: reqwest::Error) -> Error {
    let message = format!("libsql admin API request failed: {error}");
    if error.is_connect() || error.is_timeout() {
        Error::storage(StorageErrorKind::Unavailable, message)
    } else {
        Error::storage(StorageErrorKind::Transient, message)
    }
}

fn map_permit_error(_error: tokio::sync::AcquireError) -> Error {
    Error::Internal("libsql replica executor unexpectedly closed".to_string())
}

fn map_join_error(error: tokio::task::JoinError) -> Error {
    Error::Internal(format!("libsql replica read task failed: {error}"))
}

fn storage_io_error(error: impl std::fmt::Display) -> Error {
    Error::storage(StorageErrorKind::Io, error.to_string())
}

fn map_sqlite_result_code(code: i32, message: String) -> Error {
    match code & 0xff {
        5 | 6 => Error::storage(StorageErrorKind::Busy, message),
        3 | 8 | 23 => Error::PermissionDenied(message),
        7 | 13 => Error::ResourceExhausted(message),
        10 => Error::storage(StorageErrorKind::Io, message),
        11 | 26 => Error::storage(StorageErrorKind::Corruption, message),
        14 => Error::storage(StorageErrorKind::Unavailable, message),
        9 | 15 | 17 => Error::storage(StorageErrorKind::Transient, message),
        _ => Error::storage(StorageErrorKind::Other, message),
    }
}
