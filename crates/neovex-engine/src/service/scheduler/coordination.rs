use std::sync::Arc;

use neovex_core::{Error, Result, TenantId, Timestamp};

use crate::persistence::TenantPersistence;
use crate::tenant::TenantRuntime;

use super::super::Service;
use super::access::{read_loaded_tenant_store, with_scheduler_runtime};

impl Service {
    /// Returns the IDs for all tenants currently loaded in memory.
    pub fn loaded_tenant_ids(&self) -> Vec<TenantId> {
        let mut tenant_ids = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        tenant_ids.sort();
        tenant_ids
    }

    /// Returns the earliest due scheduled or cron work across all loaded tenants asynchronously.
    pub(crate) async fn next_loaded_scheduled_work_at_async(
        self: &Arc<Self>,
    ) -> Result<Option<Timestamp>> {
        self.ensure_provider_background_tasks_started();
        let loaded_tenants = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .iter()
            .map(|(tenant_id, runtime)| (tenant_id.clone(), runtime.clone()))
            .collect::<Vec<_>>();

        let mut next_due: Option<Timestamp> = None;
        for (tenant_id, runtime) in loaded_tenants {
            if let Some(candidate) =
                next_loaded_tenant_scheduled_work_at(runtime, tenant_id).await?
            {
                next_due = Some(match next_due {
                    Some(current) => current.min(candidate),
                    None => candidate,
                });
            }
        }

        Ok(next_due)
    }

    /// Loads tenants that have scheduled work and recovers orphaned running jobs.
    pub fn load_tenants_with_scheduled_work(&self) -> Result<()> {
        let embedded_provider_kind = self.require_embedded_provider_kind()?;
        let entries = std::fs::read_dir(&self.data_dir)
            .map_err(|error| Error::Internal(error.to_string()))?;
        let now = self.now();

        for entry in entries {
            let entry = entry.map_err(|error| Error::Internal(error.to_string()))?;
            let path = entry.path();
            if path
                .extension()
                .is_none_or(|extension| extension != embedded_provider_kind.tenant_file_extension())
            {
                continue;
            }

            let stem = path.file_stem().ok_or_else(|| {
                Error::Internal(format!(
                    "tenant database path missing file stem: {}",
                    path.display()
                ))
            })?;
            let tenant_id = TenantId::new(stem.to_string_lossy().to_string())?;
            let store = self.open_tenant_store(&path)?;
            let has_scheduled_work = store.has_scheduled_work()?;
            drop(store);
            if !has_scheduled_work {
                continue;
            }

            with_scheduler_runtime(self, &tenant_id, move |runtime| {
                runtime.store.recover_running_jobs(now)
            })?;
        }

        Ok(())
    }

    /// Loads tenants with scheduled work asynchronously across the active provider.
    pub async fn load_tenants_with_scheduled_work_async(self: &Arc<Self>) -> Result<()> {
        self.load_tenants_with_scheduled_work_from_provider_async(true)
            .await
    }

    /// Preloads scheduled-work tenants during startup, recovers orphaned
    /// running jobs, and only then starts the provider hint worker for
    /// steady-state wake delivery. The regular scheduler loop remains the
    /// owner of actually executing due work after startup.
    pub async fn recover_scheduled_work_on_startup_async(self: &Arc<Self>) -> Result<()> {
        self.load_tenants_with_scheduled_work_from_provider_async(false)
            .await?;
        self.ensure_provider_background_tasks_started();
        Ok(())
    }

    async fn load_tenants_with_scheduled_work_from_provider_async(
        self: &Arc<Self>,
        start_provider_background_tasks: bool,
    ) -> Result<()> {
        if start_provider_background_tasks {
            self.ensure_provider_background_tasks_started();
        }
        let tenant_ids = self.persistence_provider.list_tenants().await?;
        let mut loaded_any = false;
        for tenant_id in tenant_ids {
            loaded_any |= self
                .load_tenant_with_scheduled_work_if_present(tenant_id)
                .await?;
        }
        if loaded_any {
            self.wake_scheduler();
        }
        Ok(())
    }

    pub(crate) async fn load_tenant_with_scheduled_work_if_present(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<bool> {
        let now = self.now();
        if let Some(runtime) = self.loaded_runtime_for_scheduler(&tenant_id) {
            let _operation = runtime.enter_operation(&tenant_id)?;
            let has_scheduled_work = match &runtime.store {
                TenantPersistence::Postgres(store) => store.has_scheduled_work_async().await?,
                TenantPersistence::Redb(_)
                | TenantPersistence::Sqlite(_)
                | TenantPersistence::SqliteReplica(_)
                | TenantPersistence::MySql(_) => {
                    runtime
                        .read_storage
                        .execute(|store| store.has_scheduled_work())
                        .await?
                }
            };
            if !has_scheduled_work {
                return Ok(false);
            }
            return Ok(true);
        }

        let _tenant_load_guard = self.tenant_load_gate.lock().await;
        if let Some(runtime) = self.loaded_runtime_for_scheduler(&tenant_id) {
            let _operation = runtime.enter_operation(&tenant_id)?;
            let has_scheduled_work = match &runtime.store {
                TenantPersistence::Postgres(store) => store.has_scheduled_work_async().await?,
                TenantPersistence::Redb(_)
                | TenantPersistence::Sqlite(_)
                | TenantPersistence::SqliteReplica(_)
                | TenantPersistence::MySql(_) => {
                    runtime
                        .read_storage
                        .execute(|store| store.has_scheduled_work())
                        .await?
                }
            };
            if !has_scheduled_work {
                return Ok(false);
            }
            return Ok(true);
        }

        let Some(opened) = self
            .persistence_provider
            .open_existing_tenant(&tenant_id)
            .await?
        else {
            return Ok(false);
        };
        let has_scheduled_work = match &opened.persistence {
            TenantPersistence::Postgres(store) => store.has_scheduled_work_async().await?,
            TenantPersistence::Redb(_)
            | TenantPersistence::Sqlite(_)
            | TenantPersistence::SqliteReplica(_)
            | TenantPersistence::MySql(_) => {
                opened
                    .executor
                    .execute(|store| store.has_scheduled_work())
                    .await?
            }
        };
        if !has_scheduled_work {
            return Ok(false);
        }

        let opened_executor = opened.executor.clone();
        let runtime = Arc::new(
            TenantRuntime::from_parts_async(opened.persistence.clone(), opened_executor).await?,
        );
        let progress = match &opened.persistence {
            TenantPersistence::Postgres(store) => store.recover_durable_journal_async().await?,
            TenantPersistence::Redb(_)
            | TenantPersistence::Sqlite(_)
            | TenantPersistence::SqliteReplica(_)
            | TenantPersistence::MySql(_) => {
                opened
                    .executor
                    .execute(|store| store.recover_durable_journal())
                    .await?
            }
        };
        runtime.sync_mutation_journal_progress(progress);
        if !self.provider_background_ready() {
            self.catch_up_loaded_provider_tenant_async(runtime.clone(), &tenant_id, true, true)
                .await?;
        }
        // Running-job recovery belongs to startup/unloaded-tenant activation.
        // Once a tenant is already loaded, the live scheduler owns claim
        // state and provider wake paths must not requeue in-flight jobs.
        runtime
            .read_storage
            .execute(move |store| store.recover_running_jobs(now))
            .await?;
        self.tenants
            .write()
            .expect("tenant registry lock should not be poisoned")
            .insert(tenant_id, runtime);
        Ok(true)
    }

    fn loaded_runtime_for_scheduler(&self, tenant_id: &TenantId) -> Option<Arc<TenantRuntime>> {
        self.tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .get(tenant_id)
            .cloned()
    }
}

async fn next_loaded_tenant_scheduled_work_at(
    runtime: Arc<crate::tenant::TenantRuntime>,
    tenant_id: TenantId,
) -> Result<Option<Timestamp>> {
    read_loaded_tenant_store(runtime, tenant_id, move |store| {
        store.next_scheduled_work_at()
    })
    .await
}
