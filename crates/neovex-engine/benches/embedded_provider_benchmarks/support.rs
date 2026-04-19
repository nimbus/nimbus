use super::*;

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
    tokio::time::timeout(Duration::from_secs(QUIESCE_TIMEOUT_SECS), service.quiesce())
        .await
        .map_err(|_| format!("service quiesce timed out during {context}").into())
}

pub(super) fn capture_sqlite_query_plan<P>(
    sqlite_path: &Path,
    statement: &str,
    params: P,
) -> BenchResult<Vec<String>>
where
    P: rusqlite::Params,
{
    let conn = Connection::open(sqlite_path)?;
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
