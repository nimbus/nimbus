use super::common::{copy_dir_all, read_u64_override};
use super::config::{BenchmarkLane, EncryptionMode, WorkloadKind};
use super::models::{BackendPair, BackendSamples};
use super::*;

static ENCRYPTION_MODE: OnceLock<EncryptionMode> = OnceLock::new();

pub(super) fn configure_encryption_mode(mode: EncryptionMode) -> BenchResult<()> {
    ENCRYPTION_MODE
        .set(mode)
        .map_err(|_| "embedded benchmark encryption mode was already configured".into())
}

fn encryption_mode() -> EncryptionMode {
    *ENCRYPTION_MODE.get().unwrap_or(&EncryptionMode::Disabled)
}

pub(super) async fn build_backend_pair_async<T, F, Fut>(mut build: F) -> BenchResult<BackendPair<T>>
where
    F: FnMut(EmbeddedProviderKind) -> Fut,
    Fut: std::future::Future<Output = BenchResult<T>>,
{
    Ok(BackendPair {
        redb: build(EmbeddedProviderKind::Redb).await?,
        sqlite: build(EmbeddedProviderKind::Sqlite).await?,
    })
}

pub(super) async fn measure_backend_pair_async<F, Fut>(
    workload: WorkloadKind,
    lane: BenchmarkLane,
    mut run_sample: F,
) -> BenchResult<BackendSamples>
where
    F: FnMut(EmbeddedProviderKind) -> Fut,
    Fut: std::future::Future<Output = BenchResult<Duration>>,
{
    eprintln!("  starting {} lane", lane.label().to_lowercase());
    let started = Instant::now();
    let mut samples = BackendSamples::default();
    let total_rounds = lane.warmup_rounds() + lane.measure_rounds();
    for round in 0..total_rounds {
        for backend in provider_order_for_round(round) {
            let sample = run_sample(backend).await?;
            if round >= lane.warmup_rounds() {
                samples.push(backend, sample);
            } else {
                black_box(sample);
            }
        }
    }
    eprintln!(
        "  finished {} lane for {} in {:?}",
        lane.label().to_lowercase(),
        workload.label(),
        started.elapsed()
    );
    Ok(samples)
}

pub(super) async fn quiesce_service(service: &Arc<Service>, context: &str) -> BenchResult<()> {
    let timeout_secs = read_u64_override("NEOVEX_BENCH_QUIESCE_TIMEOUT_SECS", QUIESCE_TIMEOUT_SECS);
    eprintln!(
        "  quiesce-start context={context} timeout={}s",
        timeout_secs
    );
    let started = Instant::now();
    if tokio::time::timeout(Duration::from_secs(timeout_secs), service.quiesce())
        .await
        .is_err()
    {
        return Err(format!("service quiesce timed out during {context}").into());
    }
    eprintln!(
        "  quiesce-finished context={context} total={:?}",
        started.elapsed()
    );
    Ok(())
}

pub(super) async fn open_embedded_service(
    data_dir: &Path,
    backend: EmbeddedProviderKind,
) -> BenchResult<Arc<Service>> {
    match encryption_mode() {
        EncryptionMode::Disabled => Ok(Arc::new(Service::new_with_embedded_provider(
            data_dir, backend,
        )?)),
        EncryptionMode::TempMasterKeyFile => {
            let key_path = super::common::write_benchmark_master_key(data_dir)?;
            let config = ServicePersistenceConfig::embedded(data_dir, backend)
                .with_local_encryption(LocalEncryptionConfig::Enabled(
                    LocalKeyProviderConfig::MasterKeyFile(MasterKeyFileConfig { path: key_path }),
                ));
            Ok(Arc::new(
                Service::new_with_persistence_config(config).await?,
            ))
        }
    }
}

pub(super) fn emit_cold_open_breakdown(
    workload: WorkloadKind,
    backend: EmbeddedProviderKind,
    service_bootstrap: Duration,
    first_operation: Duration,
) {
    if std::env::var_os("NEOVEX_BENCH_COLD_OPEN_BREAKDOWN").is_none() {
        return;
    }

    eprintln!(
        "cold-open-breakdown workload={} backend={} service_bootstrap={:?} first_operation={:?} total={:?}",
        workload.label(),
        provider_label(backend),
        service_bootstrap,
        first_operation,
        service_bootstrap + first_operation,
    );
}

pub(super) fn open_benchmark_sqlite_connection(
    sqlite_path: &Path,
    tenant_id: &TenantId,
) -> BenchResult<Connection> {
    let conn = Connection::open(sqlite_path)?;
    if matches!(encryption_mode(), EncryptionMode::TempMasterKeyFile) {
        let key_path = super::common::write_benchmark_master_key(sqlite_path)?;
        let provider = MasterKeyFileProvider::new(key_path)?;
        let logical_name = sqlite_path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or("sqlite benchmark path must have a UTF-8 filename")?;
        let subject = LocalKeySubject::sqlite_tenant(tenant_id.clone(), logical_name);
        let dek = resolve_database_encryption_key(
            sqlite_path,
            &provider,
            &subject,
            ManifestCipher::SqlCipher,
        )?;
        neovex_storage::sqlite::encryption::apply_encryption_key(&conn, &dek)?;
    }
    Ok(conn)
}

pub(super) fn capture_sqlite_query_plan<P>(
    sqlite_path: &Path,
    tenant_id: &TenantId,
    statement: &str,
    params: P,
) -> BenchResult<Vec<String>>
where
    P: rusqlite::Params,
{
    let conn = open_benchmark_sqlite_connection(sqlite_path, tenant_id)?;
    let explain = format!("EXPLAIN QUERY PLAN {statement}");
    let mut stmt = conn.prepare(explain.as_str())?;
    let mut rows = stmt.query(params)?;
    let mut detail_rows = Vec::new();
    while let Some(row) = rows.next()? {
        let select_id = row.get::<_, i64>(0)?;
        let parent_id = row.get::<_, i64>(1)?;
        let order = row.get::<_, i64>(2)?;
        let detail = row.get::<_, String>(3)?;
        detail_rows.push(format!("{select_id} | {parent_id} | {order} | {detail}"));
    }
    Ok(detail_rows)
}

pub(super) fn warm_sqlite_index_id_only(
    sqlite_path: &Path,
    tenant_id: &TenantId,
    status: &str,
) -> BenchResult<()> {
    let total_started = Instant::now();
    let open_started = Instant::now();
    let conn = open_benchmark_sqlite_connection(sqlite_path, tenant_id)?;
    let open_elapsed = open_started.elapsed();
    let query_started = Instant::now();
    let mut stmt = conn.prepare(
        "SELECT id
         FROM documents
         WHERE table_name = ?1 AND json_extract(data_json, '$.\"status\"') = ?2
         ORDER BY id
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![super::common::tasks_table().as_str(), status])?;
    if let Some(row) = rows.next()? {
        black_box(row.get::<_, String>(0)?);
    }
    let query_elapsed = query_started.elapsed();
    eprintln!(
        "sqlite-query-warmup-profile tenant={} mode=raw-id-only open={:?} query={:?} total={:?}",
        tenant_id,
        open_elapsed,
        query_elapsed,
        total_started.elapsed(),
    );
    Ok(())
}

#[derive(Debug)]
pub(super) struct BenchDir {
    path: PathBuf,
}

impl BenchDir {
    pub(super) fn new(label: &str, backend: EmbeddedProviderKind) -> BenchResult<Self> {
        let counter = BENCH_DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = env::temp_dir().join(format!(
            "neovex-storage-bench-{}-{}-{}-{}",
            label,
            provider_label(backend),
            std::process::id(),
            counter
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for BenchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug, Clone)]
pub(super) struct TenantState {
    pub(super) tenant_id: TenantId,
    pub(super) ids: Vec<DocumentId>,
}

fn provider_label(backend: EmbeddedProviderKind) -> &'static str {
    match backend {
        EmbeddedProviderKind::Redb => "redb",
        EmbeddedProviderKind::Sqlite => "sqlite",
    }
}

fn provider_order_for_round(round: usize) -> [EmbeddedProviderKind; 2] {
    if round.is_multiple_of(2) {
        [EmbeddedProviderKind::Redb, EmbeddedProviderKind::Sqlite]
    } else {
        [EmbeddedProviderKind::Sqlite, EmbeddedProviderKind::Redb]
    }
}

pub(super) fn clone_seeded_data_dir(
    source: &Path,
    label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<BenchDir> {
    let cloned = BenchDir::new(label, backend)?;
    copy_dir_all(source, cloned.path())?;
    Ok(cloned)
}

pub(super) fn tenant_store_path(
    data_dir: &Path,
    backend: EmbeddedProviderKind,
    tenant_id: &TenantId,
) -> PathBuf {
    data_dir.join(format!(
        "{}.{}",
        tenant_id.as_str(),
        backend.tenant_file_extension()
    ))
}
