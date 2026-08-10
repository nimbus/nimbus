//! Narrow provision phases for the krun execution provider.
//!
//! The compute saga owns ordering. This adapter preserves krun's provider-local
//! launch authority while keeping attachment, activation, readiness, and later
//! ingress publication as distinct effects.

use super::readiness::synchronize_handle_status;
use super::*;
use crate::backends::oci::egress::PepPreAdoptionReleaseAuthority;
use crate::backends::oci::network::OciAttachmentBaseReadinessState;
use crate::backends::readiness_probe::inspect_application_readiness;
use crate::provision::{
    ProvisionActivationObservationKind, ProvisionActivationRuntimeState,
    SandboxProvisionPhaseObservation, classify_provision_activation,
};

fn phase_evidence(phase: &'static str, value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(&(phase, value)).map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to encode krun {phase} evidence: {error}"),
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

impl KrunSandboxBackend {
    /// Reserve the exact attachment and listener leases before materializing
    /// workload-owned artifacts.
    pub fn reserve_provision_network(
        &self,
        spec: SandboxSpec,
        sandbox_id: SandboxId,
        execution_attempt_id: crate::SandboxExecutionAttemptId,
        network_plan: SandboxProvisionNetworkPlan,
    ) -> Result<SandboxHandle> {
        if network_plan.tenant_id() != &spec.tenant_id {
            return Err(SandboxError::InvalidSpec {
                message: "krun provision network plan belongs to a different tenant".to_owned(),
            });
        }
        if self.read_manifest(&sandbox_id)?.is_some() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun provision reservation for {sandbox_id} already has a durable manifest; inspect it instead of replacing it"
                ),
            });
        }
        self.plan_reserved_provision_with_id(
            &spec,
            &sandbox_id,
            execution_attempt_id,
            &network_plan,
        )
        .map(|plan| plan.manifest.handle)
    }

    /// Inspect durable reservation evidence without allocating or repairing.
    pub fn inspect_provision_network_reservation(
        &self,
        sandbox_id: &SandboxId,
        expected_attempt_id: &crate::SandboxExecutionAttemptId,
        expected_plan: &SandboxProvisionNetworkPlan,
    ) -> Result<Option<SandboxHandle>> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(None);
        };
        if &manifest.execution_attempt_id != expected_attempt_id
            || manifest.provision_network_plan.as_ref() != Some(expected_plan)
            || manifest.spec.tenant_id != *expected_plan.tenant_id()
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun provision reservation inspection for {sandbox_id} crossed its exact execution attempt or compiled network plan"
                ),
            });
        }
        let network_config = manifest.require_network_config()?;
        if network_config.attachment_id != *expected_plan.attachment_id()
            || network_config.network_plan.as_ref() != Some(expected_plan.network_plan())
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun provision reservation inspection for {sandbox_id} crossed its durable attachment identity or network plan"
                ),
            });
        }
        if !matches!(
            manifest.launch_authority,
            KrunLaunchAuthority::Reserved { .. }
        ) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun provision reservation inspection for {sandbox_id} requires reserved launch authority, got {:?}",
                    manifest.launch_authority
                ),
            });
        }
        let reservation_claim = manifest.require_reserved_claim()?;
        let attachment = self.segment_allocator.inspect_attachment_reservation(
            expected_plan.tenant_id(),
            expected_plan.attachment_id(),
            reservation_claim,
        )?;
        let Some(association) = attachment.association() else {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun provision reservation inspection for {sandbox_id} lacks its exact bound segment association"
                ),
            });
        };
        if attachment.state() != nimbus_network::NetworkAttachmentReservationState::Reserved
            || association.reservation_claim() != reservation_claim
            || association.segment_id().as_str() != network_config.segment_id
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun provision reservation inspection for {sandbox_id} crossed or lost its exact allocator authority"
                ),
            });
        }

        let expected_requests = Self::provision_port_plan_witness(&manifest);
        let port_coordinator = self.port_lease_coordinator();
        let port_authority = port_coordinator.authority()?;
        let never_effected = port_authority
            .inspect_plan_members_never_effected(
                &expected_requests,
                &expected_requests,
                reservation_claim,
            )
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "krun provision reservation inspection for {sandbox_id} could not authenticate its complete exact port plan: {error}"
                ),
            })?;
        if !never_effected {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun provision reservation inspection for {sandbox_id} found prior bind, adoption, binding, stop, failure, or lifetime evidence in its port authority"
                ),
            });
        }

        Ok(Some(manifest.handle))
    }

    /// Materialize the already-reserved rootfs, bundle, and VM configuration.
    pub fn prepare_provision_workload(
        &self,
        sandbox_id: &SandboxId,
        expected_attempt_id: &crate::SandboxExecutionAttemptId,
    ) -> Result<SandboxHandle> {
        let Some(snapshot) = self.read_manifest(sandbox_id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            });
        };
        snapshot.require_execution_attempt(expected_attempt_id, "krun provision preparation")?;
        let _lifecycle = self.lock_launch_lifecycle(&snapshot)?;
        let Some(mut manifest) = self.read_manifest(sandbox_id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            });
        };
        manifest.require_execution_attempt(expected_attempt_id, "krun provision preparation")?;
        manifest.require_execution_admission_open("Krun provision preparation")?;
        self.require_current_launch_plan(&manifest)?;
        if manifest.network_config.is_none() || manifest.reservation_claim().is_none() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun provision preparation for {sandbox_id} requires an exact prior network reservation"
                ),
            });
        }
        if manifest.provision_prepared {
            return Ok(manifest.handle);
        }

        let resolved_launch = match &manifest.spec.root {
            SandboxRootSpec::Rootfs(_) => start::resolve_start_spec(&manifest.spec, None)?,
            SandboxRootSpec::OciImage(image) => {
                let prepared = self.prepare_oci_image_start(
                    &manifest.spec,
                    &manifest.handle.id,
                    &image.source,
                )?;
                let resolved =
                    start::resolve_start_spec(&manifest.spec, Some(&prepared.launch_defaults))?;
                manifest.launch_artifact = Some(KrunLaunchArtifact::Rootfs(prepared.artifact));
                resolved
            }
        };
        let reserved_bindings = manifest.spec.port_bindings.clone();
        manifest.spec = resolved_launch.spec;
        manifest.spec.port_bindings = reserved_bindings;
        manifest.image_metadata = resolved_launch.image_metadata;
        start::apply_guest_user_switch(&mut manifest.spec, &manifest.image_metadata)?;

        // Persist the resolved artifact identity before writing into it. A
        // retry after a crash adopts this exact rootfs instead of materializing
        // a second workload artifact.
        self.write_manifest(&manifest)?;
        let options = start::krun_bundle_options(
            &self.config,
            &manifest.spec,
            &manifest.image_metadata,
            sandbox_id,
            manifest.egress_proxy.as_ref(),
        )?;
        write_bundle_config(
            &manifest.bundle_layout,
            &start::hostname_for(&manifest.spec),
            &manifest.spec,
            Some(manifest.network_layout.netns_path.as_path()),
            &options,
        )?;
        self.materialize_krun_vm_config(&manifest)?;
        manifest.provision_prepared = true;
        self.write_manifest(&manifest)?;
        Ok(manifest.handle)
    }

    /// Inspect preparation without creating or repairing artifacts.
    pub fn inspect_provision_preparation(
        &self,
        sandbox_id: &SandboxId,
        expected_attempt_id: &crate::SandboxExecutionAttemptId,
    ) -> Result<Option<SandboxHandle>> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(None);
        };
        manifest.require_execution_attempt(
            expected_attempt_id,
            "krun provision preparation inspection",
        )?;
        if manifest.provision_prepared && manifest.bundle_layout.config_path.is_file() {
            Ok(Some(manifest.handle))
        } else {
            Ok(None)
        }
    }

    pub(super) fn provision_port_plan_witness(
        manifest: &KrunSandboxManifest,
    ) -> Vec<nimbus_network::PortLeaseRequest> {
        let mut plan_members = manifest.port_leases.clone();
        if let Some(egress_proxy) = manifest.egress_proxy.as_ref() {
            plan_members.push(egress_proxy.port_lease.clone());
        }
        plan_members
    }

    fn require_never_bound_provision_attachment(
        &self,
        manifest: &KrunSandboxManifest,
        reservation_claim: &nimbus_network::NetworkReservationClaim,
    ) -> Result<()> {
        self.port_lease_coordinator()
            .require_never_bound_launch_batch(
                &Self::provision_port_plan_witness(manifest),
                reservation_claim,
            )
    }

    pub(super) fn start_planned_provision_pep(
        &self,
        manifest: &KrunSandboxManifest,
        reservation_claim: &nimbus_network::NetworkReservationClaim,
    ) -> Result<()> {
        let plan_members = Self::provision_port_plan_witness(manifest);
        self.ensure_egress_proxy_running_with_release_authority(
            manifest,
            PepPreAdoptionReleaseAuthority::FreshPlannedLaunch {
                reservation_claim,
                plan_members: &plan_members,
            },
        )
    }

    fn recover_active_planned_pep_after_owner_death(
        &self,
        manifest: &KrunSandboxManifest,
        expected_attempt_id: &crate::SandboxExecutionAttemptId,
        reservation_claim: &nimbus_network::NetworkReservationClaim,
    ) -> Result<bool> {
        let never_bound =
            match self.require_never_bound_provision_attachment(manifest, reservation_claim) {
                Ok(()) => return Ok(false),
                Err(error) => error,
            };
        let ports = self.port_lease_coordinator();
        let hostname = start::hostname_for(&manifest.spec);
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
        self.start_planned_provision_pep(manifest, reservation_claim)?;
        match self.inspect_provision_network_attachment(&manifest.handle.id, expected_attempt_id)? {
            SandboxProvisionPhaseObservation::Succeeded { .. } => Ok(true),
            observation => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun provision attachment {} recovered its exact dead PEP owner but private readiness remains {observation:?}",
                    manifest.handle.id
                ),
            }),
        }
    }

    fn require_current_provision_attachment_plan(
        &self,
        candidate: &KrunSandboxManifest,
    ) -> Result<()> {
        let persisted = self
            .read_manifest(&candidate.handle.id)?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "krun provision attachment {} has no durable manifest; refusing provider effects",
                    candidate.handle.id
                ),
            })?;
        if persisted == *candidate
            && matches!(
                persisted.launch_authority,
                KrunLaunchAuthority::Reserved { .. } | KrunLaunchAuthority::Adopted { .. }
            )
        {
            return Ok(());
        }
        Err(SandboxError::OperationFailed {
            message: format!(
                "krun provision attachment {} no longer owns its exact durable reserved or adopted plan",
                candidate.handle.id
            ),
        })
    }

    /// Attach the private network and start its PEP without publishing ingress.
    pub fn attach_provision_network(
        &self,
        sandbox_id: &SandboxId,
        expected_attempt_id: &crate::SandboxExecutionAttemptId,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(snapshot) = self.read_manifest(sandbox_id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            });
        };
        snapshot.require_execution_attempt(expected_attempt_id, "krun provision attachment")?;
        let _lifecycle = self.lock_launch_lifecycle(&snapshot)?;
        let Some(mut manifest) = self.read_manifest(sandbox_id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            });
        };
        manifest.require_execution_attempt(expected_attempt_id, "krun provision attachment")?;
        manifest.require_execution_admission_open("Krun provision attachment")?;
        self.require_current_provision_attachment_plan(&manifest)?;
        if !manifest.provision_prepared {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun provision attachment for {sandbox_id} requires durable preparation"
                ),
            });
        }
        let reservation_claim = manifest.reservation_claim().cloned().ok_or_else(|| {
            SandboxError::OperationFailed {
                message: format!(
                    "krun provision attachment for {sandbox_id} lacks reservation-derived attachment authority"
                ),
            }
        })?;
        if self.recover_active_planned_pep_after_owner_death(
            &manifest,
            expected_attempt_id,
            &reservation_claim,
        )? {
            return Ok(SandboxProvisionPhaseObservation::Succeeded {
                evidence: phase_evidence("network_attachment_pep_recovered", sandbox_id)?,
            });
        }
        self.require_never_bound_provision_attachment(&manifest, &reservation_claim)?;
        manifest.mark_adopting()?;
        self.persist_effect_barrier(&manifest, "krun provision attachment-adoption intent")?;
        let attachment_id = manifest
            .network_config
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "krun provision attachment for {sandbox_id} lacks reserved network config"
                ),
            })?
            .attachment_id
            .clone();
        self.segment_allocator.adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &attachment_id,
            &reservation_claim,
        )?;
        manifest.mark_adopted()?;
        self.persist_effect_barrier(&manifest, "krun provision adopted attachment authority")?;
        self.configure_network(
            &manifest,
            AttachmentAttachAuthority::FreshLaunch(&reservation_claim),
            false,
        )?;
        self.start_planned_provision_pep(&manifest, &reservation_claim)?;
        self.require_non_routable_activation_prerequisites(&manifest)?;
        Ok(SandboxProvisionPhaseObservation::Succeeded {
            evidence: phase_evidence("network_attached", &manifest.handle)?,
        })
    }

    /// Inspect the complete private attachment without provider effects.
    pub fn inspect_provision_network_attachment(
        &self,
        sandbox_id: &SandboxId,
        expected_attempt_id: &crate::SandboxExecutionAttemptId,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: phase_evidence("network_attachment_absent", sandbox_id)?,
            });
        };
        manifest.require_execution_attempt(
            expected_attempt_id,
            "krun provision attachment inspection",
        )?;
        let readiness = self.non_routable_attachment_readiness(&manifest)?;
        let evidence = phase_evidence("network_attachment_inspection", &format!("{readiness:?}"))?;
        match readiness {
            OciAttachmentBaseReadinessState::Ready(_) => {
                Ok(SandboxProvisionPhaseObservation::Succeeded { evidence })
            }
            OciAttachmentBaseReadinessState::NotReady(_) => {
                Ok(SandboxProvisionPhaseObservation::InProgress { evidence })
            }
        }
    }

    pub fn inspect_provision_activation_prerequisites(
        &self,
        sandbox_id: &SandboxId,
        expected_attempt_id: &crate::SandboxExecutionAttemptId,
    ) -> Result<SandboxProvisionPhaseObservation> {
        self.inspect_provision_network_attachment(sandbox_id, expected_attempt_id)
    }

    /// Activate the VMM after the private attachment is ready. This does not
    /// install a host listener or make a published endpoint visible.
    pub fn activate_provision_workload(
        &self,
        sandbox_id: &SandboxId,
        expected_attempt_id: &crate::SandboxExecutionAttemptId,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(snapshot) = self.read_manifest(sandbox_id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            });
        };
        snapshot.require_execution_attempt(expected_attempt_id, "krun provision activation")?;
        let _lifecycle = self.lock_launch_lifecycle(&snapshot)?;
        let Some(mut manifest) = self.read_manifest(sandbox_id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            });
        };
        manifest.require_execution_attempt(expected_attempt_id, "krun provision activation")?;
        manifest.require_execution_admission_open("Krun provision activation")?;
        self.require_current_launch_plan(&manifest)?;
        ensure_linux_host("krun")?;
        if !matches!(
            manifest.launch_authority,
            KrunLaunchAuthority::Adopted { .. }
        ) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun provision activation for {sandbox_id} requires adopted attachment authority, got {:?}",
                    manifest.launch_authority
                ),
            });
        }
        start::ensure_guest_user_helper_available(&self.config, &manifest)?;
        self.require_non_routable_activation_prerequisites(&manifest)?;
        let runtime_state = self.spawn_creator_and_wait_for_runtime(&mut manifest)?;
        if runtime_state != "running" {
            run_status_checked(&manifest.conmon_launch.start_command)?;
        }
        manifest.shutdown_requested = false;
        manifest.last_exit_code = None;
        manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;
        synchronize_handle_status(&mut manifest, SandboxStatus::Starting);
        self.persist_effect_barrier(&manifest, "krun provision activation result")?;
        Ok(SandboxProvisionPhaseObservation::Succeeded {
            evidence: phase_evidence("workload_activated", &manifest.handle)?,
        })
    }

    /// Inspect activation without starting, restarting, or repairing the VMM.
    pub fn inspect_provision_workload_activation(
        &self,
        sandbox_id: &SandboxId,
        expected_attempt_id: &crate::SandboxExecutionAttemptId,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: phase_evidence("workload_activation_absent", sandbox_id)?,
            });
        };
        manifest.require_execution_attempt(
            expected_attempt_id,
            "krun provision activation inspection",
        )?;
        match runtime_state(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
        ) {
            Ok(crate::backends::conmon::lifecycle::RuntimeStateObservation::Present(state)) => {
                let evidence = phase_evidence(
                    "workload_activation_runtime_state",
                    &(&manifest.handle, &state),
                )?;
                match classify_provision_activation(normalized_activation_runtime_state(&state)) {
                    ProvisionActivationObservationKind::Succeeded => {
                        Ok(SandboxProvisionPhaseObservation::Succeeded { evidence })
                    }
                    ProvisionActivationObservationKind::Absent => {
                        Ok(SandboxProvisionPhaseObservation::Absent { evidence })
                    }
                    ProvisionActivationObservationKind::InProgress => {
                        Ok(SandboxProvisionPhaseObservation::InProgress { evidence })
                    }
                    ProvisionActivationObservationKind::Ambiguous => {
                        Ok(SandboxProvisionPhaseObservation::Ambiguous { evidence })
                    }
                }
            }
            Ok(crate::backends::conmon::lifecycle::RuntimeStateObservation::ExplicitlyAbsent) => {
                Ok(SandboxProvisionPhaseObservation::Absent {
                    evidence: phase_evidence("workload_activation_absent", &manifest.handle)?,
                })
            }
            Err(error) => Ok(SandboxProvisionPhaseObservation::Ambiguous {
                evidence: phase_evidence("workload_activation_unknown", &error.to_string())?,
            }),
        }
    }

    /// Inspect application readiness through the assigned private address.
    pub fn inspect_provision_workload_readiness(
        &self,
        sandbox_id: &SandboxId,
        expected_attempt_id: &crate::SandboxExecutionAttemptId,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: phase_evidence("workload_readiness_absent", sandbox_id)?,
            });
        };
        manifest.require_execution_attempt(
            expected_attempt_id,
            "krun provision readiness inspection",
        )?;
        if !matches!(
            self.inspect_provision_workload_activation(sandbox_id, expected_attempt_id)?,
            SandboxProvisionPhaseObservation::Succeeded { .. }
        ) {
            return Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "workload_readiness_waiting_for_activation",
                    &manifest.handle,
                )?,
            });
        }
        let attachment = self.non_routable_attachment_readiness(&manifest)?;
        let OciAttachmentBaseReadinessState::Ready(attachment) = attachment else {
            return Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "workload_readiness_waiting_for_attachment",
                    &manifest.handle,
                )?,
            });
        };
        let Some(assigned_ip) = attachment.assigned_ips().first().copied() else {
            return Ok(SandboxProvisionPhaseObservation::Ambiguous {
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
                PublishedEndpoint::new(
                    binding.name.clone(),
                    binding.protocol,
                    std::net::SocketAddr::new(assigned_ip.into(), binding.guest_port),
                )
            })
            .collect::<Vec<_>>();
        let application = inspect_application_readiness(
            manifest.status,
            &private_endpoints,
            readiness::readiness_probe_timeout(&manifest),
            self.readiness_probe_provider.as_ref(),
        );
        let evidence = phase_evidence(
            "workload_readiness_inspection",
            &(manifest.handle, application.clone()),
        )?;
        if application.status() == SandboxStatus::Ready {
            Ok(SandboxProvisionPhaseObservation::Succeeded { evidence })
        } else {
            Ok(SandboxProvisionPhaseObservation::InProgress { evidence })
        }
    }

    /// Inspect private routes for the server-owned deferred ingress adapter.
    ///
    /// No listener, proxy, Netavark, or VMM effect occurs here. The private IP
    /// is route data authenticated by stable plan, attachment, listener, lease,
    /// tenant, and generation identities.
    pub fn inspect_provision_server_ingress_targets(
        &self,
        sandbox_id: &SandboxId,
        expected_attempt_id: &crate::SandboxExecutionAttemptId,
        network_plan: &SandboxProvisionNetworkPlan,
    ) -> Result<crate::SandboxProvisionIngressTargetObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(crate::SandboxProvisionIngressTargetObservation::Absent {
                evidence: phase_evidence("server_ingress_target_absent", sandbox_id)?,
            });
        };
        manifest.require_execution_attempt(
            expected_attempt_id,
            "krun provision server-ingress target inspection",
        )?;
        let config = manifest.require_network_config()?;
        let Some(durable_plan) = config.network_plan.as_ref() else {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun sandbox {sandbox_id} lacks its durable compiled network plan"
                ),
            });
        };
        let readiness = self.non_routable_attachment_readiness(&manifest)?;
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
                    "krun sandbox {sandbox_id} has ready attachment evidence without a private address"
                ),
            });
        };
        let reservation_claim = manifest.require_reserved_claim()?.clone();
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
                "krun sandbox {sandbox_id} rejected crossed server-ingress targets: {error}"
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

    fn non_routable_attachment_readiness(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<OciAttachmentBaseReadinessState> {
        let network_config = manifest.require_network_config()?;
        let ports = self.port_lease_coordinator();
        let hostname = start::hostname_for(&manifest.spec);
        Ok(self
            .non_routable_attachment_adapter(manifest, network_config, &hostname)
            .inspect_non_routable_readiness(
                &self.attachment_lifecycle(&ports),
                self.egress_pin_provider.as_ref(),
                manifest.egress_proxy.as_ref(),
                self.authenticated_egress_readiness(manifest)?,
            ))
    }

    fn require_non_routable_activation_prerequisites(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<()> {
        ensure_linux_host("krun")?;
        match self.non_routable_attachment_readiness(manifest)? {
            OciAttachmentBaseReadinessState::Ready(_) => Ok(()),
            OciAttachmentBaseReadinessState::NotReady(reason) => {
                Err(SandboxError::OperationFailed {
                    message: format!(
                        "krun sandbox {} denied activation: private attachment is not ready: {reason:?}",
                        manifest.handle.id
                    ),
                })
            }
        }
    }
}
