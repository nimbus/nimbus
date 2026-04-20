use super::common::tasks_table;
use super::config::{BenchmarkEnvironment, BenchmarkLane, WorkloadKind};
use super::fixtures::{
    create_composite_query_fixture, create_crud_fixture, create_indexed_query_fixture,
    create_mixed_load_fixture, create_peer_catch_up_fixture, create_point_read_fixture,
    create_tenant_service, freeze_mixed_load_seed, freeze_point_read_seed, freeze_query_seed,
};
use super::models::{BenchmarkReport, MeasuredBackend};
use super::scenarios::{
    exercise_crud_sample, exercise_mixed_load_sample, exercise_peer_catch_up_sample,
    exercise_point_read_sample, exercise_query_sample, run_mixed_load_sample,
};
use super::support::{
    measure_single_backend_async, measure_two_backends_async, record_contrast_measurements,
    run_workload,
};
use super::*;

pub(super) async fn benchmark_crud_throughput(
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::CrudThroughput, async {
        let sqlite_fixture =
            create_crud_fixture("crud-steady", "crud", MeasuredBackend::Sqlite, environment)
                .await?;
        let replica_fixture = create_crud_fixture(
            "crud-steady",
            "crud",
            MeasuredBackend::LibsqlReplica,
            environment,
        )
        .await?;
        let (sqlite_steady, replica_steady) = measure_two_backends_async(
            WorkloadKind::CrudThroughput,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let replica_fixture = replica_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::LibsqlReplica => replica_fixture,
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
        replica_fixture
            .resource
            .cleanup(
                replica_fixture.service.clone(),
                "CRUD steady-state libsql-replica teardown",
            )
            .await?;

        let (sqlite_cold, replica_cold) = measure_two_backends_async(
            WorkloadKind::CrudThroughput,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
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

        record_contrast_measurements(
            report,
            WorkloadKind::CrudThroughput,
            BenchmarkLane::SteadyState,
            (CRUD_DOCUMENTS * 3) as u64,
            sqlite_steady,
            replica_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::CrudThroughput,
            BenchmarkLane::ColdStart,
            (CRUD_DOCUMENTS * 3) as u64,
            sqlite_cold,
            replica_cold,
        );
        Ok(())
    })
    .await
}

pub(super) async fn benchmark_point_read_latency(
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
        let replica_fixture = create_point_read_fixture(
            "point-read-steady",
            "point-read",
            MeasuredBackend::LibsqlReplica,
            environment,
        )
        .await?;
        let (sqlite_steady, replica_steady) = measure_two_backends_async(
            WorkloadKind::PointReadLatency,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let replica_fixture = replica_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::LibsqlReplica => replica_fixture,
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
        replica_fixture
            .tenant
            .resource
            .cleanup(
                replica_fixture.tenant.service.clone(),
                "point-read steady-state libsql-replica teardown",
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
            "point-read sqlite seed freeze",
        )
        .await?;
        let replica_seed = freeze_point_read_seed(
            create_point_read_fixture(
                "point-read-cold-seed",
                "point-read",
                MeasuredBackend::LibsqlReplica,
                environment,
            )
            .await?,
            "point-read libsql-replica seed freeze",
        )
        .await?;
        let (sqlite_cold, replica_cold) = measure_two_backends_async(
            WorkloadKind::PointReadLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let replica_seed = replica_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::LibsqlReplica => replica_seed,
                    };
                    let (service, resource) = seed
                        .resource
                        .reopen_service("point-read-cold", backend, environment)
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
                    resource
                        .cleanup(service, "point-read cold-start teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        replica_seed.resource.cleanup_seed().await?;

        record_contrast_measurements(
            report,
            WorkloadKind::PointReadLatency,
            BenchmarkLane::SteadyState,
            POINT_READ_BATCH_SIZE as u64,
            sqlite_steady,
            replica_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::PointReadLatency,
            BenchmarkLane::ColdStart,
            POINT_READ_BATCH_SIZE as u64,
            sqlite_cold,
            replica_cold,
        );
        Ok(())
    })
    .await
}

pub(super) async fn benchmark_indexed_query_latency(
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    benchmark_query_latency(
        WorkloadKind::IndexedQueryLatency,
        QueryFixtureKind::Indexed,
        environment,
        report,
    )
    .await
}

pub(super) async fn benchmark_composite_indexed_query_latency(
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    benchmark_query_latency(
        WorkloadKind::CompositeIndexedQueryLatency,
        QueryFixtureKind::Composite,
        environment,
        report,
    )
    .await
}

#[derive(Clone, Copy)]
enum QueryFixtureKind {
    Indexed,
    Composite,
}

async fn benchmark_query_latency(
    workload: WorkloadKind,
    query_kind: QueryFixtureKind,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(workload, async move {
        let sqlite_fixture = create_query_fixture(
            query_kind,
            "query-steady",
            "query",
            MeasuredBackend::Sqlite,
            environment,
        )
        .await?;
        let replica_fixture = create_query_fixture(
            query_kind,
            "query-steady",
            "query",
            MeasuredBackend::LibsqlReplica,
            environment,
        )
        .await?;
        let (sqlite_steady, replica_steady) = measure_two_backends_async(
            workload,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let replica_fixture = replica_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::LibsqlReplica => replica_fixture,
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
        replica_fixture
            .tenant
            .resource
            .cleanup(
                replica_fixture.tenant.service.clone(),
                "query steady-state libsql-replica teardown",
            )
            .await?;

        let sqlite_seed = freeze_query_seed(
            create_query_fixture(
                query_kind,
                "query-cold-seed",
                "query",
                MeasuredBackend::Sqlite,
                environment,
            )
            .await?,
            "query sqlite seed freeze",
        )
        .await?;
        let replica_seed = freeze_query_seed(
            create_query_fixture(
                query_kind,
                "query-cold-seed",
                "query",
                MeasuredBackend::LibsqlReplica,
                environment,
            )
            .await?,
            "query libsql-replica seed freeze",
        )
        .await?;
        let (sqlite_cold, replica_cold) = measure_two_backends_async(
            workload,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let replica_seed = replica_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::LibsqlReplica => replica_seed,
                    };
                    let (service, resource) = seed
                        .resource
                        .reopen_service("query-cold", backend, environment)
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
                    resource
                        .cleanup(service, "query cold-start teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        replica_seed.resource.cleanup_seed().await?;

        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::SteadyState,
            INDEXED_QUERY_BATCH_SIZE as u64,
            sqlite_steady,
            replica_steady,
        );
        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::ColdStart,
            INDEXED_QUERY_BATCH_SIZE as u64,
            sqlite_cold,
            replica_cold,
        );
        Ok(())
    })
    .await
}

async fn create_query_fixture(
    kind: QueryFixtureKind,
    label: &'static str,
    tenant_label: &'static str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<super::fixtures::QueryFixture> {
    match kind {
        QueryFixtureKind::Indexed => {
            create_indexed_query_fixture(label, tenant_label, backend, environment).await
        }
        QueryFixtureKind::Composite => {
            create_composite_query_fixture(label, tenant_label, backend, environment).await
        }
    }
}

pub(super) async fn benchmark_mixed_multi_tenant_load(
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::MixedMultiTenantLoad, async {
        let sqlite_fixture =
            create_mixed_load_fixture("mixed-load-steady", MeasuredBackend::Sqlite, environment)
                .await?;
        let replica_fixture = create_mixed_load_fixture(
            "mixed-load-steady",
            MeasuredBackend::LibsqlReplica,
            environment,
        )
        .await?;
        let (sqlite_steady, replica_steady) = measure_two_backends_async(
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let replica_fixture = replica_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::LibsqlReplica => replica_fixture,
                    };
                    let started = Instant::now();
                    run_mixed_load_sample(
                        "mixed-load steady-state sample",
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
        replica_fixture
            .resource
            .cleanup(
                replica_fixture.service.clone(),
                "mixed-load steady-state libsql-replica teardown",
            )
            .await?;

        let sqlite_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture("mixed-load-cold-seed", MeasuredBackend::Sqlite, environment)
                .await?,
            "mixed-load sqlite seed freeze",
        )
        .await?;
        let replica_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture(
                "mixed-load-cold-seed",
                MeasuredBackend::LibsqlReplica,
                environment,
            )
            .await?,
            "mixed-load libsql-replica seed freeze",
        )
        .await?;
        let (sqlite_cold, replica_cold) = measure_two_backends_async(
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let replica_seed = replica_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::LibsqlReplica => replica_seed,
                    };
                    let (service, resource) = seed
                        .resource
                        .reopen_service("mixed-load-cold", backend, environment)
                        .await?;
                    let started = Instant::now();
                    run_mixed_load_sample(
                        "mixed-load cold-start sample",
                        exercise_mixed_load_sample(
                            &service,
                            &seed.tenant_states,
                            MIXED_LOAD_TENANTS,
                            MIXED_LOAD_OPS_PER_TENANT,
                        ),
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    resource
                        .cleanup(service, "mixed-load cold-start teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        replica_seed.resource.cleanup_seed().await?;

        record_contrast_measurements(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::SteadyState,
            (MIXED_LOAD_TENANTS * MIXED_LOAD_OPS_PER_TENANT) as u64,
            sqlite_steady,
            replica_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::ColdStart,
            (MIXED_LOAD_TENANTS * MIXED_LOAD_OPS_PER_TENANT) as u64,
            sqlite_cold,
            replica_cold,
        );
        Ok(())
    })
    .await
}

pub(super) async fn benchmark_barrier_refresh_latency(
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::BarrierRefreshLatency, async {
        let fixture = create_tenant_service(
            "barrier-refresh",
            "barrier-refresh",
            MeasuredBackend::LibsqlReplica,
            environment,
        )
        .await?;
        let samples = measure_single_backend_async(
            WorkloadKind::BarrierRefreshLatency,
            BenchmarkLane::ReplicaOperational,
            || {
                let fixture = fixture.clone();
                async move {
                    let created_id = fixture
                        .service
                        .insert_document_async(
                            fixture.tenant_id.clone(),
                            tasks_table(),
                            serde_json::Map::from_iter([
                                ("status".to_string(), json!("open")),
                                (
                                    "title".to_string(),
                                    json!(format!(
                                        "barrier-{}",
                                        BENCH_COUNTER.fetch_add(1, Ordering::SeqCst)
                                    )),
                                ),
                            ]),
                        )
                        .await?;
                    let started = Instant::now();
                    let document = fixture
                        .service
                        .get_document_async(fixture.tenant_id.clone(), tasks_table(), created_id)
                        .await?;
                    black_box(document);
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        fixture
            .resource
            .cleanup(
                fixture.service.clone(),
                "barrier-refresh libsql-replica teardown",
            )
            .await?;
        report.push_measurement(
            WorkloadKind::BarrierRefreshLatency,
            BenchmarkLane::ReplicaOperational,
            MeasuredBackend::LibsqlReplica,
            1,
            samples,
        );
        Ok(())
    })
    .await
}

pub(super) async fn benchmark_peer_catch_up_latency(
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::PeerCatchUpLatency, async {
        let fixture = create_peer_catch_up_fixture("peer-catch-up", environment).await?;
        let samples = measure_single_backend_async(
            WorkloadKind::PeerCatchUpLatency,
            BenchmarkLane::ReplicaOperational,
            || {
                let fixture = fixture.clone();
                async move { exercise_peer_catch_up_sample(&fixture).await }
            },
        )
        .await?;
        fixture
            .cleanup("peer-catch-up libsql-replica teardown")
            .await?;
        report.push_measurement(
            WorkloadKind::PeerCatchUpLatency,
            BenchmarkLane::ReplicaOperational,
            MeasuredBackend::LibsqlReplica,
            1,
            samples,
        );
        Ok(())
    })
    .await
}
