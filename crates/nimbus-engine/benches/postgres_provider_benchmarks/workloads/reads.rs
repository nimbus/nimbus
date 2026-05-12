use super::*;

pub(crate) async fn benchmark_point_read_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::PointReadLatency, async {
        let sqlite_fixture = create_point_read_fixture(
            "point-read-steady",
            "point-read",
            MeasuredBackend::Sqlite,
            environment,
        )
        .await?;
        let postgres_fixture = create_point_read_fixture(
            "point-read-steady",
            "point-read",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            WorkloadKind::PointReadLatency,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let postgres_fixture = postgres_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::PostgresLoopback => postgres_fixture,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_point_read_sample(
                        &fixture.tenant.service,
                        &fixture.tenant.tenant_id,
                        &fixture.ids,
                        POINT_READ_BATCH_SIZE,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        sqlite_fixture
            .tenant
            .resource
            .cleanup(
                sqlite_fixture.tenant.service.clone(),
                "point-read steady-state sqlite teardown",
            )
            .await?;
        postgres_fixture
            .tenant
            .resource
            .cleanup(
                postgres_fixture.tenant.service.clone(),
                "point-read steady-state postgres teardown",
            )
            .await?;

        let sqlite_seed = freeze_point_read_seed(
            create_point_read_fixture(
                "point-read-cold-seed",
                "point-read",
                MeasuredBackend::Sqlite,
                environment,
            )
            .await?,
            "point-read cold-start sqlite seed freeze",
        )
        .await?;
        let postgres_seed = freeze_point_read_seed(
            create_point_read_fixture(
                "point-read-cold-seed",
                "point-read",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "point-read cold-start postgres seed freeze",
        )
        .await?;
        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            WorkloadKind::PointReadLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let postgres_seed = postgres_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::PostgresLoopback => postgres_seed,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("point-read-cold-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_point_read_sample(
                        &service,
                        &seed.tenant_id,
                        &seed.ids,
                        POINT_READ_BATCH_SIZE,
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "point-read cold-start reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        postgres_seed.resource.cleanup_seed().await?;

        let loopback_seed = freeze_point_read_seed(
            create_point_read_fixture(
                "point-read-rtt-loopback-seed",
                "point-read-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "point-read RTT loopback seed freeze",
        )
        .await?;
        let rtt_seed = freeze_point_read_seed(
            create_point_read_fixture(
                "point-read-rtt-injected-seed",
                "point-read-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "point-read RTT injected seed freeze",
        )
        .await?;
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            WorkloadKind::PointReadLatency,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::PostgresLoopback,
                MeasuredBackend::PostgresInjectedRtt,
            ],
            |backend| {
                let loopback_seed = loopback_seed.clone();
                let rtt_seed = rtt_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::PostgresLoopback => loopback_seed,
                        MeasuredBackend::PostgresInjectedRtt => rtt_seed,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("point-read-rtt-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_point_read_sample(
                        &service,
                        &seed.tenant_id,
                        &seed.ids,
                        POINT_READ_RTT_BATCH_SIZE,
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "point-read RTT reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        loopback_seed.resource.cleanup_seed().await?;
        rtt_seed.resource.cleanup_seed().await?;

        let operations_per_sample = u64::try_from(POINT_READ_BATCH_SIZE)?;
        let rtt_operations_per_sample = u64::try_from(POINT_READ_RTT_BATCH_SIZE)?;
        record_contrast_measurements(
            report,
            WorkloadKind::PointReadLatency,
            BenchmarkLane::SteadyState,
            operations_per_sample,
            sqlite_steady,
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::PointReadLatency,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::PointReadLatency,
            rtt_operations_per_sample,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        Ok(())
    })
    .await
}

pub(crate) async fn benchmark_indexed_query_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    benchmark_query_workload(
        report,
        environment,
        WorkloadKind::IndexedQueryLatency,
        || async move {
            create_indexed_query_fixture(
                "indexed-query",
                "indexed-query",
                MeasuredBackend::Sqlite,
                environment,
            )
            .await
        },
        || async move {
            create_indexed_query_fixture(
                "indexed-query",
                "indexed-query",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await
        },
        || async move {
            create_indexed_query_fixture(
                "indexed-query-rtt-loopback-seed",
                "indexed-query-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await
        },
        || async move {
            create_indexed_query_fixture(
                "indexed-query-rtt-injected-seed",
                "indexed-query-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await
        },
    )
    .await
}

pub(crate) async fn benchmark_composite_indexed_query_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    benchmark_query_workload(
        report,
        environment,
        WorkloadKind::CompositeIndexedQueryLatency,
        || async move {
            create_composite_query_fixture(
                "composite-query",
                "composite-query",
                MeasuredBackend::Sqlite,
                environment,
            )
            .await
        },
        || async move {
            create_composite_query_fixture(
                "composite-query",
                "composite-query",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await
        },
        || async move {
            create_composite_query_fixture(
                "composite-query-rtt-loopback-seed",
                "composite-query-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await
        },
        || async move {
            create_composite_query_fixture(
                "composite-query-rtt-injected-seed",
                "composite-query-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await
        },
    )
    .await
}

async fn benchmark_query_workload<F1, F2, F3, F4, Fut1, Fut2, Fut3, Fut4>(
    report: &mut BenchmarkReport,
    environment: &BenchmarkEnvironment,
    workload: WorkloadKind,
    sqlite_builder: F1,
    postgres_builder: F2,
    loopback_rtt_builder: F3,
    injected_rtt_builder: F4,
) -> BenchResult<()>
where
    F1: Fn() -> Fut1,
    F2: Fn() -> Fut2,
    F3: Fn() -> Fut3,
    F4: Fn() -> Fut4,
    Fut1: std::future::Future<Output = BenchResult<QueryFixture>>,
    Fut2: std::future::Future<Output = BenchResult<QueryFixture>>,
    Fut3: std::future::Future<Output = BenchResult<QueryFixture>>,
    Fut4: std::future::Future<Output = BenchResult<QueryFixture>>,
{
    run_workload(workload, async move {
        let sqlite_fixture = sqlite_builder().await?;
        let postgres_fixture = postgres_builder().await?;
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            workload,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let postgres_fixture = postgres_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::PostgresLoopback => postgres_fixture,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_query_sample(
                        &fixture.tenant.service,
                        &fixture.tenant.tenant_id,
                        &fixture.query,
                        INDEXED_QUERY_BATCH_SIZE,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        sqlite_fixture
            .tenant
            .resource
            .cleanup(
                sqlite_fixture.tenant.service.clone(),
                "query steady-state sqlite teardown",
            )
            .await?;
        postgres_fixture
            .tenant
            .resource
            .cleanup(
                postgres_fixture.tenant.service.clone(),
                "query steady-state postgres teardown",
            )
            .await?;

        let sqlite_seed =
            freeze_query_seed(sqlite_builder().await?, "query cold-start sqlite seed").await?;
        let postgres_seed =
            freeze_query_seed(postgres_builder().await?, "query cold-start postgres seed").await?;
        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            workload,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let postgres_seed = postgres_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::PostgresLoopback => postgres_seed,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("query-cold-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_query_sample(
                        &service,
                        &seed.tenant_id,
                        &seed.query,
                        INDEXED_QUERY_BATCH_SIZE,
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "query cold-start reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        postgres_seed.resource.cleanup_seed().await?;

        let loopback_seed =
            freeze_query_seed(loopback_rtt_builder().await?, "query RTT loopback seed").await?;
        let rtt_seed =
            freeze_query_seed(injected_rtt_builder().await?, "query RTT injected seed").await?;
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            workload,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::PostgresLoopback,
                MeasuredBackend::PostgresInjectedRtt,
            ],
            |backend| {
                let loopback_seed = loopback_seed.clone();
                let rtt_seed = rtt_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::PostgresLoopback => loopback_seed,
                        MeasuredBackend::PostgresInjectedRtt => rtt_seed,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("query-rtt-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_query_sample(
                        &service,
                        &seed.tenant_id,
                        &seed.query,
                        INDEXED_QUERY_RTT_BATCH_SIZE,
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "query RTT reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        loopback_seed.resource.cleanup_seed().await?;
        rtt_seed.resource.cleanup_seed().await?;

        let operations_per_sample = u64::try_from(INDEXED_QUERY_BATCH_SIZE)?;
        let rtt_operations_per_sample = u64::try_from(INDEXED_QUERY_RTT_BATCH_SIZE)?;
        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::SteadyState,
            operations_per_sample,
            sqlite_steady,
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            workload,
            rtt_operations_per_sample,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        Ok(())
    })
    .await
}
