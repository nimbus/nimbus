use super::*;

impl LibsqlReplicaProvider {
    pub async fn connect(config: LibsqlReplicaProviderConfig) -> Result<Self> {
        Self::connect_with_simulation(
            config,
            TokioRuntimeHandle::current(),
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
        )
        .await
    }

    pub async fn connect_with_simulation(
        config: LibsqlReplicaProviderConfig,
        runtime_handle: TokioRuntimeHandle,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        validate_namespace_input(&config.metadata_namespace, "metadata namespace")?;
        validate_namespace_input(&config.tenant_namespace_prefix, "tenant namespace prefix")?;
        if config.admin_api_url.trim().is_empty() {
            return Err(Error::InvalidInput(
                "libsql admin API URL cannot be empty".to_string(),
            ));
        }
        std::fs::create_dir_all(&config.replica_cache_dir).map_err(storage_io_error)?;
        ensure_remote_namespace_exists(
            &config.admin_api_url,
            config.admin_auth_header.as_deref(),
            &config.metadata_namespace,
        )
        .await?;

        let metadata_database = Arc::new(
            open_remote_database(
                &config.primary_url,
                config.auth_token.as_deref(),
                &config.metadata_namespace,
            )
            .await?,
        );
        let provider = Self {
            primary_url: config.primary_url,
            auth_token: config.auth_token,
            admin_api_url: config.admin_api_url,
            admin_auth_header: config.admin_auth_header,
            metadata_namespace: config.metadata_namespace,
            tenant_namespace_prefix: config.tenant_namespace_prefix,
            replica_cache_dir: config.replica_cache_dir,
            runtime_handle,
            clock,
            fault_injector,
            tenant_read_parallelism: LIBSQL_TENANT_READ_PARALLELISM,
            metadata_database,
        };
        provider.ensure_metadata_namespace().await?;
        Ok(provider)
    }

    pub fn metadata_namespace(&self) -> &str {
        &self.metadata_namespace
    }

    pub fn tenant_namespace(&self, tenant_id: &TenantId) -> Result<String> {
        tenant_namespace_name(&self.tenant_namespace_prefix, tenant_id)
    }

    pub fn replica_cache_root(&self) -> &Path {
        &self.replica_cache_dir
    }

    pub fn replica_path_for_tenant(&self, tenant_id: &TenantId) -> PathBuf {
        self.replica_cache_dir
            .join(tenant_id.as_str())
            .join(LIBSQL_REPLICA_FILENAME)
    }

    pub fn read_storage_for_store(
        &self,
        store: Arc<LibsqlReplicaTenantStore>,
    ) -> Arc<LibsqlReplicaTenantStorage> {
        Arc::new(LibsqlReplicaTenantStorage::with_max_concurrent_reads(
            store,
            self.runtime_handle.clone(),
            self.tenant_read_parallelism,
        ))
    }

    pub async fn create_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<OpenedLibsqlReplicaTenant> {
        let registration = self.create_tenant(tenant_id).await?;
        self.open_registration(registration).await
    }

    pub async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedLibsqlReplicaTenant>> {
        let Some(registration) = self.open_existing_tenant(tenant_id).await? else {
            return Ok(None);
        };
        self.open_registration(registration).await.map(Some)
    }

    pub async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        let conn = self.metadata_connection()?;
        let mut rows = conn
            .query("SELECT tenant_id FROM tenants ORDER BY tenant_id", ())
            .await
            .map_err(map_libsql_error)?;
        let mut tenants = Vec::new();
        while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
            let tenant_id = row.get::<String>(0).map_err(map_libsql_error)?;
            tenants.push(TenantId::new(tenant_id)?);
        }
        Ok(tenants)
    }

    pub async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        let conn = self.metadata_connection()?;
        let mut rows = conn
            .query(
                "SELECT namespace FROM tenants WHERE tenant_id = ?",
                libsql::params![tenant_id.as_str()],
            )
            .await
            .map_err(map_libsql_error)?;
        Ok(rows.next().await.map_err(map_libsql_error)?.is_some())
    }

    pub async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<LibsqlReplicaTenantRegistration>> {
        let conn = self.metadata_connection()?;
        let mut rows = conn
            .query(
                "SELECT namespace FROM tenants WHERE tenant_id = ?",
                libsql::params![tenant_id.as_str()],
            )
            .await
            .map_err(map_libsql_error)?;
        let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
            return Ok(None);
        };
        let namespace = row.get::<String>(0).map_err(map_libsql_error)?;
        if !tenant_namespace_has_foundation(
            &self.primary_url,
            self.auth_token.as_deref(),
            &namespace,
        )
        .await?
        {
            return Err(Error::Internal(format!(
                "tenant registry points at missing libsql namespace '{namespace}'"
            )));
        }
        Ok(Some(LibsqlReplicaTenantRegistration {
            tenant_id: tenant_id.clone(),
            namespace,
        }))
    }

    pub async fn create_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<LibsqlReplicaTenantRegistration> {
        if self.tenant_exists(tenant_id).await? {
            return Err(Error::AlreadyExists(format!(
                "tenant '{}' already exists",
                tenant_id.as_str()
            )));
        }
        let namespace = self.tenant_namespace(tenant_id)?;
        ensure_remote_namespace_exists(
            &self.admin_api_url,
            self.admin_auth_header.as_deref(),
            &namespace,
        )
        .await?;
        bootstrap_tenant_namespace(&self.primary_url, self.auth_token.as_deref(), &namespace)
            .await?;
        let conn = self.metadata_connection()?;
        conn.execute(
            "INSERT INTO tenants (tenant_id, namespace) VALUES (?, ?)",
            libsql::params![tenant_id.as_str(), namespace.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
        Ok(LibsqlReplicaTenantRegistration {
            tenant_id: tenant_id.clone(),
            namespace,
        })
    }

    pub async fn refresh_tenant_snapshot(&self, tenant_id: &TenantId) -> Result<PathBuf> {
        let Some(registration) = self.open_existing_tenant(tenant_id).await? else {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        };
        self.sync_registration_snapshot(&registration).await
    }

    pub async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        let Some(registration) = self.open_existing_tenant(tenant_id).await? else {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        };
        clear_tenant_namespace(
            &self.primary_url,
            self.auth_token.as_deref(),
            &registration.namespace,
        )
        .await?;
        drop_remote_namespace(
            &self.admin_api_url,
            self.admin_auth_header.as_deref(),
            &registration.namespace,
        )
        .await?;
        let conn = self.metadata_connection()?;
        conn.execute(
            "DELETE FROM tenants WHERE tenant_id = ?",
            libsql::params![tenant_id.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
        let replica_dir = self.replica_dir_for_tenant(tenant_id);
        if replica_dir.exists() {
            std::fs::remove_dir_all(&replica_dir).map_err(storage_io_error)?;
        }
        Ok(())
    }

    async fn sync_registration_snapshot(
        &self,
        registration: &LibsqlReplicaTenantRegistration,
    ) -> Result<PathBuf> {
        let snapshot = fetch_remote_namespace_snapshot(
            &self.primary_url,
            self.auth_token.as_deref(),
            &registration.namespace,
        )
        .await?;
        let replica_path = self.replica_path_for_tenant(&registration.tenant_id);
        let path_for_publish = replica_path.clone();
        let replica_dir = self.replica_dir_for_tenant(&registration.tenant_id);
        self.runtime_handle
            .spawn_blocking(move || {
                materialize_snapshot_to_replica_cache(
                    replica_dir.as_path(),
                    path_for_publish.as_path(),
                    snapshot,
                )
            })
            .await
            .map_err(map_join_error)??;
        Ok(replica_path)
    }

    pub async fn drop_provider_namespaces_for_test(&self) -> Result<()> {
        let tenants = self.list_tenants().await?;
        for tenant_id in tenants {
            self.delete_tenant(&tenant_id).await?;
        }
        let conn = self.metadata_connection()?;
        conn.execute_batch("DROP TABLE IF EXISTS tenants")
            .await
            .map_err(map_libsql_error)?;
        let _ = drop_remote_namespace(
            &self.admin_api_url,
            self.admin_auth_header.as_deref(),
            &self.metadata_namespace,
        )
        .await;
        Ok(())
    }

    async fn open_registration(
        &self,
        registration: LibsqlReplicaTenantRegistration,
    ) -> Result<OpenedLibsqlReplicaTenant> {
        let replica_path = self.sync_registration_snapshot(&registration).await?;
        let remote_database = Arc::new(
            open_remote_database(
                &self.primary_url,
                self.auth_token.as_deref(),
                &registration.namespace,
            )
            .await?,
        );
        let clock = self.clock.clone();
        let fault_injector = self.fault_injector.clone();
        let path_for_open = replica_path.clone();
        let read_parallelism = self.tenant_read_parallelism;
        let local_store = self
            .runtime_handle
            .spawn_blocking(move || {
                SqliteTenantStore::open_with_simulation_and_max_read_connections(
                    path_for_open,
                    clock,
                    fault_injector,
                    read_parallelism,
                )
            })
            .await
            .map_err(map_join_error)??;
        let store = Arc::new(LibsqlReplicaTenantStore::new(
            self.clone(),
            registration.tenant_id.clone(),
            registration.namespace.clone(),
            remote_database,
            Arc::new(local_store),
            replica_path.clone(),
        ));
        let read_storage = self.read_storage_for_store(store.clone());
        Ok(OpenedLibsqlReplicaTenant {
            store,
            read_storage,
            tenant_id: registration.tenant_id,
            namespace: registration.namespace,
            replica_path,
            primary_url: self.primary_url.clone(),
        })
    }

    async fn ensure_metadata_namespace(&self) -> Result<()> {
        let conn = self.metadata_connection()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tenants (
                tenant_id TEXT NOT NULL PRIMARY KEY,
                namespace TEXT NOT NULL
            );",
        )
        .await
        .map_err(map_libsql_error)?;
        Ok(())
    }

    fn metadata_connection(&self) -> Result<Connection> {
        self.metadata_database.connect().map_err(map_libsql_error)
    }

    pub(super) fn replica_dir_for_tenant(&self, tenant_id: &TenantId) -> PathBuf {
        self.replica_cache_dir.join(tenant_id.as_str())
    }
}

impl OpenedLibsqlReplicaTenant {
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn replica_path(&self) -> &Path {
        &self.replica_path
    }

    pub fn primary_url(&self) -> &str {
        &self.primary_url
    }
}
