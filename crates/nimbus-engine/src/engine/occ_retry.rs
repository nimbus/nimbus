use std::future::Future;
use std::sync::Arc;

use nimbus_core::{Result, SequenceNumber, TenantId};

use super::Engine;

impl Engine {
    /// Waits until a conflicting commit is applied before a caller re-prepares
    /// a mutation against a new snapshot.
    pub async fn wait_for_applied_sequence_cancellable<Fut>(
        self: &Arc<Self>,
        tenant_id: &TenantId,
        sequence: SequenceNumber,
        cancel_wait: Fut,
    ) -> Result<()>
    where
        Fut: Future<Output = ()>,
    {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime
            .wait_for_applied_sequence_cancellable(sequence, cancel_wait)
            .await
    }

    /// Blocking counterpart for caller layers whose runtime invocation path is
    /// synchronous (currently Cloud Functions HTTP and trigger execution).
    pub fn wait_for_applied_sequence_blocking(
        &self,
        tenant_id: &TenantId,
        sequence: SequenceNumber,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.wait_for_applied_sequence_blocking(sequence)
    }

    pub fn record_mutation_conflict_retry(&self, tenant_id: &TenantId) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime
            .commit_phase_metrics()
            .record_mutation_conflict_retry();
        Ok(())
    }

    pub fn record_mutation_conflict_exhausted(&self, tenant_id: &TenantId) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime
            .commit_phase_metrics()
            .record_mutation_conflict_exhausted();
        Ok(())
    }
}
