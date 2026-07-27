//! Durable container creator-attempt orchestration.

use crate::backends::conmon::creator::OwnedConmonCreator;
use crate::backends::conmon::lifecycle::wait_for_runtime_state;
use ulid::Ulid;

use super::*;

impl ContainerSandboxBackend {
    pub(super) fn spawn_creator_and_wait_for_runtime(
        &self,
        manifest: &mut ContainerSandboxManifest,
    ) -> Result<String> {
        let attempt_id = Ulid::new().to_string().to_ascii_lowercase();
        self.persist_creator_intent_before_spawn(manifest, &attempt_id)?;
        let mut creator = match OwnedConmonCreator::spawn_with_pid_receipt(
            &manifest.conmon_launch.create_command,
            &manifest.conmon_layout.conmon_pidfile,
        ) {
            Ok(creator) => creator,
            Err(error) => {
                manifest.creator_handoff = ContainerCreatorHandoffState::Quiesced { attempt_id };
                let persistence = self.write_manifest(manifest).err();
                return Err(combine_launch_failure(error, None, persistence));
            }
        };
        let runtime_state = match wait_for_runtime_state(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
            self.config.start_timeout,
        ) {
            Ok(runtime_state) => runtime_state,
            Err(error) => {
                let quiescence = creator.cancel_and_confirm_quiesced();
                if quiescence.is_ok() {
                    manifest.creator_handoff =
                        ContainerCreatorHandoffState::Quiesced { attempt_id };
                }
                let persistence = quiescence
                    .as_ref()
                    .ok()
                    .and_then(|()| self.write_manifest(manifest).err());
                return Err(combine_launch_failure(error, quiescence.err(), persistence));
            }
        };
        if let Err(error) = creator.reap_after_runtime_observed(self.config.start_timeout) {
            let quiescence = creator.cancel_and_confirm_quiesced();
            if quiescence.is_ok() {
                manifest.creator_handoff = ContainerCreatorHandoffState::Quiesced { attempt_id };
            }
            let persistence = quiescence
                .as_ref()
                .ok()
                .and_then(|()| self.write_manifest(manifest).err());
            return Err(combine_launch_failure(error, quiescence.err(), persistence));
        }
        manifest.creator_handoff = ContainerCreatorHandoffState::RuntimeObserved { attempt_id };
        Ok(runtime_state)
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
}

fn persist_creator_intent_before_spawn_with(
    manifest: &mut ContainerSandboxManifest,
    attempt_id: &str,
    mut persist: impl FnMut(&ContainerSandboxManifest) -> Result<()>,
    mut inspect: impl FnMut(&SandboxId) -> Result<Option<ContainerSandboxManifest>>,
) -> Result<()> {
    manifest.creator_handoff = ContainerCreatorHandoffState::Pending {
        attempt_id: attempt_id.to_owned(),
    };
    let Err(intent_error) = persist(manifest) else {
        return Ok(());
    };

    // The provider command has not been spawned. Resolve both a definite
    // pre-commit failure and a lost post-rename acknowledgement by publishing
    // the exact attempt as quiesced before launch compensation may proceed.
    manifest.creator_handoff = ContainerCreatorHandoffState::Quiesced {
        attempt_id: attempt_id.to_owned(),
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
            manifest.creator_handoff = ContainerCreatorHandoffState::Pending {
                attempt_id: attempt_id.to_owned(),
            };
            Err(combine_launch_failure(
                intent_error,
                None,
                Some(quiescence_error),
            ))
        }
        Err(inspect_error) => {
            manifest.creator_handoff = ContainerCreatorHandoffState::Pending {
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
