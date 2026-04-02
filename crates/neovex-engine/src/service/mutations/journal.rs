use std::{
    collections::{HashMap, HashSet},
    future,
    sync::Arc,
};

use neovex_core::{
    AccessAction, CommitEntry, Document, DocumentId, DurableMutationRecord, Error, Mutation,
    Result, TableName, TenantId,
};
use tracing::warn;

use crate::tenant::{QueuedMutationRequest, QueuedMutationResult, TenantRuntime};

use super::{
    MutationExecutionMode, MutationExecutionResult, Service, enforce_mutation_authorization,
};

const MUTATION_JOURNAL_BATCH_SIZE: usize = 32;

struct PlannedQueuedMutation {
    request: QueuedMutationRequest,
    result: QueuedMutationResult,
    scheduled_execution_id: Option<String>,
    candidate_documents: Vec<Document>,
    deleted_documents: Vec<Document>,
    writes: Vec<neovex_core::WriteOp>,
}

struct AppliedQueuedMutation {
    commit: CommitEntry,
    candidate_documents: Vec<Document>,
    deleted_documents: Vec<Document>,
}

struct QueuedMutationBatchResult {
    applied: Vec<AppliedQueuedMutation>,
}

impl Service {
    pub(super) fn spawn_journal_mutation_worker(self: &Arc<Self>, runtime: Arc<TenantRuntime>) {
        let service = self.clone();
        tokio::spawn(async move {
            service.run_journal_mutation_worker(runtime).await;
        });
    }

    async fn run_journal_mutation_worker(self: Arc<Self>, runtime: Arc<TenantRuntime>) {
        loop {
            let batch = runtime
                .drain_mutation_batch(MUTATION_JOURNAL_BATCH_SIZE)
                .await;
            if batch.is_empty() {
                if runtime.release_mutation_worker().await {
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
                    for applied in batch_result.applied {
                        self.process_commit(
                            runtime.clone(),
                            &applied.commit,
                            &applied.candidate_documents,
                            &applied.deleted_documents,
                        );
                    }
                }
                Ok(Err(error)) => {
                    warn!(error = %error, "mutation journal batch failed");
                    if let Ok(progress) = runtime.store.recover_durable_journal() {
                        runtime.sync_mutation_journal_progress(progress);
                    }
                }
                Err(error) => {
                    warn!(error = %error, "mutation journal worker panicked");
                    if let Ok(progress) = runtime.store.recover_durable_journal() {
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
        principal: neovex_core::PrincipalContext,
        cancel_wait: Fut,
    ) -> Result<MutationExecutionResult>
    where
        Fut: future::Future<Output = ()> + Send + 'static,
    {
        let operation = runtime.enter_operation(tenant_id)?;
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let request_cancelled = cancelled.clone();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let should_start_worker = runtime
            .enqueue_mutation_request(QueuedMutationRequest {
                mutation,
                principal,
                scheduled_execution_id: match mode {
                    MutationExecutionMode::Immediate => None,
                    MutationExecutionMode::Scheduled { execution_id } => Some(execution_id),
                },
                cancelled: request_cancelled,
                _operation: operation,
                response: response_tx,
            })
            .await;
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
    let _sequence_guard = runtime.lock_mutation_sequence();
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
    let mut next_sequence = runtime.durable_head().0.saturating_add(1);
    for planned_request in planned {
        if planned_request
            .request
            .cancelled
            .load(std::sync::atomic::Ordering::Acquire)
        {
            let _ = planned_request.request.response.send(Err(Error::Cancelled));
            continue;
        }
        let record = match DurableMutationRecord::new(
            neovex_core::SequenceNumber(next_sequence),
            runtime.store.now(),
            planned_request.writes.clone(),
            planned_request.scheduled_execution_id.clone(),
        ) {
            Ok(record) => record,
            Err(error) => {
                let _ = planned_request.request.response.send(Err(error));
                continue;
            }
        };
        next_sequence = next_sequence.saturating_add(1);
        active.push((planned_request, record));
    }

    if active.is_empty() {
        return Ok(QueuedMutationBatchResult {
            applied: Vec::new(),
        });
    }

    let records = active
        .iter()
        .map(|(_, record)| record.clone())
        .collect::<Vec<_>>();
    if let Err(error) = runtime.store.append_durable_records_batch(records.clone()) {
        let message = format!("durable journal append failed: {error}");
        for (planned_request, _) in active {
            let _ = planned_request
                .request
                .response
                .send(Err(Error::Internal(message.clone())));
        }
        return Err(Error::Internal(message));
    }

    if let Some(last_record) = records.last() {
        runtime.mark_durable_head(last_record.sequence);
    }

    let mut applied = Vec::with_capacity(records.len());
    for (planned_request, record) in active {
        let _ = planned_request
            .request
            .response
            .send(Ok(planned_request.result));
        applied.push(AppliedQueuedMutation {
            commit: record.as_commit_entry(),
            candidate_documents: planned_request.candidate_documents.clone(),
            deleted_documents: planned_request.deleted_documents.clone(),
        });
    }

    runtime
        .store
        .check_fault(neovex_storage::FaultPoint::JournalDurableAppendBeforeApply)?;

    let applied_head = match runtime.store.apply_durable_records_batch(&records) {
        Ok(()) => runtime.store.applied_sequence()?,
        Err(_) => {
            let progress = runtime.store.recover_durable_journal()?;
            progress.applied_head
        }
    };
    runtime.mark_applied_head(applied_head);

    Ok(QueuedMutationBatchResult { applied })
}

fn plan_queued_mutation_request(
    runtime: &TenantRuntime,
    request: QueuedMutationRequest,
    overlay: &mut HashMap<(TableName, DocumentId), Option<Document>>,
    scheduled_execution_overlay: &mut HashSet<String>,
) -> Option<PlannedQueuedMutation> {
    if request.cancelled.load(std::sync::atomic::Ordering::Acquire) {
        let _ = request.response.send(Err(Error::Cancelled));
        return None;
    }

    if let Some(execution_id) = request.scheduled_execution_id.as_deref() {
        if scheduled_execution_overlay.contains(execution_id) {
            let _ = request
                .response
                .send(Ok(QueuedMutationResult::Scheduled(false)));
            return None;
        }
        match runtime.store.scheduled_execution_exists(execution_id) {
            Ok(true) => {
                let _ = request
                    .response
                    .send(Ok(QueuedMutationResult::Scheduled(false)));
                return None;
            }
            Ok(false) => {}
            Err(error) => {
                let _ = request.response.send(Err(error));
                return None;
            }
        }
    }

    let schema = runtime.schema();
    match request.mutation.clone() {
        Mutation::Insert { table, fields } => {
            let table_schema = schema.get_table(&table).cloned();
            let planned = (|| -> Result<(Document, Vec<neovex_core::WriteOp>)> {
                if let Some(table_schema) = table_schema.as_ref() {
                    table_schema.validate(&fields)?;
                }
                let document = Document::new(table.clone(), fields);
                enforce_mutation_authorization(
                    table_schema.as_ref(),
                    AccessAction::Create,
                    &request.principal,
                    Some(&document),
                    None,
                )?;
                Ok((
                    document.clone(),
                    vec![neovex_core::WriteOp {
                        table: document.table.clone(),
                        op_type: neovex_core::WriteOpType::Insert,
                        doc_id: document.id,
                        previous: None,
                        current: Some(document),
                    }],
                ))
            })();
            match planned {
                Ok((document, writes)) => {
                    overlay.insert((table, document.id), Some(document.clone()));
                    let scheduled_execution_id = request.scheduled_execution_id.clone();
                    if let Some(execution_id) = scheduled_execution_id.clone() {
                        scheduled_execution_overlay.insert(execution_id);
                    }
                    let result = match scheduled_execution_id {
                        Some(_) => QueuedMutationResult::Scheduled(true),
                        None => QueuedMutationResult::Immediate(Some(document.id)),
                    };
                    Some(PlannedQueuedMutation {
                        request,
                        result,
                        scheduled_execution_id,
                        candidate_documents: vec![document],
                        deleted_documents: Vec::new(),
                        writes,
                    })
                }
                Err(error) => {
                    let _ = request.response.send(Err(error));
                    None
                }
            }
        }
        Mutation::Update { table, id, patch } => {
            let table_schema = schema.get_table(&table).cloned();
            let planned = (|| -> Result<(Document, Document, Vec<neovex_core::WriteOp>)> {
                let existing = load_batched_document(runtime, overlay, &table, id)?
                    .ok_or(Error::DocumentNotFound(id))?;
                let mut document = existing.clone();
                for (field, value) in patch {
                    document.fields.insert(field, value);
                }
                if let Some(table_schema) = table_schema.as_ref() {
                    table_schema.validate(&document.fields)?;
                }
                enforce_mutation_authorization(
                    table_schema.as_ref(),
                    AccessAction::Update,
                    &request.principal,
                    Some(&document),
                    Some(&existing),
                )?;
                Ok((
                    existing.clone(),
                    document.clone(),
                    vec![neovex_core::WriteOp {
                        table: table.clone(),
                        op_type: neovex_core::WriteOpType::Update,
                        doc_id: id,
                        previous: Some(existing),
                        current: Some(document),
                    }],
                ))
            })();
            match planned {
                Ok((_existing, document, writes)) => {
                    overlay.insert((table.clone(), id), Some(document));
                    let scheduled_execution_id = request.scheduled_execution_id.clone();
                    if let Some(execution_id) = scheduled_execution_id.clone() {
                        scheduled_execution_overlay.insert(execution_id);
                    }
                    let result = match scheduled_execution_id {
                        Some(_) => QueuedMutationResult::Scheduled(true),
                        None => QueuedMutationResult::Immediate(Some(id)),
                    };
                    Some(PlannedQueuedMutation {
                        request,
                        result,
                        scheduled_execution_id,
                        candidate_documents: Vec::new(),
                        deleted_documents: Vec::new(),
                        writes,
                    })
                }
                Err(error) => {
                    let _ = request.response.send(Err(error));
                    None
                }
            }
        }
        Mutation::Delete { table, id } => {
            let table_schema = schema.get_table(&table).cloned();
            let planned = (|| -> Result<(Document, Vec<neovex_core::WriteOp>)> {
                let existing = load_batched_document(runtime, overlay, &table, id)?
                    .ok_or(Error::DocumentNotFound(id))?;
                enforce_mutation_authorization(
                    table_schema.as_ref(),
                    AccessAction::Delete,
                    &request.principal,
                    None,
                    Some(&existing),
                )?;
                Ok((
                    existing.clone(),
                    vec![neovex_core::WriteOp {
                        table: table.clone(),
                        op_type: neovex_core::WriteOpType::Delete,
                        doc_id: id,
                        previous: Some(existing),
                        current: None,
                    }],
                ))
            })();
            match planned {
                Ok((existing, writes)) => {
                    overlay.insert((table.clone(), id), None);
                    let scheduled_execution_id = request.scheduled_execution_id.clone();
                    if let Some(execution_id) = scheduled_execution_id.clone() {
                        scheduled_execution_overlay.insert(execution_id);
                    }
                    let result = match scheduled_execution_id {
                        Some(_) => QueuedMutationResult::Scheduled(true),
                        None => QueuedMutationResult::Immediate(None),
                    };
                    Some(PlannedQueuedMutation {
                        request,
                        result,
                        scheduled_execution_id,
                        candidate_documents: vec![existing.clone()],
                        deleted_documents: vec![existing],
                        writes,
                    })
                }
                Err(error) => {
                    let _ = request.response.send(Err(error));
                    None
                }
            }
        }
    }
}

fn load_batched_document(
    runtime: &TenantRuntime,
    overlay: &HashMap<(TableName, DocumentId), Option<Document>>,
    table: &TableName,
    id: DocumentId,
) -> Result<Option<Document>> {
    if let Some(document) = overlay.get(&(table.clone(), id)) {
        return Ok(document.clone());
    }
    runtime.store.get(table, &id)
}
