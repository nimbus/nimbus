use super::readiness::{running_status, synchronize_handle_status};
use super::start::{ensure_guest_user_helper_available, hostname_for};
use super::*;
use crate::backends::conmon::lifecycle::{DetectedRuntimeStatus, RuntimeStateObservation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NetworkArtifactTeardownMode {
    Restart,
    Final,
}

const KRUN_LIFECYCLE_LOCK_FILE: &str = ".nimbus-krun-lifecycle.lock";
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

impl NetworkArtifactTeardownMode {
    fn releases_authority(self) -> bool {
        self == Self::Final
    }
}

impl KrunSandboxBackend {
    pub(super) fn inspect_sync(&self, id: &SandboxId) -> Result<Option<SandboxHandle>> {
        let Some(observed) = self.read_manifest(id)? else {
            return Ok(None);
        };
        let _lifecycle = self.lock_launch_lifecycle(&observed)?;
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Ok(None);
        };

        if self.config.start_mode == KrunStartMode::Execute
            && manifest.shutdown_requested
            && manifest.status == SandboxStatus::Stopping
            && manifest.launch_authority != KrunLaunchAuthority::Released
        {
            // NNC3.4 deliberately leaves recovery of nonterminal launch
            // authority to NNC3.8. Inspection is only an observed projection:
            // it must not manufacture terminal status while exact provider or
            // reservation cleanup is still fenced.
            return Ok(Some(manifest.handle));
        }

        if self.startup_network_reconciliation_error.is_some() {
            if self.config.start_mode == KrunStartMode::PlanOnly
                || !manifest.conmon_layout.exit_status_file.exists()
            {
                return Ok(Some(manifest.handle));
            }
            let exit_code = read_exit_code(&manifest.conmon_layout.exit_status_file)?;
            if !manifest.shutdown_requested
                && restart_policy_allows_restart(
                    manifest.spec.lifecycle.restart_policy,
                    exit_code,
                    manifest.restart_count,
                )
            {
                // A retained startup failure fences provider relaunch, not
                // observation or exact final cleanup of an existing workload.
                return Ok(Some(manifest.handle));
            }
        }
        let restarted = self.config.start_mode == KrunStartMode::Execute
            && self.maybe_restart_after_exit(&mut manifest)?;
        let runtime_observation = match self.config.start_mode {
            KrunStartMode::PlanOnly => None,
            KrunStartMode::Execute if restarted => None,
            KrunStartMode::Execute => Some(self.observe_runtime_status(&manifest)?),
        };
        let terminal_status = match self.config.start_mode {
            KrunStartMode::PlanOnly => manifest.status,
            KrunStartMode::Execute if restarted => manifest.status,
            KrunStartMode::Execute => {
                runtime_observation
                    .expect("execute observation is present when restart did not occur")
                    .status
            }
        };
        if self.config.start_mode == KrunStartMode::Execute
            && !restarted
            && !manifest.shutdown_requested
            && !manifest.conmon_layout.exit_status_file.exists()
            && runtime_observation.is_some_and(|observation| observation.explicitly_absent)
            && matches!(
                manifest.launch_authority,
                KrunLaunchAuthority::Adopted { .. } | KrunLaunchAuthority::ProviderOwned
            )
        {
            // Provider absence without an exit receipt does not prove whether
            // restart or final cleanup is desired. Withdraw the observed
            // projection and retain exact authority for NNC3.8 reconciliation.
            synchronize_handle_status(&mut manifest, SandboxStatus::Stopping);
            self.write_manifest(&manifest)?;
            return Ok(Some(manifest.handle));
        }
        if self.config.start_mode == KrunStartMode::Execute
            && !restarted
            && !manifest.shutdown_requested
            && manifest.conmon_layout.exit_status_file.exists()
        {
            self.finalize_natural_exit(&mut manifest, terminal_status)?;
        } else {
            synchronize_handle_status(&mut manifest, terminal_status);
        }
        self.persist_effect_barrier(&manifest, "krun observed-state publication")?;
        Ok(Some(manifest.handle))
    }

    /// Converge provider and durable network authority before publishing a
    /// naturally exited VMM as terminal. A cleanup failure leaves a durable
    /// `Stopping` checkpoint so the exact tenant/workload authority can retry.
    fn finalize_natural_exit(
        &self,
        manifest: &mut KrunSandboxManifest,
        terminal_status: SandboxStatus,
    ) -> Result<()> {
        manifest.last_exit_code = Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
        manifest.next_restart_at_millis = None;
        if manifest.launch_authority == KrunLaunchAuthority::Released {
            // A prior inspection may have durably published terminal authority
            // before post-publication IPAM witness retirement succeeded. Do not
            // replay provider cleanup from Released; republish the exact
            // terminal projection so the outer effect barrier can idempotently
            // retry witness retirement.
            synchronize_handle_status(manifest, terminal_status);
            return Ok(());
        }
        manifest.shutdown_requested = true;
        synchronize_handle_status(manifest, SandboxStatus::Stopping);
        self.persist_effect_barrier(manifest, "natural-exit cleanup intent")?;
        self.release_network_artifacts(manifest, NetworkArtifactTeardownMode::Final)?;
        self.cleanup_manifest_launch_artifacts(manifest)?;
        manifest.launch_artifact = None;
        manifest.launch_authority = KrunLaunchAuthority::Released;
        synchronize_handle_status(manifest, terminal_status);
        Ok(())
    }

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

    pub(super) fn execute_start(&self, launch_plan: &KrunStartPlan) -> Result<SandboxHandle> {
        let preflight = ensure_linux_host("krun")
            .and_then(|()| ensure_guest_user_helper_available(&self.config, &launch_plan.manifest));
        self.execute_start_after_preflight(launch_plan, preflight)
    }

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

    pub(super) fn lock_launch_lifecycle_for(
        &self,
        tenant: &nimbus_core::TenantId,
        sandbox_id: &SandboxId,
    ) -> Result<KrunLifecycleGuard> {
        let layout = OciConmonLayout::new_for_tenant(&self.config.state_root, tenant, sandbox_id);
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
                KrunLaunchAuthority::Reserved { .. }
            )
        {
            return Ok(());
        }
        Err(SandboxError::OperationFailed {
            message: format!(
                "krun launch {} no longer owns the durable reserved launch plan; \
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
        let network_released = if artifact_released {
            let compensation = match reservations {
                Some(reservations) => {
                    self.release_unpublished_reserved_launch(manifest, reservations)
                }
                None => self.release_reserved_launch(manifest),
            };
            match compensation {
                Ok(()) => true,
                Err(error) => {
                    secondary.push(format!(
                        "exact krun launch reservation compensation failed: {error}"
                    ));
                    false
                }
            }
        } else {
            false
        };
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
            || Ok(running_status(manifest)),
        )
    }

    fn observe_runtime_status(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<DetectedRuntimeStatus> {
        crate::backends::conmon::lifecycle::observe_runtime_status(
            RuntimeStatusProbe {
                exit_status_file: &manifest.conmon_layout.exit_status_file,
                state_command: &manifest.conmon_launch.state_command,
                runtime_id: manifest.handle.id.as_str(),
                pidfile: &manifest.conmon_layout.pidfile,
                shutdown_requested: manifest.shutdown_requested,
                current_status: manifest.status,
            },
            || Ok(running_status(manifest)),
        )
    }

    fn maybe_restart_after_exit(&self, manifest: &mut KrunSandboxManifest) -> Result<bool> {
        if manifest.shutdown_requested || !manifest.conmon_layout.exit_status_file.exists() {
            return Ok(false);
        }

        let exit_code = read_exit_code(&manifest.conmon_layout.exit_status_file)?;
        if !restart_policy_allows_restart(
            manifest.spec.lifecycle.restart_policy,
            exit_code,
            manifest.restart_count,
        ) {
            return Ok(false);
        }

        manifest.last_exit_code = Some(exit_code);
        let now_millis = nimbus_core::clock::system_now_millis();
        let next_restart_at_millis = manifest.next_restart_at_millis.get_or_insert_with(|| {
            now_millis.saturating_add(restart_backoff_delay(manifest.restart_count).as_millis() as u64)
        });
        if now_millis < *next_restart_at_millis {
            synchronize_handle_status(manifest, SandboxStatus::Starting);
            return Ok(true);
        }

        manifest.restart_count += 1;
        manifest.next_restart_at_millis = None;
        self.reset_runtime_for_restart(manifest)?;
        self.launch_manifest(manifest, false)?;
        Ok(true)
    }

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
            if let Err(error) = self.segment_allocator.adopt_reserved_attachment(
                &manifest.spec.tenant_id,
                &default_network_attachment_id(&manifest.handle.id),
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

        let provider_launch = ensure_linux_host("krun")
            .and_then(|()| ensure_guest_user_helper_available(&self.config, manifest))
            .and_then(|()| self.configure_network(manifest))
            .and_then(|()| {
                self.launch_into_network(manifest, clear_last_exit_code, reservation_claim.as_ref())
            });
        if let Err(error) = provider_launch {
            return Err(self.persist_provider_launch_failure(manifest, error));
        }
        Ok(())
    }

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

    fn delete_runtime_and_confirm_absent(&self, manifest: &KrunSandboxManifest) -> Result<()> {
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
    pub(super) fn configure_network(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        // Validate attachment authority before any provider or filesystem
        // effect. PlanOnly manifests carry `None` and can never reach Netavark.
        let network_config = manifest.require_network_config()?.clone();
        authenticate_container_network_generation(
            &manifest.network_layout,
            &network_config,
            &manifest.handle.id,
        )?;
        // One-shot: drop the legacy shared nimbus0 bridge before the first
        // per-tenant setup (pre-launch migration, breaking).
        purge_legacy_nimbus0_once(&self.config.state_root.join("networks"))?;
        let port_lease_coordinator = self.port_lease_coordinator();
        port_lease_coordinator.require_binding_leases(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )?;
        // Reuse the config resolved + persisted at manifest-prepare; never
        // reassign it so setup and teardown agree on the bridge.
        create_persistent_network_namespace(&manifest.network_layout.netns_path)?;
        let mut netavark_lifetimes = match port_lease_coordinator
            .claim_netavark_bindings_with_lifetimes(
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &manifest.spec.port_bindings,
                &manifest.port_leases,
            ) {
            Ok(batch) => Some(batch),
            Err(error) => {
                let _ = remove_persistent_network_namespace(&manifest.network_layout.netns_path);
                return Err(error);
            }
        };
        if let Err(error) = setup_container_network(
            &manifest.network_layout,
            &network_config,
            &manifest.handle.id,
            manifest.spec.display_name(),
            &hostname_for(&manifest.spec),
            &manifest.spec.port_bindings,
            None,
        ) {
            return Err(self.failed_netavark_configuration(
                manifest,
                &network_config,
                netavark_lifetimes
                    .take()
                    .expect("Krun Netavark setup retains its lifetime batch"),
                error,
            ));
        }
        if let Err(error) = port_lease_coordinator.activate_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
            netavark_lifetimes
                .as_ref()
                .expect("Krun Netavark activation retains its lifetime batch"),
        ) {
            return Err(self.failed_netavark_configuration(
                manifest,
                &network_config,
                netavark_lifetimes
                    .take()
                    .expect("Krun Netavark activation retains its lifetime batch"),
                error,
            ));
        }
        // Pin the netns so the ONLY reachable egress is this sandbox's own PEP.
        // The netavark deny is route-based, but the shared bridge gateway is
        // on-link and every sibling sandbox's PEP listens on it at a distinct
        // port; without this pin an execute-mode guest could egress through a
        // sibling tenant's proxy and its injected credentials (audit H1). Under
        // libkrun TSI the guest's outbound sockets are issued by this host VMM
        // process inside the netns, so the output-hook pin governs the guest
        // exactly as it governs a container. Tear the namespace back down on
        // failure so the VMM never launches into an unpinned netns.
        if let Some(proxy) = manifest.egress_proxy.as_ref()
            && let Err(error) = pin_netns_egress_to_own_proxy(&manifest.network_layout, proxy)
        {
            return Err(self.failed_netavark_configuration(
                manifest,
                &network_config,
                netavark_lifetimes
                    .take()
                    .expect("Krun Netavark pin retains its lifetime batch"),
                error,
            ));
        }
        if let Some(batch) = netavark_lifetimes.take()
            && let Err((error, batch)) = self.netavark_port_lifetimes.insert(
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                batch,
            )
        {
            return Err(self.failed_netavark_configuration(
                manifest,
                &network_config,
                batch,
                error,
            ));
        }
        // Take the tenant's refcount hold now the netns is up and pinned; the
        // reaper frees the index + bridge when the last hold releases.
        if let Err(error) = self.segment_allocator.acquire(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
        ) {
            let batch = self
                .netavark_port_lifetimes
                .take(&manifest.spec.tenant_id, &manifest.handle.id)?
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "Krun Netavark segment-hold failure for {} lost its exact live lifetime \
                         batch",
                        manifest.handle.id
                    ),
                })?;
            return Err(self.failed_netavark_configuration(
                manifest,
                &network_config,
                batch,
                error,
            ));
        }
        Ok(())
    }

    /// Compensate one failed provider setup under its exact live lifetime.
    pub(super) fn failed_netavark_configuration(
        &self,
        manifest: &KrunSandboxManifest,
        network_config: &OciNetworkConfig,
        batch: crate::backends::oci::port_lease::OciPortBindLifetimeBatch,
        primary: SandboxError,
    ) -> SandboxError {
        let cleanup = teardown_container_network(
            &manifest.network_layout,
            network_config,
            &manifest.handle.id,
            manifest.spec.display_name(),
            &hostname_for(&manifest.spec),
            &manifest.spec.port_bindings,
            None,
        )
        .and_then(|()| remove_persistent_network_namespace(&manifest.network_layout.netns_path));
        if let Err(cleanup) = cleanup {
            return SandboxError::OperationFailed {
                message: format!(
                    "krun network configuration failed: {primary}; exact-generation detach \
                     compensation also failed while the lifetime-fenced namespace remains \
                     recoverable: {cleanup}"
                ),
            };
        }

        let port_lease_coordinator = self.port_lease_coordinator();
        let compensation = port_lease_coordinator
            .abandon_netavark_bind_claims_with_lifetimes_without_effect(
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &manifest.spec.port_bindings,
                &manifest.port_leases,
                &batch,
                manifest.reservation_claim(),
            )
            .or_else(|abandon_error| {
                let expected = port_lease_coordinator.expected_netavark_bindings(
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                    &manifest.spec.port_bindings,
                    &manifest.port_leases,
                )?;
                port_lease_coordinator
                    .prepare_netavark_bindings_for_rebind_with_lifetimes(
                        &manifest.port_leases,
                        &expected,
                        &batch,
                    )
                    .map_err(|rebind_error| SandboxError::OperationFailed {
                        message: format!(
                            "Netavark claim abandonment failed: {abandon_error}; exact Active \
                             lifetime compensation also failed: {rebind_error}"
                        ),
                    })
            });
        match compensation {
            Ok(()) => primary,
            Err(cleanup) => SandboxError::OperationFailed {
                message: format!(
                    "krun network configuration failed: {primary}; detached Netavark \
                     port-lifetime compensation also failed: {cleanup}"
                ),
            },
        }
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
    pub(super) fn ensure_execute_egress_enforced(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<()> {
        // (1) Platform: the krun execute path is a Linux build target. This is a
        // compile-time cfg check, not a /dev/kvm probe; a Linux host without KVM
        // still fails closed because crun/libkrun cannot spawn the VMM without
        // /dev/kvm. Deny on every non-Linux build.
        ensure_linux_host("krun")?;
        self.ensure_execute_egress_preconditions(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.network_layout.netns_path,
        )
    }

    /// Platform-independent half of the readiness gate: deny unless the
    /// deny-by-default netns is installed AND the egress PEP for `id` is running
    /// with an active policy generation. Split out from
    /// [`KrunSandboxBackend::ensure_execute_egress_enforced`] so the deny/permit
    /// matrix is unit-testable without a Linux host or `/dev/kvm`.
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
            Some(readiness) if !readiness.ready || readiness.policy_generation.is_none() => {
                Err(SandboxError::OperationFailed {
                    message: format!(
                        "krun sandbox {id} denied launch: egress policy-enforcement proxy is not ready (no active policy generation loaded)"
                    ),
                })
            }
            Some(_) => Ok(()),
        }
    }

    fn ensure_egress_proxy_running_with_release_authority(
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
        authenticate_container_network_generation_for_cleanup(
            &manifest.network_layout,
            network_config,
            &manifest.handle.id,
        )?;
        let mut errors = Vec::new();
        let mut detach_confirmed = true;
        let port_lease_coordinator = self.port_lease_coordinator();
        let launch_claim = manifest.reservation_claim();
        let published_batch_state = if manifest.start_mode == KrunStartMode::Execute {
            port_lease_coordinator.classify_netavark_cleanup_batch(
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &manifest.spec.port_bindings,
                &manifest.port_leases,
                launch_claim,
            )
        } else {
            Ok(LaunchPortBatchState::TerminalNoEffect)
        };
        let pep_requests = manifest
            .egress_proxy
            .as_ref()
            .map(|assignment| vec![assignment.port_lease.clone()])
            .unwrap_or_default();
        let pep_batch_state = if mode.releases_authority()
            && let Some(claim) = launch_claim
        {
            port_lease_coordinator.classify_launch_port_batch(&pep_requests, claim)
        } else {
            Ok(LaunchPortBatchState::ProviderOwned)
        };
        if let Err(error) = &published_batch_state {
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        if let Err(error) = &pep_batch_state {
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        if mode.releases_authority()
            && let Err(error) = quarantine_network_segment_hold(
                self.segment_allocator.as_ref(),
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &network_config.reservation_claim,
            )
        {
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        let mut netavark_port_cleanup = None;
        let mut netavark_claim_recoveries = None;
        if manifest.start_mode == KrunStartMode::Execute
            && matches!(
                &published_batch_state,
                Ok(LaunchPortBatchState::ProviderOwned)
            )
        {
            match port_lease_coordinator.begin_netavark_cleanup(
                &self.netavark_port_lifetimes,
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &manifest.spec.port_bindings,
                &manifest.port_leases,
            ) {
                Ok(cleanup) => netavark_port_cleanup = cleanup,
                Err(error) => {
                    detach_confirmed = false;
                    errors.push(error.to_string());
                }
            }
        }
        if manifest.start_mode == KrunStartMode::Execute
            && let Ok(LaunchPortBatchState::NetavarkClaimed(claims)) = &published_batch_state
        {
            match port_lease_coordinator.recover_netavark_claims_after_owner_death(
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &manifest.spec.port_bindings,
                &manifest.port_leases,
                claims,
            ) {
                Ok(recoveries) => netavark_claim_recoveries = Some(recoveries),
                Err(error) => {
                    detach_confirmed = false;
                    errors.push(error.to_string());
                }
            }
        }
        if matches!(&pep_batch_state, Ok(LaunchPortBatchState::ProviderOwned)) {
            let stop_pep = if mode.releases_authority() {
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
            };
            if let Err(error) = stop_pep {
                detach_confirmed = false;
                errors.push(error.to_string());
            }
        }
        let mut netavark_detach_confirmed = if detach_confirmed {
            match manifest
                .require_network_config()
                .and_then(|network_config| {
                    teardown_container_network(
                        &manifest.network_layout,
                        network_config,
                        &manifest.handle.id,
                        manifest.spec.display_name(),
                        &hostname_for(&manifest.spec),
                        &manifest.spec.port_bindings,
                        None,
                    )
                }) {
                Ok(()) => true,
                Err(error) => {
                    detach_confirmed = false;
                    errors.push(error.to_string());
                    false
                }
            }
        } else {
            false
        };
        if netavark_detach_confirmed
            && detach_confirmed
            && let Err(error) =
                remove_persistent_network_namespace(&manifest.network_layout.netns_path)
        {
            netavark_detach_confirmed = false;
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        if netavark_detach_confirmed
            && manifest.start_mode == KrunStartMode::Execute
            && matches!(
                &published_batch_state,
                Ok(LaunchPortBatchState::ProviderOwned)
            )
        {
            match port_lease_coordinator.complete_netavark_cleanup(
                &manifest.port_leases,
                netavark_port_cleanup.as_ref(),
                mode.releases_authority(),
            ) {
                Ok(()) => netavark_port_cleanup = None,
                Err(error) => {
                    detach_confirmed = false;
                    errors.push(error.to_string());
                }
            }
        }
        if netavark_detach_confirmed
            && let Some(recoveries) = netavark_claim_recoveries.take()
            && let Err(error) = if mode.releases_authority() {
                port_lease_coordinator
                    .release_recovered_netavark_bindings(&manifest.port_leases, &recoveries)
            } else {
                port_lease_coordinator.prepare_recovered_netavark_claims_for_rebind(
                    &manifest.port_leases,
                    &recoveries,
                )
            }
        {
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        if !netavark_detach_confirmed
            && let Err(error) = port_lease_coordinator.retain_ambiguous_netavark_cleanup(
                &self.netavark_port_lifetimes,
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                netavark_port_cleanup.take(),
            )
        {
            detach_confirmed = false;
            errors.push(error.to_string());
        }
        // Drop the quarantined hold only after provider and persistent-netns
        // deletion are confirmed. A failed bridge cleanup leaves the allocation
        // cleanup-pending and unavailable.
        if detach_confirmed {
            if mode.releases_authority() && manifest.start_mode == KrunStartMode::Execute {
                match published_batch_state {
                    Ok(LaunchPortBatchState::NeverBound) => {
                        if let Some(claim) = launch_claim
                            && let Err(error) = port_lease_coordinator
                                .release_never_bound_requests(&manifest.port_leases, claim)
                        {
                            detach_confirmed = false;
                            errors.push(error.to_string());
                        }
                    }
                    Ok(LaunchPortBatchState::NetavarkClaimed(_)) => {
                        // Dead-owner recovery plus exact provider absence
                        // completed the terminal release above.
                    }
                    Ok(LaunchPortBatchState::RestartRetained) => {
                        if let Err(error) = port_lease_coordinator
                            .release_restart_retained_bindings(
                                &manifest.spec.tenant_id,
                                &manifest.handle.id,
                                &manifest.spec.port_bindings,
                                &manifest.port_leases,
                            )
                        {
                            detach_confirmed = false;
                            errors.push(error.to_string());
                        }
                    }
                    Ok(LaunchPortBatchState::ProviderOwned) => {
                        // Exact Netavark provider absence already completed the
                        // lifetime-authenticated release above.
                    }
                    Ok(LaunchPortBatchState::TerminalNoEffect) => {
                        // Every exact lease already carries terminal proof that
                        // no provider effect remains. Cleanup replay has no port
                        // mutation left to perform.
                    }
                    Err(_) => {}
                }
                if detach_confirmed
                    && matches!(&pep_batch_state, Ok(LaunchPortBatchState::NeverBound))
                    && let Some(claim) = launch_claim
                    && let Err(error) =
                        port_lease_coordinator.release_never_bound_requests(&pep_requests, claim)
                {
                    detach_confirmed = false;
                    errors.push(error.to_string());
                }
                if detach_confirmed
                    && let Ok(network_config) = manifest.require_network_config()
                    && let Err(error) = deallocate_container_ips_after_confirmed_detach(
                        &manifest.network_layout,
                        &manifest.handle.id,
                        &network_config.reservation_claim,
                    )
                {
                    detach_confirmed = false;
                    errors.push(error.to_string());
                }
            }
            if mode.releases_authority() && detach_confirmed {
                errors.extend(
                    release_network_segment_hold(
                        self.segment_allocator.as_ref(),
                        &manifest.spec.tenant_id,
                        &manifest.handle.id,
                        &network_config.reservation_claim,
                    )
                    .into_iter()
                    .map(|error| error.to_string()),
                );
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to release krun sandbox {} network artifacts: {}",
                    manifest.handle.id,
                    errors.join("; ")
                ),
            })
        }
    }

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
        let Some(manifest_path) =
            crate::artifact_paths::manifest_path_for_sandbox_id(&self.config.state_root, id)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to find krun sandbox manifest for {} under {}: {error}",
                        id,
                        self.config.state_root.display()
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
        let manifest =
            serde_json::from_slice(&contents).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse sandbox manifest {}: {error}",
                    manifest_path.display()
                ),
            })?;
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
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &manifest.network_layout,
                manifest.network_config.as_ref(),
                &manifest.port_leases,
                manifest
                    .egress_proxy
                    .as_ref()
                    .map(|assignment| &assignment.port_lease),
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
                &manifest.network_layout,
                &manifest.handle.id,
                &network_config.reservation_claim,
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
