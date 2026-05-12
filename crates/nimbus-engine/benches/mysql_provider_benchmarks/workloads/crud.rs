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
