//! Exact Container attachment detach and release adapters.

use std::cell::RefCell;

use crate::backends::CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY;
use crate::backends::oci::network::{
    AttachmentAuxiliaryDisposition, AttachmentReleaseActions,
    HostManagedAttachmentCommandInspection, HostManagedAttachmentCommandInspectionError,
    HostManagedAttachmentTeardownState, OciMachinePortForwarderConfig,
};
use crate::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandExecutionClaim,
    ProviderCommandJournalError, ProviderCommandObservation, ProviderCommandObservationKind,
    SandboxError, SandboxNetworkTeardownCommand, SandboxNetworkTeardownObservation,
    SandboxNetworkTeardownOperation,
};

use super::teardown::state::ContainerNetworkStopRequirementError;
use super::{
    ContainerLifecycleCoordinator, ContainerNetworkPublicationMode, ContainerSandboxBackend,
    ContainerSandboxManifest, ContainerStartMode, hostname_for,
};

mod forwarded;

#[derive(Clone, Copy)]
pub(super) enum NetworkTeardownComposition<'a> {
    HostManaged,
    Forwarded {
        expected_forwarder: &'a OciMachinePortForwarderConfig,
        prior_observation: &'a ProviderCommandObservation,
    },
}

type NetworkTeardownResult<T> = std::result::Result<T, NetworkTeardownAdapterError>;

#[derive(Debug)]
enum NetworkTeardownAdapterError {
    Definite { code: &'static str, message: String },
    Ambiguous { message: String },
}

impl ContainerSandboxBackend {
    /// Inspect independent provider and publication absence after an exact
    /// forwarded ReleaseNetwork result. This method is read-only.
    #[doc(hidden)]
    pub fn inspect_forwarded_network_release_absence_evidence(
        &self,
        command: &SandboxNetworkTeardownCommand,
        prior_observation: &ProviderCommandObservation,
        expected_forwarder: &OciMachinePortForwarderConfig,
    ) -> crate::Result<crate::SandboxNetworkReleaseAbsenceEvidence> {
        if command.operation() != SandboxNetworkTeardownOperation::Release {
            return Err(SandboxError::InvalidSpec {
                message: "forwarded release absence inspection requires ReleaseNetwork".to_owned(),
            });
        }
        let composition = NetworkTeardownComposition::Forwarded {
            expected_forwarder,
            prior_observation,
        };
        self.preflight_network_teardown_for_composition(command, composition)
            .map_err(|observation| SandboxError::OperationFailed {
                message: format!(
                    "forwarded release absence preflight did not authenticate: {observation:?}"
                ),
            })?;
        let snapshot = self.read_exact_network_manifest(command)?;
        let (_inspection, manifest) =
            super::runner::lock_current_inspection_for_backend(self, &snapshot)?;
        self.authenticate_network_teardown_manifest(command, &manifest, composition)
            .map_err(NetworkTeardownAdapterError::into_sandbox_error)?;
        self.require_execution_predecessor(command, &manifest, composition)
            .map_err(NetworkTeardownAdapterError::into_sandbox_error)?;
        self.require_forwarded_publication_absence_if_selected(&manifest, composition)
            .map_err(NetworkTeardownAdapterError::into_sandbox_error)?;
        manifest
            .network_teardown
            .forwarded_release_absence_evidence()
    }

    /// Authenticate exact durable network teardown authority without writes or effects.
    ///
    /// Compute calls this before it creates a provider-command claim. Execute
    /// repeats the same checks while holding its command stream, then
    /// reauthenticates the manifest after it acquires lifecycle authority.
    pub fn preflight_network_teardown_command(
        &self,
        command: &SandboxNetworkTeardownCommand,
    ) -> Result<(), SandboxNetworkTeardownObservation> {
        self.preflight_network_teardown_for_composition(
            command,
            NetworkTeardownComposition::HostManaged,
        )
    }

    pub(super) fn preflight_network_teardown_for_composition(
        &self,
        command: &SandboxNetworkTeardownCommand,
        composition: NetworkTeardownComposition<'_>,
    ) -> Result<(), SandboxNetworkTeardownObservation> {
        let result = (|| {
            let snapshot = self
                .read_exact_network_manifest(command)
                .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
            let journal = self
                .attempt_idempotency_journal()
                .map_err(|error| NetworkTeardownAdapterError::ambiguous(error.to_string()))?;
            self.authenticate_network_teardown_snapshot(command, &snapshot, &journal, composition)?;
            Ok(())
        })();
        result.map_err(NetworkTeardownAdapterError::into_observation)
    }

    /// Execute one exact attachment transition while its provider stream lock is held.
    pub fn execute_network_teardown_with_claim(
        &self,
        command: &SandboxNetworkTeardownCommand,
        execution_claim: ProviderCommandExecutionClaim,
    ) -> Result<ProviderCommandObservation, ProviderCommandJournalError> {
        if execution_claim.claim() != command.provider_claim()
            || execution_claim.observation().kind() != ProviderCommandObservationKind::Claimed
        {
            return Err(ProviderCommandJournalError::InvalidClaim {
                message: "Container network authorization crossed its provider command".to_owned(),
            });
        }
        let journal = self.attempt_idempotency_journal()?;
        let (_, provider_observation) =
            journal.execute_current_claim(execution_claim, |current_claim| {
                let observation = self.execute_network_teardown_inner(
                    command,
                    current_claim.observation(),
                    &journal,
                    NetworkTeardownComposition::HostManaged,
                );
                let kind = network_observation_kind(&observation);
                let failure_code = observation.failure_code().map(str::to_owned);
                let evidence = observation.evidence().to_vec();
                (observation, kind, failure_code, evidence)
            })?;
        Ok(provider_observation)
    }

    /// Read exact attachment progress without provider effects or durable writes.
    pub fn inspect_network_teardown_with_observation(
        &self,
        command: &SandboxNetworkTeardownCommand,
        provider_observation: &ProviderCommandObservation,
    ) -> SandboxNetworkTeardownObservation {
        if provider_observation.claim() != command.provider_claim()
            || !matches!(
                provider_observation.kind(),
                ProviderCommandObservationKind::Claimed
                    | ProviderCommandObservationKind::InProgress
                    | ProviderCommandObservationKind::Ambiguous
            )
        {
            return definite_failure(
                "sandbox_teardown_command_crossed",
                "Container network inspection authorization crossed its provider command",
            );
        }
        let journal = match self.attempt_idempotency_journal() {
            Ok(journal) => journal,
            Err(error) => return ambiguous(error.to_string()),
        };
        match journal.inspect_current_claim(provider_observation, |current| {
            self.inspect_network_teardown_inner(
                command,
                current,
                &journal,
                NetworkTeardownComposition::HostManaged,
            )
        }) {
            Ok(Ok(observation)) => observation,
            Ok(Err(error)) => error.into_observation(),
            Err(error) => ambiguous(error.to_string()),
        }
    }

    fn execute_network_teardown_inner(
        &self,
        command: &SandboxNetworkTeardownCommand,
        provider_observation: &ProviderCommandObservation,
        journal: &ProviderCommandAttemptJournal,
        composition: NetworkTeardownComposition<'_>,
    ) -> SandboxNetworkTeardownObservation {
        match self.execute_network_teardown_locked(
            command,
            provider_observation,
            journal,
            composition,
        ) {
            Ok(observation) => observation,
            Err(error) => error.into_observation(),
        }
    }

    fn execute_network_teardown_locked(
        &self,
        command: &SandboxNetworkTeardownCommand,
        provider_observation: &ProviderCommandObservation,
        journal: &ProviderCommandAttemptJournal,
        composition: NetworkTeardownComposition<'_>,
    ) -> NetworkTeardownResult<SandboxNetworkTeardownObservation> {
        if provider_observation.claim() != command.provider_claim() {
            return Err(NetworkTeardownAdapterError::crossed(
                "Container provider observation",
            ));
        }
        let snapshot = self
            .read_exact_network_manifest(command)
            .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        let prior_detach_success =
            self.authenticate_network_teardown_snapshot(command, &snapshot, journal, composition)?;

        let (_lifecycle, mut manifest) = match composition {
            NetworkTeardownComposition::HostManaged => {
                super::runner::lock_execute_lifecycle_and_read_current_for_backend(self, &snapshot)
            }
            NetworkTeardownComposition::Forwarded { .. } => {
                super::runner::lock_current_provision_lifecycle_for_backend(self, &snapshot)
            }
        }
        .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        self.authenticate_network_teardown_manifest(command, &manifest, composition)?;
        self.require_execution_predecessor(command, &manifest, composition)?;
        if command.operation() == SandboxNetworkTeardownOperation::Release {
            require_prior_detach_success_evidence(
                command,
                &manifest.network_teardown,
                prior_detach_success.as_ref(),
            )?;
        }
        let rebase = manifest
            .network_teardown
            .inspect_and_rebase_command(command, provider_observation)
            .map_err(network_state_error)?;
        if rebase == HostManagedAttachmentCommandInspection::AuthorizedImmediatePredecessor {
            let context = manifest.clone();
            self.persist_network_progress(&context, &manifest.network_teardown)
                .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        }
        self.apply_network_teardown(command, manifest, composition)
            .map_err(NetworkTeardownAdapterError::ambiguous_error)
    }

    fn apply_network_teardown(
        &self,
        command: &SandboxNetworkTeardownCommand,
        manifest: ContainerSandboxManifest,
        composition: NetworkTeardownComposition<'_>,
    ) -> crate::Result<SandboxNetworkTeardownObservation> {
        let context = manifest.clone();
        let progress = RefCell::new(manifest.network_teardown.clone());
        let ports = self.port_lease_coordinator_for_manifest(&context)?;
        let lifecycle = self.attachment_lifecycle(&ports);
        let network_config = context.require_network_config()?;
        let hostname = hostname_for(&context.spec);
        let forwarder = match composition {
            NetworkTeardownComposition::HostManaged => None,
            NetworkTeardownComposition::Forwarded {
                expected_forwarder, ..
            } => Some(expected_forwarder),
        };
        let adapter = self.attachment_adapter(&context, network_config, &hostname, forwarder);

        match command.operation() {
            SandboxNetworkTeardownOperation::Detach => {
                let current_phase = progress.borrow().detach_phase();
                let record_phase = |phase| {
                    let mut next = progress.borrow_mut();
                    if next.record_detach_phase(command.provider_claim(), phase)? {
                        self.persist_network_progress(&context, &next)?;
                    }
                    Ok(())
                };
                let stop_auxiliary = |disposition| {
                    if disposition == AttachmentAuxiliaryDisposition::Unknown {
                        return Err(SandboxError::OperationFailed {
                            message: "Container detach has ambiguous PEP ownership".to_owned(),
                        });
                    }
                    self.egress_proxies.stop_for_detach(
                        &context.spec.tenant_id,
                        &context.handle.id,
                        context
                            .provision_network_plan
                            .as_ref()
                            .ok_or_else(|| sandbox_crossed("Container compiled network plan"))?,
                        context.egress_proxy.as_ref(),
                    )
                };
                let proof = match composition {
                    NetworkTeardownComposition::HostManaged => adapter
                        .detach_host_managed_retained(
                            &lifecycle,
                            command,
                            current_phase,
                            record_phase,
                            stop_auxiliary,
                        )?,
                    NetworkTeardownComposition::Forwarded { .. } => adapter
                        .detach_machine_forwarded_retained(
                            &lifecycle,
                            command,
                            current_phase,
                            record_phase,
                            || forwarded::retain_forwarded_publication(self, &context, composition),
                            stop_auxiliary,
                        )?,
                };
                {
                    let mut next = progress.borrow_mut();
                    if next.record_detached_for_command(command, proof)? {
                        self.persist_network_progress(&context, &next)?;
                    }
                }
            }
            SandboxNetworkTeardownOperation::Release => {
                let current = progress.borrow().clone();
                let proof = current.require_detached_for_release(command)?.clone();
                let record_phase = |phase| {
                    let mut next = progress.borrow_mut();
                    if next.record_release_phase(command, phase)? {
                        self.persist_network_progress(&context, &next)?;
                    }
                    Ok(())
                };
                let release_auxiliary = || {
                    self.egress_proxies.release_after_detach(
                        &context.spec.tenant_id,
                        &context.handle.id,
                        context
                            .provision_network_plan
                            .as_ref()
                            .ok_or_else(|| sandbox_crossed("Container compiled network plan"))?,
                        context.egress_proxy.as_ref(),
                    )
                };
                match composition {
                    NetworkTeardownComposition::HostManaged => adapter
                        .release_host_managed_detached(
                            &lifecycle,
                            command,
                            &proof,
                            current.release_phase(),
                            record_phase,
                            release_auxiliary,
                        )?,
                    NetworkTeardownComposition::Forwarded { .. } => adapter
                        .release_machine_forwarded_detached(
                            &lifecycle,
                            command,
                            &proof,
                            current.release_phase(),
                            record_phase,
                            AttachmentReleaseActions::new(
                                || {
                                    forwarded::inspect_forwarded_publication_absence(
                                        self,
                                        &context,
                                        composition,
                                    )
                                },
                                || forwarded::release_forwarded_listener_authority(self, &context),
                                release_auxiliary,
                            ),
                        )?,
                }
            }
        }
        progress.borrow().validate()?;
        succeeded(
            match composition {
                NetworkTeardownComposition::HostManaged => "container_host_attachment_teardown",
                NetworkTeardownComposition::Forwarded { .. } => {
                    "container_forwarded_attachment_teardown"
                }
            },
            &progress.borrow(),
        )
    }

    fn persist_network_progress(
        &self,
        context: &ContainerSandboxManifest,
        progress: &HostManagedAttachmentTeardownState,
    ) -> crate::Result<()> {
        let mut next = context.clone();
        next.network_teardown = progress.clone();
        self.write_existing_workload_manifest(&next)?;
        #[cfg(test)]
        if let Some(probe) = self.network_teardown_checkpoint_test_probe {
            probe.exit_if_reached(progress);
        }
        Ok(())
    }

    fn inspect_network_teardown_inner(
        &self,
        command: &SandboxNetworkTeardownCommand,
        provider_observation: &ProviderCommandObservation,
        journal: &ProviderCommandAttemptJournal,
        composition: NetworkTeardownComposition<'_>,
    ) -> NetworkTeardownResult<SandboxNetworkTeardownObservation> {
        let snapshot = self
            .read_exact_network_manifest(command)
            .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        let prior_detach_success = match composition {
            NetworkTeardownComposition::Forwarded {
                prior_observation, ..
            } if command.operation() == SandboxNetworkTeardownOperation::Release => {
                let local_observation =
                    inspect_prior_detach_success(command, &snapshot.network_teardown, journal)?;
                if prior_observation != &local_observation {
                    return Err(NetworkTeardownAdapterError::ambiguous(
                        "forwarded ReleaseNetwork caller evidence crossed the exact local DetachNetwork journal result",
                    ));
                }
                Some(local_observation)
            }
            _ => None,
        };
        let (_inspection, manifest) =
            super::runner::lock_current_inspection_for_backend(self, &snapshot)
                .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        self.authenticate_network_teardown_manifest(command, &manifest, composition)?;
        self.require_execution_predecessor(command, &manifest, composition)?;
        if let Some(prior_detach_success) = prior_detach_success.as_ref() {
            require_inspected_prior_detach_success_evidence(
                command,
                &manifest.network_teardown,
                prior_detach_success,
            )?;
        }
        let mut inspected = manifest.network_teardown.clone();
        match inspected.inspect_and_rebase_command(command, provider_observation) {
            Ok(HostManagedAttachmentCommandInspection::ExactTerminalSuccess) => {
                self.require_forwarded_publication_absence_if_selected(&manifest, composition)?;
                succeeded(
                    match composition {
                        NetworkTeardownComposition::HostManaged => {
                            "container_host_attachment_terminal"
                        }
                        NetworkTeardownComposition::Forwarded { .. } => {
                            "container_forwarded_attachment_terminal"
                        }
                    },
                    &manifest.network_teardown,
                )
                .map_err(NetworkTeardownAdapterError::ambiguous_error)
            }
            Ok(HostManagedAttachmentCommandInspection::AuthorizedImmediatePredecessor) => {
                in_progress("Container attachment retry is durably authorized but not started")
                    .map_err(NetworkTeardownAdapterError::ambiguous_error)
            }
            Ok(HostManagedAttachmentCommandInspection::ExactCurrentPartial) => {
                let not_started = match command.operation() {
                    SandboxNetworkTeardownOperation::Detach => {
                        manifest.network_teardown.detach_phase()
                            == crate::backends::oci::network::HostManagedAttachmentDetachPhase::NotStarted
                    }
                    SandboxNetworkTeardownOperation::Release => {
                        manifest.network_teardown.release_phase()
                            == crate::backends::oci::network::HostManagedAttachmentReleasePhase::NotStarted
                    }
                };
                if !not_started {
                    match (command.operation(), composition) {
                        (
                            SandboxNetworkTeardownOperation::Detach,
                            NetworkTeardownComposition::Forwarded { .. },
                        ) => match forwarded::inspect_forwarded_publication_for_detach(
                            self,
                            &manifest,
                            composition,
                        )
                        .map_err(NetworkTeardownAdapterError::ambiguous_error)?
                        {
                            forwarded::ForwardedPublicationTeardownInspection::Present
                                if manifest.network_teardown.detach_phase()
                                    <= crate::backends::oci::network::HostManagedAttachmentDetachPhase::ListenerStopMayExist =>
                            {
                                return in_progress(
                                    "Container forwarded publication is exactly present; detach can resume withdrawal",
                                )
                                .map_err(NetworkTeardownAdapterError::ambiguous_error);
                            }
                            forwarded::ForwardedPublicationTeardownInspection::Present => {
                                return Err(NetworkTeardownAdapterError::ambiguous(
                                    "Container forwarded publication remains present after the durable withdrawal boundary",
                                ));
                            }
                            forwarded::ForwardedPublicationTeardownInspection::Partial => {
                                return Err(NetworkTeardownAdapterError::ambiguous(
                                    "Container forwarded publication has partial durable withdrawal evidence",
                                ));
                            }
                            forwarded::ForwardedPublicationTeardownInspection::Absent => {}
                        },
                        _ => self.require_forwarded_publication_absence_if_selected(
                            &manifest,
                            composition,
                        )?,
                    }
                }
                match (provider_observation.kind(), not_started) {
                    (ProviderCommandObservationKind::Claimed, true) => in_progress(
                        "Container attachment teardown is claimed and can still start",
                    )
                    .map_err(NetworkTeardownAdapterError::ambiguous_error),
                    (ProviderCommandObservationKind::Claimed, false) => {
                        in_progress("Container attachment teardown is durably in progress")
                            .map_err(NetworkTeardownAdapterError::ambiguous_error)
                    }
                    (
                        ProviderCommandObservationKind::InProgress
                        | ProviderCommandObservationKind::Ambiguous,
                        _,
                    ) => Ok(SandboxNetworkTeardownObservation::RetryAuthorized {
                        evidence: b"Container exact durable attachment progress authorizes inspected recovery"
                            .to_vec(),
                    }),
                    _ => Err(NetworkTeardownAdapterError::crossed(
                        "terminal provider observation during live inspection",
                    )),
                }
            }
            Err(error) => Err(network_state_error(error)),
        }
    }

    fn require_forwarded_publication_absence_if_selected(
        &self,
        manifest: &ContainerSandboxManifest,
        composition: NetworkTeardownComposition<'_>,
    ) -> NetworkTeardownResult<()> {
        if matches!(composition, NetworkTeardownComposition::Forwarded { .. }) {
            forwarded::inspect_forwarded_publication_absence(self, manifest, composition)
                .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        }
        Ok(())
    }

    fn read_exact_network_manifest(
        &self,
        command: &SandboxNetworkTeardownCommand,
    ) -> crate::Result<ContainerSandboxManifest> {
        let path = crate::artifact_paths::manifest_path(
            &self.config.workload_state_root,
            command.tenant_id(),
            command.sandbox_id(),
        );
        let bytes = std::fs::read(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                SandboxError::NotFound {
                    sandbox_id: command.sandbox_id().as_str().to_owned(),
                }
            } else {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to read exact Container manifest {}: {error}",
                        path.display()
                    ),
                }
            }
        })?;
        let manifest: ContainerSandboxManifest =
            serde_json::from_slice(&bytes).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse exact Container manifest {}: {error}",
                    path.display()
                ),
            })?;
        self.validate_manifest_execution_context(&manifest)?;
        if manifest.conmon_layout.manifest_path != path {
            return Err(sandbox_crossed("tenant-qualified Container manifest path"));
        }
        Ok(manifest)
    }

    fn authenticate_network_teardown_manifest(
        &self,
        command: &SandboxNetworkTeardownCommand,
        manifest: &ContainerSandboxManifest,
        composition: NetworkTeardownComposition<'_>,
    ) -> NetworkTeardownResult<()> {
        if command.provider_registration_key() != CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY
            || &manifest.spec.tenant_id != command.tenant_id()
            || &manifest.handle.id != command.sandbox_id()
        {
            return Err(NetworkTeardownAdapterError::crossed(
                "Container attachment composition",
            ));
        }
        let durable_forwarder = manifest
            .runner_config
            .validated_machine_port_forwarder(&manifest.handle.id)
            .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        match composition {
            NetworkTeardownComposition::HostManaged
                if self.config.start_mode == ContainerStartMode::Execute
                    && manifest.start_mode == ContainerStartMode::Execute
                    && manifest.runner_config.network_publication_mode
                        == ContainerNetworkPublicationMode::HostManaged
                    && durable_forwarder.is_none() => {}
            NetworkTeardownComposition::Forwarded {
                expected_forwarder, ..
            } if self.config.start_mode == ContainerStartMode::PlanOnly
                && manifest.start_mode == ContainerStartMode::PlanOnly
                && manifest.lifecycle_coordinator
                    == ContainerLifecycleCoordinator::PreparedServiceRunner
                && manifest.runner_config.network_publication_mode
                    == ContainerNetworkPublicationMode::MachineForwarded
                && durable_forwarder == Some(expected_forwarder) => {}
            NetworkTeardownComposition::HostManaged => {
                return Err(NetworkTeardownAdapterError::crossed(
                    "Container host-managed attachment composition",
                ));
            }
            NetworkTeardownComposition::Forwarded { .. } => {
                return Err(NetworkTeardownAdapterError::crossed(
                    "Container forwarded-machine attachment composition or forwarder generation",
                ));
            }
        }
        manifest
            .require_execution_attempt(command.execution_attempt_id(), "Container network teardown")
            .map_err(|error| NetworkTeardownAdapterError::crossed(error.to_string()))?;
        let plan = manifest.provision_network_plan.as_ref().ok_or_else(|| {
            NetworkTeardownAdapterError::ambiguous(
                "Container manifest omitted its durable compiled network plan",
            )
        })?;
        let config = manifest
            .require_network_config()
            .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        if plan.network_plan() != command.network_plan()
            || plan.attachment_id() != command.attachment_id()
            || config.network_plan.as_ref() != Some(command.network_plan())
            || &config.attachment_id != command.attachment_id()
        {
            return Err(NetworkTeardownAdapterError::crossed(
                "Container plan, generation, or attachment identity",
            ));
        }
        manifest
            .network_teardown
            .validate()
            .map_err(NetworkTeardownAdapterError::ambiguous_error)
    }

    fn authenticate_network_teardown_snapshot(
        &self,
        command: &SandboxNetworkTeardownCommand,
        manifest: &ContainerSandboxManifest,
        journal: &ProviderCommandAttemptJournal,
        composition: NetworkTeardownComposition<'_>,
    ) -> NetworkTeardownResult<Option<ProviderCommandObservation>> {
        self.authenticate_network_teardown_manifest(command, manifest, composition)?;
        self.require_execution_predecessor(command, manifest, composition)?;
        match command.operation() {
            SandboxNetworkTeardownOperation::Detach => Ok(None),
            SandboxNetworkTeardownOperation::Release => {
                let observation =
                    read_prior_detach_success(command, &manifest.network_teardown, journal)?;
                if let NetworkTeardownComposition::Forwarded {
                    prior_observation, ..
                } = composition
                    && prior_observation != &observation
                {
                    return Err(NetworkTeardownAdapterError::crossed(
                        "forwarded ReleaseNetwork prior DetachNetwork observation",
                    ));
                }
                Ok(Some(observation))
            }
        }
    }

    fn require_execution_predecessor(
        &self,
        command: &SandboxNetworkTeardownCommand,
        manifest: &ContainerSandboxManifest,
        composition: NetworkTeardownComposition<'_>,
    ) -> NetworkTeardownResult<()> {
        match (command.operation(), composition) {
            (
                SandboxNetworkTeardownOperation::Detach,
                NetworkTeardownComposition::Forwarded {
                    prior_observation, ..
                },
            ) => require_execution_stopped(
                manifest
                    .execution_teardown
                    .require_stopped_observation_for_network(
                        command.provider_claim(),
                        prior_observation,
                    ),
            ),
            _ => require_execution_stopped(
                manifest
                    .execution_teardown
                    .require_stopped_for_network(command.provider_claim()),
            ),
        }
    }
}

fn read_prior_detach_success(
    command: &SandboxNetworkTeardownCommand,
    state: &HostManagedAttachmentTeardownState,
    journal: &ProviderCommandAttemptJournal,
) -> NetworkTeardownResult<ProviderCommandObservation> {
    state
        .validate()
        .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
    if state.detach_phase()
        != crate::backends::oci::network::HostManagedAttachmentDetachPhase::Detached
    {
        return Err(NetworkTeardownAdapterError::order_invalid(
            "ReleaseNetwork requires completed retained detach progress",
        ));
    }
    let proof = state
        .require_detached_for_release(command)
        .map_err(|error| NetworkTeardownAdapterError::crossed(error.to_string()))?;
    match journal.adopt_exact_attempt(proof.detach_claim()) {
        Ok(Some(observation))
            if observation.kind() == ProviderCommandObservationKind::Succeeded =>
        {
            Ok(observation)
        }
        Ok(_) => Err(NetworkTeardownAdapterError::order_invalid(
            "ReleaseNetwork requires exact prior DetachNetwork journal success",
        )),
        Err(error) => Err(NetworkTeardownAdapterError::ambiguous(format!(
            "could not authenticate prior DetachNetwork journal result: {error}"
        ))),
    }
}

fn inspect_prior_detach_success(
    command: &SandboxNetworkTeardownCommand,
    state: &HostManagedAttachmentTeardownState,
    journal: &ProviderCommandAttemptJournal,
) -> NetworkTeardownResult<ProviderCommandObservation> {
    let detach_claim = inspected_detach_claim(command, state)?;
    match journal.adopt_exact_attempt(&detach_claim) {
        Ok(Some(observation))
            if observation.kind() == ProviderCommandObservationKind::Succeeded =>
        {
            Ok(observation)
        }
        Ok(_) => Err(NetworkTeardownAdapterError::ambiguous(
            "ReleaseNetwork inspection could not adopt exact prior DetachNetwork journal success",
        )),
        Err(error) => Err(NetworkTeardownAdapterError::ambiguous(format!(
            "ReleaseNetwork inspection could not authenticate the prior DetachNetwork journal result: {error}"
        ))),
    }
}

fn require_inspected_prior_detach_success_evidence(
    command: &SandboxNetworkTeardownCommand,
    state: &HostManagedAttachmentTeardownState,
    observation: &ProviderCommandObservation,
) -> NetworkTeardownResult<()> {
    let detach_claim = inspected_detach_claim(command, state)?;
    if observation.claim() == &detach_claim
        && observation.kind() == ProviderCommandObservationKind::Succeeded
    {
        Ok(())
    } else {
        Err(NetworkTeardownAdapterError::ambiguous(
            "locked retained detach proof crossed the adopted DetachNetwork journal result",
        ))
    }
}

fn inspected_detach_claim(
    command: &SandboxNetworkTeardownCommand,
    state: &HostManagedAttachmentTeardownState,
) -> NetworkTeardownResult<ProviderCommandClaim> {
    state.validate().map_err(|error| {
        NetworkTeardownAdapterError::ambiguous(format!(
            "could not authenticate retained detach progress during ReleaseNetwork inspection: {error}"
        ))
    })?;
    if state.detach_phase()
        != crate::backends::oci::network::HostManagedAttachmentDetachPhase::Detached
    {
        return Err(NetworkTeardownAdapterError::ambiguous(
            "ReleaseNetwork inspection requires completed retained detach progress",
        ));
    }
    state
        .require_detached_for_release(command)
        .map(|proof| proof.detach_claim().clone())
        .map_err(|error| {
            NetworkTeardownAdapterError::ambiguous(format!(
                "could not authenticate the retained detach proof during ReleaseNetwork inspection: {error}"
            ))
        })
}

fn require_prior_detach_success_evidence(
    command: &SandboxNetworkTeardownCommand,
    state: &HostManagedAttachmentTeardownState,
    observation: Option<&ProviderCommandObservation>,
) -> NetworkTeardownResult<()> {
    state
        .validate()
        .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
    if state.detach_phase()
        != crate::backends::oci::network::HostManagedAttachmentDetachPhase::Detached
    {
        return Err(NetworkTeardownAdapterError::order_invalid(
            "ReleaseNetwork requires completed retained detach progress",
        ));
    }
    let proof = state
        .require_detached_for_release(command)
        .map_err(|error| NetworkTeardownAdapterError::crossed(error.to_string()))?;
    match observation {
        Some(observation)
            if observation.claim() == proof.detach_claim()
                && observation.kind() == ProviderCommandObservationKind::Succeeded =>
        {
            Ok(())
        }
        _ => Err(NetworkTeardownAdapterError::crossed(
            "retained detach proof changed after prior journal authentication",
        )),
    }
}

fn require_execution_stopped(
    result: Result<&[u8], ContainerNetworkStopRequirementError>,
) -> NetworkTeardownResult<()> {
    match result {
        Ok(_) => Ok(()),
        Err(ContainerNetworkStopRequirementError::NotStopped) => {
            Err(NetworkTeardownAdapterError::order_invalid(
                "Container network teardown requires exact durable ExecutionStopped evidence",
            ))
        }
        Err(ContainerNetworkStopRequirementError::Crossed) => Err(
            NetworkTeardownAdapterError::crossed("Container durable ExecutionStopped fence"),
        ),
    }
}

fn network_observation_kind(
    observation: &SandboxNetworkTeardownObservation,
) -> ProviderCommandObservationKind {
    match observation {
        SandboxNetworkTeardownObservation::Succeeded { .. } => {
            ProviderCommandObservationKind::Succeeded
        }
        SandboxNetworkTeardownObservation::DefiniteFailure { .. } => {
            ProviderCommandObservationKind::DefiniteFailure
        }
        SandboxNetworkTeardownObservation::Absent { .. } => ProviderCommandObservationKind::Absent,
        SandboxNetworkTeardownObservation::RetryAuthorized { .. } => {
            ProviderCommandObservationKind::RetryAuthorized
        }
        SandboxNetworkTeardownObservation::InProgress { .. } => {
            ProviderCommandObservationKind::InProgress
        }
        SandboxNetworkTeardownObservation::Ambiguous { .. } => {
            ProviderCommandObservationKind::Ambiguous
        }
    }
}

fn succeeded(
    label: &str,
    progress: &HostManagedAttachmentTeardownState,
) -> crate::Result<SandboxNetworkTeardownObservation> {
    serde_json::to_vec(&(label, progress))
        .map(|evidence| SandboxNetworkTeardownObservation::Succeeded { evidence })
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to serialize Container network teardown evidence: {error}"),
        })
}

fn in_progress(message: &str) -> crate::Result<SandboxNetworkTeardownObservation> {
    Ok(SandboxNetworkTeardownObservation::InProgress {
        evidence: message.as_bytes().to_vec(),
    })
}

fn definite_failure(code: &str, message: impl AsRef<str>) -> SandboxNetworkTeardownObservation {
    SandboxNetworkTeardownObservation::DefiniteFailure {
        code: code.to_owned(),
        evidence: message.as_ref().as_bytes().to_vec(),
    }
}

fn ambiguous(message: impl AsRef<str>) -> SandboxNetworkTeardownObservation {
    SandboxNetworkTeardownObservation::Ambiguous {
        evidence: message.as_ref().as_bytes().to_vec(),
    }
}

fn sandbox_crossed(subject: &str) -> SandboxError {
    SandboxError::InvalidSpec {
        message: format!("Container network teardown crossed {subject}"),
    }
}

fn network_state_error(
    error: HostManagedAttachmentCommandInspectionError,
) -> NetworkTeardownAdapterError {
    match error {
        HostManagedAttachmentCommandInspectionError::Crossed => {
            NetworkTeardownAdapterError::crossed("Container durable network progress")
        }
        HostManagedAttachmentCommandInspectionError::EpochInvalid => {
            NetworkTeardownAdapterError::epoch_invalid(
                "Container durable provider-local progress rejected a stale or nonadjacent epoch",
            )
        }
        HostManagedAttachmentCommandInspectionError::Corrupt => {
            NetworkTeardownAdapterError::ambiguous(
                "Container durable network teardown progress is corrupt or incomplete",
            )
        }
    }
}

impl NetworkTeardownAdapterError {
    fn crossed(message: impl Into<String>) -> Self {
        Self::Definite {
            code: "sandbox_teardown_command_crossed",
            message: message.into(),
        }
    }

    fn epoch_invalid(message: impl Into<String>) -> Self {
        Self::Definite {
            code: "sandbox_teardown_epoch_invalid",
            message: message.into(),
        }
    }

    fn order_invalid(message: impl Into<String>) -> Self {
        Self::Definite {
            code: "sandbox_teardown_order_invalid",
            message: message.into(),
        }
    }

    fn ambiguous(message: impl Into<String>) -> Self {
        Self::Ambiguous {
            message: message.into(),
        }
    }

    fn ambiguous_error(error: SandboxError) -> Self {
        Self::ambiguous(error.to_string())
    }

    fn into_sandbox_error(self) -> SandboxError {
        let message = match self {
            Self::Definite { code, message } => format!("{code}: {message}"),
            Self::Ambiguous { message } => message,
        };
        SandboxError::OperationFailed { message }
    }

    fn into_observation(self) -> SandboxNetworkTeardownObservation {
        match self {
            Self::Definite { code, message } => definite_failure(code, message),
            Self::Ambiguous { message } => ambiguous(message),
        }
    }
}
