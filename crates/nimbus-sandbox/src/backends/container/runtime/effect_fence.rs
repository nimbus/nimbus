//! Bounded persistence of the pre-provider-effect runner fence.
//!
//! Both direct and prepared-runner starts must durably publish
//! `EffectsStarted` before entering any provider. A permanent state-volume
//! failure must return control without creating effects or retaining the
//! lifecycle lock forever, while an acknowledgement loss after the atomic
//! phase replacement must remain fail-closed.

use super::*;

pub(super) const EFFECT_FENCE_PERSIST_ATTEMPTS: usize = 4;

pub(super) fn converge_persistence_with(
    max_attempts: usize,
    mut transition: impl FnMut() -> Result<()>,
    mut wait: impl FnMut(runner::RunnerOwnershipConvergenceStage, &SandboxError),
) -> Result<()> {
    let stage = runner::RunnerOwnershipConvergenceStage::EffectsStarted;
    if max_attempts == 0 {
        return Err(SandboxError::OperationFailed {
            message: "container runner EffectsStarted convergence requires at least one \
                      persistence attempt"
                .to_owned(),
        });
    }
    let mut attempt = 1;
    loop {
        match transition() {
            Ok(()) => return Ok(()),
            Err(error) if attempt < max_attempts => {
                wait(stage, &error);
                attempt += 1;
            }
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container runner failed to persist {stage:?} after {max_attempts} \
                         attempts: {error}"
                    ),
                });
            }
        }
    }
}

pub(super) fn diagnose_exhaustion(
    manifest: &ContainerSandboxManifest,
    persistence: SandboxError,
) -> SandboxError {
    match runner::execute_handoff_phase(manifest) {
        Ok(Some(runner::RunnerHandoffPhase::ClaimedBeforeEffects)) => {
            SandboxError::OperationFailed {
                message: format!(
                    "{persistence}; no provider effect began for sandbox {}; explicit stop can \
                     authenticate and release the durable pre-effect handoff",
                    manifest.handle.id
                ),
            }
        }
        Ok(Some(runner::RunnerHandoffPhase::EffectsStarted)) => SandboxError::OperationFailed {
            message: format!(
                "{persistence}; the durable EffectsStarted boundary for sandbox {} may have \
                     published despite acknowledgement loss; no provider launch was attempted by \
                     this owner, and inspect-before-retry reconciliation is required",
                manifest.handle.id
            ),
        },
        Ok(Some(phase)) => SandboxError::OperationFailed {
            message: format!(
                "{persistence}; sandbox {} reached unexpected handoff phase {phase:?} before \
                 provider launch; lifecycle mutation remains fenced",
                manifest.handle.id
            ),
        },
        Ok(None) => SandboxError::OperationFailed {
            message: format!(
                "{persistence}; sandbox {} has no authenticated pre-provider handoff after \
                 persistence exhaustion; lifecycle mutation remains fenced",
                manifest.handle.id
            ),
        },
        Err(observation) => SandboxError::OperationFailed {
            message: format!(
                "{persistence}; cannot authenticate sandbox {} handoff after persistence \
                 exhaustion: {observation}; lifecycle mutation remains fenced",
                manifest.handle.id
            ),
        },
    }
}
