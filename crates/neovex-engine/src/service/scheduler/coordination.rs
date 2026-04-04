use std::sync::Arc;

use neovex_core::{Error, Result, TenantId, Timestamp};

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
        let entries = std::fs::read_dir(&self.data_dir)
            .map_err(|error| Error::Internal(error.to_string()))?;
        let now = self.now();

        for entry in entries {
            let entry = entry.map_err(|error| Error::Internal(error.to_string()))?;
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "redb") {
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
