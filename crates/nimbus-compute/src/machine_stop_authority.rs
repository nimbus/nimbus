//! Compute-owned policy for fencing physical-machine stop against workloads.
//!
//! Provider adapters own the durable machine barrier and their exact effect
//! witnesses. The server adapter owns Engine reads. This module joins both
//! evidence sets and is the only owner of the stop decision.

use std::future::Future;
use std::pin::Pin;

use nimbus_machine::MachineForwarderAuthority;
use nimbus_workloads::{
    DesiredWorkloadState, WorkloadDesiredDigest, WorkloadExecutionProviderId, WorkloadGeneration,
    WorkloadProvisionSourceDigest, WorkloadSagaIntent, WorkloadSagaKey, WorkloadSagaPhase,
    WorkloadSagaRecord,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Provider-owned monotonic epoch for one machine stop barrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MachineStopBarrierEpoch(u64);

impl MachineStopBarrierEpoch {
    pub fn new(value: u64) -> Result<Self, MachineStopAuthorityEvidenceError> {
        if value == 0 {
            Err(MachineStopAuthorityEvidenceError::InvalidBarrierEpoch)
        } else {
            Ok(Self(value))
        }
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// Digest of the exact provider-owned barrier envelope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MachineStopBarrierDigest(String);

impl MachineStopBarrierDigest {
    pub fn new(value: impl Into<String>) -> Result<Self, MachineStopAuthorityEvidenceError> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(MachineStopAuthorityEvidenceError::InvalidBarrierDigest);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Authenticated provider claim that stops new machine workload admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MachineStopAdmissionBarrier {
    machine_name: String,
    forwarder_authority: MachineForwarderAuthority,
    epoch: MachineStopBarrierEpoch,
    digest: MachineStopBarrierDigest,
}

impl MachineStopAdmissionBarrier {
    pub fn new(
        machine_name: impl Into<String>,
        forwarder_authority: MachineForwarderAuthority,
        epoch: MachineStopBarrierEpoch,
        digest: MachineStopBarrierDigest,
    ) -> Result<Self, MachineStopAuthorityEvidenceError> {
        let machine_name = machine_name.into();
        if machine_name.is_empty()
            || machine_name.trim() != machine_name
            || machine_name.chars().any(char::is_control)
        {
            return Err(MachineStopAuthorityEvidenceError::InvalidMachineName);
        }
        Ok(Self {
            machine_name,
            forwarder_authority,
            epoch,
            digest,
        })
    }

    pub fn machine_name(&self) -> &str {
        &self.machine_name
    }

    pub fn forwarder_authority(&self) -> &MachineForwarderAuthority {
        &self.forwarder_authority
    }

    pub const fn epoch(&self) -> MachineStopBarrierEpoch {
        self.epoch
    }

    pub fn digest(&self) -> &MachineStopBarrierDigest {
        &self.digest
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MachineStopAuthorityEvidenceError {
    #[error("machine stop barrier requires a canonical machine name")]
    InvalidMachineName,
    #[error("machine stop barrier epoch must be greater than zero")]
    InvalidBarrierEpoch,
    #[error("machine stop barrier digest must be 64 lowercase hexadecimal characters")]
    InvalidBarrierDigest,
    #[error("machine workload saga authority is corrupt")]
    CorruptSagaAuthority,
}

/// Compute classification of one canonical Engine saga record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineWorkloadSagaAuthorityState {
    ActiveDesired,
    Retiring,
    Terminal,
}

/// Minimal canonical Engine evidence needed by the stop policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineWorkloadSagaAuthority {
    key: WorkloadSagaKey,
    execution_provider_id: WorkloadExecutionProviderId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source_digest: WorkloadProvisionSourceDigest,
    state: MachineWorkloadSagaAuthorityState,
}

impl MachineWorkloadSagaAuthority {
    /// Construct exact durable saga authority from an adapter-owned read.
    pub fn new(
        key: WorkloadSagaKey,
        execution_provider_id: WorkloadExecutionProviderId,
        generation: WorkloadGeneration,
        desired_digest: WorkloadDesiredDigest,
        source_digest: WorkloadProvisionSourceDigest,
        state: MachineWorkloadSagaAuthorityState,
    ) -> Self {
        Self {
            key,
            execution_provider_id,
            generation,
            desired_digest,
            source_digest,
            state,
        }
    }

    /// Extract every exact intent in one record that belongs to `provider`.
    ///
    /// A record can retain an active intent while a successor intent is
    /// pending. Both are canonical authority and must remain visible to a
    /// machine-stop scan.
    pub fn from_record_for_provider(
        record: &WorkloadSagaRecord,
        provider: &WorkloadExecutionProviderId,
    ) -> Result<Vec<Self>, MachineStopAuthorityEvidenceError> {
        record
            .validate()
            .map_err(|_| MachineStopAuthorityEvidenceError::CorruptSagaAuthority)?;
        let mut authorities = Vec::with_capacity(2);
        let active = record.active_intent();
        if active.source().execution_provider_id() == provider {
            let state = if active.desired_state() == DesiredWorkloadState::Running {
                MachineWorkloadSagaAuthorityState::ActiveDesired
            } else if record.phase() == WorkloadSagaPhase::Recorded
                && record.successor_intent().is_none()
            {
                MachineWorkloadSagaAuthorityState::Terminal
            } else {
                MachineWorkloadSagaAuthorityState::Retiring
            };
            authorities.push(Self::from_intent(record.key().clone(), active, state));
        }
        if let Some(successor) = record.successor_intent()
            && successor.source().execution_provider_id() == provider
        {
            let state = if successor.desired_state() == DesiredWorkloadState::Running {
                MachineWorkloadSagaAuthorityState::ActiveDesired
            } else {
                MachineWorkloadSagaAuthorityState::Retiring
            };
            authorities.push(Self::from_intent(record.key().clone(), successor, state));
        }
        Ok(authorities)
    }

    fn from_intent(
        key: WorkloadSagaKey,
        intent: &WorkloadSagaIntent,
        state: MachineWorkloadSagaAuthorityState,
    ) -> Self {
        Self::new(
            key,
            intent.source().execution_provider_id().clone(),
            intent.generation(),
            intent.desired_digest(),
            intent.source().source_digest(),
            state,
        )
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn execution_provider_id(&self) -> &WorkloadExecutionProviderId {
        &self.execution_provider_id
    }

    pub const fn generation(&self) -> WorkloadGeneration {
        self.generation
    }

    pub const fn desired_digest(&self) -> WorkloadDesiredDigest {
        self.desired_digest
    }

    pub const fn source_digest(&self) -> WorkloadProvisionSourceDigest {
        self.source_digest
    }

    pub const fn state(&self) -> MachineWorkloadSagaAuthorityState {
        self.state
    }

    fn same_generation(&self, witness: &MachineProviderWorkloadWitness) -> bool {
        self.key == witness.key && self.generation == witness.generation
    }

    fn same_version(&self, witness: &MachineProviderWorkloadWitness) -> bool {
        self.same_generation(witness)
            && self.desired_digest == witness.desired_digest
            && self.source_digest == witness.source_digest
    }

    #[cfg(test)]
    fn fixture(
        key: WorkloadSagaKey,
        execution_provider_id: WorkloadExecutionProviderId,
        state: MachineWorkloadSagaAuthorityState,
    ) -> Self {
        Self {
            key,
            execution_provider_id,
            generation: WorkloadGeneration::new(1),
            desired_digest: WorkloadDesiredDigest::sha256(b"machine-authority-fixture"),
            source_digest: WorkloadProvisionSourceDigest::sha256(b"machine-source-fixture"),
            state,
        }
    }

    #[cfg(test)]
    fn crossed_digest(mut self) -> Self {
        self.desired_digest = WorkloadDesiredDigest::sha256(b"crossed-machine-authority");
        self
    }
}

/// Durable provider evidence that an exact machine workload may still exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineProviderWorkloadWitnessState {
    Active,
    RetirementPending,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineProviderWorkloadWitness {
    key: WorkloadSagaKey,
    execution_provider_id: WorkloadExecutionProviderId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source_digest: WorkloadProvisionSourceDigest,
    forwarder_authority: MachineForwarderAuthority,
    state: MachineProviderWorkloadWitnessState,
}

impl MachineProviderWorkloadWitness {
    pub fn new(
        key: WorkloadSagaKey,
        execution_provider_id: WorkloadExecutionProviderId,
        generation: WorkloadGeneration,
        desired_digest: WorkloadDesiredDigest,
        source_digest: WorkloadProvisionSourceDigest,
        forwarder_authority: MachineForwarderAuthority,
        state: MachineProviderWorkloadWitnessState,
    ) -> Self {
        Self {
            key,
            execution_provider_id,
            generation,
            desired_digest,
            source_digest,
            forwarder_authority,
            state,
        }
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn execution_provider_id(&self) -> &WorkloadExecutionProviderId {
        &self.execution_provider_id
    }

    pub const fn generation(&self) -> WorkloadGeneration {
        self.generation
    }

    pub const fn desired_digest(&self) -> WorkloadDesiredDigest {
        self.desired_digest
    }

    pub const fn source_digest(&self) -> WorkloadProvisionSourceDigest {
        self.source_digest
    }

    pub fn forwarder_authority(&self) -> &MachineForwarderAuthority {
        &self.forwarder_authority
    }

    pub const fn state(&self) -> MachineProviderWorkloadWitnessState {
        self.state
    }

    #[cfg(test)]
    fn fixture(
        key: WorkloadSagaKey,
        execution_provider_id: WorkloadExecutionProviderId,
        forwarder_authority: MachineForwarderAuthority,
        state: MachineProviderWorkloadWitnessState,
    ) -> Self {
        Self::new(
            key,
            execution_provider_id,
            WorkloadGeneration::new(1),
            WorkloadDesiredDigest::sha256(b"machine-authority-fixture"),
            WorkloadProvisionSourceDigest::sha256(b"machine-source-fixture"),
            forwarder_authority,
            state,
        )
    }
}

pub type MachineWorkloadAuthorityFuture<'a> = Pin<
    Box<
        dyn Future<
                Output = Result<
                    Vec<MachineWorkloadSagaAuthority>,
                    MachineWorkloadAuthorityStoreError,
                >,
            > + Send
            + 'a,
    >,
>;

/// Canonical desired-authority read port. Engine adapters implement it.
pub trait MachineWorkloadAuthorityStore: Send + Sync + 'static {
    fn list_machine_workload_authority_from_engine<'a>(
        &'a self,
        execution_provider_id: &'a WorkloadExecutionProviderId,
    ) -> MachineWorkloadAuthorityFuture<'a>;
}

pub type MachineStopBarrierAuthorityFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, MachineStopBarrierAuthorityError>> + Send + 'a>>;

/// Exact provider evidence returned only after the stop barrier is durable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineStopBarrierClaim {
    barrier: MachineStopAdmissionBarrier,
    provider_witnesses: Vec<MachineProviderWorkloadWitness>,
}

impl MachineStopBarrierClaim {
    pub fn new(
        barrier: MachineStopAdmissionBarrier,
        provider_witnesses: Vec<MachineProviderWorkloadWitness>,
    ) -> Self {
        Self {
            barrier,
            provider_witnesses,
        }
    }

    pub fn barrier(&self) -> &MachineStopAdmissionBarrier {
        &self.barrier
    }

    pub fn provider_witnesses(&self) -> &[MachineProviderWorkloadWitness] {
        &self.provider_witnesses
    }

    fn into_parts(
        self,
    ) -> (
        MachineStopAdmissionBarrier,
        Vec<MachineProviderWorkloadWitness>,
    ) {
        (self.barrier, self.provider_witnesses)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MachineStopBarrierAuthorityError {
    #[error("machine stop barrier authority is unavailable")]
    Unavailable,
    #[error("machine stop barrier authority is ambiguous")]
    Ambiguous,
    #[error("machine stop barrier authority is corrupt")]
    Corrupt,
    #[error("machine stop barrier authority is stale")]
    Stale,
    #[error("machine stop barrier authority is crossed")]
    Crossed,
}

/// Provider-owned persistence port used by the compute stop coordinator.
pub trait MachineStopBarrierAuthority: Send + Sync + 'static {
    fn claim_effect_free_barrier<'a>(
        &'a self,
        machine_name: &'a str,
        forwarder_authority: &'a MachineForwarderAuthority,
    ) -> MachineStopBarrierAuthorityFuture<'a, MachineStopBarrierClaim>;

    fn clear_effect_free_barrier<'a>(
        &'a self,
        barrier: &'a MachineStopAdmissionBarrier,
    ) -> MachineStopBarrierAuthorityFuture<'a, ()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MachineWorkloadAuthorityStoreError {
    #[error("machine workload authority store is unavailable")]
    Unavailable,
    #[error("machine workload authority store outcome is ambiguous")]
    Ambiguous,
    #[error("machine workload authority store is corrupt")]
    Corrupt,
}

/// Exact evidence union available after the provider barrier is durable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineWorkloadAuthoritySnapshot {
    sagas: Vec<MachineWorkloadSagaAuthority>,
    provider_witnesses: Vec<MachineProviderWorkloadWitness>,
}

/// Complete outcome of the two durable authority reads made after fencing.
///
/// The stop policy must receive an explicit complete outcome. A failed or
/// partial read can never be represented as an empty snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineWorkloadAuthorityEvidence {
    Complete(MachineWorkloadAuthoritySnapshot),
    Unavailable,
    Ambiguous,
    Corrupt,
}

impl MachineWorkloadAuthoritySnapshot {
    pub fn new(
        sagas: Vec<MachineWorkloadSagaAuthority>,
        provider_witnesses: Vec<MachineProviderWorkloadWitness>,
    ) -> Self {
        Self {
            sagas,
            provider_witnesses,
        }
    }

    pub fn sagas(&self) -> &[MachineWorkloadSagaAuthority] {
        &self.sagas
    }

    pub fn provider_witnesses(&self) -> &[MachineProviderWorkloadWitness] {
        &self.provider_witnesses
    }
}

/// Unforgeable compute result consumed by the physical effect owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedMachineStopAuthorization {
    barrier: MachineStopAdmissionBarrier,
    execution_provider_id: WorkloadExecutionProviderId,
}

impl ConfirmedMachineStopAuthorization {
    pub fn barrier(&self) -> &MachineStopAdmissionBarrier {
        &self.barrier
    }

    pub fn execution_provider_id(&self) -> &WorkloadExecutionProviderId {
        &self.execution_provider_id
    }
}

/// Exhaustive compute-owned decision for one physical stop request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineWorkloadStopDecision {
    EmptyWithFence(ConfirmedMachineStopAuthorization),
    ActiveWorkloadTeardownRequired,
    AuthorityUnavailable,
    Ambiguous,
    Corrupt,
    Stale,
    Crossed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MachineStopAuthorizationError {
    #[error("physical-machine stop requires workload teardown first")]
    ActiveWorkloadTeardownRequired,
    #[error("physical-machine workload authority is unavailable")]
    AuthorityUnavailable,
    #[error("physical-machine workload authority is ambiguous")]
    Ambiguous,
    #[error("physical-machine workload authority is corrupt")]
    Corrupt,
    #[error("physical-machine workload authority is stale")]
    Stale,
    #[error("physical-machine workload authority is crossed")]
    Crossed,
}

/// Claim the provider fence first, then join complete Engine and provider
/// evidence. Only an exact empty union returns the opaque physical-stop token.
pub async fn authorize_physical_machine_stop(
    barriers: &dyn MachineStopBarrierAuthority,
    workloads: &dyn MachineWorkloadAuthorityStore,
    machine_name: &str,
    forwarder_authority: &MachineForwarderAuthority,
    execution_provider_id: &WorkloadExecutionProviderId,
) -> Result<ConfirmedMachineStopAuthorization, MachineStopAuthorizationError> {
    let claim = barriers
        .claim_effect_free_barrier(machine_name, forwarder_authority)
        .await
        .map_err(map_barrier_error)?;
    let sagas = match workloads
        .list_machine_workload_authority_from_engine(execution_provider_id)
        .await
    {
        Ok(sagas) => MachineWorkloadAuthorityEvidence::Complete(
            MachineWorkloadAuthoritySnapshot::new(sagas, claim.provider_witnesses.clone()),
        ),
        Err(MachineWorkloadAuthorityStoreError::Unavailable) => {
            MachineWorkloadAuthorityEvidence::Unavailable
        }
        Err(MachineWorkloadAuthorityStoreError::Ambiguous) => {
            MachineWorkloadAuthorityEvidence::Ambiguous
        }
        Err(MachineWorkloadAuthorityStoreError::Corrupt) => {
            MachineWorkloadAuthorityEvidence::Corrupt
        }
    };
    let (barrier, _) = claim.into_parts();
    match classify_machine_stop_authority(barrier.clone(), execution_provider_id.clone(), sagas) {
        MachineWorkloadStopDecision::EmptyWithFence(authorization) => Ok(authorization),
        MachineWorkloadStopDecision::ActiveWorkloadTeardownRequired => {
            barriers
                .clear_effect_free_barrier(&barrier)
                .await
                .map_err(map_barrier_error)?;
            Err(MachineStopAuthorizationError::ActiveWorkloadTeardownRequired)
        }
        MachineWorkloadStopDecision::AuthorityUnavailable => {
            Err(MachineStopAuthorizationError::AuthorityUnavailable)
        }
        MachineWorkloadStopDecision::Ambiguous => Err(MachineStopAuthorizationError::Ambiguous),
        MachineWorkloadStopDecision::Corrupt => Err(MachineStopAuthorizationError::Corrupt),
        MachineWorkloadStopDecision::Stale => Err(MachineStopAuthorizationError::Stale),
        MachineWorkloadStopDecision::Crossed => Err(MachineStopAuthorizationError::Crossed),
    }
}

const fn map_barrier_error(
    error: MachineStopBarrierAuthorityError,
) -> MachineStopAuthorizationError {
    match error {
        MachineStopBarrierAuthorityError::Unavailable => {
            MachineStopAuthorizationError::AuthorityUnavailable
        }
        MachineStopBarrierAuthorityError::Ambiguous => MachineStopAuthorizationError::Ambiguous,
        MachineStopBarrierAuthorityError::Corrupt => MachineStopAuthorizationError::Corrupt,
        MachineStopBarrierAuthorityError::Stale => MachineStopAuthorizationError::Stale,
        MachineStopBarrierAuthorityError::Crossed => MachineStopAuthorizationError::Crossed,
    }
}

/// Classify the exact Engine/provider union after the stop barrier is durable.
pub fn classify_machine_stop_authority(
    barrier: MachineStopAdmissionBarrier,
    execution_provider_id: WorkloadExecutionProviderId,
    evidence: MachineWorkloadAuthorityEvidence,
) -> MachineWorkloadStopDecision {
    let snapshot = match evidence {
        MachineWorkloadAuthorityEvidence::Complete(snapshot) => snapshot,
        MachineWorkloadAuthorityEvidence::Unavailable => {
            return MachineWorkloadStopDecision::AuthorityUnavailable;
        }
        MachineWorkloadAuthorityEvidence::Ambiguous => {
            return MachineWorkloadStopDecision::Ambiguous;
        }
        MachineWorkloadAuthorityEvidence::Corrupt => {
            return MachineWorkloadStopDecision::Corrupt;
        }
    };
    if snapshot
        .sagas
        .iter()
        .any(|saga| saga.execution_provider_id != execution_provider_id)
        || snapshot
            .provider_witnesses
            .iter()
            .any(|witness| witness.execution_provider_id != execution_provider_id)
    {
        return MachineWorkloadStopDecision::Crossed;
    }
    for (index, saga) in snapshot.sagas.iter().enumerate() {
        if let Some(existing) = snapshot.sagas[..index]
            .iter()
            .find(|existing| existing.key == saga.key && existing.generation == saga.generation)
        {
            return if existing == saga {
                MachineWorkloadStopDecision::Corrupt
            } else {
                MachineWorkloadStopDecision::Crossed
            };
        }
    }
    for witness in &snapshot.provider_witnesses {
        if witness.forwarder_authority.provider_instance()
            != barrier.forwarder_authority.provider_instance()
        {
            return MachineWorkloadStopDecision::Crossed;
        }
        if witness.forwarder_authority.generation() != barrier.forwarder_authority.generation() {
            return MachineWorkloadStopDecision::Stale;
        }
        if witness.state == MachineProviderWorkloadWitnessState::Ambiguous {
            return MachineWorkloadStopDecision::Ambiguous;
        }
        if snapshot
            .sagas
            .iter()
            .any(|saga| saga.same_generation(witness) && !saga.same_version(witness))
        {
            return MachineWorkloadStopDecision::Crossed;
        }
    }
    for (index, witness) in snapshot.provider_witnesses.iter().enumerate() {
        if snapshot.provider_witnesses[..index]
            .iter()
            .any(|existing| existing.key == witness.key)
        {
            return MachineWorkloadStopDecision::Corrupt;
        }
    }
    if snapshot
        .sagas
        .iter()
        .any(|saga| saga.state != MachineWorkloadSagaAuthorityState::Terminal)
        || !snapshot.provider_witnesses.is_empty()
    {
        return MachineWorkloadStopDecision::ActiveWorkloadTeardownRequired;
    }
    MachineWorkloadStopDecision::EmptyWithFence(ConfirmedMachineStopAuthorization {
        barrier,
        execution_provider_id,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use nimbus_core::{TenantId, WorkloadId};
    use nimbus_machine::MachineForwarderAuthority;
    use nimbus_network::{NetworkProviderHandle, NetworkProviderId, NetworkResourceGeneration};
    use nimbus_workloads::{WorkloadExecutionProviderId, WorkloadSagaKey};

    use super::*;

    fn provider_id() -> WorkloadExecutionProviderId {
        WorkloadExecutionProviderId::for_registration_key("forwarded-machine")
    }

    fn key(name: &str) -> WorkloadSagaKey {
        WorkloadSagaKey::new(
            TenantId::new("tenant-machine-stop").unwrap(),
            WorkloadId::new(name).unwrap(),
        )
    }

    fn authority(provider: &str, generation: u64) -> MachineForwarderAuthority {
        MachineForwarderAuthority::new(
            NetworkProviderHandle::new(
                NetworkProviderId::for_registration_key(provider),
                format!("handle-{provider}"),
            )
            .unwrap(),
            NetworkResourceGeneration::new(generation),
        )
    }

    fn barrier() -> MachineStopAdmissionBarrier {
        MachineStopAdmissionBarrier::new(
            "default",
            authority("machine-provider", 7),
            MachineStopBarrierEpoch::new(1).unwrap(),
            MachineStopBarrierDigest::new("a".repeat(64)).unwrap(),
        )
        .unwrap()
    }

    fn complete(snapshot: MachineWorkloadAuthoritySnapshot) -> MachineWorkloadAuthorityEvidence {
        MachineWorkloadAuthorityEvidence::Complete(snapshot)
    }

    fn witness(
        name: &str,
        provider: WorkloadExecutionProviderId,
        forwarder: MachineForwarderAuthority,
        state: MachineProviderWorkloadWitnessState,
    ) -> MachineProviderWorkloadWitness {
        MachineProviderWorkloadWitness::fixture(key(name), provider, forwarder, state)
    }

    #[test]
    fn empty_exact_union_returns_unforgeable_fence() {
        let provider = provider_id();
        let decision = classify_machine_stop_authority(
            barrier(),
            provider.clone(),
            complete(MachineWorkloadAuthoritySnapshot::new(
                Vec::new(),
                Vec::new(),
            )),
        );
        let MachineWorkloadStopDecision::EmptyWithFence(authorization) = decision else {
            panic!("empty exact authority should authorize physical stop");
        };
        assert_eq!(authorization.execution_provider_id(), &provider);
        assert_eq!(authorization.barrier().epoch().as_u64(), 1);
    }

    #[test]
    fn active_or_retiring_engine_authority_requires_teardown() {
        for state in [
            MachineWorkloadSagaAuthorityState::ActiveDesired,
            MachineWorkloadSagaAuthorityState::Retiring,
        ] {
            let provider = provider_id();
            let snapshot = MachineWorkloadAuthoritySnapshot::new(
                vec![MachineWorkloadSagaAuthority::fixture(
                    key("api"),
                    provider.clone(),
                    state,
                )],
                Vec::new(),
            );
            assert_eq!(
                classify_machine_stop_authority(barrier(), provider, complete(snapshot)),
                MachineWorkloadStopDecision::ActiveWorkloadTeardownRequired
            );
        }
    }

    #[test]
    fn provider_witness_is_active_even_after_terminal_engine_record() {
        let provider = provider_id();
        let forwarder = barrier().forwarder_authority().clone();
        let snapshot = MachineWorkloadAuthoritySnapshot::new(
            vec![MachineWorkloadSagaAuthority::fixture(
                key("api"),
                provider.clone(),
                MachineWorkloadSagaAuthorityState::Terminal,
            )],
            vec![witness(
                "api",
                provider.clone(),
                forwarder,
                MachineProviderWorkloadWitnessState::RetirementPending,
            )],
        );
        assert_eq!(
            classify_machine_stop_authority(barrier(), provider, complete(snapshot)),
            MachineWorkloadStopDecision::ActiveWorkloadTeardownRequired
        );
    }

    #[test]
    fn crossed_stale_and_ambiguous_provider_evidence_fail_closed() {
        let provider = provider_id();
        let crossed = MachineWorkloadAuthoritySnapshot::new(
            Vec::new(),
            vec![witness(
                "api",
                provider.clone(),
                authority("crossed-provider", 7),
                MachineProviderWorkloadWitnessState::Active,
            )],
        );
        assert_eq!(
            classify_machine_stop_authority(barrier(), provider.clone(), complete(crossed)),
            MachineWorkloadStopDecision::Crossed
        );

        let stale = MachineWorkloadAuthoritySnapshot::new(
            Vec::new(),
            vec![witness(
                "api",
                provider.clone(),
                authority("machine-provider", 6),
                MachineProviderWorkloadWitnessState::Active,
            )],
        );
        assert_eq!(
            classify_machine_stop_authority(barrier(), provider.clone(), complete(stale)),
            MachineWorkloadStopDecision::Stale
        );

        let ambiguous = MachineWorkloadAuthoritySnapshot::new(
            Vec::new(),
            vec![witness(
                "api",
                provider.clone(),
                barrier().forwarder_authority().clone(),
                MachineProviderWorkloadWitnessState::Ambiguous,
            )],
        );
        assert_eq!(
            classify_machine_stop_authority(barrier(), provider, complete(ambiguous)),
            MachineWorkloadStopDecision::Ambiguous
        );
    }

    #[test]
    fn unavailable_ambiguous_and_corrupt_reads_are_not_empty_authority() {
        let provider = provider_id();
        for (evidence, expected) in [
            (
                MachineWorkloadAuthorityEvidence::Unavailable,
                MachineWorkloadStopDecision::AuthorityUnavailable,
            ),
            (
                MachineWorkloadAuthorityEvidence::Ambiguous,
                MachineWorkloadStopDecision::Ambiguous,
            ),
            (
                MachineWorkloadAuthorityEvidence::Corrupt,
                MachineWorkloadStopDecision::Corrupt,
            ),
        ] {
            assert_eq!(
                classify_machine_stop_authority(barrier(), provider.clone(), evidence),
                expected
            );
        }
    }

    #[test]
    fn duplicate_or_crossed_version_authority_fails_closed() {
        let provider = provider_id();
        let saga = MachineWorkloadSagaAuthority::fixture(
            key("api"),
            provider.clone(),
            MachineWorkloadSagaAuthorityState::Terminal,
        );
        assert_eq!(
            classify_machine_stop_authority(
                barrier(),
                provider.clone(),
                complete(MachineWorkloadAuthoritySnapshot::new(
                    vec![saga.clone(), saga.clone()],
                    Vec::new(),
                )),
            ),
            MachineWorkloadStopDecision::Corrupt
        );

        let forwarder = barrier().forwarder_authority().clone();
        assert_eq!(
            classify_machine_stop_authority(
                barrier(),
                provider.clone(),
                complete(MachineWorkloadAuthoritySnapshot::new(
                    vec![saga.clone().crossed_digest()],
                    vec![witness(
                        "api",
                        provider.clone(),
                        forwarder.clone(),
                        MachineProviderWorkloadWitnessState::Active,
                    )],
                )),
            ),
            MachineWorkloadStopDecision::Crossed
        );

        let duplicate = witness(
            "api",
            provider.clone(),
            forwarder,
            MachineProviderWorkloadWitnessState::Active,
        );
        assert_eq!(
            classify_machine_stop_authority(
                barrier(),
                provider,
                complete(MachineWorkloadAuthoritySnapshot::new(
                    vec![saga],
                    vec![duplicate.clone(), duplicate],
                )),
            ),
            MachineWorkloadStopDecision::Corrupt
        );
    }

    #[test]
    fn barrier_identity_rejects_invalid_name_epoch_and_digest() {
        assert_eq!(
            MachineStopBarrierEpoch::new(0),
            Err(MachineStopAuthorityEvidenceError::InvalidBarrierEpoch)
        );
        assert_eq!(
            MachineStopBarrierDigest::new("A".repeat(64)),
            Err(MachineStopAuthorityEvidenceError::InvalidBarrierDigest)
        );
        assert_eq!(
            MachineStopAdmissionBarrier::new(
                " default ",
                authority("machine-provider", 7),
                MachineStopBarrierEpoch::new(1).unwrap(),
                MachineStopBarrierDigest::new("b".repeat(64)).unwrap(),
            ),
            Err(MachineStopAuthorityEvidenceError::InvalidMachineName)
        );
    }

    #[derive(Clone)]
    struct RecordingBarrierAuthority {
        events: Arc<Mutex<Vec<&'static str>>>,
        witnesses: Vec<MachineProviderWorkloadWitness>,
    }

    impl MachineStopBarrierAuthority for RecordingBarrierAuthority {
        fn claim_effect_free_barrier<'a>(
            &'a self,
            machine_name: &'a str,
            forwarder_authority: &'a MachineForwarderAuthority,
        ) -> MachineStopBarrierAuthorityFuture<'a, MachineStopBarrierClaim> {
            let events = Arc::clone(&self.events);
            let witnesses = self.witnesses.clone();
            let barrier = MachineStopAdmissionBarrier::new(
                machine_name,
                forwarder_authority.clone(),
                MachineStopBarrierEpoch::new(1).unwrap(),
                MachineStopBarrierDigest::new("c".repeat(64)).unwrap(),
            )
            .unwrap();
            Box::pin(async move {
                events.lock().unwrap().push("claim");
                Ok(MachineStopBarrierClaim::new(barrier, witnesses))
            })
        }

        fn clear_effect_free_barrier<'a>(
            &'a self,
            _barrier: &'a MachineStopAdmissionBarrier,
        ) -> MachineStopBarrierAuthorityFuture<'a, ()> {
            let events = Arc::clone(&self.events);
            Box::pin(async move {
                events.lock().unwrap().push("clear");
                Ok(())
            })
        }
    }

    struct RecordingWorkloadStore {
        events: Arc<Mutex<Vec<&'static str>>>,
        result: Result<Vec<MachineWorkloadSagaAuthority>, MachineWorkloadAuthorityStoreError>,
    }

    impl MachineWorkloadAuthorityStore for RecordingWorkloadStore {
        fn list_machine_workload_authority_from_engine<'a>(
            &'a self,
            _execution_provider_id: &'a WorkloadExecutionProviderId,
        ) -> MachineWorkloadAuthorityFuture<'a> {
            let events = Arc::clone(&self.events);
            let result = self.result.clone();
            Box::pin(async move {
                events.lock().unwrap().push("engine");
                result
            })
        }
    }

    #[tokio::test]
    async fn machine_stop_rejects_active_workload_saga_authority() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let provider = provider_id();
        let barriers = RecordingBarrierAuthority {
            events: Arc::clone(&events),
            witnesses: Vec::new(),
        };
        let workloads = RecordingWorkloadStore {
            events: Arc::clone(&events),
            result: Ok(vec![MachineWorkloadSagaAuthority::fixture(
                key("active"),
                provider.clone(),
                MachineWorkloadSagaAuthorityState::ActiveDesired,
            )]),
        };

        assert_eq!(
            authorize_physical_machine_stop(
                &barriers,
                &workloads,
                "default",
                &authority("machine-provider", 7),
                &provider,
            )
            .await,
            Err(MachineStopAuthorizationError::ActiveWorkloadTeardownRequired)
        );
        assert_eq!(*events.lock().unwrap(), ["claim", "engine", "clear"]);
    }

    #[tokio::test]
    async fn machine_stop_exact_empty_fence_precedes_publication_and_vmm_effects() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let provider = provider_id();
        let authorization = authorize_physical_machine_stop(
            &RecordingBarrierAuthority {
                events: Arc::clone(&events),
                witnesses: Vec::new(),
            },
            &RecordingWorkloadStore {
                events: Arc::clone(&events),
                result: Ok(Vec::new()),
            },
            "default",
            &authority("machine-provider", 7),
            &provider,
        )
        .await
        .expect("an exact empty authority union should return the opaque stop fence");

        assert_eq!(authorization.execution_provider_id(), &provider);
        events.lock().unwrap().extend(["publication", "vmm"]);
        assert_eq!(
            *events.lock().unwrap(),
            ["claim", "engine", "publication", "vmm"]
        );
    }

    #[tokio::test]
    async fn machine_stop_active_authority_makes_zero_publication_ssh_vmm_or_state_effects() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let provider = provider_id();
        let result = authorize_physical_machine_stop(
            &RecordingBarrierAuthority {
                events: Arc::clone(&events),
                witnesses: Vec::new(),
            },
            &RecordingWorkloadStore {
                events: Arc::clone(&events),
                result: Ok(vec![MachineWorkloadSagaAuthority::fixture(
                    key("active-zero-effects"),
                    provider.clone(),
                    MachineWorkloadSagaAuthorityState::ActiveDesired,
                )]),
            },
            "default",
            &authority("machine-provider", 7),
            &provider,
        )
        .await;

        assert_eq!(
            result,
            Err(MachineStopAuthorizationError::ActiveWorkloadTeardownRequired)
        );
        assert_eq!(*events.lock().unwrap(), ["claim", "engine", "clear"]);
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .all(|event| !matches!(*event, "publication" | "ssh" | "vmm" | "state")),
            "active authority must return before every physical-effect owner"
        );
    }

    #[tokio::test]
    async fn machine_restart_cannot_bypass_active_workload_fence() {
        let (result, events) = active_authority_caller_evidence("restart").await;
        assert_eq!(
            result,
            Err(MachineStopAuthorizationError::ActiveWorkloadTeardownRequired)
        );
        assert_eq!(events, ["claim", "engine", "clear"]);
    }

    #[tokio::test]
    async fn machine_os_restart_cannot_bypass_active_workload_fence() {
        let (result, events) = active_authority_caller_evidence("os-restart").await;
        assert_eq!(
            result,
            Err(MachineStopAuthorizationError::ActiveWorkloadTeardownRequired)
        );
        assert_eq!(events, ["claim", "engine", "clear"]);
    }

    #[tokio::test]
    async fn stopped_machine_with_active_durable_authority_returns_typed_conflict() {
        // Observed machine lifecycle is intentionally not an input to this
        // durable authority decision. A stopped projection cannot override an
        // active Engine record.
        let (result, events) = active_authority_caller_evidence("stopped-observation").await;
        assert_eq!(
            result,
            Err(MachineStopAuthorizationError::ActiveWorkloadTeardownRequired)
        );
        assert_eq!(events, ["claim", "engine", "clear"]);
    }

    async fn active_authority_caller_evidence(
        effect: &'static str,
    ) -> (
        Result<ConfirmedMachineStopAuthorization, MachineStopAuthorizationError>,
        Vec<&'static str>,
    ) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let provider = provider_id();
        let result = authorize_physical_machine_stop(
            &RecordingBarrierAuthority {
                events: Arc::clone(&events),
                witnesses: Vec::new(),
            },
            &RecordingWorkloadStore {
                events: Arc::clone(&events),
                result: Ok(vec![MachineWorkloadSagaAuthority::fixture(
                    key(effect),
                    provider.clone(),
                    MachineWorkloadSagaAuthorityState::ActiveDesired,
                )]),
            },
            "default",
            &authority("machine-provider", 7),
            &provider,
        )
        .await;
        let observed = events.lock().unwrap().clone();
        assert!(!observed.contains(&effect));
        (result, observed)
    }

    #[tokio::test]
    async fn machine_stop_ambiguous_unavailable_or_corrupt_authority_fails_closed() {
        let provider = provider_id();
        for (store_error, expected) in [
            (
                MachineWorkloadAuthorityStoreError::Unavailable,
                MachineStopAuthorizationError::AuthorityUnavailable,
            ),
            (
                MachineWorkloadAuthorityStoreError::Ambiguous,
                MachineStopAuthorizationError::Ambiguous,
            ),
            (
                MachineWorkloadAuthorityStoreError::Corrupt,
                MachineStopAuthorizationError::Corrupt,
            ),
        ] {
            let events = Arc::new(Mutex::new(Vec::new()));
            let barriers = RecordingBarrierAuthority {
                events: Arc::clone(&events),
                witnesses: Vec::new(),
            };
            let workloads = RecordingWorkloadStore {
                events: Arc::clone(&events),
                result: Err(store_error),
            };

            assert_eq!(
                authorize_physical_machine_stop(
                    &barriers,
                    &workloads,
                    "default",
                    &authority("machine-provider", 7),
                    &provider,
                )
                .await,
                Err(expected)
            );
            assert_eq!(*events.lock().unwrap(), ["claim", "engine"]);
        }
    }

    #[test]
    fn machine_stop_ignores_observed_projection_and_address_identity() {
        let provider = provider_id();
        let active = MachineWorkloadSagaAuthority::fixture(
            key("projection-independent"),
            provider.clone(),
            MachineWorkloadSagaAuthorityState::ActiveDesired,
        );
        let snapshot = MachineWorkloadAuthoritySnapshot::new(vec![active], Vec::new());

        assert_eq!(
            classify_machine_stop_authority(barrier(), provider, complete(snapshot)),
            MachineWorkloadStopDecision::ActiveWorkloadTeardownRequired
        );
        // The policy accepts no projection, address, listener, or provider
        // handle input that could override canonical Engine authority.
    }
}
