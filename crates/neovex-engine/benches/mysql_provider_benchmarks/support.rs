use super::*;

pub(super) struct BenchDir {
    path: PathBuf,
}

impl BenchDir {
    pub(super) fn new(label: &str) -> BenchResult<Self> {
        let counter = BENCH_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = env::temp_dir().join(format!(
            "neovex-mysql-bench-{label}-{}-{counter}",
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
        let proxy = LatencyProxy::new(config.mysql_url.as_str(), config.rtt_delay).await?;
        Ok(Self {
            loopback_connection_string: config.mysql_url.clone(),
            injected_rtt_connection_string: proxy.connection_string().to_string(),
            _proxy: proxy,
        })
    }

    pub(super) fn connection_string(&self, backend: MeasuredBackend) -> Option<&str> {
        match backend {
            MeasuredBackend::Sqlite => None,
            MeasuredBackend::MySqlLoopback => Some(self.loopback_connection_string.as_str()),
            MeasuredBackend::MySqlInjectedRtt => Some(self.injected_rtt_connection_string.as_str()),
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
    mysql_loopback: Vec<Duration>,
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
        MeasuredBackend::MySqlLoopback,
        operations_per_sample,
        mysql_loopback,
    );
}

pub(super) fn record_rtt_measurements(
    report: &mut BenchmarkReport,
    workload: WorkloadKind,
    operations_per_sample: u64,
    mysql_loopback: Vec<Duration>,
    mysql_injected_rtt: Vec<Duration>,
) {
    report.push_measurement(
        workload,
        BenchmarkLane::RttSensitive,
        MeasuredBackend::MySqlLoopback,
        operations_per_sample,
        mysql_loopback,
    );
    report.push_measurement(
        workload,
        BenchmarkLane::RttSensitive,
        MeasuredBackend::MySqlInjectedRtt,
        operations_per_sample,
        mysql_injected_rtt,
    );
}

pub(super) async fn cleanup_mysql_provider(config: &MySqlProviderConfig) -> BenchResult<()> {
    benchmark_mysql_cleanup(
        "registered MySQL connection termination",
        terminate_benchmark_mysql_connections(config),
    )
    .await?;
    benchmark_mysql_cleanup("registered MySQL database cleanup", async {
        MySqlProvider::connect(config.clone())
            .await?
            .drop_provider_databases_for_test()
            .await
            .map_err(|error| -> Box<dyn std::error::Error> { Box::new(error) })
    })
    .await?;
    Ok(())
}

pub(super) async fn terminate_benchmark_mysql_connections(
    config: &MySqlProviderConfig,
) -> BenchResult<()> {
    let pool = Pool::new(Opts::from_url(config.connection_string.as_str())?);
    let mut conn = pool.get_conn().await?;
    let db_like = format!("{}%", config.tenant_database_prefix);
    let info_metadata = format!("%{}%", config.metadata_database);
    let info_tenant = format!("%{}%", config.tenant_database_prefix);
    let connection_ids = conn
        .exec_map::<u64, _, _, _, _>(
            "SELECT ID \
             FROM INFORMATION_SCHEMA.PROCESSLIST \
             WHERE ID <> CONNECTION_ID() \
               AND USER = SUBSTRING_INDEX(CURRENT_USER(), '@', 1) \
               AND ((DB = ?) OR (DB LIKE ?) OR (INFO IS NOT NULL AND (INFO LIKE ? OR INFO LIKE ?)))",
            (
                config.metadata_database.as_str(),
                db_like.as_str(),
                info_metadata.as_str(),
                info_tenant.as_str(),
            ),
            |id| id,
        )
        .await?;
    for connection_id in connection_ids {
        conn.query_drop(format!("KILL CONNECTION {connection_id}"))
            .await?;
    }
    conn.disconnect().await?;
    pool.disconnect().await?;
    Ok(())
}

pub(super) async fn benchmark_mysql_cleanup<F>(context: &str, future: F) -> BenchResult<()>
where
    F: std::future::Future<Output = BenchResult<()>>,
{
    match tokio::time::timeout(
        Duration::from_secs(BENCHMARK_MYSQL_CLEANUP_TIMEOUT_SECS),
        future,
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            eprintln!(
                "MySQL benchmark cleanup timed out during {context}; continuing with best-effort deferred cleanup"
            );
            Ok(())
        }
    }
}

pub(super) fn register_mysql_cleanup(config: &MySqlProviderConfig) {
    let queue = MYSQL_CLEANUP_QUEUE.get_or_init(|| StdMutex::new(Vec::new()));
    queue
        .lock()
        .expect("cleanup queue lock should not be poisoned")
        .push(config.clone());
}

pub(super) async fn cleanup_registered_mysql_providers() {
    let Some(queue) = MYSQL_CLEANUP_QUEUE.get() else {
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

    drained.sort_by(|left, right| left.metadata_database.cmp(&right.metadata_database));
    drained.dedup_by(|left, right| left.metadata_database == right.metadata_database);

    tokio::time::sleep(Duration::from_millis(250)).await;
    for config in drained {
        if let Err(error) = cleanup_mysql_provider(&config).await {
            eprintln!(
                "warning: failed to drop benchmark metadata database {}: {error}",
                config.metadata_database
            );
        }
    }
}

pub(super) fn mysql_service_config(
    control_dir: &Path,
    provider_config: &MySqlProviderConfig,
) -> ServicePersistenceConfig {
    ServicePersistenceConfig {
        tenant_provider: TenantProviderConfig {
            dialect: PersistenceDialect::MySql,
            topology: PersistenceTopology::ExternalPrimary,
            routing: TenantRoutingConfig::DatabasePerTenant {
                metadata_database: provider_config.metadata_database.clone(),
                tenant_database_prefix: provider_config.tenant_database_prefix.clone(),
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
    }
}

pub(super) fn benchmark_mysql_provider_config(
    label: &str,
    connection_string: &str,
    min_connections: Option<usize>,
    max_connections: Option<usize>,
) -> BenchResult<MySqlProviderConfig> {
    let counter = BENCH_COUNTER.fetch_add(1, Ordering::SeqCst);
    let label_slug = label
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(12)
        .collect::<String>()
        .to_lowercase();
    let metadata_database = format!("nvx_{}_{}_{counter:x}", label_slug, std::process::id());
    let prefix_base = format!("t_{}_{}_{counter:x}_", label_slug, std::process::id());
    let tenant_database_prefix = prefix_base.chars().take(24).collect::<String>();
    Ok(MySqlProviderConfig {
        connection_string: connection_string.to_string(),
        metadata_database,
        tenant_database_prefix,
        min_connections,
        max_connections,
    })
}

pub(super) fn proxied_connection_string(
    connection_string: &str,
) -> BenchResult<(String, u16, String)> {
    let config = Opts::from_url(connection_string)?;
    if config.socket().is_some() {
        return Err(
            "RTT-sensitive MySQL benchmarks require a TCP host; unix sockets are not supported"
                .into(),
        );
    }
    let host = config.ip_or_hostname().to_string();
    if host.is_empty() {
        return Err("MySQL benchmark connection string must specify an explicit TCP host".into());
    }
    let port = config.tcp_port();
    Ok((host, port, connection_string.to_string()))
}

pub(super) fn rewrite_connection_string_host_port(
    connection_string: &str,
    host: IpAddr,
    port: u16,
) -> BenchResult<String> {
    let scheme_index = connection_string
        .find("://")
        .ok_or("MySQL benchmark connection string must be a URL")?;
    let authority_start = scheme_index + 3;
    let authority_end = connection_string[authority_start..]
        .find(['/', '?', '#'])
        .map(|offset| authority_start + offset)
        .unwrap_or(connection_string.len());
    let authority = &connection_string[authority_start..authority_end];
    let suffix = &connection_string[authority_end..];
    let credentials = authority
        .rsplit_once('@')
        .map(|(credentials, _)| credentials);
    let host_text = match host {
        IpAddr::V4(addr) => addr.to_string(),
        IpAddr::V6(addr) => format!("[{addr}]"),
    };

    let mut rewritten = String::with_capacity(connection_string.len() + 16);
    rewritten.push_str(&connection_string[..authority_start]);
    if let Some(credentials) = credentials {
        rewritten.push_str(credentials);
        rewritten.push('@');
    }
    rewritten.push_str(&host_text);
    rewritten.push(':');
    rewritten.push_str(&port.to_string());
    rewritten.push_str(suffix);
    Ok(rewritten)
}
