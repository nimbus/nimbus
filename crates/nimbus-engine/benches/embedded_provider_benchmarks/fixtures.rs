use super::common::{
    benchmark_tenant_id, composite_schema, filter, single_field_schema, tasks_table,
};
use super::scenarios::{register_subscription_receivers, seed_subscription_fixture};
use super::support::{
    BenchDir, TenantState, open_embedded_service, quiesce_service, tenant_store_path,
};
use super::*;

#[derive(Clone)]
pub(super) struct CrudFixture {
    pub(super) _bench_dir: Arc<BenchDir>,
    pub(super) service: Arc<Service>,
    pub(super) tenant_id: TenantId,
}

#[derive(Clone)]
pub(super) struct PointReadFixture {
    pub(super) bench_dir: Arc<BenchDir>,
    pub(super) data_dir: PathBuf,
    pub(super) service: Arc<Service>,
    pub(super) tenant_id: TenantId,
    pub(super) ids: Vec<DocumentId>,
}

#[derive(Clone)]
pub(super) struct QueryFixture {
    pub(super) bench_dir: Arc<BenchDir>,
    pub(super) data_dir: PathBuf,
    pub(super) service: Arc<Service>,
    pub(super) tenant_id: TenantId,
    pub(super) query: Query,
    pub(super) tenant_path: PathBuf,
}

#[derive(Clone)]
pub(super) struct JournalFixture {
    pub(super) bench_dir: Arc<BenchDir>,
    pub(super) data_dir: PathBuf,
    pub(super) service: Arc<Service>,
    pub(super) tenant_id: TenantId,
}

pub(super) struct SubscriptionFixture {
    pub(super) bench_dir: Arc<BenchDir>,
    pub(super) data_dir: PathBuf,
    pub(super) service: Arc<Service>,
    pub(super) tenant_id: TenantId,
    pub(super) registrations: Vec<SubscriptionRegistration>,
    pub(super) receivers: Vec<mpsc::Receiver<SubscriptionUpdate>>,
}

#[derive(Clone)]
pub(super) struct MixedLoadFixture {
    pub(super) bench_dir: Arc<BenchDir>,
    pub(super) data_dir: PathBuf,
    pub(super) service: Arc<Service>,
    pub(super) tenant_states: Vec<TenantState>,
}

#[derive(Clone)]
pub(super) struct PointReadSeed {
    pub(super) _bench_dir: Arc<BenchDir>,
    pub(super) data_dir: PathBuf,
    pub(super) tenant_id: TenantId,
    pub(super) ids: Vec<DocumentId>,
}

#[derive(Clone)]
pub(super) struct QuerySeed {
    pub(super) _bench_dir: Arc<BenchDir>,
    pub(super) data_dir: PathBuf,
    pub(super) tenant_id: TenantId,
    pub(super) query: Query,
}

#[derive(Clone)]
pub(super) struct JournalSeed {
    pub(super) _bench_dir: Arc<BenchDir>,
    pub(super) data_dir: PathBuf,
    pub(super) tenant_id: TenantId,
}

#[derive(Clone)]
pub(super) struct MixedLoadSeed {
    pub(super) _bench_dir: Arc<BenchDir>,
    pub(super) data_dir: PathBuf,
    pub(super) tenant_states: Vec<TenantState>,
}

pub(super) async fn create_crud_fixture(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<CrudFixture> {
    let (bench_dir, _data_dir, service, tenant_id) =
        create_tenant_service(label, tenant_label, backend).await?;
    Ok(CrudFixture {
        _bench_dir: bench_dir,
        service,
        tenant_id,
    })
}

pub(super) async fn create_point_read_fixture(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<PointReadFixture> {
    let (bench_dir, data_dir, service, tenant_id) =
        create_tenant_service(label, tenant_label, backend).await?;
    let mut ids = Vec::with_capacity(POINT_READ_DOCUMENTS);
    for rank in 0..POINT_READ_DOCUMENTS {
        ids.push(
            service
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([
                        (
                            "status".to_string(),
                            json!(if rank % 2 == 0 { "open" } else { "done" }),
                        ),
                        ("rank".to_string(), json!(rank)),
                        ("title".to_string(), json!(format!("task-{rank:05}"))),
                    ]),
                )
                .await?,
        );
    }
    Ok(PointReadFixture {
        bench_dir,
        data_dir,
        service,
        tenant_id,
        ids,
    })
}

pub(super) async fn create_indexed_query_fixture(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<QueryFixture> {
    let (bench_dir, data_dir, service, tenant_id) =
        create_tenant_service(label, tenant_label, backend).await?;
    service
        .set_table_schema_async(tenant_id.clone(), single_field_schema())
        .await?;
    for rank in 0..INDEXED_QUERY_DOCUMENTS {
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([
                    (
                        "status".to_string(),
                        json!(if rank % 5 == 0 { "open" } else { "done" }),
                    ),
                    ("rank".to_string(), json!(rank)),
                    ("title".to_string(), json!(format!("task-{rank:05}"))),
                ]),
            )
            .await?;
    }
    Ok(QueryFixture {
        tenant_path: tenant_store_path(&data_dir, backend, &tenant_id),
        bench_dir,
        data_dir,
        service,
        tenant_id,
        query: Query {
            table: tasks_table(),
            filters: vec![filter("status", FilterOp::Eq, json!("open"))],
            order: None,
            limit: None,
        },
    })
}

pub(super) async fn create_composite_query_fixture(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<QueryFixture> {
    let (bench_dir, data_dir, service, tenant_id) =
        create_tenant_service(label, tenant_label, backend).await?;
    service
        .set_table_schema_async(tenant_id.clone(), composite_schema())
        .await?;
    for rank in 0..INDEXED_QUERY_DOCUMENTS {
        let team = if rank % 2 == 0 { "alpha" } else { "beta" };
        let status = if rank % 3 == 0 { "open" } else { "done" };
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([
                    ("team".to_string(), json!(team)),
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                    ("title".to_string(), json!(format!("task-{rank:05}"))),
                ]),
            )
            .await?;
    }
    Ok(QueryFixture {
        tenant_path: tenant_store_path(&data_dir, backend, &tenant_id),
        bench_dir,
        data_dir,
        service,
        tenant_id,
        query: Query {
            table: tasks_table(),
            filters: vec![
                filter("team", FilterOp::Eq, json!("alpha")),
                filter("status", FilterOp::Eq, json!("open")),
                filter("rank", FilterOp::Gte, json!(500)),
                filter("rank", FilterOp::Lt, json!(2_500)),
            ],
            order: Some(OrderBy {
                field: "rank".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        },
    })
}

pub(super) async fn create_journal_fixture(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<JournalFixture> {
    let (bench_dir, data_dir, service, tenant_id) =
        create_tenant_service(label, tenant_label, backend).await?;
    for rank in 0..JOURNAL_DOCUMENTS {
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!("open")),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .await?;
    }
    Ok(JournalFixture {
        bench_dir,
        data_dir,
        service,
        tenant_id,
    })
}

pub(super) async fn create_subscription_fixture(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<SubscriptionFixture> {
    let (bench_dir, data_dir, service, tenant_id) =
        create_tenant_service(label, tenant_label, backend).await?;
    seed_subscription_fixture(&service, &tenant_id).await?;
    let (registrations, receivers) = register_subscription_receivers(&service, &tenant_id).await?;
    Ok(SubscriptionFixture {
        bench_dir,
        data_dir,
        service,
        tenant_id,
        registrations,
        receivers,
    })
}

pub(super) async fn create_mixed_load_fixture(
    label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<MixedLoadFixture> {
    let bench_dir = Arc::new(BenchDir::new(label, backend)?);
    let data_dir = bench_dir.path().to_path_buf();
    let service = open_embedded_service(&data_dir, backend).await?;
    let mut tenant_states = Vec::with_capacity(MIXED_LOAD_TENANTS);
    for tenant_index in 0..MIXED_LOAD_TENANTS {
        let tenant_id = TenantId::new(format!("tenant-{tenant_index}"))?;
        service.create_tenant_async(tenant_id.clone()).await?;
        service
            .set_table_schema_async(tenant_id.clone(), single_field_schema())
            .await?;
        let mut ids = Vec::with_capacity(MIXED_LOAD_OPS_PER_TENANT);
        for rank in 0..MIXED_LOAD_OPS_PER_TENANT {
            ids.push(
                service
                    .insert_document_async(
                        tenant_id.clone(),
                        tasks_table(),
                        serde_json::Map::from_iter([
                            (
                                "status".to_string(),
                                json!(if rank % 2 == 0 { "open" } else { "done" }),
                            ),
                            ("rank".to_string(), json!(rank)),
                            (
                                "title".to_string(),
                                json!(format!("tenant-{tenant_index}-task-{rank}")),
                            ),
                        ]),
                    )
                    .await?,
            );
        }
        tenant_states.push(TenantState { tenant_id, ids });
    }
    Ok(MixedLoadFixture {
        bench_dir,
        data_dir,
        service,
        tenant_states,
    })
}

async fn create_tenant_service(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<(Arc<BenchDir>, PathBuf, Arc<Service>, TenantId)> {
    let bench_dir = Arc::new(BenchDir::new(label, backend)?);
    let data_dir = bench_dir.path().to_path_buf();
    let service = open_embedded_service(&data_dir, backend).await?;
    let tenant_id = benchmark_tenant_id(tenant_label)?;
    service.create_tenant_async(tenant_id.clone()).await?;
    Ok((bench_dir, data_dir, service, tenant_id))
}

pub(super) async fn freeze_point_read_seed(
    fixture: PointReadFixture,
    context: &str,
) -> BenchResult<PointReadSeed> {
    let PointReadFixture {
        bench_dir,
        data_dir,
        service,
        tenant_id,
        ids,
    } = fixture;
    quiesce_service(&service, context).await?;
    drop(service);
    Ok(PointReadSeed {
        _bench_dir: bench_dir,
        data_dir,
        tenant_id,
        ids,
    })
}

pub(super) async fn freeze_query_seed(
    fixture: QueryFixture,
    context: &str,
) -> BenchResult<QuerySeed> {
    let QueryFixture {
        bench_dir,
        data_dir,
        service,
        tenant_id,
        query,
        ..
    } = fixture;
    quiesce_service(&service, context).await?;
    drop(service);
    Ok(QuerySeed {
        _bench_dir: bench_dir,
        data_dir,
        tenant_id,
        query,
    })
}

pub(super) async fn freeze_journal_seed(
    fixture: JournalFixture,
    context: &str,
) -> BenchResult<JournalSeed> {
    let JournalFixture {
        bench_dir,
        data_dir,
        service,
        tenant_id,
    } = fixture;
    quiesce_service(&service, context).await?;
    drop(service);
    Ok(JournalSeed {
        _bench_dir: bench_dir,
        data_dir,
        tenant_id,
    })
}

pub(super) async fn freeze_mixed_load_seed(
    fixture: MixedLoadFixture,
    context: &str,
) -> BenchResult<MixedLoadSeed> {
    let MixedLoadFixture {
        bench_dir,
        data_dir,
        service,
        tenant_states,
    } = fixture;
    quiesce_service(&service, context).await?;
    drop(service);
    Ok(MixedLoadSeed {
        _bench_dir: bench_dir,
        data_dir,
        tenant_states,
    })
}
