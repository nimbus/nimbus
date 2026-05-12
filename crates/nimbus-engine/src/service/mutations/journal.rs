use std::{
    collections::{HashMap, HashSet},
    future,
    sync::Arc,
    sync::atomic::AtomicBool,
};

use nimbus_core::{
    AccessAction, CommitEntry, Document, DocumentId, DurableMutationRecord, Error, Mutation,
    Result, TableName, TenantId,
};
use tokio::sync::oneshot;
use tracing::warn;

use crate::Service;
use crate::tenant::{
    QueuedMutationRequest, QueuedMutationResult, TenantOperationGuard, TenantRuntime,
};

use super::direct::{MutationExecutionMode, MutationExecutionResult};
use super::enforce_mutation_authorization;

const MUTATION_JOURNAL_BATCH_SIZE: usize = 32;

struct PendingMutationResponseGuard {
    runtime: Arc<TenantRuntime>,
}

impl Drop for PendingMutationResponseGuard {
    fn drop(&mut self) {
        self.runtime.finish_pending_mutation_response();
    }
}

struct PlannedQueuedMutation {
    cancelled: Arc<AtomicBool>,
    _operation: TenantOperationGuard,
    response: oneshot::Sender<Result<QueuedMutationResult>>,
    result: QueuedMutationResult,
    scheduled_execution_id: Option<String>,
    writes: Vec<nimbus_core::WriteOp>,
}

struct ActiveQueuedMutation {
    _operation: TenantOperationGuard,
    response: oneshot::Sender<Result<QueuedMutationResult>>,
    result: QueuedMutationResult,
}

struct PendingQueuedMutationResponse {
    response: oneshot::Sender<Result<QueuedMutationResult>>,
    result: QueuedMutationResult,
}

struct QueuedMutationBatchResult {
    applied: Vec<CommitEntry>,
    responses: Vec<PendingQueuedMutationResponse>,
}

impl Service {
    pub(super) fn spawn_journal_mutation_worker(self: &Arc<Self>, runtime: Arc<TenantRuntime>) {
        let service = self.clone();
        runtime.record_mutation_worker_start();
        self.spawn_background("mutation_journal", async move {
            service.run_journal_mutation_worker(runtime).await;
        });
    }

    async fn run_journal_mutation_worker(self: Arc<Self>, runtime: Arc<TenantRuntime>) {
        #[cfg(any(test, debug_assertions))]
        Service::assert_running_on_background_task("mutation_journal");

        loop {
            runtime.drain_mutation_admission_queue();
            let batch = runtime
                .drain_mutation_batch(MUTATION_JOURNAL_BATCH_SIZE)
                .await;
            if batch.is_empty() {
                if runtime.release_mutation_worker() {
                    continue;
                }
                break;
            }

            let runtime_for_task = runtime.clone();
            let batch_result = tokio::task::spawn_blocking(move || {
                process_queued_mutation_batch(runtime_for_task, batch)
            })
            .await;

            match batch_result {
                Ok(Ok(batch_result)) => {
                    for pending_response in batch_result.responses {
                        let _ = pending_response.response.send(Ok(pending_response.result));
                    }
                    self.process_applied_commit_batch(runtime.clone(), &batch_result.applied, true);
                }
                Ok(Err(error)) => {
                    runtime.record_mutation_worker_failure();
                    warn!(error = %error, "mutation journal batch failed");
                    if let Ok(progress) = runtime
                        .read_storage
                        .execute(|store| store.recover_durable_journal())
                        .await
                    {
                        runtime.sync_mutation_journal_progress(progress);
                    }
                }
                Err(error) => {
                    runtime.record_mutation_worker_failure();
                    warn!(error = %error, "mutation journal worker panicked");
                    if let Ok(progress) = runtime
                        .read_storage
                        .execute(|store| store.recover_durable_journal())
                        .await
                    {
                        runtime.sync_mutation_journal_progress(progress);
                    }
                }
            }
        }
    }

    pub(super) async fn submit_journaled_async_mutation<Fut>(
        self: &Arc<Self>,
        runtime: Arc<TenantRuntime>,
        tenant_id: &TenantId,
        mode: MutationExecutionMode,
        mutation: Mutation,
        principal: nimbus_core::PrincipalContext,
        cancel_wait: Fut,
    ) -> Result<MutationExecutionResult>
    where
        Fut: future::Future<Output = ()> + Send + 'static,
    {
        let operation = runtime.enter_operation(tenant_id)?;
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let request_cancelled = cancelled.clone();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        runtime.begin_pending_mutation_response();
        let _pending_response = PendingMutationResponseGuard {
            runtime: runtime.clone(),
        };
        let should_start_worker =
            runtime.enqueue_mutation_admission_request(QueuedMutationRequest {
                mutation,
                principal,
                scheduled_execution_id: match mode {
                    MutationExecutionMode::Immediate => None,
                    MutationExecutionMode::Scheduled { execution_id } => Some(execution_id),
                },
                cancelled: request_cancelled,
                _operation: operation,
                response: response_tx,
                enqueued_at: std::time::Instant::now(),
            })?;
        if should_start_worker {
            self.spawn_journal_mutation_worker(runtime.clone());
        }

        tokio::pin!(cancel_wait);
        let mut response_rx = response_rx;
        let result = tokio::select! {
            result = &mut response_rx => {
                result
            }
            _ = &mut cancel_wait => {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                (&mut response_rx).await
            }
        }
        .map_err(|_| Error::Internal("mutation journal worker dropped response".to_string()))??;
        Ok(match result {
            QueuedMutationResult::Immediate(document_id) => {
                MutationExecutionResult::Immediate(document_id)
            }
            QueuedMutationResult::Scheduled(applied) => MutationExecutionResult::Scheduled(applied),
        })
    }
}

fn process_queued_mutation_batch(
    runtime: Arc<TenantRuntime>,
    batch: Vec<QueuedMutationRequest>,
) -> Result<QueuedMutationBatchResult> {
    let sequence_guard = runtime.lock_mutation_sequence();
    let mut overlay = HashMap::<(TableName, DocumentId), Option<Document>>::new();
    let mut scheduled_execution_overlay = HashSet::new();
    let mut planned = Vec::new();

    for request in batch {
        if let Some(planned_request) = plan_queued_mutation_request(
            runtime.as_ref(),
            request,
            &mut overlay,
            &mut scheduled_execution_overlay,
        ) {
            planned.push(planned_request);
        }
    }

    let mut active = Vec::new();
    let mut records = Vec::new();
    let mut next_sequence = runtime.durable_head().0.saturating_add(1);
    for planned_request in planned {
        let PlannedQueuedMutation {
            cancelled,
            _operation,
            response,
            result,
            scheduled_execution_id,
            writes,
        } = planned_request;
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            let _ = response.send(Err(Error::Cancelled));
            continue;
        }
        let record = match DurableMutationRecord::new(
            nimbus_core::SequenceNumber(next_sequence),
            runtime.store.now(),
            writes,
            scheduled_execution_id,
        ) {
            Ok(record) => record,
            Err(error) => {
                let _ = response.send(Err(error));
                continue;
            }
        };
        next_sequence = next_sequence.saturating_add(1);
        active.push(ActiveQueuedMutation {
            _operation,
            response,
            result,
        });
        records.push(record);
    }

    if active.is_empty() {
        return Ok(QueuedMutationBatchResult {
            applied: Vec::new(),
            responses: Vec::new(),
        });
    }

    if let Err(error) = runtime.store.append_durable_records_batch(&records) {
        let mapped_error = map_durable_journal_append_error(&error);
        for active_request in active {
            let _ = active_request
                .response
                .send(Err(map_durable_journal_append_error(&error)));
        }
        return Err(mapped_error);
    }

    if let Some(last_record) = records.last() {
        runtime.mark_durable_head(last_record.sequence);
    }

    let mut applied = Vec::with_capacity(records.len());
    let mut responses = Vec::with_capacity(records.len());
    for (active_request, record) in active.into_iter().zip(records.iter()) {
        responses.push(PendingQueuedMutationResponse {
            response: active_request.response,
            result: active_request.result,
        });
        applied.push(record.as_commit_entry());
    }

    runtime
        .store
        .check_fault(nimbus_storage::FaultPoint::JournalDurableAppendBeforeApply)?;

    let applied_head = match runtime.store.apply_durable_records_batch(&records) {
        Ok(()) => runtime.store.applied_head_after_durable_apply(&records)?,
        Err(_) => {
            let progress = runtime.store.recover_durable_journal()?;
            progress.applied_head
        }
    };
    runtime.invalidate_document_cache_for_commits(applied.iter());
    runtime.mark_applied_head(applied_head);
    drop(sequence_guard);

    Ok(QueuedMutationBatchResult { applied, responses })
}

fn plan_queued_mutation_request(
    runtime: &TenantRuntime,
    request: QueuedMutationRequest,
    overlay: &mut HashMap<(TableName, DocumentId), Option<Document>>,
    scheduled_execution_overlay: &mut HashSet<String>,
) -> Option<PlannedQueuedMutation> {
    let QueuedMutationRequest {
        mutation,
        principal,
        scheduled_execution_id,
        cancelled,
        _operation,
        response,
        ..
    } = request;

    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        let _ = response.send(Err(Error::Cancelled));
        return None;
    }

    if let Some(execution_id) = scheduled_execution_id.as_deref() {
        if scheduled_execution_overlay.contains(execution_id) {
            let _ = response.send(Ok(QueuedMutationResult::Scheduled(false)));
            return None;
        }
        match runtime.store.scheduled_execution_exists(execution_id) {
            Ok(true) => {
                let _ = response.send(Ok(QueuedMutationResult::Scheduled(false)));
                return None;
            }
            Ok(false) => {}
            Err(error) => {
                let _ = response.send(Err(error));
                return None;
            }
        }
    }

    let schema = runtime.schema();
    match mutation {
        Mutation::Insert { table, id, fields } => {
            let table_schema = schema.get_table(&table).cloned();
            if let Some(table_schema) = table_schema.as_ref()
                && let Err(error) = table_schema.validate(&fields)
            {
                let _ = response.send(Err(error));
                return None;
            }
            let document = match id {
                Some(document_id) => Document::with_id(document_id, table.clone(), fields),
                None => Document::new(table.clone(), fields),
            };
            if let Err(error) = enforce_mutation_authorization(
                table_schema.as_ref(),
                AccessAction::Create,
                &principal,
                Some(&document),
                None,
            ) {
                let _ = response.send(Err(error));
                return None;
            }
            let document_id = document.id.clone();
            overlay.insert((table, document_id.clone()), Some(document.clone()));
            if let Some(execution_id) = scheduled_execution_id.as_ref() {
                scheduled_execution_overlay.insert(execution_id.clone());
            }
            let result = match scheduled_execution_id.as_ref() {
                Some(_) => QueuedMutationResult::Scheduled(true),
                None => QueuedMutationResult::Immediate(Some(document_id.clone())),
            };
            Some(PlannedQueuedMutation {
                cancelled,
                _operation,
                response,
                result,
                scheduled_execution_id,
                writes: vec![nimbus_core::WriteOp {
                    table: document.table.clone(),
                    op_type: nimbus_core::WriteOpType::Insert,
                    doc_id: document_id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(document),
                }],
            })
        }
        Mutation::Update { table, id, patch } => {
            let table_schema = schema.get_table(&table).cloned();
            let existing = match load_batched_document(runtime, overlay, &table, &id) {
                Ok(Some(existing)) => existing,
                Ok(None) => {
                    let _ = response.send(Err(Error::DocumentNotFound(id)));
                    return None;
                }
                Err(error) => {
                    let _ = response.send(Err(error));
                    return None;
                }
            };
            let mut document = existing.clone();
            for (field, value) in patch {
                document.fields.insert(field, value);
            }
            if let Some(table_schema) = table_schema.as_ref()
                && let Err(error) = table_schema.validate(&document.fields)
            {
                let _ = response.send(Err(error));
                return None;
            }
            if let Err(error) = enforce_mutation_authorization(
                table_schema.as_ref(),
                AccessAction::Update,
                &principal,
                Some(&document),
                Some(&existing),
            ) {
                let _ = response.send(Err(error));
                return None;
            }
            overlay.insert((table.clone(), id.clone()), Some(document.clone()));
            if let Some(execution_id) = scheduled_execution_id.as_ref() {
                scheduled_execution_overlay.insert(execution_id.clone());
            }
            let result = match scheduled_execution_id.as_ref() {
                Some(_) => QueuedMutationResult::Scheduled(true),
                None => QueuedMutationResult::Immediate(Some(id.clone())),
            };
            Some(PlannedQueuedMutation {
                cancelled,
                _operation,
                response,
                result,
                scheduled_execution_id,
                writes: vec![nimbus_core::WriteOp {
                    table: table.clone(),
                    op_type: nimbus_core::WriteOpType::Update,
                    doc_id: id,
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: Some(existing),
                    current: Some(document),
                }],
            })
        }
        Mutation::Delete { table, id } => {
            let table_schema = schema.get_table(&table).cloned();
            let existing = match load_batched_document(runtime, overlay, &table, &id) {
                Ok(Some(existing)) => existing,
                Ok(None) => {
                    let _ = response.send(Err(Error::DocumentNotFound(id)));
                    return None;
                }
                Err(error) => {
                    let _ = response.send(Err(error));
                    return None;
                }
            };
            if let Err(error) = enforce_mutation_authorization(
                table_schema.as_ref(),
                AccessAction::Delete,
                &principal,
                None,
                Some(&existing),
            ) {
                let _ = response.send(Err(error));
                return None;
            }
            overlay.insert((table.clone(), id.clone()), None);
            if let Some(execution_id) = scheduled_execution_id.as_ref() {
                scheduled_execution_overlay.insert(execution_id.clone());
            }
            let result = match scheduled_execution_id.as_ref() {
                Some(_) => QueuedMutationResult::Scheduled(true),
                None => QueuedMutationResult::Immediate(None),
            };
            Some(PlannedQueuedMutation {
                cancelled,
                _operation,
                response,
                result,
                scheduled_execution_id,
                writes: vec![nimbus_core::WriteOp {
                    table: table.clone(),
                    op_type: nimbus_core::WriteOpType::Delete,
                    doc_id: id,
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: Some(existing),
                    current: None,
                }],
            })
        }
    }
}

fn map_durable_journal_append_error(error: &Error) -> Error {
    match error {
        Error::InvalidInput(message) => Error::InvalidInput(message.clone()),
        _ => Error::Internal(format!("durable journal append failed: {error}")),
    }
}

fn load_batched_document(
    runtime: &TenantRuntime,
    overlay: &HashMap<(TableName, DocumentId), Option<Document>>,
    table: &TableName,
    id: &DocumentId,
) -> Result<Option<Document>> {
    if let Some(document) = overlay.get(&(table.clone(), id.clone())) {
        return Ok(document.clone());
    }
    runtime.store.get(table, id)
}
