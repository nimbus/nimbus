use crate::inspection::{
    SandboxRestartAssessment, SandboxRestartBlocker, SandboxRestartIneligibility,
};
use crate::spec::SandboxRestartPolicy;

use super::conmon::lifecycle::restart_backoff_delay;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RestartAssessmentInput {
    pub(crate) policy: SandboxRestartPolicy,
    pub(crate) exit_code: i32,
    pub(crate) completed_restarts: u32,
    pub(crate) persisted_not_before_millis: Option<u64>,
    pub(crate) shutdown_requested: bool,
    pub(crate) blocker: Option<SandboxRestartBlocker>,
}

pub(crate) fn assess_restart(input: RestartAssessmentInput) -> SandboxRestartAssessment {
    if input.shutdown_requested {
        return SandboxRestartAssessment::Ineligible {
            reason: SandboxRestartIneligibility::ShutdownRequested,
        };
    }

    let max_restarts = match input.policy {
        SandboxRestartPolicy::Never => {
            return SandboxRestartAssessment::Ineligible {
                reason: SandboxRestartIneligibility::PolicyNever,
            };
        }
        SandboxRestartPolicy::OnFailure { max_restarts: _ } if input.exit_code == 0 => {
            return SandboxRestartAssessment::Ineligible {
                reason: SandboxRestartIneligibility::SuccessfulExitExcluded,
            };
        }
        SandboxRestartPolicy::OnFailure { max_restarts }
        | SandboxRestartPolicy::Always { max_restarts } => max_restarts,
    };

    if input.completed_restarts >= max_restarts {
        return SandboxRestartAssessment::Ineligible {
            reason: SandboxRestartIneligibility::AttemptsExhausted,
        };
    }

    SandboxRestartAssessment::Candidate {
        exit_code: input.exit_code,
        completed_restarts: input.completed_restarts,
        retry_delay_millis: restart_backoff_delay(input.completed_restarts).as_millis() as u64,
        persisted_not_before_millis: input.persisted_not_before_millis,
        blocker: input.blocker,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(policy: SandboxRestartPolicy, exit_code: i32) -> RestartAssessmentInput {
        RestartAssessmentInput {
            policy,
            exit_code,
            completed_restarts: 0,
            persisted_not_before_millis: None,
            shutdown_requested: false,
            blocker: None,
        }
    }

    #[test]
    fn restart_assessment_covers_the_complete_policy_attempt_and_shutdown_matrix() {
        let ineligible = |reason| SandboxRestartAssessment::Ineligible { reason };
        let candidate = |exit_code, completed_restarts| SandboxRestartAssessment::Candidate {
            exit_code,
            completed_restarts,
            retry_delay_millis: restart_backoff_delay(completed_restarts).as_millis() as u64,
            persisted_not_before_millis: None,
            blocker: None,
        };
        let mut cases = Vec::new();

        for exit_code in [0, 42] {
            cases.push((
                format!("never-exit-{exit_code}"),
                input(SandboxRestartPolicy::Never, exit_code),
                ineligible(SandboxRestartIneligibility::PolicyNever),
            ));
        }

        for completed_restarts in [0, 1, 2, 3] {
            for exit_code in [0, 42] {
                let mut on_failure = input(
                    SandboxRestartPolicy::OnFailure { max_restarts: 2 },
                    exit_code,
                );
                on_failure.completed_restarts = completed_restarts;
                let expected = if exit_code == 0 {
                    ineligible(SandboxRestartIneligibility::SuccessfulExitExcluded)
                } else if completed_restarts >= 2 {
                    ineligible(SandboxRestartIneligibility::AttemptsExhausted)
                } else {
                    candidate(exit_code, completed_restarts)
                };
                cases.push((
                    format!("on-failure-exit-{exit_code}-attempt-{completed_restarts}"),
                    on_failure,
                    expected,
                ));

                let mut always = input(SandboxRestartPolicy::Always { max_restarts: 2 }, exit_code);
                always.completed_restarts = completed_restarts;
                let expected = if completed_restarts >= 2 {
                    ineligible(SandboxRestartIneligibility::AttemptsExhausted)
                } else {
                    candidate(exit_code, completed_restarts)
                };
                cases.push((
                    format!("always-exit-{exit_code}-attempt-{completed_restarts}"),
                    always,
                    expected,
                ));
            }
        }

        for (name, policy, exit_code) in [
            ("shutdown-never", SandboxRestartPolicy::Never, 42),
            (
                "shutdown-on-failure-clean",
                SandboxRestartPolicy::OnFailure { max_restarts: 0 },
                0,
            ),
            (
                "shutdown-on-failure-exhausted",
                SandboxRestartPolicy::OnFailure { max_restarts: 0 },
                42,
            ),
            (
                "shutdown-always-exhausted",
                SandboxRestartPolicy::Always { max_restarts: 0 },
                42,
            ),
        ] {
            let mut shutdown = input(policy, exit_code);
            shutdown.shutdown_requested = true;
            cases.push((
                name.to_owned(),
                shutdown,
                ineligible(SandboxRestartIneligibility::ShutdownRequested),
            ));
        }

        for (name, actual, expected) in cases {
            assert_eq!(assess_restart(actual), expected, "{name}");
        }
    }

    #[test]
    fn candidate_reports_existing_evidence_without_reading_a_clock() {
        let mut candidate = input(SandboxRestartPolicy::Always { max_restarts: 3 }, 0);
        candidate.completed_restarts = 1;
        candidate.persisted_not_before_millis = Some(8_000);
        candidate.blocker = Some(SandboxRestartBlocker::StartupReconciliationUnavailable);

        assert_eq!(
            assess_restart(candidate),
            SandboxRestartAssessment::Candidate {
                exit_code: 0,
                completed_restarts: 1,
                retry_delay_millis: 2_000,
                persisted_not_before_millis: Some(8_000),
                blocker: Some(SandboxRestartBlocker::StartupReconciliationUnavailable),
            }
        );
    }

    #[test]
    fn blocker_is_reported_only_after_policy_attempt_and_shutdown_admit_restart() {
        let blocker = SandboxRestartBlocker::StartupReconciliationUnavailable;
        let mut cases = vec![
            (
                "shutdown",
                input(SandboxRestartPolicy::Always { max_restarts: 3 }, 42),
                SandboxRestartIneligibility::ShutdownRequested,
            ),
            (
                "never",
                input(SandboxRestartPolicy::Never, 42),
                SandboxRestartIneligibility::PolicyNever,
            ),
            (
                "successful-on-failure",
                input(SandboxRestartPolicy::OnFailure { max_restarts: 3 }, 0),
                SandboxRestartIneligibility::SuccessfulExitExcluded,
            ),
            (
                "attempts-exhausted",
                input(SandboxRestartPolicy::Always { max_restarts: 1 }, 42),
                SandboxRestartIneligibility::AttemptsExhausted,
            ),
        ];
        cases[0].1.shutdown_requested = true;
        cases[3].1.completed_restarts = 1;

        for (name, mut input, reason) in cases {
            input.blocker = Some(blocker);
            assert_eq!(
                assess_restart(input),
                SandboxRestartAssessment::Ineligible { reason },
                "{name}: a provider blocker must not override a stronger ineligibility reason"
            );
        }

        let mut admitted = input(SandboxRestartPolicy::Always { max_restarts: 1 }, 42);
        admitted.blocker = Some(blocker);
        assert_eq!(
            assess_restart(admitted),
            SandboxRestartAssessment::Candidate {
                exit_code: 42,
                completed_restarts: 0,
                retry_delay_millis: 1_000,
                persisted_not_before_millis: None,
                blocker: Some(blocker),
            },
            "an otherwise eligible restart must carry the read-only blocker evidence"
        );
    }
}
