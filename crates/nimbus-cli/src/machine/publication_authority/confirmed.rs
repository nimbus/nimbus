//! Exact parent-host authority for compute-confirmed machine publication.
//!
//! The legacy service publication store cannot represent a canonical compute
//! command. This journal deliberately uses a separate format and directory. It
//! records the exact command, parent-issued forwarder authority, and complete
//! listener/lease batch before either a parent lease or Machine API effect can
//! be treated as belonging to the forwarded provision attempt.

use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt as _;
use nimbus::{Error, SandboxId, SandboxPortBinding};
use nimbus_core::TenantId;
use nimbus_machine::{
    MachineForwarderAuthority,
    api::{MachineApiWorkloadProvisionCommandEnvelope, MachineApiWorkloadRestartCommandEnvelope},
};
use nimbus_network::{
    ListenerId, NetworkLeaseEpoch, NetworkProviderHandle, NetworkProviderId,
    NetworkResourceGeneration, PortBindClaim, PortBindRealm, PortBindingProvenance,
    PortBindingSpec, PortBoundEndpoint, PortExposure, PortLeaseAccounting, PortLeaseBinding,
    PortLeaseFence, PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, WorkloadDesiredDigest, WorkloadExecutionReference,
    WorkloadGeneration, WorkloadNetworkPortRequestMode, WorkloadProvisionAttemptId,
    WorkloadProvisionCommandMode, WorkloadProvisionDispatchEpoch, WorkloadProvisionProviderTarget,
    WorkloadProvisionSourceDigest, WorkloadProvisionStep, WorkloadSagaId, WorkloadSagaKey,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{
    create_owner_directory, lock_is_contended, machine_host_bind_target, open_owner_file,
    remove_file_if_exists,
};

const STORE_DIRECTORY: &str = "machine-provision-publications";
const STATE_FILE: &str = "confirmed.json";
const LOCK_FILE: &str = "authority.lock";
const STAGE_FILE: &str = ".confirmed.stage";
const FORMAT_MAGIC: &str = "nimbus-confirmed-machine-publications";
const FORMAT_VERSION: u32 = 2;
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const LOCK_RETRY: Duration = Duration::from_millis(10);

#[derive(Clone)]
pub(crate) struct ConfirmedMachinePublicationJournal {
    root: PathBuf,
    state_path: PathBuf,
    lock_path: PathBuf,
    stage_path: PathBuf,
}

impl ConfirmedMachinePublicationJournal {
    pub(crate) fn authenticate_retirement_witness(
        &self,
        command: &MachineApiWorkloadProvisionCommandEnvelope,
        authority: &MachineForwarderAuthority,
    ) -> Result<(), Error> {
        let candidate = ConfirmedMachineRetirementWitness::new(command, authority)?;
        self.mutate(|body| {
            if let Some(existing) = body
                .retirement_witnesses
                .iter()
                .find(|existing| existing.sandbox_id == candidate.sandbox_id)
            {
                if existing.retired {
                    return Err(Error::conflict(
                        "machine workload command cannot resurrect retired execution authority",
                    ));
                }
                return existing.authenticate(&candidate);
            }
            body.retirement_witnesses.push(candidate);
            body.retirement_witnesses
                .sort_by(|left, right| left.sandbox_id.as_str().cmp(right.sandbox_id.as_str()));
            Ok(())
        })
    }

    pub(crate) fn open(parent_network_state_root: &Path) -> Result<Self, Error> {
        let root = parent_network_state_root
            .join("networks")
            .join(STORE_DIRECTORY);
        create_owner_directory(&root)?;
        let store = Self {
            state_path: root.join(STATE_FILE),
            lock_path: root.join(LOCK_FILE),
            stage_path: root.join(STAGE_FILE),
            root,
        };
        store.with_body(|_| Ok(()))?;
        Ok(store)
    }

    /// Authenticate or durably stage the complete parent publication fence.
    ///
    /// Exact replay adopts the same record. One execute command and its later
    /// canonical inspection command may share an epoch. A higher epoch is
    /// admitted only after this journal recorded exact provider absence.
    pub(crate) fn authenticate_or_stage(
        &self,
        command: &MachineApiWorkloadProvisionCommandEnvelope,
        authority: &MachineForwarderAuthority,
        members: &[ConfirmedMachinePublicationMember],
    ) -> Result<(), Error> {
        let candidate = ConfirmedMachinePublicationRecord::new(command, authority, members)?;
        self.mutate(|body| {
            let related = body
                .records
                .iter()
                .filter(|record| {
                    record.saga_id == candidate.saga_id && record.step == candidate.step
                })
                .collect::<Vec<_>>();
            if let Some(newer) = related
                .iter()
                .find(|record| record.dispatch_epoch > candidate.dispatch_epoch)
            {
                return Err(Error::conflict(format!(
                    "machine publication command epoch {} is stale behind durable epoch {}",
                    candidate.dispatch_epoch.as_u64(),
                    newer.dispatch_epoch.as_u64()
                )));
            }
            if let Some(existing) = related
                .iter()
                .find(|record| record.dispatch_epoch == candidate.dispatch_epoch)
            {
                if existing.retired {
                    return Err(Error::conflict(
                        "machine publication command cannot resurrect retired parent authority",
                    ));
                }
                existing.authenticate_common(&candidate)?;
                let existing = body
                    .records
                    .iter_mut()
                    .find(|record| {
                        record.saga_id == candidate.saga_id
                            && record.step == candidate.step
                            && record.dispatch_epoch == candidate.dispatch_epoch
                    })
                    .expect("the immutable lookup found this exact publication record");
                existing.add_command(command.clone())?;
                return Ok(());
            }
            if let Some(previous) = related.iter().max_by_key(|record| record.dispatch_epoch) {
                if previous.retired {
                    return Err(Error::conflict(
                        "machine publication retry cannot replace retired parent authority",
                    ));
                }
                previous.authenticate_retry(&candidate)?;
                let expected = previous
                    .dispatch_epoch
                    .as_u64()
                    .checked_add(1)
                    .ok_or_else(|| Error::conflict("machine publication epoch exhausted"))?;
                if candidate.dispatch_epoch.as_u64() != expected {
                    return Err(Error::conflict(format!(
                        "machine publication command skipped epoch {expected}"
                    )));
                }
                if previous.observations.last()
                    != Some(&ConfirmedMachinePublicationObservation::Absent)
                {
                    return Err(Error::conflict(
                        "machine publication retry lacks exact prior absence evidence",
                    ));
                }
            }
            body.records.push(candidate);
            body.records.sort_by(|left, right| {
                (&left.saga_id, step_order(left.step), left.dispatch_epoch).cmp(&(
                    &right.saga_id,
                    step_order(right.step),
                    right.dispatch_epoch,
                ))
            });
            Ok(())
        })
    }

    /// Cross the durable request barrier before any Machine API byte is sent.
    pub(crate) fn commit_before_machine_api(
        &self,
        command: &MachineApiWorkloadProvisionCommandEnvelope,
        authority: &MachineForwarderAuthority,
        members: &[ConfirmedMachinePublicationMember],
    ) -> Result<(), Error> {
        let candidate = ConfirmedMachinePublicationRecord::new(command, authority, members)?;
        self.mutate(|body| {
            let record = exact_record_mut(body, &candidate)?;
            record.authenticate_common(&candidate)?;
            record.authenticate_command(command)?;
            record.machine_api_committed = true;
            Ok(())
        })
    }

    /// Retain the latest exact provider observation without erasing history.
    pub(crate) fn record_observation(
        &self,
        command: &MachineApiWorkloadProvisionCommandEnvelope,
        authority: &MachineForwarderAuthority,
        members: &[ConfirmedMachinePublicationMember],
        observation: ConfirmedMachinePublicationObservation,
    ) -> Result<(), Error> {
        let candidate = ConfirmedMachinePublicationRecord::new(command, authority, members)?;
        self.mutate(|body| {
            let record = exact_record_mut(body, &candidate)?;
            record.authenticate_common(&candidate)?;
            record.authenticate_command(command)?;
            if !record.machine_api_committed {
                return Err(Error::PreconditionFailed(
                    "machine publication observation precedes the durable Machine API barrier"
                        .to_owned(),
                ));
            }
            if record.observations.last() != Some(&observation) {
                record.observations.push(observation);
            }
            Ok(())
        })
    }

    pub(crate) fn retirement_for(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<Option<ConfirmedMachinePublicationRetirement>, Error> {
        self.with_body(|body| {
            let witness = body
                .retirement_witnesses
                .iter()
                .find(|witness| witness.sandbox_id == *sandbox_id);
            let related = body
                .records
                .iter()
                .filter(|record| record.execution.execution_id().as_str() == sandbox_id.as_str())
                .collect::<Vec<_>>();
            let Some(first) = related.first() else {
                return Ok(
                    witness.map(|witness| ConfirmedMachinePublicationRetirement {
                        tenant_id: witness.tenant_id.clone(),
                        sandbox_id: witness.sandbox_id.clone(),
                        forwarder_authority: witness.forwarder_authority.clone(),
                        expected_guest_bindings: witness.expected_guest_bindings.clone(),
                        members: Vec::new(),
                        retired: witness.retired,
                    }),
                );
            };
            if related.iter().any(|record| {
                record.workload_key.tenant_id() != first.workload_key.tenant_id()
                    || record.forwarder_authority != first.forwarder_authority
                    || !same_stable_members(&record.members, &first.members)
                    || record.retired != first.retired
            }) {
                return Err(Error::PreconditionFailed(
                    "confirmed machine publication retirement records are crossed".to_owned(),
                ));
            }
            let Some(witness) = witness else {
                return Err(Error::PreconditionFailed(
                    "confirmed machine publication lacks exact retirement authority".to_owned(),
                ));
            };
            if witness.tenant_id != *first.workload_key.tenant_id()
                || witness.forwarder_authority != first.forwarder_authority
                || witness.retired != first.retired
                || first.commands.iter().any(|command| {
                    !matches!(
                        canonical_machine_guest_bindings(command.compiled_network_plan()),
                        Ok(bindings) if bindings == witness.expected_guest_bindings
                    )
                })
            {
                return Err(Error::PreconditionFailed(
                    "confirmed machine publication lacks exact retirement authority".to_owned(),
                ));
            }
            Ok(Some(ConfirmedMachinePublicationRetirement {
                tenant_id: first.workload_key.tenant_id().clone(),
                sandbox_id: sandbox_id.clone(),
                forwarder_authority: first.forwarder_authority.clone(),
                expected_guest_bindings: witness.expected_guest_bindings.clone(),
                members: first.members.clone(),
                retired: first.retired,
            }))
        })
    }

    pub(crate) fn mark_retired(
        &self,
        retirement: &ConfirmedMachinePublicationRetirement,
    ) -> Result<(), Error> {
        self.mutate(|body| {
            let mut matched = false;
            for witness in body
                .retirement_witnesses
                .iter_mut()
                .filter(|witness| witness.sandbox_id == retirement.sandbox_id)
            {
                matched = true;
                if witness.tenant_id != retirement.tenant_id
                    || witness.forwarder_authority != retirement.forwarder_authority
                    || witness.expected_guest_bindings != retirement.expected_guest_bindings
                {
                    return Err(Error::conflict(
                        "machine retirement is crossed with durable execution authority",
                    ));
                }
                witness.retired = true;
            }
            for record in body.records.iter_mut().filter(|record| {
                record.execution.execution_id().as_str() == retirement.sandbox_id.as_str()
            }) {
                matched = true;
                if record.workload_key.tenant_id() != &retirement.tenant_id
                    || record.forwarder_authority != retirement.forwarder_authority
                    || !same_stable_members(&record.members, &retirement.members)
                {
                    return Err(Error::conflict(
                        "machine publication retirement is crossed with durable parent authority",
                    ));
                }
                record.retired = true;
            }
            if !matched {
                return Err(Error::NotFound(
                    "confirmed machine publication retirement is not durably staged".to_owned(),
                ));
            }
            Ok(())
        })
    }

    fn mutate<T>(
        &self,
        operation: impl FnOnce(&mut ConfirmedMachinePublicationBody) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let lock = self.acquire_lock()?;
        self.validate_directory_entries()?;
        remove_file_if_exists(&self.stage_path)?;
        let envelope = self.load_envelope()?;
        let mut body = envelope.body.clone();
        body.validate()?;
        let output = operation(&mut body)?;
        body.validate()?;
        if body != envelope.body {
            let revision = envelope.revision.checked_add(1).ok_or_else(|| {
                Error::PreconditionFailed(
                    "confirmed machine publication revision exhausted".to_owned(),
                )
            })?;
            self.publish(revision, &body)?;
        }
        drop(lock);
        Ok(output)
    }

    fn with_body<T>(
        &self,
        operation: impl FnOnce(&ConfirmedMachinePublicationBody) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let lock = self.acquire_lock()?;
        self.validate_directory_entries()?;
        remove_file_if_exists(&self.stage_path)?;
        let envelope = self.load_envelope()?;
        envelope.body.validate()?;
        let output = operation(&envelope.body)?;
        drop(lock);
        Ok(output)
    }

    fn acquire_lock(&self) -> Result<ConfirmedMachinePublicationLock, Error> {
        let file = open_owner_file(&self.lock_path, true)?;
        let deadline = Instant::now() + LOCK_TIMEOUT;
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(ConfirmedMachinePublicationLock { file }),
                Err(error) if lock_is_contended(&error) && Instant::now() < deadline => {
                    thread::sleep(LOCK_RETRY);
                }
                Err(error) if lock_is_contended(&error) => {
                    return Err(Error::ResourceExhausted(format!(
                        "timed out acquiring confirmed machine publication lock {}",
                        self.lock_path.display()
                    )));
                }
                Err(error) => {
                    return Err(io_error(
                        "lock confirmed machine publication authority",
                        &self.lock_path,
                        error,
                    ));
                }
            }
        }
    }

    fn validate_directory_entries(&self) -> Result<(), Error> {
        for entry in fs::read_dir(&self.root).map_err(|error| {
            io_error(
                "read confirmed machine publication directory",
                &self.root,
                error,
            )
        })? {
            let entry = entry.map_err(|error| {
                io_error(
                    "read confirmed machine publication entry",
                    &self.root,
                    error,
                )
            })?;
            let name = entry.file_name();
            if name != STATE_FILE && name != LOCK_FILE && name != STAGE_FILE {
                return Err(Error::PreconditionFailed(format!(
                    "confirmed machine publication directory {} contains an unknown entry",
                    self.root.display()
                )));
            }
        }
        Ok(())
    }

    fn load_envelope(&self) -> Result<ConfirmedMachinePublicationEnvelope, Error> {
        let bytes = match fs::read(&self.state_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ConfirmedMachinePublicationEnvelope::empty());
            }
            Err(error) => {
                return Err(io_error(
                    "read confirmed machine publication authority",
                    &self.state_path,
                    error,
                ));
            }
        };
        let envelope: ConfirmedMachinePublicationEnvelope = serde_json::from_slice(&bytes)
            .map_err(|error| {
                Error::PreconditionFailed(format!(
                    "confirmed machine publication authority {} is not a strict envelope: {error}",
                    self.state_path.display()
                ))
            })?;
        envelope.validate(&self.state_path)?;
        Ok(envelope)
    }

    fn publish(&self, revision: u64, body: &ConfirmedMachinePublicationBody) -> Result<(), Error> {
        let envelope = ConfirmedMachinePublicationEnvelope::new(revision, body.clone())?;
        let bytes = serde_json::to_vec_pretty(&envelope).map_err(|error| {
            Error::Internal(format!(
                "failed to encode confirmed machine publication authority: {error}"
            ))
        })?;
        remove_file_if_exists(&self.stage_path)?;
        let mut stage = open_owner_file(&self.stage_path, false)?;
        stage.write_all(&bytes).map_err(|error| {
            io_error(
                "write confirmed machine publication stage",
                &self.stage_path,
                error,
            )
        })?;
        stage.sync_all().map_err(|error| {
            io_error(
                "sync confirmed machine publication stage",
                &self.stage_path,
                error,
            )
        })?;
        fs::rename(&self.stage_path, &self.state_path).map_err(|error| {
            io_error(
                "replace confirmed machine publication authority",
                &self.state_path,
                error,
            )
        })?;
        File::open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                io_error(
                    "sync confirmed machine publication directory",
                    &self.root,
                    error,
                )
            })
    }
}

/// Exact listener and lease authority retained for one forwarded command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ConfirmedMachinePublicationMember {
    listener_id: ListenerId,
    binding: SandboxPortBinding,
    request: PortLeaseRequest,
    bind_claim: PortBindClaim,
    expected_binding: PortLeaseBinding,
}

impl ConfirmedMachinePublicationMember {
    pub(crate) fn new(
        listener_id: ListenerId,
        binding: SandboxPortBinding,
        request: PortLeaseRequest,
        bind_claim: PortBindClaim,
        expected_binding: PortLeaseBinding,
    ) -> Self {
        Self {
            listener_id,
            binding,
            request,
            bind_claim,
            expected_binding,
        }
    }

    pub(crate) fn request(&self) -> &PortLeaseRequest {
        &self.request
    }

    pub(crate) fn listener_id(&self) -> &ListenerId {
        &self.listener_id
    }

    #[cfg(test)]
    pub(crate) fn binding(&self) -> &SandboxPortBinding {
        &self.binding
    }

    pub(crate) fn bind_claim(&self) -> &PortBindClaim {
        &self.bind_claim
    }

    pub(crate) fn expected_binding(&self) -> &PortLeaseBinding {
        &self.expected_binding
    }

    #[cfg(test)]
    pub(crate) fn replace_binding_for_test(&mut self, binding: SandboxPortBinding) {
        self.binding = binding;
    }
}

pub(crate) fn canonical_machine_publication_members(
    command: &MachineApiWorkloadProvisionCommandEnvelope,
    authority: &MachineForwarderAuthority,
) -> Result<Vec<ConfirmedMachinePublicationMember>, Error> {
    canonical_machine_publication_members_for(
        command.compiled_network_plan(),
        command.machine_provider_generation(),
        authority,
    )
}

pub(crate) fn canonical_machine_restart_publication_members(
    command: &MachineApiWorkloadRestartCommandEnvelope,
    authority: &MachineForwarderAuthority,
) -> Result<Vec<ConfirmedMachinePublicationMember>, Error> {
    canonical_machine_publication_members_for(
        command.compiled_network_plan(),
        command.machine_provider_generation(),
        authority,
    )
}

fn canonical_machine_publication_members_for(
    compiled: &CompiledWorkloadNetworkPlan,
    machine_provider_generation: NetworkResourceGeneration,
    authority: &MachineForwarderAuthority,
) -> Result<Vec<ConfirmedMachinePublicationMember>, Error> {
    if authority.generation() != machine_provider_generation {
        return Err(Error::PreconditionFailed(
            "canonical machine publication authority generation is crossed".to_owned(),
        ));
    }
    let content = compiled.content();
    let plan = compiled.plan();
    let guest_bindings = canonical_machine_guest_bindings(compiled)?;
    let mut members = Vec::with_capacity(content.listeners().len());
    for (blueprint, binding) in content.listeners().iter().zip(guest_bindings) {
        let WorkloadNetworkPortRequestMode::Exact { port } = blueprint.port_request() else {
            return Err(Error::PreconditionFailed(
                "forwarded machine publication requires an exact canonical host port".to_owned(),
            ));
        };
        let target = machine_host_bind_target(blueprint.desired_host_address())?;
        let request = PortLeaseRequest::new(
            blueprint.port_lease_id().clone(),
            blueprint.listener_id().clone().into(),
            Some(content.identity().tenant_id().clone()),
            PortLeaseFence::new(plan.generation(), NetworkLeaseEpoch::new(1)),
            PortLeaseAccounting::TenantPublished,
            PortPublicationIntent::host(blueprint.desired_host_address()),
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                target.clone(),
                canonical_port_exposure(blueprint.desired_host_address()),
                PortRequestMode::Exact(port),
            ),
        )
        .with_plan_id(plan.plan_id().clone());
        // Publish execution and later observation are distinct compute
        // attempts over the same concrete parent binding. Bind authority is
        // therefore stable for the forwarder generation and compiled listener;
        // the provider journals separately fence each compute attempt/epoch.
        let provider_attempt = NetworkProviderHandle::new(
            authority.provider_instance().provider_id().clone(),
            format!(
                "confirmed-publication:{}:{}:{}",
                authority.generation().as_u64(),
                plan.plan_id(),
                blueprint.listener_id()
            ),
        )
        .map_err(|error| {
            Error::PreconditionFailed(format!(
                "canonical machine publication provider attempt is invalid: {error}"
            ))
        })?;
        let endpoint = PortBoundEndpoint::new(PortProtocol::Tcp, PortBindRealm::Host, target, port)
            .map_err(|error| {
                Error::PreconditionFailed(format!(
                    "canonical machine publication endpoint is invalid: {error}"
                ))
            })?;
        members.push(ConfirmedMachinePublicationMember::new(
            blueprint.listener_id().clone(),
            binding,
            request,
            PortBindClaim::new(provider_attempt),
            PortLeaseBinding::new(
                endpoint,
                PortBindingProvenance::NimbusOwned,
                authority.provider_instance().clone(),
            ),
        ));
    }
    members.sort_by(|left, right| left.request.lease_id().cmp(right.request.lease_id()));
    Ok(members)
}

fn canonical_machine_guest_bindings(
    compiled: &CompiledWorkloadNetworkPlan,
) -> Result<Vec<SandboxPortBinding>, Error> {
    compiled
        .content()
        .listeners()
        .iter()
        .map(|blueprint| {
            let WorkloadNetworkPortRequestMode::Exact { port } = blueprint.port_request() else {
                return Err(Error::PreconditionFailed(
                    "forwarded machine publication requires an exact canonical host port"
                        .to_owned(),
                ));
            };
            let guest_port = blueprint.guest_port().ok_or_else(|| {
                Error::PreconditionFailed(
                    "forwarded machine publication requires a canonical guest port".to_owned(),
                )
            })?;
            Ok(SandboxPortBinding {
                name: blueprint.name().to_owned(),
                protocol: blueprint.protocol(),
                host_address: blueprint.desired_host_address(),
                host_port: port.get(),
                guest_port,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfirmedMachinePublicationRetirement {
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    forwarder_authority: MachineForwarderAuthority,
    expected_guest_bindings: Vec<SandboxPortBinding>,
    members: Vec<ConfirmedMachinePublicationMember>,
    retired: bool,
}

impl ConfirmedMachinePublicationRetirement {
    pub(crate) fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub(crate) fn sandbox_id(&self) -> &SandboxId {
        &self.sandbox_id
    }

    pub(crate) fn forwarder_authority(&self) -> &MachineForwarderAuthority {
        &self.forwarder_authority
    }

    pub(crate) fn expected_guest_bindings(&self) -> &[SandboxPortBinding] {
        &self.expected_guest_bindings
    }

    pub(crate) fn members(&self) -> &[ConfirmedMachinePublicationMember] {
        &self.members
    }

    pub(crate) const fn is_retired(&self) -> bool {
        self.retired
    }

    #[cfg(test)]
    pub(crate) fn replace_expected_guest_bindings_for_test(
        &mut self,
        bindings: Vec<SandboxPortBinding>,
    ) {
        self.expected_guest_bindings = bindings;
    }
}

/// Durable classification of the latest exact Machine API observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConfirmedMachinePublicationObservation {
    Succeeded,
    DefiniteFailure,
    Absent,
    InProgress,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ConfirmedMachineRetirementWitness {
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    execution: WorkloadExecutionReference,
    generation: WorkloadGeneration,
    source_digest: WorkloadProvisionSourceDigest,
    forwarder_authority: MachineForwarderAuthority,
    expected_guest_bindings: Vec<SandboxPortBinding>,
    retired: bool,
}

impl ConfirmedMachineRetirementWitness {
    fn new(
        command: &MachineApiWorkloadProvisionCommandEnvelope,
        authority: &MachineForwarderAuthority,
    ) -> Result<Self, Error> {
        Ok(Self {
            tenant_id: command.claim().attempt().key().tenant_id().clone(),
            sandbox_id: SandboxId::new(command.execution().execution_id().as_str()),
            execution: command.execution().clone(),
            generation: command.generation(),
            source_digest: command.source_digest(),
            forwarder_authority: authority.clone(),
            expected_guest_bindings: canonical_machine_guest_bindings(
                command.compiled_network_plan(),
            )?,
            retired: false,
        })
    }

    fn authenticate(&self, candidate: &Self) -> Result<(), Error> {
        if self == candidate {
            Ok(())
        } else {
            Err(Error::conflict(
                "machine command is crossed with durable workload retirement authority",
            ))
        }
    }

    fn validate(&self) -> Result<(), Error> {
        if self.sandbox_id.as_str() != self.execution.execution_id().as_str()
            || self.generation != self.execution.generation()
            || self
                .expected_guest_bindings
                .iter()
                .enumerate()
                .any(|(index, binding)| self.expected_guest_bindings[..index].contains(binding))
        {
            return Err(Error::PreconditionFailed(
                "machine retirement witness carries crossed execution identity or duplicate guest bindings"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct ConfirmedMachinePublicationRecord {
    saga_id: WorkloadSagaId,
    workload_key: WorkloadSagaKey,
    step: WorkloadProvisionStep,
    attempt_id: WorkloadProvisionAttemptId,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    execution: WorkloadExecutionReference,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source_digest: WorkloadProvisionSourceDigest,
    network_plan_digest: nimbus_network::NetworkPlanDigest,
    provider_target: WorkloadProvisionProviderTarget,
    provider_target_digest: String,
    forwarder_authority: MachineForwarderAuthority,
    forwarder_provider_id: NetworkProviderId,
    forwarder_generation: NetworkResourceGeneration,
    members: Vec<ConfirmedMachinePublicationMember>,
    commands: Vec<MachineApiWorkloadProvisionCommandEnvelope>,
    machine_api_committed: bool,
    observations: Vec<ConfirmedMachinePublicationObservation>,
    retired: bool,
}

impl ConfirmedMachinePublicationRecord {
    fn new(
        command: &MachineApiWorkloadProvisionCommandEnvelope,
        authority: &MachineForwarderAuthority,
        members: &[ConfirmedMachinePublicationMember],
    ) -> Result<Self, Error> {
        let attempt = command.claim().attempt();
        let provider_target = serde_json::to_vec(command.provider_target()).map_err(|error| {
            Error::Internal(format!(
                "failed to encode confirmed machine provider target: {error}"
            ))
        })?;
        let record = Self {
            saga_id: attempt.saga_id().clone(),
            workload_key: attempt.key().clone(),
            step: attempt.step(),
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            execution: command.execution().clone(),
            generation: command.generation(),
            desired_digest: command.desired_digest(),
            source_digest: command.source_digest(),
            network_plan_digest: command.network_plan_digest(),
            provider_target: command.provider_target().clone(),
            provider_target_digest: format!("{:x}", Sha256::digest(provider_target)),
            forwarder_authority: authority.clone(),
            forwarder_provider_id: authority.provider_instance().provider_id().clone(),
            forwarder_generation: authority.generation(),
            members: members.to_vec(),
            commands: vec![command.clone()],
            machine_api_committed: false,
            observations: Vec::new(),
            retired: false,
        };
        record.validate()?;
        Ok(record)
    }

    fn add_command(
        &mut self,
        command: MachineApiWorkloadProvisionCommandEnvelope,
    ) -> Result<(), Error> {
        if self.commands.iter().any(|existing| existing == &command) {
            return Ok(());
        }
        if self
            .commands
            .iter()
            .any(|existing| existing.mode() == command.mode())
        {
            return Err(Error::conflict(format!(
                "machine publication epoch {} already records a different {:?} command",
                self.dispatch_epoch.as_u64(),
                command.mode()
            )));
        }
        self.commands.push(command);
        self.commands.sort_by_key(|command| match command.mode() {
            WorkloadProvisionCommandMode::Execute => 0_u8,
            WorkloadProvisionCommandMode::Inspect => 1_u8,
        });
        self.validate()
    }

    fn authenticate_command(
        &self,
        command: &MachineApiWorkloadProvisionCommandEnvelope,
    ) -> Result<(), Error> {
        if self.commands.iter().any(|existing| existing == command) {
            Ok(())
        } else {
            Err(Error::conflict(
                "Machine API operation is crossed with the durable canonical command",
            ))
        }
    }

    fn authenticate_common(&self, candidate: &Self) -> Result<(), Error> {
        if self.saga_id == candidate.saga_id
            && self.workload_key == candidate.workload_key
            && self.step == candidate.step
            && self.attempt_id == candidate.attempt_id
            && self.dispatch_epoch == candidate.dispatch_epoch
            && self.execution == candidate.execution
            && self.generation == candidate.generation
            && self.desired_digest == candidate.desired_digest
            && self.source_digest == candidate.source_digest
            && self.network_plan_digest == candidate.network_plan_digest
            && self.provider_target == candidate.provider_target
            && self.provider_target_digest == candidate.provider_target_digest
            && self.forwarder_authority == candidate.forwarder_authority
            && self.forwarder_provider_id == candidate.forwarder_provider_id
            && self.forwarder_generation == candidate.forwarder_generation
            && self.members == candidate.members
        {
            Ok(())
        } else {
            Err(Error::conflict(
                "machine publication command is crossed with durable parent authority",
            ))
        }
    }

    fn authenticate_retry(&self, candidate: &Self) -> Result<(), Error> {
        if self.saga_id == candidate.saga_id
            && self.workload_key == candidate.workload_key
            && self.step == candidate.step
            && self.attempt_id == candidate.attempt_id
            && self.execution == candidate.execution
            && self.generation == candidate.generation
            && self.desired_digest == candidate.desired_digest
            && self.source_digest == candidate.source_digest
            && self.network_plan_digest == candidate.network_plan_digest
            && self.provider_target == candidate.provider_target
            && self.provider_target_digest == candidate.provider_target_digest
            && self.forwarder_authority == candidate.forwarder_authority
            && self.forwarder_provider_id == candidate.forwarder_provider_id
            && self.forwarder_generation == candidate.forwarder_generation
            && same_stable_members(&self.members, &candidate.members)
        {
            Ok(())
        } else {
            Err(Error::conflict(
                "machine publication retry is crossed with durable parent authority",
            ))
        }
    }

    fn validate(&self) -> Result<(), Error> {
        if self.forwarder_provider_id != *self.forwarder_authority.provider_instance().provider_id()
            || self.forwarder_generation != self.forwarder_authority.generation()
        {
            return Err(Error::PreconditionFailed(
                "confirmed machine publication carries crossed forwarder authority".to_owned(),
            ));
        }
        let expected_target = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&self.provider_target).map_err(|error| {
                Error::Internal(format!(
                    "failed to encode durable machine provider target: {error}"
                ))
            })?)
        );
        if self.provider_target_digest != expected_target {
            return Err(Error::PreconditionFailed(
                "confirmed machine publication provider-target digest is corrupt".to_owned(),
            ));
        }
        if self.commands.is_empty() {
            return Err(Error::PreconditionFailed(
                "confirmed machine publication has no canonical command".to_owned(),
            ));
        }
        for command in &self.commands {
            let attempt = command.claim().attempt();
            if attempt.saga_id() != &self.saga_id
                || attempt.key() != &self.workload_key
                || attempt.step() != self.step
                || command.attempt_id() != &self.attempt_id
                || command.dispatch_epoch() != self.dispatch_epoch
                || command.execution() != &self.execution
                || command.generation() != self.generation
                || command.desired_digest() != self.desired_digest
                || command.source_digest() != self.source_digest
                || command.network_plan_digest() != self.network_plan_digest
                || command.provider_target() != &self.provider_target
                || command.machine_provider_generation() != self.forwarder_generation
            {
                return Err(Error::PreconditionFailed(
                    "confirmed machine publication command fence is corrupt".to_owned(),
                ));
            }
            self.validate_members_against_command(command)?;
        }
        let listener_ids = self
            .members
            .iter()
            .map(|member| &member.listener_id)
            .collect::<std::collections::BTreeSet<_>>();
        let lease_ids = self
            .members
            .iter()
            .map(|member| member.request.lease_id())
            .collect::<std::collections::BTreeSet<_>>();
        if listener_ids.len() != self.members.len() || lease_ids.len() != self.members.len() {
            return Err(Error::PreconditionFailed(
                "confirmed machine publication members are not unique".to_owned(),
            ));
        }
        for member in &self.members {
            if member.request.owner_id()
                != &nimbus_network::NetworkResourceId::from(member.listener_id.clone())
                || member.request.lease_id()
                    != &nimbus_network::PortLeaseId::for_listener(&member.listener_id)
                || member.bind_claim.provider_attempt().provider_id() != &self.forwarder_provider_id
                || member.expected_binding.provider_handle()
                    != self.forwarder_authority.provider_instance()
            {
                return Err(Error::PreconditionFailed(
                    "confirmed machine publication member authority is corrupt".to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn validate_members_against_command(
        &self,
        command: &MachineApiWorkloadProvisionCommandEnvelope,
    ) -> Result<(), Error> {
        let expected = canonical_machine_publication_members(command, &self.forwarder_authority)?;
        if self.members != expected {
            return Err(Error::PreconditionFailed(
                "confirmed machine publication members differ from complete canonical command authority"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

fn canonical_port_exposure(address: std::net::IpAddr) -> PortExposure {
    match address {
        address if address.is_loopback() => PortExposure::Loopback,
        std::net::IpAddr::V4(address) if address.is_private() || address.is_link_local() => {
            PortExposure::Private
        }
        std::net::IpAddr::V6(address)
            if address.is_unique_local() || address.is_unicast_link_local() =>
        {
            PortExposure::Private
        }
        _ => PortExposure::Public,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmedMachinePublicationBody {
    retirement_witnesses: Vec<ConfirmedMachineRetirementWitness>,
    records: Vec<ConfirmedMachinePublicationRecord>,
}

impl ConfirmedMachinePublicationBody {
    fn validate(&self) -> Result<(), Error> {
        for (index, witness) in self.retirement_witnesses.iter().enumerate() {
            witness.validate()?;
            if self.retirement_witnesses[..index]
                .iter()
                .any(|existing| existing.sandbox_id == witness.sandbox_id)
            {
                return Err(Error::PreconditionFailed(
                    "confirmed machine retirement authority contains duplicate execution identity"
                        .to_owned(),
                ));
            }
        }
        for (index, record) in self.records.iter().enumerate() {
            record.validate()?;
            let witness = self
                .retirement_witnesses
                .iter()
                .find(|witness| witness.execution == record.execution)
                .ok_or_else(|| {
                    Error::PreconditionFailed(
                        "confirmed machine publication lacks durable retirement authority"
                            .to_owned(),
                    )
                })?;
            if witness.tenant_id != *record.workload_key.tenant_id()
                || witness.generation != record.generation
                || witness.source_digest != record.source_digest
                || witness.forwarder_authority != record.forwarder_authority
                || witness.retired != record.retired
                || record.commands.iter().any(|command| {
                    !matches!(
                        canonical_machine_guest_bindings(command.compiled_network_plan()),
                        Ok(bindings) if bindings == witness.expected_guest_bindings
                    )
                })
            {
                return Err(Error::PreconditionFailed(
                    "confirmed machine publication is crossed with durable retirement authority"
                        .to_owned(),
                ));
            }
            if self.records[..index].iter().any(|existing| {
                existing.saga_id == record.saga_id
                    && existing.step == record.step
                    && existing.dispatch_epoch == record.dispatch_epoch
            }) {
                return Err(Error::PreconditionFailed(
                    "confirmed machine publication contains a duplicate epoch record".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ConfirmedMachinePublicationEnvelope {
    magic: String,
    format_version: u32,
    revision: u64,
    checksum: String,
    body: ConfirmedMachinePublicationBody,
}

impl ConfirmedMachinePublicationEnvelope {
    fn empty() -> Self {
        let body = ConfirmedMachinePublicationBody::default();
        Self {
            magic: FORMAT_MAGIC.to_owned(),
            format_version: FORMAT_VERSION,
            revision: 0,
            checksum: checksum(&body).expect("the empty confirmed publication body encodes"),
            body,
        }
    }

    fn new(revision: u64, body: ConfirmedMachinePublicationBody) -> Result<Self, Error> {
        Ok(Self {
            magic: FORMAT_MAGIC.to_owned(),
            format_version: FORMAT_VERSION,
            revision,
            checksum: checksum(&body)?,
            body,
        })
    }

    fn validate(&self, path: &Path) -> Result<(), Error> {
        if self.magic != FORMAT_MAGIC || self.format_version != FORMAT_VERSION {
            return Err(Error::PreconditionFailed(format!(
                "confirmed machine publication authority {} has unsupported format identity",
                path.display()
            )));
        }
        if self.checksum != checksum(&self.body)? {
            return Err(Error::PreconditionFailed(format!(
                "confirmed machine publication authority {} failed checksum validation",
                path.display()
            )));
        }
        Ok(())
    }
}

fn exact_record_mut<'a>(
    body: &'a mut ConfirmedMachinePublicationBody,
    candidate: &ConfirmedMachinePublicationRecord,
) -> Result<&'a mut ConfirmedMachinePublicationRecord, Error> {
    body.records
        .iter_mut()
        .find(|record| {
            record.saga_id == candidate.saga_id
                && record.step == candidate.step
                && record.dispatch_epoch == candidate.dispatch_epoch
        })
        .ok_or_else(|| {
            Error::NotFound(
                "confirmed machine publication command is not durably staged".to_owned(),
            )
        })
}

fn same_stable_members(
    left: &[ConfirmedMachinePublicationMember],
    right: &[ConfirmedMachinePublicationMember],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.listener_id == right.listener_id
                && left.binding == right.binding
                && left.request == right.request
                && left.expected_binding == right.expected_binding
        })
}

fn checksum(body: &ConfirmedMachinePublicationBody) -> Result<String, Error> {
    let bytes = serde_json::to_vec(body).map_err(|error| {
        Error::Internal(format!(
            "failed to encode confirmed machine publication checksum: {error}"
        ))
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

const fn step_order(step: WorkloadProvisionStep) -> u8 {
    match step {
        WorkloadProvisionStep::ReserveNetwork => 0,
        WorkloadProvisionStep::PrepareWorkload => 1,
        WorkloadProvisionStep::AttachNetwork => 2,
        WorkloadProvisionStep::InspectActivationPrerequisites => 3,
        WorkloadProvisionStep::ActivateWorkload => 4,
        WorkloadProvisionStep::InspectWorkloadReadiness => 5,
        WorkloadProvisionStep::Publish => 6,
        WorkloadProvisionStep::ObservePublication => 7,
    }
}

fn io_error(operation: &str, path: &Path, error: std::io::Error) -> Error {
    Error::Internal(format!("{operation} {}: {error}", path.display()))
}

struct ConfirmedMachinePublicationLock {
    file: File,
}

impl Drop for ConfirmedMachinePublicationLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
