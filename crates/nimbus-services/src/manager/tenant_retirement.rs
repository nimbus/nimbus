//! Tenant-wide source fencing and effect-free terminal finalization.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;

use nimbus_core::{Error, TenantId};
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceKind,
    WorkloadProvisionSourceResourceVersion, WorkloadSagaPhase, WorkloadSagaRecord,
};

use crate::{ServiceBackend, ServiceDefinition};

use super::ServiceManager;
use super::types::{ServiceManagerState, WorkloadSourceRetirementKey};

/// Exact process-local fence for one Engine tenant incarnation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantSourceRetirementClaim {
    tenant_id: TenantId,
    tenant_incarnation: NonZeroU64,
}

impl TenantSourceRetirementClaim {
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub const fn tenant_incarnation(&self) -> NonZeroU64 {
        self.tenant_incarnation
    }
}

/// Immutable source-owner facts captured when tenant retirement wins its
/// process-local source barrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantWorkloadSourceSnapshot {
    identity: WorkloadProvisionSourceIdentity,
    source_generation: WorkloadProvisionSourceGeneration,
    resource_version: WorkloadProvisionSourceResourceVersion,
    has_observation: bool,
}

impl TenantWorkloadSourceSnapshot {
    pub fn identity(&self) -> &WorkloadProvisionSourceIdentity {
        &self.identity
    }

    pub const fn source_generation(&self) -> WorkloadProvisionSourceGeneration {
        self.source_generation
    }

    pub fn resource_version(&self) -> &WorkloadProvisionSourceResourceVersion {
        &self.resource_version
    }

    pub const fn has_observation(&self) -> bool {
        self.has_observation
    }
}

/// Exact claim plus the frozen source inventory that compute must retire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantSourceRetirementSnapshot {
    claim: TenantSourceRetirementClaim,
    sources: Vec<TenantWorkloadSourceSnapshot>,
}

/// Installed tenant fence. A failed snapshot remains fenced so retry cannot
/// race new admission into an incarnation whose retirement already started.
#[derive(Debug)]
pub(super) enum TenantSourceRetirementBarrier {
    Claimed(TenantSourceRetirementSnapshot),
    Finalized(TenantSourceRetirementSnapshot),
    Failed {
        claim: TenantSourceRetirementClaim,
        reason: String,
    },
}

impl TenantSourceRetirementBarrier {
    fn claim(&self) -> &TenantSourceRetirementClaim {
        match self {
            Self::Claimed(snapshot) | Self::Finalized(snapshot) => snapshot.claim(),
            Self::Failed { claim, .. } => claim,
        }
    }

    fn replay(&self) -> Result<TenantSourceRetirementSnapshot, Error> {
        match self {
            Self::Claimed(snapshot) | Self::Finalized(snapshot) => Ok(snapshot.clone()),
            Self::Failed { claim, reason } => Err(Error::PreconditionFailed(format!(
                "tenant {} source-retirement barrier retained its snapshot failure: {reason}",
                claim.tenant_id()
            ))),
        }
    }
}

impl TenantSourceRetirementSnapshot {
    pub fn claim(&self) -> &TenantSourceRetirementClaim {
        &self.claim
    }

    pub fn sources(&self) -> &[TenantWorkloadSourceSnapshot] {
        &self.sources
    }
}

impl ServiceManager {
    /// Fence all new source and session admission for one exact Engine tenant
    /// incarnation and return an immutable inventory for compute-owned teardown.
    pub fn claim_tenant_source_retirement(
        &self,
        tenant_id: &TenantId,
        tenant_incarnation: NonZeroU64,
    ) -> Result<TenantSourceRetirementSnapshot, Error> {
        let catalog_definitions = self
            .service_definitions
            .service_definitions_for_tenant(tenant_id);
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");

        if let Some(existing) = state.tenant_source_retirements.get(tenant_id) {
            if existing.claim().tenant_incarnation == tenant_incarnation {
                return existing.replay();
            }
            return Err(Error::conflict(format!(
                "tenant {tenant_id} source retirement belongs to Engine incarnation {}, not {tenant_incarnation}",
                existing.claim().tenant_incarnation
            )));
        }

        let claim = TenantSourceRetirementClaim {
            tenant_id: tenant_id.clone(),
            tenant_incarnation,
        };
        match tenant_source_snapshot(&state, tenant_id, catalog_definitions) {
            Ok(sources) => {
                let snapshot = TenantSourceRetirementSnapshot { claim, sources };
                state.tenant_source_retirements.insert(
                    tenant_id.clone(),
                    TenantSourceRetirementBarrier::Claimed(snapshot.clone()),
                );
                Ok(snapshot)
            }
            Err(error) => {
                state.tenant_source_retirements.insert(
                    tenant_id.clone(),
                    TenantSourceRetirementBarrier::Failed {
                        claim,
                        reason: error.to_string(),
                    },
                );
                Err(error)
            }
        }
    }

    /// Remove process-local tenant source state only after compute supplies the
    /// complete, exact, terminal durable saga inventory. This method performs
    /// no provider inspection, stop, cleanup, or other effect.
    pub fn finalize_tenant_sources_after_recorded(
        &self,
        claim: &TenantSourceRetirementClaim,
        records: &[WorkloadSagaRecord],
    ) -> Result<(), Error> {
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let barrier = state
            .tenant_source_retirements
            .get(claim.tenant_id())
            .ok_or_else(|| {
                Error::PreconditionFailed(format!(
                    "tenant {} has no source-retirement barrier",
                    claim.tenant_id()
                ))
            })?;
        if barrier.claim() != claim {
            return Err(Error::PreconditionFailed(format!(
                "tenant {} source-retirement claim is stale or crossed",
                claim.tenant_id()
            )));
        }
        let retained = match barrier {
            TenantSourceRetirementBarrier::Claimed(snapshot)
            | TenantSourceRetirementBarrier::Finalized(snapshot) => snapshot.clone(),
            TenantSourceRetirementBarrier::Failed { reason, .. } => {
                return Err(Error::PreconditionFailed(format!(
                    "tenant {} source-retirement barrier retained its snapshot failure: {reason}",
                    claim.tenant_id()
                )));
            }
        };

        authenticate_terminal_inventory(&retained, records)?;
        authenticate_current_sources(&state, &retained)?;

        let tenant_id = claim.tenant_id();
        state
            .service_definition_observations
            .retain(|key, _| &key.tenant_id != tenant_id);
        state
            .definitions
            .retain(|key, _| &key.tenant_id != tenant_id);
        state
            .sandbox_resource_sources
            .retain(|key, _| &key.tenant_id != tenant_id);
        state
            .sandbox_resource_observations
            .retain(|key, _| &key.tenant_id != tenant_id);
        state
            .sessions
            .retain(|_, session| &session.tenant_id != tenant_id);
        state
            .session_channels
            .retain(|_, channel| &channel.tenant_id != tenant_id);
        state
            .source_retirement_claims
            .retain(|key, _| source_retirement_tenant(key) != tenant_id);
        state
            .service_resolution_withdrawals
            .retain(|key, _| &key.tenant_id != tenant_id);
        state.tenant_source_retirements.insert(
            tenant_id.clone(),
            TenantSourceRetirementBarrier::Finalized(retained),
        );
        Ok(())
    }

    /// Release the process-local source fence after the Engine has durably
    /// finished deleting this exact tenant incarnation.
    pub fn release_tenant_source_retirement(
        &self,
        claim: &TenantSourceRetirementClaim,
    ) -> Result<(), Error> {
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let retained = state
            .tenant_source_retirements
            .get(claim.tenant_id())
            .ok_or_else(|| {
                Error::PreconditionFailed(format!(
                    "tenant {} has no source-retirement barrier to release",
                    claim.tenant_id()
                ))
            })?;
        if retained.claim() != claim {
            return Err(Error::PreconditionFailed(format!(
                "tenant {} source-retirement release is stale or crossed",
                claim.tenant_id()
            )));
        }
        if !matches!(retained, TenantSourceRetirementBarrier::Finalized(_)) {
            return Err(Error::PreconditionFailed(format!(
                "tenant {} source-retirement state is not finalized",
                claim.tenant_id()
            )));
        }
        state.tenant_source_retirements.remove(claim.tenant_id());
        Ok(())
    }

    pub(super) fn ensure_tenant_source_admission_open(
        state: &ServiceManagerState,
        tenant_id: &TenantId,
        operation: &str,
    ) -> Result<(), Error> {
        if state.tenant_source_retirements.contains_key(tenant_id) {
            return Err(Error::conflict(format!(
                "tenant {tenant_id} source retirement is in progress; {operation} is closed"
            )));
        }
        Ok(())
    }
}

fn tenant_source_snapshot(
    state: &ServiceManagerState,
    tenant_id: &TenantId,
    catalog_definitions: BTreeMap<String, ServiceDefinition>,
) -> Result<Vec<TenantWorkloadSourceSnapshot>, Error> {
    let mut definitions = catalog_definitions;
    for (key, definition) in &state.definitions {
        if &key.tenant_id != tenant_id {
            continue;
        }
        if definition.tenant_id != key.tenant_id || definition.name != key.service_name {
            return Err(Error::Internal(format!(
                "tenant {tenant_id} service source store contains crossed source identity"
            )));
        }
        definitions.insert(key.service_name.clone(), definition.clone());
    }
    let mut sources = Vec::new();
    for (source_name, definition) in definitions {
        if &definition.tenant_id != tenant_id || definition.name != source_name {
            return Err(Error::Internal(format!(
                "tenant {tenant_id} service catalog returned crossed source identity"
            )));
        }
        if !matches!(definition.backend, ServiceBackend::Sandbox(_)) {
            continue;
        }
        let identity = WorkloadProvisionSourceIdentity::sandbox_backed_service(&definition.name)
            .map_err(|error| Error::Internal(error.to_string()))?;
        sources.push(TenantWorkloadSourceSnapshot {
            identity,
            source_generation: WorkloadProvisionSourceGeneration::new(definition.generation),
            resource_version: WorkloadProvisionSourceResourceVersion::new(
                definition.resource_version,
            )
            .map_err(|error| Error::Internal(error.to_string()))?,
            has_observation: state
                .service_definition_observations
                .keys()
                .any(|key| key.tenant_id == *tenant_id && key.service_name == definition.name),
        });
    }
    for (key, source) in &state.sandbox_resource_sources {
        if &key.tenant_id != tenant_id {
            continue;
        }
        if source.tenant_id != key.tenant_id || source.id != key.resource_id {
            return Err(Error::Internal(format!(
                "tenant {tenant_id} sandbox source store contains crossed source identity"
            )));
        }
        let identity =
            WorkloadProvisionSourceIdentity::standalone_sandbox(&source.id, &source.profile)
                .map_err(|error| Error::Internal(error.to_string()))?;
        sources.push(TenantWorkloadSourceSnapshot {
            identity,
            source_generation: WorkloadProvisionSourceGeneration::new(source.generation),
            resource_version: WorkloadProvisionSourceResourceVersion::new(
                source.resource_version.clone(),
            )
            .map_err(|error| Error::Internal(error.to_string()))?,
            has_observation: state.sandbox_resource_observations.contains_key(key),
        });
    }
    sources.sort_by_key(|source| source_identity_key(source.identity()));
    let mut seen_identities = BTreeSet::new();
    let mut seen_workload_names = BTreeSet::new();
    if sources.iter().any(|source| {
        !seen_identities.insert(source_identity_key(source.identity()))
            || !seen_workload_names.insert(source.identity().stable_name().to_owned())
    }) {
        return Err(Error::Internal(format!(
            "tenant {tenant_id} has duplicate workload source identity"
        )));
    }
    Ok(sources)
}

fn authenticate_terminal_inventory(
    retained: &TenantSourceRetirementSnapshot,
    records: &[WorkloadSagaRecord],
) -> Result<(), Error> {
    let expected = retained
        .sources
        .iter()
        .map(|source| (source_identity_key(source.identity()), source))
        .collect::<BTreeMap<_, _>>();
    let mut observed = BTreeSet::new();
    for record in records {
        if record.key().tenant_id() != retained.claim.tenant_id() {
            return Err(terminal_inventory_error(
                retained.claim.tenant_id(),
                "contains a crossed-tenant record",
            ));
        }
        if record.phase() != WorkloadSagaPhase::Recorded
            || record.active_intent().desired_state() != DesiredWorkloadState::Stopped
            || record.successor_intent().is_some()
        {
            return Err(terminal_inventory_error(
                retained.claim.tenant_id(),
                "contains a non-terminal or non-stopped record",
            ));
        }
        let source = record.active_intent().source();
        let key = source_identity_key(source.source_identity());
        let Some(expected_source) = expected.get(&key) else {
            return Err(terminal_inventory_error(
                retained.claim.tenant_id(),
                "contains a record outside the frozen source snapshot",
            ));
        };
        let expected_kind = match expected_source.identity().kind() {
            WorkloadProvisionSourceKind::StandaloneSandbox => DesiredWorkloadKind::Sandbox,
            WorkloadProvisionSourceKind::SandboxBackedService => DesiredWorkloadKind::Service,
        };
        if record.key().workload_id().as_str() != source.source_identity().stable_name()
            || record.active_intent().kind() != expected_kind
            || source.source_generation() != expected_source.source_generation
            || source.resource_version() != &expected_source.resource_version
        {
            return Err(terminal_inventory_error(
                retained.claim.tenant_id(),
                "contains crossed source identity, generation, or resource version",
            ));
        }
        if !observed.insert(key) {
            return Err(terminal_inventory_error(
                retained.claim.tenant_id(),
                "contains duplicate source evidence",
            ));
        }
    }
    if retained.sources.iter().any(|source| {
        source.has_observation && !observed.contains(&source_identity_key(source.identity()))
    }) {
        return Err(terminal_inventory_error(
            retained.claim.tenant_id(),
            "does not cover every observed source",
        ));
    }
    Ok(())
}

fn authenticate_current_sources(
    state: &ServiceManagerState,
    retained: &TenantSourceRetirementSnapshot,
) -> Result<(), Error> {
    let expected = retained
        .sources
        .iter()
        .map(|source| (source_identity_key(source.identity()), source))
        .collect::<BTreeMap<_, _>>();
    for definition in state
        .definitions
        .values()
        .filter(|definition| &definition.tenant_id == retained.claim.tenant_id())
    {
        if !matches!(definition.backend, ServiceBackend::Sandbox(_)) {
            continue;
        }
        let key = source_identity_key(
            &WorkloadProvisionSourceIdentity::sandbox_backed_service(&definition.name)
                .map_err(|error| Error::Internal(error.to_string()))?,
        );
        let Some(source) = expected.get(&key) else {
            return Err(terminal_inventory_error(
                retained.claim.tenant_id(),
                "current services state contains a source created after the barrier",
            ));
        };
        if source.source_generation.as_u64() != definition.generation
            || source.resource_version.as_str() != definition.resource_version
        {
            return Err(terminal_inventory_error(
                retained.claim.tenant_id(),
                "current service source changed after the barrier",
            ));
        }
    }
    for (key, sandbox) in &state.sandbox_resource_sources {
        if &key.tenant_id != retained.claim.tenant_id() {
            continue;
        }
        let identity =
            WorkloadProvisionSourceIdentity::standalone_sandbox(&sandbox.id, &sandbox.profile)
                .map_err(|error| Error::Internal(error.to_string()))?;
        let Some(source) = expected.get(&source_identity_key(&identity)) else {
            return Err(terminal_inventory_error(
                retained.claim.tenant_id(),
                "current services state contains a sandbox created after the barrier",
            ));
        };
        if source.source_generation.as_u64() != sandbox.generation
            || source.resource_version.as_str() != sandbox.resource_version
        {
            return Err(terminal_inventory_error(
                retained.claim.tenant_id(),
                "current sandbox source changed after the barrier",
            ));
        }
    }
    Ok(())
}

fn source_identity_key(identity: &WorkloadProvisionSourceIdentity) -> (u8, String, Option<String>) {
    let kind = match identity.kind() {
        WorkloadProvisionSourceKind::StandaloneSandbox => 0,
        WorkloadProvisionSourceKind::SandboxBackedService => 1,
    };
    (
        kind,
        identity.stable_name().to_owned(),
        identity.profile().map(str::to_owned),
    )
}

fn source_retirement_tenant(key: &WorkloadSourceRetirementKey) -> &TenantId {
    match key {
        WorkloadSourceRetirementKey::Service(key) => &key.tenant_id,
        WorkloadSourceRetirementKey::Sandbox(key) => &key.tenant_id,
    }
}

fn terminal_inventory_error(tenant_id: &TenantId, message: &str) -> Error {
    Error::PreconditionFailed(format!(
        "tenant {tenant_id} terminal workload inventory {message}"
    ))
}

#[cfg(test)]
#[path = "tenant_retirement/tests.rs"]
mod tests;
