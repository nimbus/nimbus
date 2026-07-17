use nimbus_core::{
    AccessAction, DependencySet, Document, Error, Mutation, Result, Timestamp, WriteOp, WriteOpType,
};

use crate::tenant::TenantRuntime;

use super::enforce_mutation_authorization;
use super::prepared::{PreparedCommit, PreparedSerializedEffects};
use super::write_log::{SingleDocumentWindowChange, ValidationSource, WindowDocumentState};

pub(super) enum InlineReprepareOutcome {
    Fresh,
    Reprepared,
    CallerWait(Error),
}

/// Pure serial-step recovery for stale path-A/C single-document prepares.
/// The only input image comes from the retained write-log view; this function
/// performs no storage access and cannot await.
pub(super) fn reprepare_single_document_from_window(
    runtime: &TenantRuntime,
    prepared: &mut PreparedCommit,
    dependencies: &DependencySet,
) -> Result<InlineReprepareOutcome> {
    let durable_head = runtime.durable_head();
    if matches!(
        prepared.serialized_effects,
        PreparedSerializedEffects::Direct { .. }
    ) && runtime.applied_head() < durable_head
    {
        // PPSC4's direct persistence call consumes the applied storage image
        // and publishes through its assigned sequence. It must therefore wait
        // behind every durable-but-unapplied predecessor even when its prepare
        // already observed that predecessor's storage image and would
        // otherwise validate as unchanged.
        return Ok(InlineReprepareOutcome::CallerWait(
            Error::retryable_conflict(
                "direct prepare requires the durable prefix to be applied",
                Some(durable_head),
            ),
        ));
    }
    if dependencies.is_empty() || !runtime.store.has_process_local_sequence_authority() {
        return Ok(InlineReprepareOutcome::Fresh);
    }
    if prepared.snapshot_sequence == durable_head
        && runtime.applied_head() == durable_head
        && runtime.write_log.assigned_through() == durable_head
    {
        return Ok(InlineReprepareOutcome::Fresh);
    }
    let Some(plan) = prepared.inline_reprepare.as_ref() else {
        let source = match runtime
            .write_log
            .validation_source(prepared.snapshot_sequence, durable_head)
        {
            Ok(source) => source,
            Err(_) => {
                return Ok(InlineReprepareOutcome::CallerWait(caller_wait_conflict(
                    runtime,
                    "prepared mutation predates the retained full-image window",
                )));
            }
        };
        let ValidationSource::InMemory(view) = source else {
            return Ok(InlineReprepareOutcome::CallerWait(caller_wait_conflict(
                runtime,
                "prepared mutation is outside the process-local conflict window",
            )));
        };
        let Some(conflicting_sequence) = view.first_conflicting_sequence(dependencies, |_, _| {
            Err(Error::Internal(
                "full-image write-log validation unexpectedly requested storage".to_string(),
            ))
        }) else {
            return Ok(InlineReprepareOutcome::Fresh);
        };
        return Ok(InlineReprepareOutcome::CallerWait(
            Error::retryable_conflict(
                "prepared mutation became stale before sequence assignment",
                Some(conflicting_sequence),
            ),
        ));
    };
    let (table, document_id) = mutation_key(&plan.mutation)?;
    let change = match runtime.write_log.single_document_change_since(
        prepared.snapshot_sequence,
        table,
        document_id,
    ) {
        Ok(Some(change)) => change,
        Ok(None) | Err(_) => {
            return Ok(InlineReprepareOutcome::CallerWait(caller_wait_conflict(
                runtime,
                "stale prepare has no safe retained document image",
            )));
        }
    };
    let base = match change {
        SingleDocumentWindowChange::Unchanged => return Ok(InlineReprepareOutcome::Fresh),
        SingleDocumentWindowChange::Changed { latest } => *latest,
        SingleDocumentWindowChange::WholeTable { sequence } => {
            return Ok(InlineReprepareOutcome::CallerWait(
                Error::retryable_conflict(
                    "stale prepare crossed a table-wide schema or lifecycle change",
                    Some(sequence),
                ),
            ));
        }
    };
    let plan = prepared
        .inline_reprepare
        .take()
        .expect("single-document inline plan must remain attached through validation");
    rebuild_prepared_commit(prepared, plan, base)?;
    runtime.commit_phase_metrics().record_inline_reprepare();
    Ok(InlineReprepareOutcome::Reprepared)
}

fn caller_wait_conflict(runtime: &TenantRuntime, message: &'static str) -> Error {
    Error::retryable_conflict(message, Some(runtime.applied_head()))
}

fn mutation_key(
    mutation: &Mutation,
) -> Result<(&nimbus_core::TableName, &nimbus_core::DocumentId)> {
    match mutation {
        Mutation::Insert {
            table,
            id: Some(document_id),
            ..
        }
        | Mutation::Update {
            table,
            id: document_id,
            ..
        }
        | Mutation::Delete {
            table,
            id: document_id,
        } => Ok((table, document_id)),
        Mutation::Insert { id: None, .. } => Err(Error::Internal(
            "inline re-prepare requires a normalized insert id".to_string(),
        )),
    }
}

fn rebuild_prepared_commit(
    prepared: &mut PreparedCommit,
    plan: super::prepared::InlineRepreparePlan,
    base: WindowDocumentState,
) -> Result<()> {
    let scheduled_execution_id = match &prepared.serialized_effects {
        PreparedSerializedEffects::Journal {
            scheduled_execution_id,
        }
        | PreparedSerializedEffects::Direct {
            scheduled_execution_id,
            ..
        } => scheduled_execution_id.clone(),
        PreparedSerializedEffects::ExecutionUnit { .. } => {
            return Err(Error::Internal(
                "execution-unit commits must not use single-document inline re-prepare".to_string(),
            ));
        }
    };
    let is_direct = matches!(
        prepared.serialized_effects,
        PreparedSerializedEffects::Direct { .. }
    );
    let (table, _) = mutation_key(&plan.mutation)?;
    let table_schema = plan.schema.get_table(table).cloned();
    let write = rebuild_write(&plan, &base, table_schema.as_ref())?;
    let mut dependencies = DependencySet::default();
    dependencies.record_document(&write.table, &write.table_id, write.doc_id.clone());
    let mut rebuilt = if is_direct {
        PreparedCommit::for_direct(
            base.sequence,
            dependencies,
            write,
            table_schema
                .as_ref()
                .map(|schema| schema.indexes.clone())
                .unwrap_or_default(),
            scheduled_execution_id.as_deref(),
        )?
    } else {
        PreparedCommit::for_journal(base.sequence, vec![write], scheduled_execution_id)
    };
    rebuilt.inline_reprepare = Some(plan);
    *prepared = rebuilt;
    Ok(())
}

fn rebuild_write(
    plan: &super::prepared::InlineRepreparePlan,
    base: &WindowDocumentState,
    table_schema: Option<&nimbus_core::TableSchema>,
) -> Result<WriteOp> {
    let (table, document_id) = mutation_key(&plan.mutation)?;
    let (op_type, previous, current) = match &plan.mutation {
        Mutation::Insert { fields, .. } => {
            if base.document.is_some() {
                return Err(Error::conflict(format!(
                    "insert precondition failed against the latest document image at sequence {}",
                    base.sequence
                )));
            }
            if let Some(schema) = table_schema {
                schema.validate(fields)?;
            }
            let document = Document::with_id_at(
                document_id.clone(),
                table.clone(),
                fields.clone(),
                Timestamp(0),
            );
            enforce_mutation_authorization(
                table_schema,
                AccessAction::Create,
                &plan.principal,
                Some(&document),
                None,
            )?;
            (WriteOpType::Insert, None, Some(document))
        }
        Mutation::Update { patch, .. } => {
            let Some(previous) = base.document.clone() else {
                return Err(Error::conflict(format!(
                    "update precondition failed against the latest document image at sequence {}",
                    base.sequence
                )));
            };
            let mut current = previous.clone();
            for (field, value) in patch {
                current.fields.insert(field.clone(), value.clone());
            }
            if let Some(schema) = table_schema {
                schema.validate(&current.fields)?;
            }
            enforce_mutation_authorization(
                table_schema,
                AccessAction::Update,
                &plan.principal,
                Some(&current),
                Some(&previous),
            )?;
            (WriteOpType::Update, Some(previous), Some(current))
        }
        Mutation::Delete { .. } => {
            let Some(previous) = base.document.clone() else {
                return Err(Error::conflict(format!(
                    "delete precondition failed against the latest document image at sequence {}",
                    base.sequence
                )));
            };
            enforce_mutation_authorization(
                table_schema,
                AccessAction::Delete,
                &plan.principal,
                None,
                Some(&previous),
            )?;
            (WriteOpType::Delete, Some(previous), None)
        }
    };
    Ok(WriteOp {
        table: table.clone(),
        table_id: base.table_id.clone(),
        op_type,
        doc_id: document_id.clone(),
        resource_path_binding: base.resource_path_binding.clone(),
        // Trigger origin belongs to this logical write, not to the document
        // image it supersedes. Paths A/C originate as client writes.
        trigger_write_origin: None,
        previous,
        current,
    })
}
