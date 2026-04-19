use super::*;

pub(super) async fn benchmark_crud_throughput(
    config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::CrudThroughput, async {
        let sqlite_fixture =
            create_crud_fixture("crud-steady", "crud", MeasuredBackend::Sqlite, environment)
                .await?;
        let mysql_fixture = create_crud_fixture(
            "crud-steady",
            "crud",
            MeasuredBackend::MySqlLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, mysql_steady) = measure_two_backends_async(
            WorkloadKind::CrudThroughput,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let mysql_fixture = mysql_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::MySqlLoopback => mysql_fixture,
                        MeasuredBackend::MySqlInjectedRtt => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_crud_sample(&fixture.service, &fixture.tenant_id, CRUD_DOCUMENTS)
                        .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        sqlite_fixture
            .resource
            .cleanup(
                sqlite_fixture.service.clone(),
                "CRUD steady-state sqlite teardown",
            )
            .await?;
        mysql_fixture
            .resource
            .cleanup(
                mysql_fixture.service.clone(),
                "CRUD steady-state mysql teardown",
            )
            .await?;

        let (sqlite_cold, mysql_cold) = measure_two_backends_async(
            WorkloadKind::CrudThroughput,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| async move {
                let fixture =
                    create_crud_fixture("crud-cold", "crud", backend, environment).await?;
                let started = Instant::now();
                exercise_crud_sample(&fixture.service, &fixture.tenant_id, CRUD_DOCUMENTS).await?;
                let elapsed = started.elapsed();
                fixture
                    .resource
                    .cleanup(fixture.service.clone(), "CRUD cold-start teardown")
                    .await?;
                Ok(elapsed)
            },
        )
        .await?;

        let loopback_fixture = create_crud_fixture(
            "crud-rtt-loopback",
            "crud-rtt",
            MeasuredBackend::MySqlLoopback,
            environment,
        )
        .await?;
        let rtt_fixture = create_crud_fixture(
            "crud-rtt-injected",
            "crud-rtt",
            MeasuredBackend::MySqlInjectedRtt,
            environment,
        )
        .await?;
        let (mysql_loopback_rtt, mysql_injected_rtt) = measure_two_backends_async(
            WorkloadKind::CrudThroughput,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::MySqlLoopback,
                MeasuredBackend::MySqlInjectedRtt,
            ],
            |backend| {
                let loopback_fixture = loopback_fixture.clone();
                let rtt_fixture = rtt_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::MySqlLoopback => loopback_fixture,
                        MeasuredBackend::MySqlInjectedRtt => rtt_fixture,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_crud_sample(&fixture.service, &fixture.tenant_id, CRUD_RTT_DOCUMENTS)
                        .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        loopback_fixture
            .resource
            .cleanup(
                loopback_fixture.service.clone(),
                "CRUD RTT loopback teardown",
            )
            .await?;
        rtt_fixture
            .resource
            .cleanup(rtt_fixture.service.clone(), "CRUD RTT injected teardown")
            .await?;

        let operations_per_sample = u64::try_from(CRUD_DOCUMENTS * 3)?;
        let rtt_operations_per_sample = u64::try_from(CRUD_RTT_DOCUMENTS * 3)?;
        record_contrast_measurements(
            report,
            WorkloadKind::CrudThroughput,
            BenchmarkLane::SteadyState,
            operations_per_sample,
            sqlite_steady,
            mysql_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::CrudThroughput,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            mysql_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::CrudThroughput,
            rtt_operations_per_sample,
            mysql_loopback_rtt,
            mysql_injected_rtt,
        );
        let _ = config;
        Ok(())
    })
    .await
}

pub(super) async fn benchmark_point_read_latency(
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
        let mysql_fixture = create_point_read_fixture(
            "point-read-steady",
            "point-read",
            MeasuredBackend::MySqlLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, mysql_steady) = measure_two_backends_async(
            WorkloadKind::PointReadLatency,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let mysql_fixture = mysql_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::MySqlLoopback => mysql_fixture,
                        MeasuredBackend::MySqlInjectedRtt => unreachable!(),
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
        mysql_fixture
            .tenant
            .resource
            .cleanup(
                mysql_fixture.tenant.service.clone(),
                "point-read steady-state mysql teardown",
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
        let mysql_seed = freeze_point_read_seed(
            create_point_read_fixture(
                "point-read-cold-seed",
                "point-read",
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await?,
            "point-read cold-start mysql seed freeze",
        )
        .await?;
        let (sqlite_cold, mysql_cold) = measure_two_backends_async(
            WorkloadKind::PointReadLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let mysql_seed = mysql_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::MySqlLoopback => mysql_seed,
                        MeasuredBackend::MySqlInjectedRtt => unreachable!(),
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
        mysql_seed.resource.cleanup_seed().await?;

        let loopback_seed = freeze_point_read_seed(
            create_point_read_fixture(
                "point-read-rtt-loopback-seed",
                "point-read-rtt",
                MeasuredBackend::MySqlLoopback,
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
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await?,
            "point-read RTT injected seed freeze",
        )
        .await?;
        let (mysql_loopback_rtt, mysql_injected_rtt) = measure_two_backends_async(
            WorkloadKind::PointReadLatency,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::MySqlLoopback,
                MeasuredBackend::MySqlInjectedRtt,
            ],
            |backend| {
                let loopback_seed = loopback_seed.clone();
                let rtt_seed = rtt_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::MySqlLoopback => loopback_seed,
                        MeasuredBackend::MySqlInjectedRtt => rtt_seed,
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
            mysql_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::PointReadLatency,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            mysql_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::PointReadLatency,
            rtt_operations_per_sample,
            mysql_loopback_rtt,
            mysql_injected_rtt,
        );
        Ok(())
    })
    .await
}

pub(super) async fn benchmark_indexed_query_latency(
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
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await
        },
        || async move {
            create_indexed_query_fixture(
                "indexed-query-rtt-loopback-seed",
                "indexed-query-rtt",
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await
        },
        || async move {
            create_indexed_query_fixture(
                "indexed-query-rtt-injected-seed",
                "indexed-query-rtt",
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await
        },
    )
    .await
}

pub(super) async fn benchmark_composite_indexed_query_latency(
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
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await
        },
        || async move {
            create_composite_query_fixture(
                "composite-query-rtt-loopback-seed",
                "composite-query-rtt",
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await
        },
        || async move {
            create_composite_query_fixture(
                "composite-query-rtt-injected-seed",
                "composite-query-rtt",
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await
        },
    )
    .await
}

pub(super) async fn benchmark_query_workload<F1, F2, F3, F4, Fut1, Fut2, Fut3, Fut4>(
    report: &mut BenchmarkReport,
    environment: &BenchmarkEnvironment,
    workload: WorkloadKind,
    sqlite_builder: F1,
    mysql_builder: F2,
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
        let mysql_fixture = mysql_builder().await?;
        let (sqlite_steady, mysql_steady) = measure_two_backends_async(
            workload,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let mysql_fixture = mysql_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::MySqlLoopback => mysql_fixture,
                        MeasuredBackend::MySqlInjectedRtt => unreachable!(),
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
        mysql_fixture
            .tenant
            .resource
            .cleanup(
                mysql_fixture.tenant.service.clone(),
                "query steady-state mysql teardown",
            )
            .await?;

        let sqlite_seed =
            freeze_query_seed(sqlite_builder().await?, "query cold-start sqlite seed").await?;
        let mysql_seed =
            freeze_query_seed(mysql_builder().await?, "query cold-start mysql seed").await?;
        let (sqlite_cold, mysql_cold) = measure_two_backends_async(
            workload,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let mysql_seed = mysql_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::MySqlLoopback => mysql_seed,
                        MeasuredBackend::MySqlInjectedRtt => unreachable!(),
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
        mysql_seed.resource.cleanup_seed().await?;

        let loopback_seed =
            freeze_query_seed(loopback_rtt_builder().await?, "query RTT loopback seed").await?;
        let rtt_seed =
            freeze_query_seed(injected_rtt_builder().await?, "query RTT injected seed").await?;
        let (mysql_loopback_rtt, mysql_injected_rtt) = measure_two_backends_async(
            workload,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::MySqlLoopback,
                MeasuredBackend::MySqlInjectedRtt,
            ],
            |backend| {
                let loopback_seed = loopback_seed.clone();
                let rtt_seed = rtt_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::MySqlLoopback => loopback_seed,
                        MeasuredBackend::MySqlInjectedRtt => rtt_seed,
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
            mysql_steady,
        );
        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            mysql_cold,
        );
        record_rtt_measurements(
            report,
            workload,
            rtt_operations_per_sample,
            mysql_loopback_rtt,
            mysql_injected_rtt,
        );
        Ok(())
    })
    .await
}

pub(super) async fn benchmark_durable_journal_stream_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    benchmark_journal_workload(
        report,
        environment,
        WorkloadKind::DurableJournalStreamLatency,
    )
    .await
}

pub(super) async fn benchmark_durable_journal_bootstrap_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    benchmark_journal_workload(
        report,
        environment,
        WorkloadKind::DurableJournalBootstrapLatency,
    )
    .await
}

pub(super) async fn benchmark_journal_workload(
    report: &mut BenchmarkReport,
    environment: &BenchmarkEnvironment,
    workload: WorkloadKind,
) -> BenchResult<()> {
    run_workload(workload, async move {
        let sqlite_fixture = create_journal_fixture(
            "journal-steady",
            "journal",
            MeasuredBackend::Sqlite,
            environment,
        )
        .await?;
        let mysql_fixture = create_journal_fixture(
            "journal-steady",
            "journal",
            MeasuredBackend::MySqlLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, mysql_steady) = measure_two_backends_async(
            workload,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let mysql_fixture = mysql_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::MySqlLoopback => mysql_fixture,
                        MeasuredBackend::MySqlInjectedRtt => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_journal_workload_sample(
                        workload,
                        &fixture.tenant.service,
                        &fixture.tenant.tenant_id,
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
                "journal steady-state sqlite teardown",
            )
            .await?;
        mysql_fixture
            .tenant
            .resource
            .cleanup(
                mysql_fixture.tenant.service.clone(),
                "journal steady-state mysql teardown",
            )
            .await?;

        let sqlite_seed = freeze_journal_seed(
            create_journal_fixture(
                "journal-cold-seed",
                "journal",
                MeasuredBackend::Sqlite,
                environment,
            )
            .await?,
            "journal cold-start sqlite seed",
        )
        .await?;
        let mysql_seed = freeze_journal_seed(
            create_journal_fixture(
                "journal-cold-seed",
                "journal",
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await?,
            "journal cold-start mysql seed",
        )
        .await?;
        let (sqlite_cold, mysql_cold) = measure_two_backends_async(
            workload,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let mysql_seed = mysql_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::MySqlLoopback => mysql_seed,
                        MeasuredBackend::MySqlInjectedRtt => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("journal-cold-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_journal_workload_sample(workload, &service, &seed.tenant_id).await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "journal cold-start reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        mysql_seed.resource.cleanup_seed().await?;

        let loopback_seed = freeze_journal_seed(
            create_journal_fixture(
                "journal-rtt-loopback-seed",
                "journal-rtt",
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await?,
            "journal RTT loopback seed freeze",
        )
        .await?;
        let rtt_seed = freeze_journal_seed(
            create_journal_fixture(
                "journal-rtt-injected-seed",
                "journal-rtt",
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await?,
            "journal RTT injected seed freeze",
        )
        .await?;
        let (mysql_loopback_rtt, mysql_injected_rtt) = measure_two_backends_async(
            workload,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::MySqlLoopback,
                MeasuredBackend::MySqlInjectedRtt,
            ],
            |backend| {
                let loopback_seed = loopback_seed.clone();
                let rtt_seed = rtt_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::MySqlLoopback => loopback_seed,
                        MeasuredBackend::MySqlInjectedRtt => rtt_seed,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("journal-rtt-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_journal_workload_sample(workload, &service, &seed.tenant_id).await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "journal RTT reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        loopback_seed.resource.cleanup_seed().await?;
        rtt_seed.resource.cleanup_seed().await?;

        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::SteadyState,
            1,
            sqlite_steady,
            mysql_steady,
        );
        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::ColdStart,
            1,
            sqlite_cold,
            mysql_cold,
        );
        record_rtt_measurements(report, workload, 1, mysql_loopback_rtt, mysql_injected_rtt);
        Ok(())
    })
    .await
}

pub(super) async fn benchmark_subscription_bootstrap_catchup_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::SubscriptionBootstrapCatchupLatency, async {
        let sqlite_fixture = create_tenant_service(
            "subscription-bootstrap-steady",
            "subscription-bootstrap",
            MeasuredBackend::Sqlite,
            environment,
        )
        .await?;
        let mysql_fixture = create_tenant_service(
            "subscription-bootstrap-steady",
            "subscription-bootstrap",
            MeasuredBackend::MySqlLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, mysql_steady) = measure_two_backends_async(
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let mysql_fixture = mysql_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::MySqlLoopback => mysql_fixture,
                        MeasuredBackend::MySqlInjectedRtt => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_subscription_bootstrap_catchup_sample(
                        &fixture.service,
                        &fixture.tenant_id,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        sqlite_fixture
            .resource
            .cleanup(
                sqlite_fixture.service.clone(),
                "subscription bootstrap steady-state sqlite teardown",
            )
            .await?;
        mysql_fixture
            .resource
            .cleanup(
                mysql_fixture.service.clone(),
                "subscription bootstrap steady-state mysql teardown",
            )
            .await?;

        let (sqlite_cold, mysql_cold) = measure_two_backends_async(
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| async move {
                let fixture = create_tenant_service(
                    "subscription-bootstrap-cold",
                    "subscription-bootstrap",
                    backend,
                    environment,
                )
                .await?;
                let started = Instant::now();
                exercise_subscription_bootstrap_catchup_sample(
                    &fixture.service,
                    &fixture.tenant_id,
                )
                .await?;
                let elapsed = started.elapsed();
                fixture
                    .resource
                    .cleanup(
                        fixture.service.clone(),
                        "subscription bootstrap cold-start teardown",
                    )
                    .await?;
                Ok(elapsed)
            },
        )
        .await?;

        let loopback_fixture = create_tenant_service(
            "subscription-bootstrap-rtt",
            "subscription-bootstrap-rtt",
            MeasuredBackend::MySqlLoopback,
            environment,
        )
        .await?;
        let rtt_fixture = create_tenant_service(
            "subscription-bootstrap-rtt",
            "subscription-bootstrap-rtt",
            MeasuredBackend::MySqlInjectedRtt,
            environment,
        )
        .await?;
        let (mysql_loopback_rtt, mysql_injected_rtt) = measure_two_backends_async(
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::MySqlLoopback,
                MeasuredBackend::MySqlInjectedRtt,
            ],
            |backend| {
                let loopback_fixture = loopback_fixture.clone();
                let rtt_fixture = rtt_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::MySqlLoopback => loopback_fixture,
                        MeasuredBackend::MySqlInjectedRtt => rtt_fixture,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_subscription_bootstrap_catchup_sample(
                        &fixture.service,
                        &fixture.tenant_id,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        loopback_fixture
            .resource
            .cleanup(
                loopback_fixture.service.clone(),
                "subscription bootstrap RTT loopback teardown",
            )
            .await?;
        rtt_fixture
            .resource
            .cleanup(
                rtt_fixture.service.clone(),
                "subscription bootstrap RTT injected teardown",
            )
            .await?;

        record_contrast_measurements(
            report,
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            BenchmarkLane::SteadyState,
            1,
            sqlite_steady,
            mysql_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            BenchmarkLane::ColdStart,
            1,
            sqlite_cold,
            mysql_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            1,
            mysql_loopback_rtt,
            mysql_injected_rtt,
        );
        Ok(())
    })
    .await
}

pub(super) async fn benchmark_subscription_fanout_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::SubscriptionFanoutLatency, async {
        let sqlite_fixture = Arc::new(Mutex::new(
            create_subscription_fixture(
                "subscription-fanout-steady",
                "subscription-fanout",
                MeasuredBackend::Sqlite,
                environment,
            )
            .await?,
        ));
        let mysql_fixture = Arc::new(Mutex::new(
            create_subscription_fixture(
                "subscription-fanout-steady",
                "subscription-fanout",
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await?,
        ));
        let (sqlite_steady, mysql_steady) = measure_two_backends_async(
            WorkloadKind::SubscriptionFanoutLatency,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let mysql_fixture = mysql_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::MySqlLoopback => mysql_fixture,
                        MeasuredBackend::MySqlInjectedRtt => unreachable!(),
                    };
                    let mut fixture = fixture.lock().await;
                    let service = fixture.tenant.service.clone();
                    let tenant_id = fixture.tenant.tenant_id.clone();
                    let started = Instant::now();
                    exercise_subscription_fanout_sample(
                        &service,
                        &tenant_id,
                        &mut fixture.receivers,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        {
            let fixture = sqlite_fixture.lock().await;
            black_box(fixture.registrations.len());
            fixture
                .tenant
                .resource
                .cleanup(
                    fixture.tenant.service.clone(),
                    "subscription fanout steady-state sqlite teardown",
                )
                .await?;
        }
        {
            let fixture = mysql_fixture.lock().await;
            black_box(fixture.registrations.len());
            fixture
                .tenant
                .resource
                .cleanup(
                    fixture.tenant.service.clone(),
                    "subscription fanout steady-state mysql teardown",
                )
                .await?;
        }

        let (sqlite_cold, mysql_cold) = measure_two_backends_async(
            WorkloadKind::SubscriptionFanoutLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| async move {
                let fixture = create_subscription_fixture(
                    "subscription-fanout-cold",
                    "subscription-fanout",
                    backend,
                    environment,
                )
                .await?;
                let mut receivers = fixture.receivers;
                let registrations = fixture.registrations;
                let started = Instant::now();
                exercise_subscription_fanout_sample(
                    &fixture.tenant.service,
                    &fixture.tenant.tenant_id,
                    &mut receivers,
                )
                .await?;
                let elapsed = started.elapsed();
                drop(registrations);
                fixture
                    .tenant
                    .resource
                    .cleanup(
                        fixture.tenant.service.clone(),
                        "subscription fanout cold-start teardown",
                    )
                    .await?;
                Ok(elapsed)
            },
        )
        .await?;

        let loopback_fixture = Arc::new(Mutex::new(
            create_subscription_fixture(
                "subscription-fanout-rtt",
                "subscription-fanout-rtt",
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await?,
        ));
        let rtt_fixture = Arc::new(Mutex::new(
            create_subscription_fixture(
                "subscription-fanout-rtt",
                "subscription-fanout-rtt",
                MeasuredBackend::MySqlInjectedRtt,
                environment,
            )
            .await?,
        ));
        let (mysql_loopback_rtt, mysql_injected_rtt) = measure_two_backends_async(
            WorkloadKind::SubscriptionFanoutLatency,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::MySqlLoopback,
                MeasuredBackend::MySqlInjectedRtt,
            ],
            |backend| {
                let loopback_fixture = loopback_fixture.clone();
                let rtt_fixture = rtt_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::MySqlLoopback => loopback_fixture,
                        MeasuredBackend::MySqlInjectedRtt => rtt_fixture,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let mut fixture = fixture.lock().await;
                    let service = fixture.tenant.service.clone();
                    let tenant_id = fixture.tenant.tenant_id.clone();
                    let started = Instant::now();
                    exercise_subscription_fanout_sample(
                        &service,
                        &tenant_id,
                        &mut fixture.receivers,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        {
            let fixture = loopback_fixture.lock().await;
            black_box(fixture.registrations.len());
            fixture
                .tenant
                .resource
                .cleanup(
                    fixture.tenant.service.clone(),
                    "subscription fanout RTT loopback teardown",
                )
                .await?;
        }
        {
            let fixture = rtt_fixture.lock().await;
            black_box(fixture.registrations.len());
            fixture
                .tenant
                .resource
                .cleanup(
                    fixture.tenant.service.clone(),
                    "subscription fanout RTT injected teardown",
                )
                .await?;
        }

        let operations_per_sample = u64::try_from(SUBSCRIPTION_FANOUT_COUNT)?;
        record_contrast_measurements(
            report,
            WorkloadKind::SubscriptionFanoutLatency,
            BenchmarkLane::SteadyState,
            operations_per_sample,
            sqlite_steady,
            mysql_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::SubscriptionFanoutLatency,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            mysql_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::SubscriptionFanoutLatency,
            operations_per_sample,
            mysql_loopback_rtt,
            mysql_injected_rtt,
        );
        Ok(())
    })
    .await
}

pub(super) async fn benchmark_mixed_multi_tenant_load(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::MixedMultiTenantLoad, async {
        let sqlite_fixture =
            create_mixed_load_fixture("mixed-load-steady", MeasuredBackend::Sqlite, environment)
                .await?;
        let mysql_fixture = create_mixed_load_fixture(
            "mixed-load-steady",
            MeasuredBackend::MySqlLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, mysql_steady) = measure_two_backends_async(
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let mysql_fixture = mysql_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::MySqlLoopback => mysql_fixture,
                        MeasuredBackend::MySqlInjectedRtt => unreachable!(),
                    };
                    let started = Instant::now();
                    run_mixed_load_sample(
                        &format!("mixed-load steady-state {}", backend.label()),
                        exercise_mixed_load_sample(
                            &fixture.service,
                            &fixture.tenant_states,
                            MIXED_LOAD_TENANTS,
                            MIXED_LOAD_OPS_PER_TENANT,
                        ),
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        sqlite_fixture
            .resource
            .cleanup(
                sqlite_fixture.service.clone(),
                "mixed-load steady-state sqlite teardown",
            )
            .await?;
        mysql_fixture
            .resource
            .cleanup(
                mysql_fixture.service.clone(),
                "mixed-load steady-state mysql teardown",
            )
            .await?;

        let sqlite_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture("mixed-load-cold-seed", MeasuredBackend::Sqlite, environment)
                .await?,
            "mixed-load cold-start sqlite seed",
        )
        .await?;
        let mysql_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture(
                "mixed-load-cold-seed",
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await?,
            "mixed-load cold-start mysql seed",
        )
        .await?;
        let (sqlite_cold, mysql_cold) = measure_two_backends_async(
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let mysql_seed = mysql_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::MySqlLoopback => mysql_seed,
                        MeasuredBackend::MySqlInjectedRtt => unreachable!(),
                    };
                    let (service, reopened_resource) = tokio::time::timeout(
                        Duration::from_secs(BENCHMARK_REOPEN_TIMEOUT_SECS),
                        seed.resource
                            .reopen_service("mixed-load-cold-sample", backend, environment),
                    )
                    .await
                    .map_err(|_| -> Box<dyn std::error::Error> {
                        format!(
                            "mixed-load cold-start {} reopen exceeded {BENCHMARK_REOPEN_TIMEOUT_SECS}s",
                            backend.label()
                        )
                        .into()
                    })??;
                    let started = Instant::now();
                    run_mixed_load_sample(
                        &format!("mixed-load cold-start {}", backend.label()),
                        exercise_mixed_load_sample(
                            &service,
                            &seed.tenant_states,
                            MIXED_LOAD_TENANTS,
                            MIXED_LOAD_OPS_PER_TENANT,
                        ),
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    tokio::time::timeout(
                        Duration::from_secs(BENCHMARK_REOPEN_TIMEOUT_SECS),
                        reopened_resource.cleanup(service, "mixed-load cold-start reopened teardown"),
                    )
                    .await
                    .map_err(|_| -> Box<dyn std::error::Error> {
                        format!(
                            "mixed-load cold-start {} cleanup exceeded {BENCHMARK_REOPEN_TIMEOUT_SECS}s",
                            backend.label()
                        )
                        .into()
                    })??;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        mysql_seed.resource.cleanup_seed().await?;

        let loopback_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture(
                "mixed-load-rtt-loopback-seed",
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await?,
            "mixed-load RTT loopback seed freeze",
        )
        .await?;
        let rtt_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture(
                "mixed-load-rtt-injected-seed",
                MeasuredBackend::MySqlLoopback,
                environment,
            )
            .await?,
            "mixed-load RTT injected seed freeze",
        )
        .await?;
        let (mysql_loopback_rtt, mysql_injected_rtt) = measure_two_backends_async(
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::MySqlLoopback,
                MeasuredBackend::MySqlInjectedRtt,
            ],
            |backend| {
                let loopback_seed = loopback_seed.clone();
                let rtt_seed = rtt_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::MySqlLoopback => loopback_seed,
                        MeasuredBackend::MySqlInjectedRtt => rtt_seed,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let (service, reopened_resource) = tokio::time::timeout(
                        Duration::from_secs(BENCHMARK_REOPEN_TIMEOUT_SECS),
                        seed.resource
                            .reopen_service("mixed-load-rtt-sample", backend, environment),
                    )
                    .await
                    .map_err(|_| -> Box<dyn std::error::Error> {
                        format!(
                            "mixed-load RTT {} reopen exceeded {BENCHMARK_REOPEN_TIMEOUT_SECS}s",
                            backend.label()
                        )
                        .into()
                    })??;
                    let started = Instant::now();
                    run_mixed_load_sample(
                        &format!("mixed-load RTT {}", backend.label()),
                        exercise_mixed_load_sample(
                            &service,
                            &seed.tenant_states,
                            MIXED_LOAD_RTT_TENANTS,
                            MIXED_LOAD_RTT_OPS_PER_TENANT,
                        ),
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    tokio::time::timeout(
                        Duration::from_secs(BENCHMARK_REOPEN_TIMEOUT_SECS),
                        reopened_resource.cleanup(service, "mixed-load RTT reopened teardown"),
                    )
                    .await
                    .map_err(|_| -> Box<dyn std::error::Error> {
                        format!(
                            "mixed-load RTT {} cleanup exceeded {BENCHMARK_REOPEN_TIMEOUT_SECS}s",
                            backend.label()
                        )
                        .into()
                    })??;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        loopback_seed.resource.cleanup_seed().await?;
        rtt_seed.resource.cleanup_seed().await?;

        let operations_per_sample = u64::try_from(MIXED_LOAD_TENANTS * MIXED_LOAD_OPS_PER_TENANT)?;
        let rtt_operations_per_sample =
            u64::try_from(MIXED_LOAD_RTT_TENANTS * MIXED_LOAD_RTT_OPS_PER_TENANT)?;
        record_contrast_measurements(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::SteadyState,
            operations_per_sample,
            sqlite_steady,
            mysql_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            mysql_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            rtt_operations_per_sample,
            mysql_loopback_rtt,
            mysql_injected_rtt,
        );
        Ok(())
    })
    .await
}

pub(super) async fn benchmark_tenant_lifecycle_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::TenantLifecycleLatency, async {
        let sqlite_fixture = create_tenant_lifecycle_fixture(
            "tenant-lifecycle",
            MeasuredBackend::Sqlite,
            environment,
        )
        .await?;
        let mysql_fixture = create_tenant_lifecycle_fixture(
            "tenant-lifecycle",
            MeasuredBackend::MySqlLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, mysql_steady) = measure_two_backends_async(
            WorkloadKind::TenantLifecycleLatency,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let mysql_fixture = mysql_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::MySqlLoopback => mysql_fixture,
                        MeasuredBackend::MySqlInjectedRtt => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_tenant_lifecycle_sample(
                        &fixture.creator_service,
                        &fixture.opener_service,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        sqlite_fixture
            .cleanup("tenant-lifecycle steady-state sqlite teardown")
            .await?;
        mysql_fixture
            .cleanup("tenant-lifecycle steady-state mysql teardown")
            .await?;

        let (sqlite_cold, mysql_cold) = measure_two_backends_async(
            WorkloadKind::TenantLifecycleLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::MySqlLoopback],
            |backend| async move {
                let fixture =
                    create_tenant_lifecycle_fixture("tenant-lifecycle-cold", backend, environment)
                        .await?;
                let started = Instant::now();
                exercise_tenant_lifecycle_sample(&fixture.creator_service, &fixture.opener_service)
                    .await?;
                let elapsed = started.elapsed();
                fixture
                    .cleanup("tenant-lifecycle cold-start teardown")
                    .await?;
                Ok(elapsed)
            },
        )
        .await?;
        let loopback_fixture = create_tenant_lifecycle_fixture(
            "tenant-lifecycle-rtt",
            MeasuredBackend::MySqlLoopback,
            environment,
        )
        .await?;
        let rtt_fixture = create_tenant_lifecycle_fixture(
            "tenant-lifecycle-rtt",
            MeasuredBackend::MySqlInjectedRtt,
            environment,
        )
        .await?;
        let (mysql_loopback_rtt, mysql_injected_rtt) = measure_two_backends_async(
            WorkloadKind::TenantLifecycleLatency,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::MySqlLoopback,
                MeasuredBackend::MySqlInjectedRtt,
            ],
            |backend| {
                let loopback_fixture = loopback_fixture.clone();
                let rtt_fixture = rtt_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::MySqlLoopback => loopback_fixture,
                        MeasuredBackend::MySqlInjectedRtt => rtt_fixture,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_tenant_lifecycle_sample(
                        &fixture.creator_service,
                        &fixture.opener_service,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        loopback_fixture
            .cleanup("tenant-lifecycle RTT loopback teardown")
            .await?;
        rtt_fixture
            .cleanup("tenant-lifecycle RTT injected teardown")
            .await?;

        record_contrast_measurements(
            report,
            WorkloadKind::TenantLifecycleLatency,
            BenchmarkLane::SteadyState,
            3,
            sqlite_steady,
            mysql_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::TenantLifecycleLatency,
            BenchmarkLane::ColdStart,
            3,
            sqlite_cold,
            mysql_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::TenantLifecycleLatency,
            3,
            mysql_loopback_rtt,
            mysql_injected_rtt,
        );
        Ok(())
    })
    .await
}
