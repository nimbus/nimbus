use super::*;

pub(crate) async fn benchmark_mixed_multi_tenant_load(
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

pub(crate) async fn benchmark_tenant_lifecycle_latency(
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
