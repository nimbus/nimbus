use super::*;

pub(crate) async fn benchmark_crud_throughput(
    config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::CrudThroughput, async {
        let sqlite_fixture =
            create_crud_fixture("crud-steady", "crud", MeasuredBackend::Sqlite, environment)
                .await?;
        let postgres_fixture = create_crud_fixture(
            "crud-steady",
            "crud",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            WorkloadKind::CrudThroughput,
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
        postgres_fixture
            .resource
            .cleanup(
                postgres_fixture.service.clone(),
                "CRUD steady-state postgres teardown",
            )
            .await?;

        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            WorkloadKind::CrudThroughput,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
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
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let rtt_fixture = create_crud_fixture(
            "crud-rtt-injected",
            "crud-rtt",
            MeasuredBackend::PostgresInjectedRtt,
            environment,
        )
        .await?;
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            WorkloadKind::CrudThroughput,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::PostgresLoopback,
                MeasuredBackend::PostgresInjectedRtt,
            ],
            |backend| {
                let loopback_fixture = loopback_fixture.clone();
                let rtt_fixture = rtt_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::PostgresLoopback => loopback_fixture,
                        MeasuredBackend::PostgresInjectedRtt => rtt_fixture,
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
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::CrudThroughput,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::CrudThroughput,
            rtt_operations_per_sample,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        let _ = config;
        Ok(())
    })
    .await
}
