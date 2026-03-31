use std::path::PathBuf;
use std::sync::Arc;

use neovex_core::{Error, Result, TenantId};
use neovex_storage::TenantStore;

use crate::tenant::TenantRuntime;

use super::Service;

impl Service {
    /// Creates a tenant explicitly.
    pub fn create_tenant(&self, tenant_id: TenantId) -> Result<()> {
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

        let runtime = Arc::new(TenantRuntime::new(TenantStore::open(&path)?)?);
        tenants.insert(tenant_id, runtime);
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

    /// Deletes a tenant database and evicts it from memory.
    pub fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
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
            runtime
                .subscriptions
                .shutdown_all(format!("tenant deleted: {tenant_id}"));
        }
        std::fs::remove_file(path).map_err(|error| Error::Internal(error.to_string()))?;
        Ok(())
    }

    /// Verifies that a tenant exists.
    pub fn ensure_tenant_exists(&self, tenant_id: &TenantId) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
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

        let runtime = Arc::new(TenantRuntime::new(TenantStore::open(&path)?)?);
        tenants.insert(tenant_id.clone(), runtime.clone());
        Ok(runtime)
    }

    pub(super) fn tenant_path(&self, tenant_id: &TenantId) -> PathBuf {
        self.data_dir.join(format!("{}.redb", tenant_id.as_str()))
    }
}
