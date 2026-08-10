//! Operations that keep one provider-command stream locked through an effect or inspection.

use std::future::Future;
use std::pin::Pin;

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
            &ProviderCommandCurrentExecution,
        ) -> (T, ProviderCommandObservationKind, Option<String>, Vec<u8>),
    ) -> Result<(T, ProviderCommandObservation), ProviderCommandJournalError> {
        let locked = self.clone().lock_current_execution(execution_claim)?;
        let (output, kind, failure_code, evidence) = execute(&locked.current);
        let observation = locked.finish(kind, failure_code, evidence)?;
        Ok((output, observation))
    }

    /// Run awaited provider work and publish while its Execute claim stays current.
    ///
    /// An internal worker owns the stream lock, child future, and durable
    /// publication. Dropping the caller future detaches that worker instead of
    /// canceling an effect that can still commit. Blocking journal reads and
    /// writes run on blocking workers while the same OS lock remains held.
    pub async fn execute_current_claim_async<T, Execute>(
        &self,
        execution_claim: ProviderCommandExecutionClaim,
        execute: Execute,
    ) -> Result<(T, ProviderCommandObservation), ProviderCommandJournalError>
    where
        T: Send + 'static,
        Execute: for<'a> FnOnce(
                &'a ProviderCommandCurrentExecution,
            ) -> Pin<
                Box<
                    dyn Future<
                            Output = (T, ProviderCommandObservationKind, Option<String>, Vec<u8>),
                        > + Send
                        + 'a,
                >,
            > + Send
            + 'static,
    {
        let journal = self.clone();
        tokio::spawn(async move {
            let locked =
                spawn_blocking_journal(move || journal.lock_current_execution(execution_claim))
                    .await?;
            let (output, kind, failure_code, evidence) = execute(&locked.current).await;
            spawn_blocking_journal(move || {
                let observation = locked.finish(kind, failure_code, evidence)?;
                Ok((output, observation))
            })
            .await
        })
        .await
        .map_err(join_error)?
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

    /// Await a read-only provider inspection while its exact stream stays current.
    ///
    /// A `Claimed` stream returns `EffectCanStillStart` without polling the
    /// callback. Therefore, a delayed older Execute can never cross an Inspect
    /// result that the caller classifies as `NotCompleted`. `InProgress` and
    /// `Ambiguous` observations already invalidate that older token.
    pub async fn inspect_current_claim_async<T, Inspect>(
        &self,
        expected: &ProviderCommandObservation,
        inspect: Inspect,
    ) -> Result<ProviderCommandCurrentInspection<T>, ProviderCommandJournalError>
    where
        Inspect: for<'a> FnOnce(
                &'a ProviderCommandObservation,
            ) -> Pin<Box<dyn Future<Output = T> + Send + 'a>>
            + Send,
    {
        let journal = self.clone();
        let expected = expected.clone();
        let locked =
            spawn_blocking_journal(move || journal.lock_current_inspection(&expected)).await?;
        if locked.current.kind == ProviderCommandObservationKind::Claimed {
            return Ok(ProviderCommandCurrentInspection::EffectCanStillStart(
                Box::new(locked.current.clone()),
            ));
        }
        let output = inspect(&locked.current).await;
        Ok(ProviderCommandCurrentInspection::Inspected(output))
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

    fn lock_current_execution(
        self,
        execution_claim: ProviderCommandExecutionClaim,
    ) -> Result<LockedCurrentExecution, ProviderCommandJournalError> {
        execution_claim.observation.validate()?;
        let paths = self.paths(execution_claim.claim());
        self.require_current_directory(&paths, "execution")?;
        let guard = lock(&paths.lock)?;
        remove_stale_stage(&paths.stage)?;
        let current = self.read_required_current(&paths, "execution")?;
        self.authenticate_locked_observation(&current, &execution_claim.observation)?;
        if current.kind != ProviderCommandObservationKind::Claimed {
            return Err(ProviderCommandJournalError::InvalidClaim {
                message: "provider execution requires a current claimed observation".to_owned(),
            });
        }
        Ok(LockedCurrentExecution {
            journal: self,
            paths,
            current: ProviderCommandCurrentExecution {
                observation: current,
            },
            _guard: guard,
        })
    }

    fn lock_current_inspection(
        self,
        expected: &ProviderCommandObservation,
    ) -> Result<LockedCurrentInspection, ProviderCommandJournalError> {
        expected.validate()?;
        let paths = self.paths(expected.claim());
        self.require_current_directory(&paths, "asynchronous inspection")?;
        let guard = lock(&paths.lock)?;
        let current = self.read_required_current(&paths, "asynchronous inspection")?;
        self.authenticate_locked_observation(&current, expected)?;
        if !matches!(
            current.kind,
            ProviderCommandObservationKind::Claimed
                | ProviderCommandObservationKind::InProgress
                | ProviderCommandObservationKind::Ambiguous
        ) {
            return Err(ProviderCommandJournalError::InvalidClaim {
                message:
                    "asynchronous provider inspection requires a nonterminal current observation"
                        .to_owned(),
            });
        }
        Ok(LockedCurrentInspection {
            current,
            _guard: guard,
        })
    }
}

struct LockedCurrentExecution {
    journal: ProviderCommandAttemptJournal,
    paths: JournalPaths,
    current: ProviderCommandCurrentExecution,
    _guard: JournalGuard,
}

impl LockedCurrentExecution {
    fn finish(
        self,
        kind: ProviderCommandObservationKind,
        failure_code: Option<String>,
        evidence: Vec<u8>,
    ) -> Result<ProviderCommandObservation, ProviderCommandJournalError> {
        let Self {
            journal,
            paths,
            current,
            _guard,
        } = self;
        journal.record_observation_locked(
            &paths,
            current.observation,
            kind,
            failure_code.as_deref(),
            &evidence,
        )
    }
}

struct LockedCurrentInspection {
    current: ProviderCommandObservation,
    _guard: JournalGuard,
}

async fn spawn_blocking_journal<T>(
    operation: impl FnOnce() -> Result<T, ProviderCommandJournalError> + Send + 'static,
) -> Result<T, ProviderCommandJournalError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(join_error)?
}

fn join_error(error: tokio::task::JoinError) -> ProviderCommandJournalError {
    ProviderCommandJournalError::Store {
        message: format!("provider journal worker failed: {error}"),
    }
}
