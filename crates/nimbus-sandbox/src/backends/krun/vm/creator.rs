//! Durable krun creator-attempt orchestration and recovery.

use crate::backends::conmon::creator::{
    CreatorContainmentObservation, CreatorQuiescenceProof, OwnedConmonCreator,
    confirm_dead_conmon_receipt, observe_creator_containment, publish_creator_attempt_annotation,
};
use crate::backends::conmon::lifecycle::{
    RuntimeStateObservation, runtime_state_for_creator_attempt,
    wait_for_runtime_state_for_creator_attempt,
};

use super::*;

impl KrunSandboxBackend {
    /// Convert a runtime-observed creator receipt into durable quiescence only
    /// after the caller has independently confirmed runtime absence.
    pub(super) fn persist_restart_creator_quiescence_after_runtime_absence(
        &self,
        manifest: &mut KrunSandboxManifest,
    ) -> Result<()> {
        match manifest.creator_handoff.clone() {
            KrunCreatorHandoffState::Quiesced { .. } => Ok(()),
            KrunCreatorHandoffState::RuntimeObserved { receipt } => {
                match observe_creator_containment(&receipt) {
                    CreatorContainmentObservation::DeadContained => {}
                    CreatorContainmentObservation::Live => {
                        return Err(SandboxError::OperationFailed {
                            message: format!(
                                "krun restart creator attempt {} remains live after runtime absence; target attempt remains fenced",
                                receipt.attempt_id()
                            ),
                        });
                    }
                    CreatorContainmentObservation::Escaped { reason } => {
                        return Err(SandboxError::OperationFailed {
                            message: format!(
                                "krun restart creator attempt {} escaped its authenticated containment after runtime absence: {reason}; target attempt remains fenced",
                                receipt.attempt_id()
                            ),
                        });
                    }
                    CreatorContainmentObservation::Unknown { reason } => {
                        return Err(SandboxError::OperationFailed {
                            message: format!(
                                "krun restart creator attempt {} cannot be authenticated after runtime absence: {reason}; target attempt remains fenced",
                                receipt.attempt_id()
                            ),
                        });
                    }
                }
                confirm_dead_conmon_receipt(&manifest.conmon_layout.conmon_pidfile)?;
                self.persist_krun_creator_quiescence(
                    manifest,
                    CreatorQuiescenceProof::dead_contained(receipt),
                )
            }
            KrunCreatorHandoffState::NotSpawned => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun restart for {} cannot quiesce provider-owned execution without a creator-attempt receipt",
                    manifest.handle.id
                ),
            }),
            KrunCreatorHandoffState::SpawnIntent { .. }
            | KrunCreatorHandoffState::Pending { .. } => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun restart for {} cannot publish quiescence while creator handoff {:?} may still materialize provider effects",
                    manifest.handle.id, manifest.creator_handoff
                ),
            }),
        }
    }

    pub(super) fn spawn_creator_and_wait_for_runtime(
        &self,
        manifest: &mut KrunSandboxManifest,
    ) -> Result<String> {
        let attempt_id = Ulid::new().to_string().to_ascii_lowercase();
        self.persist_krun_creator_intent_before_spawn(manifest, &attempt_id)?;
        if let Err(error) =
            publish_creator_attempt_annotation(&manifest.bundle_layout.config_path, &attempt_id)
        {
            let persistence = self
                .persist_krun_creator_quiescence(
                    manifest,
                    CreatorQuiescenceProof::never_spawned(attempt_id),
                )
                .err();
            return Err(combine_krun_creator_failure(error, None, persistence));
        }
        let mut creator = match OwnedConmonCreator::spawn_gated_with_pid_receipt(
            &manifest.conmon_launch.create_command,
            &manifest.conmon_layout.conmon_pidfile,
        ) {
            Ok(creator) => creator,
            Err(error) => {
                let persistence = self
                    .persist_krun_creator_quiescence(
                        manifest,
                        CreatorQuiescenceProof::never_spawned(attempt_id),
                    )
                    .err();
                return Err(combine_krun_creator_failure(error, None, persistence));
            }
        };
        let receipt = match creator.attempt_receipt(&attempt_id) {
            Ok(receipt) => receipt,
            Err(error) => {
                let quiescence = creator.cancel_before_gate_release_and_confirm_quiesced();
                let persistence = quiescence.as_ref().ok().and_then(|()| {
                    self.persist_krun_creator_quiescence(
                        manifest,
                        CreatorQuiescenceProof::launch_gate_never_released(attempt_id.clone()),
                    )
                    .err()
                });
                return Err(combine_krun_creator_failure(
                    error,
                    quiescence.err(),
                    persistence,
                ));
            }
        };
        if let Err(error) = self.persist_krun_creator_state(
            manifest,
            KrunCreatorHandoffState::Pending {
                receipt: receipt.clone(),
            },
            "krun creator birth/containment receipt",
        ) {
            let quiescence = creator.cancel_before_gate_release_and_confirm_quiesced();
            let persistence = quiescence.as_ref().ok().and_then(|()| {
                self.persist_krun_creator_quiescence(
                    manifest,
                    CreatorQuiescenceProof::launch_gate_never_released(attempt_id.clone()),
                )
                .err()
            });
            return Err(combine_krun_creator_failure(
                error,
                quiescence.err(),
                persistence,
            ));
        }

        if let Err(error) = creator.release_after_receipt_persisted() {
            let quiescence = creator.cancel_before_gate_release_and_confirm_quiesced();
            let persistence = quiescence.as_ref().ok().and_then(|()| {
                self.persist_krun_creator_quiescence(
                    manifest,
                    CreatorQuiescenceProof::launch_gate_never_released(attempt_id.clone()),
                )
                .err()
            });
            return Err(combine_krun_creator_failure(
                error,
                quiescence.err(),
                persistence,
            ));
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
                    self.persist_krun_creator_quiescence(
                        manifest,
                        CreatorQuiescenceProof::dead_contained(receipt.clone()),
                    )
                    .err()
                });
                return Err(combine_krun_creator_failure(
                    error,
                    quiescence.err(),
                    persistence,
                ));
            }
        };
        if let Err(error) = creator.reap_after_runtime_observed(self.config.start_timeout) {
            let quiescence = creator.cancel_and_confirm_quiesced();
            let persistence = quiescence.as_ref().ok().and_then(|()| {
                self.persist_krun_creator_quiescence(
                    manifest,
                    CreatorQuiescenceProof::dead_contained(receipt.clone()),
                )
                .err()
            });
            return Err(combine_krun_creator_failure(
                error,
                quiescence.err(),
                persistence,
            ));
        }
        manifest.creator_handoff = KrunCreatorHandoffState::RuntimeObserved { receipt };
        Ok(runtime_state)
    }

    pub(super) fn reconcile_pending_creator_before_cleanup(
        &self,
        manifest: &mut KrunSandboxManifest,
    ) -> Result<()> {
        let receipt = match &manifest.creator_handoff {
            KrunCreatorHandoffState::NotSpawned
            | KrunCreatorHandoffState::Quiesced { .. }
            | KrunCreatorHandoffState::RuntimeObserved { .. } => return Ok(()),
            KrunCreatorHandoffState::SpawnIntent { attempt_id } => {
                return self.persist_krun_creator_quiescence(
                    manifest,
                    CreatorQuiescenceProof::launch_gate_never_released(attempt_id.clone()),
                );
            }
            KrunCreatorHandoffState::Pending { receipt } => receipt.clone(),
        };

        match observe_creator_containment(&receipt) {
            CreatorContainmentObservation::Live => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "krun creator attempt {} remains live with its exact process birth; cleanup \
                         remains fenced",
                        receipt.attempt_id()
                    ),
                });
            }
            CreatorContainmentObservation::Escaped { reason } => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "krun creator attempt {} escaped its authenticated containment: {reason}; \
                         cleanup remains fenced",
                        receipt.attempt_id()
                    ),
                });
            }
            CreatorContainmentObservation::Unknown { reason } => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "krun creator attempt {} cannot be authenticated: {reason}; cleanup remains \
                         fenced",
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
            RuntimeStateObservation::Present(_) => self.persist_krun_creator_state(
                manifest,
                KrunCreatorHandoffState::RuntimeObserved { receipt },
                "fresh-process runtime-observed krun creator handoff",
            ),
            RuntimeStateObservation::ExplicitlyAbsent => {
                confirm_dead_conmon_receipt(&manifest.conmon_layout.conmon_pidfile)?;
                self.persist_krun_creator_quiescence(
                    manifest,
                    CreatorQuiescenceProof::dead_contained(receipt),
                )
            }
        }
    }

    fn persist_krun_creator_intent_before_spawn(
        &self,
        manifest: &mut KrunSandboxManifest,
        attempt_id: &str,
    ) -> Result<()> {
        manifest.creator_handoff = KrunCreatorHandoffState::SpawnIntent {
            attempt_id: attempt_id.to_owned(),
        };
        let Err(intent_error) =
            self.persist_effect_barrier(manifest, "krun creator-attempt intent")
        else {
            return Ok(());
        };

        let quiescence = self
            .persist_krun_creator_quiescence(
                manifest,
                CreatorQuiescenceProof::never_spawned(attempt_id),
            )
            .err();
        Err(combine_krun_creator_failure(intent_error, None, quiescence))
    }

    fn persist_krun_creator_quiescence(
        &self,
        manifest: &mut KrunSandboxManifest,
        proof: CreatorQuiescenceProof,
    ) -> Result<()> {
        self.persist_krun_creator_state(
            manifest,
            KrunCreatorHandoffState::Quiesced { proof },
            "krun creator quiescence result",
        )
    }

    fn persist_krun_creator_state(
        &self,
        manifest: &mut KrunSandboxManifest,
        state: KrunCreatorHandoffState,
        context: &str,
    ) -> Result<()> {
        manifest.creator_handoff = state;
        let candidate = manifest.clone();
        let Err(persist_error) = self.persist_effect_barrier(manifest, context) else {
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

fn combine_krun_creator_failure(
    primary: SandboxError,
    quiescence: Option<SandboxError>,
    persistence: Option<SandboxError>,
) -> SandboxError {
    let mut secondary = Vec::new();
    if let Some(error) = quiescence {
        secondary.push(format!("creator quiescence failed: {error}"));
    }
    if let Some(error) = persistence {
        secondary.push(format!("creator handoff persistence failed: {error}"));
    }
    if secondary.is_empty() {
        primary
    } else {
        SandboxError::OperationFailed {
            message: format!("{primary}; {}", secondary.join("; ")),
        }
    }
}
