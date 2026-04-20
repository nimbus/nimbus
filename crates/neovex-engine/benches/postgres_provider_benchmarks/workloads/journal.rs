use super::*;

pub(crate) async fn benchmark_durable_journal_stream_latency(
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

pub(crate) async fn benchmark_durable_journal_bootstrap_latency(
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

async fn benchmark_journal_workload(
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
        let postgres_fixture = create_journal_fixture(
            "journal-steady",
            "journal",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
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
        postgres_fixture
            .tenant
            .resource
            .cleanup(
                postgres_fixture.tenant.service.clone(),
                "journal steady-state postgres teardown",
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
        let postgres_seed = freeze_journal_seed(
            create_journal_fixture(
                "journal-cold-seed",
                "journal",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "journal cold-start postgres seed",
        )
        .await?;
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
        postgres_seed.resource.cleanup_seed().await?;

        let loopback_seed = freeze_journal_seed(
            create_journal_fixture(
                "journal-rtt-loopback-seed",
                "journal-rtt",
                MeasuredBackend::PostgresLoopback,
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
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "journal RTT injected seed freeze",
        )
        .await?;
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
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::ColdStart,
            1,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            workload,
            1,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        Ok(())
    })
    .await
}
