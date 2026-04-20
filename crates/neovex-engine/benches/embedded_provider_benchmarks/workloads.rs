use super::common::tasks_table;
use super::config::{BenchmarkLane, WorkloadKind};
use super::fixtures::{
    create_composite_query_fixture, create_crud_fixture, create_indexed_query_fixture,
    create_journal_fixture, create_mixed_load_fixture, create_point_read_fixture,
    create_subscription_fixture, freeze_journal_seed, freeze_mixed_load_seed,
    freeze_point_read_seed, freeze_query_seed,
};
use super::models::{SqliteQueryPlan, WorkloadOutcome};
use super::scenarios::{
    exercise_crud_sample, exercise_journal_bootstrap_sample, exercise_journal_stream_sample,
    exercise_mixed_load_sample, exercise_point_read_sample, exercise_query_sample,
    exercise_subscription_fanout_sample, register_subscription_receivers,
    seed_subscription_fixture,
};
use super::support::{
    BenchDir, build_backend_pair_async, capture_sqlite_query_plan, clone_seeded_data_dir,
    measure_backend_pair_async, quiesce_service,
};
use super::*;

pub(super) async fn benchmark_crud_throughput() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_crud_fixture("crud-steady", "crud", backend).await
    })
    .await?;
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::CrudThroughput,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_crud_sample(&fixture.service, &fixture.tenant_id).await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "CRUD steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "CRUD steady-state sqlite teardown",
    )
    .await?;

    let cold_samples = measure_backend_pair_async(
        WorkloadKind::CrudThroughput,
        BenchmarkLane::ColdStart,
        |backend| async move {
            let bench_dir = Arc::new(BenchDir::new("crud-cold", backend)?);
            let data_dir = bench_dir.path().to_path_buf();
            let tenant_id = super::common::benchmark_tenant_id("crud")?;
            let started = Instant::now();
            let service = Arc::new(Service::new_with_embedded_provider(&data_dir, backend)?);
            service.create_tenant_async(tenant_id.clone()).await?;
            exercise_crud_sample(&service, &tenant_id).await?;
            let elapsed = started.elapsed();
            quiesce_service(&service, "CRUD cold-start sample teardown").await?;
            drop(bench_dir);
            Ok(elapsed)
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    let operations_per_sample = u64::try_from(CRUD_DOCUMENTS * 3)?;
    outcome.push_measurements(
        WorkloadKind::CrudThroughput,
        BenchmarkLane::SteadyState,
        operations_per_sample,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::CrudThroughput,
        BenchmarkLane::ColdStart,
        operations_per_sample,
        cold_samples,
    );
    Ok(outcome)
}

pub(super) async fn benchmark_point_read_latency() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_point_read_fixture("point-read-steady", "point-read", backend).await
    })
    .await?;
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::PointReadLatency,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_point_read_sample(&fixture.service, &fixture.tenant_id, &fixture.ids)
                    .await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "point-read steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "point-read steady-state sqlite teardown",
    )
    .await?;

    let cold_seeds = build_backend_pair_async(|backend| async move {
        freeze_point_read_seed(
            create_point_read_fixture("point-read-cold-seed", "point-read", backend).await?,
            "point-read cold-start seed freeze",
        )
        .await
    })
    .await?;
    let cold_samples = measure_backend_pair_async(
        WorkloadKind::PointReadLatency,
        BenchmarkLane::ColdStart,
        |backend| {
            let seed = cold_seeds.get(backend).clone();
            async move {
                let sample_dir =
                    clone_seeded_data_dir(&seed.data_dir, "point-read-cold-sample", backend)?;
                let started = Instant::now();
                let reopened = Arc::new(Service::new_with_embedded_provider(
                    sample_dir.path(),
                    backend,
                )?);
                exercise_point_read_sample(&reopened, &seed.tenant_id, &seed.ids).await?;
                let elapsed = started.elapsed();
                quiesce_service(&reopened, "point-read cold-start reopened teardown").await?;
                drop(sample_dir);
                Ok(elapsed)
            }
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    let operations_per_sample = u64::try_from(POINT_READ_BATCH_SIZE)?;
    outcome.push_measurements(
        WorkloadKind::PointReadLatency,
        BenchmarkLane::SteadyState,
        operations_per_sample,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::PointReadLatency,
        BenchmarkLane::ColdStart,
        operations_per_sample,
        cold_samples,
    );
    Ok(outcome)
}

pub(super) async fn benchmark_indexed_query_latency() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_indexed_query_fixture("indexed-query-steady", "indexed-query", backend).await
    })
    .await?;
    let sqlite_statement =
        sqlite_index_scan_prefix_query_sql(&["status"], 1).expect("indexed query SQL should build");
    let sqlite_plan = SqliteQueryPlan {
        workload: WorkloadKind::IndexedQueryLatency,
        statement: sqlite_statement.clone(),
        detail_rows: capture_sqlite_query_plan(
            &steady_fixtures.sqlite.tenant_path,
            sqlite_statement.as_str(),
            params![tasks_table().as_str(), "open"],
        )?,
    };
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::IndexedQueryLatency,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_query_sample(
                    &fixture.service,
                    &fixture.tenant_id,
                    &fixture.query,
                    INDEXED_QUERY_BATCH_SIZE,
                )
                .await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "indexed-query steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "indexed-query steady-state sqlite teardown",
    )
    .await?;

    let cold_seeds = build_backend_pair_async(|backend| async move {
        freeze_query_seed(
            create_indexed_query_fixture("indexed-query-cold-seed", "indexed-query", backend)
                .await?,
            "indexed-query cold-start seed freeze",
        )
        .await
    })
    .await?;
    let cold_samples = measure_backend_pair_async(
        WorkloadKind::IndexedQueryLatency,
        BenchmarkLane::ColdStart,
        |backend| {
            let seed = cold_seeds.get(backend).clone();
            async move {
                let sample_dir =
                    clone_seeded_data_dir(&seed.data_dir, "indexed-query-cold-sample", backend)?;
                let started = Instant::now();
                let reopened = Arc::new(Service::new_with_embedded_provider(
                    sample_dir.path(),
                    backend,
                )?);
                exercise_query_sample(
                    &reopened,
                    &seed.tenant_id,
                    &seed.query,
                    INDEXED_QUERY_BATCH_SIZE,
                )
                .await?;
                let elapsed = started.elapsed();
                quiesce_service(&reopened, "indexed-query cold-start reopened teardown").await?;
                drop(sample_dir);
                Ok(elapsed)
            }
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    let operations_per_sample = u64::try_from(INDEXED_QUERY_BATCH_SIZE)?;
    outcome.push_measurements(
        WorkloadKind::IndexedQueryLatency,
        BenchmarkLane::SteadyState,
        operations_per_sample,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::IndexedQueryLatency,
        BenchmarkLane::ColdStart,
        operations_per_sample,
        cold_samples,
    );
    outcome.sqlite_query_plans.push(sqlite_plan);
    Ok(outcome)
}

pub(super) async fn benchmark_composite_indexed_query_latency() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_composite_query_fixture("composite-query-steady", "composite-query", backend).await
    })
    .await?;
    let sqlite_statement = sqlite_index_scan_composite_range_query_sql(
        &["team", "status", "rank"],
        2,
        true,
        true,
        true,
        false,
    )
    .expect("composite indexed query SQL should build");
    let sqlite_plan = SqliteQueryPlan {
        workload: WorkloadKind::CompositeIndexedQueryLatency,
        statement: sqlite_statement.clone(),
        detail_rows: capture_sqlite_query_plan(
            &steady_fixtures.sqlite.tenant_path,
            sqlite_statement.as_str(),
            params![tasks_table().as_str(), "alpha", "open", 500_i64, 2_500_i64],
        )?,
    };
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::CompositeIndexedQueryLatency,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_query_sample(
                    &fixture.service,
                    &fixture.tenant_id,
                    &fixture.query,
                    INDEXED_QUERY_BATCH_SIZE,
                )
                .await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "composite indexed-query steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "composite indexed-query steady-state sqlite teardown",
    )
    .await?;

    let cold_seeds = build_backend_pair_async(|backend| async move {
        freeze_query_seed(
            create_composite_query_fixture("composite-query-cold-seed", "composite-query", backend)
                .await?,
            "composite indexed-query cold-start seed freeze",
        )
        .await
    })
    .await?;
    let cold_samples = measure_backend_pair_async(
        WorkloadKind::CompositeIndexedQueryLatency,
        BenchmarkLane::ColdStart,
        |backend| {
            let seed = cold_seeds.get(backend).clone();
            async move {
                let sample_dir =
                    clone_seeded_data_dir(&seed.data_dir, "composite-query-cold-sample", backend)?;
                let started = Instant::now();
                let reopened = Arc::new(Service::new_with_embedded_provider(
                    sample_dir.path(),
                    backend,
                )?);
                exercise_query_sample(
                    &reopened,
                    &seed.tenant_id,
                    &seed.query,
                    INDEXED_QUERY_BATCH_SIZE,
                )
                .await?;
                let elapsed = started.elapsed();
                quiesce_service(
                    &reopened,
                    "composite indexed-query cold-start reopened teardown",
                )
                .await?;
                drop(sample_dir);
                Ok(elapsed)
            }
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    let operations_per_sample = u64::try_from(INDEXED_QUERY_BATCH_SIZE)?;
    outcome.push_measurements(
        WorkloadKind::CompositeIndexedQueryLatency,
        BenchmarkLane::SteadyState,
        operations_per_sample,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::CompositeIndexedQueryLatency,
        BenchmarkLane::ColdStart,
        operations_per_sample,
        cold_samples,
    );
    outcome.sqlite_query_plans.push(sqlite_plan);
    Ok(outcome)
}

pub(super) async fn benchmark_durable_journal_stream_latency() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_journal_fixture("journal-stream-steady", "journal-stream", backend).await
    })
    .await?;
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::DurableJournalStreamLatency,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_journal_stream_sample(&fixture.service, &fixture.tenant_id).await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "journal-stream steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "journal-stream steady-state sqlite teardown",
    )
    .await?;

    let cold_seeds = build_backend_pair_async(|backend| async move {
        freeze_journal_seed(
            create_journal_fixture("journal-stream-cold-seed", "journal-stream", backend).await?,
            "journal-stream cold-start seed freeze",
        )
        .await
    })
    .await?;
    let cold_samples = measure_backend_pair_async(
        WorkloadKind::DurableJournalStreamLatency,
        BenchmarkLane::ColdStart,
        |backend| {
            let seed = cold_seeds.get(backend).clone();
            async move {
                let sample_dir =
                    clone_seeded_data_dir(&seed.data_dir, "journal-stream-cold-sample", backend)?;
                let started = Instant::now();
                let reopened = Arc::new(Service::new_with_embedded_provider(
                    sample_dir.path(),
                    backend,
                )?);
                exercise_journal_stream_sample(&reopened, &seed.tenant_id).await?;
                let elapsed = started.elapsed();
                quiesce_service(&reopened, "journal-stream cold-start reopened teardown").await?;
                drop(sample_dir);
                Ok(elapsed)
            }
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    outcome.push_measurements(
        WorkloadKind::DurableJournalStreamLatency,
        BenchmarkLane::SteadyState,
        1,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::DurableJournalStreamLatency,
        BenchmarkLane::ColdStart,
        1,
        cold_samples,
    );
    Ok(outcome)
}

pub(super) async fn benchmark_durable_journal_bootstrap_latency() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_journal_fixture("journal-bootstrap-steady", "journal-bootstrap", backend).await
    })
    .await?;
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::DurableJournalBootstrapLatency,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_journal_bootstrap_sample(&fixture.service, &fixture.tenant_id).await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "journal-bootstrap steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "journal-bootstrap steady-state sqlite teardown",
    )
    .await?;

    let cold_seeds = build_backend_pair_async(|backend| async move {
        freeze_journal_seed(
            create_journal_fixture("journal-bootstrap-cold-seed", "journal-bootstrap", backend)
                .await?,
            "journal-bootstrap cold-start seed freeze",
        )
        .await
    })
    .await?;
    let cold_samples = measure_backend_pair_async(
        WorkloadKind::DurableJournalBootstrapLatency,
        BenchmarkLane::ColdStart,
        |backend| {
            let seed = cold_seeds.get(backend).clone();
            async move {
                let sample_dir = clone_seeded_data_dir(
                    &seed.data_dir,
                    "journal-bootstrap-cold-sample",
                    backend,
                )?;
                let started = Instant::now();
                let reopened = Arc::new(Service::new_with_embedded_provider(
                    sample_dir.path(),
                    backend,
                )?);
                exercise_journal_bootstrap_sample(&reopened, &seed.tenant_id).await?;
                let elapsed = started.elapsed();
                quiesce_service(&reopened, "journal-bootstrap cold-start reopened teardown")
                    .await?;
                drop(sample_dir);
                Ok(elapsed)
            }
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    outcome.push_measurements(
        WorkloadKind::DurableJournalBootstrapLatency,
        BenchmarkLane::SteadyState,
        1,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::DurableJournalBootstrapLatency,
        BenchmarkLane::ColdStart,
        1,
        cold_samples,
    );
    Ok(outcome)
}

pub(super) async fn benchmark_subscription_fanout_latency() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        Ok(Arc::new(Mutex::new(
            create_subscription_fixture(
                "subscription-fanout-steady",
                "subscription-fanout",
                backend,
            )
            .await?,
        )))
    })
    .await?;
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::SubscriptionFanoutLatency,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let mut fixture = fixture.lock().await;
                let service = fixture.service.clone();
                let tenant_id = fixture.tenant_id.clone();
                let started = Instant::now();
                exercise_subscription_fanout_sample(&service, &tenant_id, &mut fixture.receivers)
                    .await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    {
        let fixture = steady_fixtures.redb.lock().await;
        black_box(fixture.registrations.len());
        black_box(fixture.data_dir.as_os_str());
        black_box(fixture.bench_dir.path());
        quiesce_service(&fixture.service, "subscription steady-state redb teardown").await?;
    }
    {
        let fixture = steady_fixtures.sqlite.lock().await;
        black_box(fixture.registrations.len());
        black_box(fixture.data_dir.as_os_str());
        black_box(fixture.bench_dir.path());
        quiesce_service(
            &fixture.service,
            "subscription steady-state sqlite teardown",
        )
        .await?;
    }

    let cold_samples = measure_backend_pair_async(
        WorkloadKind::SubscriptionFanoutLatency,
        BenchmarkLane::ColdStart,
        |backend| async move {
            let bench_dir = Arc::new(BenchDir::new("subscription-fanout-cold", backend)?);
            let data_dir = bench_dir.path().to_path_buf();
            let tenant_id = super::common::benchmark_tenant_id("subscription-fanout")?;
            let started = Instant::now();
            let service = Arc::new(Service::new_with_embedded_provider(&data_dir, backend)?);
            service.create_tenant_async(tenant_id.clone()).await?;
            seed_subscription_fixture(&service, &tenant_id).await?;
            let (registrations, mut receivers) =
                register_subscription_receivers(&service, &tenant_id).await?;
            exercise_subscription_fanout_sample(&service, &tenant_id, &mut receivers).await?;
            let elapsed = started.elapsed();
            drop(registrations);
            quiesce_service(&service, "subscription cold-start teardown").await?;
            drop(bench_dir);
            Ok(elapsed)
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    let operations_per_sample = u64::try_from(SUBSCRIPTION_FANOUT_COUNT)?;
    outcome.push_measurements(
        WorkloadKind::SubscriptionFanoutLatency,
        BenchmarkLane::SteadyState,
        operations_per_sample,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::SubscriptionFanoutLatency,
        BenchmarkLane::ColdStart,
        operations_per_sample,
        cold_samples,
    );
    Ok(outcome)
}

pub(super) async fn benchmark_mixed_multi_tenant_load() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_mixed_load_fixture("mixed-load-steady", backend).await
    })
    .await?;
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::MixedMultiTenantLoad,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_mixed_load_sample(&fixture.service, &fixture.tenant_states).await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "mixed-load steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "mixed-load steady-state sqlite teardown",
    )
    .await?;

    let cold_seeds = build_backend_pair_async(|backend| async move {
        freeze_mixed_load_seed(
            create_mixed_load_fixture("mixed-load-cold-seed", backend).await?,
            "mixed-load cold-start seed freeze",
        )
        .await
    })
    .await?;
    let cold_samples = measure_backend_pair_async(
        WorkloadKind::MixedMultiTenantLoad,
        BenchmarkLane::ColdStart,
        |backend| {
            let seed = cold_seeds.get(backend).clone();
            async move {
                let sample_dir =
                    clone_seeded_data_dir(&seed.data_dir, "mixed-load-cold-sample", backend)?;
                let started = Instant::now();
                let reopened = Arc::new(Service::new_with_embedded_provider(
                    sample_dir.path(),
                    backend,
                )?);
                exercise_mixed_load_sample(&reopened, &seed.tenant_states).await?;
                let elapsed = started.elapsed();
                quiesce_service(&reopened, "mixed-load cold-start reopened teardown").await?;
                drop(sample_dir);
                Ok(elapsed)
            }
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    let operations_per_sample = u64::try_from(MIXED_LOAD_TENANTS * MIXED_LOAD_OPS_PER_TENANT)?;
    outcome.push_measurements(
        WorkloadKind::MixedMultiTenantLoad,
        BenchmarkLane::SteadyState,
        operations_per_sample,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::MixedMultiTenantLoad,
        BenchmarkLane::ColdStart,
        operations_per_sample,
        cold_samples,
    );
    Ok(outcome)
}
