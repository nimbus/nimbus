use super::common::{
    benchmark_tenant_id, composite_schema, copy_dir_all, filter, single_field_schema, tasks_table,
};
use super::config::BenchmarkEnvironment;
use super::models::MeasuredBackend;
use super::support::{
    benchmark_libsql_provider_config, libsql_replica_engine_config, quiesce_engine,
    register_libsql_replica_cleanup, slugify_label,
};
use super::*;
use libsql::{Builder, Database};
use nimbus_storage::libsql::libsql_transport_connector;

#[derive(Clone)]
pub(super) struct TenantFixture {
    pub(super) resource: LiveResource,
    pub(super) engine: Arc<Engine>,
    pub(super) tenant_id: TenantId,
}

#[derive(Clone)]
pub(super) struct PointReadFixture {
    pub(super) tenant: TenantFixture,
    pub(super) ids: Vec<DocumentId>,
}

#[derive(Clone)]
pub(super) struct QueryFixture {
    pub(super) tenant: TenantFixture,
    pub(super) query: Query,
}

#[derive(Clone)]
pub(super) struct MixedLoadFixture {
    pub(super) resource: LiveResource,
    pub(super) engine: Arc<Engine>,
    pub(super) tenant_states: Vec<TenantState>,
}

#[derive(Clone)]
pub(super) struct PointReadSeed {
    pub(super) resource: SeedResource,
    pub(super) tenant_id: TenantId,
    pub(super) ids: Vec<DocumentId>,
}

#[derive(Clone)]
pub(super) struct QuerySeed {
    pub(super) resource: SeedResource,
    pub(super) tenant_id: TenantId,
    pub(super) query: Query,
}

#[derive(Clone)]
pub(super) struct MixedLoadSeed {
    pub(super) resource: SeedResource,
    pub(super) tenant_states: Vec<TenantState>,
}

#[derive(Clone)]
pub(super) struct TenantState {
    pub(super) tenant_id: TenantId,
    pub(super) ids: Vec<DocumentId>,
}

pub(super) struct PeerCatchUpFixture {
    creator_resource: LiveResource,
    pub(super) creator_engine: Arc<Engine>,
    opener_resource: LiveResource,
    pub(super) opener_engine: Arc<Engine>,
    pub(super) tenant_id: TenantId,
}

#[derive(Clone)]
pub(super) enum LiveResource {
    Sqlite {
        bench_dir: Arc<BenchDir>,
        data_dir: PathBuf,
    },
    LibsqlReplica {
        control_dir: Arc<BenchDir>,
        replica_cache_dir: Arc<BenchDir>,
        provider_config: LibsqlReplicaProviderConfig,
    },
}

#[derive(Clone)]
pub(super) enum SeedResource {
    Sqlite {
        bench_dir: Arc<BenchDir>,
        data_dir: PathBuf,
    },
    LibsqlReplica {
        provider_config: LibsqlReplicaProviderConfig,
    },
}

pub(super) enum ReopenedResource {
    Sqlite {
        bench_dir: Arc<BenchDir>,
    },
    LibsqlReplica {
        control_dir: Arc<BenchDir>,
        replica_cache_dir: Arc<BenchDir>,
    },
}

#[derive(Debug)]
pub(super) struct BenchDir {
    path: PathBuf,
}

impl BenchDir {
    pub(super) fn new(label: &str) -> BenchResult<Self> {
        let counter = BENCH_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = env::temp_dir().join(format!(
            "nimbus-libsql-replica-bench-{label}-{}-{counter}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for BenchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(super) async fn create_tenant_engine(
    label: &str,
    tenant_label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<TenantFixture> {
    match backend {
        MeasuredBackend::Sqlite => {
            let bench_dir = Arc::new(BenchDir::new(&format!("{label}-sqlite"))?);
            let data_dir = bench_dir.path().to_path_buf();
            let engine = Arc::new(
                Engine::new_with_persistence_config(EnginePersistenceConfig::embedded(
                    &data_dir,
                    EmbeddedProviderKind::Sqlite,
                ))
                .await?,
            );
            let tenant_id = benchmark_tenant_id(tenant_label)?;
            engine.create_tenant_async(tenant_id.clone()).await?;
            Ok(TenantFixture {
                resource: LiveResource::Sqlite {
                    bench_dir,
                    data_dir,
                },
                engine,
                tenant_id,
            })
        }
        MeasuredBackend::LibsqlReplica => {
            let control_dir = Arc::new(BenchDir::new(&format!("{label}-replica-control"))?);
            let replica_cache_dir = Arc::new(BenchDir::new(&format!("{label}-replica-cache"))?);
            let provider_config =
                benchmark_libsql_provider_config(label, environment, replica_cache_dir.path());
            let engine = Arc::new(
                Engine::new_with_persistence_config(libsql_replica_engine_config(
                    control_dir.path(),
                    &provider_config,
                )?)
                .await?,
            );
            let tenant_id = benchmark_tenant_id(tenant_label)?;
            engine.create_tenant_async(tenant_id.clone()).await?;
            Ok(TenantFixture {
                resource: LiveResource::LibsqlReplica {
                    control_dir,
                    replica_cache_dir,
                    provider_config,
                },
                engine,
                tenant_id,
            })
        }
    }
}

pub(super) async fn create_crud_fixture(
    label: &str,
    tenant_label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<TenantFixture> {
    create_tenant_engine(label, tenant_label, backend, environment).await
}

pub(super) async fn create_point_read_fixture(
    label: &str,
    tenant_label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<PointReadFixture> {
    let (tenant, ids) = match backend {
        MeasuredBackend::Sqlite => {
            let tenant = create_tenant_engine(label, tenant_label, backend, environment).await?;
            let mut ids = Vec::with_capacity(POINT_READ_DOCUMENTS);
            for rank in 0..POINT_READ_DOCUMENTS {
                ids.push(
                    tenant
                        .engine
                        .insert_document_async(
                            tenant.tenant_id.clone(),
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
            (tenant, ids)
        }
        MeasuredBackend::LibsqlReplica => {
            let mut seeded_documents = Vec::with_capacity(POINT_READ_DOCUMENTS);
            let mut ids = Vec::with_capacity(POINT_READ_DOCUMENTS);
            for rank in 0..POINT_READ_DOCUMENTS {
                let id = DocumentId::new();
                ids.push(id);
                seeded_documents.push((
                    ids.last()
                        .expect("point-read document id should exist")
                        .clone(),
                    serde_json::Map::from_iter([
                        (
                            "status".to_string(),
                            json!(if rank % 2 == 0 { "open" } else { "done" }),
                        ),
                        ("rank".to_string(), json!(rank)),
                        ("title".to_string(), json!(format!("task-{rank:05}"))),
                    ]),
                ));
            }
            let tenant = create_seeded_libsql_replica_tenant_engine(
                label,
                tenant_label,
                environment,
                None,
                seeded_documents.as_slice(),
            )
            .await?;
            (tenant, ids)
        }
    };
    Ok(PointReadFixture { tenant, ids })
}

pub(super) async fn create_indexed_query_fixture(
    label: &'static str,
    tenant_label: &'static str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<QueryFixture> {
    let tenant = match backend {
        MeasuredBackend::Sqlite => {
            let tenant = create_tenant_engine(label, tenant_label, backend, environment).await?;
            tenant
                .engine
                .set_table_schema_async(tenant.tenant_id.clone(), single_field_schema())
                .await?;
            for rank in 0..INDEXED_QUERY_DOCUMENTS {
                tenant
                    .engine
                    .insert_document_async(
                        tenant.tenant_id.clone(),
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
            tenant
        }
        MeasuredBackend::LibsqlReplica => {
            let mut seeded_documents = Vec::with_capacity(INDEXED_QUERY_DOCUMENTS);
            for rank in 0..INDEXED_QUERY_DOCUMENTS {
                seeded_documents.push((
                    DocumentId::new(),
                    serde_json::Map::from_iter([
                        (
                            "status".to_string(),
                            json!(if rank % 5 == 0 { "open" } else { "done" }),
                        ),
                        ("rank".to_string(), json!(rank)),
                        ("title".to_string(), json!(format!("task-{rank:05}"))),
                    ]),
                ));
            }
            create_seeded_libsql_replica_tenant_engine(
                label,
                tenant_label,
                environment,
                Some(single_field_schema()),
                seeded_documents.as_slice(),
            )
            .await?
        }
    };
    Ok(QueryFixture {
        tenant,
        query: Query {
            table: tasks_table(),
            filters: vec![filter("status", FilterOp::Eq, json!("open"))],
            order: None,
            limit: None,
        },
    })
}

pub(super) async fn create_composite_query_fixture(
    label: &'static str,
    tenant_label: &'static str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<QueryFixture> {
    let tenant = match backend {
        MeasuredBackend::Sqlite => {
            let tenant = create_tenant_engine(label, tenant_label, backend, environment).await?;
            tenant
                .engine
                .set_table_schema_async(tenant.tenant_id.clone(), composite_schema())
                .await?;
            for rank in 0..INDEXED_QUERY_DOCUMENTS {
                let team = if rank % 2 == 0 { "alpha" } else { "beta" };
                let status = if rank % 3 == 0 { "open" } else { "done" };
                tenant
                    .engine
                    .insert_document_async(
                        tenant.tenant_id.clone(),
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
            tenant
        }
        MeasuredBackend::LibsqlReplica => {
            let mut seeded_documents = Vec::with_capacity(INDEXED_QUERY_DOCUMENTS);
            for rank in 0..INDEXED_QUERY_DOCUMENTS {
                let team = if rank % 2 == 0 { "alpha" } else { "beta" };
                let status = if rank % 3 == 0 { "open" } else { "done" };
                seeded_documents.push((
                    DocumentId::new(),
                    serde_json::Map::from_iter([
                        ("team".to_string(), json!(team)),
                        ("status".to_string(), json!(status)),
                        ("rank".to_string(), json!(rank)),
                        ("title".to_string(), json!(format!("task-{rank:05}"))),
                    ]),
                ));
            }
            create_seeded_libsql_replica_tenant_engine(
                label,
                tenant_label,
                environment,
                Some(composite_schema()),
                seeded_documents.as_slice(),
            )
            .await?
        }
    };
    Ok(QueryFixture {
        tenant,
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

pub(super) async fn create_mixed_load_fixture(
    label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<MixedLoadFixture> {
    let (resource, engine) = match backend {
        MeasuredBackend::Sqlite => {
            let bench_dir = Arc::new(BenchDir::new(&format!("{label}-sqlite"))?);
            let data_dir = bench_dir.path().to_path_buf();
            let engine = Arc::new(
                Engine::new_with_persistence_config(EnginePersistenceConfig::embedded(
                    &data_dir,
                    EmbeddedProviderKind::Sqlite,
                ))
                .await?,
            );
            (
                LiveResource::Sqlite {
                    bench_dir,
                    data_dir,
                },
                engine,
            )
        }
        MeasuredBackend::LibsqlReplica => {
            let control_dir = Arc::new(BenchDir::new(&format!("{label}-replica-control"))?);
            let replica_cache_dir = Arc::new(BenchDir::new(&format!("{label}-replica-cache"))?);
            let provider_config =
                benchmark_libsql_provider_config(label, environment, replica_cache_dir.path());
            let engine = Arc::new(
                Engine::new_with_persistence_config(libsql_replica_engine_config(
                    control_dir.path(),
                    &provider_config,
                )?)
                .await?,
            );
            (
                LiveResource::LibsqlReplica {
                    control_dir,
                    replica_cache_dir,
                    provider_config,
                },
                engine,
            )
        }
    };

    let mut tenant_states = Vec::with_capacity(MIXED_LOAD_TENANTS);
    for tenant_index in 0..MIXED_LOAD_TENANTS {
        let tenant_id = TenantId::new(format!("tenant-{tenant_index}"))?;
        engine.create_tenant_async(tenant_id.clone()).await?;
        engine
            .set_table_schema_async(tenant_id.clone(), single_field_schema())
            .await?;
        let mut ids = Vec::with_capacity(MIXED_LOAD_OPS_PER_TENANT);
        for rank in 0..MIXED_LOAD_OPS_PER_TENANT {
            ids.push(
                engine
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
        resource,
        engine,
        tenant_states,
    })
}

pub(super) async fn create_peer_catch_up_fixture(
    label: &str,
    environment: &BenchmarkEnvironment,
) -> BenchResult<PeerCatchUpFixture> {
    let suffix = BENCH_COUNTER.fetch_add(1, Ordering::SeqCst);
    let base_slug = slugify_label(label, 12);
    let metadata_namespace = format!("nvx_{}_{}_{suffix:x}", base_slug, std::process::id());
    let tenant_namespace_prefix = format!("t_{}_{}_{suffix:x}_", base_slug, std::process::id());

    let creator_control = Arc::new(BenchDir::new(&format!("{label}-creator-control"))?);
    let creator_cache = Arc::new(BenchDir::new(&format!("{label}-creator-cache"))?);
    let opener_control = Arc::new(BenchDir::new(&format!("{label}-opener-control"))?);
    let opener_cache = Arc::new(BenchDir::new(&format!("{label}-opener-cache"))?);

    let creator_provider_config = LibsqlReplicaProviderConfig {
        primary_url: environment.primary_url.clone(),
        auth_token: environment.auth_token.clone(),
        admin_api_url: environment.admin_api_url.clone(),
        admin_auth_header: environment.admin_auth_header.clone(),
        metadata_namespace: metadata_namespace.clone(),
        tenant_namespace_prefix: tenant_namespace_prefix.clone(),
        replica_cache_dir: creator_cache.path().to_path_buf(),
        encryption_provider: None,
    };
    let opener_provider_config = LibsqlReplicaProviderConfig {
        replica_cache_dir: opener_cache.path().to_path_buf(),
        ..creator_provider_config.clone()
    };

    let creator_engine = Arc::new(
        Engine::new_with_persistence_config(libsql_replica_engine_config(
            creator_control.path(),
            &creator_provider_config,
        )?)
        .await?,
    );
    let opener_engine = Arc::new(
        Engine::new_with_persistence_config(libsql_replica_engine_config(
            opener_control.path(),
            &opener_provider_config,
        )?)
        .await?,
    );

    let tenant_id = benchmark_tenant_id("peer-catch-up")?;
    creator_engine
        .create_tenant_async(tenant_id.clone())
        .await?;
    creator_engine
        .set_table_schema_async(tenant_id.clone(), single_field_schema())
        .await?;
    opener_engine
        .ensure_tenant_exists_async(tenant_id.clone())
        .await?;
    let _ = opener_engine.get_schema_async(tenant_id.clone()).await?;

    Ok(PeerCatchUpFixture {
        creator_resource: LiveResource::LibsqlReplica {
            control_dir: creator_control,
            replica_cache_dir: creator_cache,
            provider_config: creator_provider_config,
        },
        creator_engine,
        opener_resource: LiveResource::LibsqlReplica {
            control_dir: opener_control,
            replica_cache_dir: opener_cache,
            provider_config: opener_provider_config,
        },
        opener_engine,
        tenant_id,
    })
}

pub(super) async fn freeze_point_read_seed(
    fixture: PointReadFixture,
    context: &str,
) -> BenchResult<PointReadSeed> {
    let PointReadFixture { tenant, ids } = fixture;
    quiesce_engine(&tenant.engine, context).await?;
    drop(tenant.engine);
    Ok(PointReadSeed {
        resource: tenant.resource.into_seed_resource(),
        tenant_id: tenant.tenant_id,
        ids,
    })
}

pub(super) async fn freeze_query_seed(
    fixture: QueryFixture,
    context: &str,
) -> BenchResult<QuerySeed> {
    let QueryFixture { tenant, query } = fixture;
    quiesce_engine(&tenant.engine, context).await?;
    drop(tenant.engine);
    Ok(QuerySeed {
        resource: tenant.resource.into_seed_resource(),
        tenant_id: tenant.tenant_id,
        query,
    })
}

pub(super) async fn freeze_mixed_load_seed(
    fixture: MixedLoadFixture,
    context: &str,
) -> BenchResult<MixedLoadSeed> {
    let MixedLoadFixture {
        resource,
        engine,
        tenant_states,
    } = fixture;
    quiesce_engine(&engine, context).await?;
    drop(engine);
    Ok(MixedLoadSeed {
        resource: resource.into_seed_resource(),
        tenant_states,
    })
}

impl LiveResource {
    pub(super) async fn cleanup(&self, engine: Arc<Engine>, context: &str) -> BenchResult<()> {
        quiesce_engine(&engine, context).await?;
        drop(engine);
        match self {
            Self::Sqlite {
                bench_dir,
                data_dir,
            } => {
                black_box(bench_dir.path());
                black_box(data_dir.as_os_str());
            }
            Self::LibsqlReplica {
                control_dir,
                replica_cache_dir,
                provider_config,
            } => {
                black_box(control_dir.path());
                black_box(replica_cache_dir.path());
                register_libsql_replica_cleanup(provider_config);
            }
        }
        Ok(())
    }

    fn into_seed_resource(self) -> SeedResource {
        match self {
            Self::Sqlite {
                bench_dir,
                data_dir,
            } => SeedResource::Sqlite {
                bench_dir,
                data_dir,
            },
            Self::LibsqlReplica {
                provider_config, ..
            } => SeedResource::LibsqlReplica { provider_config },
        }
    }
}

impl SeedResource {
    pub(super) async fn reopen_engine(
        &self,
        label: &str,
        backend: MeasuredBackend,
        environment: &BenchmarkEnvironment,
    ) -> BenchResult<(Arc<Engine>, ReopenedResource)> {
        match self {
            Self::Sqlite { data_dir, .. } => {
                let cloned = Arc::new(BenchDir::new(&format!(
                    "{label}-{}",
                    backend.label().replace(' ', "-")
                ))?);
                copy_dir_all(data_dir, cloned.path())?;
                let engine = Arc::new(
                    Engine::new_with_persistence_config(EnginePersistenceConfig::embedded(
                        cloned.path(),
                        EmbeddedProviderKind::Sqlite,
                    ))
                    .await?,
                );
                Ok((engine, ReopenedResource::Sqlite { bench_dir: cloned }))
            }
            Self::LibsqlReplica { provider_config } => {
                let control_dir = Arc::new(BenchDir::new(&format!("{label}-replica-control"))?);
                let replica_cache_dir = Arc::new(BenchDir::new(&format!("{label}-replica-cache"))?);
                let mut reopened_config = provider_config.clone();
                reopened_config.primary_url = environment.primary_url.clone();
                reopened_config.auth_token = environment.auth_token.clone();
                reopened_config.admin_api_url = environment.admin_api_url.clone();
                reopened_config.admin_auth_header = environment.admin_auth_header.clone();
                reopened_config.replica_cache_dir = replica_cache_dir.path().to_path_buf();
                let engine = Arc::new(
                    Engine::new_with_persistence_config(libsql_replica_engine_config(
                        control_dir.path(),
                        &reopened_config,
                    )?)
                    .await?,
                );
                Ok((
                    engine,
                    ReopenedResource::LibsqlReplica {
                        control_dir,
                        replica_cache_dir,
                    },
                ))
            }
        }
    }

    pub(super) async fn cleanup_seed(&self) -> BenchResult<()> {
        match self {
            Self::Sqlite {
                bench_dir,
                data_dir,
            } => {
                black_box(bench_dir.path());
                black_box(data_dir.as_os_str());
            }
            Self::LibsqlReplica { provider_config } => {
                register_libsql_replica_cleanup(provider_config);
            }
        }
        Ok(())
    }
}

impl ReopenedResource {
    pub(super) async fn cleanup(self, engine: Arc<Engine>, context: &str) -> BenchResult<()> {
        quiesce_engine(&engine, context).await?;
        drop(engine);
        match self {
            Self::Sqlite { bench_dir } => {
                drop(bench_dir);
            }
            Self::LibsqlReplica {
                control_dir,
                replica_cache_dir,
            } => {
                drop(control_dir);
                drop(replica_cache_dir);
            }
        }
        Ok(())
    }
}

impl Clone for PeerCatchUpFixture {
    fn clone(&self) -> Self {
        Self {
            creator_resource: self.creator_resource.clone(),
            creator_engine: self.creator_engine.clone(),
            opener_resource: self.opener_resource.clone(),
            opener_engine: self.opener_engine.clone(),
            tenant_id: self.tenant_id.clone(),
        }
    }
}

impl PeerCatchUpFixture {
    pub(super) async fn cleanup(self, context: &str) -> BenchResult<()> {
        self.creator_resource
            .cleanup(self.creator_engine.clone(), &format!("{context} creator"))
            .await?;
        self.opener_resource
            .cleanup(self.opener_engine.clone(), &format!("{context} opener"))
            .await?;
        Ok(())
    }
}

async fn create_seeded_libsql_replica_tenant_engine(
    label: &str,
    tenant_label: &str,
    environment: &BenchmarkEnvironment,
    schema: Option<TableSchema>,
    documents: &[(DocumentId, serde_json::Map<String, serde_json::Value>)],
) -> BenchResult<TenantFixture> {
    let control_dir = Arc::new(BenchDir::new(&format!("{label}-replica-control"))?);
    let replica_cache_dir = Arc::new(BenchDir::new(&format!("{label}-replica-cache"))?);
    let provider_config =
        benchmark_libsql_provider_config(label, environment, replica_cache_dir.path());
    let provider = LibsqlReplicaProvider::connect(provider_config.clone()).await?;
    let tenant_id = benchmark_tenant_id(tenant_label)?;
    let registration = provider
        .create_tenant(&tenant_id) // tenant-lifecycle: provider-adapter-internal
        .await?;
    seed_remote_namespace_documents(
        &provider_config,
        &registration.namespace,
        schema.as_ref(),
        documents,
    )
    .await?;
    drop(provider);

    let engine = Arc::new(
        Engine::new_with_persistence_config(libsql_replica_engine_config(
            control_dir.path(),
            &provider_config,
        )?)
        .await?,
    );
    engine.ensure_tenant_exists_async(tenant_id.clone()).await?;
    Ok(TenantFixture {
        resource: LiveResource::LibsqlReplica {
            control_dir,
            replica_cache_dir,
            provider_config,
        },
        engine,
        tenant_id,
    })
}

async fn seed_remote_namespace_documents(
    config: &LibsqlReplicaProviderConfig,
    namespace: &str,
    schema: Option<&TableSchema>,
    documents: &[(DocumentId, serde_json::Map<String, serde_json::Value>)],
) -> BenchResult<()> {
    let database = open_remote_namespace_database(config, namespace).await?;
    let conn = database.connect()?;
    conn.execute_batch("BEGIN IMMEDIATE").await?;
    if let Some(schema) = schema {
        conn.execute(
            "INSERT OR REPLACE INTO schemas (table_name, schema_json) VALUES (?, ?)",
            libsql::params![schema.table.as_str(), serde_json::to_string(schema)?,],
        )
        .await?;
    }
    for (index, (document_id, fields)) in documents.iter().enumerate() {
        conn.execute(
            "INSERT INTO documents (table_name, id, data_json, typed_fields_json, creation_time, update_time) VALUES (?, ?, ?, ?, ?, ?)",
            libsql::params![
                tasks_table().as_str(),
                document_id.to_string(),
                serde_json::Value::Object(fields.clone()).to_string(),
                "{}",
                i64::try_from(index + 1)?,
                i64::try_from(index + 1)?,
            ],
        )
        .await?;
    }
    conn.execute_batch("COMMIT").await?;
    Ok(())
}

async fn open_remote_namespace_database(
    config: &LibsqlReplicaProviderConfig,
    namespace: &str,
) -> BenchResult<Database> {
    let builder = Builder::new_remote(
        config.primary_url.clone(),
        config.auth_token.clone().unwrap_or_default(),
    )
    .namespace(namespace.to_string())
    .connector(libsql_transport_connector()?);
    Ok(builder.build().await?)
}
