use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod artifact_cleanup;
mod attachment_readiness;
mod attachment_teardown;
mod config;
mod creator;
mod direct_execution;
mod effect_fence;
mod egress_reload;
mod execution_cleanup;
mod inspection;
mod launch;
mod machine_port_publication;
pub use machine_port_publication::MachinePortAbsenceEvidence;
mod machine_ports;
mod manifest;
mod network_composition;
mod network_launch;
mod provider_context;
mod provision;
mod restart;
mod runner;
mod status;
mod teardown;
#[cfg(any(test, feature = "test-hooks"))]
mod test_hooks;
#[cfg(any(test, feature = "test-hooks"))]
pub(in crate::backends) use test_hooks::{
    prepare_network_teardown_fixture, reopen_network_teardown_fixture,
};

use super::bundle::{
    ContainerBundleLayout, ContainerBundleMount, ContainerBundleOptions, write_bundle_config,
};
use crate::backend::{SandboxBackend, SandboxBackendKind, SandboxFuture};
use crate::backends::capabilities::{
    SandboxAttachmentRegistrationError, SandboxAttachmentRegistrationKind,
    host_managed_attachment_registration,
};
#[cfg(test)]
use crate::backends::conmon::lifecycle::RestartLaunchTestProbe;
#[cfg(test)]
use crate::backends::conmon::lifecycle::{
    RuntimeStatusProbe, detect_runtime_status as detect_conmon_runtime_status,
};
use crate::backends::conmon::lifecycle::{
    configured_stop_signal, configured_stop_timeout, ensure_linux_host, read_exit_code, read_pid,
    run_status_checked, signal_process, wait_for_path,
};
use crate::backends::oci::buildah::OciImageLaunchDefaults;
use crate::backends::oci::builder::OciDockerfileBuilder;
use crate::backends::oci::conmon::{OciConmonConfig, OciConmonLayout, build_launch_plan};
#[cfg(test)]
use crate::backends::oci::egress::egress_trust_anchor_root;
use crate::backends::oci::egress::{
    EgressProxyAssignment, EgressProxyRegistry, EgressReadinessState,
    PepPreAdoptionReleaseAuthority, egress_listener_reservation, egress_proxy_assignment,
    egress_trust_anchor_mount, ensure_egress_proxy_running as ensure_oci_egress_proxy_running,
    ensure_egress_proxy_running_with_release_authority,
};
use crate::backends::oci::materializer::{OciImageMaterializer, PreparedMaterializedImageLaunch};
#[cfg(test)]
use crate::backends::oci::network::HostManagedAttachmentCheckpointTestProbe;
use crate::backends::oci::network::{
    AttachmentAttachAuthority, MachinePortPreparationReleaseAuthority,
    MachinePortProxyLifetimeRegistry, OciEgressPinProvider, OciIpamAuthority, OciNetworkLayout,
    OciNetworkProcess, OciSegmentAllocator, default_network_attachment_id,
};
#[cfg(test)]
use crate::backends::oci::network::{
    MachinePortProxyEntry, MachinePortProxyRegistration, OciNetavarkOperation,
    authenticate_container_network_generation_for_cleanup, setup_container_network,
};
use crate::backends::oci::port_lease::new_launch_reservation_claim;
use crate::backends::oci::port_lifecycle::{
    NetavarkPortLifetimeRegistry, OciPortLeaseCoordinator, ReservedLaunchPorts,
    SandboxLaunchPortPlan,
};
use crate::backends::oci::resource_quota::ResourceQuotaManager;
use crate::backends::readiness_probe::ReadinessProbeProvider;
use crate::error::{Result, SandboxError};
use crate::execution_attempt::{SandboxExecutionAttemptId, SandboxRestartAttemptFence};
use crate::instance::{SandboxHandle, SandboxId, SandboxStatus};
use crate::provision::SandboxProvisionNetworkPlan;
use crate::spec::{SandboxOciImageSource, SandboxRootSpec, SandboxSpec};
use nimbus_egress::EgressPolicy;

pub use config::{ContainerSandboxBackendConfig, ContainerStartMode};
use launch::{hostname_for, next_sandbox_id, resolve_start_spec};
use manifest::{
    ContainerCreatorHandoffState, ContainerLaunchArtifact, ContainerLifecycleCoordinator,
    ContainerNetworkPublicationMode, ContainerRunnerExecutionConfig, ContainerSandboxManifest,
    ContainerStartPlan,
};
#[cfg(test)]
use provision::ProvisionAdmissionTestProbe;
use runner::RUNNER_MANIFEST_POINTER_FILE;
#[cfg(test)]
use runner::RunnerLifecycleLockTestProbe;
pub use runner::run_prepared_container_service_workload;
#[cfg(test)]
use status::running_status;
use status::{synchronize_handle_status, visible_published_endpoints};
pub use teardown::{CONTAINER_EXECUTION_TEARDOWN_PROVIDER_KEY, ContainerHostTerminalEvidence};

#[derive(Clone)]
pub struct ContainerSandboxBackend {
    config: ContainerSandboxBackendConfig,
    segment_allocator: Arc<OciSegmentAllocator>,
    attachment_authority: Option<nimbus_network::LocalNetworkAttachmentAuthority>,
    ipam_authority: OciIpamAuthority,
    port_lease_coordinator: OciPortLeaseCoordinator,
    egress_proxies: EgressProxyRegistry,
    egress_pin_provider: Arc<dyn OciEgressPinProvider>,
    readiness_probe_provider: Arc<dyn ReadinessProbeProvider>,
    teardown_runtime_provider: Arc<dyn teardown::effects::ContainerExecutionTeardownRuntime>,
    netavark_port_lifetimes: NetavarkPortLifetimeRegistry,
    machine_port_proxies: MachinePortProxyLifetimeRegistry,
    _network_process: Option<Arc<OciNetworkProcess>>,
    startup_reconciliation_error: Option<Arc<str>>,
    #[cfg(test)]
    restart_launch_test_probe: Option<RestartLaunchTestProbe>,
    #[cfg(test)]
    runner_handoff_failure: Option<RunnerHandoffFailure>,
    #[cfg(test)]
    runner_lifecycle_lock_test_probe: Option<RunnerLifecycleLockTestProbe>,
    #[cfg(test)]
    provision_admission_test_probe: Option<ProvisionAdmissionTestProbe>,
    #[cfg(test)]
    post_egress_reload_ack_observer: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    network_teardown_checkpoint_test_probe: Option<HostManagedAttachmentCheckpointTestProbe>,
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum RunnerHandoffFailure {
    Manifest,
    Pointer,
    PointerAfterExecuteDecision,
    DirectEffectFencePersistence,
    DirectEffectFenceAcknowledgementLoss,
    DirectAfterEffectFence,
    DirectTerminalManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedContainerServiceWorkload {
    pub handle: SandboxHandle,
    pub bundle_dir: PathBuf,
}

struct ContainerStartPlanningOptions<'a> {
    execution_attempt_id: SandboxExecutionAttemptId,
    launch_defaults: Option<&'a OciImageLaunchDefaults>,
    launch_artifact: Option<ContainerLaunchArtifact>,
    provision_network_plan: Option<&'a SandboxProvisionNetworkPlan>,
    reserve_execute_network: bool,
    prepare_bundle: bool,
}

fn combine_launch_failure(
    primary: SandboxError,
    cleanup: Option<SandboxError>,
    persistence: Option<SandboxError>,
) -> SandboxError {
    match (cleanup, persistence) {
        (None, None) => primary,
        (Some(cleanup), None) => SandboxError::OperationFailed {
            message: format!("container launch failed: {primary}; cleanup also failed: {cleanup}"),
        },
        (None, Some(persistence)) => SandboxError::OperationFailed {
            message: format!(
                "container launch failed: {primary}; compensated-manifest persistence also failed: \
                 {persistence}"
            ),
        },
        (Some(cleanup), Some(persistence)) => SandboxError::OperationFailed {
            message: format!(
                "container launch failed: {primary}; cleanup also failed: {cleanup}; \
                 compensated-manifest persistence also failed: {persistence}"
            ),
        },
    }
}

impl ContainerSandboxBackend {
    /// Report conservative host-managed attachment evidence for this composition.
    ///
    /// This refuses configurations that cannot own the exact local Execute
    /// composition and performs no provider effects or runtime readiness probes.
    pub fn host_managed_attachment_registration(
        &self,
    ) -> std::result::Result<
        nimbus_network::NetworkAttachmentProviderRegistration,
        SandboxAttachmentRegistrationError,
    > {
        host_managed_attachment_registration(
            SandboxAttachmentRegistrationKind::Container,
            self.config.start_mode == ContainerStartMode::Execute,
            self.config.machine_port_forwarder.is_some(),
            self.startup_reconciliation_error.as_ref(),
        )
    }

    fn ensure_startup_reconciliation_ready(&self) -> Result<()> {
        if let Some(error) = self.startup_reconciliation_error.as_ref() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container backend refuses new durable work because startup reconciliation \
                     did not complete: {error}"
                ),
            });
        }
        Ok(())
    }

    #[cfg(test)]
    fn with_runner_handoff_failure(mut self, failure: RunnerHandoffFailure) -> Self {
        self.runner_handoff_failure = Some(failure);
        self
    }

    #[cfg(test)]
    fn with_restart_launch_test_probe(mut self, probe: RestartLaunchTestProbe) -> Self {
        self.restart_launch_test_probe = Some(probe);
        self
    }

    #[cfg(test)]
    fn with_runner_lifecycle_lock_test_probe(
        mut self,
        probe: RunnerLifecycleLockTestProbe,
    ) -> Self {
        self.runner_lifecycle_lock_test_probe = Some(probe);
        self
    }

    #[cfg(test)]
    fn with_provision_admission_test_probe(mut self, probe: ProvisionAdmissionTestProbe) -> Self {
        self.provision_admission_test_probe = Some(probe);
        self
    }

    #[cfg(test)]
    fn pause_after_provision_admission_for_test(&self) -> Result<()> {
        if let Some(probe) = self.provision_admission_test_probe.as_ref() {
            probe.pause()?;
        }
        Ok(())
    }

    #[cfg(not(test))]
    fn pause_after_provision_admission_for_test(&self) -> Result<()> {
        Ok(())
    }

    #[cfg(test)]
    fn with_network_teardown_checkpoint_test_probe(
        mut self,
        probe: HostManagedAttachmentCheckpointTestProbe,
    ) -> Self {
        self.network_teardown_checkpoint_test_probe = Some(probe);
        self
    }

    fn remove_tenant_artifacts_sync(&self, tenant_id: &nimbus_core::TenantId) -> Result<()> {
        for root in [&self.config.bundle_root, &self.config.workload_state_root] {
            crate::artifact_paths::remove_tenant_root(root, tenant_id).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to remove container sandbox tenant artifacts for {} under {}: {error}",
                        tenant_id,
                        root.display()
                    ),
                }
            })?;
        }
        Ok(())
    }

    fn resource_quota_manager(&self) -> ResourceQuotaManager {
        ResourceQuotaManager::new(
            self.config.workload_state_root.clone(),
            self.config.resource_quota_policy.clone(),
        )
    }

    pub fn prepare_plan_only_service_workload(
        &self,
        spec: SandboxSpec,
    ) -> Result<PreparedContainerServiceWorkload> {
        self.prepare_plan_only_service_workload_inner(spec, None)
    }

    /// Materialize one service-owned PlanOnly workload under a caller-selected
    /// sandbox incarnation.
    ///
    /// The caller allocates the stable incarnation before crossing a process or
    /// provider boundary. This method preserves that exact identity through the
    /// canonical manifest, bundle, network attachment, listener leases, and
    /// runner handoff. It never replaces a currently owned durable manifest.
    pub fn prepare_plan_only_service_workload_with_id(
        &self,
        spec: SandboxSpec,
        sandbox_id: SandboxId,
    ) -> Result<PreparedContainerServiceWorkload> {
        self.prepare_plan_only_service_workload_inner(spec, Some(sandbox_id))
    }

    fn prepare_plan_only_service_workload_inner(
        &self,
        spec: SandboxSpec,
        sandbox_id: Option<SandboxId>,
    ) -> Result<PreparedContainerServiceWorkload> {
        if self.config.start_mode != ContainerStartMode::PlanOnly {
            return Err(SandboxError::InvalidSpec {
                message: "container service workload materialization requires plan-only mode"
                    .to_owned(),
            });
        }
        if spec.service_name().is_none() {
            return Err(SandboxError::InvalidSpec {
                message:
                    "container service workload materialization requires service owner metadata"
                        .to_owned(),
            });
        }
        let sandbox_id = sandbox_id.unwrap_or_else(|| next_sandbox_id(spec.display_name()));
        if sandbox_id.as_str().is_empty() {
            return Err(SandboxError::InvalidSpec {
                message: "container service workload identity cannot be empty".to_owned(),
            });
        }
        if self.read_manifest(&sandbox_id)?.is_some() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container service workload {} already has a durable manifest; refusing to \
                     replace its current owner",
                    sandbox_id
                ),
            });
        }
        let mut launch_plan = self.plan_start_for_id(&spec, &sandbox_id)?;
        launch_plan.manifest.assign_prepared_service_runner()?;
        if let Err(error) = self.attach_runner_owned_egress_proxy(&mut launch_plan) {
            return Err(self.compensate_prepared_runner_failure(&mut launch_plan.manifest, error));
        }
        let bundle_dir = launch_plan.manifest.bundle_layout.bundle_dir.clone();
        // The manifest is the durable handoff barrier. Publish the pointer only
        // after the exact claim-bearing plan can be reopened by the runner.
        #[cfg(test)]
        let manifest_write = if self
            .runner_handoff_failure
            .is_some_and(|failure| matches!(failure, RunnerHandoffFailure::Manifest))
        {
            Err(SandboxError::OperationFailed {
                message: "injected runner manifest handoff failure".to_owned(),
            })
        } else {
            self.write_manifest(&launch_plan.manifest)
        };
        #[cfg(not(test))]
        let manifest_write = self.write_manifest(&launch_plan.manifest);
        if let Err(error) = manifest_write {
            return Err(self.compensate_prepared_runner_failure(&mut launch_plan.manifest, error));
        }
        #[cfg(test)]
        let pointer_write = match self.runner_handoff_failure {
            Some(RunnerHandoffFailure::Pointer) => Err(SandboxError::OperationFailed {
                message: "injected runner pointer handoff failure".to_owned(),
            }),
            Some(RunnerHandoffFailure::PointerAfterExecuteDecision) => {
                self.write_runner_manifest_pointer(&launch_plan.manifest)?;
                runner::claim_runner_execution_for_test(&launch_plan.manifest)?;
                Err(SandboxError::OperationFailed {
                    message: "injected runner pointer acknowledgement failure after Execute"
                        .to_owned(),
                })
            }
            Some(
                RunnerHandoffFailure::Manifest
                | RunnerHandoffFailure::DirectEffectFencePersistence
                | RunnerHandoffFailure::DirectEffectFenceAcknowledgementLoss
                | RunnerHandoffFailure::DirectAfterEffectFence
                | RunnerHandoffFailure::DirectTerminalManifest,
            )
            | None => self.write_runner_manifest_pointer(&launch_plan.manifest),
        };
        #[cfg(not(test))]
        let pointer_write = self.write_runner_manifest_pointer(&launch_plan.manifest);
        if let Err(error) = pointer_write {
            let _handoff = match runner::lock_plan_only_status_update(&launch_plan.manifest, true) {
                Ok(handoff) => handoff,
                Err(fence) => {
                    return Err(combine_launch_failure(error, Some(fence), None));
                }
            };
            return Err(self.compensate_prepared_runner_failure(&mut launch_plan.manifest, error));
        }
        let handle = launch_plan.manifest.handle.clone();
        Ok(PreparedContainerServiceWorkload { handle, bundle_dir })
    }

    fn compensate_prepared_runner_failure(
        &self,
        manifest: &mut ContainerSandboxManifest,
        primary: SandboxError,
    ) -> SandboxError {
        let cleanup = self.release_plan_only_execution_artifacts(manifest).err();
        manifest.shutdown_requested = true;
        if cleanup.is_none() {
            manifest.last_exit_code = Some(0);
        }
        synchronize_handle_status(
            manifest,
            if cleanup.is_none() {
                SandboxStatus::Stopped
            } else {
                SandboxStatus::Stopping
            },
        );
        let persistence = self.write_manifest(manifest).err();
        combine_launch_failure(primary, cleanup, persistence)
    }

    pub fn mark_plan_only_service_workload_stopped(
        &self,
        id: &SandboxId,
    ) -> Result<Option<SandboxHandle>> {
        self.update_plan_only_service_workload_status(id, SandboxStatus::Stopped)
    }

    pub fn refresh_plan_only_service_workload_status(
        &self,
        id: &SandboxId,
        status: SandboxStatus,
    ) -> Result<Option<SandboxHandle>> {
        self.update_plan_only_service_workload_status(id, status)
    }

    fn update_plan_only_service_workload_status(
        &self,
        id: &SandboxId,
        status: SandboxStatus,
    ) -> Result<Option<SandboxHandle>> {
        if self.config.start_mode != ContainerStartMode::PlanOnly {
            return Err(SandboxError::OperationFailed {
                message: "container service workload status refresh requires plan-only mode"
                    .to_owned(),
            });
        }
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Ok(None);
        };
        if manifest.lifecycle_coordinator != ContainerLifecycleCoordinator::PreparedServiceRunner {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container service workload status refresh requires the \
                     PreparedServiceRunner lifecycle coordinator; workload {id} is owned by {:?}",
                    manifest.lifecycle_coordinator
                ),
            });
        }
        let _execute_handoff = if manifest.start_mode == ContainerStartMode::Execute {
            let (handoff, current) =
                runner::lock_current_execute_lifecycle_for_backend(self, &manifest)?;
            manifest = current;
            Some(handoff)
        } else {
            None
        };
        if manifest.start_mode == ContainerStartMode::Execute
            && let Some(phase) = runner::execute_handoff_phase(&manifest)?
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container workload {id} is owned by runner handoff phase {phase:?}; \
                     external status mutation remains fenced"
                ),
            });
        }
        let terminal_callback = matches!(status, SandboxStatus::Stopped | SandboxStatus::Failed);
        let terminal_finality = manifest.has_terminal_network_finality();
        if terminal_finality {
            self.reconcile_terminal_ipam_retirement(&manifest)?;
            return Ok(Some(manifest.handle));
        }
        if manifest.shutdown_requested && !terminal_callback {
            return Ok(Some(manifest.handle));
        }
        let _handoff = (manifest.start_mode == ContainerStartMode::PlanOnly)
            .then(|| runner::lock_plan_only_status_update(&manifest, terminal_callback))
            .transpose()?;
        let status = if terminal_callback {
            let prior_failure = manifest.status == SandboxStatus::Failed
                || manifest.last_exit_code.is_some_and(|exit| exit != 0);
            let terminal_status = if status == SandboxStatus::Failed || prior_failure {
                SandboxStatus::Failed
            } else {
                SandboxStatus::Stopped
            };
            manifest.shutdown_requested = true;
            manifest.last_exit_code = match terminal_status {
                SandboxStatus::Stopped => Some(0),
                SandboxStatus::Failed => manifest.last_exit_code.filter(|exit| *exit != 0),
                _ => unreachable!("terminal callback status is normalized above"),
            };
            match manifest.start_mode {
                ContainerStartMode::PlanOnly => {
                    self.release_plan_only_execution_artifacts(&mut manifest)?;
                }
                ContainerStartMode::Execute => {
                    self.release_execution_artifacts(&mut manifest)?;
                }
            }
            terminal_status
        } else {
            status
        };
        synchronize_handle_status(&mut manifest, status);
        self.write_existing_workload_manifest(&manifest)?;
        Ok(Some(manifest.handle))
    }

    fn attach_runner_owned_egress_proxy(
        &self,
        launch_plan: &mut ContainerStartPlan,
    ) -> Result<ReservedLaunchPorts> {
        if launch_plan.manifest.egress_proxy.is_some() {
            return Err(SandboxError::OperationFailed {
                message: "runner launch already owns an egress proxy reservation".to_owned(),
            });
        }
        let manager = self.port_lease_coordinator();
        let reallocatable = manager.validate_plan_binding_provenance(
            &launch_plan.manifest.requested_port_bindings,
            &launch_plan.manifest.spec.port_bindings,
            &launch_plan.manifest.image_metadata.exposed_ports,
        )?;
        let reservation_claim = self.begin_launch_reservation(&mut launch_plan.manifest)?;
        let network_config = self.place_sandbox_config(
            &launch_plan.manifest.spec.tenant_id,
            &launch_plan.manifest.network_layout,
            &launch_plan.manifest.handle.id,
            &default_network_attachment_id(&launch_plan.manifest.handle.id),
            &reservation_claim,
        )?;
        launch_plan.manifest.network_config = Some(network_config.clone());
        let internal_listener = egress_listener_reservation(&network_config)?;
        let mut reservations = manager.reserve_launch_ports_for_sandbox(
            SandboxLaunchPortPlan::new(
                &launch_plan.manifest.spec.tenant_id,
                &launch_plan.manifest.handle.id,
                &launch_plan.manifest.spec.port_bindings,
                &[],
            )
            .with_reallocatable_listener_names(&reallocatable)
            .with_internal_listener(internal_listener),
            &reservation_claim,
        )?;
        let update_result = (|| {
            let internal = reservations.internal_listener.clone().ok_or_else(|| {
                SandboxError::OperationFailed {
                    message: "runner launch reservation omitted the required egress listener"
                        .to_owned(),
                }
            })?;
            let egress_proxy = egress_proxy_assignment(&network_config, internal)?;
            launch_plan.manifest.spec.port_bindings = reservations.published_bindings.clone();
            launch_plan.manifest.port_leases = reservations.published_leases.clone();
            launch_plan.manifest.launch_reservation_claim =
                Some(reservations.reservation_claim.clone());
            launch_plan.manifest.egress_proxy = Some(egress_proxy.clone());
            let status = launch_plan.manifest.status;
            synchronize_handle_status(&mut launch_plan.manifest, status);
            write_bundle_config(
                &launch_plan.manifest.bundle_layout,
                &hostname_for(&launch_plan.manifest.spec),
                &launch_plan.manifest.spec,
                launch_plan.manifest.image_metadata.user.as_deref(),
                Some(launch_plan.manifest.network_layout.netns_path.as_path()),
                &container_bundle_options(
                    &self.config.workload_state_root,
                    &launch_plan.manifest.spec,
                    &launch_plan.manifest.handle.id,
                    Some(&egress_proxy),
                )?,
            )?;
            self.write_manifest(&launch_plan.manifest)
        })();
        update_result?;
        reservations.confirm_manifest_published()?;
        Ok(reservations)
    }

    fn begin_launch_reservation(
        &self,
        manifest: &mut ContainerSandboxManifest,
    ) -> Result<nimbus_network::NetworkReservationClaim> {
        if manifest.launch_reservation_claim.is_some() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container launch {} already carries a reservation coordinator",
                    manifest.handle.id
                ),
            });
        }
        let reservation_claim = new_launch_reservation_claim()?;
        manifest.launch_reservation_claim = Some(reservation_claim.clone());
        self.write_manifest(manifest)?;
        Ok(reservation_claim)
    }

    fn write_runner_manifest_pointer(&self, manifest: &ContainerSandboxManifest) -> Result<()> {
        let pointer_path = manifest
            .bundle_layout
            .bundle_dir
            .join(RUNNER_MANIFEST_POINTER_FILE);
        let expected = format!("{}\n", manifest.conmon_layout.manifest_path.display());
        match std::fs::symlink_metadata(&pointer_path) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container runner manifest pointer {} is not a regular file",
                        pointer_path.display()
                    ),
                });
            }
            Ok(_) => {
                let current = std::fs::read_to_string(&pointer_path).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "failed to read container runner manifest pointer {}: {error}",
                            pointer_path.display()
                        ),
                    }
                })?;
                if current == expected {
                    return Ok(());
                }
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container runner manifest pointer {} names a different durable manifest; replacement remains fenced",
                        pointer_path.display()
                    ),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to inspect container runner manifest pointer {}: {error}",
                        pointer_path.display()
                    ),
                });
            }
        }
        let mut pointer = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pointer_path)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create container runner manifest pointer {}: {error}",
                    pointer_path.display()
                ),
            })?;
        pointer
            .write_all(expected.as_bytes())
            .and_then(|()| pointer.sync_all())
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to durably write container runner manifest pointer {}: {error}",
                    pointer_path.display()
                ),
            })?;
        std::fs::File::open(&manifest.bundle_layout.bundle_dir)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "runner manifest pointer {} reached its commit point but the bundle-directory sync failed: {error}",
                    pointer_path.display()
                ),
            })
    }

    fn stop_sync(&self, id: &SandboxId) -> Result<()> {
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: id.as_str().to_owned(),
            });
        };
        let execute_handoff = if manifest.start_mode == ContainerStartMode::Execute {
            let (handoff, current) =
                runner::lock_current_execute_lifecycle_for_backend(self, &manifest)?;
            manifest = current;
            Some(handoff)
        } else {
            None
        };
        if manifest.start_mode == ContainerStartMode::Execute {
            if let Some(phase) = runner::execute_handoff_phase(&manifest)? {
                if phase == runner::RunnerHandoffPhase::ClaimedBeforeEffects {
                    self.reconcile_pending_creator_before_cleanup(&mut manifest)?;
                    let cleanup_complete = manifest.status == SandboxStatus::Stopped
                        && manifest.has_terminal_network_finality();
                    if !cleanup_complete {
                        self.release_unstarted_launch_artifacts(&mut manifest)?;
                        manifest.shutdown_requested = true;
                        manifest.last_exit_code = Some(0);
                        synchronize_handle_status(&mut manifest, SandboxStatus::Stopped);
                        self.write_existing_workload_manifest(&manifest)?;
                    }
                    runner::publish_runner_lifecycle_ownership(
                        &manifest,
                        execute_handoff
                            .as_ref()
                            .expect("execute manifest must own its lifecycle lock"),
                    )?;
                    return Ok(());
                }
                if phase == runner::RunnerHandoffPhase::EffectsStarted {
                    let outcome = runner::reconcile_runner_effects_started(
                        self,
                        &mut manifest,
                        execute_handoff
                            .as_ref()
                            .expect("execute manifest must own its lifecycle lock"),
                    )?;
                    if outcome == runner::RunnerEffectOutcome::Absent {
                        return Ok(());
                    }
                } else {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "container workload {id} is owned by runner handoff phase {phase:?}; \
                             external teardown remains fenced"
                        ),
                    });
                }
            }
            self.reconcile_pending_creator_before_cleanup(&mut manifest)?;
            if manifest.has_terminal_network_finality() {
                self.reconcile_terminal_ipam_retirement(&manifest)?;
                return Ok(());
            }
            if manifest.launch_reservation_claim.is_some() {
                self.release_unstarted_launch_artifacts(&mut manifest)?;
                manifest.shutdown_requested = true;
                manifest.last_exit_code = Some(0);
                synchronize_handle_status(&mut manifest, SandboxStatus::Stopped);
                return self.write_existing_workload_manifest(&manifest);
            }
        } else if manifest.has_terminal_network_finality() {
            self.reconcile_terminal_ipam_retirement(&manifest)?;
            return Ok(());
        }

        match manifest.start_mode {
            ContainerStartMode::PlanOnly => {
                if manifest.has_terminal_network_finality() {
                    self.reconcile_terminal_ipam_retirement(&manifest)?;
                    return Ok(());
                }
                let _handoff = (manifest.lifecycle_coordinator
                    == ContainerLifecycleCoordinator::PreparedServiceRunner)
                    .then(|| runner::lock_plan_only_status_update(&manifest, true))
                    .transpose()?;
                manifest.shutdown_requested = true;
                manifest.last_exit_code = Some(0);
                synchronize_handle_status(&mut manifest, SandboxStatus::Stopped);
                self.release_plan_only_execution_artifacts(&mut manifest)?;
                self.write_existing_workload_manifest(&manifest)
            }
            ContainerStartMode::Execute => self.execute_stop(&mut manifest),
        }
    }

    #[cfg(test)]
    pub(crate) fn plan_start(&self, spec: &SandboxSpec) -> Result<ContainerStartPlan> {
        let sandbox_id = next_sandbox_id(spec.display_name());
        self.plan_start_for_id(spec, &sandbox_id)
    }

    fn plan_start_for_id(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
    ) -> Result<ContainerStartPlan> {
        self.plan_start_for_id_with_network_reservation(spec, sandbox_id, true, true)
    }

    fn plan_start_for_id_with_network_reservation(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        reserve_execute_network: bool,
        prepare_bundle: bool,
    ) -> Result<ContainerStartPlan> {
        self.ensure_startup_reconciliation_ready()?;
        match &spec.root {
            SandboxRootSpec::Rootfs(_) => self.plan_start_with_id_with_network_reservation(
                spec,
                sandbox_id,
                ContainerStartPlanningOptions {
                    execution_attempt_id: SandboxExecutionAttemptId::provider_initial(),
                    launch_defaults: None,
                    launch_artifact: None,
                    provision_network_plan: None,
                    reserve_execute_network,
                    prepare_bundle,
                },
            ),
            SandboxRootSpec::OciImage(image) => {
                self.resource_quota_manager().ensure_launch_quota(spec)?;
                let prepared_launch =
                    self.prepare_oci_image_start(spec, sandbox_id, &image.source)?;
                self.plan_start_with_materialized_image(
                    spec,
                    sandbox_id,
                    prepared_launch,
                    reserve_execute_network,
                    prepare_bundle,
                )
            }
        }
    }

    fn plan_start_with_materialized_image(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        prepared_launch: PreparedMaterializedImageLaunch,
        reserve_execute_network: bool,
        prepare_bundle: bool,
    ) -> Result<ContainerStartPlan> {
        self.plan_start_with_id_with_network_reservation(
            spec,
            sandbox_id,
            ContainerStartPlanningOptions {
                execution_attempt_id: SandboxExecutionAttemptId::provider_initial(),
                launch_defaults: Some(&prepared_launch.launch_defaults),
                launch_artifact: Some(ContainerLaunchArtifact::Rootfs(prepared_launch.artifact)),
                provision_network_plan: None,
                reserve_execute_network,
                prepare_bundle,
            },
        )
    }

    #[cfg(test)]
    fn plan_start_with_id(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        launch_defaults: Option<&OciImageLaunchDefaults>,
        launch_artifact: Option<ContainerLaunchArtifact>,
    ) -> Result<ContainerStartPlan> {
        self.plan_start_with_id_with_network_reservation(
            spec,
            sandbox_id,
            ContainerStartPlanningOptions {
                execution_attempt_id: SandboxExecutionAttemptId::provider_initial(),
                launch_defaults,
                launch_artifact,
                provision_network_plan: None,
                reserve_execute_network: true,
                prepare_bundle: true,
            },
        )
    }

    fn plan_start_with_id_with_network_reservation(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        options: ContainerStartPlanningOptions<'_>,
    ) -> Result<ContainerStartPlan> {
        let ContainerStartPlanningOptions {
            execution_attempt_id,
            launch_defaults,
            launch_artifact,
            provision_network_plan,
            reserve_execute_network,
            prepare_bundle,
        } = options;
        self.ensure_startup_reconciliation_ready()?;
        if spec.backend != SandboxBackendKind::Container {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "container backend cannot lower sandbox spec for backend {:?}",
                    spec.backend
                ),
            });
        }

        let resolved_launch = resolve_start_spec(spec, launch_defaults)?;
        let mut resolved_spec = resolved_launch.spec.clone();
        self.resource_quota_manager()
            .ensure_launch_quota(&resolved_spec)?;
        let manager = self.port_lease_coordinator();
        let requested_port_bindings = resolved_spec.port_bindings.clone();
        if self.config.start_mode == ContainerStartMode::PlanOnly
            && provision_network_plan.is_none()
        {
            // Preview is a pure admission/rendering step. Run it before
            // resolving the tenant segment so a rejected plan cannot create a
            // durable allocation for a workload that acquired no attachment.
            let auto_bindings = manager.preview_bindings_for_sandbox(
                &resolved_spec.tenant_id,
                &resolved_spec.port_bindings,
                &resolved_launch.image_metadata.exposed_ports,
            )?;
            resolved_spec.port_bindings.extend(auto_bindings);
        }
        let network_layout = OciNetworkLayout::with_roots(
            &self.config.workload_state_root,
            &self.config.network_state_root,
            &resolved_spec.tenant_id,
            sandbox_id,
        );
        network_layout.ensure_directories()?;
        let bundle_layout = ContainerBundleLayout::new(crate::artifact_paths::bundle_dir(
            &self.config.bundle_root,
            &resolved_launch.spec.tenant_id,
            sandbox_id,
        ));
        let conmon_layout = OciConmonLayout::new_for_tenant(
            &self.config.workload_state_root,
            &resolved_launch.spec.tenant_id,
            sandbox_id,
        );
        conmon_layout
            .ensure_directories()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create container state directories under {}: {error}",
                    self.config.workload_state_root.display()
                ),
            })?;
        let conmon_launch = build_launch_plan(
            &OciConmonConfig {
                conmon_path: self.config.conmon_path.clone(),
                runtime_path: self.config.runtime_path.clone(),
                buildah_path: self.config.buildah_path.clone(),
                use_buildah_unshare: launch_artifact
                    .as_ref()
                    .is_some_and(ContainerLaunchArtifact::uses_mount_session_unshare)
                    && self.config.use_buildah_unshare,
                log_level: self.config.log_level.clone(),
                log_size_max_bytes: resolved_spec.resources.log_limit_bytes,
            },
            &conmon_layout,
            sandbox_id,
            resolved_launch.spec.display_name(),
            &bundle_layout.bundle_dir,
            launch_artifact
                .as_ref()
                .and_then(ContainerLaunchArtifact::mount_session_name),
            &[],
        );
        let handle = SandboxHandle::new(
            resolved_spec.tenant_id.clone(),
            sandbox_id.clone(),
            resolved_spec.display_name().to_owned(),
            SandboxBackendKind::Container,
            SandboxStatus::Starting,
            visible_published_endpoints(
                self.config.start_mode,
                &resolved_spec,
                SandboxStatus::Starting,
            ),
        );
        let mut plan = ContainerStartPlan {
            manifest: ContainerSandboxManifest {
                handle,
                execution_attempt_id,
                spec: resolved_spec,
                provision_prepared: prepare_bundle,
                image_metadata: resolved_launch.image_metadata,
                launch_artifact,
                bundle_layout,
                conmon_layout,
                network_layout,
                provision_network_plan: provision_network_plan.cloned(),
                network_config: None,
                network_cleanup_complete: false,
                creator_handoff: ContainerCreatorHandoffState::NotSpawned,
                restart_transition: None,
                runner_handoff_id: None,
                requested_port_bindings,
                port_leases: Vec::new(),
                launch_reservation_claim: None,
                egress_proxy: None,
                egress_policy_reload:
                    crate::backends::oci::egress::EgressPolicyReloadState::initial(),
                conmon_launch,
                runner_config: ContainerRunnerExecutionConfig::from_backend_config(&self.config),
                last_exit_code: None,
                lifecycle_coordinator: ContainerLifecycleCoordinator::DirectBackend,
                start_mode: self.config.start_mode,
                shutdown_requested: false,
                execution_teardown: Default::default(),
                network_teardown: Default::default(),
                status: SandboxStatus::Starting,
            },
        };

        if reserve_execute_network
            && (self.config.start_mode == ContainerStartMode::Execute
                || provision_network_plan.is_some())
        {
            self.reserve_execute_launch_network(&mut plan.manifest, provision_network_plan)?;
        } else if prepare_bundle {
            write_bundle_config(
                &plan.manifest.bundle_layout,
                &hostname_for(&plan.manifest.spec),
                &plan.manifest.spec,
                plan.manifest.image_metadata.user.as_deref(),
                Some(plan.manifest.network_layout.netns_path.as_path()),
                &container_bundle_options(
                    &self.config.workload_state_root,
                    &plan.manifest.spec,
                    sandbox_id,
                    None,
                )?,
            )?;
            if self.config.start_mode == ContainerStartMode::Execute {
                self.write_manifest(&plan.manifest)?;
            }
        } else if self.config.start_mode == ContainerStartMode::Execute {
            self.write_manifest(&plan.manifest)?;
        }
        Ok(plan)
    }

    fn reserve_execute_launch_network(
        &self,
        manifest: &mut ContainerSandboxManifest,
        provision_network_plan: Option<&SandboxProvisionNetworkPlan>,
    ) -> Result<()> {
        let reservation_claim = match self.begin_launch_reservation(manifest) {
            Ok(claim) => claim,
            Err(error) => {
                let cleanup = self.release_unstarted_launch_artifacts(manifest).err();
                return Err(self.persist_failed_initial_launch(manifest, error, cleanup));
            }
        };
        self.complete_execute_launch_network_reservation(
            manifest,
            provision_network_plan,
            &reservation_claim,
        )
    }

    fn resume_execute_launch_network_reservation(
        &self,
        manifest: &mut ContainerSandboxManifest,
        provision_network_plan: &SandboxProvisionNetworkPlan,
        reservation_claim: &nimbus_network::NetworkReservationClaim,
    ) -> Result<()> {
        if manifest.launch_reservation_claim.as_ref() != Some(reservation_claim)
            || manifest.provision_network_plan.as_ref() != Some(provision_network_plan)
            || manifest.network_config.is_some()
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container provision reservation retry for {} crossed its exact durable desired plan or reservation phase",
                    manifest.handle.id
                ),
            });
        }
        self.complete_execute_launch_network_reservation(
            manifest,
            Some(provision_network_plan),
            reservation_claim,
        )
    }

    fn complete_execute_launch_network_reservation(
        &self,
        manifest: &mut ContainerSandboxManifest,
        provision_network_plan: Option<&SandboxProvisionNetworkPlan>,
        reservation_claim: &nimbus_network::NetworkReservationClaim,
    ) -> Result<()> {
        let manager = self.port_lease_coordinator();
        let sandbox_id = manifest.handle.id.clone();
        let planning_result = (|| -> Result<()> {
            // Placement begins only after the exact provider reservation claim
            // is durable in the canonical manifest.
            let attachment_id = provision_network_plan.map_or_else(
                || default_network_attachment_id(&sandbox_id),
                |plan| plan.attachment_id().clone(),
            );
            let mut network_config = self.place_sandbox_config(
                &manifest.spec.tenant_id,
                &manifest.network_layout,
                &sandbox_id,
                &attachment_id,
                reservation_claim,
            )?;
            network_config.network_plan =
                provision_network_plan.map(|plan| plan.network_plan().clone());
            #[cfg(test)]
            if network_config.network_plan.is_none() {
                network_config.network_plan = Some(
                    crate::provision::test_support::legacy_start_attachment_network_plan_fixture(
                        &manifest.spec,
                        &sandbox_id,
                        "container-coarse-start",
                    ),
                );
            }
            // Placement has returned with an exact attachment hold. Publish
            // that identity into the in-memory compensation authority before
            // any later reservation can fail; the outer failure path must not
            // fall back to a workload-derived attachment after a compiler-
            // supplied ID has acquired durable state.
            manifest.network_config = Some(network_config.clone());
            let internal_listener = egress_listener_reservation(&network_config)?;
            let mut reservations = match provision_network_plan {
                Some(plan) => manager.reserve_exact_provision_ports(
                    plan,
                    Some(internal_listener),
                    reservation_claim,
                )?,
                None => manager.reserve_launch_ports_for_sandbox(
                    SandboxLaunchPortPlan::new(
                        &manifest.spec.tenant_id,
                        &sandbox_id,
                        &manifest.spec.port_bindings,
                        &manifest.image_metadata.exposed_ports,
                    )
                    .with_internal_listener(internal_listener),
                    reservation_claim,
                )?,
            };
            let internal = reservations.internal_listener.clone().ok_or_else(|| {
                SandboxError::OperationFailed {
                    message: "container launch reservation omitted the required egress listener"
                        .to_owned(),
                }
            })?;
            let egress_proxy = egress_proxy_assignment(&network_config, internal)?;
            manifest.spec.port_bindings = reservations.published_bindings.clone();
            manifest.port_leases = reservations.published_leases.clone();
            manifest.egress_proxy = Some(egress_proxy);
            let status = manifest.status;
            synchronize_handle_status(manifest, status);
            if manifest.provision_prepared {
                write_bundle_config(
                    &manifest.bundle_layout,
                    &hostname_for(&manifest.spec),
                    &manifest.spec,
                    manifest.image_metadata.user.as_deref(),
                    Some(manifest.network_layout.netns_path.as_path()),
                    &container_bundle_options(
                        &self.config.workload_state_root,
                        &manifest.spec,
                        &sandbox_id,
                        manifest.egress_proxy.as_ref(),
                    )?,
                )?;
            }
            self.write_manifest(manifest)?;
            reservations.confirm_manifest_published()
        })();
        if let Err(error) = planning_result {
            let cleanup = self.release_unstarted_launch_artifacts(manifest).err();
            return Err(self.persist_failed_initial_launch(manifest, error, cleanup));
        }
        Ok(())
    }

    fn launch_manifest(
        &self,
        manifest: &mut ContainerSandboxManifest,
        clear_last_exit_code: bool,
    ) -> Result<()> {
        self.ensure_startup_reconciliation_ready()?;
        manifest.require_execution_admission_open("container launch")?;
        manifest.network_cleanup_complete = false;
        let reservation_claim = if clear_last_exit_code {
            Some(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .ok_or_else(|| SandboxError::OperationFailed {
                        message: format!(
                            "initial container launch for {} lacks never-bound reservation authority",
                            manifest.handle.id
                        ),
                    })?
                    .clone(),
            )
        } else {
            None
        };
        if let Some(reservation_claim) = reservation_claim.as_ref() {
            let attachment_id = manifest.require_network_config()?.attachment_id.clone();
            let mut launch_batch = manifest.port_leases.clone();
            if let Some(egress_proxy) = manifest.egress_proxy.as_ref() {
                launch_batch.push(egress_proxy.port_lease.clone());
            }
            if let Err(error) = self
                .port_lease_coordinator_for_manifest(manifest)?
                .require_never_bound_launch_batch(&launch_batch, reservation_claim)
            {
                return Err(self.compensate_reserved_launch(
                    &manifest.network_layout,
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                    &attachment_id,
                    reservation_claim,
                    error,
                ));
            }
            if let Err(error) = self.segment_allocator.adopt_reserved_attachment(
                &manifest.spec.tenant_id,
                &attachment_id,
                reservation_claim,
            ) {
                return Err(self.compensate_reserved_launch(
                    &manifest.network_layout,
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                    &attachment_id,
                    reservation_claim,
                    error,
                ));
            }
        }

        #[cfg(test)]
        if let Some(probe) = self.restart_launch_test_probe.as_ref() {
            if clear_last_exit_code {
                return Err(SandboxError::OperationFailed {
                    message:
                        "restart launch test probe cannot substitute for initial provider adoption"
                            .to_owned(),
                });
            }
            probe.intercept_provider_launch()?;
            manifest.shutdown_requested = false;
            synchronize_handle_status(manifest, SandboxStatus::Starting);
            return self.write_manifest(manifest);
        }

        let listener_release_authority = match reservation_claim.as_ref() {
            Some(claim) => MachinePortPreparationReleaseAuthority::FreshLaunch(claim),
            None => MachinePortPreparationReleaseAuthority::Retain,
        };
        let attachment_authority = reservation_claim.as_ref().map_or(
            AttachmentAttachAuthority::RestartRetained,
            AttachmentAttachAuthority::FreshLaunch,
        );
        self.configure_network(
            manifest,
            attachment_authority,
            listener_release_authority,
            true,
        )?;
        self.ensure_egress_proxy_running_with_release_authority(
            manifest,
            match reservation_claim.as_ref() {
                Some(claim) => PepPreAdoptionReleaseAuthority::FreshLaunch(claim),
                None => PepPreAdoptionReleaseAuthority::Retain,
            },
        )?;
        self.require_authenticated_egress_readiness(manifest)?;
        self.require_complete_attachment_readiness(manifest)?;
        let runtime_state = self.spawn_creator_and_wait_for_runtime(manifest)?;
        if runtime_state != "running" {
            run_status_checked(&manifest.conmon_launch.start_command)?;
        }

        manifest.shutdown_requested = false;
        if clear_last_exit_code {
            manifest.last_exit_code = None;
            manifest.launch_reservation_claim = None;
        }
        synchronize_handle_status(manifest, SandboxStatus::Starting);
        self.write_manifest(manifest)
    }

    fn execute_stop(&self, manifest: &mut ContainerSandboxManifest) -> Result<()> {
        if manifest.conmon_layout.exit_status_file.exists() {
            manifest.shutdown_requested = true;
            manifest.last_exit_code =
                Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
            synchronize_handle_status(manifest, SandboxStatus::Stopped);
            self.release_execution_artifacts(manifest)?;
            return self.write_existing_workload_manifest(manifest);
        }

        manifest.shutdown_requested = true;
        let pid = read_pid(&manifest.conmon_layout.pidfile)?;
        let stop_signal = configured_stop_signal(manifest.image_metadata.stop_signal.as_deref());
        signal_process(&stop_signal, pid)?;
        let stop_timeout = configured_stop_timeout(&manifest.spec, self.config.stop_timeout);
        if !wait_for_path(&manifest.conmon_layout.exit_status_file, stop_timeout) {
            signal_process("KILL", pid)?;
            if !wait_for_path(&manifest.conmon_layout.exit_status_file, stop_timeout) {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "sandbox {} did not write an exit file after TERM/KILL",
                        manifest.handle.id
                    ),
                });
            }
        }

        manifest.last_exit_code = Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
        synchronize_handle_status(manifest, SandboxStatus::Stopped);
        self.release_execution_artifacts(manifest)?;
        self.write_existing_workload_manifest(manifest)
    }

    #[cfg(test)]
    fn detect_runtime_status(&self, manifest: &ContainerSandboxManifest) -> Result<SandboxStatus> {
        detect_conmon_runtime_status(
            RuntimeStatusProbe {
                exit_status_file: &manifest.conmon_layout.exit_status_file,
                state_command: &manifest.conmon_launch.state_command,
                runtime_id: manifest.handle.id.as_str(),
                pidfile: &manifest.conmon_layout.pidfile,
                shutdown_requested: manifest.shutdown_requested,
                current_status: manifest.status,
            },
            || {
                let application_status =
                    running_status(manifest, self.readiness_probe_provider.as_ref());
                let mut readiness = self.authenticated_egress_readiness(manifest)?;
                if readiness.is_missing_registration() {
                    self.ensure_egress_proxy_running(manifest)?;
                    readiness = self.authenticated_egress_readiness(manifest)?;
                }
                let network_ready = self
                    .complete_attachment_readiness(manifest, readiness)?
                    .is_ready();
                if network_ready {
                    Ok(application_status)
                } else {
                    Ok(SandboxStatus::NotReady)
                }
            },
        )
    }

    fn prepare_image_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        image_reference: &str,
    ) -> Result<PreparedMaterializedImageLaunch> {
        OciImageMaterializer::for_tenant_sandbox(
            &self.config.workload_state_root,
            &spec.tenant_id,
            sandbox_id,
        )
        .prepare_image_launch(sandbox_id, image_reference, &spec.process)
    }

    fn prepare_built_image_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        image_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
    ) -> Result<PreparedMaterializedImageLaunch> {
        OciDockerfileBuilder::for_tenant_sandbox(
            &self.config.workload_state_root,
            &spec.tenant_id,
            sandbox_id,
        )
        .prepare_built_image_launch(
            sandbox_id,
            image_name,
            dockerfile_path,
            context_path,
            &spec.process,
        )
    }

    fn prepare_oci_image_start(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        source: &SandboxOciImageSource,
    ) -> Result<PreparedMaterializedImageLaunch> {
        match source {
            SandboxOciImageSource::Reference(reference) => {
                self.prepare_image_launch(spec, sandbox_id, &reference.reference)
            }
            SandboxOciImageSource::Build(build) => self.prepare_built_image_launch(
                spec,
                sandbox_id,
                &build.image_name,
                &build.dockerfile_path,
                &build.context_path,
            ),
        }
    }

    fn configure_network(
        &self,
        manifest: &ContainerSandboxManifest,
        attachment_authority: AttachmentAttachAuthority<'_>,
        listener_release_authority: MachinePortPreparationReleaseAuthority<'_>,
        publish_ingress: bool,
    ) -> Result<()> {
        let network_config = manifest.require_network_config()?.clone();
        self.validate_manifest_execution_context(manifest)?;
        let runner_config = &manifest.runner_config;
        let machine_port_forwarder =
            runner_config.validated_machine_port_forwarder(&manifest.handle.id)?;
        let ports = self.port_lease_coordinator_for_manifest(manifest)?;
        let hostname = hostname_for(&manifest.spec);
        let lifecycle = self.attachment_lifecycle(&ports);
        let adapter = if publish_ingress {
            self.attachment_adapter(manifest, &network_config, &hostname, machine_port_forwarder)
        } else {
            self.non_routable_attachment_adapter(manifest, &network_config, &hostname)
        };
        adapter.attach(&lifecycle, attachment_authority, |assigned_ips| {
            // The shared host-managed lifecycle deliberately leaves the
            // sandbox-specific PEP fence and machine publication adapters
            // at this composition boundary.
            if let Some(proxy) = manifest.egress_proxy.as_ref() {
                self.egress_pin_provider
                    .apply(&manifest.network_layout, proxy)?;
            }
            if publish_ingress && machine_port_forwarder.is_some() {
                self.ensure_machine_port_proxies_running_with_publication(
                    &manifest.handle.id,
                    assigned_ips,
                    manifest,
                    listener_release_authority,
                    || self.converge_exposed_machine_port_publication(manifest),
                )?;
            }
            Ok(())
        })?;
        Ok(())
    }

    fn ensure_egress_proxy_running(&self, manifest: &ContainerSandboxManifest) -> Result<()> {
        ensure_oci_egress_proxy_running(
            &self.egress_proxies,
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            manifest.egress_proxy.as_ref(),
            &manifest.spec.egress,
        )?;
        self.replay_stable_egress_reload_attempt(manifest)
    }

    fn ensure_egress_proxy_running_with_release_authority(
        &self,
        manifest: &ContainerSandboxManifest,
        release_authority: PepPreAdoptionReleaseAuthority<'_>,
    ) -> Result<()> {
        ensure_egress_proxy_running_with_release_authority(
            &self.egress_proxies,
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            manifest.egress_proxy.as_ref(),
            &manifest.spec.egress,
            release_authority,
        )?;
        // Once activation succeeds, replay failure must leave the registered
        // PEP and its Active lifetime evidence intact for exact retry. The
        // generic start path already owns pre-adoption compensation.
        self.replay_stable_egress_reload_attempt(manifest)
    }

    fn authenticated_egress_readiness(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<EgressReadinessState> {
        self.egress_proxies.authenticated_readiness(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            manifest.egress_proxy.as_ref(),
            &manifest.spec.egress,
            Some(&manifest.egress_policy_reload),
        )
    }

    fn require_authenticated_egress_readiness(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        match self.authenticated_egress_readiness(manifest)? {
            EgressReadinessState::Ready(_) => Ok(()),
            EgressReadinessState::NotReady(reason) => Err(SandboxError::OperationFailed {
                message: format!(
                    "container sandbox {} denied launch: egress PEP dependency is not ready: \
                     {reason:?}",
                    manifest.handle.id
                ),
            }),
        }
    }
}

fn container_tenant_volume_mounts(
    state_root: &Path,
    spec: &SandboxSpec,
) -> Result<Vec<ContainerBundleMount>> {
    crate::spec::validate_sandbox_mounts(&spec.mounts)
        .map_err(|message| SandboxError::InvalidSpec { message })?;
    let mut mounts = Vec::new();
    for mount in &spec.mounts {
        let destination = mount.destination.to_string_lossy().into_owned();
        let volume_name = mount
            .tenant_volume_name()
            .ok_or_else(|| SandboxError::InvalidSpec {
                message: "unsupported container sandbox mount source".to_owned(),
            })?;
        let source =
            crate::artifact_paths::tenant_volume_dir(state_root, &spec.tenant_id, volume_name);
        std::fs::create_dir_all(&source).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to create tenant volume {} for sandbox {} under {}: {error}",
                volume_name,
                spec.display_name(),
                source.display()
            ),
        })?;
        mounts.push(ContainerBundleMount {
            destination,
            source,
            options: tenant_volume_mount_options(mount.read_only),
        });
    }
    Ok(mounts)
}

fn container_bundle_options(
    state_root: &Path,
    spec: &SandboxSpec,
    sandbox_id: &SandboxId,
    egress_proxy: Option<&EgressProxyAssignment>,
) -> Result<ContainerBundleOptions> {
    let mut additional_mounts = container_tenant_volume_mounts(state_root, spec)?;
    let mut egress_trust_anchor_guest_path = None;
    if egress_proxy.is_some() {
        let trust_anchor = egress_trust_anchor_mount(state_root, &spec.tenant_id, sandbox_id)?;
        egress_trust_anchor_guest_path = Some(trust_anchor.guest_path.clone());
        additional_mounts.push(ContainerBundleMount {
            destination: trust_anchor.guest_path,
            source: trust_anchor.host_path,
            options: egress_trust_anchor_mount_options(),
        });
    }
    Ok(ContainerBundleOptions {
        additional_mounts,
        egress_proxy_url: egress_proxy
            .map(EgressProxyAssignment::proxy_url)
            .transpose()?,
        egress_trust_anchor_guest_path,
    })
}

fn tenant_volume_mount_options(read_only: bool) -> Vec<String> {
    vec![
        "rbind".to_owned(),
        if read_only { "ro" } else { "rw" }.to_owned(),
        "nosuid".to_owned(),
        "nodev".to_owned(),
    ]
}

fn egress_trust_anchor_mount_options() -> Vec<String> {
    vec![
        "rbind".to_owned(),
        "ro".to_owned(),
        "nosuid".to_owned(),
        "nodev".to_owned(),
        "noexec".to_owned(),
    ]
}

impl SandboxBackend for ContainerSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<crate::SandboxInspection>> {
        let backend = self.clone();
        let sandbox_id = id.clone();
        Box::pin(async move { backend.inspect_sync(&sandbox_id) })
    }

    fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
        let backend = self.clone();
        let sandbox_id = id.clone();
        Box::pin(async move { backend.stop_sync(&sandbox_id) })
    }

    fn reload_egress_policy(&self, id: &SandboxId, egress: EgressPolicy) -> SandboxFuture<()> {
        let backend = self.clone();
        let sandbox_id = id.clone();
        Box::pin(async move {
            ContainerSandboxBackend::reload_egress_policy(&backend, &sandbox_id, egress)
        })
    }

    fn remove_tenant_artifacts(&self, tenant_id: nimbus_core::TenantId) -> SandboxFuture<()> {
        let backend = self.clone();
        Box::pin(async move { backend.remove_tenant_artifacts_sync(&tenant_id) })
    }
}

#[cfg(test)]
mod support;

#[cfg(test)]
mod tests;
