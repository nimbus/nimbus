use std::path::PathBuf;
use std::sync::Arc;

use neovex_storage::StorageEngine;

use neovex_core::{Error, Result, TenantId};

use crate::tenant::TenantRuntime;

use super::Service;

impl Service {
    /// Creates a tenant explicitly.
    pub fn create_tenant(&self, tenant_id: TenantId) -> Result<()> {
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

        let runtime = self.build_loaded_tenant_runtime(self.open_tenant_store(&path)?)?;
        tenants.insert(tenant_id, runtime);
        Ok(())
    }

    /// Creates a tenant explicitly asynchronously.
    pub async fn create_tenant_async(self: &Arc<Self>, tenant_id: TenantId) -> Result<()> {
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

        let opened = self.storage_engine.create_tenant(&tenant_id).await?;
        let runtime = Arc::new(TenantRuntime::from_parts(
            opened.store,
            opened.read_storage,
        )?);
        self.tenants
            .write()
            .expect("tenant registry lock should not be poisoned")
            .insert(tenant_id, runtime);
        Ok(())
    }

    /// Lists all tenant ids on disk.
    pub fn list_tenants(&self) -> Result<Vec<TenantId>> {
        let mut tenants = Vec::new();
        let entries = std::fs::read_dir(&self.data_dir)
            .map_err(|error| Error::Internal(error.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|error| Error::Internal(error.to_string()))?;
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "redb")
                && let Some(stem) = path.file_stem()
            {
                tenants.push(TenantId::new(stem.to_string_lossy().to_string())?);
            }
        }
        tenants.sort();
        Ok(tenants)
    }

    /// Lists all tenant ids on disk asynchronously.
    pub async fn list_tenants_async(self: &Arc<Self>) -> Result<Vec<TenantId>> {
        self.storage_engine.list_tenants().await
    }

    /// Deletes a tenant database and evicts it from memory.
    pub fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
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
        let _tenant_load_guard = self.tenant_load_gate.lock().await;
        let runtime = {
            self.tenants
                .write()
                .expect("tenant registry lock should not be poisoned")
                .remove(&tenant_id)
        };
        if runtime.is_none() && !self.storage_engine.tenant_exists(&tenant_id).await? {
            return Err(Error::TenantNotFound(tenant_id));
        }
        if let Some(runtime) = runtime {
            let _deletion = runtime.begin_delete_async().await;
            runtime.shutdown_subscription_delivery();
            runtime
                .subscriptions
                .shutdown_all(format!("tenant deleted: {tenant_id}"));
        }
        self.storage_engine.delete_tenant(&tenant_id).await?;
        Ok(())
    }

    /// Verifies that a tenant exists.
    pub fn ensure_tenant_exists(&self, tenant_id: &TenantId) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(())
    }

    /// Verifies that a tenant exists asynchronously.
    pub async fn ensure_tenant_exists_async(self: &Arc<Self>, tenant_id: TenantId) -> Result<()> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        Ok(())
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

        let runtime = self.build_loaded_tenant_runtime(self.open_tenant_store(&path)?)?;
        tenants.insert(tenant_id.clone(), runtime.clone());
        Ok(runtime)
    }

    pub(super) async fn get_existing_tenant_async(
        self: &Arc<Self>,
        tenant_id: &TenantId,
    ) -> Result<Arc<TenantRuntime>> {
        if let Some(runtime) = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .get(tenant_id)
            .cloned()
        {
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
            return Ok(runtime);
        }

        let Some(opened) = self.storage_engine.open_existing_tenant(tenant_id).await? else {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        };
        let runtime = Arc::new(TenantRuntime::from_parts(
            opened.store.clone(),
            opened.read_storage,
        )?);
        let progress = opened.store.recover_durable_journal()?;
        runtime.sync_mutation_journal_progress(progress);
        self.tenants
            .write()
            .expect("tenant registry lock should not be poisoned")
            .insert(tenant_id.clone(), runtime.clone());
        Ok(runtime)
    }

    pub(super) fn tenant_path(&self, tenant_id: &TenantId) -> PathBuf {
        self.data_dir.join(format!("{}.redb", tenant_id.as_str()))
    }
}
