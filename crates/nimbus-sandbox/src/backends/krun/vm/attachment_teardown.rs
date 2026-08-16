//! Exact Krun host-managed attachment detach and release adapter.

use std::cell::RefCell;

use nimbus_network::NetworkAttachmentReservationState;

use crate::backends::KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY;
use crate::backends::conmon::lifecycle::{inspect_runtime_artifact_presence, remove_if_exists};
use crate::backends::oci::network::{
    AttachmentAuxiliaryDisposition, HostManagedAttachmentCommandInspection,
    HostManagedAttachmentCommandInspectionError, HostManagedAttachmentTeardownState,
};
use crate::{
    ProviderCommandAttemptJournal, ProviderCommandExecutionClaim, ProviderCommandJournalError,
    ProviderCommandObservation, ProviderCommandObservationKind, SandboxError,
    SandboxNetworkTeardownCommand, SandboxNetworkTeardownObservation,
    SandboxNetworkTeardownOperation,
};

use super::teardown::state::KrunNetworkStopRequirementError;
use super::{
    KrunCreatorHandoffState, KrunLaunchAuthority, KrunSandboxBackend, KrunSandboxManifest,
    KrunStartMode,
};

type NetworkTeardownResult<T> = std::result::Result<T, NetworkTeardownAdapterError>;

#[derive(Debug)]
enum NetworkTeardownAdapterError {
    Definite { code: &'static str, message: String },
    Ambiguous { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterruptedAdoptionDisposition {
    Unchanged,
    ConfirmedNoProviderEffect,
}

impl KrunSandboxBackend {
    /// Authenticate exact durable network teardown authority without writes or effects.
    ///
    /// Compute calls this before it creates a provider-command claim. Execute
    /// repeats the same checks while holding its command stream, then
    /// reauthenticates the manifest after it acquires lifecycle authority.
    pub fn preflight_network_teardown_command(
        &self,
        command: &SandboxNetworkTeardownCommand,
    ) -> Result<(), SandboxNetworkTeardownObservation> {
        let result = (|| {
            let snapshot = self
                .read_exact_network_manifest(command)
                .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
            let journal = self
                .attempt_idempotency_journal()
                .map_err(|error| NetworkTeardownAdapterError::ambiguous(error.to_string()))?;
            self.authenticate_network_teardown_snapshot(command, &snapshot, &journal)?;
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
                message: "Krun network authorization crossed its provider command".to_owned(),
            });
        }
        let journal = self.attempt_idempotency_journal()?;
        let (_, provider_observation) =
            journal.execute_current_claim(execution_claim, |current_claim| {
                let observation = self.execute_network_teardown_inner(
                    command,
                    current_claim.observation(),
                    &journal,
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
                "Krun network inspection authorization crossed its provider command",
            );
        }
        let journal = match self.attempt_idempotency_journal() {
            Ok(journal) => journal,
            Err(error) => return ambiguous(error.to_string()),
        };
        match journal.inspect_current_claim(provider_observation, |current| {
            self.inspect_network_teardown_inner(command, current)
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
    ) -> SandboxNetworkTeardownObservation {
        match self.execute_network_teardown_locked(command, provider_observation, journal) {
            Ok(observation) => observation,
            Err(error) => error.into_observation(),
        }
    }

    fn execute_network_teardown_locked(
        &self,
        command: &SandboxNetworkTeardownCommand,
        provider_observation: &ProviderCommandObservation,
        journal: &ProviderCommandAttemptJournal,
    ) -> NetworkTeardownResult<SandboxNetworkTeardownObservation> {
        if provider_observation.claim() != command.provider_claim() {
            return Err(NetworkTeardownAdapterError::crossed(
                "Krun provider observation",
            ));
        }
        let snapshot = self
            .read_exact_network_manifest(command)
            .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        let prior_detach_success =
            self.authenticate_network_teardown_snapshot(command, &snapshot, journal)?;

        let _lifecycle = self
            .lock_launch_lifecycle(&snapshot)
            .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        let mut manifest = self
            .read_exact_network_manifest(command)
            .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        self.authenticate_network_teardown_manifest(command, &manifest)?;
        let interrupted_adoption = self.reconcile_interrupted_attachment_adoption(&mut manifest)?;
        self.authenticate_network_teardown_manifest(command, &manifest)?;
        require_execution_stopped(
            manifest
                .execution_teardown
                .require_stopped_for_network(command.provider_claim()),
        )?;
        if command.operation() == SandboxNetworkTeardownOperation::Release {
            let expected = snapshot
                .network_teardown
                .require_detached_for_release(command)
                .map_err(|error| NetworkTeardownAdapterError::crossed(error.to_string()))?;
            let current = manifest
                .network_teardown
                .require_detached_for_release(command)
                .map_err(|error| NetworkTeardownAdapterError::crossed(error.to_string()))?;
            if current != expected {
                return Err(NetworkTeardownAdapterError::crossed(
                    "Krun durable detached proof after lifecycle lock",
                ));
            }
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
        self.apply_network_teardown(command, manifest, interrupted_adoption)
            .map_err(NetworkTeardownAdapterError::ambiguous_error)
    }

    fn apply_network_teardown(
        &self,
        command: &SandboxNetworkTeardownCommand,
        manifest: KrunSandboxManifest,
        interrupted_adoption: InterruptedAdoptionDisposition,
    ) -> crate::Result<SandboxNetworkTeardownObservation> {
        let context = manifest.clone();
        let progress = RefCell::new(manifest.network_teardown.clone());
        let ports = self.port_lease_coordinator();
        let lifecycle = self.attachment_lifecycle(&ports);
        let network_config = context.require_network_config()?;
        let hostname = super::start::hostname_for(&context.spec);
        let adapter = self.retained_attachment_adapter(&context, network_config, &hostname);
        let confirmed_no_effect = context.creator_handoff
            == super::KrunCreatorHandoffState::NotSpawned
            && (interrupted_adoption == InterruptedAdoptionDisposition::ConfirmedNoProviderEffect
                || matches!(
                    context.launch_authority,
                    KrunLaunchAuthority::Adopted { .. }
                ));

        match command.operation() {
            SandboxNetworkTeardownOperation::Detach => {
                let proof = if confirmed_no_effect {
                    adapter.detach_deferred_never_effected_retained(
                        &lifecycle,
                        command,
                        context.adopting_association(),
                    )?
                } else {
                    let current_phase = progress.borrow().detach_phase();
                    adapter.detach_deferred_retained(
                        &lifecycle,
                        command,
                        current_phase,
                        |phase| {
                            let mut next = progress.borrow_mut();
                            if next.record_detach_phase(command.provider_claim(), phase)? {
                                self.persist_network_progress(&context, &next)?;
                            }
                            Ok(())
                        },
                        |disposition| {
                            if disposition == AttachmentAuxiliaryDisposition::Unknown {
                                return Err(SandboxError::OperationFailed {
                                    message: "Krun detach has ambiguous PEP ownership".to_owned(),
                                });
                            }
                            self.egress_proxies.stop_for_detach(
                                &context.spec.tenant_id,
                                &context.handle.id,
                                context
                                    .provision_network_plan
                                    .as_ref()
                                    .ok_or_else(|| sandbox_crossed("krun compiled network plan"))?,
                                context.egress_proxy.as_ref(),
                            )
                        },
                    )?
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
                adapter.release_deferred_detached(
                    &lifecycle,
                    command,
                    &proof,
                    current.release_phase(),
                    |phase| {
                        let mut next = progress.borrow_mut();
                        if next.record_release_phase(command, phase)? {
                            self.persist_network_progress(&context, &next)?;
                        }
                        Ok(())
                    },
                    || {
                        let plan = context
                            .provision_network_plan
                            .as_ref()
                            .ok_or_else(|| sandbox_crossed("krun compiled network plan"))?;
                        if proof.confirmed_no_provider_effect() {
                            let Some(assignment) = context.egress_proxy.as_ref() else {
                                return Ok(());
                            };
                            assignment
                                .require_compiled_plan_authority(&context.spec.tenant_id, plan)?;
                            let claim = context.reservation_claim().ok_or_else(|| {
                                sandbox_crossed("krun no-provider-effect reservation claim")
                            })?;
                            ports.release_never_bound_plan_members(
                                &assignment.compiled_plan_members(plan),
                                std::slice::from_ref(&assignment.port_lease),
                                claim,
                            )
                        } else {
                            self.egress_proxies.release_after_detach(
                                &context.spec.tenant_id,
                                &context.handle.id,
                                plan,
                                context.egress_proxy.as_ref(),
                            )
                        }
                    },
                )?;
            }
        }
        progress.borrow().validate()?;
        if command.operation() == SandboxNetworkTeardownOperation::Release {
            let mut terminal = context.clone();
            terminal.network_teardown = progress.borrow().clone();
            self.finalize_released_network_manifest(&mut terminal)?;
        }
        succeeded("krun_host_attachment_teardown", &progress.borrow())
    }

    fn persist_network_progress(
        &self,
        context: &KrunSandboxManifest,
        progress: &HostManagedAttachmentTeardownState,
    ) -> crate::Result<()> {
        let mut next = context.clone();
        next.network_teardown = progress.clone();
        self.persist_effect_barrier(&next, "krun host attachment teardown progress")?;
        #[cfg(test)]
        if let Some(probe) = self.network_teardown_checkpoint_test_probe {
            probe.exit_if_reached(progress);
        }
        Ok(())
    }

    /// Finish provider-local cleanup only after durable network release.
    ///
    /// The released phase is the durable authorization before the artifact
    /// effect. A crash after artifact removal retains its exact descriptor, so
    /// retry can authenticate the same path, confirm absence idempotently, and
    /// publish terminal status without repeating network release.
    pub(super) fn finalize_released_network_manifest(
        &self,
        manifest: &mut KrunSandboxManifest,
    ) -> crate::Result<()> {
        manifest.network_teardown.validate()?;
        if manifest.network_teardown.release_phase()
            != crate::backends::oci::network::HostManagedAttachmentReleasePhase::Released
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "Krun workload {} cannot finalize provider artifacts before durable network release",
                    manifest.handle.id
                ),
            });
        }
        if manifest.has_terminal_network_finality() {
            return Ok(());
        }
        if !manifest.shutdown_requested
            || !manifest.creator_handoff.authorizes_provider_cleanup()
            || manifest.provider_failure_cleanup.is_active()
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "Krun workload {} cannot finalize released provider artifacts from shutdown={}, creator={:?}, provider_failure_cleanup={:?}",
                    manifest.handle.id,
                    manifest.shutdown_requested,
                    manifest.creator_handoff,
                    manifest.provider_failure_cleanup,
                ),
            });
        }
        self.finalize_released_runtime_artifacts(manifest)?;
        self.cleanup_manifest_launch_artifacts(manifest)?;
        manifest.launch_artifact = None;
        manifest.launch_authority = KrunLaunchAuthority::Released;
        super::readiness::synchronize_handle_status(manifest, crate::SandboxStatus::Stopped);
        self.persist_effect_barrier(manifest, "krun released provider finality")
    }

    fn finalize_released_runtime_artifacts(
        &self,
        manifest: &mut KrunSandboxManifest,
    ) -> crate::Result<()> {
        if manifest.creator_handoff == KrunCreatorHandoffState::NotSpawned {
            for (path, label) in [
                (&manifest.conmon_layout.pidfile, "runtime pidfile"),
                (&manifest.conmon_layout.conmon_pidfile, "conmon pidfile"),
                (
                    &manifest.conmon_layout.exit_status_file,
                    "exit-status receipt",
                ),
            ] {
                if inspect_runtime_artifact_presence(path, label)? {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "Krun workload {} is marked never spawned but retains {label}; provider finality remains fenced",
                            manifest.handle.id
                        ),
                    });
                }
            }
            return Ok(());
        }

        self.delete_runtime_and_confirm_absent(manifest)?;
        self.persist_creator_quiescence_after_runtime_absence(manifest)?;
        remove_if_exists(&manifest.conmon_layout.pidfile)?;
        remove_if_exists(&manifest.conmon_layout.conmon_pidfile)?;
        remove_if_exists(&manifest.conmon_layout.exit_status_file)
    }

    fn inspect_network_teardown_inner(
        &self,
        command: &SandboxNetworkTeardownCommand,
        provider_observation: &ProviderCommandObservation,
    ) -> NetworkTeardownResult<SandboxNetworkTeardownObservation> {
        let snapshot = self
            .read_exact_network_manifest(command)
            .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        let (_inspection, manifest) = self
            .lock_current_inspection(&snapshot)
            .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        self.authenticate_network_teardown_manifest(command, &manifest)?;
        let mut inspected = manifest.network_teardown.clone();
        match inspected.inspect_and_rebase_command(command, provider_observation) {
            Ok(HostManagedAttachmentCommandInspection::ExactTerminalSuccess) => {
                if command.operation() == SandboxNetworkTeardownOperation::Release
                    && !manifest.has_terminal_network_finality()
                {
                    return Ok(SandboxNetworkTeardownObservation::RetryAuthorized {
                        evidence: b"Krun network authority is released; provider-local artifact and terminal-manifest finalization remains authorized"
                            .to_vec(),
                    });
                }
                succeeded("krun_host_attachment_terminal", &manifest.network_teardown)
                    .map_err(NetworkTeardownAdapterError::ambiguous_error)
            }
            Ok(HostManagedAttachmentCommandInspection::AuthorizedImmediatePredecessor) => {
                in_progress("Krun attachment retry is durably authorized but not started")
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
                match (provider_observation.kind(), not_started) {
                    (ProviderCommandObservationKind::Claimed, true) => {
                        in_progress("Krun attachment teardown is claimed and can still start")
                            .map_err(NetworkTeardownAdapterError::ambiguous_error)
                    }
                    (ProviderCommandObservationKind::Claimed, false) => {
                        in_progress("Krun attachment teardown is durably in progress")
                            .map_err(NetworkTeardownAdapterError::ambiguous_error)
                    }
                    (
                        ProviderCommandObservationKind::InProgress
                        | ProviderCommandObservationKind::Ambiguous,
                        _,
                    ) => Ok(SandboxNetworkTeardownObservation::RetryAuthorized {
                        evidence:
                            b"Krun exact durable attachment progress authorizes inspected recovery"
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

    fn read_exact_network_manifest(
        &self,
        command: &SandboxNetworkTeardownCommand,
    ) -> crate::Result<KrunSandboxManifest> {
        self.read_exact_manifest(command.tenant_id(), command.sandbox_id())?
            .ok_or_else(|| SandboxError::NotFound {
                sandbox_id: command.sandbox_id().as_str().to_owned(),
            })
    }

    fn authenticate_network_teardown_manifest(
        &self,
        command: &SandboxNetworkTeardownCommand,
        manifest: &KrunSandboxManifest,
    ) -> NetworkTeardownResult<()> {
        if self.config.start_mode != KrunStartMode::Execute
            || manifest.start_mode != KrunStartMode::Execute
            || command.provider_registration_key() != KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY
            || &manifest.spec.tenant_id != command.tenant_id()
            || &manifest.handle.id != command.sandbox_id()
            || !manifest.permits_exact_network_teardown()
        {
            return Err(NetworkTeardownAdapterError::crossed(
                "Krun host-managed attachment composition",
            ));
        }
        manifest
            .require_execution_attempt(command.execution_attempt_id(), "Krun network teardown")
            .map_err(|error| NetworkTeardownAdapterError::crossed(error.to_string()))?;
        let plan = manifest.provision_network_plan.as_ref().ok_or_else(|| {
            NetworkTeardownAdapterError::ambiguous(
                "Krun manifest omitted its durable compiled network plan",
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
                "Krun plan, generation, or attachment identity",
            ));
        }
        manifest
            .network_teardown
            .validate()
            .map_err(NetworkTeardownAdapterError::ambiguous_error)
    }

    fn reconcile_interrupted_attachment_adoption(
        &self,
        manifest: &mut KrunSandboxManifest,
    ) -> NetworkTeardownResult<InterruptedAdoptionDisposition> {
        let KrunLaunchAuthority::Adopting {
            reservation_claim,
            association,
        } = manifest.launch_authority.clone()
        else {
            return Ok(InterruptedAdoptionDisposition::Unchanged);
        };
        let network_config = manifest
            .require_network_config()
            .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        let observation = self
            .segment_allocator
            .inspect_attachment_reservation(
                &manifest.spec.tenant_id,
                &network_config.attachment_id,
                &reservation_claim,
            )
            .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
        if observation.state() != NetworkAttachmentReservationState::Absent {
            let observed_association = observation.association().ok_or_else(|| {
                NetworkTeardownAdapterError::ambiguous(format!(
                    "Krun interrupted attachment adoption allocator state {:?} omitted its exact segment association",
                    observation.state()
                ))
            })?;
            if observed_association != &association {
                return Err(NetworkTeardownAdapterError::crossed(
                    "Krun interrupted attachment adoption segment association",
                ));
            }
        }
        match observation.state() {
            NetworkAttachmentReservationState::Reserved => {
                self.segment_allocator
                    .adopt_reserved_attachment(
                        &manifest.spec.tenant_id,
                        &network_config.attachment_id,
                        &reservation_claim,
                    )
                    .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
                manifest.launch_authority = KrunLaunchAuthority::Adopted { reservation_claim };
                self.persist_effect_barrier(
                    manifest,
                    "Krun interrupted attachment adoption recovery",
                )
                .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
            }
            NetworkAttachmentReservationState::Adopted
            | NetworkAttachmentReservationState::ProviderCleanupPending => {
                manifest.launch_authority = KrunLaunchAuthority::Adopted { reservation_claim };
                self.persist_effect_barrier(
                    manifest,
                    "Krun interrupted attachment adoption recovery",
                )
                .map_err(NetworkTeardownAdapterError::ambiguous_error)?;
            }
            NetworkAttachmentReservationState::Absent
            | NetworkAttachmentReservationState::ReservationCleanupPending => {}
        }
        Ok(InterruptedAdoptionDisposition::ConfirmedNoProviderEffect)
    }

    fn authenticate_network_teardown_snapshot(
        &self,
        command: &SandboxNetworkTeardownCommand,
        manifest: &KrunSandboxManifest,
        journal: &ProviderCommandAttemptJournal,
    ) -> NetworkTeardownResult<Option<ProviderCommandObservation>> {
        self.authenticate_network_teardown_manifest(command, manifest)?;
        require_execution_stopped(
            manifest
                .execution_teardown
                .require_stopped_for_network(command.provider_claim()),
        )?;
        match command.operation() {
            SandboxNetworkTeardownOperation::Detach => Ok(None),
            SandboxNetworkTeardownOperation::Release => {
                read_prior_detach_success(command, &manifest.network_teardown, journal).map(Some)
            }
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
    result: Result<&[u8], KrunNetworkStopRequirementError>,
) -> NetworkTeardownResult<()> {
    match result {
        Ok(_) => Ok(()),
        Err(KrunNetworkStopRequirementError::NotStopped) => {
            Err(NetworkTeardownAdapterError::order_invalid(
                "Krun network teardown requires exact durable ExecutionStopped evidence",
            ))
        }
        Err(KrunNetworkStopRequirementError::Crossed) => Err(NetworkTeardownAdapterError::crossed(
            "Krun durable ExecutionStopped fence",
        )),
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
            message: format!("failed to serialize Krun network teardown evidence: {error}"),
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
        message: format!("Krun network teardown crossed {subject}"),
    }
}

fn network_state_error(
    error: HostManagedAttachmentCommandInspectionError,
) -> NetworkTeardownAdapterError {
    match error {
        HostManagedAttachmentCommandInspectionError::Crossed => {
            NetworkTeardownAdapterError::crossed("Krun durable network progress")
        }
        HostManagedAttachmentCommandInspectionError::EpochInvalid => {
            NetworkTeardownAdapterError::epoch_invalid(
                "Krun durable provider-local progress rejected a stale or nonadjacent epoch",
            )
        }
        HostManagedAttachmentCommandInspectionError::Corrupt => {
            NetworkTeardownAdapterError::ambiguous(
                "Krun durable network teardown progress is corrupt or incomplete",
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

    fn into_observation(self) -> SandboxNetworkTeardownObservation {
        match self {
            Self::Definite { code, message } => definite_failure(code, message),
            Self::Ambiguous { message } => ambiguous(message),
        }
    }
}
