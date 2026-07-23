use super::*;
use nimbus_crypto::{KeyManifest, LocalKeySubject, ManifestCipher, resolve_subject_encryption_key};

impl LibsqlReplicaProvider {
    /// Retires provider-global metadata and scheduler-probe transports after
    /// all engine-owned tenant and background work has drained.
    pub async fn retire_after_drain(&self) -> Result<()> {
        let probe_sessions = self
            .scheduler_probe_sessions
            .lock()
            .map_err(|_| {
                Error::Internal("libsql scheduler probe session lock is poisoned".to_string())
            })?
            .drain();
        let mut first_error = None;
        for session in probe_sessions {
            if let Err(error) = session.retire_after_drain().await
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        if let Err(error) = self.metadata_session.retire_after_drain().await
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        first_error.map_or(Ok(()), Err)
    }

    pub async fn connect(config: LibsqlReplicaProviderConfig) -> Result<Self> {
        Self::connect_with_simulation(
            config,
            TokioRuntimeHandle::current(),
            Arc::new(SystemWallClock),
            Arc::new(NoopFaultInjector),
        )
        .await
    }

    pub async fn connect_with_simulation(
        config: LibsqlReplicaProviderConfig,
        runtime_handle: TokioRuntimeHandle,
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        Self::connect_with_simulation_faults(
            config,
            runtime_handle,
            clock,
            fault_injector.clone(),
            fault_injector,
        )
        .await
    }

    #[doc(hidden)]
    pub async fn connect_with_simulation_faults(
        config: LibsqlReplicaProviderConfig,
        runtime_handle: TokioRuntimeHandle,
        clock: Arc<dyn WallClock>,
        remote_fault_injector: Arc<dyn FaultInjector>,
        replica_fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        Self::connect_with_simulation_faults_and_id_source(
            config,
            runtime_handle,
            clock,
            remote_fault_injector,
            replica_fault_injector,
            Arc::new(SystemIdSource),
        )
        .await
    }

    #[doc(hidden)]
    pub async fn connect_with_simulation_faults_and_id_source(
        config: LibsqlReplicaProviderConfig,
        runtime_handle: TokioRuntimeHandle,
        clock: Arc<dyn WallClock>,
        remote_fault_injector: Arc<dyn FaultInjector>,
        replica_fault_injector: Arc<dyn FaultInjector>,
        id_source: Arc<dyn IdSource>,
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

        let metadata_database = open_remote_database(
            &config.primary_url,
            config.auth_token.as_deref(),
            &config.metadata_namespace,
        )
        .await?;
        let metadata_session = LibsqlRemoteSession::new(metadata_database)?;
        let provider = Self {
            primary_url: config.primary_url,
            auth_token: config.auth_token,
            admin_api_url: config.admin_api_url,
            admin_auth_header: config.admin_auth_header,
            metadata_namespace: config.metadata_namespace,
            tenant_namespace_prefix: config.tenant_namespace_prefix,
            replica_cache_dir: config.replica_cache_dir,
            encryption_provider: config.encryption_provider,
            runtime_handle,
            clock,
            id_source,
            remote_fault_injector,
            replica_fault_injector,
            tenant_read_parallelism: LIBSQL_TENANT_READ_PARALLELISM,
            metadata_session,
            scheduler_probe_sessions: Arc::new(Mutex::new(BoundedSchedulerProbeSessions::new(
                LIBSQL_SCHEDULER_PROBE_SESSION_LIMIT,
            ))),
            #[cfg(test)]
            scheduler_probe_session_open_count: Arc::new(AtomicU64::new(0)),
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

    /// Returns whether local replica cache files are encrypted.
    pub fn is_encrypted(&self) -> bool {
        self.encryption_provider.is_some()
    }

    pub fn replica_path_for_tenant(&self, tenant_id: &TenantId) -> PathBuf {
        self.replica_cache_dir
            .join(tenant_id.as_str())
            .join(LIBSQL_REPLICA_FILENAME)
    }

    fn replica_cache_subject(&self, tenant_id: &TenantId) -> LocalKeySubject {
        LocalKeySubject::libsql_cache(tenant_id.clone(), LIBSQL_REPLICA_FILENAME)
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
        let registration = self
            .create_tenant(tenant_id) // tenant-lifecycle: provider-adapter-internal
            .await?;
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

    /// Opens an existing tenant only when its durable scheduler tables contain work.
    ///
    /// The provider poller calls this for unloaded tenants. Inspecting the
    /// remote scheduler tables before materializing the local replica keeps a
    /// poll proportional to the small scheduler predicate rather than a full
    /// namespace snapshot. A false result is advisory: a concurrent scheduler
    /// write is observed by a later bounded poll.
    pub async fn open_existing_opened_tenant_with_scheduled_work(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedLibsqlReplicaTenant>> {
        let Some(registration) = self.load_tenant_registration(tenant_id).await? else {
            return Ok(None);
        };
        let remote_session = self
            .scheduler_probe_session(&registration.tenant_id, &registration.namespace)
            .await?;
        let has_scheduled_work = retry_idempotent_remote_operation(
            &remote_session,
            "inspect unloaded libsql tenant scheduler state",
            |conn| async move {
                Ok(table_has_entries_remote(&conn, "scheduled_jobs").await?
                    || table_has_entries_remote(&conn, "running_scheduled_jobs").await?
                    || table_has_entries_remote(&conn, "cron_jobs").await?)
            },
        )
        .await?;
        if !has_scheduled_work {
            return Ok(None);
        }
        self.take_scheduler_probe_session(&registration.tenant_id, &registration.namespace)
            .await?;
        let opened = self
            .open_registration_with_session(registration, remote_session.clone())
            .await;
        if opened.is_err() {
            let _ = remote_session.retire_after_drain().await;
        }
        opened.map(Some)
    }

    pub async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        retry_idempotent_remote_operation(
            &self.metadata_session,
            "list provider tenants",
            |conn| async move {
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
            },
        )
        .await
    }

    /// Lists one ordered tenant-registration page strictly after `after`.
    ///
    /// Provider pollers use this instead of materializing the full registry.
    /// Callers advance with the last returned tenant and reset to `None` after
    /// a short page to begin the next sweep.
    pub async fn list_tenants_page(
        &self,
        after: Option<&TenantId>,
        limit: usize,
    ) -> Result<Vec<TenantId>> {
        if limit == 0 {
            return Err(Error::InvalidInput(
                "libsql tenant page limit must be greater than zero".to_string(),
            ));
        }
        let after = after.cloned();
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        retry_idempotent_remote_operation(
            &self.metadata_session,
            "list provider tenant page",
            |conn| {
                let after = after.clone();
                async move {
                    let mut rows = match after {
                        Some(after) => {
                            conn.query(
                                "SELECT tenant_id FROM tenants \
                                 WHERE tenant_id > ?1 ORDER BY tenant_id LIMIT ?2",
                                libsql::params![after.as_str(), limit],
                            )
                            .await
                        }
                        None => {
                            conn.query(
                                "SELECT tenant_id FROM tenants ORDER BY tenant_id LIMIT ?1",
                                libsql::params![limit],
                            )
                            .await
                        }
                    }
                    .map_err(map_libsql_error)?;
                    let mut tenants = Vec::new();
                    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
                        tenants.push(TenantId::new(
                            row.get::<String>(0).map_err(map_libsql_error)?,
                        )?);
                    }
                    Ok(tenants)
                }
            },
        )
        .await
    }

    pub async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        let tenant_id = tenant_id.clone();
        retry_idempotent_remote_operation(
            &self.metadata_session,
            "inspect libsql tenant registration",
            |conn| {
                let tenant_id = tenant_id.clone();
                async move {
                    let rows = conn
                        .query(
                            "SELECT 1 FROM tenants WHERE tenant_id = ?",
                            libsql::params![tenant_id.as_str()],
                        )
                        .await
                        .map_err(map_libsql_error)?;
                    Ok(take_single_remote_row(rows).await?.is_some())
                }
            },
        )
        .await
    }

    pub async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<LibsqlReplicaTenantRegistration>> {
        let Some(registration) = self.load_tenant_registration(tenant_id).await? else {
            return Ok(None);
        };
        if !tenant_namespace_has_foundation(
            &self.primary_url,
            self.auth_token.as_deref(),
            &registration.namespace,
        )
        .await?
        {
            return Err(Error::Internal(format!(
                "tenant registry points at missing libsql namespace '{}'",
                registration.namespace
            )));
        }
        Ok(Some(registration))
    }

    async fn load_tenant_registration(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<LibsqlReplicaTenantRegistration>> {
        let query_tenant_id = tenant_id.clone();
        let registration = retry_idempotent_remote_operation(
            &self.metadata_session,
            "load libsql tenant registration",
            |conn| {
                let tenant_id = query_tenant_id.clone();
                async move {
                    let rows = conn
                        .query(
                            "SELECT tenants.namespace, tenant_incarnations.incarnation \
                             FROM tenants LEFT JOIN tenant_incarnations USING (tenant_id) \
                             WHERE tenants.tenant_id = ?",
                            libsql::params![tenant_id.as_str()],
                        )
                        .await
                        .map_err(map_libsql_error)?;
                    let Some(row) = take_single_remote_row(rows).await? else {
                        return Ok(None);
                    };
                    let namespace = row.get::<String>(0).map_err(map_libsql_error)?;
                    let incarnation = incarnation_from_i64(
                        row.get::<Option<i64>>(1).map_err(map_libsql_error)?,
                        &tenant_id,
                    )?;
                    Ok(Some((namespace, incarnation)))
                }
            },
        )
        .await?;
        let Some((namespace, incarnation)) = registration else {
            return Ok(None);
        };
        Ok(Some(LibsqlReplicaTenantRegistration {
            tenant_id: tenant_id.clone(),
            namespace,
            incarnation,
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
        self.remote_fault_injector
            .check_for_tenant(crate::FaultPoint::TenantCreateBeforeRegistration, tenant_id)?;
        let conn = self.metadata_write_connection()?;
        let transaction = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(map_libsql_error)?;
        let registration = async {
            transaction
                .execute(
                    "INSERT INTO tenant_incarnations (tenant_id, incarnation) VALUES (?, 1) \
                     ON CONFLICT (tenant_id) DO UPDATE \
                     SET incarnation = tenant_incarnations.incarnation + 1",
                    libsql::params![tenant_id.as_str()],
                )
                .await
                .map_err(map_libsql_error)?;
            let rows = transaction
                .query(
                    "SELECT incarnation FROM tenant_incarnations WHERE tenant_id = ?",
                    libsql::params![tenant_id.as_str()],
                )
                .await
                .map_err(map_libsql_error)?;
            let row = take_single_remote_row(rows).await?.ok_or_else(|| {
                Error::Internal(format!(
                    "tenant incarnation allocation disappeared for {tenant_id}"
                ))
            })?;
            let incarnation = incarnation_from_i64(
                Some(row.get::<i64>(0).map_err(map_libsql_error)?),
                tenant_id,
            )?;
            let inserted = transaction
                .execute(
                    "INSERT INTO tenants (tenant_id, namespace) VALUES (?, ?) \
                     ON CONFLICT (tenant_id) DO NOTHING",
                    libsql::params![tenant_id.as_str(), namespace.as_str()],
                )
                .await
                .map_err(map_libsql_error)?;
            if inserted == 0 {
                return Err(Error::AlreadyExists(format!(
                    "tenant '{}' already exists",
                    tenant_id.as_str()
                )));
            }
            transaction.commit().await.map_err(map_libsql_error)?;
            Ok::<_, Error>(incarnation)
        }
        .await;
        let incarnation = match registration {
            Ok(incarnation) => incarnation,
            // The deterministic namespace may have been created or adopted by
            // a concurrent creator. Registration is the ownership boundary:
            // a loser must leave the namespace intact because deleting it can
            // destroy the winner's live tenant. An unregistered bootstrap is
            // safe to reuse on a later create attempt.
            Err(error) => return Err(error),
        };
        Ok(LibsqlReplicaTenantRegistration {
            tenant_id: tenant_id.clone(),
            namespace,
            incarnation,
        })
    }

    pub async fn refresh_tenant_snapshot(&self, tenant_id: &TenantId) -> Result<PathBuf> {
        let Some(registration) = self.open_existing_tenant(tenant_id).await? else {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        };
        let remote_session = self.open_remote_session(&registration.namespace).await?;
        self.sync_registration_snapshot(&registration, &remote_session)
            .await
    }

    pub async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        let Some(registration) = self.open_existing_tenant(tenant_id).await? else {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        };
        if let Some(session) = self.remove_scheduler_probe_session(tenant_id)? {
            session.retire_after_drain().await?;
        }
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
        let tenant_id_for_delete = tenant_id.clone();
        retry_idempotent_remote_operation(
            &self.metadata_session,
            "delete libsql tenant registration",
            |conn| {
                let tenant_id = tenant_id_for_delete.clone();
                async move {
                    conn.execute(
                        "DELETE FROM tenants WHERE tenant_id = ?",
                        libsql::params![tenant_id.as_str()],
                    )
                    .await
                    .map_err(map_libsql_error)?;
                    Ok(())
                }
            },
        )
        .await?;
        let replica_dir = self.replica_dir_for_tenant(tenant_id);
        if replica_dir.exists() {
            std::fs::remove_dir_all(&replica_dir).map_err(storage_io_error)?;
        }
        if self.encryption_provider.is_some() {
            let manifest_path =
                KeyManifest::manifest_path(&self.replica_path_for_tenant(tenant_id));
            if manifest_path.exists() {
                let _ = std::fs::remove_file(manifest_path);
            }
        }
        Ok(())
    }

    async fn sync_registration_snapshot(
        &self,
        registration: &LibsqlReplicaTenantRegistration,
        remote_session: &LibsqlRemoteSession,
    ) -> Result<PathBuf> {
        let snapshot = fetch_remote_namespace_snapshot(remote_session).await?;
        let replica_path = self.replica_path_for_tenant(&registration.tenant_id);
        let path_for_publish = replica_path.clone();
        let replica_dir = self.replica_dir_for_tenant(&registration.tenant_id);
        let subject = self.replica_cache_subject(&registration.tenant_id);
        let provider = self.encryption_provider.clone();
        self.runtime_handle
            .spawn_blocking(move || {
                let encryption_dek = if let Some(provider) = provider {
                    Some(resolve_subject_encryption_key(
                        path_for_publish.as_path(),
                        provider.as_ref(),
                        &subject,
                        ManifestCipher::SqlCipher,
                    )?)
                } else {
                    None
                };
                materialize_snapshot_to_replica_cache(
                    replica_dir.as_path(),
                    path_for_publish.as_path(),
                    snapshot,
                    encryption_dek.as_ref().map(|key| key.as_bytes()),
                )
            })
            .await
            .map_err(|error| map_executor_join_error(LIBSQL_REPLICA_EXECUTOR_CONTEXT, error))??;
        Ok(replica_path)
    }

    pub async fn drop_provider_namespaces_for_test(&self) -> Result<()> {
        let tenants = self.list_tenants().await?;
        for tenant_id in tenants {
            self.delete_tenant(&tenant_id).await?;
        }
        let conn = self.metadata_write_connection()?;
        conn.execute_batch(
            "DROP TABLE IF EXISTS tenants; DROP TABLE IF EXISTS tenant_incarnations",
        )
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
        let remote_session = match self
            .take_scheduler_probe_session(&registration.tenant_id, &registration.namespace)
            .await?
        {
            Some(session) => session,
            None => self.open_remote_session(&registration.namespace).await?,
        };
        self.open_registration_with_session(registration, remote_session)
            .await
    }

    async fn open_registration_with_session(
        &self,
        registration: LibsqlReplicaTenantRegistration,
        remote_session: LibsqlRemoteSession,
    ) -> Result<OpenedLibsqlReplicaTenant> {
        let incarnation = registration.incarnation;
        let replica_path = self
            .sync_registration_snapshot(&registration, &remote_session)
            .await?;
        let clock = self.clock.clone();
        let fault_injector = crate::simulation::tenant_scoped_fault_injector(
            self.replica_fault_injector.clone(),
            registration.tenant_id.clone(),
        );
        let id_source = self.id_source.clone();
        let path_for_open = replica_path.clone();
        let read_parallelism = self.tenant_read_parallelism;
        let provider = self.encryption_provider.clone();
        let subject = self.replica_cache_subject(&registration.tenant_id);
        let local_store = self
            .runtime_handle
            .spawn_blocking(move || {
                if let Some(provider) = provider {
                    let key = resolve_subject_encryption_key(
                        &path_for_open,
                        provider.as_ref(),
                        &subject,
                        ManifestCipher::SqlCipher,
                    )?;
                    SqliteTenantStore::open_encrypted_with_simulation_and_max_read_connections_and_id_source(
                        path_for_open,
                        &key,
                        clock,
                        fault_injector,
                        read_parallelism,
                        id_source,
                    )
                } else {
                    SqliteTenantStore::open_with_simulation_and_max_read_connections_and_id_source(
                        path_for_open,
                        clock,
                        fault_injector,
                        read_parallelism,
                        id_source,
                    )
                }
            })
            .await
            .map_err(|error| map_executor_join_error(LIBSQL_REPLICA_EXECUTOR_CONTEXT, error))??;
        let store = Arc::new(LibsqlReplicaTenantStore::new(
            self.clone(),
            registration.tenant_id.clone(),
            registration.namespace.clone(),
            remote_session,
            Arc::new(local_store),
            replica_path.clone(),
        ));
        let read_storage = self.read_storage_for_store(store.clone());
        Ok(OpenedLibsqlReplicaTenant {
            store,
            read_storage,
            incarnation,
            tenant_id: registration.tenant_id,
            namespace: registration.namespace,
            replica_path,
            primary_url: self.primary_url.clone(),
        })
    }

    async fn ensure_metadata_namespace(&self) -> Result<()> {
        retry_idempotent_remote_operation(
            &self.metadata_session,
            "initialize provider metadata namespace",
            |conn| async move {
                conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS tenants (
                        tenant_id TEXT NOT NULL PRIMARY KEY,
                        namespace TEXT NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS tenant_incarnations (
                        tenant_id TEXT NOT NULL PRIMARY KEY,
                        incarnation INTEGER NOT NULL CHECK (incarnation > 0)
                    );",
                )
                .await
                .map_err(map_libsql_error)?;
                Ok(())
            },
        )
        .await
    }

    fn metadata_write_connection(&self) -> Result<Connection> {
        self.metadata_session.write_connection()
    }

    async fn open_remote_session(&self, namespace: &str) -> Result<LibsqlRemoteSession> {
        let database =
            open_remote_database(&self.primary_url, self.auth_token.as_deref(), namespace).await?;
        LibsqlRemoteSession::new(database)
    }

    async fn scheduler_probe_session(
        &self,
        tenant_id: &TenantId,
        namespace: &str,
    ) -> Result<LibsqlRemoteSession> {
        let (cached, stale) = self
            .scheduler_probe_sessions
            .lock()
            .map_err(|_| {
                Error::Internal("libsql scheduler probe session lock is poisoned".to_string())
            })?
            .get(tenant_id, namespace);
        if let Some(stale) = stale {
            stale.retire_after_drain().await?;
        }
        if let Some(cached) = cached {
            return Ok(cached);
        }

        #[cfg(test)]
        self.scheduler_probe_session_open_count
            .fetch_add(1, Ordering::Relaxed);
        let opened = self.open_remote_session(namespace).await?;
        let (cached, retired) = {
            let mut sessions = self.scheduler_probe_sessions.lock().map_err(|_| {
                Error::Internal("libsql scheduler probe session lock is poisoned".to_string())
            })?;
            let (cached, stale) = sessions.get(tenant_id, namespace);
            if let Some(cached) = cached {
                (cached, Some(opened))
            } else {
                debug_assert!(
                    stale.is_none(),
                    "namespace mismatch was removed before opening a scheduler probe session"
                );
                let retired =
                    sessions.insert(tenant_id.clone(), namespace.to_string(), opened.clone());
                (opened, retired)
            }
        };
        if let Some(retired) = retired {
            retired.retire_after_drain().await?;
        }
        Ok(cached)
    }

    async fn take_scheduler_probe_session(
        &self,
        tenant_id: &TenantId,
        namespace: &str,
    ) -> Result<Option<LibsqlRemoteSession>> {
        let (matched, stale) = self
            .scheduler_probe_sessions
            .lock()
            .map_err(|_| {
                Error::Internal("libsql scheduler probe session lock is poisoned".to_string())
            })?
            .take(tenant_id, namespace);
        if let Some(stale) = stale {
            stale.retire_after_drain().await?;
        }
        Ok(matched)
    }

    fn remove_scheduler_probe_session(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<LibsqlRemoteSession>> {
        self.scheduler_probe_sessions
            .lock()
            .map_err(|_| {
                Error::Internal("libsql scheduler probe session lock is poisoned".to_string())
            })
            .map(|mut sessions| sessions.remove(tenant_id))
    }

    #[cfg(test)]
    pub(crate) fn scheduler_probe_session_stats_for_testing(&self) -> (u64, usize) {
        let open_count = self
            .scheduler_probe_session_open_count
            .load(Ordering::Relaxed);
        let cached_count = self
            .scheduler_probe_sessions
            .lock()
            .expect("libsql scheduler probe session lock should not be poisoned")
            .len();
        (open_count, cached_count)
    }

    pub(super) fn replica_dir_for_tenant(&self, tenant_id: &TenantId) -> PathBuf {
        self.replica_cache_dir.join(tenant_id.as_str())
    }
}

fn incarnation_from_i64(value: Option<i64>, tenant_id: &TenantId) -> Result<u64> {
    crate::tenant_incarnation::require_tenant_incarnation(
        value.and_then(|value| u64::try_from(value).ok()),
        tenant_id,
    )
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

#[cfg(test)]
mod scheduler_probe_session_tests {
    use super::*;

    fn tenant(name: &str) -> TenantId {
        TenantId::new(name).expect("test tenant id should parse")
    }

    #[test]
    fn matching_probe_session_is_reused_and_transferred() {
        let tenant = tenant("probe-reuse");
        let mut sessions = BoundedSchedulerProbeSessions::new(2);
        assert_eq!(
            sessions.insert(tenant.clone(), "namespace-a".to_string(), "session-a"),
            None
        );

        assert_eq!(
            sessions.get(&tenant, "namespace-a"),
            (Some("session-a"), None)
        );
        assert_eq!(
            sessions.take(&tenant, "namespace-a"),
            (Some("session-a"), None)
        );
        assert_eq!(sessions.get(&tenant, "namespace-a"), (None, None));
    }

    #[test]
    fn namespace_replacement_retires_stale_probe_session() {
        let tenant = tenant("probe-replaced");
        let mut sessions = BoundedSchedulerProbeSessions::new(2);
        sessions.insert(tenant.clone(), "namespace-a".to_string(), "session-a");

        assert_eq!(
            sessions.get(&tenant, "namespace-b"),
            (None, Some("session-a"))
        );
        assert_eq!(sessions.get(&tenant, "namespace-b"), (None, None));
    }

    #[test]
    fn probe_session_cache_evicts_least_recently_used_entry_at_bound() {
        let tenant_a = tenant("probe-a");
        let tenant_b = tenant("probe-b");
        let tenant_c = tenant("probe-c");
        let mut sessions = BoundedSchedulerProbeSessions::new(2);
        sessions.insert(tenant_a.clone(), "namespace-a".to_string(), "session-a");
        sessions.insert(tenant_b.clone(), "namespace-b".to_string(), "session-b");
        assert_eq!(
            sessions.get(&tenant_a, "namespace-a"),
            (Some("session-a"), None)
        );

        assert_eq!(
            sessions.insert(tenant_c.clone(), "namespace-c".to_string(), "session-c"),
            Some("session-b")
        );
        assert_eq!(sessions.get(&tenant_b, "namespace-b"), (None, None));
        assert_eq!(
            sessions.get(&tenant_a, "namespace-a"),
            (Some("session-a"), None)
        );
        assert_eq!(
            sessions.get(&tenant_c, "namespace-c"),
            (Some("session-c"), None)
        );
    }

    #[test]
    fn probe_session_removal_and_shutdown_drain_return_owned_sessions() {
        let tenant_a = tenant("probe-remove");
        let tenant_b = tenant("probe-drain");
        let mut sessions = BoundedSchedulerProbeSessions::new(2);
        sessions.insert(tenant_a.clone(), "namespace-a".to_string(), "session-a");
        sessions.insert(tenant_b, "namespace-b".to_string(), "session-b");

        assert_eq!(sessions.remove(&tenant_a), Some("session-a"));
        assert_eq!(sessions.remove(&tenant_a), None);
        assert_eq!(sessions.drain(), vec!["session-b"]);
        assert!(sessions.drain().is_empty());
    }
}
