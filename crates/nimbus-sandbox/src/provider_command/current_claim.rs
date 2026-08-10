//! Operations that keep one provider-command stream locked through an effect or inspection.

use super::*;

impl ProviderCommandAttemptJournal {
    /// Recover effect authority from one exact current claimed observation.
    ///
    /// This read-only check does not reserve a second winner. Execution must
    /// revalidate the returned token while the stream lock is held.
    pub fn resume_current_claim(
        &self,
        expected: &ProviderCommandObservation,
    ) -> Result<ProviderCommandExecutionClaim, ProviderCommandJournalError> {
        expected.validate()?;
        let paths = self.paths(expected.claim());
        self.require_current_directory(&paths, "execution recovery")?;
        let _guard = lock(&paths.lock)?;
        let current = self.read_required_current(&paths, "execution recovery")?;
        self.authenticate_locked_observation(&current, expected)?;
        if current.kind != ProviderCommandObservationKind::Claimed {
            return Err(ProviderCommandJournalError::InvalidClaim {
                message: "provider execution recovery requires a current claimed observation"
                    .to_owned(),
            });
        }
        Ok(ProviderCommandExecutionClaim {
            observation: current,
        })
    }

    /// Run and publish one provider effect while its exact claimed epoch remains current.
    ///
    /// The journal lock stays held through the callback. An inspection cannot
    /// authorize a later epoch while an older claimant can still start its effect.
    pub(crate) fn execute_current_claim<T>(
        &self,
        execution_claim: ProviderCommandExecutionClaim,
        execute: impl FnOnce(
            &ProviderCommandExecutionClaim,
        ) -> (T, ProviderCommandObservationKind, Option<String>, Vec<u8>),
    ) -> Result<(T, ProviderCommandObservation), ProviderCommandJournalError> {
        execution_claim.observation.validate()?;
        let claim = execution_claim.claim();
        let paths = self.paths(claim);
        self.require_current_directory(&paths, "execution")?;
        let _guard = lock(&paths.lock)?;
        remove_stale_stage(&paths.stage)?;
        let current = self.read_required_current(&paths, "execution")?;
        self.authenticate_locked_observation(&current, &execution_claim.observation)?;
        debug_assert_eq!(current.kind, ProviderCommandObservationKind::Claimed);
        let (output, kind, failure_code, evidence) = execute(&execution_claim);
        let observation = self.record_observation_locked(
            &paths,
            current,
            kind,
            failure_code.as_deref(),
            &evidence,
        )?;
        Ok((output, observation))
    }

    /// Inspect a provider while its exact command stream remains current.
    ///
    /// The callback must be read-only. Holding the stream lock before the
    /// backend lifecycle lock prevents a successor epoch from crossing the
    /// inspected provider evidence.
    pub fn inspect_current_claim<T>(
        &self,
        expected: &ProviderCommandObservation,
        inspect: impl FnOnce(&ProviderCommandObservation) -> T,
    ) -> Result<T, ProviderCommandJournalError> {
        expected.validate()?;
        let paths = self.paths(expected.claim());
        self.require_current_directory(&paths, "inspection")?;
        let _guard = lock(&paths.lock)?;
        let current = self.read_required_current(&paths, "inspection")?;
        self.authenticate_locked_observation(&current, expected)?;
        if !matches!(
            current.kind,
            ProviderCommandObservationKind::Claimed
                | ProviderCommandObservationKind::InProgress
                | ProviderCommandObservationKind::Ambiguous
        ) {
            return Err(ProviderCommandJournalError::InvalidClaim {
                message: "provider inspection requires a nonterminal current observation"
                    .to_owned(),
            });
        }
        Ok(inspect(&current))
    }

    fn require_current_directory(
        &self,
        paths: &JournalPaths,
        operation: &str,
    ) -> Result<(), ProviderCommandJournalError> {
        if self.journal_directory_exists(&paths.directory)? {
            return Ok(());
        }
        Err(ProviderCommandJournalError::Store {
            message: format!("provider {operation} claim has no durable journal directory"),
        })
    }

    fn read_required_current(
        &self,
        paths: &JournalPaths,
        operation: &str,
    ) -> Result<ProviderCommandObservation, ProviderCommandJournalError> {
        read_if_present(&paths.record)?.ok_or_else(|| ProviderCommandJournalError::Store {
            message: format!("provider {operation} claim has no durable journal record"),
        })
    }

    fn authenticate_locked_observation(
        &self,
        current: &ProviderCommandObservation,
        expected: &ProviderCommandObservation,
    ) -> Result<(), ProviderCommandJournalError> {
        if current == expected {
            return Ok(());
        }
        if current.claim != expected.claim {
            self.reject_stale_or_crossed(&current.claim, &expected.claim)?;
            return Err(ProviderCommandJournalError::CrossedClaim);
        }
        Err(ProviderCommandJournalError::PriorEffectUnresolved)
    }
}
