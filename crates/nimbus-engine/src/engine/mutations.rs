mod authorization;
pub(in crate::engine) mod caps;
mod commit_processing;
#[cfg(test)]
mod crash_recovery;
mod direct;
pub(crate) mod durable_outcome;
mod inline_reprepare;
mod journal;
pub(in crate::engine) mod phase_metrics;
pub(crate) mod prepared;
mod publisher;
mod shadow_conflicts;
mod window_prepare;
pub(crate) mod write_log;

use std::future::Future;
use std::sync::Arc;

use nimbus_core::{Error, Result, TenantId};

use crate::tenant::{MutationIsolateAdmissionPermit, TenantOperationGuard};

pub(crate) use authorization::enforce_mutation_authorization;
pub(in crate::engine) use commit_processing::document_bearing_commit_identity;
pub use direct::{AsyncMutationContext, MutationActor};
pub(crate) use publisher::{
    begin_definitive_fence_eviction, begin_durable_recovery_eviction,
    finish_durable_recovery_eviction_locked, run_ordered_publisher,
};

use super::Engine;

/// Engine-admitted seat held only while a tenant mutation executes guest code.
/// Dropping the guard releases both the isolate count and the tenant operation.
pub struct MutationIsolatePermit {
    _admission: MutationIsolateAdmissionPermit,
    _operation: TenantOperationGuard,
}

impl Engine {
    /// Waits for this tenant's mutation-isolate ceiling before runtime dispatch.
    ///
    /// Waiting is bounded by the caller's existing request cancellation/timeout
    /// future. A saturated admission wait queue sheds with typed `Overloaded`;
    /// ordinary over-ceiling callers wait instead of failing immediately.
    pub async fn acquire_mutation_isolate_permit_cancellable<Fut>(
        self: &Arc<Self>,
        tenant_id: &TenantId,
        cancel_wait: Fut,
    ) -> Result<MutationIsolatePermit>
    where
        Fut: Future<Output = ()>,
    {
        let runtime = self.get_existing_tenant_async(tenant_id).await?;
        let operation = runtime.enter_operation(tenant_id)?;
        tokio::pin!(cancel_wait);
        tokio::select! {
            permit = runtime.acquire_mutation_isolate_permit() => {
                permit.map(|permit| MutationIsolatePermit {
                    _admission: permit,
                    _operation: operation,
                })
            }
            _ = &mut cancel_wait => Err(Error::Cancelled),
        }
    }
}
