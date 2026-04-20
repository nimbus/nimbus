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
