use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

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
    pub projection_revision: u64,
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
    audit_observed_steps(history)?;
    audit_sequence_claims(history)?;
    for (tenant, state) in &history.terminal.tenants {
        audit_tenant_state(history, tenant, state)?;
    }
    audit_tenant_isolation(history)?;
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
    }
    Ok(())
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
                && matches!(ownership, PpscSequenceOwnership::Ambiguous)
                && matches!(claim.ownership, PpscSequenceOwnership::DefinitiveRollback)
            {
                return Err(failure(
                    history,
                    "ambiguous-ownership-downgrade",
                    Some(claim.step),
                    Some(&claim.tenant),
                    format!(
                        "sequence {} ambiguous ownership cannot become a definitive rollback",
                        claim.sequence
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
