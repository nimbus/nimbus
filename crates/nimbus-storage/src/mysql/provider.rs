use super::*;

impl MySqlProvider {
    pub async fn connect(config: MySqlProviderConfig) -> Result<Self> {
        Self::connect_with_simulation(
            config,
            TokioRuntimeHandle::current(),
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
        )
        .await
    }

    pub async fn connect_with_runtime(
        config: MySqlProviderConfig,
        runtime_handle: TokioRuntimeHandle,
    ) -> Result<Self> {
        Self::connect_with_simulation(
            config,
            runtime_handle,
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
        )
        .await
    }

    pub async fn connect_with_simulation(
        config: MySqlProviderConfig,
        runtime_handle: TokioRuntimeHandle,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        validate_identifier_input(&config.metadata_database, "metadata database")?;
        validate_identifier_input(&config.tenant_database_prefix, "tenant database prefix")?;

        let pool = build_pool(&config)?;
        let provider = Self {
            pool,
            metadata_database: config.metadata_database,
            tenant_database_prefix: config.tenant_database_prefix,
            runtime_handle,
            clock,
            fault_injector,
            tenant_read_parallelism: default_mysql_read_parallelism(),
        };
        provider.ensure_metadata_database().await?;
        Ok(provider)
    }

    pub fn metadata_database(&self) -> &str {
        &self.metadata_database
    }

    pub fn tenant_database_name(&self, tenant_id: &TenantId) -> Result<String> {
        tenant_database_name(&self.tenant_database_prefix, tenant_id)
    }

    pub fn read_storage_for_store(&self, store: Arc<MySqlTenantStore>) -> Arc<MySqlTenantStorage> {
        Arc::new(MySqlTenantStorage::with_max_concurrent_reads(
            store,
            self.runtime_handle.clone(),
            self.tenant_read_parallelism,
        ))
    }

    pub async fn create_opened_tenant(&self, tenant_id: &TenantId) -> Result<OpenedMySqlTenant> {
        let registration = self.create_tenant(tenant_id).await?;
        Ok(self.open_registration(registration))
    }

    pub async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedMySqlTenant>> {
        self.open_existing_tenant(tenant_id)
            .await?
            .map(|registration| Ok(self.open_registration(registration)))
            .transpose()
    }

    pub async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        let mut conn = self.conn().await?;
        let query = format!(
            "SELECT tenant_id FROM {} ORDER BY tenant_id",
            qualified_table(&self.metadata_database, "tenants")
        );
        let rows: Vec<Row> = conn.query(query).await.map_err(map_mysql_error)?;
        rows.into_iter()
            .map(|row| {
                let (tenant_id,): (String,) = mysql_async::from_row(row);
                TenantId::new(tenant_id)
            })
            .collect()
    }

    pub async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        let mut conn = self.conn().await?;
        let query = format!(
            "SELECT database_name FROM {} WHERE tenant_id = ?",
            qualified_table(&self.metadata_database, "tenants")
        );
        let row = conn
            .exec_first::<Row, _, _>(query, (tenant_id.as_str(),))
            .await
            .map_err(map_mysql_error)?;
        Ok(row.is_some())
    }

    pub async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<MySqlTenantRegistration>> {
        let mut conn = self.conn().await?;
        let query = format!(
            "SELECT database_name FROM {} WHERE tenant_id = ?",
            qualified_table(&self.metadata_database, "tenants")
        );
        let row = conn
            .exec_first::<Row, _, _>(query, (tenant_id.as_str(),))
            .await
            .map_err(map_mysql_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let (database_name,): (String,) = mysql_async::from_row(row);
        if !database_exists(&mut conn, &database_name).await? {
            return Err(Error::Internal(format!(
                "tenant registry points at missing MySQL database '{database_name}'"
            )));
        }
        Ok(Some(MySqlTenantRegistration {
            tenant_id: tenant_id.clone(),
            database_name,
        }))
    }

    pub async fn create_tenant(&self, tenant_id: &TenantId) -> Result<MySqlTenantRegistration> {
        let mut conn = self.conn().await?;
        let fetch_query = format!(
            "SELECT database_name FROM {} WHERE tenant_id = ?",
            qualified_table(&self.metadata_database, "tenants")
        );
        if conn
            .exec_first::<Row, _, _>(fetch_query, (tenant_id.as_str(),))
            .await
            .map_err(map_mysql_error)?
            .is_some()
        {
            return Err(Error::AlreadyExists(format!(
                "tenant already exists: {tenant_id}"
            )));
        }

        let database_name = self.tenant_database_name(tenant_id)?;
        let create_database_sql = format!("CREATE DATABASE {}", quote_identifier(&database_name));
        if let Err(error) = conn.query_drop(create_database_sql).await {
            if mysql_server_error_code(&error) == Some(1007) {
                return Err(Error::AlreadyExists(format!(
                    "tenant already exists: {tenant_id}"
                )));
            }
            return Err(map_mysql_error(error));
        }
        if let Err(error) = initialize_tenant_database(&mut conn, &database_name).await {
            let cleanup_sql = format!(
                "DROP DATABASE IF EXISTS {}",
                quote_identifier(&database_name)
            );
            let _ = conn.query_drop(cleanup_sql).await;
            return Err(error);
        }

        let insert_query = format!(
            "INSERT INTO {} (tenant_id, database_name) VALUES (?, ?)",
            qualified_table(&self.metadata_database, "tenants")
        );
        if let Err(error) = conn
            .exec_drop(insert_query, (tenant_id.as_str(), database_name.as_str()))
            .await
        {
            let cleanup_sql = format!(
                "DROP DATABASE IF EXISTS {}",
                quote_identifier(&database_name)
            );
            let _ = conn.query_drop(cleanup_sql).await;
            if mysql_server_error_code(&error) == Some(1062) {
                return Err(Error::AlreadyExists(format!(
                    "tenant already exists: {tenant_id}"
                )));
            }
            return Err(map_mysql_error(error));
        }

        Ok(MySqlTenantRegistration {
            tenant_id: tenant_id.clone(),
            database_name,
        })
    }

    pub async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        let mut conn = self.conn().await?;
        let fetch_query = format!(
            "SELECT database_name FROM {} WHERE tenant_id = ?",
            qualified_table(&self.metadata_database, "tenants")
        );
        let Some(row) = conn
            .exec_first::<Row, _, _>(fetch_query, (tenant_id.as_str(),))
            .await
            .map_err(map_mysql_error)?
        else {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        };
        let (database_name,): (String,) = mysql_async::from_row(row);

        let drop_database_sql = format!("DROP DATABASE {}", quote_identifier(&database_name));
        conn.query_drop(drop_database_sql)
            .await
            .map_err(map_mysql_error)?;

        let delete_query = format!(
            "DELETE FROM {} WHERE tenant_id = ?",
            qualified_table(&self.metadata_database, "tenants")
        );
        conn.exec_drop(delete_query, (tenant_id.as_str(),))
            .await
            .map_err(map_mysql_error)?;
        Ok(())
    }

    #[doc(hidden)]
    pub async fn drop_provider_databases_for_test(&self) -> Result<()> {
        let mut conn = self.conn().await?;
        if !database_exists(&mut conn, &self.metadata_database).await? {
            return Ok(());
        }

        let query = format!(
            "SELECT database_name FROM {}",
            qualified_table(&self.metadata_database, "tenants")
        );
        let rows: Vec<Row> = conn.query(query).await.map_err(map_mysql_error)?;
        for row in rows {
            let (database_name,): (String,) = mysql_async::from_row(row);
            let drop_tenant_sql = format!(
                "DROP DATABASE IF EXISTS {}",
                quote_identifier(&database_name)
            );
            conn.query_drop(drop_tenant_sql)
                .await
                .map_err(map_mysql_error)?;
        }

        let drop_metadata_sql = format!(
            "DROP DATABASE IF EXISTS {}",
            quote_identifier(&self.metadata_database)
        );
        conn.query_drop(drop_metadata_sql)
            .await
            .map_err(map_mysql_error)
    }

    fn open_registration(&self, registration: MySqlTenantRegistration) -> OpenedMySqlTenant {
        let store = Arc::new(MySqlTenantStore::new(self.clone(), registration));
        let read_storage = self.read_storage_for_store(store.clone());
        OpenedMySqlTenant {
            store,
            read_storage,
        }
    }

    async fn ensure_metadata_database(&self) -> Result<()> {
        let mut conn = self.conn().await?;
        let create_database_sql = format!(
            "CREATE DATABASE IF NOT EXISTS {}",
            quote_identifier(&self.metadata_database)
        );
        conn.query_drop(create_database_sql)
            .await
            .map_err(map_mysql_error)?;
        let bootstrap = format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                tenant_id VARCHAR(191) PRIMARY KEY,\
                database_name VARCHAR(191) NOT NULL UNIQUE,\
                created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6)\
            ) ENGINE=InnoDB",
            qualified_table(&self.metadata_database, "tenants")
        );
        conn.query_drop(bootstrap).await.map_err(map_mysql_error)
    }

    pub(super) async fn conn(&self) -> Result<Conn> {
        self.pool.get_conn().await.map_err(map_mysql_error)
    }
}
