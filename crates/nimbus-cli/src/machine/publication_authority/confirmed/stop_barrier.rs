//! Provider-owned physical-machine stop barrier and admission guard.
//!
//! Compute owns the stop decision. This module owns only the durable provider
//! fence, exact provider witnesses, and the process-safe permit that excludes
//! a workload-desire CAS while a stop barrier is active.

use std::sync::Arc;

use nimbus::Error;
use nimbus_compute::machine_stop_authority::{
    MachineProviderWorkloadWitness, MachineProviderWorkloadWitnessState,
    MachineStopAdmissionBarrier, MachineStopBarrierAuthority, MachineStopBarrierAuthorityError,
    MachineStopBarrierAuthorityFuture, MachineStopBarrierClaim, MachineStopBarrierDigest,
    MachineStopBarrierEpoch,
};
use nimbus_compute::workload_saga::{
    WorkloadDesireAdmissionError, WorkloadDesireAdmissionFuture, WorkloadDesireAdmissionGuard,
    WorkloadDesireAdmissionPermit, WorkloadDesireAdmissionRequest,
};
use nimbus_machine::MachineForwarderAuthority;
use nimbus_workloads::WorkloadExecutionProviderId;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{
    ConfirmedMachinePublicationBody, ConfirmedMachinePublicationJournal,
    ConfirmedMachinePublicationLock, ConfirmedMachinePublicationRetirementProgress, FORMAT_VERSION,
    remove_file_if_exists,
};
#[cfg(test)]
use super::{LOCK_CONTENTION_ARMED, LOCK_CONTENTION_FIFO, STOP_BARRIER_STAGED_FIFO};

const STOP_BARRIER_DIGEST_DOMAIN: &str = "nimbus.machine.stop-barrier.v1";

/// Process-safe provider adapter used by the compute-owned stop coordinator.
#[derive(Clone)]
pub(crate) struct ConfirmedMachineStopBarrierAuthority {
    journal: ConfirmedMachinePublicationJournal,
}

impl ConfirmedMachineStopBarrierAuthority {
    pub(crate) fn new(journal: ConfirmedMachinePublicationJournal) -> Self {
        Self { journal }
    }

    pub(crate) fn journal(&self) -> &ConfirmedMachinePublicationJournal {
        &self.journal
    }
}

impl MachineStopBarrierAuthority for ConfirmedMachineStopBarrierAuthority {
    fn claim_effect_free_barrier<'a>(
        &'a self,
        machine_name: &'a str,
        forwarder_authority: &'a MachineForwarderAuthority,
    ) -> MachineStopBarrierAuthorityFuture<'a, MachineStopBarrierClaim> {
        let journal = self.journal.clone();
        let machine_name = machine_name.to_owned();
        let forwarder_authority = forwarder_authority.clone();
        Box::pin(async move {
            let claim = tokio::task::spawn_blocking(move || {
                journal.claim_machine_stop_barrier(&machine_name, &forwarder_authority)
            })
            .await
            .map_err(|_| MachineStopBarrierAuthorityError::Unavailable)??;
            if claim.state() != DurableMachineStopBarrierState::EffectFreeFenced {
                return Err(MachineStopBarrierAuthorityError::Ambiguous);
            }
            Ok(MachineStopBarrierClaim::new(
                claim.barrier().clone(),
                claim.provider_witnesses().to_vec(),
            ))
        })
    }

    fn clear_effect_free_barrier<'a>(
        &'a self,
        barrier: &'a MachineStopAdmissionBarrier,
    ) -> MachineStopBarrierAuthorityFuture<'a, ()> {
        let journal = self.journal.clone();
        let barrier = barrier.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                journal.clear_effect_free_machine_stop_barrier(&barrier)
            })
            .await
            .map_err(|_| MachineStopBarrierAuthorityError::Unavailable)?
            .map_err(map_stop_barrier_store_error)?;
            Ok(())
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DurableMachineStopBarrierState {
    EffectFreeFenced,
    ClearedEffectFree,
    StopMayExist,
    StoppedObservedAbsent,
}

impl DurableMachineStopBarrierState {
    const fn is_terminal(self) -> bool {
        matches!(self, Self::ClearedEffectFree | Self::StoppedObservedAbsent)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct DurableMachineStopBarrier {
    machine_name: String,
    forwarder_authority: MachineForwarderAuthority,
    epoch: MachineStopBarrierEpoch,
    state: DurableMachineStopBarrierState,
    digest: MachineStopBarrierDigest,
}

impl DurableMachineStopBarrier {
    fn new(
        machine_name: &str,
        forwarder_authority: &MachineForwarderAuthority,
        epoch: MachineStopBarrierEpoch,
    ) -> Result<Self, Error> {
        validate_machine_name(machine_name)?;
        let mut barrier = Self {
            machine_name: machine_name.to_owned(),
            forwarder_authority: forwarder_authority.clone(),
            epoch,
            state: DurableMachineStopBarrierState::EffectFreeFenced,
            digest: MachineStopBarrierDigest::new("0".repeat(64)).map_err(evidence_error)?,
        };
        barrier.digest = barrier.derive_digest()?;
        barrier.validate()?;
        Ok(barrier)
    }

    fn validate(&self) -> Result<(), Error> {
        validate_machine_name(&self.machine_name)?;
        MachineStopBarrierEpoch::new(self.epoch.as_u64()).map_err(evidence_error)?;
        if self.digest != self.derive_digest()? {
            return Err(Error::PreconditionFailed(
                "confirmed machine stop barrier failed digest validation".to_owned(),
            ));
        }
        Ok(())
    }

    fn derive_digest(&self) -> Result<MachineStopBarrierDigest, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct DigestPayload<'a> {
            domain: &'static str,
            format_version: u32,
            machine_name: &'a str,
            forwarder_authority: &'a MachineForwarderAuthority,
            epoch: MachineStopBarrierEpoch,
            state: DurableMachineStopBarrierState,
        }

        let bytes = serde_json::to_vec(&DigestPayload {
            domain: STOP_BARRIER_DIGEST_DOMAIN,
            format_version: FORMAT_VERSION,
            machine_name: &self.machine_name,
            forwarder_authority: &self.forwarder_authority,
            epoch: self.epoch,
            state: self.state,
        })
        .map_err(|error| {
            Error::Internal(format!(
                "failed to encode confirmed machine stop barrier digest: {error}"
            ))
        })?;
        MachineStopBarrierDigest::new(format!("{:x}", Sha256::digest(bytes)))
            .map_err(evidence_error)
    }

    fn admission_barrier(&self) -> Result<MachineStopAdmissionBarrier, Error> {
        MachineStopAdmissionBarrier::new(
            self.machine_name.clone(),
            self.forwarder_authority.clone(),
            self.epoch,
            self.digest.clone(),
        )
        .map_err(evidence_error)
    }

    fn authenticate(&self, presented: &MachineStopAdmissionBarrier) -> Result<(), Error> {
        if self.machine_name == presented.machine_name()
            && self.forwarder_authority == *presented.forwarder_authority()
            && self.epoch == presented.epoch()
            && self.digest == *presented.digest()
        {
            Ok(())
        } else {
            Err(Error::conflict(
                "machine stop barrier is stale or crossed with durable provider authority",
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaimedMachineStopBarrier {
    barrier: MachineStopAdmissionBarrier,
    state: DurableMachineStopBarrierState,
    provider_witnesses: Vec<MachineProviderWorkloadWitness>,
}

impl ClaimedMachineStopBarrier {
    pub(crate) fn barrier(&self) -> &MachineStopAdmissionBarrier {
        &self.barrier
    }

    pub(crate) const fn state(&self) -> DurableMachineStopBarrierState {
        self.state
    }

    pub(crate) fn provider_witnesses(&self) -> &[MachineProviderWorkloadWitness] {
        &self.provider_witnesses
    }
}

impl ConfirmedMachinePublicationJournal {
    /// Persist or replay the exact effect-free barrier and read every matching
    /// provider witness while the same process-safe lock remains held.
    pub(crate) fn claim_machine_stop_barrier(
        &self,
        machine_name: &str,
        forwarder_authority: &MachineForwarderAuthority,
    ) -> Result<ClaimedMachineStopBarrier, MachineStopBarrierAuthorityError> {
        validate_machine_name(machine_name).map_err(map_stop_barrier_store_error)?;
        self.mutate_with_error(
            |body| {
                let latest = body
                    .stop_barriers
                    .iter()
                    .filter(|barrier| barrier.machine_name == machine_name)
                    .max_by_key(|barrier| barrier.epoch);
                let barrier = match latest {
                    Some(existing) if !existing.state.is_terminal() => {
                        if existing.forwarder_authority.provider_instance()
                            != forwarder_authority.provider_instance()
                        {
                            return Err(MachineStopBarrierAuthorityError::Crossed);
                        }
                        if existing.forwarder_authority.generation()
                            != forwarder_authority.generation()
                        {
                            return Err(MachineStopBarrierAuthorityError::Stale);
                        }
                        existing.clone()
                    }
                    previous => {
                        let next = previous.map_or(1, |previous| {
                            previous.epoch.as_u64().checked_add(1).unwrap_or(0)
                        });
                        let epoch = MachineStopBarrierEpoch::new(next)
                            .map_err(evidence_error)
                            .map_err(map_stop_barrier_store_error)?;
                        let barrier = DurableMachineStopBarrier::new(
                            machine_name,
                            forwarder_authority,
                            epoch,
                        )
                        .map_err(map_stop_barrier_store_error)?;
                        body.stop_barriers.push(barrier.clone());
                        body.stop_barriers.sort_by(|left, right| {
                            (&left.machine_name, left.epoch)
                                .cmp(&(&right.machine_name, right.epoch))
                        });
                        barrier
                    }
                };
                #[cfg(test)]
                self.pause_stop_barrier_before_publish_for_test()
                    .map_err(map_stop_barrier_store_error)?;
                let provider_witnesses =
                    provider_witnesses(body, machine_name).map_err(map_stop_barrier_store_error)?;
                Ok(ClaimedMachineStopBarrier {
                    barrier: barrier
                        .admission_barrier()
                        .map_err(map_stop_barrier_store_error)?,
                    state: barrier.state,
                    provider_witnesses,
                })
            },
            map_stop_barrier_store_error,
        )
    }

    #[cfg(test)]
    fn pause_stop_barrier_before_publish_for_test(&self) -> Result<(), Error> {
        use std::io::{Read as _, Write as _};

        let staged_path = self.root.join(STOP_BARRIER_STAGED_FIFO);
        if !staged_path.exists() {
            return Ok(());
        }
        let armed_path = self.root.join(LOCK_CONTENTION_ARMED);
        std::fs::write(&armed_path, b"armed").map_err(|error| {
            Error::Internal(format!(
                "arm stop-barrier lock-contention hook {}: {error}",
                armed_path.display()
            ))
        })?;
        let mut staged = std::fs::OpenOptions::new()
            .write(true)
            .open(&staged_path)
            .map_err(|error| {
                Error::Internal(format!(
                    "open stop-barrier-staged test FIFO {}: {error}",
                    staged_path.display()
                ))
            })?;
        staged.write_all(b"1").map_err(|error| {
            Error::Internal(format!(
                "signal stop-barrier-staged test FIFO {}: {error}",
                staged_path.display()
            ))
        })?;
        staged.flush().map_err(|error| {
            Error::Internal(format!(
                "flush stop-barrier-staged test FIFO {}: {error}",
                staged_path.display()
            ))
        })?;

        let contention_path = self.root.join(LOCK_CONTENTION_FIFO);
        let mut contention = std::fs::OpenOptions::new()
            .read(true)
            .open(&contention_path)
            .map_err(|error| {
                Error::Internal(format!(
                    "open lock-contention test FIFO {}: {error}",
                    contention_path.display()
                ))
            })?;
        let mut byte = [0_u8; 1];
        contention.read_exact(&mut byte).map_err(|error| {
            Error::Internal(format!(
                "read lock-contention test FIFO {}: {error}",
                contention_path.display()
            ))
        })?;
        std::fs::remove_file(&armed_path).map_err(|error| {
            Error::Internal(format!(
                "disarm stop-barrier lock-contention hook {}: {error}",
                armed_path.display()
            ))
        })?;
        if byte != *b"1" {
            return Err(Error::PreconditionFailed(
                "lock-contention test FIFO carried an invalid semantic token".to_owned(),
            ));
        }
        Ok(())
    }

    pub(crate) fn clear_effect_free_machine_stop_barrier(
        &self,
        barrier: &MachineStopAdmissionBarrier,
    ) -> Result<MachineStopAdmissionBarrier, Error> {
        self.transition_machine_stop_barrier(
            barrier,
            DurableMachineStopBarrierState::EffectFreeFenced,
            DurableMachineStopBarrierState::ClearedEffectFree,
        )
    }

    pub(crate) fn begin_physical_machine_stop(
        &self,
        barrier: &MachineStopAdmissionBarrier,
    ) -> Result<MachineStopAdmissionBarrier, Error> {
        self.transition_machine_stop_barrier(
            barrier,
            DurableMachineStopBarrierState::EffectFreeFenced,
            DurableMachineStopBarrierState::StopMayExist,
        )
    }

    pub(crate) fn record_physical_machine_stop_absent(
        &self,
        barrier: &MachineStopAdmissionBarrier,
    ) -> Result<MachineStopAdmissionBarrier, Error> {
        self.transition_machine_stop_barrier(
            barrier,
            DurableMachineStopBarrierState::StopMayExist,
            DurableMachineStopBarrierState::StoppedObservedAbsent,
        )
    }

    fn transition_machine_stop_barrier(
        &self,
        presented: &MachineStopAdmissionBarrier,
        expected: DurableMachineStopBarrierState,
        next: DurableMachineStopBarrierState,
    ) -> Result<MachineStopAdmissionBarrier, Error> {
        self.mutate(|body| {
            let durable = body
                .stop_barriers
                .iter_mut()
                .find(|barrier| {
                    barrier.machine_name == presented.machine_name()
                        && barrier.epoch == presented.epoch()
                })
                .ok_or_else(|| Error::NotFound("machine stop barrier is not durable".to_owned()))?;
            durable.authenticate(presented)?;
            if durable.state != expected {
                return Err(Error::conflict(
                    "machine stop barrier is not in the required effect state",
                ));
            }
            durable.state = next;
            durable.digest = durable.derive_digest()?;
            durable.admission_barrier()
        })
    }
}

/// Reject a workload-provider admission while the exact machine has a
/// non-terminal physical-stop barrier. The caller runs this check inside the
/// same confirmed-journal mutation that stages its provider authority.
pub(super) fn authenticate_workload_admission_absence(
    body: &ConfirmedMachinePublicationBody,
    machine_name: &str,
    forwarder_authority: &MachineForwarderAuthority,
) -> Result<(), Error> {
    validate_machine_name(machine_name)?;
    let Some(barrier) = body
        .stop_barriers
        .iter()
        .filter(|barrier| barrier.machine_name == machine_name)
        .max_by_key(|barrier| barrier.epoch)
        .filter(|barrier| !barrier.state.is_terminal())
    else {
        return Ok(());
    };
    if barrier.forwarder_authority.provider_instance() != forwarder_authority.provider_instance() {
        return Err(Error::conflict(
            "workload admission is crossed with an unresolved machine stop barrier",
        ));
    }
    if barrier.forwarder_authority.generation() != forwarder_authority.generation() {
        return Err(Error::conflict(
            "workload admission is stale behind an unresolved machine stop barrier",
        ));
    }
    Err(Error::conflict(
        "workload admission is fenced by a physical-machine stop barrier",
    ))
}

fn provider_witnesses(
    body: &ConfirmedMachinePublicationBody,
    machine_name: &str,
) -> Result<Vec<MachineProviderWorkloadWitness>, Error> {
    let mut witnesses = body
        .retirement_witnesses
        .iter()
        .filter(|witness| witness.machine_name == machine_name)
        .filter_map(|witness| {
            let state = match witness.progress {
                ConfirmedMachinePublicationRetirementProgress::Active => {
                    MachineProviderWorkloadWitnessState::Active
                }
                ConfirmedMachinePublicationRetirementProgress::WithdrawalMayExist { .. }
                | ConfirmedMachinePublicationRetirementProgress::ReleaseMayExist { .. } => {
                    MachineProviderWorkloadWitnessState::Ambiguous
                }
                ConfirmedMachinePublicationRetirementProgress::WithdrawnRetained { .. } => {
                    MachineProviderWorkloadWitnessState::RetirementPending
                }
                ConfirmedMachinePublicationRetirementProgress::Released { .. }
                | ConfirmedMachinePublicationRetirementProgress::LegacyReleased => return None,
            };
            Some(MachineProviderWorkloadWitness::new(
                witness.workload_key.clone(),
                witness.execution_provider_id.clone(),
                witness.generation,
                witness.desired_digest,
                witness.source_digest,
                witness.forwarder_authority.clone(),
                state,
            ))
        })
        .collect::<Vec<_>>();
    witnesses.sort_by(|left, right| {
        (left.key(), left.generation()).cmp(&(right.key(), right.generation()))
    });
    for (index, witness) in witnesses.iter().enumerate() {
        if witnesses[..index].iter().any(|existing| {
            existing.key() == witness.key() && existing.generation() == witness.generation()
        }) {
            return Err(Error::PreconditionFailed(
                "confirmed machine provider witnesses contain duplicate workload authority"
                    .to_owned(),
            ));
        }
    }
    Ok(witnesses)
}

pub(super) fn validate_stop_barrier_history(
    barriers: &[DurableMachineStopBarrier],
) -> Result<(), Error> {
    for (index, barrier) in barriers.iter().enumerate() {
        barrier.validate()?;
        if barriers[..index].iter().any(|existing| {
            existing.machine_name == barrier.machine_name && existing.epoch == barrier.epoch
        }) {
            return Err(Error::PreconditionFailed(
                "confirmed machine stop barrier history contains a duplicate epoch".to_owned(),
            ));
        }
        if !barrier.state.is_terminal()
            && barriers
                .iter()
                .skip(index + 1)
                .any(|later| later.machine_name == barrier.machine_name)
        {
            return Err(Error::PreconditionFailed(
                "an unresolved machine stop barrier was overwritten by a later epoch".to_owned(),
            ));
        }
    }
    if barriers.windows(2).any(|pair| {
        (&pair[0].machine_name, pair[0].epoch) >= (&pair[1].machine_name, pair[1].epoch)
    }) {
        return Err(Error::PreconditionFailed(
            "confirmed machine stop barrier history is not canonically sorted".to_owned(),
        ));
    }
    Ok(())
}

/// Exact provider adapter for the compute-owned desire-admission seam.
#[derive(Clone)]
pub(crate) struct ConfirmedMachineDesireAdmissionGuard {
    journal: ConfirmedMachinePublicationJournal,
    machine_name: Arc<str>,
    forwarder_authority: MachineForwarderAuthority,
    execution_provider_id: WorkloadExecutionProviderId,
}

impl ConfirmedMachineDesireAdmissionGuard {
    pub(crate) fn new(
        journal: ConfirmedMachinePublicationJournal,
        machine_name: impl Into<String>,
        forwarder_authority: MachineForwarderAuthority,
        execution_provider_id: WorkloadExecutionProviderId,
    ) -> Result<Self, Error> {
        let machine_name = machine_name.into();
        validate_machine_name(&machine_name)?;
        Ok(Self {
            journal,
            machine_name: Arc::from(machine_name),
            forwarder_authority,
            execution_provider_id,
        })
    }

    fn acquire_blocking(
        &self,
        request: &WorkloadDesireAdmissionRequest,
    ) -> Result<ConfirmedMachinePublicationLock, WorkloadDesireAdmissionError> {
        if request.execution_provider_id() != &self.execution_provider_id {
            return Err(WorkloadDesireAdmissionError::Crossed);
        }
        let lock = self
            .journal
            .acquire_lock()
            .map_err(map_admission_store_error)?;
        self.journal
            .validate_directory_entries()
            .map_err(map_admission_store_error)?;
        remove_file_if_exists(&self.journal.stage_path).map_err(map_admission_store_error)?;
        let envelope = self
            .journal
            .load_envelope()
            .map_err(map_admission_store_error)?;
        envelope
            .body
            .validate()
            .map_err(map_admission_store_error)?;
        if let Some(barrier) = envelope
            .body
            .stop_barriers
            .iter()
            .filter(|barrier| barrier.machine_name.as_str() == self.machine_name.as_ref())
            .max_by_key(|barrier| barrier.epoch)
            .filter(|barrier| !barrier.state.is_terminal())
        {
            if barrier.forwarder_authority.provider_instance()
                != self.forwarder_authority.provider_instance()
            {
                return Err(WorkloadDesireAdmissionError::Crossed);
            }
            if barrier.forwarder_authority.generation() != self.forwarder_authority.generation() {
                return Err(WorkloadDesireAdmissionError::Stale);
            }
            return Err(WorkloadDesireAdmissionError::Fenced);
        }
        Ok(lock)
    }
}

struct ConfirmedMachineDesireAdmissionPermit {
    _lock: ConfirmedMachinePublicationLock,
}

impl WorkloadDesireAdmissionPermit for ConfirmedMachineDesireAdmissionPermit {}

impl WorkloadDesireAdmissionGuard for ConfirmedMachineDesireAdmissionGuard {
    fn acquire<'a>(
        &'a self,
        request: &'a WorkloadDesireAdmissionRequest,
    ) -> WorkloadDesireAdmissionFuture<'a> {
        let guard = self.clone();
        let request = request.clone();
        Box::pin(async move {
            let lock = tokio::task::spawn_blocking(move || guard.acquire_blocking(&request))
                .await
                .map_err(|_| WorkloadDesireAdmissionError::Unavailable)??;
            Ok(
                Box::new(ConfirmedMachineDesireAdmissionPermit { _lock: lock })
                    as Box<dyn WorkloadDesireAdmissionPermit>,
            )
        })
    }
}

pub(super) fn validate_machine_name(machine_name: &str) -> Result<(), Error> {
    if machine_name.is_empty()
        || machine_name.trim() != machine_name
        || machine_name.chars().any(char::is_control)
    {
        Err(Error::InvalidInput(
            "machine stop barrier requires a canonical machine name".to_owned(),
        ))
    } else {
        Ok(())
    }
}

fn evidence_error(error: impl std::fmt::Display) -> Error {
    Error::PreconditionFailed(error.to_string())
}

fn map_admission_store_error(error: Error) -> WorkloadDesireAdmissionError {
    match error {
        Error::ResourceExhausted(_)
        | Error::Overloaded { .. }
        | Error::Storage { .. }
        | Error::Transport(_)
        | Error::Internal(_) => WorkloadDesireAdmissionError::Unavailable,
        Error::Conflict { .. } => WorkloadDesireAdmissionError::Crossed,
        _ => WorkloadDesireAdmissionError::Corrupt,
    }
}

fn map_stop_barrier_store_error(error: Error) -> MachineStopBarrierAuthorityError {
    match error {
        Error::ResourceExhausted(_)
        | Error::Overloaded { .. }
        | Error::Storage { .. }
        | Error::Transport(_)
        | Error::Internal(_) => MachineStopBarrierAuthorityError::Unavailable,
        Error::Conflict { .. } => MachineStopBarrierAuthorityError::Crossed,
        _ => MachineStopBarrierAuthorityError::Corrupt,
    }
}

#[cfg(test)]
mod tests {
    use nimbus_core::{TenantId, WorkloadId};
    use nimbus_network::{NetworkProviderHandle, NetworkProviderId, NetworkResourceGeneration};
    use nimbus_workloads::{
        WorkloadDesiredDigest, WorkloadExecutionReference, WorkloadGeneration,
        WorkloadProvisionSourceDigest, WorkloadRestartEpoch, WorkloadSagaKey,
    };
    use tempfile::TempDir;

    use super::super::{ConfirmedMachinePublicationEnvelope, envelope_checksum};
    use super::*;

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

    fn execution_provider() -> WorkloadExecutionProviderId {
        WorkloadExecutionProviderId::for_registration_key("forwarded-machine")
    }

    fn desire_request() -> WorkloadDesireAdmissionRequest {
        WorkloadDesireAdmissionRequest::new(
            WorkloadSagaKey::new(
                TenantId::new("tenant-stop-barrier").unwrap(),
                WorkloadId::new("workload-stop-barrier").unwrap(),
            ),
            execution_provider(),
            WorkloadGeneration::new(1),
            WorkloadDesiredDigest::sha256(b"stop-barrier-desire"),
            WorkloadProvisionSourceDigest::sha256(b"stop-barrier-source"),
        )
    }

    #[tokio::test]
    async fn machine_stop_stale_or_crossed_machine_generation_makes_zero_effects() {
        let root = TempDir::new().unwrap();
        let journal = ConfirmedMachinePublicationJournal::open(root.path()).unwrap();
        let exact = authority("machine-provider", 7);
        let claimed = journal
            .claim_machine_stop_barrier("default", &exact)
            .unwrap();
        let durable = std::fs::read(&journal.state_path).unwrap();

        assert_eq!(
            journal.claim_machine_stop_barrier("default", &authority("machine-provider", 8)),
            Err(MachineStopBarrierAuthorityError::Stale)
        );
        assert_eq!(std::fs::read(&journal.state_path).unwrap(), durable);
        assert_eq!(
            journal.claim_machine_stop_barrier("default", &authority("crossed-provider", 7)),
            Err(MachineStopBarrierAuthorityError::Crossed)
        );
        assert_eq!(std::fs::read(&journal.state_path).unwrap(), durable);
        assert_eq!(claimed.barrier().forwarder_authority(), &exact);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn machine_stop_barrier_waits_for_inflight_engine_desire_commit() {
        let root = TempDir::new().unwrap();
        let exact = authority("machine-provider", 7);
        let journal = ConfirmedMachinePublicationJournal::open(root.path()).unwrap();
        process_tests::create_fifo(&journal.root, LOCK_CONTENTION_FIFO);
        let guard = ConfirmedMachineDesireAdmissionGuard::new(
            journal.clone(),
            "default",
            exact.clone(),
            execution_provider(),
        )
        .unwrap();
        let permit = guard.acquire(&desire_request()).await.unwrap();
        let contention_armed = journal.root.join(LOCK_CONTENTION_ARMED);
        std::fs::write(&contention_armed, b"armed").unwrap();
        let barriers = ConfirmedMachineStopBarrierAuthority::new(journal);
        let contention_root = root
            .path()
            .join("networks")
            .join(super::super::STORE_DIRECTORY);
        let contention = std::thread::spawn(move || {
            process_tests::await_fifo(&contention_root, LOCK_CONTENTION_FIFO)
        });
        let stop =
            tokio::spawn(
                async move { barriers.claim_effect_free_barrier("default", &exact).await },
            );

        contention
            .join()
            .expect("contention observer should not panic")
            .expect("stop must make one actual lock-contention observation");
        std::fs::remove_file(&contention_armed).unwrap();
        assert!(
            !stop.is_finished(),
            "stop must wait while the Engine desire permit owns the provider lock"
        );
        drop(permit);
        assert!(stop.await.unwrap().is_ok());
    }

    #[test]
    fn restart_witness_replay_authenticates_the_persisted_source_execution() {
        let intent = process_tests::running_intent();
        let key = process_tests::desire_request().key().clone();
        let source =
            WorkloadExecutionReference::for_restart_epoch(&intent, WorkloadRestartEpoch::new(1));
        let target =
            WorkloadExecutionReference::for_restart_epoch(&intent, WorkloadRestartEpoch::new(2));
        let build =
            |execution: WorkloadExecutionReference,
             restart_source_execution: Option<WorkloadExecutionReference>| {
                super::super::ConfirmedMachineRetirementWitness {
                    machine_name: "default".to_owned(),
                    tenant_id: key.tenant_id().clone(),
                    workload_key: key.clone(),
                    sandbox_id: nimbus::SandboxId::new(execution.execution_id().as_str()),
                    execution_provider_id: intent.source().execution_provider_id().clone(),
                    execution,
                    restart_source_execution,
                    generation: intent.generation(),
                    desired_digest: intent.desired_digest(),
                    source_digest: intent.source().source_digest(),
                    network_plan_digest: intent.network().digest(),
                    forwarder_authority: authority("machine-provider", 7),
                    expected_guest_bindings: Vec::new(),
                    progress: ConfirmedMachinePublicationRetirementProgress::Active,
                }
            };
        let current = build(source.clone(), None);
        let candidate = build(target.clone(), Some(source));
        current
            .authenticate_restart_transition(&candidate)
            .expect("the first exact source-to-target transition should authenticate");
        candidate
            .authenticate_restart_transition(&candidate)
            .expect("exact replay must authenticate its persisted source execution");

        let successor = build(
            WorkloadExecutionReference::for_restart_epoch(&intent, WorkloadRestartEpoch::new(3)),
            Some(target.clone()),
        );
        candidate
            .authenticate_restart_transition(&successor)
            .expect("a later exact restart must advance from the current execution");

        let crossed_source =
            WorkloadExecutionReference::for_restart_epoch(&intent, WorkloadRestartEpoch::new(0));
        let crossed = build(target, Some(crossed_source));
        crossed
            .validate()
            .expect("the crossed source is intrinsically valid and precedes the target");
        assert!(
            candidate.authenticate_restart_transition(&crossed).is_err(),
            "same-target replay with a different source execution must fail before any effect"
        );
    }

    #[test]
    fn machine_stop_barrier_rejects_truncated_version_checksum_and_digest_corruption() {
        let root = TempDir::new().unwrap();
        let journal = ConfirmedMachinePublicationJournal::open(root.path()).unwrap();
        let exact = authority("machine-provider", 7);
        journal
            .claim_machine_stop_barrier("default", &exact)
            .unwrap();
        let original = std::fs::read(&journal.state_path).unwrap();

        let assert_corrupt = |bytes: Vec<u8>| {
            std::fs::write(&journal.state_path, &bytes).unwrap();
            assert_eq!(
                journal.claim_machine_stop_barrier("default", &exact),
                Err(MachineStopBarrierAuthorityError::Corrupt)
            );
            assert_eq!(
                std::fs::read(&journal.state_path).unwrap(),
                bytes,
                "inspection of corrupt barrier evidence must be byte-stable"
            );
        };

        assert_corrupt(original[..original.len() / 2].to_vec());

        let mut version: ConfirmedMachinePublicationEnvelope =
            serde_json::from_slice(&original).unwrap();
        version.format_version += 1;
        assert_corrupt(serde_json::to_vec_pretty(&version).unwrap());

        let mut checksum: ConfirmedMachinePublicationEnvelope =
            serde_json::from_slice(&original).unwrap();
        checksum.checksum = "0".repeat(64);
        assert_corrupt(serde_json::to_vec_pretty(&checksum).unwrap());

        let mut digest: ConfirmedMachinePublicationEnvelope =
            serde_json::from_slice(&original).unwrap();
        digest.body.stop_barriers[0].digest =
            MachineStopBarrierDigest::new("0".repeat(64)).unwrap();
        digest.checksum = envelope_checksum(
            &digest.magic,
            digest.format_version,
            digest.revision,
            &digest.body,
        )
        .unwrap();
        assert_corrupt(serde_json::to_vec_pretty(&digest).unwrap());
    }

    #[tokio::test]
    async fn stop_may_exist_retains_exact_barrier_and_fences_every_admission() {
        let root = TempDir::new().unwrap();
        let journal = ConfirmedMachinePublicationJournal::open(root.path()).unwrap();
        let exact = authority("machine-provider", 7);
        let claimed = journal
            .claim_machine_stop_barrier("default", &exact)
            .unwrap();
        let stop_may_exist = journal
            .begin_physical_machine_stop(claimed.barrier())
            .unwrap();
        let durable = std::fs::read(&journal.state_path).unwrap();

        assert!(matches!(
            journal.clear_effect_free_machine_stop_barrier(&stop_may_exist),
            Err(Error::Conflict { .. })
        ));
        assert_eq!(std::fs::read(&journal.state_path).unwrap(), durable);

        let guard = ConfirmedMachineDesireAdmissionGuard::new(
            journal.clone(),
            "default",
            exact.clone(),
            execution_provider(),
        )
        .unwrap();
        assert!(matches!(
            guard.acquire(&desire_request()).await,
            Err(WorkloadDesireAdmissionError::Fenced)
        ));
        assert_eq!(std::fs::read(&journal.state_path).unwrap(), durable);

        let barriers = ConfirmedMachineStopBarrierAuthority::new(journal.clone());
        assert!(matches!(
            barriers.claim_effect_free_barrier("default", &exact).await,
            Err(MachineStopBarrierAuthorityError::Ambiguous)
        ));
        assert_eq!(std::fs::read(&journal.state_path).unwrap(), durable);

        assert_eq!(
            journal.claim_machine_stop_barrier("default", &authority("machine-provider", 8)),
            Err(MachineStopBarrierAuthorityError::Stale)
        );
        assert_eq!(std::fs::read(&journal.state_path).unwrap(), durable);
        assert_eq!(
            journal.claim_machine_stop_barrier("default", &authority("crossed-provider", 7)),
            Err(MachineStopBarrierAuthorityError::Crossed)
        );
        assert_eq!(std::fs::read(&journal.state_path).unwrap(), durable);
    }
}

#[cfg(test)]
mod process_tests;
