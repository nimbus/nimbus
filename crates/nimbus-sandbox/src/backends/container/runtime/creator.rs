//! Durable container creator-attempt orchestration and recovery.

use crate::backends::conmon::creator::{
    CreatorAttemptReceipt, CreatorContainmentObservation, CreatorQuiescenceProof,
    OwnedConmonCreator, confirm_dead_conmon_receipt, observe_creator_containment,
    publish_creator_attempt_annotation,
};
use crate::backends::conmon::lifecycle::{
    RuntimeStateObservation, runtime_state_for_creator_attempt,
    wait_for_runtime_state_for_creator_attempt,
};
use ulid::Ulid;

use super::*;

impl ContainerSandboxBackend {
    /// Authenticate that the exact source creator can no longer materialize
    /// provider effects for a coordinator-issued restart.
    ///
    /// This does not inspect or stop the runtime and does not remove the
    /// conmon receipt. The restart phase owns those later checks and persists
    /// the returned proof before it advances the workload execution attempt.
    pub(super) fn authenticate_restart_creator_quiescence(
        &self,
        manifest: &mut ContainerSandboxManifest,
    ) -> Result<CreatorQuiescenceProof> {
        self.reconcile_pending_creator_before_cleanup(manifest)?;
        match &manifest.creator_handoff {
            ContainerCreatorHandoffState::NotSpawned => Ok(CreatorQuiescenceProof::never_spawned(
                format!("restart-source:{}", manifest.execution_attempt_id),
            )),
            ContainerCreatorHandoffState::Quiesced { proof } => Ok(proof.clone()),
            ContainerCreatorHandoffState::RuntimeObserved { receipt } => {
                match observe_creator_containment(receipt) {
                    CreatorContainmentObservation::DeadContained => {
                        Ok(CreatorQuiescenceProof::dead_contained(receipt.clone()))
                    }
                    CreatorContainmentObservation::Live => Err(SandboxError::OperationFailed {
                        message: format!(
                            "container creator attempt {} remains live; restart source quiescence remains fenced",
                            receipt.attempt_id()
                        ),
                    }),
                    CreatorContainmentObservation::Escaped { reason } => {
                        Err(SandboxError::OperationFailed {
                            message: format!(
                                "container creator attempt {} escaped its authenticated containment: {reason}; restart source quiescence remains fenced",
                                receipt.attempt_id()
                            ),
                        })
                    }
                    CreatorContainmentObservation::Unknown { reason } => {
                        Err(SandboxError::OperationFailed {
                            message: format!(
                                "container creator attempt {} cannot be authenticated: {reason}; restart source quiescence remains fenced",
                                receipt.attempt_id()
                            ),
                        })
                    }
                }
            }
            ContainerCreatorHandoffState::SpawnIntent { .. }
            | ContainerCreatorHandoffState::Pending { .. } => Err(SandboxError::OperationFailed {
                message: format!(
                    "container creator handoff for {} remained pending after restart reconciliation",
                    manifest.handle.id
                ),
            }),
        }
    }

    pub(super) fn spawn_creator_and_wait_for_runtime(
        &self,
        manifest: &mut ContainerSandboxManifest,
    ) -> Result<String> {
        manifest.require_execution_admission_open("container creator spawn")?;
        let attempt_id = Ulid::new().to_string().to_ascii_lowercase();
        self.persist_creator_intent_before_spawn(manifest, &attempt_id)?;
        if let Err(error) =
            publish_creator_attempt_annotation(&manifest.bundle_layout.config_path, &attempt_id)
        {
            let persistence = self
                .persist_creator_quiescence(
                    manifest,
                    CreatorQuiescenceProof::never_spawned(attempt_id),
                )
                .err();
            return Err(combine_launch_failure(error, None, persistence));
        }
        let mut creator = match OwnedConmonCreator::spawn_gated_with_pid_receipt(
            &manifest.conmon_launch.create_command,
            &manifest.conmon_layout.conmon_pidfile,
        ) {
            Ok(creator) => creator,
            Err(error) => {
                let persistence = self
                    .persist_creator_quiescence(
                        manifest,
                        CreatorQuiescenceProof::never_spawned(attempt_id),
                    )
                    .err();
                return Err(combine_launch_failure(error, None, persistence));
            }
        };
        let receipt = match creator.attempt_receipt(&attempt_id) {
            Ok(receipt) => receipt,
            Err(error) => {
                let quiescence = creator.cancel_before_gate_release_and_confirm_quiesced();
                let persistence = quiescence.as_ref().ok().and_then(|()| {
                    self.persist_creator_quiescence(
                        manifest,
                        CreatorQuiescenceProof::launch_gate_never_released(attempt_id.clone()),
                    )
                    .err()
                });
                return Err(combine_launch_failure(error, quiescence.err(), persistence));
            }
        };
        if let Err(error) = self.persist_pending_creator_receipt(manifest, &receipt) {
            let quiescence = creator.cancel_before_gate_release_and_confirm_quiesced();
            let persistence = quiescence.as_ref().ok().and_then(|()| {
                self.persist_creator_quiescence(
                    manifest,
                    CreatorQuiescenceProof::launch_gate_never_released(attempt_id.clone()),
                )
                .err()
            });
            return Err(combine_launch_failure(error, quiescence.err(), persistence));
        }
        if let Err(error) = manifest.require_execution_admission_open("container creator release") {
            let quiescence = creator.cancel_before_gate_release_and_confirm_quiesced();
            let persistence = quiescence.as_ref().ok().and_then(|()| {
                self.persist_creator_quiescence(
                    manifest,
                    CreatorQuiescenceProof::launch_gate_never_released(attempt_id.clone()),
                )
                .err()
            });
            return Err(combine_launch_failure(error, quiescence.err(), persistence));
        }
        if let Err(error) = creator.release_after_receipt_persisted() {
            let quiescence = creator.cancel_before_gate_release_and_confirm_quiesced();
            let persistence = quiescence.as_ref().ok().and_then(|()| {
                self.persist_creator_quiescence(
                    manifest,
                    CreatorQuiescenceProof::launch_gate_never_released(attempt_id.clone()),
                )
                .err()
            });
            return Err(combine_launch_failure(error, quiescence.err(), persistence));
        }
        let runtime_state = match wait_for_runtime_state_for_creator_attempt(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
            &attempt_id,
            self.config.start_timeout,
        ) {
            Ok(runtime_state) => runtime_state,
            Err(error) => {
                let quiescence = creator.cancel_and_confirm_quiesced();
                let persistence = quiescence.as_ref().ok().and_then(|()| {
                    self.persist_creator_quiescence(
                        manifest,
                        CreatorQuiescenceProof::dead_contained(receipt.clone()),
                    )
                    .err()
                });
                return Err(combine_launch_failure(error, quiescence.err(), persistence));
            }
        };
        if let Err(error) = creator.reap_after_runtime_observed(self.config.start_timeout) {
            let quiescence = creator.cancel_and_confirm_quiesced();
            let persistence = quiescence.as_ref().ok().and_then(|()| {
                self.persist_creator_quiescence(
                    manifest,
                    CreatorQuiescenceProof::dead_contained(receipt.clone()),
                )
                .err()
            });
            return Err(combine_launch_failure(error, quiescence.err(), persistence));
        }
        manifest.creator_handoff = ContainerCreatorHandoffState::RuntimeObserved { receipt };
        Ok(runtime_state)
    }

    /// Reconcile only a durably identified creator attempt before a fresh
    /// process may run provider cleanup. Provider/runtime state remains local
    /// to this adapter.
    pub(super) fn reconcile_pending_creator_before_cleanup(
        &self,
        manifest: &mut ContainerSandboxManifest,
    ) -> Result<()> {
        let receipt = match &manifest.creator_handoff {
            ContainerCreatorHandoffState::NotSpawned
            | ContainerCreatorHandoffState::Quiesced { .. }
            | ContainerCreatorHandoffState::RuntimeObserved { .. } => return Ok(()),
            ContainerCreatorHandoffState::SpawnIntent { attempt_id } => {
                return self.persist_creator_quiescence(
                    manifest,
                    CreatorQuiescenceProof::launch_gate_never_released(attempt_id.clone()),
                );
            }
            ContainerCreatorHandoffState::Pending { receipt } => receipt.clone(),
        };

        match observe_creator_containment(&receipt) {
            CreatorContainmentObservation::Live => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container creator attempt {} remains live with its exact process birth; \
                         cleanup remains fenced",
                        receipt.attempt_id()
                    ),
                });
            }
            CreatorContainmentObservation::Escaped { reason } => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container creator attempt {} escaped its authenticated containment: \
                         {reason}; cleanup remains fenced",
                        receipt.attempt_id()
                    ),
                });
            }
            CreatorContainmentObservation::Unknown { reason } => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container creator attempt {} cannot be authenticated: {reason}; cleanup \
                         remains fenced",
                        receipt.attempt_id()
                    ),
                });
            }
            CreatorContainmentObservation::DeadContained => {}
        }

        match runtime_state_for_creator_attempt(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
            receipt.attempt_id(),
        )? {
            RuntimeStateObservation::Present(_) => self.persist_creator_state(
                manifest,
                ContainerCreatorHandoffState::RuntimeObserved { receipt },
                "fresh-process runtime-observed creator handoff",
            ),
            RuntimeStateObservation::ExplicitlyAbsent => {
                confirm_dead_conmon_receipt(&manifest.conmon_layout.conmon_pidfile)?;
                self.persist_creator_state(
                    manifest,
                    ContainerCreatorHandoffState::Quiesced {
                        proof: CreatorQuiescenceProof::dead_contained(receipt),
                    },
                    "fresh-process quiesced creator handoff",
                )
            }
        }
    }

    fn persist_creator_intent_before_spawn(
        &self,
        manifest: &mut ContainerSandboxManifest,
        attempt_id: &str,
    ) -> Result<()> {
        persist_creator_intent_before_spawn_with(
            manifest,
            attempt_id,
            |candidate| self.write_manifest(candidate),
            |id| self.read_manifest(id),
        )
    }

    #[cfg(test)]
    pub(super) fn persist_creator_intent_before_spawn_for_test(
        &self,
        manifest: &mut ContainerSandboxManifest,
        attempt_id: &str,
        persist: impl FnMut(&ContainerSandboxManifest) -> Result<()>,
        inspect: impl FnMut(&SandboxId) -> Result<Option<ContainerSandboxManifest>>,
    ) -> Result<()> {
        persist_creator_intent_before_spawn_with(manifest, attempt_id, persist, inspect)
    }

    fn persist_pending_creator_receipt(
        &self,
        manifest: &mut ContainerSandboxManifest,
        receipt: &CreatorAttemptReceipt,
    ) -> Result<()> {
        let candidate = ContainerCreatorHandoffState::Pending {
            receipt: receipt.clone(),
        };
        self.persist_creator_state(
            manifest,
            candidate,
            "container creator birth/containment receipt",
        )
    }

    pub(super) fn persist_creator_quiescence(
        &self,
        manifest: &mut ContainerSandboxManifest,
        proof: CreatorQuiescenceProof,
    ) -> Result<()> {
        self.persist_creator_state(
            manifest,
            ContainerCreatorHandoffState::Quiesced { proof },
            "container creator quiescence",
        )
    }

    fn persist_creator_state(
        &self,
        manifest: &mut ContainerSandboxManifest,
        state: ContainerCreatorHandoffState,
        context: &str,
    ) -> Result<()> {
        manifest.creator_handoff = state;
        let candidate = manifest.clone();
        let Err(persist_error) = self.write_manifest(manifest) else {
            return Ok(());
        };
        match self.read_manifest(&manifest.handle.id) {
            Ok(Some(observed)) if observed == candidate => {
                *manifest = observed;
                Ok(())
            }
            Ok(Some(observed)) => {
                *manifest = observed;
                Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to publish {context}: {persist_error}; exact readback differs from \
                         the candidate and retains its own authority"
                    ),
                })
            }
            Ok(None) => Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to publish {context}: {persist_error}; canonical manifest disappeared \
                     during readback"
                ),
            }),
            Err(inspect_error) => Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to publish {context}: {persist_error}; canonical readback also failed: \
                     {inspect_error}"
                ),
            }),
        }
    }
}

fn persist_creator_intent_before_spawn_with(
    manifest: &mut ContainerSandboxManifest,
    attempt_id: &str,
    mut persist: impl FnMut(&ContainerSandboxManifest) -> Result<()>,
    mut inspect: impl FnMut(&SandboxId) -> Result<Option<ContainerSandboxManifest>>,
) -> Result<()> {
    manifest.creator_handoff = ContainerCreatorHandoffState::SpawnIntent {
        attempt_id: attempt_id.to_owned(),
    };
    let Err(intent_error) = persist(manifest) else {
        return Ok(());
    };

    // The provider command has not been spawned. Resolve both a definite
    // pre-commit failure and a lost post-rename acknowledgement by publishing
    // the exact attempt as quiesced before launch compensation may proceed.
    manifest.creator_handoff = ContainerCreatorHandoffState::Quiesced {
        proof: CreatorQuiescenceProof::never_spawned(attempt_id),
    };
    let quiesced_candidate = manifest.clone();
    let Err(quiescence_error) = persist(manifest) else {
        return Err(intent_error);
    };

    match inspect(&manifest.handle.id) {
        Ok(Some(observed)) if observed == quiesced_candidate => {
            *manifest = observed;
            Err(intent_error)
        }
        Ok(Some(observed)) => {
            *manifest = observed;
            Err(combine_launch_failure(
                intent_error,
                None,
                Some(quiescence_error),
            ))
        }
        Ok(None) => {
            manifest.creator_handoff = ContainerCreatorHandoffState::SpawnIntent {
                attempt_id: attempt_id.to_owned(),
            };
            Err(combine_launch_failure(
                intent_error,
                None,
                Some(quiescence_error),
            ))
        }
        Err(inspect_error) => {
            manifest.creator_handoff = ContainerCreatorHandoffState::SpawnIntent {
                attempt_id: attempt_id.to_owned(),
            };
            Err(combine_launch_failure(
                intent_error,
                None,
                Some(SandboxError::OperationFailed {
                    message: format!(
                        "{quiescence_error}; creator-quiescence manifest readback also failed: \
                         {inspect_error}"
                    ),
                }),
            ))
        }
    }
}
