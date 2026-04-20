use super::*;

pub(crate) async fn benchmark_subscription_bootstrap_catchup_latency(
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
        let postgres_fixture = create_tenant_service(
            "subscription-bootstrap-steady",
            "subscription-bootstrap",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
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
        postgres_fixture
            .resource
            .cleanup(
                postgres_fixture.service.clone(),
                "subscription bootstrap steady-state postgres teardown",
            )
            .await?;

        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
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
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let rtt_fixture = create_tenant_service(
            "subscription-bootstrap-rtt",
            "subscription-bootstrap-rtt",
            MeasuredBackend::PostgresInjectedRtt,
            environment,
        )
        .await?;
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
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
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            BenchmarkLane::ColdStart,
            1,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            1,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        Ok(())
    })
    .await
}

pub(crate) async fn benchmark_subscription_fanout_latency(
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
        let postgres_fixture = Arc::new(Mutex::new(
            create_subscription_fixture(
                "subscription-fanout-steady",
                "subscription-fanout",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
        ));
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            WorkloadKind::SubscriptionFanoutLatency,
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
            let fixture = postgres_fixture.lock().await;
            black_box(fixture.registrations.len());
            fixture
                .tenant
                .resource
                .cleanup(
                    fixture.tenant.service.clone(),
                    "subscription fanout steady-state postgres teardown",
                )
                .await?;
        }

        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            WorkloadKind::SubscriptionFanoutLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
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
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
        ));
        let rtt_fixture = Arc::new(Mutex::new(
            create_subscription_fixture(
                "subscription-fanout-rtt",
                "subscription-fanout-rtt",
                MeasuredBackend::PostgresInjectedRtt,
                environment,
            )
            .await?,
        ));
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            WorkloadKind::SubscriptionFanoutLatency,
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
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::SubscriptionFanoutLatency,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::SubscriptionFanoutLatency,
            operations_per_sample,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        Ok(())
    })
    .await
}
