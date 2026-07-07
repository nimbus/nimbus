use super::*;

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
        MeasuredBackend::PostgresLoopback | MeasuredBackend::PostgresInjectedRtt => {
            let control_dir = Arc::new(BenchDir::new(&format!(
                "{label}-{}",
                backend.label().replace([' ', '(', ')'], "-")
            ))?);
            let provider_config = benchmark_postgres_provider_config(
                label,
                environment
                    .connection_string(backend)
                    .expect("postgres backend should have connection string"),
                Some(1),
                Some(4),
            )?;
            let engine = Arc::new(
                Engine::new_with_persistence_config(postgres_engine_config(
                    control_dir.path(),
                    &provider_config,
                ))
                .await?,
            );
            let tenant_id = benchmark_tenant_id(tenant_label)?;
            engine.create_tenant_async(tenant_id.clone()).await?;
            Ok(TenantFixture {
                resource: LiveResource::Postgres {
                    control_dir,
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
    Ok(PointReadFixture { tenant, ids })
}

pub(super) async fn create_indexed_query_fixture(
    label: &str,
    tenant_label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<QueryFixture> {
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
    label: &str,
    tenant_label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<QueryFixture> {
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

pub(super) async fn create_journal_fixture(
    label: &str,
    tenant_label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<QueryFixture> {
    let tenant = create_tenant_engine(label, tenant_label, backend, environment).await?;
    for rank in 0..JOURNAL_DOCUMENTS {
        tenant
            .engine
            .insert_document_async(
                tenant.tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!("open")),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .await?;
    }
    Ok(QueryFixture {
        tenant,
        query: query_for_all(),
    })
}

pub(super) async fn create_subscription_fixture(
    label: &str,
    tenant_label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<SubscriptionFixture> {
    let tenant = create_tenant_engine(label, tenant_label, backend, environment).await?;
    let (registrations, receivers) =
        register_subscription_receivers(&tenant.engine, &tenant.tenant_id).await?;
    Ok(SubscriptionFixture {
        tenant,
        registrations,
        receivers,
    })
}

pub(super) async fn create_mixed_load_fixture(
    label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<MixedLoadFixture> {
    let resource_engine = match backend {
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
        MeasuredBackend::PostgresLoopback | MeasuredBackend::PostgresInjectedRtt => {
            let control_dir = Arc::new(BenchDir::new(&format!(
                "{label}-{}",
                backend.label().replace([' ', '(', ')'], "-")
            ))?);
            let provider_config = benchmark_postgres_provider_config(
                label,
                environment
                    .connection_string(backend)
                    .expect("postgres backend should have connection string"),
                Some(1),
                Some(4),
            )?;
            let engine = Arc::new(
                Engine::new_with_persistence_config(postgres_engine_config(
                    control_dir.path(),
                    &provider_config,
                ))
                .await?,
            );
            (
                LiveResource::Postgres {
                    control_dir,
                    provider_config,
                },
                engine,
            )
        }
    };
    let (resource, engine) = resource_engine;
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

pub(super) async fn create_tenant_lifecycle_fixture(
    label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<TenantLifecycleFixture> {
    match backend {
        MeasuredBackend::Sqlite => {
            let bench_dir = Arc::new(BenchDir::new(&format!("{label}-sqlite"))?);
            let data_dir = bench_dir.path().to_path_buf();
            let creator_engine = Arc::new(
                Engine::new_with_persistence_config(EnginePersistenceConfig::embedded(
                    &data_dir,
                    EmbeddedProviderKind::Sqlite,
                ))
                .await?,
            );
            Ok(TenantLifecycleFixture {
                creator_resource: LiveResource::Sqlite {
                    bench_dir: bench_dir.clone(),
                    data_dir: data_dir.clone(),
                },
                creator_engine: creator_engine.clone(),
                opener_resource: LiveResource::Sqlite {
                    bench_dir,
                    data_dir,
                },
                opener_engine: creator_engine,
            })
        }
        MeasuredBackend::PostgresLoopback | MeasuredBackend::PostgresInjectedRtt => {
            let creator_control = Arc::new(BenchDir::new(&format!(
                "{label}-creator-{}",
                backend.label().replace([' ', '(', ')'], "-")
            ))?);
            let opener_control = Arc::new(BenchDir::new(&format!(
                "{label}-opener-{}",
                backend.label().replace([' ', '(', ')'], "-")
            ))?);
            let provider_config = benchmark_postgres_provider_config(
                label,
                environment
                    .connection_string(backend)
                    .expect("postgres backend should have connection string"),
                Some(1),
                Some(4),
            )?;
            let creator_engine = Arc::new(
                Engine::new_with_persistence_config(postgres_engine_config(
                    creator_control.path(),
                    &provider_config,
                ))
                .await?,
            );
            let opener_engine = Arc::new(
                Engine::new_with_persistence_config(postgres_engine_config(
                    opener_control.path(),
                    &provider_config,
                ))
                .await?,
            );
            Ok(TenantLifecycleFixture {
                creator_resource: LiveResource::Postgres {
                    control_dir: creator_control,
                    provider_config: provider_config.clone(),
                },
                creator_engine,
                opener_resource: LiveResource::Postgres {
                    control_dir: opener_control,
                    provider_config: provider_config.clone(),
                },
                opener_engine,
            })
        }
    }
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

pub(super) async fn freeze_journal_seed(
    fixture: QueryFixture,
    context: &str,
) -> BenchResult<JournalSeed> {
    let QueryFixture { tenant, .. } = fixture;
    quiesce_engine(&tenant.engine, context).await?;
    drop(tenant.engine);
    Ok(JournalSeed {
        resource: tenant.resource.into_seed_resource(),
        tenant_id: tenant.tenant_id,
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
            Self::Postgres {
                control_dir,
                provider_config,
            } => {
                black_box(control_dir.path());
                terminate_benchmark_postgres_connections(provider_config).await?;
                register_postgres_cleanup(provider_config);
            }
        }
        Ok(())
    }

    pub(super) fn into_seed_resource(self) -> SeedResource {
        match self {
            Self::Sqlite {
                bench_dir,
                data_dir,
            } => SeedResource::Sqlite {
                bench_dir,
                data_dir,
            },
            Self::Postgres {
                provider_config, ..
            } => SeedResource::Postgres { provider_config },
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
                    backend.label().replace([' ', '(', ')'], "-")
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
            Self::Postgres { provider_config } => {
                let control_dir = Arc::new(BenchDir::new(&format!(
                    "{label}-{}",
                    backend.label().replace([' ', '(', ')'], "-")
                ))?);
                let mut reopened_config = provider_config.clone();
                reopened_config.connection_string = environment
                    .connection_string(backend)
                    .expect("postgres backend should have connection string")
                    .to_string();
                let engine = Arc::new(
                    Engine::new_with_persistence_config(postgres_engine_config(
                        control_dir.path(),
                        &reopened_config,
                    ))
                    .await?,
                );
                Ok((
                    engine,
                    ReopenedResource::Postgres {
                        control_dir,
                        provider_config: reopened_config,
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
            Self::Postgres { provider_config } => {
                terminate_benchmark_postgres_connections(provider_config).await?;
                register_postgres_cleanup(provider_config);
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
            Self::Postgres {
                control_dir,
                provider_config,
            } => {
                terminate_benchmark_postgres_connections(&provider_config).await?;
                drop(control_dir);
            }
        }
        Ok(())
    }
}

impl TenantLifecycleFixture {
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

pub(super) async fn exercise_crud_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    document_count: usize,
) -> BenchResult<()> {
    let mut ids = Vec::with_capacity(document_count);
    for rank in 0..document_count {
        ids.push(
            engine
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([
                        ("status".to_string(), json!("open")),
                        ("rank".to_string(), json!(rank)),
                        ("title".to_string(), json!(format!("task-{rank:05}"))),
                    ]),
                )
                .await?,
        );
    }
    for (rank, id) in ids.iter().cloned().enumerate() {
        let _ = engine
            .update_document_async(
                tenant_id.clone(),
                tasks_table(),
                id,
                serde_json::Map::from_iter([("rank".to_string(), json!(rank + document_count))]),
            )
            .await?;
    }
    for id in ids {
        engine
            .delete_document_async(tenant_id.clone(), tasks_table(), id)
            .await?;
    }
    Ok(())
}

pub(super) async fn exercise_point_read_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    ids: &[DocumentId],
    batch_size: usize,
) -> BenchResult<()> {
    for step in 0..batch_size {
        let id = ids[(step * 17) % ids.len()].clone();
        let document = engine
            .get_document_async(tenant_id.clone(), tasks_table(), id)
            .await?;
        black_box(document);
    }
    Ok(())
}

pub(super) async fn exercise_query_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    query: &Query,
    batch_size: usize,
) -> BenchResult<()> {
    for _ in 0..batch_size {
        let documents = engine
            .query_documents_async(tenant_id.clone(), query.clone())
            .await?;
        black_box(documents);
    }
    Ok(())
}

pub(super) async fn exercise_journal_stream_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> BenchResult<()> {
    let page = engine
        .stream_durable_journal_async(tenant_id.clone(), SequenceNumber(0), JOURNAL_STREAM_LIMIT)
        .await?;
    black_box(page);
    Ok(())
}

pub(super) async fn exercise_journal_bootstrap_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> BenchResult<()> {
    let bootstrap = engine
        .export_durable_journal_bootstrap_async(tenant_id.clone())
        .await?;
    black_box(bootstrap);
    Ok(())
}

pub(super) async fn exercise_journal_workload_sample(
    workload: WorkloadKind,
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> BenchResult<()> {
    match workload {
        WorkloadKind::DurableJournalStreamLatency => {
            exercise_journal_stream_sample(engine, tenant_id).await
        }
        WorkloadKind::DurableJournalBootstrapLatency => {
            exercise_journal_bootstrap_sample(engine, tenant_id).await
        }
        _ => Err(format!("invalid journal workload: {}", workload.label()).into()),
    }
}

pub(super) async fn exercise_subscription_bootstrap_catchup_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> BenchResult<()> {
    let token = format!("bootstrap-{}", BENCH_COUNTER.fetch_add(1, Ordering::SeqCst));
    let query = Query {
        table: tasks_table(),
        filters: vec![filter("topic", FilterOp::Eq, json!(token.clone()))],
        order: None,
        limit: None,
    };
    let (sender, mut receiver) = mpsc::channel(8);
    let registration = engine
        .subscribe_async(
            tenant_id.clone(),
            query,
            token.clone(),
            sender,
            SubscribeOptions::anonymous(),
        )
        .await?;
    let bootstrap = receiver
        .recv()
        .await
        .ok_or("subscription bootstrap should arrive")?;
    black_box(bootstrap);
    let _ = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([
                ("topic".to_string(), json!(token)),
                ("title".to_string(), json!("catchup")),
            ]),
        )
        .await?;
    let update = receiver
        .recv()
        .await
        .ok_or("subscription catch-up should arrive")?;
    black_box(update);
    drop(registration);
    Ok(())
}

pub(super) async fn register_subscription_receivers(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> BenchResult<(
    Vec<SubscriptionRegistration>,
    Vec<mpsc::Receiver<SubscriptionUpdate>>,
)> {
    let query = query_for_all();
    let mut registrations = Vec::with_capacity(SUBSCRIPTION_FANOUT_COUNT);
    let mut receivers = Vec::with_capacity(SUBSCRIPTION_FANOUT_COUNT);
    for index in 0..SUBSCRIPTION_FANOUT_COUNT {
        let (sender, mut receiver) = mpsc::channel(8);
        let registration = engine
            .subscribe_async(
                tenant_id.clone(),
                query.clone(),
                format!("fanout-{index}"),
                sender,
                SubscribeOptions::anonymous(),
            )
            .await?;
        let initial = receiver
            .recv()
            .await
            .ok_or("subscription bootstrap should arrive")?;
        black_box(initial);
        registrations.push(registration);
        receivers.push(receiver);
    }
    Ok((registrations, receivers))
}

pub(super) async fn exercise_subscription_fanout_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    receivers: &mut [mpsc::Receiver<SubscriptionUpdate>],
) -> BenchResult<()> {
    let _ = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([(
                "title".to_string(),
                json!(format!(
                    "fanout-{}",
                    BENCH_COUNTER.fetch_add(1, Ordering::SeqCst)
                )),
            )]),
        )
        .await?;
    for receiver in receivers {
        let update = receiver
            .recv()
            .await
            .ok_or("subscription update should arrive")?;
        match update {
            SubscriptionUpdate::Result { .. } => {}
            SubscriptionUpdate::Error { message, .. } => {
                return Err(format!("unexpected subscription error: {message}").into());
            }
        }
    }
    Ok(())
}

pub(super) async fn exercise_mixed_load_sample(
    engine: &Arc<Engine>,
    tenant_states: &[TenantState],
    tenant_limit: usize,
    ops_per_tenant: usize,
) -> BenchResult<()> {
    let selected = tenant_states
        .iter()
        .take(tenant_limit)
        .cloned()
        .collect::<Vec<_>>();
    let mut handles = Vec::with_capacity(selected.len());
    for (task_index, state) in selected.into_iter().enumerate() {
        let engine = engine.clone();
        handles.push(tokio::spawn(async move {
            let query = Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("open"))],
                order: None,
                limit: Some(25),
            };
            for step in 0..ops_per_tenant {
                let id = state.ids[step % state.ids.len()].clone();
                match step % 4 {
                    0 => {
                        let document = tokio::time::timeout(
                            Duration::from_secs(MIXED_LOAD_OPERATION_TIMEOUT_SECS),
                            engine.get_document_async(state.tenant_id.clone(), tasks_table(), id),
                        )
                        .await
                        .map_err(|_| {
                            nimbus_core::Error::Internal(format!(
                                "mixed-load point read timed out for tenant {} at step {step}",
                                state.tenant_id
                            ))
                        })??;
                        black_box(document);
                    }
                    1 => {
                        let documents = tokio::time::timeout(
                            Duration::from_secs(MIXED_LOAD_OPERATION_TIMEOUT_SECS),
                            engine.query_documents_async(state.tenant_id.clone(), query.clone()),
                        )
                        .await
                        .map_err(|_| {
                            nimbus_core::Error::Internal(format!(
                                "mixed-load indexed query timed out for tenant {} at step {step}",
                                state.tenant_id
                            ))
                        })??;
                        black_box(documents);
                    }
                    2 => {
                        let _ = tokio::time::timeout(
                            Duration::from_secs(MIXED_LOAD_OPERATION_TIMEOUT_SECS),
                            engine.insert_document_async(
                                state.tenant_id.clone(),
                                tasks_table(),
                                serde_json::Map::from_iter([
                                    ("status".to_string(), json!("open")),
                                    (
                                        "rank".to_string(),
                                        json!(task_index * ops_per_tenant + step),
                                    ),
                                    (
                                        "title".to_string(),
                                        json!(format!("tenant-{task_index}-insert-{step}")),
                                    ),
                                ]),
                            ),
                        )
                        .await
                        .map_err(|_| {
                            nimbus_core::Error::Internal(format!(
                                "mixed-load insert timed out for tenant {} at step {step}",
                                state.tenant_id
                            ))
                        })??;
                    }
                    _ => {
                        let _ = tokio::time::timeout(
                            Duration::from_secs(MIXED_LOAD_OPERATION_TIMEOUT_SECS),
                            engine.update_document_async(
                                state.tenant_id.clone(),
                                tasks_table(),
                                id,
                                serde_json::Map::from_iter([(
                                    "rank".to_string(),
                                    json!(step + ops_per_tenant),
                                )]),
                            ),
                        )
                        .await
                        .map_err(|_| {
                            nimbus_core::Error::Internal(format!(
                                "mixed-load update timed out for tenant {} at step {step}",
                                state.tenant_id
                            ))
                        })??;
                    }
                }
            }
            Ok::<(), nimbus_core::Error>(())
        }));
    }
    for handle in handles {
        handle.await??;
    }
    Ok(())
}

pub(super) async fn exercise_tenant_lifecycle_sample(
    creator_engine: &Arc<Engine>,
    opener_engine: &Arc<Engine>,
) -> BenchResult<()> {
    let suffix = BENCH_COUNTER.fetch_add(1, Ordering::SeqCst);
    let tenant_id = TenantId::new(format!("bench-tenant-lifecycle-{suffix}"))?;
    creator_engine
        .create_tenant_async(tenant_id.clone())
        .await?;
    opener_engine
        .ensure_tenant_exists_async(tenant_id.clone())
        .await?;
    creator_engine.delete_tenant_async(tenant_id).await?;
    Ok(())
}

pub(super) async fn observe_pool_pressure(
    environment: &BenchmarkEnvironment,
) -> BenchResult<PoolPressureObservation> {
    let provider_config = benchmark_postgres_provider_config(
        "pool-pressure",
        environment.loopback_connection_string.as_str(),
        Some(1),
        Some(POOL_PRESSURE_MAX_CONNECTIONS),
    )?;
    let control_dir = Arc::new(BenchDir::new("pool-pressure")?);
    let engine = Arc::new(
        Engine::new_with_persistence_config(postgres_engine_config(
            control_dir.path(),
            &provider_config,
        ))
        .await?,
    );
    let application_name = provider_config.derived_pool_application_name()?;
    let (client, connection) =
        tokio_postgres::connect(provider_config.connection_string.as_str(), NoTls).await?;
    let connection_task = tokio::spawn(async move {
        let _ = connection.await;
    });

    let seeded_fixture = create_pool_pressure_fixture(
        engine.clone(),
        LiveResource::Postgres {
            control_dir: control_dir.clone(),
            provider_config: provider_config.clone(),
        },
    )
    .await?;
    let (stop_tx, stop_rx) = watch::channel(false);
    let sampler = tokio::spawn(sample_pool_backends(client, application_name, stop_rx));

    let mut samples = Vec::with_capacity(POOL_PRESSURE_SAMPLES);
    for _ in 0..POOL_PRESSURE_SAMPLES {
        let started = Instant::now();
        exercise_pool_pressure_read_sample(
            &seeded_fixture.tenant.engine,
            &seeded_fixture.tenant.tenant_id,
            &seeded_fixture.ids,
            POOL_PRESSURE_TASKS,
        )
        .await?;
        samples.push(started.elapsed());
    }

    let _ = stop_tx.send(true);
    let max_backends_observed = sampler
        .await
        .map_err(|error| format!("pool-pressure sampler join failed: {error}"))?
        .map_err(|error| format!("pool-pressure sampler failed: {error}"))?;
    connection_task.abort();
    seeded_fixture
        .tenant
        .resource
        .cleanup(
            seeded_fixture.tenant.engine.clone(),
            "pool-pressure observation teardown",
        )
        .await?;

    let stats = SampleStats::from_samples(&samples, 1);
    Ok(PoolPressureObservation {
        sample_count: samples.len(),
        max_backends_observed,
        mean_sample_latency: stats.mean_per_operation,
        median_sample_latency: stats.median_per_operation,
        p95_sample_latency: stats.p95_per_operation,
        configured_max_connections: POOL_PRESSURE_MAX_CONNECTIONS,
        concurrent_tasks: POOL_PRESSURE_TASKS,
    })
}

pub(super) async fn sample_pool_backends(
    client: tokio_postgres::Client,
    application_name: String,
    mut stop_rx: watch::Receiver<bool>,
) -> Result<i64, nimbus_core::Error> {
    let mut max_observed = 0_i64;
    loop {
        if *stop_rx.borrow() {
            break;
        }
        let row = client
            .query_one(
                "SELECT COUNT(*) FROM pg_stat_activity WHERE application_name = $1",
                &[&application_name],
            )
            .await
            .map_err(|error| nimbus_core::Error::Internal(error.to_string()))?;
        let count = row.get::<_, i64>(0);
        max_observed = max_observed.max(count);
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(POOL_PRESSURE_SAMPLE_INTERVAL_MS)) => {}
        }
    }
    Ok(max_observed)
}

pub(super) async fn create_pool_pressure_fixture(
    engine: Arc<Engine>,
    resource: LiveResource,
) -> BenchResult<PointReadFixture> {
    let tenant_id = TenantId::new("pool-pressure-tenant")?;
    engine.create_tenant_async(tenant_id.clone()).await?;
    let mut ids = Vec::with_capacity(POINT_READ_DOCUMENTS);
    for rank in 0..POINT_READ_DOCUMENTS {
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
                        ("title".to_string(), json!(format!("pool-task-{rank}"))),
                    ]),
                )
                .await?,
        );
    }
    Ok(PointReadFixture {
        tenant: TenantFixture {
            resource,
            engine,
            tenant_id,
        },
        ids,
    })
}

pub(super) async fn exercise_pool_pressure_read_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    ids: &[DocumentId],
    parallel_tasks: usize,
) -> BenchResult<()> {
    let mut handles = Vec::with_capacity(parallel_tasks);
    for task_index in 0..parallel_tasks {
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let ids = ids.to_vec();
        handles.push(tokio::spawn(async move {
            for step in 0..POINT_READ_BATCH_SIZE {
                let id = ids[(task_index * 17 + step) % ids.len()].clone();
                let document = engine
                    .get_document_async(tenant_id.clone(), tasks_table(), id)
                    .await?;
                black_box(document);
            }
            Ok::<(), nimbus_core::Error>(())
        }));
    }
    for handle in handles {
        handle.await??;
    }
    Ok(())
}
