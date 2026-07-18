#[cfg(test)]
use std::future::Future;
#[cfg(test)]
use std::sync::Arc;

#[cfg(any(test, feature = "test-hooks"))]
use nimbus_core::Result;
#[cfg(any(test, feature = "test-hooks"))]
use nimbus_core::TenantId;
#[cfg(test)]
use nimbus_core::{
    DocumentId, ResourcePathBinding, SequenceNumber, TableName, TenantEventRecord,
    TriggerDeliveryCursor, TriggerInvocationRecord, WriteOp, WriteOpType,
};

#[cfg(test)]
use crate::TriggerRegistration;
use crate::engine::Engine;
#[cfg(test)]
use crate::engine::mutations::document_bearing_commit_identity;

impl Engine {
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
    pub(crate) fn document_cache_invalidation_pause_handle_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::DocumentCacheInvalidationPauseHandle> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.document_cache_invalidation_pause_handle_for_testing()
        })
    }

    #[cfg(test)]
    pub(crate) fn mutation_journal_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::MutationJournalStats> {
        let mut stats =
            self.with_runtime_for_testing(tenant_id, |runtime| runtime.mutation_journal_stats())?;
        self.apply_committed_mutation_observer_work_stats(tenant_id, &mut stats);
        Ok(stats)
    }

    #[cfg(test)]
    pub(crate) fn tenant_runtime_identity_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<usize> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        Ok(Arc::as_ptr(&runtime) as usize)
    }

    #[cfg(test)]
    pub(crate) fn tenant_operation_guard_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::TenantOperationGuard> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        runtime.enter_operation(tenant_id)
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub fn park_applied_sequence_waiters_for_testing(
        &self,
        tenant_id: &TenantId,
        required_sequence: nimbus_core::SequenceNumber,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        runtime.mark_durable_head(required_sequence);
        Ok(())
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub fn fail_applied_sequence_waiters_for_testing(&self, tenant_id: &TenantId) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        runtime.fail_applied_waiters_for_testing(nimbus_core::Error::storage(
            nimbus_core::StorageErrorKind::Unavailable,
            format!("tenant {tenant_id} runtime evicted during test"),
        ));
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn write_log_assignment_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<(SequenceNumber, Vec<SequenceNumber>)> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            (
                runtime.assigned_head(),
                runtime.write_log.pending_sequences_for_testing(),
            )
        })
    }

    #[cfg(test)]
    pub(crate) async fn enqueue_publisher_response_fence_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<crate::tenant::QueuedMutationResult>>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let operation = runtime.enter_operation(tenant_id)?;
        let (response, completion) = tokio::sync::oneshot::channel();
        runtime
            .send_publisher_response_fence(vec![crate::tenant::DeferredPublisherResponse {
                _operation: operation,
                response: crate::tenant::MutationResponseSender::new(response),
                result: Ok(crate::tenant::QueuedMutationResult::Scheduled(false)),
            }])
            .await
            .map_err(|error| error.1)?;
        Ok(completion)
    }

    #[cfg(test)]
    pub(crate) async fn enqueue_publisher_conflict_response_fence_for_testing(
        &self,
        tenant_id: &TenantId,
        conflicting_sequence: SequenceNumber,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<crate::tenant::QueuedMutationResult>>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let operation = runtime.enter_operation(tenant_id)?;
        let (response, completion) = tokio::sync::oneshot::channel();
        runtime
            .send_publisher_response_fence(vec![crate::tenant::DeferredPublisherResponse {
                _operation: operation,
                response: crate::tenant::MutationResponseSender::new(response),
                result: Err(nimbus_core::Error::retryable_conflict(
                    "assigned conflict target is not yet applied",
                    Some(conflicting_sequence),
                )),
            }])
            .await
            .map_err(|error| error.1)?;
        Ok(completion)
    }

    #[cfg(test)]
    pub(crate) fn set_committer_pipeline_requested_for_testing(
        &self,
        tenant_id: &TenantId,
        enabled: bool,
    ) -> Result<()> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.set_committer_pipeline_requested_for_testing(enabled);
        })
    }

    #[cfg(test)]
    pub(crate) fn set_prepared_table_id_for_testing(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        table_id: nimbus_core::TableId,
    ) -> Result<()> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.prepared_table_id(table, Some(table_id));
        })
    }

    #[cfg(test)]
    pub(crate) fn mutation_admission_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::MutationAdmissionStats> {
        self.with_runtime_for_testing(tenant_id, |runtime| runtime.mutation_admission_stats())
    }

    #[cfg(test)]
    pub(crate) fn force_write_log_storage_fallback_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<()> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.write_log.mark_coverage_unknown();
        })
    }

    #[cfg(test)]
    pub(crate) fn stage_assigned_pending_update_for_testing(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        document_id: &DocumentId,
        field: &str,
        value: serde_json::Value,
    ) -> Result<TenantEventRecord> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let runtime_for_commit = runtime.clone();
        let table = table.clone();
        let document_id = document_id.clone();
        let field = field.to_string();
        runtime.submit_internal_committer(move || {
            let runtime = runtime_for_commit;
            let previous = runtime
                .store
                .get(&table, &document_id)?
                .ok_or_else(|| nimbus_core::Error::DocumentNotFound(document_id.clone()))?;
            let table_id = runtime.store.table_id(&table)?.ok_or_else(|| {
                nimbus_core::Error::Internal(
                    "assigned-pending test fixture requires an existing table identity".to_string(),
                )
            })?;
            let sequence = crate::tenant::assign_and_validate(runtime.durable_head(), 1)?[0];
            let timestamp = runtime.assign_commit_timestamp();
            let mut current = previous.clone();
            current.fields.insert(field, value);
            current.creation_time = previous.creation_time;
            current.update_time = timestamp;
            let record = TenantEventRecord::new(
                sequence,
                timestamp,
                vec![WriteOp {
                    table,
                    table_id,
                    op_type: WriteOpType::Update,
                    doc_id: document_id,
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: Some(previous),
                    current: Some(current),
                }],
                None,
            )?;
            runtime
                .stage_pending_write_log_commits([record.as_commit_entry()], runtime.store.now());
            runtime
                .store
                .append_durable_records_batch(std::slice::from_ref(&record))?;
            runtime.mark_durable_head(sequence);
            Ok(record)
        })
    }

    #[cfg(test)]
    pub(crate) fn apply_assigned_pending_record_for_testing(
        &self,
        tenant_id: &TenantId,
        record: &TenantEventRecord,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let runtime_for_commit = runtime.clone();
        let record = record.clone();
        runtime.submit_internal_committer(move || {
            let runtime = runtime_for_commit;
            runtime
                .store
                .apply_durable_records_batch(std::slice::from_ref(&record))?;
            let commit = record.as_commit_entry();
            let published_frontier = runtime.publish_write_log_through(commit.sequence);
            runtime.invalidate_document_cache_for_commit(&commit);
            runtime.mark_applied_head(published_frontier);
            Ok(())
        })
    }

    #[cfg(test)]
    pub(crate) fn apply_assigned_pending_record_without_publish_for_testing(
        &self,
        tenant_id: &TenantId,
        record: &TenantEventRecord,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let runtime_for_commit = runtime.clone();
        let record = record.clone();
        runtime.submit_internal_committer(move || {
            runtime_for_commit
                .store
                .apply_durable_records_batch(std::slice::from_ref(&record))
        })
    }

    #[cfg(test)]
    pub(crate) fn publish_assigned_pending_record_for_testing(
        &self,
        tenant_id: &TenantId,
        record: &TenantEventRecord,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let runtime_for_commit = runtime.clone();
        let commit = record.as_commit_entry();
        runtime.submit_internal_committer(move || {
            let published_frontier = runtime_for_commit.publish_write_log_through(commit.sequence);
            runtime_for_commit.invalidate_document_cache_for_commit(&commit);
            runtime_for_commit.mark_applied_head(published_frontier);
            Ok(())
        })
    }

    #[cfg(test)]
    pub(crate) fn sync_mutation_journal_progress_for_testing(
        &self,
        tenant_id: &TenantId,
        progress: nimbus_storage::JournalProgress,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let runtime_for_commit = runtime.clone();
        runtime.submit_internal_committer(move || {
            runtime_for_commit.sync_mutation_journal_progress_in_actor(progress);
            Ok(())
        })
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
    pub(crate) fn subscription_delivery_publish_pause_handle_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::SubscriptionDeliveryPublishPauseHandle> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.subscription_delivery_publish_pause_handle_for_testing()
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
    pub(crate) fn shutdown_trigger_candidates_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<()> {
        self.with_runtime_for_testing(tenant_id, |runtime| runtime.shutdown_trigger_candidates())
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub fn mutation_journal_pause_handle_for_testing(
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
        let runtime = self.get_existing_tenant(tenant_id)?;
        let runtime_for_commit = runtime.clone();
        runtime.submit_internal_committer(move || {
            runtime_for_commit
                .store
                .set_trigger_delivery_cursor(cursor)?;
            let progress = runtime_for_commit.store.journal_progress()?;
            runtime_for_commit.advance_write_log_zero_write_coverage(progress.durable_head);
            runtime_for_commit.sync_mutation_journal_progress_in_actor(progress);
            Ok(())
        })?;
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

    #[cfg(test)]
    pub(crate) fn save_trigger_invocation_for_testing(
        &self,
        tenant_id: &TenantId,
        record: &TriggerInvocationRecord,
    ) -> Result<()> {
        self.with_runtime_for_testing(tenant_id, |runtime| {
            runtime.store.save_trigger_invocation(record)
        })?
    }

    /// Drives `process_applied_commit_batch` directly with a caller-supplied
    /// `records` slice, bypassing the mutation queue and provider catch-up
    /// paths that normally assemble it. This lets tests reconstruct the one
    /// real, storage-backed scenario where a coalesced batch legitimately
    /// mixes a document-bearing commit with a zero-write one -- the
    /// Postgres-provider catch-up path re-reading a raw journal tail that
    /// spans both -- without standing up an external provider fixture. Takes
    /// unflattened records and computes `commit_identity` the same
    /// kind-aware way that catch-up path does, rather than the caller's own
    /// choice, so the test hook cannot silently drift from production
    /// behavior.
    #[cfg(test)]
    pub(crate) fn process_applied_commit_batch_for_testing(
        &self,
        tenant_id: &TenantId,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let applied = records
            .iter()
            .map(TenantEventRecord::as_commit_entry)
            .collect::<Vec<_>>();
        let commit_identity = document_bearing_commit_identity(records);
        self.process_applied_commit_batch_fanout(runtime.clone(), &applied, commit_identity, false);
        self.enqueue_applied_commit_batch_observers(runtime, &applied);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn enqueue_provider_catch_up_observers_for_testing(
        &self,
        tenant_id: &TenantId,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let applied = records
            .iter()
            .map(TenantEventRecord::as_commit_entry)
            .collect::<Vec<_>>();
        self.enqueue_provider_catch_up_commit_observers(runtime, &applied)
            .await
    }
}
