//! Exact, coordinator-issued provision phases for the Container provider.
//!
//! The compute saga owns phase order. This adapter authenticates one durable
//! Container manifest and performs only the requested provider-local effect.
//! In particular, PlanOnly attachment remains private until an explicit
//! machine-publication command reaches this module.

use nimbus_network::{NetworkProviderHandle, NetworkResourceGeneration};
use serde::Serialize;

use crate::backends::conmon::lifecycle::{RuntimeStateObservation, runtime_state};
use crate::backends::oci::network::OciAttachmentBaseReadinessState;
use crate::provision::{
    ProvisionActivationObservationKind, ProvisionActivationRuntimeState,
    classify_provision_activation,
};

use super::machine_port_publication::DurableMachinePortPublicationObservation;
use super::*;

fn phase_evidence(phase: &'static str, value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(&(phase, value)).map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to encode container {phase} evidence: {error}"),
    })
}

fn normalized_activation_runtime_state(state: &str) -> ProvisionActivationRuntimeState {
    match state {
        "running" => ProvisionActivationRuntimeState::Running,
        "creating" => ProvisionActivationRuntimeState::Starting,
        "created" => ProvisionActivationRuntimeState::Startable,
        "stopped" => ProvisionActivationRuntimeState::Exited,
        _ => ProvisionActivationRuntimeState::Unknown,
    }
}

fn require_prepared_runner_pointer(manifest: &ContainerSandboxManifest) -> Result<()> {
    let pointer_path = manifest
        .bundle_layout
        .bundle_dir
        .join(RUNNER_MANIFEST_POINTER_FILE);
    let metadata = std::fs::symlink_metadata(&pointer_path).map_err(|error| {
        SandboxError::OperationFailed {
            message: format!(
                "container provision preparation for {} lacks its runner pointer {}: {error}",
                manifest.handle.id,
                pointer_path.display()
            ),
        }
    })?;
    if !metadata.file_type().is_file() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container provision runner pointer {} for {} is not a regular file",
                pointer_path.display(),
                manifest.handle.id
            ),
        });
    }
    let pointer =
        std::fs::read_to_string(&pointer_path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read container provision runner pointer {}: {error}",
                pointer_path.display()
            ),
        })?;
    if Path::new(pointer.trim()) != manifest.conmon_layout.manifest_path {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container provision runner pointer {} does not name the exact manifest for {}; handoff remains fenced",
                pointer_path.display(),
                manifest.handle.id
            ),
        });
    }
    Ok(())
}

impl ContainerSandboxBackend {
    /// Open this provider's durable provision-attempt idempotency journal.
    pub fn attempt_idempotency_journal(
        &self,
    ) -> std::result::Result<
        crate::ProviderProvisionAttemptJournal,
        crate::ProviderProvisionJournalError,
    > {
        crate::ProviderProvisionAttemptJournal::open(
            &self.config.workload_state_root,
            "container-runtime",
        )
    }

    /// Reserve network authority for one caller-identified workload without
    /// materializing its bundle, rootfs, runtime, attachment, or publication.
    pub fn reserve_provision_network(
        &self,
        spec: SandboxSpec,
        sandbox_id: SandboxId,
        network_plan: SandboxProvisionNetworkPlan,
    ) -> Result<SandboxHandle> {
        self.ensure_startup_reconciliation_ready()?;
        if network_plan.tenant_id() != &spec.tenant_id {
            return Err(SandboxError::InvalidSpec {
                message: "container provision network plan belongs to a different tenant"
                    .to_owned(),
            });
        }
        if let Some(mut manifest) = self.read_manifest(&sandbox_id)? {
            let resolved = resolve_start_spec(&spec, None)?;
            if manifest.spec != resolved.spec
                || manifest.provision_network_plan.as_ref() != Some(&network_plan)
                || manifest.start_mode != self.config.start_mode
                || manifest.provision_prepared
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container provision reservation for {sandbox_id} crossed its exact durable spec, network plan, backend mode, or phase"
                    ),
                });
            }
            if manifest.network_config.is_some() {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container provision reservation for {sandbox_id} already has a durable manifest; inspect it instead of replacing it"
                    ),
                });
            }
            let reservation_claim = manifest
                .launch_reservation_claim
                .clone()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "container provision reservation for {sandbox_id} has a durable desired plan without its reservation claim"
                    ),
                })?;
            self.resume_execute_launch_network_reservation(
                &mut manifest,
                &network_plan,
                &reservation_claim,
            )?;
            return Ok(manifest.handle);
        }
        let plan = self.plan_start_with_id_with_network_reservation(
            &spec,
            &sandbox_id,
            ContainerStartPlanningOptions {
                launch_defaults: None,
                launch_artifact: None,
                provision_network_plan: Some(&network_plan),
                reserve_execute_network: true,
                prepare_bundle: false,
            },
        )?;
        Ok(plan.manifest.handle)
    }

    /// Inspect exact reservation evidence without acquiring resources.
    pub fn inspect_provision_network_reservation(
        &self,
        sandbox_id: &SandboxId,
        expected_plan: &SandboxProvisionNetworkPlan,
    ) -> Result<Option<SandboxHandle>> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(None);
        };
        if manifest.provision_network_plan.as_ref() != Some(expected_plan)
            || manifest.spec.tenant_id != *expected_plan.tenant_id()
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container provision reservation inspection for {sandbox_id} crossed its exact compiled network plan"
                ),
            });
        }
        let Some(network_config) = manifest.network_config.as_ref() else {
            return Ok(None);
        };
        if network_config.attachment_id != *expected_plan.attachment_id()
            || network_config.network_plan.as_ref() != Some(expected_plan.network_plan())
            || manifest.launch_reservation_claim.as_ref() != Some(&network_config.reservation_claim)
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container provision reservation inspection for {sandbox_id} crossed its durable attachment identity, network plan, or reservation claim"
                ),
            });
        }
        Ok(Some(manifest.handle))
    }

    /// Materialize one already-reserved workload without attaching, activating,
    /// or publishing it.
    ///
    /// A PlanOnly workload also publishes the existing PreparedServiceRunner
    /// handoff and exact manifest pointer. Replays adopt that same pointer and
    /// never activate the runner.
    pub fn prepare_provision_workload(&self, sandbox_id: &SandboxId) -> Result<SandboxHandle> {
        let Some(mut manifest) = self.read_manifest(sandbox_id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            });
        };
        if manifest.network_config.is_none() || manifest.launch_reservation_claim.is_none() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container provision preparation for {sandbox_id} requires an exact prior network reservation"
                ),
            });
        }
        if manifest.start_mode != self.config.start_mode {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container provision preparation for {sandbox_id} crossed backend mode {:?} with manifest mode {:?}",
                    self.config.start_mode, manifest.start_mode
                ),
            });
        }

        if manifest.start_mode == ContainerStartMode::PlanOnly {
            if manifest.spec.service_name().is_none() {
                return Err(SandboxError::InvalidSpec {
                    message:
                        "container PlanOnly provision preparation requires service owner metadata"
                            .to_owned(),
                });
            }
            match manifest.lifecycle_coordinator {
                ContainerLifecycleCoordinator::DirectBackend => {
                    manifest.assign_prepared_service_runner()?;
                    self.write_manifest(&manifest)?;
                }
                ContainerLifecycleCoordinator::PreparedServiceRunner => {}
            }
        }

        if !manifest.provision_prepared {
            self.complete_provision_preparation(&mut manifest)?;
        }
        if manifest.start_mode == ContainerStartMode::PlanOnly {
            self.write_runner_manifest_pointer(&manifest)?;
            require_prepared_runner_pointer(&manifest)?;
        }
        Ok(manifest.handle)
    }

    /// Inspect preparation without creating, repairing, or launching provider state.
    pub fn inspect_provision_preparation(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<Option<SandboxHandle>> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(None);
        };
        if !manifest.provision_prepared || !manifest.bundle_layout.config_path.is_file() {
            return Ok(None);
        }
        if manifest.start_mode == ContainerStartMode::PlanOnly
            && (manifest.lifecycle_coordinator
                != ContainerLifecycleCoordinator::PreparedServiceRunner
                || require_prepared_runner_pointer(&manifest).is_err())
        {
            return Ok(None);
        }
        Ok(Some(manifest.handle))
    }

    fn complete_provision_preparation(
        &self,
        manifest: &mut ContainerSandboxManifest,
    ) -> Result<()> {
        let resolved_launch = match &manifest.spec.root {
            SandboxRootSpec::Rootfs(_) => resolve_start_spec(&manifest.spec, None)?,
            SandboxRootSpec::OciImage(image) => {
                let prepared = self.prepare_oci_image_start(
                    &manifest.spec,
                    &manifest.handle.id,
                    &image.source,
                )?;
                manifest.launch_artifact = Some(ContainerLaunchArtifact::Rootfs(prepared.artifact));
                resolve_start_spec(&manifest.spec, Some(&prepared.launch_defaults))?
            }
        };
        let reserved_bindings = manifest.spec.port_bindings.clone();
        let mut prepared_spec = resolved_launch.spec;
        prepared_spec.port_bindings = reserved_bindings;
        manifest.image_metadata = resolved_launch.image_metadata;
        // Publish the exact resolved artifact identity before writing into its
        // bundle, but retain the original OCI source in `manifest.spec` until
        // the final prepared marker commits. A fresh process therefore repeats
        // exact materializer authentication and adopts the same provenance
        // receipt instead of trusting a path or replacing ambiguous state.
        self.write_manifest(manifest)?;
        write_bundle_config(
            &manifest.bundle_layout,
            &hostname_for(&prepared_spec),
            &prepared_spec,
            manifest.image_metadata.user.as_deref(),
            Some(manifest.network_layout.netns_path.as_path()),
            &container_bundle_options(
                &self.config.workload_state_root,
                &prepared_spec,
                &manifest.handle.id,
                manifest.egress_proxy.as_ref(),
            )?,
        )?;
        manifest.spec = prepared_spec;
        manifest.provision_prepared = true;
        self.write_manifest(manifest)
    }

    fn prepared_provision_attachment(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<(
        ContainerSandboxManifest,
        nimbus_network::NetworkReservationClaim,
    )> {
        let manifest = self
            .read_manifest(sandbox_id)?
            .ok_or_else(|| SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            })?;
        if !manifest.provision_prepared {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container provision attachment for {sandbox_id} requires durable preparation"
                ),
            });
        }
        if manifest.start_mode == ContainerStartMode::PlanOnly {
            manifest.require_lifecycle_coordinator(
                ContainerLifecycleCoordinator::PreparedServiceRunner,
                "container PlanOnly provision attachment",
            )?;
            require_prepared_runner_pointer(&manifest)?;
        }
        let claim = manifest
            .launch_reservation_claim
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "container provision attachment for {sandbox_id} lacks reservation authority"
                ),
            })?
            .clone();
        manifest.require_network_config()?;
        Ok((manifest, claim))
    }

    fn require_never_bound_provision_attachment(
        &self,
        manifest: &ContainerSandboxManifest,
        reservation_claim: &nimbus_network::NetworkReservationClaim,
    ) -> Result<()> {
        let launch_batch = Self::provision_port_plan_witness(manifest);
        self.port_lease_coordinator_for_manifest(manifest)?
            .require_never_bound_launch_batch(&launch_batch, reservation_claim)
    }

    fn recover_active_planned_pep_after_owner_death(
        &self,
        manifest: &ContainerSandboxManifest,
        reservation_claim: &nimbus_network::NetworkReservationClaim,
    ) -> Result<bool> {
        let never_bound =
            match self.require_never_bound_provision_attachment(manifest, reservation_claim) {
                Ok(()) => return Ok(false),
                Err(error) => error,
            };
        let ports = self.port_lease_coordinator_for_manifest(manifest)?;
        let hostname = hostname_for(&manifest.spec);
        self.non_routable_attachment_adapter(
            manifest,
            manifest.require_network_config()?,
            &hostname,
        )
        .authenticate_active_deferred_pep_recovery(&self.attachment_lifecycle(&ports))
        .map_err(|recovery_error| SandboxError::OperationFailed {
            message: format!(
                "{never_bound}; exact Active private-attachment PEP recovery also rejected authority: {recovery_error}"
            ),
        })?;
        let plan_members = Self::provision_port_plan_witness(manifest);
        self.ensure_egress_proxy_running_with_release_authority(
            manifest,
            PepPreAdoptionReleaseAuthority::FreshPlannedLaunch {
                reservation_claim,
                plan_members: &plan_members,
            },
        )?;
        match self.inspect_provision_network_attachment(&manifest.handle.id)? {
            crate::SandboxProvisionPhaseObservation::Succeeded { .. } => Ok(true),
            observation => Err(SandboxError::OperationFailed {
                message: format!(
                    "container provision attachment {} recovered its exact dead PEP owner but private readiness remains {observation:?}",
                    manifest.handle.id
                ),
            }),
        }
    }

    pub(super) fn provision_port_plan_witness(
        manifest: &ContainerSandboxManifest,
    ) -> Vec<nimbus_network::PortLeaseRequest> {
        let mut launch_batch = manifest.port_leases.clone();
        if let Some(egress_proxy) = manifest.egress_proxy.as_ref() {
            launch_batch.push(egress_proxy.port_lease.clone());
        }
        launch_batch
    }

    /// Realize the exact private attachment and egress prerequisite without
    /// installing any ingress listener, DNAT mapping, or machine forwarding.
    pub fn attach_provision_network(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<crate::SandboxProvisionPhaseObservation> {
        if matches!(
            self.inspect_provision_network_attachment(sandbox_id)?,
            crate::SandboxProvisionPhaseObservation::Succeeded { .. }
        ) {
            return Ok(crate::SandboxProvisionPhaseObservation::Succeeded {
                evidence: phase_evidence("network_attachment_replayed", sandbox_id)?,
            });
        }
        let (mut manifest, reservation_claim) = self.prepared_provision_attachment(sandbox_id)?;
        if self.recover_active_planned_pep_after_owner_death(&manifest, &reservation_claim)? {
            return Ok(crate::SandboxProvisionPhaseObservation::Succeeded {
                evidence: phase_evidence("network_attachment_pep_recovered", sandbox_id)?,
            });
        }
        self.require_never_bound_provision_attachment(&manifest, &reservation_claim)?;
        self.segment_allocator.adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &manifest.require_network_config()?.attachment_id,
            &reservation_claim,
        )?;
        manifest.network_cleanup_complete = false;
        self.configure_network(
            &manifest,
            AttachmentAttachAuthority::FreshLaunch(&reservation_claim),
            MachinePortPreparationReleaseAuthority::FreshLaunch(&reservation_claim),
            false,
        )?;
        let plan_members = Self::provision_port_plan_witness(&manifest);
        self.ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshPlannedLaunch {
                reservation_claim: &reservation_claim,
                plan_members: &plan_members,
            },
        )?;
        self.require_authenticated_egress_readiness(&manifest)?;
        Ok(crate::SandboxProvisionPhaseObservation::Succeeded {
            evidence: phase_evidence("network_attached", &manifest.handle)?,
        })
    }

    #[cfg(test)]
    pub(super) fn attach_provision_network_with_test_host(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<crate::SandboxProvisionPhaseObservation> {
        let (manifest, reservation_claim) = self.prepared_provision_attachment(sandbox_id)?;
        if self.recover_active_planned_pep_after_owner_death(&manifest, &reservation_claim)? {
            return Ok(crate::SandboxProvisionPhaseObservation::Succeeded {
                evidence: phase_evidence("network_attachment_pep_recovered", sandbox_id)?,
            });
        }
        self.require_never_bound_provision_attachment(&manifest, &reservation_claim)?;
        self.segment_allocator.adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &manifest.require_network_config()?.attachment_id,
            &reservation_claim,
        )?;
        let ports = self.port_lease_coordinator_for_manifest(&manifest)?;
        let hostname = hostname_for(&manifest.spec);
        self.non_routable_attachment_adapter(
            &manifest,
            manifest.require_network_config()?,
            &hostname,
        )
        .attach_with_test_host(
            &self.attachment_lifecycle(&ports),
            AttachmentAttachAuthority::FreshLaunch(&reservation_claim),
            |_| {
                if let Some(proxy) = manifest.egress_proxy.as_ref() {
                    self.egress_pin_provider
                        .apply(&manifest.network_layout, proxy)?;
                }
                Ok(())
            },
        )?;
        let plan_members = Self::provision_port_plan_witness(&manifest);
        self.ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshPlannedLaunch {
                reservation_claim: &reservation_claim,
                plan_members: &plan_members,
            },
        )?;
        self.require_authenticated_egress_readiness(&manifest)?;
        Ok(crate::SandboxProvisionPhaseObservation::Succeeded {
            evidence: phase_evidence("network_attached", &manifest.handle)?,
        })
    }

    /// Inspect the non-routable attachment without creating or repairing it.
    pub fn inspect_provision_network_attachment(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<crate::SandboxProvisionPhaseObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(crate::SandboxProvisionPhaseObservation::Absent {
                evidence: phase_evidence("network_attachment_absent", sandbox_id)?,
            });
        };
        if manifest.network_config.is_none() || manifest.launch_reservation_claim.is_none() {
            return Ok(crate::SandboxProvisionPhaseObservation::Absent {
                evidence: phase_evidence("network_attachment_unreserved", &manifest.handle)?,
            });
        }
        let readiness = self.non_routable_attachment_readiness(
            &manifest,
            self.authenticated_egress_readiness(&manifest)?,
        )?;
        let evidence = phase_evidence("network_attachment_inspection", &format!("{readiness:?}"))?;
        match readiness {
            OciAttachmentBaseReadinessState::Ready(_) => {
                Ok(crate::SandboxProvisionPhaseObservation::Succeeded { evidence })
            }
            OciAttachmentBaseReadinessState::NotReady(_) => {
                Ok(crate::SandboxProvisionPhaseObservation::InProgress { evidence })
            }
        }
    }

    /// Inspect exact attachment and PEP evidence required before activation.
    pub fn inspect_provision_activation_prerequisites(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<crate::SandboxProvisionPhaseObservation> {
        self.inspect_provision_network_attachment(sandbox_id)
    }

    /// Start only a direct Execute-mode runtime. PlanOnly activation belongs
    /// to the guest node provider and cannot be inferred at this adapter.
    pub fn activate_provision_workload(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<crate::SandboxProvisionPhaseObservation> {
        if self.config.start_mode != ContainerStartMode::Execute {
            return Err(SandboxError::InvalidSpec {
                message:
                    "container PlanOnly provision activation is owned by the guest node provider"
                        .to_owned(),
            });
        }
        ensure_linux_host("container")?;
        let Some(mut manifest) = self.read_manifest(sandbox_id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            });
        };
        match self.inspect_provision_activation_prerequisites(sandbox_id)? {
            crate::SandboxProvisionPhaseObservation::Succeeded { .. } => {}
            observation => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container provision activation for {sandbox_id} lacks exact prerequisites: {observation:?}"
                    ),
                });
            }
        }
        let runtime_state = self.spawn_creator_and_wait_for_runtime(&mut manifest)?;
        if runtime_state != "running" {
            run_status_checked(&manifest.conmon_launch.start_command)?;
        }
        manifest.shutdown_requested = false;
        manifest.next_restart_at_millis = None;
        manifest.last_exit_code = None;
        synchronize_handle_status(&mut manifest, SandboxStatus::Starting);
        self.write_manifest(&manifest)?;
        Ok(crate::SandboxProvisionPhaseObservation::Succeeded {
            evidence: phase_evidence("workload_activated", &manifest.handle)?,
        })
    }

    /// Inspect direct Execute-mode activation without restarting or repairing.
    pub fn inspect_provision_workload_activation(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<crate::SandboxProvisionPhaseObservation> {
        if self.config.start_mode != ContainerStartMode::Execute {
            return Err(SandboxError::InvalidSpec {
                message:
                    "container PlanOnly activation inspection is owned by the guest node provider"
                        .to_owned(),
            });
        }
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(crate::SandboxProvisionPhaseObservation::Absent {
                evidence: phase_evidence("workload_activation_absent", sandbox_id)?,
            });
        };
        match runtime_state(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
        ) {
            Ok(RuntimeStateObservation::Present(state)) => {
                let evidence = phase_evidence(
                    "workload_activation_runtime_state",
                    &(&manifest.handle, &state),
                )?;
                match classify_provision_activation(normalized_activation_runtime_state(&state)) {
                    ProvisionActivationObservationKind::Succeeded => {
                        Ok(crate::SandboxProvisionPhaseObservation::Succeeded { evidence })
                    }
                    ProvisionActivationObservationKind::Absent => {
                        Ok(crate::SandboxProvisionPhaseObservation::Absent { evidence })
                    }
                    ProvisionActivationObservationKind::InProgress => {
                        Ok(crate::SandboxProvisionPhaseObservation::InProgress { evidence })
                    }
                    ProvisionActivationObservationKind::Ambiguous => {
                        Ok(crate::SandboxProvisionPhaseObservation::Ambiguous { evidence })
                    }
                }
            }
            Ok(RuntimeStateObservation::ExplicitlyAbsent) => {
                Ok(crate::SandboxProvisionPhaseObservation::Absent {
                    evidence: phase_evidence("workload_activation_absent", &manifest.handle)?,
                })
            }
            Err(error) => Ok(crate::SandboxProvisionPhaseObservation::Ambiguous {
                evidence: phase_evidence("workload_activation_unknown", &error.to_string())?,
            }),
        }
    }

    /// Inspect direct Execute-mode application readiness through the private
    /// attachment. Guest node readiness owns the PlanOnly path.
    pub fn inspect_provision_workload_readiness(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<crate::SandboxProvisionPhaseObservation> {
        if self.config.start_mode != ContainerStartMode::Execute {
            return Err(SandboxError::InvalidSpec {
                message:
                    "container PlanOnly workload readiness is owned by the guest node provider"
                        .to_owned(),
            });
        }
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(crate::SandboxProvisionPhaseObservation::Absent {
                evidence: phase_evidence("workload_readiness_absent", sandbox_id)?,
            });
        };
        if !matches!(
            self.inspect_provision_workload_activation(sandbox_id)?,
            crate::SandboxProvisionPhaseObservation::Succeeded { .. }
        ) {
            return Ok(crate::SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "workload_readiness_waiting_for_activation",
                    &manifest.handle,
                )?,
            });
        }
        let attachment = self.non_routable_attachment_readiness(
            &manifest,
            self.authenticated_egress_readiness(&manifest)?,
        )?;
        let OciAttachmentBaseReadinessState::Ready(attachment) = attachment else {
            return Ok(crate::SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "workload_readiness_waiting_for_attachment",
                    &manifest.handle,
                )?,
            });
        };
        let Some(assigned_ip) = attachment.assigned_ips().first().copied() else {
            return Ok(crate::SandboxProvisionPhaseObservation::Ambiguous {
                evidence: phase_evidence(
                    "workload_readiness_missing_private_address",
                    &manifest.handle,
                )?,
            });
        };
        let private_endpoints = manifest
            .spec
            .port_bindings
            .iter()
            .map(|binding| {
                nimbus_network::PublishedEndpoint::new(
                    binding.name.clone(),
                    binding.protocol,
                    std::net::SocketAddr::new(assigned_ip.into(), binding.guest_port),
                )
            })
            .collect::<Vec<_>>();
        let application = crate::backends::readiness_probe::inspect_application_readiness(
            manifest.status,
            &private_endpoints,
            status::readiness_probe_timeout(&manifest),
            self.readiness_probe_provider.as_ref(),
        );
        let evidence = phase_evidence(
            "workload_readiness_inspection",
            &(manifest.handle, application.clone()),
        )?;
        if application.status() == SandboxStatus::Ready {
            Ok(crate::SandboxProvisionPhaseObservation::Succeeded { evidence })
        } else {
            Ok(crate::SandboxProvisionPhaseObservation::InProgress { evidence })
        }
    }

    fn authenticate_machine_publication_request<'a>(
        &self,
        manifest: &'a ContainerSandboxManifest,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
    ) -> Result<&'a crate::backends::oci::network::OciMachinePortForwarderConfig> {
        self.validate_manifest_execution_context(manifest)?;
        manifest.require_lifecycle_coordinator(
            ContainerLifecycleCoordinator::PreparedServiceRunner,
            "container provision machine publication",
        )?;
        if !manifest.provision_prepared {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container provision machine publication for {} requires durable preparation",
                    manifest.handle.id
                ),
            });
        }
        require_prepared_runner_pointer(manifest)?;
        let config = manifest.require_network_config()?;
        let durable_plan = config.network_plan.as_ref().ok_or_else(|| {
            SandboxError::OperationFailed {
                message: format!(
                    "container provision machine publication for {} lacks a compiled network plan",
                    manifest.handle.id
                ),
            }
        })?;
        if network_plan.tenant_id() != &manifest.spec.tenant_id
            || network_plan.network_plan() != durable_plan
            || network_plan.generation() != durable_plan.generation()
            || network_plan.attachment_id() != &config.attachment_id
            || network_plan.bindings() != manifest.spec.port_bindings
            || network_plan.port_leases() != manifest.port_leases
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container provision machine publication for {} crossed its exact plan, attachment, listener, lease, or generation authority",
                    manifest.handle.id
                ),
            });
        }
        if manifest.launch_reservation_claim.is_none() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container provision machine publication for {} lacks its retained launch reservation",
                    manifest.handle.id
                ),
            });
        }
        let forwarder = manifest
            .runner_config
            .validated_machine_port_forwarder(&manifest.handle.id)?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "container provision machine publication for {} lacks configured forwarder authority",
                    manifest.handle.id
                ),
            })?;
        if forwarder.provider_instance() != provider_instance
            || forwarder.provider_generation() != provider_generation
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container provision machine publication for {} crossed the configured forwarder provider generation",
                    manifest.handle.id
                ),
            });
        }
        Ok(forwarder)
    }

    pub(super) fn ready_machine_publication_address(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<std::net::Ipv4Addr> {
        let readiness = self.non_routable_attachment_readiness(
            manifest,
            self.authenticated_egress_readiness(manifest)?,
        )?;
        let OciAttachmentBaseReadinessState::Ready(attachment) = readiness else {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container provision machine publication for {} requires an authenticated private attachment and egress prerequisite",
                    manifest.handle.id
                ),
            });
        };
        attachment.assigned_ips().first().copied().ok_or_else(|| {
            SandboxError::OperationFailed {
                message: format!(
                    "container provision machine publication for {} has no authenticated private route address",
                    manifest.handle.id
                ),
            }
        })
    }

    fn publish_provision_machine_ingress_with(
        &self,
        sandbox_id: &SandboxId,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
        publish: impl FnOnce(&ContainerSandboxManifest) -> Result<()>,
    ) -> Result<crate::SandboxProvisionPhaseObservation> {
        let manifest = self
            .read_manifest(sandbox_id)?
            .ok_or_else(|| SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            })?;
        self.authenticate_machine_publication_request(
            &manifest,
            network_plan,
            provider_instance,
            provider_generation,
        )?;
        let assigned_ip = self.ready_machine_publication_address(&manifest)?;
        let reservation_claim = manifest
            .launch_reservation_claim
            .as_ref()
            .expect("authenticated publication retains its launch reservation");
        let plan_members = Self::provision_port_plan_witness(&manifest);
        self.ensure_machine_port_proxies_running_with_publication(
            sandbox_id,
            &[assigned_ip],
            &manifest,
            MachinePortPreparationReleaseAuthority::FreshPlannedLaunch {
                reservation_claim,
                plan_members: &plan_members,
            },
            || publish(&manifest),
        )?;
        let receipts = self.exposed_machine_port_receipts(sandbox_id)?;
        Ok(crate::SandboxProvisionPhaseObservation::Succeeded {
            evidence: phase_evidence(
                "machine_ingress_published",
                &(
                    network_plan.plan_id(),
                    network_plan.attachment_id(),
                    provider_instance,
                    provider_generation,
                    receipts,
                ),
            )?,
        })
    }

    /// Create the first host-routable effect for one exact machine-forwarded
    /// provision attempt, then durably converge its provider receipts.
    pub fn publish_provision_machine_ingress(
        &self,
        sandbox_id: &SandboxId,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
    ) -> Result<crate::SandboxProvisionPhaseObservation> {
        self.publish_provision_machine_ingress_with(
            sandbox_id,
            network_plan,
            provider_instance,
            provider_generation,
            |manifest| self.converge_exposed_machine_port_publication(manifest),
        )
    }

    #[cfg(test)]
    pub(super) fn publish_provision_machine_ingress_with_test_provider(
        &self,
        sandbox_id: &SandboxId,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
    ) -> Result<crate::SandboxProvisionPhaseObservation> {
        self.publish_provision_machine_ingress_with(
            sandbox_id,
            network_plan,
            provider_instance,
            provider_generation,
            |manifest| self.converge_exposed_machine_port_publication_for_test(manifest),
        )
    }

    /// Inspect machine publication without starting listeners, repairing local
    /// workers, or mutating the external forwarder.
    pub fn inspect_provision_machine_ingress(
        &self,
        sandbox_id: &SandboxId,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
    ) -> Result<crate::SandboxProvisionPhaseObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(crate::SandboxProvisionPhaseObservation::Absent {
                evidence: phase_evidence("machine_ingress_manifest_absent", sandbox_id)?,
            });
        };
        self.authenticate_machine_publication_request(
            &manifest,
            network_plan,
            provider_instance,
            provider_generation,
        )?;
        let assigned_ip = match self.ready_machine_publication_address(&manifest) {
            Ok(address) => address,
            Err(error) => {
                return Ok(crate::SandboxProvisionPhaseObservation::InProgress {
                    evidence: phase_evidence(
                        "machine_ingress_waiting_for_private_attachment",
                        &error.to_string(),
                    )?,
                });
            }
        };
        match self.inspect_durable_machine_port_publication(&manifest)? {
            DurableMachinePortPublicationObservation::Absent => {
                let reservation_claim = manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("authenticated machine publication retains its reservation claim");
                let plan_members = Self::provision_port_plan_witness(&manifest);
                let never_effected = self
                    .port_lease_coordinator_for_manifest(&manifest)
                    .and_then(|coordinator| {
                        coordinator.authority()?.inspect_plan_members_never_effected(
                            &plan_members,
                            &manifest.port_leases,
                            reservation_claim,
                        )
                        .map_err(|error| SandboxError::OperationFailed {
                            message: format!(
                                "machine ingress lease inspection rejected exact authority: {error}"
                            ),
                        })
                    });
                match never_effected {
                    Ok(true) => Ok(crate::SandboxProvisionPhaseObservation::Absent {
                        evidence: phase_evidence(
                            "machine_ingress_never_published",
                            &(network_plan.plan_id(), network_plan.generation()),
                        )?,
                    }),
                    Ok(false) => Ok(crate::SandboxProvisionPhaseObservation::Ambiguous {
                        evidence: phase_evidence(
                            "machine_ingress_missing_journal_with_listener_effect",
                            &(network_plan.plan_id(), network_plan.generation()),
                        )?,
                    }),
                    Err(error) => Ok(crate::SandboxProvisionPhaseObservation::Ambiguous {
                        evidence: phase_evidence(
                            "machine_ingress_listener_authority_unverifiable",
                            &(
                                network_plan.plan_id(),
                                network_plan.generation(),
                                error.to_string(),
                            ),
                        )?,
                    }),
                }
            }
            DurableMachinePortPublicationObservation::InProgress { generation } => {
                Ok(crate::SandboxProvisionPhaseObservation::InProgress {
                    evidence: phase_evidence(
                        "machine_ingress_publication_in_progress",
                        &(network_plan.plan_id(), generation),
                    )?,
                })
            }
            DurableMachinePortPublicationObservation::Exposed { receipts } => {
                match self.inspect_machine_forwarded_publication(&manifest, &[assigned_ip]) {
                    Ok(current) => Ok(crate::SandboxProvisionPhaseObservation::Succeeded {
                        evidence: phase_evidence(
                            "machine_ingress_current",
                            &(
                                current.tenant_id(),
                                current.sandbox_id(),
                                current.provider_instance(),
                                current.provider_generation(),
                                receipts,
                            ),
                        )?,
                    }),
                    Err(error) => Ok(crate::SandboxProvisionPhaseObservation::Ambiguous {
                        evidence: phase_evidence(
                            "machine_ingress_current_observation_ambiguous",
                            &error.to_string(),
                        )?,
                    }),
                }
            }
        }
    }

    /// Inspect the exact private routes a server-owned ingress adapter may publish.
    ///
    /// This performs no bind, proxy, Netavark, or forwarding effect. Stable
    /// authority comes from `network_plan`; the assigned IP is returned only
    /// as provider-observed route data.
    pub fn inspect_provision_server_ingress_targets(
        &self,
        sandbox_id: &SandboxId,
        network_plan: &SandboxProvisionNetworkPlan,
    ) -> Result<crate::SandboxProvisionIngressTargetObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(crate::SandboxProvisionIngressTargetObservation::Absent {
                evidence: phase_evidence("server_ingress_target_absent", sandbox_id)?,
            });
        };
        if manifest
            .runner_config
            .validated_machine_port_forwarder(&manifest.handle.id)?
            .is_some()
        {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "container sandbox {sandbox_id} uses machine-forwarded publication and cannot supply server-owned ingress routes"
                ),
            });
        }
        let config = manifest.require_network_config()?;
        let Some(durable_plan) = config.network_plan.as_ref() else {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container sandbox {sandbox_id} lacks its durable compiled network plan"
                ),
            });
        };
        let readiness = self.non_routable_attachment_readiness(
            &manifest,
            self.authenticated_egress_readiness(&manifest)?,
        )?;
        let OciAttachmentBaseReadinessState::Ready(attachment) = readiness else {
            return Ok(
                crate::SandboxProvisionIngressTargetObservation::InProgress {
                    evidence: phase_evidence(
                        "server_ingress_target_waiting_for_private_attachment",
                        &manifest.handle,
                    )?,
                },
            );
        };
        let Some(assigned_ip) = attachment.assigned_ips().first().copied() else {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container sandbox {sandbox_id} has ready attachment evidence without a private address"
                ),
            });
        };
        let reservation_claim = manifest
            .launch_reservation_claim
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "container sandbox {sandbox_id} lacks launch reservation authority for deferred ingress"
                ),
            })?
            .clone();
        let targets = crate::SandboxProvisionIngressTargets::from_private_attachment(
            network_plan,
            &manifest.spec,
            durable_plan,
            &config.attachment_id,
            reservation_claim,
            assigned_ip.into(),
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "container sandbox {sandbox_id} rejected crossed server-ingress targets: {error}"
            ),
        })?;
        Ok(crate::SandboxProvisionIngressTargetObservation::Ready {
            evidence: phase_evidence(
                "server_ingress_targets_ready",
                &(
                    targets.plan_id(),
                    targets.attachment_id(),
                    targets.generation(),
                    targets
                        .routes()
                        .iter()
                        .map(|route| route.listener_id())
                        .collect::<Vec<_>>(),
                ),
            )?,
            targets,
        })
    }
}
