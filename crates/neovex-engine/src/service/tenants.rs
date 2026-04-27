use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use neovex_core::{Error, Result, TenantId};

use crate::tenant::TenantRuntime;

use super::Service;

pub(in crate::service) fn with_tenant_runtime_operation<T, F>(
    runtime: Arc<TenantRuntime>,
    tenant_id: &TenantId,
    task: F,
) -> Result<T>
where
    F: FnOnce(Arc<TenantRuntime>) -> Result<T>,
{
    let _operation = runtime.enter_operation(tenant_id)?;
    task(runtime)
}

impl Service {
    /// Creates a tenant explicitly.
    pub fn create_tenant(&self, tenant_id: TenantId) -> Result<()> {
        self.require_embedded_provider_kind()?;
        let _tenant_load_guard = self.lock_tenant_load_gate_blocking();
        let path = self.tenant_path(&tenant_id);
        let mut tenants = self
            .tenants
            .write()
            .expect("tenant registry lock should not be poisoned");
        if tenants.contains_key(&tenant_id) || path.exists() {
            return Err(Error::AlreadyExists(format!(
                "tenant already exists: {tenant_id}"
            )));
        }

        let runtime =
            self.build_loaded_tenant_runtime(&tenant_id, self.open_tenant_store(&path)?)?;
        tenants.insert(tenant_id, runtime);
        Ok(())
    }

    /// Creates a tenant explicitly asynchronously.
    pub async fn create_tenant_async(self: &Arc<Self>, tenant_id: TenantId) -> Result<()> {
        self.ensure_provider_background_tasks_started();
        let _tenant_load_guard = self.tenant_load_gate.lock().await;
        if self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .contains_key(&tenant_id)
        {
            return Err(Error::AlreadyExists(format!(
                "tenant already exists: {tenant_id}"
            )));
        }

        let opened = self.persistence_provider.create_tenant(&tenant_id).await?;
        let runtime = Arc::new(
            TenantRuntime::from_parts_async(tenant_id.clone(), opened.persistence, opened.executor)
                .await?,
        );
        if !self.provider_background_ready() {
            self.catch_up_loaded_provider_tenant_async(
                runtime.clone(),
                &tenant_id,
                true,
                true,
                false,
            )
            .await?;
        }
        self.bootstrap_trigger_candidate_feed(runtime.clone())?;
        self.bootstrap_trigger_execution(runtime.clone())?;
        self.tenants
            .write()
            .expect("tenant registry lock should not be poisoned")
            .insert(tenant_id, runtime);
        Ok(())
    }

    /// Lists all tenant ids on disk.
    pub fn list_tenants(&self) -> Result<Vec<TenantId>> {
        let embedded_provider_kind = self.require_embedded_provider_kind()?;
        let mut tenants = Vec::new();
        let entries = std::fs::read_dir(&self.data_dir)
            .map_err(|error| Error::Internal(error.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|error| Error::Internal(error.to_string()))?;
            let path = entry.path();
            if path.extension().is_some_and(|extension| {
                extension == embedded_provider_kind.tenant_file_extension()
            }) && let Some(stem) = path.file_stem()
            {
                tenants.push(TenantId::new(stem.to_string_lossy().to_string())?);
            }
        }
        tenants.sort();
        Ok(tenants)
    }

    /// Lists all tenant ids on disk asynchronously.
    pub async fn list_tenants_async(self: &Arc<Self>) -> Result<Vec<TenantId>> {
        self.ensure_provider_background_tasks_started();
        self.persistence_provider.list_tenants().await
    }

    /// Deletes a tenant database and evicts it from memory.
    pub fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        let _embedded_provider_kind = self.require_embedded_provider_kind()?;
        let _tenant_load_guard = self.lock_tenant_load_gate_blocking();
        let path = self.tenant_path(tenant_id);
        if !path.exists() {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        }
        let runtime = {
            self.tenants
                .write()
                .expect("tenant registry lock should not be poisoned")
                .remove(tenant_id)
        };
        if let Some(runtime) = runtime {
            let _deletion = runtime.begin_delete();
            runtime.shutdown_trigger_candidates();
            runtime.shutdown_trigger_execution();
            runtime.shutdown_subscription_delivery();
            runtime
                .subscriptions
                .shutdown_all(format!("tenant deleted: {tenant_id}"));
        }
        std::fs::remove_file(path).map_err(|error| Error::Internal(error.to_string()))?;
        Ok(())
    }

    /// Deletes a tenant database and evicts it from memory asynchronously.
    pub async fn delete_tenant_async(self: &Arc<Self>, tenant_id: TenantId) -> Result<()> {
        self.ensure_provider_background_tasks_started();
        let _tenant_load_guard = self.tenant_load_gate.lock().await;
        let runtime = {
            self.tenants
                .write()
                .expect("tenant registry lock should not be poisoned")
                .remove(&tenant_id)
        };
        if runtime.is_none() && !self.persistence_provider.tenant_exists(&tenant_id).await? {
            return Err(Error::TenantNotFound(tenant_id));
        }
        if let Some(runtime) = runtime {
            let _deletion = runtime.begin_delete_async().await;
            runtime.shutdown_trigger_candidates();
            runtime.shutdown_trigger_execution();
            runtime.shutdown_subscription_delivery();
            runtime
                .subscriptions
                .shutdown_all(format!("tenant deleted: {tenant_id}"));
        }
        self.persistence_provider.delete_tenant(&tenant_id).await?;
        Ok(())
    }

    /// Verifies that a tenant exists.
    pub fn ensure_tenant_exists(&self, tenant_id: &TenantId) -> Result<()> {
        with_tenant_runtime_operation(self.get_existing_tenant(tenant_id)?, tenant_id, |_| Ok(()))
    }

    /// Verifies that a tenant exists asynchronously.
    pub async fn ensure_tenant_exists_async(self: &Arc<Self>, tenant_id: TenantId) -> Result<()> {
        with_tenant_runtime_operation(
            self.get_existing_tenant_async(&tenant_id).await?,
            &tenant_id,
            |_| Ok(()),
        )
    }

    pub(super) fn get_existing_tenant(&self, tenant_id: &TenantId) -> Result<Arc<TenantRuntime>> {
        if let Some(runtime) = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .get(tenant_id)
            .cloned()
        {
            return Ok(runtime);
        }

        let _tenant_load_guard = self.lock_tenant_load_gate_blocking();
        let mut tenants = self
            .tenants
            .write()
            .expect("tenant registry lock should not be poisoned");
        if let Some(runtime) = tenants.get(tenant_id).cloned() {
            return Ok(runtime);
        }

        let path = self.tenant_path(tenant_id);
        if !path.exists() {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        }

        let runtime =
            self.build_loaded_tenant_runtime(tenant_id, self.open_tenant_store(&path)?)?;
        tenants.insert(tenant_id.clone(), runtime.clone());
        Ok(runtime)
    }

    pub(super) async fn get_existing_tenant_async(
        self: &Arc<Self>,
        tenant_id: &TenantId,
    ) -> Result<Arc<TenantRuntime>> {
        self.ensure_provider_background_tasks_started();
        let total_started = Instant::now();
        if let Some(runtime) = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .get(tenant_id)
            .cloned()
        {
            maybe_emit_tenant_load_profile(TenantLoadProfileSample {
                tenant_id,
                cache_hit: true,
                open_existing: Duration::ZERO,
                runtime_init: Duration::ZERO,
                runtime_schema_load: Duration::ZERO,
                runtime_journal_progress: Duration::ZERO,
                runtime_profile_total: Duration::ZERO,
                recover_durable: Duration::ZERO,
                catch_up: Duration::ZERO,
                total: total_started.elapsed(),
            });
            return Ok(runtime);
        }

        let _tenant_load_guard = self.tenant_load_gate.lock().await;
        if let Some(runtime) = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .get(tenant_id)
            .cloned()
        {
            maybe_emit_tenant_load_profile(TenantLoadProfileSample {
                tenant_id,
                cache_hit: true,
                open_existing: Duration::ZERO,
                runtime_init: Duration::ZERO,
                runtime_schema_load: Duration::ZERO,
                runtime_journal_progress: Duration::ZERO,
                runtime_profile_total: Duration::ZERO,
                recover_durable: Duration::ZERO,
                catch_up: Duration::ZERO,
                total: total_started.elapsed(),
            });
            return Ok(runtime);
        }

        let open_started = Instant::now();
        let Some(opened) = self
            .persistence_provider
            .open_existing_tenant(tenant_id)
            .await?
        else {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        };
        let open_elapsed = open_started.elapsed();
        let opened_executor = opened.executor.clone();
        let runtime_init_started = Instant::now();
        let (initial_state, runtime_profile) =
            TenantRuntime::load_initial_state_async(&opened.persistence, &opened_executor).await?;
        let runtime = Arc::new(TenantRuntime::from_loaded_state(
            tenant_id.clone(),
            opened.persistence.clone(),
            opened_executor,
            initial_state,
        ));
        let runtime_init_elapsed = runtime_init_started.elapsed();
        let recover_started = Instant::now();
        let progress = if runtime.applied_head().0 < runtime.durable_head().0 {
            opened
                .persistence
                .recover_durable_journal_async(&opened.executor)
                .await?
        } else {
            neovex_storage::JournalProgress {
                durable_head: runtime.durable_head(),
                applied_head: runtime.applied_head(),
            }
        };
        let recover_elapsed = recover_started.elapsed();
        runtime.sync_mutation_journal_progress(progress);
        let catch_up_started = Instant::now();
        if !self.provider_background_ready() {
            self.catch_up_loaded_provider_tenant_async(
                runtime.clone(),
                tenant_id,
                true,
                true,
                false,
            )
            .await?;
        }
        self.bootstrap_trigger_candidate_feed(runtime.clone())?;
        self.bootstrap_trigger_execution(runtime.clone())?;
        let catch_up_elapsed = catch_up_started.elapsed();
        self.tenants
            .write()
            .expect("tenant registry lock should not be poisoned")
            .insert(tenant_id.clone(), runtime.clone());
        maybe_emit_tenant_load_profile(TenantLoadProfileSample {
            tenant_id,
            cache_hit: false,
            open_existing: open_elapsed,
            runtime_init: runtime_init_elapsed,
            runtime_schema_load: runtime_profile.schema_load,
            runtime_journal_progress: runtime_profile.journal_progress,
            runtime_profile_total: runtime_profile.total,
            recover_durable: recover_elapsed,
            catch_up: catch_up_elapsed,
            total: total_started.elapsed(),
        });
        Ok(runtime)
    }

    pub(super) fn tenant_path(&self, tenant_id: &TenantId) -> PathBuf {
        let embedded_provider_kind = self
            .require_embedded_provider_kind()
            .expect("tenant path should only be used for embedded providers");
        self.data_dir.join(format!(
            "{}.{}",
            tenant_id.as_str(),
            embedded_provider_kind.tenant_file_extension()
        ))
    }
}

struct TenantLoadProfileSample<'a> {
    tenant_id: &'a TenantId,
    cache_hit: bool,
    open_existing: Duration,
    runtime_init: Duration,
    runtime_schema_load: Duration,
    runtime_journal_progress: Duration,
    runtime_profile_total: Duration,
    recover_durable: Duration,
    catch_up: Duration,
    total: Duration,
}

fn maybe_emit_tenant_load_profile(sample: TenantLoadProfileSample<'_>) {
    if std::env::var_os("NEOVEX_TENANT_LOAD_PROFILE").is_none() {
        return;
    }
    if std::env::var_os("NEOVEX_PROFILE_ONLY_COLD_SAMPLES").is_some() && sample.cache_hit {
        return;
    }

    eprintln!(
        "tenant-load-profile tenant={} cache_hit={} open_existing={:?} runtime_init={:?} runtime_schema_load={:?} runtime_journal_progress={:?} runtime_profile_total={:?} recover_durable={:?} catch_up={:?} total={:?}",
        sample.tenant_id,
        sample.cache_hit,
        sample.open_existing,
        sample.runtime_init,
        sample.runtime_schema_load,
        sample.runtime_journal_progress,
        sample.runtime_profile_total,
        sample.recover_durable,
        sample.catch_up,
        sample.total,
    );
}
