//! Atomic provider-command claims with exact prepared request bytes.

use super::*;

const MAX_PREPARED_REQUEST_BYTES: usize = 1024 * 1024;

impl ProviderCommandAttemptJournal {
    /// Atomically claim one epoch and persist its exact prepared request.
    ///
    /// The first durable record is `InProgress`; no externally visible
    /// `Claimed` gap exists. A delayed token must revalidate this exact record
    /// under the stream lock before it can send the prepared bytes.
    pub fn claim_dispatch_epoch_started(
        &self,
        claim: &ProviderCommandClaim,
        prepared_request: &[u8],
    ) -> Result<ProviderCommandStartedClaimDecision, ProviderCommandJournalError> {
        claim.validate()?;
        validate_prepared_request(prepared_request)?;
        let paths = self.paths(claim);
        self.establish_directory(&paths.directory)?;
        let _guard = lock(&paths.lock)?;
        remove_stale_stage(&paths.stage)?;
        let current = read_if_present(&paths.record)?;
        match current {
            None => {
                Self::require_initial_restart_ordinal(claim)?;
                self.publish_new_started_claim(&paths, claim.clone(), None, prepared_request)
            }
            Some(current) => self.decide_existing_started(&paths, current, claim, prepared_request),
        }
    }

    /// Atomically start one adjacent epoch after exact inspected absence.
    ///
    /// This is the prepared-request counterpart of
    /// `claim_dispatch_epoch_after_inspected_absence`. It authenticates the
    /// coordinator receipt and publishes the successor directly as
    /// `InProgress`, so a crash cannot recreate a resumable `Claimed` gap.
    pub fn claim_dispatch_epoch_after_inspected_absence_started(
        &self,
        claim: &ProviderCommandClaim,
        inspected_dispatch_epoch: u64,
        inspection_evidence: &[u8],
        prepared_request: &[u8],
    ) -> Result<ProviderCommandStartedClaimDecision, ProviderCommandJournalError> {
        claim.validate()?;
        validate_prepared_request(prepared_request)?;
        if inspected_dispatch_epoch.checked_add(1) != Some(claim.dispatch_epoch) {
            return Err(ProviderCommandJournalError::RetryWithoutAuthority);
        }
        let paths = self.paths(claim);
        self.establish_directory(&paths.directory)?;
        let _guard = lock(&paths.lock)?;
        remove_stale_stage(&paths.stage)?;
        let Some(current) = read_if_present(&paths.record)? else {
            return Err(ProviderCommandJournalError::RetryWithoutAuthority);
        };

        if current.claim == *claim {
            return self.decide_existing_started(&paths, current, claim, prepared_request);
        }
        if !claim.same_attempt_fence(&current.claim) {
            return match self.reject_stale_or_crossed(&current.claim, claim) {
                Ok(()) => Err(ProviderCommandJournalError::CrossedClaim),
                Err(error) => Err(error),
            };
        }
        if current.claim.dispatch_epoch != inspected_dispatch_epoch {
            return Err(if claim.dispatch_epoch < current.claim.dispatch_epoch {
                Self::reject_stale_dispatch_epoch(
                    current.claim.dispatch_epoch,
                    claim.dispatch_epoch,
                )
            } else {
                ProviderCommandJournalError::CrossedClaim
            });
        }

        match current.kind {
            ProviderCommandObservationKind::Absent
            | ProviderCommandObservationKind::RetryAuthorized => {
                self.decide_existing_started(&paths, current, claim, prepared_request)
            }
            ProviderCommandObservationKind::InProgress
            | ProviderCommandObservationKind::Ambiguous => {
                let absence = ProviderCommandObservation {
                    claim: current.claim.clone(),
                    kind: ProviderCommandObservationKind::Absent,
                    evidence_sha256: Some(evidence_sha256(inspection_evidence)),
                    prepared_request: current.prepared_request.clone(),
                    prepared_request_sha256: current.prepared_request_sha256.clone(),
                    failure_code: None,
                    retry_lineage: current.retry_lineage.clone(),
                };
                self.publish_new_started_claim(
                    &paths,
                    claim.clone(),
                    Some(absence),
                    prepared_request,
                )
            }
            ProviderCommandObservationKind::Claimed => {
                Err(ProviderCommandJournalError::PriorEffectUnresolved)
            }
            ProviderCommandObservationKind::Succeeded
            | ProviderCommandObservationKind::DefiniteFailure => {
                Err(ProviderCommandJournalError::RetryWithoutAuthority)
            }
        }
    }

    fn publish_new_started_claim(
        &self,
        paths: &JournalPaths,
        claim: ProviderCommandClaim,
        retry_authority: Option<ProviderCommandObservation>,
        prepared_request: &[u8],
    ) -> Result<ProviderCommandStartedClaimDecision, ProviderCommandJournalError> {
        validate_prepared_request(prepared_request)?;
        let mut retry_lineage = retry_authority
            .as_ref()
            .map_or_else(Vec::new, |observation| observation.retry_lineage.clone());
        if let Some(observation) = retry_authority.as_ref() {
            retry_lineage.push(ProviderCommandRetryReceipt::from_observation(observation)?);
        }
        let digest = evidence_sha256(prepared_request);
        let observation = ProviderCommandObservation {
            claim,
            kind: ProviderCommandObservationKind::InProgress,
            evidence_sha256: Some(digest.clone()),
            prepared_request: Some(prepared_request.to_vec()),
            prepared_request_sha256: Some(digest),
            failure_code: None,
            retry_lineage,
        };
        publish(paths, &observation)?;
        Ok(ProviderCommandStartedClaimDecision::ExecuteStarted(
            ProviderCommandStartedExecutionClaim { observation },
        ))
    }

    fn decide_existing_started(
        &self,
        paths: &JournalPaths,
        current: ProviderCommandObservation,
        candidate: &ProviderCommandClaim,
        prepared_request: &[u8],
    ) -> Result<ProviderCommandStartedClaimDecision, ProviderCommandJournalError> {
        if candidate.workload_generation < current.claim.workload_generation {
            return Err(ProviderCommandJournalError::StaleWorkloadGeneration {
                current: current.claim.workload_generation,
                candidate: candidate.workload_generation,
            });
        }
        if candidate.workload_generation > current.claim.workload_generation {
            if !matches!(
                current.kind,
                ProviderCommandObservationKind::Absent
                    | ProviderCommandObservationKind::DefiniteFailure
            ) {
                return Err(ProviderCommandJournalError::PriorEffectUnresolved);
            }
            Self::require_initial_restart_ordinal(candidate)?;
            return self.publish_new_started_claim(
                paths,
                candidate.clone(),
                None,
                prepared_request,
            );
        }
        if candidate.restart_ordinal < current.claim.restart_ordinal {
            return Err(ProviderCommandJournalError::StaleRestartOrdinal {
                current: current.claim.restart_ordinal,
                candidate: candidate.restart_ordinal,
            });
        }
        if candidate.restart_ordinal > current.claim.restart_ordinal {
            let expected = current.claim.restart_ordinal.checked_add(1).ok_or(
                ProviderCommandJournalError::SkippedRestartOrdinal {
                    current: current.claim.restart_ordinal,
                    candidate: candidate.restart_ordinal,
                },
            )?;
            if candidate.restart_ordinal != expected {
                return Err(ProviderCommandJournalError::SkippedRestartOrdinal {
                    current: current.claim.restart_ordinal,
                    candidate: candidate.restart_ordinal,
                });
            }
            if !candidate.same_workload_fence(&current.claim) {
                return Err(ProviderCommandJournalError::CrossedClaim);
            }
            if !current.kind.resolves_effect() {
                return Err(ProviderCommandJournalError::PriorEffectUnresolved);
            }
            if candidate.operation.is_restart()
                && candidate.source_attempt_id.as_deref() != Some(current.claim.attempt_id())
            {
                return Err(ProviderCommandJournalError::CrossedClaim);
            }
            return self.publish_new_started_claim(
                paths,
                candidate.clone(),
                None,
                prepared_request,
            );
        }
        if !candidate.same_attempt_fence(&current.claim) {
            return Err(ProviderCommandJournalError::CrossedClaim);
        }
        if candidate.dispatch_epoch < current.claim.dispatch_epoch {
            return Err(Self::reject_stale_dispatch_epoch(
                current.claim.dispatch_epoch,
                candidate.dispatch_epoch,
            ));
        }
        if candidate.dispatch_epoch == current.claim.dispatch_epoch {
            if current.kind == ProviderCommandObservationKind::Claimed {
                return Err(ProviderCommandJournalError::PriorEffectUnresolved);
            }
            if current.prepared_request.as_deref() != Some(prepared_request) {
                return Err(ProviderCommandJournalError::CrossedClaim);
            }
            return Ok(ProviderCommandStartedClaimDecision::AdoptExactAttempt(
                current,
            ));
        }
        let expected = current.claim.dispatch_epoch.checked_add(1).ok_or(
            ProviderCommandJournalError::SkippedDispatchEpoch {
                current: current.claim.dispatch_epoch,
                candidate: candidate.dispatch_epoch,
            },
        )?;
        if candidate.dispatch_epoch != expected {
            return Err(ProviderCommandJournalError::SkippedDispatchEpoch {
                current: current.claim.dispatch_epoch,
                candidate: candidate.dispatch_epoch,
            });
        }
        if !current.kind.authorizes_retry(current.claim.operation) {
            return Err(ProviderCommandJournalError::RetryWithoutAuthority);
        }
        self.publish_new_started_claim(paths, candidate.clone(), Some(current), prepared_request)
    }
}

pub(super) fn validate_prepared_request(request: &[u8]) -> Result<(), ProviderCommandJournalError> {
    if request.is_empty() || request.len() > MAX_PREPARED_REQUEST_BYTES {
        return Err(ProviderCommandJournalError::InvalidClaim {
            message: "prepared provider request must be non-empty and at most 1 MiB".to_owned(),
        });
    }
    Ok(())
}
