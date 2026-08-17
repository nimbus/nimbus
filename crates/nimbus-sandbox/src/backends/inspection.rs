use crate::inspection::{
    SandboxRestartAssessment, SandboxRestartBlocker, SandboxRestartIneligibility,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RestartAssessmentInput {
    pub(crate) exit_code: i32,
    pub(crate) shutdown_requested: bool,
    pub(crate) blocker: Option<SandboxRestartBlocker>,
}

pub(crate) fn assess_restart(input: RestartAssessmentInput) -> SandboxRestartAssessment {
    if input.shutdown_requested {
        return SandboxRestartAssessment::Ineligible {
            reason: SandboxRestartIneligibility::ShutdownRequested,
        };
    }

    SandboxRestartAssessment::Candidate {
        exit_code: input.exit_code,
        blocker: input.blocker,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(exit_code: i32) -> RestartAssessmentInput {
        RestartAssessmentInput {
            exit_code,
            shutdown_requested: false,
            blocker: None,
        }
    }

    #[test]
    fn assessment_reports_only_physical_exit_and_provider_blocker_evidence() {
        for exit_code in [0, 42] {
            assert_eq!(
                assess_restart(input(exit_code)),
                SandboxRestartAssessment::Candidate {
                    exit_code,
                    blocker: None,
                }
            );
        }
    }

    #[test]
    fn candidate_preserves_provider_blocker_without_policy_or_schedule_authority() {
        let mut candidate = input(0);
        candidate.blocker = Some(SandboxRestartBlocker::StartupReconciliationUnavailable);

        assert_eq!(
            assess_restart(candidate),
            SandboxRestartAssessment::Candidate {
                exit_code: 0,
                blocker: Some(SandboxRestartBlocker::StartupReconciliationUnavailable),
            }
        );
    }

    #[test]
    fn shutdown_is_the_only_provider_owned_restart_ineligibility() {
        let mut shutdown = input(42);
        shutdown.shutdown_requested = true;
        shutdown.blocker = Some(SandboxRestartBlocker::StartupReconciliationUnavailable);
        assert_eq!(
            assess_restart(shutdown),
            SandboxRestartAssessment::Ineligible {
                reason: SandboxRestartIneligibility::ShutdownRequested,
            },
        );
    }
}
