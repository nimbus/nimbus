use super::*;

impl PostgresProvider {
    pub async fn connect(config: PostgresProviderConfig) -> Result<Self> {
        Self::connect_with_simulation(
            config,
            TokioRuntimeHandle::current(),
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
        )
        .await
    }

    pub async fn connect_with_simulation(
        config: PostgresProviderConfig,
        runtime_handle: TokioRuntimeHandle,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        validate_identifier_input(&config.metadata_schema, "metadata schema")?;
        validate_identifier_input(&config.tenant_schema_prefix, "tenant schema prefix")?;

        let pool_application_name = postgres_pool_application_name(&config)?;
        let pool = build_pool(&config, &pool_application_name)?;
        let notification_channel = postgres_notification_channel_name(&config)?;
        let provider = Self {
            pool,
            connection_string: config.connection_string.clone(),
            metadata_schema: config.metadata_schema,
            tenant_schema_prefix: config.tenant_schema_prefix,
            pool_application_name,
            notification_channel,
            runtime_handle,
            clock,
            fault_injector,
            tenant_read_parallelism: default_postgres_read_parallelism(),
        };
        provider.ensure_metadata_schema().await?;
        Ok(provider)
    }

    pub fn metadata_schema(&self) -> &str {
        &self.metadata_schema
    }

    pub fn tenant_schema_name(&self, tenant_id: &TenantId) -> Result<String> {
        tenant_schema_name(&self.tenant_schema_prefix, tenant_id)
    }

    pub fn notification_channel(&self) -> &str {
        &self.notification_channel
    }

    pub fn pool_application_name(&self) -> &str {
        &self.pool_application_name
    }

    pub fn notification_listener_application_name(&self) -> &str {
        &self.notification_channel
    }

    pub async fn connect_notification_listener(&self) -> Result<PostgresNotificationListener> {
        let (client, connection) = tokio_postgres::connect(&self.connection_string, NoTls)
            .await
            .map_err(map_postgres_error)?;
        let channel = self.notification_channel.clone();
        let application_name = quote_literal(self.notification_listener_application_name());
        let quoted_channel = quote_identifier(&channel);
        let (sender, receiver) = mpsc::unbounded_channel();
        let pump_task = self.runtime_handle.spawn(async move {
            let mut connection = connection;
            loop {
                match std::future::poll_fn(|cx| connection.poll_message(cx)).await {
                    Some(Ok(AsyncMessage::Notification(notification))) => {
                        if notification.channel() != channel {
                            continue;
                        }
                        let _ = sender.send(parse_postgres_notification(notification));
                    }
                    Some(Ok(AsyncMessage::Notice(_))) => {}
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        let _ = sender.send(Err(map_postgres_error(error)));
                        break;
                    }
                    None => break,
                }
            }
        });
        if let Err(error) = client
            .batch_execute(
                format!("SET application_name = {application_name}; LISTEN {quoted_channel}")
                    .as_str(),
            )
            .await
        {
            pump_task.abort();
            return Err(map_postgres_error(error));
        }
        Ok(PostgresNotificationListener {
            _client: client,
            receiver,
            pump_task,
        })
    }

    pub fn read_storage_for_store(
        &self,
        store: Arc<PostgresTenantStore>,
    ) -> Arc<PostgresTenantStorage> {
        Arc::new(PostgresTenantStorage::with_max_concurrent_reads(
            store,
            self.runtime_handle.clone(),
            self.tenant_read_parallelism,
        ))
    }

    pub async fn create_opened_tenant(&self, tenant_id: &TenantId) -> Result<OpenedPostgresTenant> {
        let registration = self.create_tenant(tenant_id).await?;
        self.open_registration(registration)
    }

    pub async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedPostgresTenant>> {
        self.open_existing_tenant(tenant_id)
            .await?
            .map(|registration| self.open_registration(registration))
            .transpose()
    }

    pub async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        let client = self.client().await?;
        let query = format!(
            "SELECT tenant_id FROM {} ORDER BY tenant_id",
            qualified_table(&self.metadata_schema, "tenants")
        );
        let rows = client
            .query(query.as_str(), &[])
            .await
            .map_err(map_postgres_error)?;
        rows.into_iter()
            .map(|row| TenantId::new(row.get::<_, String>(0)))
            .collect()
    }

    pub async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        let client = self.client().await?;
        let query = format!(
            "SELECT 1 FROM {} WHERE tenant_id = $1",
            qualified_table(&self.metadata_schema, "tenants")
        );
        client
            .query_opt(query.as_str(), &[&tenant_id.as_str()])
            .await
            .map(|row| row.is_some())
            .map_err(map_postgres_error)
    }

    pub async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<PostgresTenantRegistration>> {
        let client = self.client().await?;
        let query = format!(
            "SELECT schema_name FROM {} WHERE tenant_id = $1",
            qualified_table(&self.metadata_schema, "tenants")
        );
        let row = client
            .query_opt(query.as_str(), &[&tenant_id.as_str()])
            .await
            .map_err(map_postgres_error)?;
        Ok(row.map(|row| PostgresTenantRegistration {
            tenant_id: tenant_id.clone(),
            schema_name: row.get(0),
        }))
    }

    pub async fn create_tenant(&self, tenant_id: &TenantId) -> Result<PostgresTenantRegistration> {
        let mut client = self.client().await?;
        let transaction = client.transaction().await.map_err(map_postgres_error)?;
        let fetch_query = format!(
            "SELECT schema_name FROM {} WHERE tenant_id = $1",
            qualified_table(&self.metadata_schema, "tenants")
        );
        if transaction
            .query_opt(fetch_query.as_str(), &[&tenant_id.as_str()])
            .await
            .map_err(map_postgres_error)?
            .is_some()
        {
            return Err(Error::AlreadyExists(format!(
                "tenant already exists: {tenant_id}"
            )));
        }

        let schema_name = self.tenant_schema_name(tenant_id)?;
        let create_schema_sql = format!("CREATE SCHEMA {}", quote_identifier(&schema_name));
        transaction
            .batch_execute(create_schema_sql.as_str())
            .await
            .map_err(map_postgres_error)?;
        transaction
            .batch_execute(tenant_init_sql(&schema_name).as_str())
            .await
            .map_err(map_postgres_error)?;
        let insert_query = format!(
            "INSERT INTO {} (tenant_id, schema_name) VALUES ($1, $2)",
            qualified_table(&self.metadata_schema, "tenants")
        );
        transaction
            .execute(insert_query.as_str(), &[&tenant_id.as_str(), &schema_name])
            .await
            .map_err(map_postgres_error)?;
        transaction.commit().await.map_err(map_postgres_error)?;

        Ok(PostgresTenantRegistration {
            tenant_id: tenant_id.clone(),
            schema_name,
        })
    }

    pub async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        let mut client = self.client().await?;
        let transaction = client.transaction().await.map_err(map_postgres_error)?;
        let delete_query = format!(
            "DELETE FROM {} WHERE tenant_id = $1 RETURNING schema_name",
            qualified_table(&self.metadata_schema, "tenants")
        );
        let Some(row) = transaction
            .query_opt(delete_query.as_str(), &[&tenant_id.as_str()])
            .await
            .map_err(map_postgres_error)?
        else {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        };

        let schema_name: String = row.get(0);
        let drop_schema_sql = format!(
            "DROP SCHEMA IF EXISTS {} CASCADE",
            quote_identifier(&schema_name)
        );
        transaction
            .batch_execute(drop_schema_sql.as_str())
            .await
            .map_err(map_postgres_error)?;
        transaction.commit().await.map_err(map_postgres_error)?;
        Ok(())
    }

    #[doc(hidden)]
    pub async fn drop_metadata_schema_for_test(&self) -> Result<()> {
        let client = self.client().await?;
        let drop_sql = format!(
            "DROP SCHEMA IF EXISTS {} CASCADE",
            quote_identifier(&self.metadata_schema)
        );
        client
            .batch_execute(drop_sql.as_str())
            .await
            .map_err(map_postgres_error)
    }

    fn open_registration(
        &self,
        registration: PostgresTenantRegistration,
    ) -> Result<OpenedPostgresTenant> {
        let store = Arc::new(PostgresTenantStore::new(self.clone(), registration));
        let read_storage = self.read_storage_for_store(store.clone());
        Ok(OpenedPostgresTenant {
            store,
            read_storage,
        })
    }

    async fn ensure_metadata_schema(&self) -> Result<()> {
        let client = self.client().await?;
        let metadata_schema = quote_identifier(&self.metadata_schema);
        let bootstrap = format!(
            "CREATE SCHEMA IF NOT EXISTS {metadata_schema}; \
             CREATE TABLE IF NOT EXISTS {metadata_schema}.tenants (\
                 tenant_id TEXT PRIMARY KEY,\
                 schema_name TEXT NOT NULL UNIQUE,\
                 created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()\
             )"
        );
        client
            .batch_execute(bootstrap.as_str())
            .await
            .map_err(map_postgres_error)
    }

    pub(super) async fn client(&self) -> Result<Client> {
        self.pool.get().await.map_err(map_pool_error)
    }
}
