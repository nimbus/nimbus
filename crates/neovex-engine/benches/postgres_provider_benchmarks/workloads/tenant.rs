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
        let postgres_fixture = create_mixed_load_fixture(
            "mixed-load-steady",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            WorkloadKind::MixedMultiTenantLoad,
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
                    exercise_mixed_load_sample(
                        &fixture.service,
                        &fixture.tenant_states,
                        MIXED_LOAD_TENANTS,
                        MIXED_LOAD_OPS_PER_TENANT,
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
        postgres_fixture
            .resource
            .cleanup(
                postgres_fixture.service.clone(),
                "mixed-load steady-state postgres teardown",
            )
            .await?;

        let sqlite_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture("mixed-load-cold-seed", MeasuredBackend::Sqlite, environment)
                .await?,
            "mixed-load cold-start sqlite seed",
        )
        .await?;
        let postgres_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture(
                "mixed-load-cold-seed",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "mixed-load cold-start postgres seed",
        )
        .await?;
        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            WorkloadKind::MixedMultiTenantLoad,
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
                        .reopen_service("mixed-load-cold-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_mixed_load_sample(
                        &service,
                        &seed.tenant_states,
                        MIXED_LOAD_TENANTS,
                        MIXED_LOAD_OPS_PER_TENANT,
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "mixed-load cold-start reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        postgres_seed.resource.cleanup_seed().await?;

        let loopback_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture(
                "mixed-load-rtt-loopback-seed",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "mixed-load RTT loopback seed freeze",
        )
        .await?;
        let rtt_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture(
                "mixed-load-rtt-injected-seed",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "mixed-load RTT injected seed freeze",
        )
        .await?;
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            WorkloadKind::MixedMultiTenantLoad,
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
                        .reopen_service("mixed-load-rtt-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_mixed_load_sample(
                        &service,
                        &seed.tenant_states,
                        MIXED_LOAD_RTT_TENANTS,
                        MIXED_LOAD_RTT_OPS_PER_TENANT,
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "mixed-load RTT reopened teardown")
                        .await?;
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
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            rtt_operations_per_sample,
            postgres_loopback_rtt,
            postgres_injected_rtt,
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
        let postgres_fixture = create_tenant_lifecycle_fixture(
            "tenant-lifecycle",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            WorkloadKind::TenantLifecycleLatency,
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
        postgres_fixture
            .cleanup("tenant-lifecycle steady-state postgres teardown")
            .await?;

        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            WorkloadKind::TenantLifecycleLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
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
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let rtt_fixture = create_tenant_lifecycle_fixture(
            "tenant-lifecycle-rtt",
            MeasuredBackend::PostgresInjectedRtt,
            environment,
        )
        .await?;
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            WorkloadKind::TenantLifecycleLatency,
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
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::TenantLifecycleLatency,
            BenchmarkLane::ColdStart,
            3,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::TenantLifecycleLatency,
            3,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        Ok(())
    })
    .await
}
