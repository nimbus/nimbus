use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use nimbus_core::{Document, Mutation, ScheduledJob, TableSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{PpscBackend, PpscExpectedOutcome, PpscOperation, PpscScenario};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscFrontiers {
    pub assigned_high_water: u64,
    pub active_assigned_head: u64,
    pub durable_head: u64,
    pub storage_applied_head: u64,
    pub published_head: u64,
    pub applied_head: u64,
}

impl PpscFrontiers {
    fn is_ordered(self) -> bool {
        self.assigned_high_water >= self.active_assigned_head
            && self.active_assigned_head >= self.durable_head
            && self.durable_head >= self.storage_applied_head
            && self.storage_applied_head >= self.published_head
            && self.published_head >= self.applied_head
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscJournalEntry {
    pub sequence: u64,
    pub canonical_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscPublication {
    pub tenant: String,
    pub sequence: u64,
    pub identity: String,
    pub step: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PpscSequenceOwnership {
    DefinitiveRollback,
    Durable,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscSequenceClaim {
    pub tenant: String,
    pub sequence: u64,
    pub identity: String,
    pub ownership: PpscSequenceOwnership,
    pub step: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscEffect {
    pub tenant: String,
    pub sequence: Option<u64>,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscObservedStep {
    pub index: usize,
    pub outcome: PpscExpectedOutcome,
    pub effects: Vec<PpscEffect>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscTenantState {
    pub frontiers: PpscFrontiers,
    pub journal: Vec<PpscJournalEntry>,
    pub publications: Vec<PpscPublication>,
    pub documents: BTreeMap<String, Vec<u8>>,
    pub schema: Vec<u8>,
    pub scheduled_jobs: Vec<Vec<u8>>,
    pub trigger_cursor: u64,
    pub projection_durable_sequence: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscTerminalState {
    pub tenants: BTreeMap<String, PpscTenantState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscHistory {
    pub backend: PpscBackend,
    pub scenario: PpscScenario,
    pub observed_steps: Vec<PpscObservedStep>,
    pub sequence_claims: Vec<PpscSequenceClaim>,
    pub terminal: PpscTerminalState,
}

impl PpscHistory {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("PPSC history serialization should be infallible")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PpscAuditError {
    pub invariant: &'static str,
    pub backend: PpscBackend,
    pub seed: u64,
    pub step: Option<usize>,
    pub tenant: Option<String>,
    pub detail: String,
    pub replay_command: String,
}

impl fmt::Display for PpscAuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "PPSC invariant {} failed on backend {} seed {}",
            self.invariant,
            self.backend.as_str(),
            self.seed
        )?;
        if let Some(step) = self.step {
            write!(formatter, " step {step}")?;
        }
        if let Some(tenant) = &self.tenant {
            write!(formatter, " tenant {tenant}")?;
        }
        write!(
            formatter,
            ": {}; replay: {}",
            self.detail, self.replay_command
        )
    }
}

impl std::error::Error for PpscAuditError {}

fn failure(
    history: &PpscHistory,
    invariant: &'static str,
    step: Option<usize>,
    tenant: Option<&str>,
    detail: impl Into<String>,
) -> PpscAuditError {
    PpscAuditError {
        invariant,
        backend: history.backend,
        seed: history.scenario.seed,
        step,
        tenant: tenant.map(ToOwned::to_owned),
        detail: detail.into(),
        replay_command: history.scenario.replay_command(history.backend),
    }
}

pub fn audit_ppsc_history(history: &PpscHistory) -> Result<(), PpscAuditError> {
    history
        .scenario
        .validate_for_backend(history.backend)
        .map_err(|error| failure(history, "backend-capability", None, None, error.to_string()))?;
    // Diagnose the scenario-level blast-radius contract before the more
    // general effect/terminal-state correspondence checks. A missing durable
    // peer after another tenant overloads is specifically an isolation
    // failure, even though it also implies a missing committed effect.
    audit_tenant_isolation(history)?;
    audit_observed_steps(history)?;
    audit_sequence_claims(history)?;
    for (tenant, state) in &history.terminal.tenants {
        audit_tenant_state(history, tenant, state)?;
    }
    Ok(())
}

fn audit_observed_steps(history: &PpscHistory) -> Result<(), PpscAuditError> {
    if history.observed_steps.len() != history.scenario.steps.len() {
        return Err(failure(
            history,
            "history-completeness",
            None,
            None,
            format!(
                "scenario has {} steps but history records {}",
                history.scenario.steps.len(),
                history.observed_steps.len()
            ),
        ));
    }
    for (index, (declared, observed)) in history
        .scenario
        .steps
        .iter()
        .zip(&history.observed_steps)
        .enumerate()
    {
        if observed.index != index {
            return Err(failure(
                history,
                "history-step-order",
                Some(index),
                declared.operation.tenant(),
                format!("recorded step index is {}", observed.index),
            ));
        }
        if observed.outcome != declared.expected {
            return Err(failure(
                history,
                "expected-outcome",
                Some(index),
                declared.operation.tenant(),
                format!(
                    "expected {:?}, observed {:?}",
                    declared.expected, observed.outcome
                ),
            ));
        }
        if outcome_must_have_no_durable_effect(observed.outcome) && !observed.effects.is_empty() {
            return Err(failure(
                history,
                "rejected-step-has-effect",
                Some(index),
                declared.operation.tenant(),
                format!(
                    "{:?} recorded {} durable effect(s)",
                    observed.outcome,
                    observed.effects.len()
                ),
            ));
        }
        if outcome_requires_durable_effect(observed.outcome)
            && expected_effect_kind(&declared.operation).is_some()
            && observed.effects.is_empty()
        {
            return Err(failure(
                history,
                "committed-step-missing-durable-effect",
                Some(index),
                declared.operation.tenant(),
                format!(
                    "{:?} for {:?} recorded no durable effect",
                    observed.outcome, declared.operation
                ),
            ));
        }
        if observed.effects.iter().any(|effect| {
            declared
                .operation
                .tenant()
                .is_some_and(|tenant| effect.tenant != tenant)
        }) {
            return Err(failure(
                history,
                "cross-tenant-effect",
                Some(index),
                declared.operation.tenant(),
                "step recorded a durable effect for a different tenant",
            ));
        }
        if outcome_requires_durable_effect(observed.outcome)
            && let Some(expected_kind) = expected_effect_kind(&declared.operation)
            && !observed
                .effects
                .iter()
                .any(|effect| effect.kind.split('+').any(|kind| kind == expected_kind))
        {
            return Err(failure(
                history,
                "operation-effect-kind",
                Some(index),
                declared.operation.tenant(),
                format!(
                    "{:?} recorded no {expected_kind} effect: {:?}",
                    declared.operation, observed.effects
                ),
            ));
        }
        for effect in &observed.effects {
            let Some(state) = history.terminal.tenants.get(&effect.tenant) else {
                return Err(failure(
                    history,
                    "effect-terminal-coverage",
                    Some(index),
                    Some(&effect.tenant),
                    "durable effect tenant is missing from terminal state",
                ));
            };
            if let Some(sequence) = effect.sequence {
                if state.journal.iter().any(|entry| entry.sequence == sequence) {
                    continue;
                }
                if has_later_restore(history, index, &effect.tenant) {
                    continue;
                }
                return Err(failure(
                    history,
                    "effect-terminal-coverage",
                    Some(index),
                    Some(&effect.tenant),
                    format!("durable effect sequence {sequence} is absent from terminal journal"),
                ));
            }
            if !allows_non_journal_effect(&declared.operation, &effect.kind) {
                return Err(failure(
                    history,
                    "effect-sequence",
                    Some(index),
                    Some(&effect.tenant),
                    format!(
                        "effect {:?} omitted its journal sequence without an operation-owned durable-state contract",
                        effect.kind
                    ),
                ));
            }
        }
        audit_operation_terminal_state(history, index, &declared.operation, observed.outcome)?;
    }
    Ok(())
}

fn outcome_requires_durable_effect(outcome: PpscExpectedOutcome) -> bool {
    matches!(
        outcome,
        PpscExpectedOutcome::Committed | PpscExpectedOutcome::AmbiguousRecovered
    )
}

fn expected_effect_kind(operation: &PpscOperation) -> Option<&'static str> {
    match operation {
        PpscOperation::Mutation { .. }
        | PpscOperation::CommitPermutation { .. }
        | PpscOperation::ConflictRetry { .. }
        | PpscOperation::CommitPhaseFault { .. }
        | PpscOperation::PublicationPredecessorRace { .. }
        | PpscOperation::ProviderTakeover { .. } => Some("document-write"),
        PpscOperation::SchemaSet { .. }
        | PpscOperation::SchemaDelete { .. }
        | PpscOperation::ProjectionUpdate { .. } => Some("schema-change"),
        PpscOperation::TriggerCursorAdvance { .. } => Some("trigger-cursor-advance"),
        PpscOperation::Schedule { .. } => Some("scheduled-job-insert"),
        PpscOperation::RestoreImport { .. } => Some("restore-import"),
        PpscOperation::ZeroWriteExecutionUnit { .. }
        | PpscOperation::ArmFault { .. }
        | PpscOperation::ReleaseFault { .. }
        | PpscOperation::CancelNext { .. }
        | PpscOperation::ForceOverload { .. }
        | PpscOperation::AdvanceWallClock { .. }
        | PpscOperation::AdvanceMonotonicClock { .. }
        | PpscOperation::ExpireProviderLease { .. }
        | PpscOperation::AttemptStaleProviderWrite { .. }
        | PpscOperation::SettledRestart
        | PpscOperation::Reopen
        | PpscOperation::Quiesce => None,
    }
}

fn allows_non_journal_effect(operation: &PpscOperation, kind: &str) -> bool {
    matches!(
        (operation, kind),
        (PpscOperation::Schedule { .. }, "scheduled-job-insert")
            | (
                PpscOperation::TriggerCursorAdvance { .. },
                "trigger-cursor-advance"
            )
            | (PpscOperation::RestoreImport { .. }, "restore-import")
    )
}

fn audit_operation_terminal_state(
    history: &PpscHistory,
    index: usize,
    operation: &PpscOperation,
    outcome: PpscExpectedOutcome,
) -> Result<(), PpscAuditError> {
    if !outcome_requires_durable_effect(outcome) {
        return Ok(());
    }
    match operation {
        PpscOperation::Mutation {
            tenant, key, value, ..
        } => audit_expected_document(
            history,
            index,
            tenant,
            BTreeMap::from([
                ("key".to_string(), Value::String(key.clone())),
                ("value".to_string(), Value::from(*value)),
            ]),
        ),
        PpscOperation::CommitPermutation {
            tenant, value_base, ..
        } => {
            for (key, offset) in [
                ("permutation-queued", 1_i64),
                ("permutation-direct", 2),
                ("permutation-execution-unit", 3),
            ] {
                audit_expected_document(
                    history,
                    index,
                    tenant,
                    BTreeMap::from([
                        ("key".to_string(), Value::String(key.to_string())),
                        (
                            "value".to_string(),
                            Value::from(value_base.saturating_add(offset)),
                        ),
                    ]),
                )?;
            }
            Ok(())
        }
        PpscOperation::ConflictRetry {
            tenant,
            key,
            second,
            ..
        } => audit_expected_document(
            history,
            index,
            tenant,
            BTreeMap::from([
                ("key".to_string(), Value::String(key.clone())),
                ("value".to_string(), Value::from(*second)),
            ]),
        ),
        PpscOperation::CommitPhaseFault { tenant, fault, .. } => {
            let (key, value) = match fault {
                super::PpscInjectedFault::DurableBeforePublish => {
                    ("durable-before-publish", 401_i64)
                }
                super::PpscInjectedFault::PanicAfterDurable => ("panic-after-durable", 421),
                _ => return Ok(()),
            };
            audit_expected_document(
                history,
                index,
                tenant,
                BTreeMap::from([
                    ("key".to_string(), Value::String(key.to_string())),
                    ("value".to_string(), Value::from(value)),
                ]),
            )
        }
        PpscOperation::PublicationPredecessorRace { tenant, .. } => {
            for (key, value) in [("held-predecessor", 411_i64), ("blocked-successor", 412)] {
                audit_expected_document(
                    history,
                    index,
                    tenant,
                    BTreeMap::from([
                        ("key".to_string(), Value::String(key.to_string())),
                        ("value".to_string(), Value::from(value)),
                    ]),
                )?;
            }
            Ok(())
        }
        PpscOperation::ProviderTakeover { tenant } => audit_expected_document(
            history,
            index,
            tenant,
            BTreeMap::from([
                (
                    "key".to_string(),
                    Value::String("provider-takeover".to_string()),
                ),
                ("value".to_string(), Value::from(history.scenario.seed)),
            ]),
        ),
        PpscOperation::Schedule { tenant, job } => {
            if has_later_restore(history, index, tenant) {
                return Ok(());
            }
            let state = operation_tenant_state(history, index, tenant)?;
            let found = state.scheduled_jobs.iter().any(|bytes| {
                serde_json::from_slice::<ScheduledJob>(bytes)
                    .ok()
                    .is_some_and(|scheduled| match scheduled.mutation {
                        Mutation::Insert { fields, .. } => {
                            fields.get("key") == Some(&Value::String(format!("scheduled-{job}")))
                                && fields.get("value") == Some(&Value::from(*job))
                        }
                        Mutation::Update { .. } | Mutation::Delete { .. } => false,
                    })
            });
            if found {
                Ok(())
            } else {
                Err(failure(
                    history,
                    "operation-terminal-state",
                    Some(index),
                    Some(tenant),
                    format!("scheduled job {job} is absent from terminal scheduler state"),
                ))
            }
        }
        PpscOperation::TriggerCursorAdvance { tenant, through } => {
            if has_later_restore(history, index, tenant) {
                return Ok(());
            }
            let state = operation_tenant_state(history, index, tenant)?;
            if state.trigger_cursor >= *through {
                Ok(())
            } else {
                Err(failure(
                    history,
                    "operation-terminal-state",
                    Some(index),
                    Some(tenant),
                    format!(
                        "trigger cursor {} did not retain committed advance through {through}",
                        state.trigger_cursor
                    ),
                ))
            }
        }
        PpscOperation::RestoreImport { tenant, archive } => {
            if has_later_restore(history, index, tenant) {
                return Ok(());
            }
            audit_expected_document(
                history,
                index,
                tenant,
                BTreeMap::from([
                    (
                        "key".to_string(),
                        Value::String(format!("archive-{archive}")),
                    ),
                    ("archive".to_string(), Value::from(*archive)),
                ]),
            )
        }
        PpscOperation::SchemaSet { tenant, revision } => {
            if has_later_tasks_schema_change(history, index, tenant) {
                return Ok(());
            }
            audit_expected_schema(
                history,
                index,
                tenant,
                "tasks",
                Some(format!("revision_{revision:016x}")),
            )
        }
        PpscOperation::SchemaDelete { tenant } => {
            if has_later_tasks_schema_change(history, index, tenant) {
                return Ok(());
            }
            audit_expected_schema(history, index, tenant, "tasks", None)
        }
        PpscOperation::ProjectionUpdate { tenant, revision } => {
            if has_later_projection_change(history, index, tenant) {
                return Ok(());
            }
            audit_expected_schema(
                history,
                index,
                tenant,
                "ppsc_projection",
                Some(format!("revision_{revision:016x}")),
            )
        }
        PpscOperation::ZeroWriteExecutionUnit { .. }
        | PpscOperation::ArmFault { .. }
        | PpscOperation::ReleaseFault { .. }
        | PpscOperation::CancelNext { .. }
        | PpscOperation::ForceOverload { .. }
        | PpscOperation::AdvanceWallClock { .. }
        | PpscOperation::AdvanceMonotonicClock { .. }
        | PpscOperation::ExpireProviderLease { .. }
        | PpscOperation::AttemptStaleProviderWrite { .. }
        | PpscOperation::SettledRestart
        | PpscOperation::Reopen
        | PpscOperation::Quiesce => Ok(()),
    }
}

fn operation_tenant_state<'a>(
    history: &'a PpscHistory,
    index: usize,
    tenant: &str,
) -> Result<&'a PpscTenantState, PpscAuditError> {
    history.terminal.tenants.get(tenant).ok_or_else(|| {
        failure(
            history,
            "operation-terminal-state",
            Some(index),
            Some(tenant),
            "committed operation tenant is missing from terminal state",
        )
    })
}

fn audit_expected_document(
    history: &PpscHistory,
    index: usize,
    tenant: &str,
    expected_fields: BTreeMap<String, Value>,
) -> Result<(), PpscAuditError> {
    if has_later_restore(history, index, tenant) {
        return Ok(());
    }
    let state = operation_tenant_state(history, index, tenant)?;
    let found = state.documents.values().any(|bytes| {
        serde_json::from_slice::<Document>(bytes)
            .ok()
            .is_some_and(|document| {
                expected_fields
                    .iter()
                    .all(|(name, value)| document.fields.get(name) == Some(value))
            })
    });
    if found {
        Ok(())
    } else {
        Err(failure(
            history,
            "operation-terminal-state",
            Some(index),
            Some(tenant),
            format!("no terminal document retains fields {expected_fields:?}"),
        ))
    }
}

fn audit_expected_schema(
    history: &PpscHistory,
    index: usize,
    tenant: &str,
    table: &str,
    expected_field: Option<String>,
) -> Result<(), PpscAuditError> {
    let state = operation_tenant_state(history, index, tenant)?;
    let schemas = serde_json::from_slice::<Vec<TableSchema>>(&state.schema).map_err(|error| {
        failure(
            history,
            "operation-terminal-state",
            Some(index),
            Some(tenant),
            format!("terminal schema did not deserialize: {error}"),
        )
    })?;
    let schema = schemas.iter().find(|schema| schema.table.as_str() == table);
    match expected_field {
        Some(field)
            if schema.is_some_and(|schema| {
                schema
                    .fields
                    .iter()
                    .any(|candidate| candidate.name == field)
            }) =>
        {
            Ok(())
        }
        None if schema.is_none() => Ok(()),
        expected => Err(failure(
            history,
            "operation-terminal-state",
            Some(index),
            Some(tenant),
            format!("table {table:?} terminal schema did not match field {expected:?}"),
        )),
    }
}

fn has_later_restore(history: &PpscHistory, index: usize, tenant: &str) -> bool {
    history.scenario.steps[index.saturating_add(1)..]
        .iter()
        .any(|step| {
            matches!(
                &step.operation,
                PpscOperation::RestoreImport {
                    tenant: candidate,
                    ..
                } if candidate == tenant
            ) && outcome_requires_durable_effect(step.expected)
        })
}

fn has_later_tasks_schema_change(history: &PpscHistory, index: usize, tenant: &str) -> bool {
    history.scenario.steps[index.saturating_add(1)..]
        .iter()
        .any(|step| {
            matches!(
                &step.operation,
                PpscOperation::SchemaSet {
                    tenant: candidate,
                    ..
                } | PpscOperation::SchemaDelete { tenant: candidate }
                    | PpscOperation::RestoreImport {
                        tenant: candidate,
                        ..
                    } if candidate == tenant
            ) && outcome_requires_durable_effect(step.expected)
        })
}

fn has_later_projection_change(history: &PpscHistory, index: usize, tenant: &str) -> bool {
    history.scenario.steps[index.saturating_add(1)..]
        .iter()
        .any(|step| {
            matches!(
                &step.operation,
                PpscOperation::ProjectionUpdate {
                    tenant: candidate,
                    ..
                } | PpscOperation::RestoreImport {
                    tenant: candidate,
                    ..
                } if candidate == tenant
            ) && outcome_requires_durable_effect(step.expected)
        })
}

fn outcome_must_have_no_durable_effect(outcome: PpscExpectedOutcome) -> bool {
    matches!(
        outcome,
        PpscExpectedOutcome::DefinitiveRollback
            | PpscExpectedOutcome::Cancelled
            | PpscExpectedOutcome::Overloaded
            | PpscExpectedOutcome::Fenced
            | PpscExpectedOutcome::ProviderError
            | PpscExpectedOutcome::Shutdown
    )
}

fn audit_sequence_claims(history: &PpscHistory) -> Result<(), PpscAuditError> {
    let mut claims: BTreeMap<(&str, u64), (&str, PpscSequenceOwnership, usize)> = BTreeMap::new();
    for claim in &history.sequence_claims {
        let key = (claim.tenant.as_str(), claim.sequence);
        if let Some((identity, ownership, first_step)) = claims.get(&key).copied() {
            if identity != claim.identity
                && !matches!(ownership, PpscSequenceOwnership::DefinitiveRollback)
            {
                return Err(failure(
                    history,
                    "durable-sequence-identity-reuse",
                    Some(claim.step),
                    Some(&claim.tenant),
                    format!(
                        "sequence {} was first claimed as {identity:?} with {ownership:?} at step {first_step}, then reused as {:?}",
                        claim.sequence, claim.identity
                    ),
                ));
            }
            if identity == claim.identity
                && matches!(
                    ownership,
                    PpscSequenceOwnership::Durable | PpscSequenceOwnership::Ambiguous
                )
                && matches!(claim.ownership, PpscSequenceOwnership::DefinitiveRollback)
            {
                return Err(failure(
                    history,
                    "owned-sequence-downgrade",
                    Some(claim.step),
                    Some(&claim.tenant),
                    format!(
                        "sequence {} {ownership:?} ownership cannot become a definitive rollback",
                        claim.sequence,
                    ),
                ));
            }
            if matches!(ownership, PpscSequenceOwnership::DefinitiveRollback) {
                claims.insert(key, (&claim.identity, claim.ownership, claim.step));
            }
        } else {
            claims.insert(key, (&claim.identity, claim.ownership, claim.step));
        }
    }
    Ok(())
}

fn audit_tenant_state(
    history: &PpscHistory,
    tenant: &str,
    state: &PpscTenantState,
) -> Result<(), PpscAuditError> {
    if !state.frontiers.is_ordered() {
        return Err(failure(
            history,
            "frontier-order",
            None,
            Some(tenant),
            format!("frontiers are {:?}", state.frontiers),
        ));
    }
    audit_journal(history, tenant, state)?;
    audit_publication(history, tenant, state)
}

fn audit_journal(
    history: &PpscHistory,
    tenant: &str,
    state: &PpscTenantState,
) -> Result<(), PpscAuditError> {
    if state.journal.len() != usize::try_from(state.frontiers.durable_head).unwrap_or(usize::MAX) {
        return Err(failure(
            history,
            "durable-prefix-length",
            None,
            Some(tenant),
            format!(
                "durable head is {} but journal contains {} records",
                state.frontiers.durable_head,
                state.journal.len()
            ),
        ));
    }
    for (index, entry) in state.journal.iter().enumerate() {
        let expected = u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1);
        if entry.sequence != expected {
            return Err(failure(
                history,
                "durable-prefix-contiguity",
                None,
                Some(tenant),
                format!(
                    "record index {index} expected sequence {expected}, got {}",
                    entry.sequence
                ),
            ));
        }
        if entry.canonical_bytes.is_empty() {
            return Err(failure(
                history,
                "canonical-journal-encoding",
                None,
                Some(tenant),
                format!("sequence {} has an empty canonical record", entry.sequence),
            ));
        }
    }
    Ok(())
}

fn audit_publication(
    history: &PpscHistory,
    tenant: &str,
    state: &PpscTenantState,
) -> Result<(), PpscAuditError> {
    let expected_len = usize::try_from(state.frontiers.published_head).unwrap_or(usize::MAX);
    if state.publications.len() != expected_len {
        return Err(failure(
            history,
            "published-prefix-length",
            None,
            Some(tenant),
            format!(
                "published head is {} but {} publications were observed",
                state.frontiers.published_head,
                state.publications.len()
            ),
        ));
    }
    let mut identities = BTreeSet::new();
    for (index, publication) in state.publications.iter().enumerate() {
        let expected = u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1);
        if publication.tenant != tenant {
            return Err(failure(
                history,
                "publication-tenant",
                Some(publication.step),
                Some(tenant),
                format!("publication belongs to {}", publication.tenant),
            ));
        }
        if publication.sequence != expected {
            return Err(failure(
                history,
                "publication-leapfrog",
                Some(publication.step),
                Some(tenant),
                format!(
                    "publication index {index} expected sequence {expected}, got {}",
                    publication.sequence
                ),
            ));
        }
        if !identities.insert((publication.sequence, publication.identity.as_str())) {
            return Err(failure(
                history,
                "double-publication",
                Some(publication.step),
                Some(tenant),
                format!(
                    "sequence {} identity {:?} was published more than once",
                    publication.sequence, publication.identity
                ),
            ));
        }
    }
    Ok(())
}

fn audit_tenant_isolation(history: &PpscHistory) -> Result<(), PpscAuditError> {
    let overloaded_tenants = history
        .scenario
        .steps
        .iter()
        .filter_map(|step| match &step.operation {
            PpscOperation::ForceOverload { tenant } => Some(tenant.as_str()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    if overloaded_tenants.is_empty() {
        return Ok(());
    }
    let peer_tenants = history
        .scenario
        .steps
        .iter()
        .filter_map(|step| {
            (step.expected == PpscExpectedOutcome::Committed)
                .then(|| step.operation.tenant())
                .flatten()
        })
        .filter(|tenant| !overloaded_tenants.contains(tenant))
        .collect::<BTreeSet<_>>();
    for peer in peer_tenants {
        let Some(state) = history.terminal.tenants.get(peer) else {
            return Err(failure(
                history,
                "tenant-isolation",
                None,
                Some(peer),
                "committed peer tenant is missing from terminal state",
            ));
        };
        if state.frontiers.durable_head == 0 {
            return Err(failure(
                history,
                "tenant-isolation",
                None,
                Some(peer),
                "peer tenant made no durable progress while another tenant overloaded",
            ));
        }
    }
    Ok(())
}
