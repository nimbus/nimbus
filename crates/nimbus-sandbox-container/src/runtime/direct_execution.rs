//! Direct Execute ownership, provider-effect fencing, and launch completion.
//!
//! This module keeps the direct-start crash cuts beside their durable runner
//! decision protocol while the runtime root remains a thin composition layer.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InitialLaunchCleanupState {
    Complete,
    UnstartedPending,
    ProviderPending,
}

pub(super) struct InitialLaunchFailure {
    pub(super) error: SandboxError,
    pub(super) cleanup_state: InitialLaunchCleanupState,
    pub(super) terminal_status: SandboxStatus,
}

impl ContainerSandboxBackend {
    pub(super) fn execute_start(
        &self,
        manifest: &mut ContainerSandboxManifest,
    ) -> std::result::Result<SandboxHandle, InitialLaunchFailure> {
        self.execute_start_after_preflight_with_cleanup(manifest, ensure_linux_host("container"))
    }

    #[cfg(test)]
    pub(super) fn execute_direct_start(
        &self,
        manifest: &mut ContainerSandboxManifest,
    ) -> Result<SandboxHandle> {
        let handoff = runner::persist_direct_execution_ownership(self, manifest)?;
        #[cfg(test)]
        let preflight = match self.runner_handoff_failure {
            Some(
                RunnerHandoffFailure::DirectEffectFencePersistence
                | RunnerHandoffFailure::DirectEffectFenceAcknowledgementLoss
                | RunnerHandoffFailure::DirectAfterEffectFence,
            ) => Ok(()),
            Some(RunnerHandoffFailure::DirectTerminalManifest) => {
                Err(SandboxError::BackendUnavailable {
                    message: "injected direct pre-provider rejection".to_owned(),
                })
            }
            _ => ensure_linux_host("container"),
        };
        #[cfg(not(test))]
        let preflight = ensure_linux_host("container");
        if preflight.is_ok() {
            if let Err(error) = effect_fence::converge_persistence_with(
                effect_fence::EFFECT_FENCE_PERSIST_ATTEMPTS,
                || {
                    #[cfg(test)]
                    if self.runner_handoff_failure.is_some_and(|failure| {
                        failure == RunnerHandoffFailure::DirectEffectFencePersistence
                    }) {
                        return Err(SandboxError::OperationFailed {
                            message: "injected direct effect-fence persistence failure".to_owned(),
                        });
                    }
                    #[cfg(test)]
                    if self.runner_handoff_failure.is_some_and(|failure| {
                        failure == RunnerHandoffFailure::DirectEffectFenceAcknowledgementLoss
                    }) {
                        if runner::execute_handoff_phase(manifest)?
                            == Some(runner::RunnerHandoffPhase::ClaimedBeforeEffects)
                        {
                            runner::mark_runner_effects_started(manifest, &handoff)?;
                        }
                        return Err(SandboxError::OperationFailed {
                            message: "injected direct effect-fence acknowledgement loss".to_owned(),
                        });
                    }
                    runner::mark_runner_effects_started(manifest, &handoff)
                },
                |stage, error| {
                    tracing::warn!(
                        sandbox_id = %manifest.handle.id,
                        ?stage,
                        %error,
                        "direct container start retains its handoff lock while the effect fence converges"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(250));
                },
            ) {
                return Err(effect_fence::diagnose_exhaustion(manifest, error));
            }
            #[cfg(test)]
            if self
                .runner_handoff_failure
                .is_some_and(|failure| failure == RunnerHandoffFailure::DirectAfterEffectFence)
            {
                return Err(SandboxError::OperationFailed {
                    message: "injected direct Execute failure after durable effect fence"
                        .to_owned(),
                });
            }
        }
        let result = self.execute_start_after_preflight_with_cleanup(manifest, preflight);
        match result {
            Ok(handle) => {
                runner::record_runner_effect_outcome(
                    manifest,
                    runner::RunnerEffectOutcome::Present,
                    &handoff,
                )?;
                runner::converge_runner_lifecycle_ownership(manifest, &handoff)?;
                Ok(handle)
            }
            Err(failure) => {
                let primary = failure.error;
                if let Err(cleanup) = runner::converge_initial_launch_cleanup(
                    self,
                    manifest,
                    failure.terminal_status,
                    failure.cleanup_state,
                ) {
                    return Err(runner::preserve_runner_primary_error(
                        Some(primary),
                        "direct initial-launch cleanup did not converge",
                        cleanup,
                    ));
                }
                let phase = match runner::execute_handoff_phase(manifest) {
                    Ok(phase) => phase,
                    Err(ownership) => {
                        return Err(runner::preserve_runner_primary_error(
                            Some(primary),
                            "direct effect-result classification did not converge",
                            ownership,
                        ));
                    }
                };
                match phase {
                    Some(runner::RunnerHandoffPhase::EffectsStarted) => {
                        if let Err(receipt) = runner::record_runner_effect_outcome(
                            manifest,
                            runner::RunnerEffectOutcome::Absent,
                            &handoff,
                        ) {
                            return Err(runner::preserve_runner_primary_error(
                                Some(primary),
                                "direct effect-result publication did not converge",
                                receipt,
                            ));
                        }
                    }
                    Some(runner::RunnerHandoffPhase::ClaimedBeforeEffects) => {
                        // Preflight failed before the effect fence. The durable
                        // terminal cleanup manifest is the no-effect receipt;
                        // publishing a provider-effect receipt here would
                        // falsely claim that effects may have started.
                    }
                    unexpected => {
                        return Err(runner::preserve_runner_primary_error(
                            Some(primary),
                            "direct effect-result classification did not converge",
                            SandboxError::OperationFailed {
                                message: format!(
                                    "direct launch failure for {} reached unexpected runner \
                                     handoff phase {unexpected:?}",
                                    manifest.handle.id
                                ),
                            },
                        ));
                    }
                }
                if let Err(publication) =
                    runner::converge_runner_lifecycle_ownership(manifest, &handoff)
                {
                    return Err(runner::preserve_runner_primary_error(
                        Some(primary),
                        "direct lifecycle publication did not converge",
                        publication,
                    ));
                }
                Err(primary)
            }
        }
    }

    #[cfg(test)]
    pub(super) fn execute_start_after_preflight(
        &self,
        manifest: &mut ContainerSandboxManifest,
        preflight: Result<()>,
    ) -> Result<SandboxHandle> {
        self.execute_start_after_preflight_with_cleanup(manifest, preflight)
            .map_err(|failure| failure.error)
    }

    fn execute_start_after_preflight_with_cleanup(
        &self,
        manifest: &mut ContainerSandboxManifest,
        preflight: Result<()>,
    ) -> std::result::Result<SandboxHandle, InitialLaunchFailure> {
        if let Err(error) = preflight {
            let cleanup = self.release_unstarted_launch_artifacts(manifest).err();
            let cleanup_state = if cleanup.is_none() {
                InitialLaunchCleanupState::Complete
            } else {
                InitialLaunchCleanupState::UnstartedPending
            };
            let terminal_status = if cleanup.is_none() {
                SandboxStatus::Stopped
            } else {
                SandboxStatus::Failed
            };
            return Err(InitialLaunchFailure {
                error: self.persist_failed_initial_launch(manifest, error, cleanup),
                cleanup_state,
                terminal_status,
            });
        }
        if let Err(error) = self.launch_manifest(manifest, true) {
            let cleanup = self.release_execution_artifacts(manifest).err();
            let cleanup_state = if cleanup.is_none() {
                InitialLaunchCleanupState::Complete
            } else {
                InitialLaunchCleanupState::ProviderPending
            };
            let terminal_status = if cleanup.is_none() {
                SandboxStatus::Stopped
            } else {
                SandboxStatus::Failed
            };
            return Err(InitialLaunchFailure {
                error: self.persist_failed_initial_launch(manifest, error, cleanup),
                cleanup_state,
                terminal_status,
            });
        }
        Ok(manifest.handle.clone())
    }

    pub(super) fn persist_failed_initial_launch(
        &self,
        manifest: &mut ContainerSandboxManifest,
        primary: SandboxError,
        cleanup: Option<SandboxError>,
    ) -> SandboxError {
        manifest.shutdown_requested = true;
        manifest.last_exit_code = None;
        synchronize_handle_status(
            manifest,
            if cleanup.is_none() {
                SandboxStatus::Stopped
            } else {
                SandboxStatus::Stopping
            },
        );
        #[cfg(test)]
        let persistence = if self
            .runner_handoff_failure
            .is_some_and(|failure| failure == RunnerHandoffFailure::DirectTerminalManifest)
        {
            Some(SandboxError::OperationFailed {
                message: "injected direct terminal manifest failure".to_owned(),
            })
        } else {
            self.write_manifest(manifest).err()
        };
        #[cfg(not(test))]
        let persistence = self.write_manifest(manifest).err();
        combine_launch_failure(primary, cleanup, persistence)
    }
}
