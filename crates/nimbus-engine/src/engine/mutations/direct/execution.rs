use std::{future, sync::Arc, time::Instant};

use nimbus_core::{
    AccessAction, DependencySet, Document, DocumentId, DocumentLocator, Error, Mutation,
    PrincipalContext, Result, Schema, TenantId, Timestamp, WriteOp, WriteOpType,
};

use crate::engine::tenants::with_tenant_runtime_operation;
use crate::{Engine, tenant::TenantRuntime};

use super::super::caps::check_mutation_caps;
use super::super::enforce_mutation_authorization;
use super::super::journal::{
    mutation_occ_backoff, mutation_occ_max_attempts, validate_prepared_for_provider,
};
use super::super::prepared::PreparedCommit;
use super::super::shadow_conflicts::{observe_shadow_conflicts, prepared_document_dependencies};
use super::super::window_prepare::prepare_single_document_write_from_window;
use super::store::DirectMutationProfile;
use super::types::{MutationExecutionMode, MutationExecutionResult};

enum PreparedDirectMutation {
    DuplicateScheduledExecution,
    Commit {
        prepared_commit: Box<PreparedCommit>,
        result_document_id: Option<DocumentId>,
    },
}

impl Engine {
    pub(super) fn apply_mutation_with_mode(
        &self,
        tenant_id: &TenantId,
        mode: MutationExecutionMode,
        mutation: Mutation,
        principal: &PrincipalContext,
    ) -> Result<MutationExecutionResult> {
        with_tenant_runtime_operation(self.get_existing_tenant(tenant_id)?, tenant_id, |runtime| {
            // A generated insert id is part of the logical mutation and must
            // survive transparent stale-prepare retries unchanged.
            let mutation = normalize_direct_insert_id(self, mutation);
            let max_attempts = mutation_occ_max_attempts();
            let mut attempt = 1;
            let mut rate_accounted = false;
            loop {
                let prepare_started = Instant::now();
                let (prepared, prepare_permit) = if let Some(prepared) =
                    prepare_direct_mutation_from_window(
                        runtime.as_ref(),
                        &mode,
                        &mutation,
                        principal,
                    )? {
                    runtime.commit_phase_metrics().record_window_prepare();
                    (prepared, None)
                } else {
                    runtime.commit_phase_metrics().record_storage_prepare();
                    let permit = runtime.acquire_prepare_permit_blocking()?;
                    let prepared = prepare_direct_mutation(
                        runtime.as_ref(),
                        runtime.schema(),
                        &mode,
                        mutation.clone(),
                        principal,
                    )?;
                    (prepared, Some(permit))
                };
                runtime
                    .commit_phase_metrics()
                    .record_prepare_pool(prepare_started.elapsed());
                let PreparedDirectMutation::Commit {
                    prepared_commit,
                    result_document_id,
                } = prepared
                else {
                    return Ok(MutationExecutionResult::Scheduled(false));
                };
                self.wait_for_commit_fault(
                    crate::engine::execution_units::labels::PREPARE_COMPLETE,
                )?;
                let shadow_dependencies =
                    prepared_document_dependencies(&prepared_commit, |_| None);
                observe_shadow_conflicts(
                    runtime.as_ref(),
                    prepared_commit.snapshot_sequence,
                    std::slice::from_ref(&shadow_dependencies),
                );
                drop(prepare_permit);
                check_mutation_caps(&runtime, prepared_commit.usage())?;
                if !rate_accounted {
                    runtime.check_tenant_write_rate(
                        self.now(),
                        prepared_commit.usage().total_write_bytes(),
                    )?;
                    rate_accounted = true;
                }
                let prepared_bytes = prepared_commit.accounted_bytes();
                let _prepared_payload =
                    crate::tenant::PreparedPayloadAccounting::new(runtime.clone(), prepared_bytes);
                let profile = DirectMutationProfile::after_prepare(prepare_started);
                match self.run_prepared_direct_mutation(runtime.clone(), *prepared_commit, profile)
                {
                    Ok(Some(_)) => {
                        return Ok(match &mode {
                            MutationExecutionMode::Immediate => {
                                MutationExecutionResult::Immediate(result_document_id)
                            }
                            MutationExecutionMode::Scheduled { .. } => {
                                MutationExecutionResult::Scheduled(true)
                            }
                        });
                    }
                    Ok(None) => return Ok(MutationExecutionResult::Scheduled(false)),
                    Err(error) if error.retryability() == nimbus_core::Retryability::Retryable => {
                        if attempt >= max_attempts {
                            runtime
                                .commit_phase_metrics()
                                .record_mutation_conflict_exhausted();
                            return Err(error.with_conflict_attempts(attempt));
                        }
                        if let Some(sequence) = error.conflicting_sequence() {
                            runtime.wait_for_applied_sequence_blocking(sequence);
                        }
                        runtime
                            .commit_phase_metrics()
                            .record_mutation_conflict_retry();
                        std::thread::sleep(mutation_occ_backoff(attempt));
                        attempt += 1;
                    }
                    Err(error) => return Err(error),
                }
            }
        })
    }

    pub(super) fn apply_mutation_with_principal(
        &self,
        tenant_id: &TenantId,
        mutation: Mutation,
        principal: &PrincipalContext,
    ) -> Result<Option<DocumentId>> {
        super::types::expect_immediate_result(
            self.apply_mutation_with_mode(
                tenant_id,
                MutationExecutionMode::Immediate,
                mutation,
                principal,
            )?,
            "immediate mutation execution should not return a scheduled result",
        )
    }

    pub(super) async fn apply_mutation_with_mode_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        mode: MutationExecutionMode,
        mutation: Mutation,
        principal: PrincipalContext,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<MutationExecutionResult>
    where
        Fut: future::Future<Output = ()> + Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        check_cancel()?;
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        self.submit_journaled_async_mutation(
            runtime,
            &tenant_id,
            mode,
            mutation,
            principal,
            cancel_wait,
        )
        .await
    }
}

fn prepare_direct_mutation_from_window(
    runtime: &TenantRuntime,
    mode: &MutationExecutionMode,
    mutation: &Mutation,
    principal: &PrincipalContext,
) -> Result<Option<PreparedDirectMutation>> {
    if !matches!(mode, MutationExecutionMode::Immediate) {
        return Ok(None);
    }
    let Some(prepared) = prepare_single_document_write_from_window(runtime, mutation, principal)?
    else {
        return Ok(None);
    };
    let result_document_id = prepared.result_document_id;
    let prepared_commit = PreparedCommit::for_direct(
        prepared.snapshot_sequence,
        prepared.dependencies,
        prepared.write,
        prepared.indexes,
        None,
    )?
    .with_inline_reprepare(
        prepared.normalized_mutation,
        principal.clone(),
        prepared.schema,
    );
    Ok(Some(PreparedDirectMutation::Commit {
        prepared_commit: Box::new(prepared_commit),
        result_document_id,
    }))
}

fn normalize_direct_insert_id(engine: &Engine, mutation: Mutation) -> Mutation {
    match mutation {
        Mutation::Insert {
            table,
            id: None,
            fields,
        } => Mutation::Insert {
            table,
            id: Some(engine.next_document_id()),
            fields,
        },
        mutation => mutation,
    }
}

fn prepare_direct_mutation(
    runtime: &TenantRuntime,
    schema: Arc<Schema>,
    mode: &MutationExecutionMode,
    mutation: Mutation,
    principal: &PrincipalContext,
) -> Result<PreparedDirectMutation> {
    let inline_mutation = mutation.clone();
    let scheduled_execution_id = match mode {
        MutationExecutionMode::Immediate => None,
        MutationExecutionMode::Scheduled { execution_id } => Some(execution_id.as_str()),
    };
    if let Some(execution_id) = scheduled_execution_id
        && runtime.store.scheduled_execution_exists(execution_id)?
    {
        return Ok(PreparedDirectMutation::DuplicateScheduledExecution);
    }

    // The opened snapshot supplies both full images and the OCC pin. Direct
    // authorization, schema validation, record serialization, and index
    // selection all finish here on the caller before DirectCommit admission.
    let snapshot = runtime.store.read_snapshot()?;
    let snapshot_sequence = snapshot.applied_sequence()?;
    let (write, indexes, result_document_id) = match mutation {
        Mutation::Insert { table, id, fields } => {
            let table_schema = schema.get_table(&table).cloned();
            let indexes = table_schema
                .as_ref()
                .map(|table_schema| {
                    table_schema.validate(&fields)?;
                    Ok(table_schema.indexes.clone())
                })
                .transpose()?
                .unwrap_or_default();
            let document_id = id.ok_or_else(|| {
                Error::Internal("direct insert id must be normalized before prepare".to_string())
            })?;
            let document =
                Document::with_id_at(document_id.clone(), table.clone(), fields, Timestamp(0));
            enforce_mutation_authorization(
                table_schema.as_ref(),
                AccessAction::Create,
                principal,
                Some(&document),
                None,
            )?;
            let table_id = runtime.prepared_table_id(&table, snapshot.table_id(&table)?);
            (
                WriteOp {
                    table,
                    table_id,
                    op_type: WriteOpType::Insert,
                    doc_id: document_id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(document),
                },
                indexes,
                Some(document_id),
            )
        }
        Mutation::Update { table, id, patch } => {
            let table_id = snapshot
                .table_id(&table)?
                .ok_or_else(|| Error::DocumentNotFound(id.clone()))?;
            let previous = snapshot
                .get(&table, &id)?
                .ok_or_else(|| Error::DocumentNotFound(id.clone()))?;
            let mut current = previous.clone();
            for (field, value) in patch {
                current.fields.insert(field, value);
            }
            let table_schema = schema.get_table(&table).cloned();
            let indexes = table_schema
                .as_ref()
                .map(|table_schema| {
                    table_schema.validate(&current.fields)?;
                    Ok(table_schema.indexes.clone())
                })
                .transpose()?
                .unwrap_or_default();
            enforce_mutation_authorization(
                table_schema.as_ref(),
                AccessAction::Update,
                principal,
                Some(&current),
                Some(&previous),
            )?;
            (
                WriteOp {
                    table,
                    table_id,
                    op_type: WriteOpType::Update,
                    doc_id: id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: Some(previous),
                    current: Some(current),
                },
                indexes,
                Some(id),
            )
        }
        Mutation::Delete { table, id } => {
            let table_id = snapshot
                .table_id(&table)?
                .ok_or_else(|| Error::DocumentNotFound(id.clone()))?;
            let previous = snapshot
                .get(&table, &id)?
                .ok_or_else(|| Error::DocumentNotFound(id.clone()))?;
            let table_schema = schema.get_table(&table).cloned();
            enforce_mutation_authorization(
                table_schema.as_ref(),
                AccessAction::Delete,
                principal,
                None,
                Some(&previous),
            )?;
            let indexes = table_schema
                .as_ref()
                .map(|table_schema| table_schema.indexes.clone())
                .unwrap_or_default();
            let resource_path_binding =
                snapshot.resource_path_binding(&DocumentLocator::new(table.clone(), id.clone()))?;
            (
                WriteOp {
                    table,
                    table_id,
                    op_type: WriteOpType::Delete,
                    doc_id: id,
                    resource_path_binding,
                    trigger_write_origin: None,
                    previous: Some(previous),
                    current: None,
                },
                indexes,
                None,
            )
        }
    };
    let mut dependencies = DependencySet::default();
    dependencies.record_document(&write.table, &write.table_id, write.doc_id.clone());
    validate_prepared_for_provider(runtime, snapshot_sequence, &dependencies)?;
    Ok(PreparedDirectMutation::Commit {
        prepared_commit: Box::new(
            PreparedCommit::for_direct(
                snapshot_sequence,
                dependencies,
                write,
                indexes,
                scheduled_execution_id,
            )?
            .with_inline_reprepare(inline_mutation, principal.clone(), schema),
        ),
        result_document_id,
    })
}
