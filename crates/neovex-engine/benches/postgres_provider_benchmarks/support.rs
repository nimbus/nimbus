use super::*;

pub(super) struct BenchDir {
    path: PathBuf,
}

impl BenchDir {
    pub(super) fn new(label: &str) -> BenchResult<Self> {
        let counter = BENCH_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = env::temp_dir().join(format!(
            "neovex-postgres-bench-{label}-{}-{counter}",
            std::process::id()
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

pub(super) struct BenchmarkEnvironment {
    pub(super) loopback_connection_string: String,
    pub(super) injected_rtt_connection_string: String,
    _proxy: LatencyProxy,
}

impl BenchmarkEnvironment {
    pub(super) async fn new(config: &BenchmarkConfig) -> BenchResult<Self> {
        let proxy = LatencyProxy::new(config.postgres_url.as_str(), config.rtt_delay).await?;
        Ok(Self {
            loopback_connection_string: config.postgres_url.clone(),
            injected_rtt_connection_string: proxy.connection_string().to_string(),
            _proxy: proxy,
        })
    }

    pub(super) fn connection_string(&self, backend: MeasuredBackend) -> Option<&str> {
        match backend {
            MeasuredBackend::Sqlite => None,
            MeasuredBackend::PostgresLoopback => Some(self.loopback_connection_string.as_str()),
            MeasuredBackend::PostgresInjectedRtt => {
                Some(self.injected_rtt_connection_string.as_str())
            }
        }
    }
}

struct LatencyProxy {
    connection_string: String,
    shutdown_tx: watch::Sender<bool>,
    accept_task: JoinHandle<()>,
}

impl LatencyProxy {
    async fn new(connection_string: &str, one_way_delay: Duration) -> BenchResult<Self> {
        let (target_host, target_port, proxied_connection_string) =
            proxied_connection_string(connection_string)?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let local_addr = listener.local_addr()?;
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let accept_task = tokio::spawn(run_latency_proxy(
            listener,
            target_host,
            target_port,
            one_way_delay,
            shutdown_rx,
        ));
        let connection_string = rewrite_connection_string_host_port(
            proxied_connection_string.as_str(),
            IpAddr::from([127, 0, 0, 1]),
            local_addr.port(),
        )?;
        Ok(Self {
            connection_string,
            shutdown_tx,
            accept_task,
        })
    }

    fn connection_string(&self) -> &str {
        &self.connection_string
    }
}

impl Drop for LatencyProxy {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        self.accept_task.abort();
    }
}

async fn run_latency_proxy(
    listener: TcpListener,
    target_host: String,
    target_port: u16,
    one_way_delay: Duration,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_ok() && *shutdown_rx.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let Ok((client_stream, _)) = accepted else {
                    break;
                };
                let target = format!("{target_host}:{target_port}");
                tokio::spawn(async move {
                    let Ok(server_stream) = TcpStream::connect(target).await else {
                        return;
                    };
                    let (mut client_read, mut client_write) = client_stream.into_split();
                    let (mut server_read, mut server_write) = server_stream.into_split();
                    let upstream = tokio::spawn(async move {
                        let _ = copy_with_delay(&mut client_read, &mut server_write, one_way_delay).await;
                    });
                    let downstream = tokio::spawn(async move {
                        let _ = copy_with_delay(&mut server_read, &mut client_write, one_way_delay).await;
                    });
                    let _ = upstream.await;
                    let _ = downstream.await;
                });
            }
        }
    }
}

async fn copy_with_delay<R, W>(
    reader: &mut R,
    writer: &mut W,
    delay: Duration,
) -> std::io::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            writer.shutdown().await?;
            return Ok(());
        }
        tokio::time::sleep(delay).await;
        writer.write_all(&buffer[..read]).await?;
        writer.flush().await?;
    }
}

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
    postgres_loopback: Vec<Duration>,
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
        MeasuredBackend::PostgresLoopback,
        operations_per_sample,
        postgres_loopback,
    );
}

pub(super) fn record_rtt_measurements(
    report: &mut BenchmarkReport,
    workload: WorkloadKind,
    operations_per_sample: u64,
    postgres_loopback: Vec<Duration>,
    postgres_injected_rtt: Vec<Duration>,
) {
    report.push_measurement(
        workload,
        BenchmarkLane::RttSensitive,
        MeasuredBackend::PostgresLoopback,
        operations_per_sample,
        postgres_loopback,
    );
    report.push_measurement(
        workload,
        BenchmarkLane::RttSensitive,
        MeasuredBackend::PostgresInjectedRtt,
        operations_per_sample,
        postgres_injected_rtt,
    );
}

pub(super) async fn cleanup_postgres_provider(config: &PostgresProviderConfig) -> BenchResult<()> {
    terminate_benchmark_postgres_connections(config).await?;
    PostgresProvider::connect(config.clone())
        .await?
        .drop_metadata_schema_for_test()
        .await?;
    Ok(())
}

pub(super) async fn terminate_benchmark_postgres_connections(
    config: &PostgresProviderConfig,
) -> BenchResult<()> {
    let pool_application_name = config.derived_pool_application_name()?;
    let notification_application_name = config.derived_notification_channel_name()?;
    let (client, connection) =
        tokio_postgres::connect(config.connection_string.as_str(), NoTls).await?;
    let connection_task = tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .execute(
            "SELECT pg_terminate_backend(pid)
             FROM pg_stat_activity
             WHERE pid <> pg_backend_pid()
               AND (application_name = $1 OR application_name = $2)",
            &[&pool_application_name, &notification_application_name],
        )
        .await?;
    connection_task.abort();
    Ok(())
}

pub(super) fn register_postgres_cleanup(config: &PostgresProviderConfig) {
    let queue = POSTGRES_CLEANUP_QUEUE.get_or_init(|| StdMutex::new(Vec::new()));
    queue
        .lock()
        .expect("cleanup queue lock should not be poisoned")
        .push(config.clone());
}

pub(super) async fn cleanup_registered_postgres_providers() {
    let Some(queue) = POSTGRES_CLEANUP_QUEUE.get() else {
        return;
    };
    let mut drained = {
        let mut configs = queue
            .lock()
            .expect("cleanup queue lock should not be poisoned");
        std::mem::take(&mut *configs)
    };

    if drained.is_empty() {
        return;
    }

    drained.sort_by(|left, right| left.metadata_schema.cmp(&right.metadata_schema));
    drained.dedup_by(|left, right| left.metadata_schema == right.metadata_schema);

    tokio::time::sleep(Duration::from_millis(250)).await;
    for config in drained {
        if let Err(error) = cleanup_postgres_provider(&config).await {
            eprintln!(
                "warning: failed to drop benchmark metadata schema {}: {error}",
                config.metadata_schema
            );
        }
    }
}

pub(super) fn postgres_service_config(
    control_dir: &Path,
    provider_config: &PostgresProviderConfig,
) -> ServicePersistenceConfig {
    ServicePersistenceConfig {
        tenant_provider: TenantProviderConfig {
            dialect: PersistenceDialect::Postgres,
            topology: PersistenceTopology::ExternalPrimary,
            routing: TenantRoutingConfig::SchemaPerTenant {
                metadata_schema: provider_config.metadata_schema.clone(),
                tenant_schema_prefix: provider_config.tenant_schema_prefix.clone(),
            },
            pool: PoolConfig {
                min_connections: provider_config.min_connections,
                max_connections: provider_config.max_connections,
            },
            credentials: ProviderCredentials::ConnectionString(
                provider_config.connection_string.clone(),
            ),
        },
        control_plane: ControlPlaneConfig::embedded_redb(control_dir),
        local_encryption: LocalEncryptionConfig::Disabled,
    }
}

pub(super) fn benchmark_postgres_provider_config(
    label: &str,
    connection_string: &str,
    min_connections: Option<usize>,
    max_connections: Option<usize>,
) -> BenchResult<PostgresProviderConfig> {
    let counter = BENCH_COUNTER.fetch_add(1, Ordering::SeqCst);
    let label_slug = label
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(12)
        .collect::<String>()
        .to_lowercase();
    let metadata_schema = format!("nvx_{}_{}_{counter:x}", label_slug, std::process::id());
    let prefix_base = format!("t_{}_{}_{counter:x}_", label_slug, std::process::id());
    let tenant_schema_prefix = prefix_base.chars().take(24).collect::<String>();
    Ok(PostgresProviderConfig {
        connection_string: connection_string.to_string(),
        metadata_schema,
        tenant_schema_prefix,
        min_connections,
        max_connections,
    })
}

pub(super) fn proxied_connection_string(
    connection_string: &str,
) -> BenchResult<(String, u16, String)> {
    let config = PostgresConfig::from_str(connection_string)?;
    let host = config
        .get_hosts()
        .first()
        .ok_or("Postgres benchmark connection string must specify an explicit TCP host")?;
    let host = match host {
        Host::Tcp(host) => host.clone(),
        #[cfg(unix)]
        Host::Unix(_) => return Err(
            "RTT-sensitive Postgres benchmarks require a TCP host; unix sockets are not supported"
                .into(),
        ),
    };
    let port = config.get_ports().first().copied().unwrap_or(5432);
    Ok((host, port, connection_string.to_string()))
}

pub(super) fn rewrite_connection_string_host_port(
    connection_string: &str,
    host: IpAddr,
    port: u16,
) -> BenchResult<String> {
    let config = PostgresConfig::from_str(connection_string)?;
    let mut parts = Vec::new();
    if let Some(user) = config.get_user() {
        parts.push(format!("user={}", quote_connection_value(user)));
    }
    if let Some(password) = config.get_password() {
        parts.push(format!(
            "password={}",
            quote_connection_value(String::from_utf8_lossy(password).as_ref())
        ));
    }
    if let Some(dbname) = config.get_dbname() {
        parts.push(format!("dbname={}", quote_connection_value(dbname)));
    }
    if let Some(options) = config.get_options() {
        parts.push(format!("options={}", quote_connection_value(options)));
    }
    if let Some(application_name) = config.get_application_name() {
        parts.push(format!(
            "application_name={}",
            quote_connection_value(application_name)
        ));
    }
    parts.push(format!(
        "sslmode={}",
        match config.get_ssl_mode() {
            tokio_postgres::config::SslMode::Disable => "disable",
            tokio_postgres::config::SslMode::Prefer => "prefer",
            tokio_postgres::config::SslMode::Require => "require",
            _ => "prefer",
        }
    ));
    parts.push(format!("host={host}"));
    parts.push(format!("port={port}"));
    Ok(parts.join(" "))
}

pub(super) fn quote_connection_value(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");
    format!("'{escaped}'")
}
