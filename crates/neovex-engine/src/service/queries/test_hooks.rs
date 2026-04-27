#[cfg(test)]
use std::future::Future;
#[cfg(test)]
use std::sync::Arc;

#[cfg(any(test, feature = "test-hooks"))]
use neovex_core::Result;
#[cfg(any(test, feature = "test-hooks"))]
use neovex_core::TenantId;
#[cfg(test)]
use neovex_core::{
    ResourcePathBinding, SequenceNumber, TableName, TriggerDeliveryCursor, TriggerInvocationRecord,
};

#[cfg(test)]
use crate::TriggerRegistration;
use crate::service::Service;

impl Service {
    #[cfg(any(test, feature = "test-hooks"))]
    fn with_runtime_for_testing<T>(
        &self,
        tenant_id: &TenantId,
        map: impl FnOnce(&crate::tenant::TenantRuntime) -> T,
    ) -> Result<T> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(map(&runtime))
    }

    #[cfg(test)]
    pub(crate) fn document_cache_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::DocumentCacheStats> {
        self.with_runtime_for_testing(tenant_id, |runtime| runtime.document_cache_stats())
    }

    #[cfg(test)]
    pub(crate) fn mutation_journal_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::MutationJournalStats> {
        self.with_runtime_for_testing(tenant_id, |runtime| runtime.mutation_journal_stats())
    }

    #[cfg(test)]
    pub(crate) fn mutation_admission_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::MutationAdmissionStats> {
        self.with_runtime_for_testing(tenant_id, |runtime| runtime.mutation_admission_stats())
    }

    #[cfg(test)]
    pub(crate) fn subscription_delivery_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::SubscriptionDeliveryStats> {
        self.with_runtime_for_testing(tenant_id, |runtime| runtime.subscription_delivery_stats())
    }

    #[cfg(test)]
    pub(crate) fn pending_trigger_candidate_count_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<usize> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.pending_trigger_candidate_count_for_testing()
        })
    }

    #[cfg(test)]
    pub(crate) fn drain_trigger_candidates_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Vec<crate::triggers::dispatch::TriggerCommitCandidate>> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.drain_trigger_candidates_for_testing()
        })
    }

    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn query_planning_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::QueryPlanningStats> {
        self.with_runtime_for_testing(tenant_id, |runtime| runtime.query_planning_stats())
    }

    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn materialized_read_surface_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::MaterializedReadSurfaceStats> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.materialized_read_surface_stats()
        })
    }

    #[cfg(test)]
    pub(crate) fn materialized_table_publication_stats_for_testing(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
    ) -> Result<Option<crate::tenant::MaterializedTablePublicationStats>> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.materialized_table_publication_stats(table)
        })
    }

    #[cfg(test)]
    pub(crate) fn materialized_serving_snapshot_for_testing(
        &self,
        tenant_id: &TenantId,
        required_sequence: SequenceNumber,
    ) -> Result<Option<crate::tenant::ServingSnapshot>> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.materialized_serving_snapshot_for_testing(required_sequence)
        })
    }

    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn serving_snapshot_manager_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::ServingSnapshotManagerStats> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.serving_snapshot_manager_stats()
        })
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_materialized_serving_snapshot_for_testing<Fut>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        required_sequence: SequenceNumber,
        cancel_wait: Fut,
    ) -> Result<crate::tenant::ServingSnapshot>
    where
        Fut: Future<Output = ()> + Send,
    {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        runtime
            .wait_for_materialized_serving_snapshot_cancellable(required_sequence, cancel_wait)
            .await
    }

    #[cfg(test)]
    pub(crate) fn set_subscription_delivery_queue_capacity_for_testing(
        &self,
        tenant_id: &TenantId,
        capacity: usize,
    ) -> Result<()> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.set_subscription_delivery_queue_capacity_for_testing(capacity);
        })?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_mutation_journal_queue_capacity_for_testing(
        &self,
        tenant_id: &TenantId,
        capacity: usize,
    ) -> Result<()> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.set_mutation_journal_queue_capacity_for_testing(capacity);
        })?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_mutation_admission_codel_for_testing(
        &self,
        tenant_id: &TenantId,
        target: std::time::Duration,
        interval: std::time::Duration,
    ) -> Result<()> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.set_mutation_admission_codel_for_testing(target, interval);
        })?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_materialized_read_surface_limits_for_testing(
        &self,
        tenant_id: &TenantId,
        table_capacity: usize,
        byte_capacity: usize,
    ) -> Result<()> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.set_materialized_read_surface_limits_for_testing(table_capacity, byte_capacity);
        })?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_materialized_read_surface_version_capacity_for_testing(
        &self,
        tenant_id: &TenantId,
        version_capacity: usize,
    ) -> Result<()> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.set_materialized_read_surface_version_capacity_for_testing(version_capacity);
        })?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn subscription_delivery_pause_handle_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::SubscriptionDeliveryPauseHandle> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.subscription_delivery_pause_handle_for_testing()
        })
    }

    #[cfg(test)]
    pub(crate) fn trigger_candidate_pause_handle_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::TriggerCandidatePauseHandle> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.trigger_candidate_pause_handle_for_testing()
        })
    }

    #[cfg(test)]
    pub(crate) fn mutation_journal_pause_handle_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::MutationJournalPauseHandle> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.mutation_journal_pause_handle_for_testing()
        })
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn subscription_bootstrap_pause_handle_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::MutationJournalPauseHandle> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.subscription_bootstrap_pause_handle_for_testing()
        })
    }

    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn arm_subscription_bootstrap_pause_for_testing(&self, tenant_id: &TenantId) -> Result<()> {
        let pause = self.subscription_bootstrap_pause_handle_for_testing(tenant_id)?;
        pause.arm();
        Ok(())
    }

    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn wait_for_subscription_bootstrap_pause_for_testing(
        &self,
        tenant_id: &TenantId,
        timeout: std::time::Duration,
    ) -> Result<bool> {
        let pause = self.subscription_bootstrap_pause_handle_for_testing(tenant_id)?;
        Ok(pause.wait_until_entered(timeout))
    }

    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn release_subscription_bootstrap_pause_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<()> {
        let pause = self.subscription_bootstrap_pause_handle_for_testing(tenant_id)?;
        pause.release();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn materialized_read_publish_pause_handle_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::MaterializedReadPublishPauseHandle> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.materialized_read_publish_pause_handle_for_testing()
        })
    }

    #[cfg(test)]
    pub(crate) fn upsert_resource_path_binding_for_testing(
        &self,
        tenant_id: &TenantId,
        binding: ResourcePathBinding,
    ) -> Result<()> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.store.upsert_resource_path_binding(&binding)
        })??;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_trigger_delivery_cursor_for_testing(
        &self,
        tenant_id: &TenantId,
        cursor: TriggerDeliveryCursor,
    ) -> Result<()> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.store.set_trigger_delivery_cursor(cursor)
        })??;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn trigger_delivery_cursor_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<TriggerDeliveryCursor> {
        self.with_runtime_for_testing(tenant_id, |runtime| runtime.store.trigger_delivery_cursor())?
    }

    #[cfg(test)]
    pub(crate) fn replace_trigger_registrations_for_testing(
        &self,
        tenant_id: &TenantId,
        registrations: Vec<TriggerRegistration>,
    ) -> Result<()> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.replace_trigger_registrations(registrations)
        })??;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn list_trigger_invocations_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Vec<TriggerInvocationRecord>> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.store.list_trigger_invocations()
        })?
    }
}
