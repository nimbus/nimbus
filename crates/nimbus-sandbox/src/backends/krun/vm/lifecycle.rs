use std::io::Write;

#[cfg(test)]
use super::readiness::running_status;
use super::readiness::synchronize_handle_status;
#[cfg(test)]
use super::start::ensure_guest_user_helper_available;
use super::start::hostname_for;
use super::*;
use crate::backends::conmon::lifecycle::RuntimeStateObservation;

pub(super) type NetworkArtifactTeardownMode = AttachmentTeardownMode;

pub(super) const KRUN_LIFECYCLE_LOCK_FILE: &str = ".nimbus-krun-lifecycle.lock";
const KRUN_LIFECYCLE_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const KRUN_LIFECYCLE_LOCK_RETRY: Duration = Duration::from_millis(25);

#[cfg(test)]
#[derive(Clone)]
pub(super) struct KrunLifecycleLockTestProbe {
    shared: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    timeout: Duration,
}

#[cfg(test)]
impl KrunLifecycleLockTestProbe {
    pub(super) fn new(timeout: Duration) -> Self {
        Self {
            shared: Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new())),
            timeout,
        }
    }

    fn record_contended(&self) -> Result<()> {
        let (lock, changed) = &*self.shared;
        let mut contended = lock.lock().map_err(|_| SandboxError::OperationFailed {
            message: "krun lifecycle-lock test probe was poisoned".to_owned(),
        })?;
        *contended = true;
        changed.notify_all();
        Ok(())
    }

    pub(super) fn wait_until_contended(&self) -> bool {
        let (lock, changed) = &*self.shared;
        let contended = lock
            .lock()
            .expect("krun lifecycle-lock test probe should not be poisoned");
        let (contended, _) = changed
            .wait_timeout_while(contended, self.timeout, |contended| !*contended)
            .expect("krun lifecycle-lock test probe wait should not be poisoned");
        *contended
    }
}

pub(super) struct KrunLifecycleGuard {
    _lock: std::fs::File,
}

pub(super) struct KrunInspectionGuard {
    _lock: std::fs::File,
}

impl KrunSandboxBackend {
    pub(super) fn stop_sync(&self, id: &SandboxId) -> Result<()> {
        let Some(observed) = self.read_manifest(id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: id.as_str().to_owned(),
            });
        };
        let _lifecycle = self.lock_launch_lifecycle(&observed)?;
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: id.as_str().to_owned(),
            });
        };

        match self.config.start_mode {
            KrunStartMode::PlanOnly => {
                manifest.shutdown_requested = true;
                manifest.last_exit_code = Some(0);
                manifest.status = SandboxStatus::Stopped;
                manifest.handle.status = SandboxStatus::Stopped;
                self.cleanup_manifest_launch_artifacts(&manifest)?;
                manifest.launch_artifact = None;
                self.write_manifest(&manifest)
            }
            KrunStartMode::Execute => {
                self.reconcile_pending_creator_before_cleanup(&mut manifest)?;
                if manifest.provider_failure_cleanup.is_active() {
                    return self.resume_provider_failure_cleanup(&mut manifest);
                }
                match &manifest.launch_authority {
                    KrunLaunchAuthority::Reserved { .. } => {
                        self.stop_reserved_launch(&mut manifest)
                    }
                    KrunLaunchAuthority::Adopting { .. } => {
                        self.stop_adopting_launch(&mut manifest)
                    }
                    KrunLaunchAuthority::Adopted { .. } | KrunLaunchAuthority::ProviderOwned => {
                        self.execute_stop(&mut manifest)
                    }
                    KrunLaunchAuthority::Released => {
                        manifest.shutdown_requested = true;
                        manifest.next_restart_at_millis = None;
                        let terminal_status = if manifest.status == SandboxStatus::Failed {
                            SandboxStatus::Failed
                        } else {
                            SandboxStatus::Stopped
                        };
                        synchronize_handle_status(&mut manifest, terminal_status);
                        self.persist_effect_barrier(&manifest, "released krun stop completion")
                    }
                    KrunLaunchAuthority::PlanOnly => Err(SandboxError::OperationFailed {
                        message: format!(
                            "execute-mode krun workload {} carries plan-only launch authority",
                            manifest.handle.id
                        ),
                    }),
                }
            }
        }
    }

    pub(super) fn stop_reserved_launch(&self, manifest: &mut KrunSandboxManifest) -> Result<()> {
        manifest.shutdown_requested = true;
        manifest.next_restart_at_millis = None;
        synchronize_handle_status(manifest, SandboxStatus::Stopping);
        self.persist_effect_barrier(manifest, "reserved krun stop intent")?;
        self.release_reserved_launch(manifest)?;
        self.cleanup_manifest_launch_artifacts(manifest)?;
        manifest.launch_artifact = None;
        manifest.launch_authority = KrunLaunchAuthority::Released;
        synchronize_handle_status(manifest, SandboxStatus::Stopped);
        self.persist_effect_barrier(manifest, "reserved krun stop completion")
    }

    #[cfg(test)]
    pub(super) fn execute_start_after_preflight(
        &self,
        launch_plan: &KrunStartPlan,
        preflight: Result<()>,
    ) -> Result<SandboxHandle> {
        let mut manifest = launch_plan.manifest.clone();
        let _lifecycle = self.lock_launch_lifecycle(&manifest)?;
        self.require_current_launch_plan(&manifest)?;
        if let Err(error) = preflight {
            return Err(self.persist_unstarted_launch_failure(&mut manifest, error));
        }
        self.launch_manifest(&mut manifest, true)?;
        Ok(manifest.handle)
    }

    pub(super) fn lock_launch_lifecycle(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<KrunLifecycleGuard> {
        self.lock_launch_lifecycle_dir(&manifest.conmon_layout.container_state_dir)
    }

    pub(super) fn lock_current_inspection(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<(KrunInspectionGuard, KrunSandboxManifest)> {
        self.lock_current_inspection_with_timeout(manifest, KRUN_LIFECYCLE_LOCK_TIMEOUT)
    }

    #[cfg(test)]
    pub(super) fn lock_current_inspection_with_timeout_for_test(
        &self,
        manifest: &KrunSandboxManifest,
        timeout: Duration,
    ) -> Result<(KrunInspectionGuard, KrunSandboxManifest)> {
        self.lock_current_inspection_with_timeout(manifest, timeout)
    }

    fn lock_current_inspection_with_timeout(
        &self,
        manifest: &KrunSandboxManifest,
        timeout: Duration,
    ) -> Result<(KrunInspectionGuard, KrunSandboxManifest)> {
        let lock_path = manifest
            .conmon_layout
            .container_state_dir
            .join(KRUN_LIFECYCLE_LOCK_FILE);
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to open existing krun inspection lock {}: {error}; \
                     inspection cannot create synchronization state",
                    lock_path.display()
                ),
            })?;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match FileExt::try_lock_shared(&lock) {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    #[cfg(test)]
                    if let Some(probe) = self.lifecycle_lock_test_probe.as_ref() {
                        probe.record_contended()?;
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(SandboxError::OperationFailed {
                            message: format!(
                                "timed out acquiring existing krun inspection lock {}; \
                                 observation remains unknown",
                                lock_path.display()
                            ),
                        });
                    }
                    std::thread::sleep(KRUN_LIFECYCLE_LOCK_RETRY);
                }
                Err(error) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "failed to acquire existing krun inspection lock {}: {error}",
                            lock_path.display()
                        ),
                    });
                }
            }
        }
        let Some(persisted) = self.read_manifest(&manifest.handle.id)? else {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun manifest {} disappeared while its inspection lock was held",
                    manifest.handle.id
                ),
            });
        };
        Ok((KrunInspectionGuard { _lock: lock }, persisted))
    }

    pub(super) fn lock_launch_lifecycle_for(
        &self,
        tenant: &nimbus_core::TenantId,
        sandbox_id: &SandboxId,
    ) -> Result<KrunLifecycleGuard> {
        let layout =
            OciConmonLayout::new_for_tenant(&self.config.workload_state_root, tenant, sandbox_id);
        self.lock_launch_lifecycle_dir(&layout.container_state_dir)
    }

    fn lock_launch_lifecycle_dir(
        &self,
        container_state_dir: &std::path::Path,
    ) -> Result<KrunLifecycleGuard> {
        std::fs::create_dir_all(container_state_dir).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to create krun lifecycle lock directory {}: {error}",
                    container_state_dir.display()
                ),
            }
        })?;
        let lock_path = container_state_dir.join(KRUN_LIFECYCLE_LOCK_FILE);
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to open krun lifecycle lock {}: {error}",
                    lock_path.display()
                ),
            })?;
        let deadline = std::time::Instant::now() + KRUN_LIFECYCLE_LOCK_TIMEOUT;
        loop {
            match FileExt::try_lock_exclusive(&lock) {
                Ok(()) => return Ok(KrunLifecycleGuard { _lock: lock }),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    #[cfg(test)]
                    if let Some(probe) = self.lifecycle_lock_test_probe.as_ref() {
                        probe.record_contended()?;
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(SandboxError::OperationFailed {
                            message: format!(
                                "timed out acquiring krun lifecycle lock {}; launch remains fenced",
                                lock_path.display()
                            ),
                        });
                    }
                    std::thread::sleep(KRUN_LIFECYCLE_LOCK_RETRY);
                }
                Err(error) => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "failed to acquire krun lifecycle lock {}: {error}",
                            lock_path.display()
                        ),
                    });
                }
            }
        }
    }

    pub(super) fn require_current_launch_plan(
        &self,
        candidate: &KrunSandboxManifest,
    ) -> Result<()> {
        let persisted = self.read_manifest(&candidate.handle.id)?.ok_or_else(|| {
            SandboxError::OperationFailed {
                message: format!(
                    "krun launch {} has no durable manifest; refusing provider effects",
                    candidate.handle.id
                ),
            }
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
                "krun launch {} no longer owns the durable reserved or adopted launch plan; \
                 refusing stale provider execution or compensation",
                candidate.handle.id
            ),
        })
    }

    pub(super) fn persist_unstarted_launch_failure(
        &self,
        manifest: &mut KrunSandboxManifest,
        primary: SandboxError,
    ) -> SandboxError {
        self.persist_unstarted_launch_failure_inner(manifest, primary, None)
    }

    pub(super) fn persist_unstarted_launch_failure_with_reservations(
        &self,
        manifest: &mut KrunSandboxManifest,
        primary: SandboxError,
        reservations: &ReservedLaunchPorts,
    ) -> SandboxError {
        self.persist_unstarted_launch_failure_inner(manifest, primary, Some(reservations))
    }

    fn persist_unstarted_launch_failure_inner(
        &self,
        manifest: &mut KrunSandboxManifest,
        primary: SandboxError,
        reservations: Option<&ReservedLaunchPorts>,
    ) -> SandboxError {
        manifest.shutdown_requested = true;
        manifest.next_restart_at_millis = None;
        manifest.last_exit_code = None;
        synchronize_handle_status(manifest, SandboxStatus::Stopping);
        if let Err(barrier) =
            self.persist_effect_barrier(manifest, "krun pre-effect cleanup intent")
        {
            return SandboxError::OperationFailed {
                message: format!(
                    "krun launch failed: {primary}; cleanup remains fenced because its durable \
                     intent could not be confirmed: {barrier}"
                ),
            };
        }
        let mut secondary = Vec::new();

        let artifact_released = match self.cleanup_manifest_launch_artifacts(manifest) {
            Ok(()) => {
                manifest.launch_artifact = None;
                true
            }
            Err(error) => {
                secondary.push(format!("krun launch artifact compensation failed: {error}"));
                false
            }
        };
        // Artifact and reservation authority are independent. A failed
        // materialization cleanup must not strand a safely releasable
        // never-bound network generation.
        let network_released = match reservations {
            Some(reservations) => self.release_unpublished_reserved_launch(manifest, reservations),
            None => self.release_reserved_launch(manifest),
        }
        .map(|()| true)
        .unwrap_or_else(|error| {
            secondary.push(format!(
                "exact krun launch reservation compensation failed: {error}"
            ));
            false
        });
        if network_released && artifact_released {
            manifest.launch_authority = KrunLaunchAuthority::Released;
            synchronize_handle_status(manifest, SandboxStatus::Failed);
        }
        if let Err(error) =
            self.persist_effect_barrier(manifest, "unstarted krun launch compensation result")
        {
            secondary.push(format!(
                "failed to persist krun launch compensation result: {error}"
            ));
        }
        if secondary.is_empty() {
            primary
        } else {
            SandboxError::OperationFailed {
                message: format!(
                    "krun launch failed: {primary}; compensation also failed: {}",
                    secondary.join("; ")
                ),
            }
        }
    }

    fn execute_stop(&self, manifest: &mut KrunSandboxManifest) -> Result<()> {
        manifest.shutdown_requested = true;
        manifest.next_restart_at_millis = None;
        synchronize_handle_status(manifest, SandboxStatus::Stopping);
        self.persist_effect_barrier(manifest, "explicit krun stop intent")?;

        if manifest.conmon_layout.exit_status_file.exists() {
            manifest.last_exit_code =
                Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
            self.release_network_artifacts(manifest, NetworkArtifactTeardownMode::Final)?;
            self.cleanup_manifest_launch_artifacts(manifest)?;
            manifest.launch_artifact = None;
            manifest.launch_authority = KrunLaunchAuthority::Released;
            synchronize_handle_status(manifest, SandboxStatus::Stopped);
            return self.persist_effect_barrier(manifest, "explicit krun stop completion");
        }

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
        self.release_network_artifacts(manifest, NetworkArtifactTeardownMode::Final)?;
        self.cleanup_manifest_launch_artifacts(manifest)?;
        manifest.launch_artifact = None;
        manifest.launch_authority = KrunLaunchAuthority::Released;
        synchronize_handle_status(manifest, SandboxStatus::Stopped);
        self.persist_effect_barrier(manifest, "explicit krun stop completion")
    }

    #[cfg(test)]
    pub(super) fn execute_stop_for_test(&self, manifest: &mut KrunSandboxManifest) -> Result<()> {
        self.execute_stop(manifest)
    }

    #[cfg(test)]
    pub(super) fn detect_runtime_status(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<SandboxStatus> {
        detect_conmon_runtime_status(
            RuntimeStatusProbe {
                exit_status_file: &manifest.conmon_layout.exit_status_file,
                state_command: &manifest.conmon_launch.state_command,
                runtime_id: manifest.handle.id.as_str(),
                pidfile: &manifest.conmon_layout.pidfile,
                shutdown_requested: manifest.shutdown_requested,
                current_status: manifest.status,
            },
            || self.running_status_with_egress(manifest),
        )
    }

    #[cfg(test)]
    pub(super) fn running_status_with_egress(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<SandboxStatus> {
        let application_status = running_status(manifest, self.readiness_probe_provider.as_ref());
        if self
            .host_managed_attachment_readiness(
                manifest,
                self.authenticated_egress_readiness(manifest)?,
            )?
            .is_ready()
        {
            Ok(application_status)
        } else {
            Ok(SandboxStatus::NotReady)
        }
    }

    pub(super) fn running_status_with_egress_evidence(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<(SandboxStatus, Vec<u8>)> {
        let application = crate::backends::readiness_probe::inspect_application_readiness(
            manifest.status,
            &super::readiness::published_endpoints(&manifest.spec),
            super::readiness::readiness_probe_timeout(manifest),
            self.readiness_probe_provider.as_ref(),
        );
        let readiness = self.host_managed_attachment_readiness(
            manifest,
            self.authenticated_egress_readiness(manifest)?,
        )?;
        let status = if readiness.is_ready() {
            application.status()
        } else {
            SandboxStatus::NotReady
        };
        let evidence =
            serde_json::to_vec(&(&application, format!("{readiness:?}"))).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to serialize krun readiness observation for {}: {error}",
                        manifest.handle.id
                    ),
                }
            })?;
        Ok((status, evidence))
    }

    #[cfg(test)]
    pub(super) fn launch_manifest(
        &self,
        manifest: &mut KrunSandboxManifest,
        clear_last_exit_code: bool,
    ) -> Result<()> {
        self.ensure_startup_network_reconciliation_ready()?;
        let reservation_claim = if clear_last_exit_code {
            Some(manifest.require_reserved_claim()?.clone())
        } else {
            if !matches!(
                manifest.launch_authority,
                KrunLaunchAuthority::ProviderOwned
            ) {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "krun restart for {} requires provider-owned launch authority, got {:?}",
                        manifest.handle.id, manifest.launch_authority
                    ),
                });
            }
            None
        };
        if let Some(reservation_claim) = reservation_claim.as_ref() {
            let mut launch_batch = manifest.port_leases.clone();
            if let Some(egress_proxy) = manifest.egress_proxy.as_ref() {
                launch_batch.push(egress_proxy.port_lease.clone());
            }
            if let Err(error) = self
                .port_lease_coordinator()
                .require_never_bound_launch_batch(&launch_batch, reservation_claim)
            {
                return Err(self.persist_unstarted_launch_failure(manifest, error));
            }
            if matches!(
                manifest.launch_authority,
                KrunLaunchAuthority::Reserved { .. }
            ) {
                manifest.mark_adopting()?;
                if let Err(error) =
                    self.persist_effect_barrier(manifest, "krun attachment-adoption intent")
                {
                    return Err(self.persist_unstarted_launch_failure(manifest, error));
                }
            }
            let attachment_id = manifest
                .network_config
                .as_ref()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "krun launch for {} lacks reserved attachment identity",
                        manifest.handle.id
                    ),
                })?
                .attachment_id
                .clone();
            if let Err(error) = self.segment_allocator.adopt_reserved_attachment(
                &manifest.spec.tenant_id,
                &attachment_id,
                reservation_claim,
            ) {
                return Err(self.persist_unstarted_launch_failure(manifest, error));
            }
            manifest.mark_adopted()?;
            if let Err(error) =
                self.persist_effect_barrier(manifest, "krun adopted attachment authority")
            {
                return Err(self.persist_provider_launch_failure(manifest, error));
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
            manifest.next_restart_at_millis = None;
            synchronize_handle_status(manifest, SandboxStatus::Starting);
            return self.write_manifest(manifest);
        }

        let attachment_authority = reservation_claim.as_ref().map_or(
            AttachmentAttachAuthority::RestartRetained,
            AttachmentAttachAuthority::FreshLaunch,
        );
        let provider_launch = ensure_linux_host("krun")
            .and_then(|()| ensure_guest_user_helper_available(&self.config, manifest))
            .and_then(|()| self.configure_network(manifest, attachment_authority, true))
            .and_then(|()| {
                self.launch_into_network(manifest, clear_last_exit_code, reservation_claim.as_ref())
            });
        if let Err(error) = provider_launch {
            return Err(self.persist_provider_launch_failure(manifest, error));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn persist_provider_launch_failure(
        &self,
        manifest: &mut KrunSandboxManifest,
        primary: SandboxError,
    ) -> SandboxError {
        if !manifest.permits_provider_teardown() {
            return SandboxError::OperationFailed {
                message: format!(
                    "krun launch failed: {primary}; broad cleanup rejected because launch authority \
                     is {:?}",
                    manifest.launch_authority
                ),
            };
        }
        if !manifest.creator_handoff.authorizes_provider_cleanup() {
            return SandboxError::OperationFailed {
                message: format!(
                    "krun launch failed: {primary}; broad cleanup rejected because creator \
                     handoff {:?} may still materialize provider effects",
                    manifest.creator_handoff
                ),
            };
        }

        manifest.shutdown_requested = true;
        manifest.next_restart_at_millis = None;
        synchronize_handle_status(manifest, SandboxStatus::Stopping);
        if !manifest.provider_failure_cleanup.is_active() {
            manifest.provider_failure_cleanup = KrunProviderFailureCleanupState::Requested;
            if let Err(barrier) =
                self.persist_effect_barrier(manifest, "provider-owned krun cleanup intent")
            {
                return SandboxError::OperationFailed {
                    message: format!(
                        "krun launch failed: {primary}; provider cleanup remains fenced because its \
                         durable intent could not be confirmed: {barrier}"
                    ),
                };
            }
        }
        match self.resume_provider_failure_cleanup(manifest) {
            Ok(()) => primary,
            Err(cleanup) => SandboxError::OperationFailed {
                message: format!("krun launch failed: {primary}; cleanup also failed: {cleanup}"),
            },
        }
    }

    pub(super) fn resume_provider_failure_cleanup(
        &self,
        manifest: &mut KrunSandboxManifest,
    ) -> Result<()> {
        if !manifest.provider_failure_cleanup.is_active() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun workload {} has no active provider-failure cleanup to resume",
                    manifest.handle.id
                ),
            });
        }
        if !manifest.permits_provider_teardown() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun workload {} cannot resume provider-failure cleanup from launch authority \
                     {:?}",
                    manifest.handle.id, manifest.launch_authority
                ),
            });
        }
        if !manifest.creator_handoff.authorizes_provider_cleanup() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun workload {} cannot resume provider-failure cleanup while creator handoff \
                     {:?} may still materialize provider effects",
                    manifest.handle.id, manifest.creator_handoff
                ),
            });
        }

        loop {
            match manifest.provider_failure_cleanup.clone() {
                KrunProviderFailureCleanupState::Inactive => {
                    unreachable!("active provider-failure cleanup cannot become inactive mid-loop")
                }
                KrunProviderFailureCleanupState::Requested => {
                    let proof = self.provider_failure_runtime_absence_proof(manifest)?;
                    manifest.provider_failure_cleanup =
                        KrunProviderFailureCleanupState::RuntimeAbsent { proof };
                    self.persist_effect_barrier(
                        manifest,
                        "provider-failure runtime-absence checkpoint",
                    )?;
                }
                KrunProviderFailureCleanupState::RuntimeAbsent { proof } => {
                    self.validate_provider_failure_runtime_absence_proof(manifest, &proof)?;
                    self.release_network_artifacts(manifest, NetworkArtifactTeardownMode::Final)?;
                    manifest.provider_failure_cleanup =
                        KrunProviderFailureCleanupState::NetworkReleased {
                            runtime_absence: proof,
                        };
                    self.persist_effect_barrier(
                        manifest,
                        "provider-failure network-release checkpoint",
                    )?;
                }
                KrunProviderFailureCleanupState::NetworkReleased { runtime_absence } => {
                    self.validate_provider_failure_runtime_absence_proof(
                        manifest,
                        &runtime_absence,
                    )?;
                    self.cleanup_manifest_launch_artifacts(manifest)?;
                    manifest.launch_artifact = None;
                    manifest.provider_failure_cleanup =
                        KrunProviderFailureCleanupState::ArtifactsReleased { runtime_absence };
                    self.persist_effect_barrier(
                        manifest,
                        "provider-failure artifact-release checkpoint",
                    )?;
                }
                KrunProviderFailureCleanupState::ArtifactsReleased { runtime_absence } => {
                    self.validate_provider_failure_runtime_absence_proof(
                        manifest,
                        &runtime_absence,
                    )?;
                    manifest.launch_authority = KrunLaunchAuthority::Released;
                    manifest.provider_failure_cleanup = KrunProviderFailureCleanupState::Inactive;
                    synchronize_handle_status(manifest, SandboxStatus::Failed);
                    self.persist_effect_barrier(manifest, "provider-owned krun cleanup result")?;
                    return Ok(());
                }
            }
        }
    }

    fn provider_failure_runtime_absence_proof(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<KrunRuntimeAbsenceProof> {
        match &manifest.creator_handoff {
            KrunCreatorHandoffState::NotSpawned
                if matches!(
                    manifest.launch_authority,
                    KrunLaunchAuthority::Adopted { .. }
                ) =>
            {
                Ok(KrunRuntimeAbsenceProof::NeverSpawned)
            }
            KrunCreatorHandoffState::NotSpawned => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun workload {} carries NotSpawned creator authority with incompatible launch \
                     authority {:?}; refusing to infer runtime absence",
                    manifest.handle.id, manifest.launch_authority
                ),
            }),
            KrunCreatorHandoffState::SpawnIntent { .. }
            | KrunCreatorHandoffState::Pending { .. } => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun workload {} cannot prove runtime absence while creator handoff {:?} may \
                     still materialize provider effects",
                    manifest.handle.id, manifest.creator_handoff
                ),
            }),
            KrunCreatorHandoffState::Quiesced { proof } => {
                let attempt_id = proof.attempt_id().to_owned();
                self.delete_runtime_and_confirm_absent(manifest)?;
                Ok(KrunRuntimeAbsenceProof::ObservedAbsent { attempt_id })
            }
            KrunCreatorHandoffState::RuntimeObserved { receipt } => {
                let attempt_id = receipt.attempt_id().to_owned();
                self.delete_runtime_and_confirm_absent(manifest)?;
                Ok(KrunRuntimeAbsenceProof::ObservedAbsent { attempt_id })
            }
        }
    }

    fn validate_provider_failure_runtime_absence_proof(
        &self,
        manifest: &KrunSandboxManifest,
        proof: &KrunRuntimeAbsenceProof,
    ) -> Result<()> {
        let valid = match (proof, &manifest.creator_handoff) {
            (KrunRuntimeAbsenceProof::NeverSpawned, KrunCreatorHandoffState::NotSpawned) => {
                matches!(
                    manifest.launch_authority,
                    KrunLaunchAuthority::Adopted { .. }
                )
            }
            (
                KrunRuntimeAbsenceProof::ObservedAbsent {
                    attempt_id: proof_attempt_id,
                },
                KrunCreatorHandoffState::Quiesced { proof },
            ) => proof_attempt_id == proof.attempt_id(),
            (
                KrunRuntimeAbsenceProof::ObservedAbsent {
                    attempt_id: proof_attempt_id,
                },
                KrunCreatorHandoffState::RuntimeObserved { receipt },
            ) => proof_attempt_id == receipt.attempt_id(),
            _ => false,
        };
        if valid {
            return Ok(());
        }

        Err(SandboxError::OperationFailed {
            message: format!(
                "krun workload {} carries provider-failure runtime-absence proof {:?} that is \
                 incompatible with creator handoff {:?} and launch authority {:?}; refusing to \
                 release provider effects",
                manifest.handle.id, proof, manifest.creator_handoff, manifest.launch_authority
            ),
        })
    }

    pub(super) fn delete_runtime_and_confirm_absent(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<()> {
        let delete_error = run_status_best_effort(&manifest.conmon_launch.delete_command).err();
        match runtime_state(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
        ) {
            Ok(RuntimeStateObservation::ExplicitlyAbsent) => Ok(()),
            Ok(RuntimeStateObservation::Present(status)) => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun runtime {} remains {status:?} after delete attempt{}",
                    manifest.handle.id,
                    delete_error
                        .as_ref()
                        .map(|error| format!(" ({error})"))
                        .unwrap_or_default()
                ),
            }),
            Err(observe_error) => Err(SandboxError::OperationFailed {
                message: format!(
                    "cannot confirm krun runtime {} absence after delete attempt: \
                     {observe_error}{}",
                    manifest.handle.id,
                    delete_error
                        .as_ref()
                        .map(|error| format!("; delete diagnostic: {error}"))
                        .unwrap_or_default()
                ),
            }),
        }
    }

    #[cfg(test)]
    fn launch_into_network(
        &self,
        manifest: &mut KrunSandboxManifest,
        clear_last_exit_code: bool,
        reservation_claim: Option<&nimbus_network::NetworkReservationClaim>,
    ) -> Result<()> {
        self.ensure_egress_proxy_running_with_release_authority(
            manifest,
            match reservation_claim {
                Some(claim) => {
                    crate::backends::oci::egress::PepPreAdoptionReleaseAuthority::FreshLaunch(claim)
                }
                None => crate::backends::oci::egress::PepPreAdoptionReleaseAuthority::Retain,
            },
        )?;
        // Fail-closed readiness gate: the last checkpoint before crun spawns the
        // VMM into the namespace. Permit only when the platform supports
        // enforcement (Linux), the deny-by-default netns is installed, and the
        // per-sandbox egress PEP is running with an active policy generation. Any
        // not-ready precondition returns Err here, which `launch_manifest`
        // converts into a full netns/VMM teardown — no path reaches
        // the owned creator with unenforced egress.
        self.ensure_execute_egress_enforced(manifest)?;
        let runtime_state = self.spawn_creator_and_wait_for_runtime(manifest)?;
        if runtime_state != "running" {
            run_status_checked(&manifest.conmon_launch.start_command)?;
        }

        manifest.shutdown_requested = false;
        manifest.next_restart_at_millis = None;
        if clear_last_exit_code {
            manifest.last_exit_code = None;
            manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;
        }
        synchronize_handle_status(manifest, SandboxStatus::Starting);
        self.persist_effect_barrier(manifest, "krun provider launch result")
    }

    /// Stand up the sandbox's deny-by-default network namespace: create the
    /// persistent netns, run the shared netavark setup (no-default-route deny
    /// chain + inbound published-port DNAT), then pin egress to this sandbox's
    /// own PEP. Fail-closed: on any failure the half-built namespace is torn
    /// down so the VMM is never launched into an unconfigured or unpinned netns.
    /// Reuses the container backend's shared netns free-functions; no
    /// netns/netavark/IPAM logic is forked here.
    pub(super) fn configure_network(
        &self,
        manifest: &KrunSandboxManifest,
        attachment_authority: AttachmentAttachAuthority<'_>,
        publish_ingress: bool,
    ) -> Result<()> {
        let network_config = manifest.require_network_config()?;
        let ports = self.port_lease_coordinator();
        let hostname = hostname_for(&manifest.spec);
        let lifecycle = self.attachment_lifecycle(&ports);
        let adapter = if publish_ingress {
            self.attachment_adapter(manifest, network_config, &hostname)
        } else {
            self.non_routable_attachment_adapter(manifest, network_config, &hostname)
        };

        // Pin the netns so the ONLY reachable egress is this sandbox's own PEP.
        // The netavark deny is route-based, but the shared bridge gateway is
        // on-link and every sibling sandbox's PEP listens on it at a distinct
        // port; without this pin an execute-mode guest could egress through a
        // sibling tenant's proxy and its injected credentials (audit H1). Under
        // libkrun TSI the guest's outbound sockets are issued by this host VMM
        // process inside the netns, so the output-hook pin governs the guest
        // exactly as it governs a container. Tear the namespace back down on
        // failure so the VMM never launches into an unpinned netns.
        adapter
            .attach(&lifecycle, attachment_authority, |_| {
                if let Some(proxy) = manifest.egress_proxy.as_ref() {
                    self.egress_pin_provider
                        .apply(&manifest.network_layout, proxy)?;
                }
                Ok(())
            })
            .map(|_| ())
    }

    /// Preserve the focused fault fixture while routing its compensation
    /// through the shared OCI attachment lifecycle.
    #[cfg(test)]
    pub(super) fn failed_netavark_configuration(
        &self,
        manifest: &KrunSandboxManifest,
        network_config: &OciNetworkConfig,
        batch: crate::backends::oci::port_lease::OciPortBindLifetimeBatch,
        primary: SandboxError,
    ) -> SandboxError {
        let ports = self.port_lease_coordinator();
        let hostname = hostname_for(&manifest.spec);
        let lifecycle = self.attachment_lifecycle(&ports);
        self.attachment_adapter(manifest, network_config, &hostname)
            .compensate_injected_host_setup_failure(&lifecycle, batch, primary)
    }

    /// Fail-closed readiness gate evaluated immediately before the VMM launches.
    ///
    /// Permits the launch only when ALL hold: (1) the binary is built for a
    /// Linux target (`ensure_linux_host`, a compile-time `cfg!(target_os =
    /// "linux")` check — it does NOT probe `/dev/kvm`; actual KVM availability
    /// is enforced downstream by crun/libkrun, which fail closed at VMM spawn
    /// when `/dev/kvm` is absent, so a Linux host without KVM never reaches an
    /// enforced-egress-bypassing state), (2) the sandbox's deny-by-default
    /// network namespace is installed, and (3) the per-sandbox egress PEP is
    /// running AND ready (it reports an active policy generation). Any missing
    /// precondition, any lookup error, or a not-ready PEP returns `Err`, which
    /// the caller treats as deny: the VMM is never spawned and the half-built
    /// namespace is torn down. The gate never degrades to allow.
    #[cfg(test)]
    pub(super) fn ensure_execute_egress_enforced(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<()> {
        // (1) Platform: the krun execute path is a Linux build target. This is a
        // compile-time cfg check, not a /dev/kvm probe; a Linux host without KVM
        // still fails closed because crun/libkrun cannot spawn the VMM without
        // /dev/kvm. Deny on every non-Linux build.
        ensure_linux_host("krun")?;
        let state = self.host_managed_attachment_readiness(
            manifest,
            self.authenticated_egress_readiness(manifest)?,
        )?;
        match state {
            OciAttachmentReadinessState::Ready(_) => Ok(()),
            OciAttachmentReadinessState::NotReady(reason) => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun sandbox {} denied launch: complete network attachment is not ready: \
                         {reason:?}",
                    manifest.handle.id
                ),
            }),
        }
    }

    /// Platform-independent test view of the production attachment-readiness
    /// decision. The real launch gate adds the host-platform check before this
    /// exact decision; keeping the platform check out of this test seam lets
    /// every host prove incomplete network evidence is denied.
    #[cfg(test)]
    pub(super) fn ensure_complete_host_managed_attachment_readiness_for_test(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<()> {
        let state = self.host_managed_attachment_readiness(
            manifest,
            self.authenticated_egress_readiness(manifest)?,
        )?;
        match state {
            OciAttachmentReadinessState::Ready(_) => Ok(()),
            OciAttachmentReadinessState::NotReady(reason) => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun sandbox {} denied launch: complete network attachment is not \
                         ready: {reason:?}",
                    manifest.handle.id
                ),
            }),
        }
    }

    #[cfg(test)]
    pub(super) fn require_authenticated_egress_readiness(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<()> {
        match self.authenticated_egress_readiness(manifest)? {
            EgressReadinessState::Ready(_) => Ok(()),
            EgressReadinessState::NotReady(reason) => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun sandbox {} denied launch: egress PEP dependency is not ready: \
                     {reason:?}",
                    manifest.handle.id
                ),
            }),
        }
    }

    /// Platform-independent half of the readiness gate: deny unless the
    /// deny-by-default netns is installed AND the egress PEP for `id` is running
    /// with an active policy generation. Split out from
    /// [`KrunSandboxBackend::ensure_execute_egress_enforced`] so the deny/permit
    /// matrix is unit-testable without a Linux host or `/dev/kvm`.
    #[cfg(test)]
    pub(super) fn ensure_execute_egress_preconditions(
        &self,
        tenant_id: &nimbus_core::TenantId,
        id: &SandboxId,
        netns_path: &Path,
    ) -> Result<()> {
        // (1) The deny-by-default network namespace must already be installed.
        //
        // Reaching this gate proves `configure_network` returned success after
        // Netavark setup and egress pinning; failed setup returns before this
        // call and deliberately retains the namespace as conservative retry
        // evidence when detach is ambiguous. Path existence is therefore only
        // consumed inside this success-only control-flow edge, never as
        // standalone proof that an arbitrary persisted namespace is enforced.
        if !netns_path.exists() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun sandbox {id} denied launch: deny-by-default network namespace {} is not installed",
                    netns_path.display()
                ),
            });
        }
        // (2) The per-sandbox egress PEP must be running AND ready.
        match self.egress_proxies.readiness(tenant_id, id)? {
            None => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun sandbox {id} denied launch: no egress policy-enforcement proxy is running for the deny-by-default namespace"
                ),
            }),
            Some(readiness) if !readiness.is_ready() || readiness.policy_generation().is_none() => {
                Err(SandboxError::OperationFailed {
                    message: format!(
                        "krun sandbox {id} denied launch: egress policy-enforcement proxy is not ready (no active policy generation loaded)"
                    ),
                })
            }
            Some(_) => Ok(()),
        }
    }

    pub(super) fn authenticated_egress_readiness(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<EgressReadinessState> {
        self.egress_proxies.authenticated_readiness(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            manifest.egress_proxy.as_ref(),
            &manifest.spec.egress,
            None,
        )
    }

    fn host_managed_attachment_readiness(
        &self,
        manifest: &KrunSandboxManifest,
        pep: EgressReadinessState,
    ) -> Result<OciAttachmentReadinessState> {
        let network_config = manifest.require_network_config()?;
        let ports = self.port_lease_coordinator();
        let hostname = hostname_for(&manifest.spec);
        Ok(self
            .attachment_adapter(manifest, network_config, &hostname)
            .inspect_host_managed_readiness(
                &self.attachment_lifecycle(&ports),
                self.egress_pin_provider.as_ref(),
                manifest.egress_proxy.as_ref(),
                pep,
            ))
    }

    pub(super) fn ensure_egress_proxy_running_with_release_authority(
        &self,
        manifest: &KrunSandboxManifest,
        release_authority: crate::backends::oci::egress::PepPreAdoptionReleaseAuthority<'_>,
    ) -> Result<()> {
        crate::backends::oci::egress::ensure_egress_proxy_running_with_release_authority(
            &self.egress_proxies,
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            manifest.egress_proxy.as_ref(),
            &manifest.spec.egress,
            release_authority,
        )
    }

    /// Stop the egress PEP, tear the sandbox network down, and remove the netns,
    /// reusing the container backend's shared teardown free-functions plus the
    /// shared `EgressProxyRegistry::stop_with_assignment`. Errors are collected
    /// so a single failing step never short-circuits the rest of the cleanup.
    pub(super) fn release_network_artifacts(
        &self,
        manifest: &KrunSandboxManifest,
        mode: NetworkArtifactTeardownMode,
    ) -> Result<()> {
        if !manifest.permits_provider_teardown() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun workload {} cannot run provider teardown from launch authority {:?}",
                    manifest.handle.id, manifest.launch_authority
                ),
            });
        }
        if !manifest.creator_handoff.authorizes_provider_cleanup() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun workload {} cannot release provider or network authority while creator \
                     handoff {:?} may still materialize effects",
                    manifest.handle.id, manifest.creator_handoff
                ),
            });
        }

        let network_config = manifest.require_network_config()?;
        let ports = self.port_lease_coordinator();
        let hostname = hostname_for(&manifest.spec);
        let lifecycle = self.attachment_lifecycle(&ports);
        self.attachment_adapter(manifest, network_config, &hostname)
            .detach_host_managed(&lifecycle, mode, |auxiliary| match auxiliary {
                AttachmentAuxiliaryDisposition::ProviderOwned => {
                    if mode.releases_authority() {
                        self.egress_proxies.stop_with_assignment(
                            &manifest.spec.tenant_id,
                            &manifest.handle.id,
                            manifest.egress_proxy.as_ref(),
                        )
                    } else {
                        self.egress_proxies.stop_for_restart(
                            &manifest.spec.tenant_id,
                            &manifest.handle.id,
                            manifest.egress_proxy.as_ref(),
                        )
                    }
                }
                AttachmentAuxiliaryDisposition::NoEffect
                | AttachmentAuxiliaryDisposition::Unknown => Ok(()),
            })
            .map_err(Into::into)
    }

    #[cfg(test)]
    pub(super) fn reset_runtime_for_restart(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        self.delete_runtime_and_confirm_absent(manifest)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to reset krun sandbox {} for restart before provider detach: {error}",
                    manifest.handle.id
                ),
            })?;
        self.release_network_artifacts(manifest, NetworkArtifactTeardownMode::Restart)?;
        remove_if_exists(&manifest.conmon_layout.pidfile)?;
        remove_if_exists(&manifest.conmon_layout.conmon_pidfile)?;
        // The exit receipt is the durable restart checkpoint. Consume it only
        // after runtime absence and every other stale artifact are confirmed,
        // so a same-process retry cannot lose restart eligibility.
        remove_if_exists(&manifest.conmon_layout.exit_status_file)?;
        Ok(())
    }

    pub(super) fn read_manifest(&self, id: &SandboxId) -> Result<Option<KrunSandboxManifest>> {
        let Some(manifest_path) = crate::artifact_paths::manifest_path_for_sandbox_id(
            &self.config.workload_state_root,
            id,
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to find krun sandbox manifest for {} under {}: {error}",
                id,
                self.config.workload_state_root.display()
            ),
        })?
        else {
            return Ok(None);
        };
        if !manifest_path.exists() {
            return Ok(None);
        }

        let contents =
            std::fs::read(&manifest_path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read sandbox manifest {}: {error}",
                    manifest_path.display()
                ),
            })?;
        let manifest: KrunSandboxManifest =
            serde_json::from_slice(&contents).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse sandbox manifest {}: {error}",
                    manifest_path.display()
                ),
            })?;
        self.validate_manifest_roots(id, &manifest)?;
        Ok(Some(manifest))
    }

    pub(super) fn persist_effect_barrier(
        &self,
        manifest: &KrunSandboxManifest,
        operation: &str,
    ) -> Result<()> {
        #[cfg(test)]
        let write = match self
            .effect_barrier_test_probe
            .as_ref()
            .and_then(|probe| probe.claim(operation))
        {
            Some(KrunEffectBarrierFailureStage::BeforeWrite) => {
                Err(SandboxError::OperationFailed {
                    message: format!("injected {operation} failure before manifest publication"),
                })
            }
            Some(KrunEffectBarrierFailureStage::AfterRenameBeforeParentSync) => self
                .write_manifest_with_post_rename_hook(manifest, || {
                    Err(SandboxError::OperationFailed {
                        message: format!(
                            "injected {operation} acknowledgement loss after manifest rename and \
                             before parent-directory sync"
                        ),
                    })
                }),
            None => self.write_manifest(manifest),
        };
        #[cfg(not(test))]
        let write = self.write_manifest(manifest);

        let publication = match write {
            Ok(()) => Ok(()),
            Err(primary) => match self.read_manifest(&manifest.handle.id) {
                Ok(Some(observed)) if observed == *manifest => self
                    .sync_manifest_parent(manifest)
                    .map_err(|retry| SandboxError::OperationFailed {
                        message: format!(
                            "{operation} became observable for {} but durability remains \
                             ambiguous: {primary}; parent-directory sync retry failed: {retry}",
                            manifest.handle.id
                        ),
                    }),
                Ok(Some(_)) => Err(SandboxError::OperationFailed {
                    message: format!(
                        "{operation} was not durably observable for {} after write failure: \
                         {primary}; refusing subsequent effects",
                        manifest.handle.id
                    ),
                }),
                Ok(None) => Err(SandboxError::OperationFailed {
                    message: format!(
                        "{operation} lost the durable manifest for {} after write failure: \
                         {primary}; refusing subsequent effects",
                        manifest.handle.id
                    ),
                }),
                Err(observe) => Err(SandboxError::OperationFailed {
                    message: format!(
                        "{operation} could not be confirmed for {} after write failure: \
                         {primary}; readback failed: {observe}; refusing subsequent effects",
                        manifest.handle.id
                    ),
                }),
            },
        };
        publication?;
        self.retire_terminal_ipam_after_publication(manifest, operation)
    }

    pub(super) fn sync_manifest_parent(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        std::fs::File::open(&manifest.conmon_layout.container_state_dir)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to sync krun manifest directory {}: {error}",
                    manifest.conmon_layout.container_state_dir.display()
                ),
            })
    }

    pub(super) fn write_manifest(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        self.write_manifest_with_post_rename_hook(manifest, || Ok(()))
    }

    fn write_manifest_with_post_rename_hook(
        &self,
        manifest: &KrunSandboxManifest,
        after_rename: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        if matches!(
            manifest.status,
            SandboxStatus::Stopped | SandboxStatus::Failed
        ) {
            if !manifest.has_terminal_network_finality() {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "refusing terminal krun manifest publication for {} while local launch or \
                         cleanup authority remains: shutdown_requested={}, status={:?}, \
                         handle_status={:?}, launch_authority={:?}, creator_handoff={:?}, \
                         provider_failure_cleanup={:?}, launch_artifact_present={}, \
                         next_restart_at_millis={:?}",
                        manifest.handle.id,
                        manifest.shutdown_requested,
                        manifest.status,
                        manifest.handle.status,
                        manifest.launch_authority,
                        manifest.creator_handoff,
                        manifest.provider_failure_cleanup,
                        manifest.launch_artifact.is_some(),
                        manifest.next_restart_at_millis,
                    ),
                });
            }
            TerminalNetworkAuthoritySet::new(
                self.segment_allocator.as_ref(),
                &self.ipam_authority,
                self.port_lease_coordinator.authority()?,
                TerminalNetworkFinalityEvidence::new(
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                    &manifest.network_layout,
                    manifest.network_config.as_ref(),
                    &manifest.port_leases,
                    manifest
                        .egress_proxy
                        .as_ref()
                        .map(|assignment| &assignment.port_lease),
                ),
            )
            .require_released()?;
        }
        std::fs::create_dir_all(&manifest.conmon_layout.container_state_dir).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to create manifest directory {}: {error}",
                    manifest.conmon_layout.container_state_dir.display()
                ),
            }
        })?;
        let mut rendered =
            serde_json::to_vec_pretty(manifest).map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to serialize sandbox manifest: {error}"),
            })?;
        rendered.push(b'\n');
        let staged_path = manifest.conmon_layout.container_state_dir.join(format!(
            ".nimbus-krun-manifest.{}.stage",
            Ulid::new().to_string().to_ascii_lowercase()
        ));
        let publish = (|| -> Result<()> {
            let mut staged = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staged_path)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to create staged krun manifest {}: {error}",
                        staged_path.display()
                    ),
                })?;
            staged
                .write_all(&rendered)
                .and_then(|()| staged.sync_all())
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to durably stage krun manifest {}: {error}",
                        staged_path.display()
                    ),
                })?;
            std::fs::rename(&staged_path, &manifest.conmon_layout.manifest_path).map_err(
                |error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to atomically publish krun manifest {}: {error}",
                        manifest.conmon_layout.manifest_path.display()
                    ),
                },
            )?;
            after_rename()?;
            std::fs::File::open(&manifest.conmon_layout.container_state_dir)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to durably publish krun manifest {}: {error}",
                        manifest.conmon_layout.manifest_path.display()
                    ),
                })
        })();
        if publish.is_err() {
            let _ = std::fs::remove_file(&staged_path);
        }
        publish.map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "krun manifest write failed for {}: {error}",
                manifest.handle.id
            ),
        })
    }

    fn retire_terminal_ipam_after_publication(
        &self,
        manifest: &KrunSandboxManifest,
        operation: &str,
    ) -> Result<()> {
        if manifest.has_terminal_network_finality()
            && let Some(network_config) = manifest.network_config.as_ref()
        {
            #[cfg(test)]
            if let Some(message) = self.terminal_ipam_retirement_failure.as_ref() {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "{operation} manifest for {} is durable, but exact IPAM witness retirement \
                         remains pending: {message}",
                        manifest.handle.id
                    ),
                });
            }
            retire_terminal_container_ipam_release(
                &self.ipam_authority,
                &manifest.network_layout,
                &manifest.handle.id,
                &network_config.attachment_id,
                &network_config.reservation_claim,
                network_config.provider_kind(),
            )
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "{operation} manifest for {} is durable, but exact IPAM witness retirement \
                     remains pending: {error}",
                    manifest.handle.id
                ),
            })?;
        }
        Ok(())
    }
}
