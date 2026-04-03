use std::sync::Arc;

use neovex_core::{Result, TenantId};

use crate::TenantEngineDiagnosticsSnapshot;

use super::Service;

impl Service {
    /// Returns a per-tenant snapshot of engine durability, worker, and serving health.
    pub fn tenant_engine_diagnostics(
        &self,
        tenant_id: &TenantId,
    ) -> Result<TenantEngineDiagnosticsSnapshot> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.engine_diagnostics_snapshot())
    }

    /// Returns a per-tenant snapshot of engine durability, worker, and serving health.
    pub async fn tenant_engine_diagnostics_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<TenantEngineDiagnosticsSnapshot> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        Ok(runtime.engine_diagnostics_snapshot())
    }
}
