use super::config::{BenchmarkEnvironment, BenchmarkLane, WorkloadKind};
use super::models::{BenchmarkReport, MeasuredBackend};
use super::*;

pub(super) async fn run_workload<Fut>(workload: WorkloadKind, run: Fut) -> BenchResult<()>
where
    Fut: std::future::Future<Output = BenchResult<()>>,
{
    eprintln!("starting {}", workload.label());
    let started = Instant::now();
    let result = run.await;
    eprintln!("finished {} in {:?}", workload.label(), started.elapsed());
    result
}

pub(super) async fn measure_two_backends_async<B, F, Fut>(
    workload: WorkloadKind,
    lane: BenchmarkLane,
    backends: [B; 2],
    mut run_sample: F,
) -> BenchResult<(Vec<Duration>, Vec<Duration>)>
where
    B: Copy + Eq,
    F: FnMut(B) -> Fut,
    Fut: std::future::Future<Output = BenchResult<Duration>>,
{
    eprintln!("  starting {} lane", lane.label().to_lowercase());
    let started = Instant::now();
    let total_rounds = lane.warmup_rounds() + lane.measure_rounds();
    let mut first = Vec::new();
    let mut second = Vec::new();
    for round in 0..total_rounds {
        let order = if round.is_multiple_of(2) {
            backends
        } else {
            [backends[1], backends[0]]
        };
        for backend in order {
            let sample = run_sample(backend).await?;
            if round >= lane.warmup_rounds() {
                if backend == backends[0] {
                    first.push(sample);
                } else {
                    second.push(sample);
                }
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
    Ok((first, second))
}

pub(super) async fn measure_single_backend_async<F, Fut>(
    workload: WorkloadKind,
    lane: BenchmarkLane,
    mut run_sample: F,
) -> BenchResult<Vec<Duration>>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = BenchResult<Duration>>,
{
    eprintln!("  starting {} lane", lane.label().to_lowercase());
    let started = Instant::now();
    let total_rounds = lane.warmup_rounds() + lane.measure_rounds();
    let mut samples = Vec::new();
    for round in 0..total_rounds {
        let sample = run_sample().await?;
        if round >= lane.warmup_rounds() {
            samples.push(sample);
        } else {
            black_box(sample);
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
    match tokio::time::timeout(
        Duration::from_secs(BENCHMARK_QUIESCE_TIMEOUT_SECS),
        service.quiesce(),
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(_) => {
            eprintln!(
                "graceful service quiesce timed out during {context}; falling back to drop-based benchmark teardown"
            );
            Ok(())
        }
    }
}

pub(super) fn record_contrast_measurements(
    report: &mut BenchmarkReport,
    workload: WorkloadKind,
    lane: BenchmarkLane,
    operations_per_sample: u64,
    sqlite: Vec<Duration>,
    replica: Vec<Duration>,
) {
    report.push_measurement(
        workload,
        lane,
        MeasuredBackend::Sqlite,
        operations_per_sample,
        sqlite,
    );
    report.push_measurement(
        workload,
        lane,
        MeasuredBackend::LibsqlReplica,
        operations_per_sample,
        replica,
    );
}

pub(super) fn libsql_replica_service_config(
    control_dir: &Path,
    provider_config: &LibsqlReplicaProviderConfig,
) -> ServicePersistenceConfig {
    ServicePersistenceConfig {
        tenant_provider: TenantProviderConfig {
            dialect: PersistenceDialect::Sqlite,
            topology: PersistenceTopology::ExternalPrimaryWithReplicas,
            routing: TenantRoutingConfig::NamespacePerTenant {
                metadata_namespace: provider_config.metadata_namespace.clone(),
                tenant_namespace_prefix: provider_config.tenant_namespace_prefix.clone(),
                replica_cache_dir: provider_config.replica_cache_dir.clone(),
            },
            pool: PoolConfig::default(),
            credentials: ProviderCredentials::LibsqlReplica {
                primary_url: provider_config.primary_url.clone(),
                auth_token: provider_config.auth_token.clone(),
                admin_api_url: provider_config.admin_api_url.clone(),
                admin_auth_header: provider_config.admin_auth_header.clone(),
            },
        },
        control_plane: ControlPlaneConfig::embedded_redb(control_dir),
    }
}

pub(super) fn benchmark_libsql_provider_config(
    label: &str,
    environment: &BenchmarkEnvironment,
    replica_cache_dir: &Path,
) -> LibsqlReplicaProviderConfig {
    let counter = BENCH_COUNTER.fetch_add(1, Ordering::SeqCst);
    let label_slug = slugify_label(label, 12);
    let metadata_namespace = format!("nvx_{}_{}_{counter:x}", label_slug, std::process::id());
    let tenant_namespace_prefix = format!("t_{}_{}_{counter:x}_", label_slug, std::process::id());
    LibsqlReplicaProviderConfig {
        primary_url: environment.primary_url.clone(),
        auth_token: environment.auth_token.clone(),
        admin_api_url: environment.admin_api_url.clone(),
        admin_auth_header: environment.admin_auth_header.clone(),
        metadata_namespace,
        tenant_namespace_prefix,
        replica_cache_dir: replica_cache_dir.to_path_buf(),
    }
}

pub(super) fn slugify_label(label: &str, limit: usize) -> String {
    let slug = label
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(limit)
        .collect::<String>()
        .to_lowercase();
    if slug.is_empty() {
        "bench".to_string()
    } else {
        slug
    }
}

pub(super) fn register_libsql_replica_cleanup(config: &LibsqlReplicaProviderConfig) {
    let queue = REPLICA_CLEANUP_QUEUE.get_or_init(|| StdMutex::new(Vec::new()));
    let mut queue = queue
        .lock()
        .expect("replica benchmark cleanup queue should not be poisoned");
    if queue.iter().any(|existing| {
        existing.primary_url == config.primary_url
            && existing.admin_api_url == config.admin_api_url
            && existing.metadata_namespace == config.metadata_namespace
            && existing.tenant_namespace_prefix == config.tenant_namespace_prefix
    }) {
        return;
    }
    queue.push(config.clone());
}

async fn cleanup_libsql_replica_provider(config: &LibsqlReplicaProviderConfig) -> BenchResult<()> {
    LibsqlReplicaProvider::connect(config.clone())
        .await?
        .drop_provider_namespaces_for_test()
        .await?;
    Ok(())
}

pub(super) async fn cleanup_registered_libsql_replica_providers() {
    let Some(queue) = REPLICA_CLEANUP_QUEUE.get() else {
        return;
    };
    let drained = {
        let mut guard = queue
            .lock()
            .expect("replica benchmark cleanup queue should not be poisoned");
        guard.drain(..).collect::<Vec<_>>()
    };
    for config in drained {
        if let Err(error) = cleanup_libsql_replica_provider(&config).await {
            eprintln!(
                "warning: failed to drop benchmark libsql metadata namespace {}: {error}",
                config.metadata_namespace
            );
        }
    }
}
