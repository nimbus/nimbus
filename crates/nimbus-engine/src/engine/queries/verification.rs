use std::sync::Arc;
use std::time::Instant;

use nimbus_core::{Error, Result, SequenceNumber, TenantEventRecord, TenantId};
use nimbus_storage::{
    MaterializedVerificationTracker, ShadowMaterializer, ShadowMaterializerConfig, TenantStore,
};

use super::verification_sessions::{FastPathOutcome, FullScrubEvidence, VerificationSession};
use crate::EmbeddedReplica;
use crate::engine::Engine;
use crate::verification::{
    ConsistencyEscalationReason, ConsistencyMismatch, ConsistencyScope,
    ConsistencyVerificationMode, ConsistencyVerificationReport, VerificationAnchor,
    bootstrap_fingerprint, collect_durable_journal_bootstrap_mismatches,
    compare_materialized_journal_snapshots, snapshot_fingerprint, verification_root_fingerprint,
};

const VERIFICATION_STREAM_LIMIT: usize = 256;

impl Engine {
    /// Builds a shadow materializer from the current authoritative journal state.
    pub async fn build_shadow_materializer_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        config: ShadowMaterializerConfig,
    ) -> Result<ShadowMaterializer> {
        let bootstrap = self
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await?;
        let tail = self
            .read_durable_journal_suffix_to_sequence_async(&tenant_id, &bootstrap)
            .await?;
        ShadowMaterializer::from_checkpoint_and_journal(bootstrap.snapshot, tail, config)
    }

    /// Verifies actual materialized state, then reuses a bounded root session.
    pub async fn verify_consistency_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<ConsistencyVerificationReport> {
        let slot = self
            .verification_sessions
            .acquire(&tenant_id, self.monotonic_now());
        let mut session_guard = slot.lock().await;
        // Another check for this tenant can wait on the session lock. Read
        // time after admission so an older waiter cannot move last-used time
        // backward or defer an expiry decision.
        let now = self.monotonic_now();
        let config = self.verification_sessions.config();

        let expiry_reason = session_guard
            .as_ref()
            .and_then(|session| session.expiry_reason(now, config));
        let report = if session_guard.is_none() {
            self.run_full_scrub(
                tenant_id.clone(),
                ConsistencyEscalationReason::ColdStart,
                None,
                0,
                now,
                &mut session_guard,
            )
            .await
        } else if let Some(reason) = expiry_reason {
            self.run_incremental_check(tenant_id.clone(), now, Some(reason), &mut session_guard)
                .await
        } else {
            self.run_incremental_check(tenant_id.clone(), now, None, &mut session_guard)
                .await
        };

        // Account for the state that remains in the slot on both success and
        // failure. A disposable session can be lost on an I/O error, but the
        // registry must not retain its old byte charge.
        let resident_index_bytes = session_guard
            .as_ref()
            .map_or(0, VerificationSession::resident_index_bytes);
        self.verification_sessions.record_usage(
            &tenant_id,
            &slot,
            resident_index_bytes,
            self.monotonic_now(),
        );
        report
    }

    async fn run_incremental_check(
        self: &Arc<Self>,
        tenant_id: TenantId,
        now: Instant,
        force_full_reason: Option<ConsistencyEscalationReason>,
        session_guard: &mut Option<VerificationSession>,
    ) -> Result<ConsistencyVerificationReport> {
        let target = self.applied_sequence_async(tenant_id.clone()).await?;
        let mut session = session_guard
            .take()
            .expect("incremental verification requires a session");
        let Some(current) = session.applied_sequence() else {
            return self
                .run_full_scrub(
                    tenant_id,
                    ConsistencyEscalationReason::RootMismatch,
                    Some(session),
                    0,
                    now,
                    session_guard,
                )
                .await;
        };
        if target.0 < current {
            return self
                .run_full_scrub(
                    tenant_id,
                    ConsistencyEscalationReason::AppliedSequenceRewind,
                    Some(session),
                    0,
                    now,
                    session_guard,
                )
                .await;
        }

        let records = if target.0 == current {
            Vec::new()
        } else {
            match self
                .read_contiguous_applied_suffix(&tenant_id, SequenceNumber(current), target)
                .await
            {
                Ok(records) => records,
                Err(error) if session_suffix_requires_rebuild(&error) => {
                    return self
                        .run_full_scrub(
                            tenant_id,
                            ConsistencyEscalationReason::RetentionGap,
                            Some(session),
                            0,
                            now,
                            session_guard,
                        )
                        .await;
                }
                Err(error) => return Err(error),
            }
        };

        match session.apply_records(&records) {
            FastPathOutcome::Applied if session.positions_match() => {
                if let Some(reason) = force_full_reason {
                    return self
                        .run_full_scrub(
                            tenant_id,
                            reason,
                            Some(session),
                            records.len() as u64,
                            now,
                            session_guard,
                        )
                        .await;
                }
                session.last_used_at = now;
                *session_guard = Some(session);
                report_from_session(
                    &tenant_id,
                    session_guard
                        .as_ref()
                        .expect("successful incremental check retains its session"),
                    ConsistencyVerificationMode::Incremental,
                    None,
                    records.len() as u64,
                    now,
                    Vec::new(),
                )
            }
            FastPathOutcome::Escalate(reason) => {
                self.run_full_scrub(
                    tenant_id,
                    reason,
                    Some(session),
                    records.len() as u64,
                    now,
                    session_guard,
                )
                .await
            }
            FastPathOutcome::Applied => {
                self.run_full_scrub(
                    tenant_id,
                    ConsistencyEscalationReason::RootMismatch,
                    Some(session),
                    records.len() as u64,
                    now,
                    session_guard,
                )
                .await
            }
        }
    }

    async fn run_full_scrub(
        self: &Arc<Self>,
        tenant_id: TenantId,
        reason: ConsistencyEscalationReason,
        prior_session: Option<VerificationSession>,
        event_count: u64,
        now: Instant,
        session_guard: &mut Option<VerificationSession>,
    ) -> Result<ConsistencyVerificationReport> {
        let bootstrap = self
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await?;

        // This snapshot is actual provider state at its applied head. A
        // durable tail can exist during recovery, but it cannot advance a
        // verification root before the provider applies those effects.
        let authoritative_snapshot = bootstrap.snapshot.clone();
        let shadow = ShadowMaterializer::from_checkpoint_and_journal(
            authoritative_snapshot.clone(),
            Vec::new(),
            ShadowMaterializerConfig::default(),
        )?;
        let shadow_snapshot = shadow.current_snapshot();
        let replica = EmbeddedReplica::bootstrap_from_bootstrap(
            tenant_id.clone(),
            TenantStore::create_in_memory()?,
            bootstrap.clone(),
            Vec::new(),
        )?;
        let replica_snapshot = replica.export_materialized_journal_snapshot()?;

        let mut mismatches = Vec::new();
        if let Some(mismatch) = compare_materialized_journal_snapshots(
            ConsistencyScope::AuthoritativeSnapshot,
            &authoritative_snapshot,
            ConsistencyScope::ShadowMaterializer,
            &shadow_snapshot,
        )? {
            mismatches.push(mismatch);
        }
        if let Some(mismatch) = compare_materialized_journal_snapshots(
            ConsistencyScope::AuthoritativeSnapshot,
            &authoritative_snapshot,
            ConsistencyScope::EmbeddedReplica,
            &replica_snapshot,
        )? {
            mismatches.push(mismatch);
        }
        mismatches.extend(collect_durable_journal_bootstrap_mismatches(
            &authoritative_snapshot,
            &bootstrap,
        )?);

        let authoritative =
            MaterializedVerificationTracker::from_snapshot(&authoritative_snapshot)?;
        let shadow_tracker = MaterializedVerificationTracker::from_snapshot(&shadow_snapshot)?;
        let replica_tracker = MaterializedVerificationTracker::from_snapshot(&replica_snapshot)?;
        let evidence = FullScrubEvidence {
            authoritative: snapshot_fingerprint(&authoritative_snapshot)?,
            shadow: snapshot_fingerprint(&shadow_snapshot)?,
            embedded_replica: snapshot_fingerprint(&replica_snapshot)?,
            bootstrap: bootstrap_fingerprint(&bootstrap)?,
        };
        let fresh_session = VerificationSession {
            anchor_started_at: now,
            last_used_at: now,
            evidence,
            authoritative,
            shadow: shadow_tracker,
            embedded_replica: replica_tracker,
        };

        if let Some(prior) = prior_session.as_ref() {
            if reason == ConsistencyEscalationReason::RootMismatch {
                mismatches.extend(compare_session_roots(prior));
            }
            mismatches.extend(compare_prior_roots_to_full_scrub(prior, &fresh_session));
        }
        mismatches.extend(compare_session_roots(&fresh_session));

        let report = report_from_session(
            &tenant_id,
            &fresh_session,
            ConsistencyVerificationMode::FullScrub,
            Some(reason),
            event_count,
            now,
            mismatches.clone(),
        )?;
        if mismatches.is_empty() {
            *session_guard = Some(fresh_session);
        } else {
            *session_guard = prior_session;
        }
        Ok(report)
    }

    async fn read_contiguous_applied_suffix(
        self: &Arc<Self>,
        tenant_id: &TenantId,
        after: SequenceNumber,
        target: SequenceNumber,
    ) -> Result<Vec<TenantEventRecord>> {
        let mut cursor = after;
        let mut records = Vec::new();
        while cursor.0 < target.0 {
            let page = self
                .stream_durable_journal_async(tenant_id.clone(), cursor, VERIFICATION_STREAM_LIMIT)
                .await?;
            let page_records = page
                .records
                .into_iter()
                .take_while(|record| record.sequence.0 <= target.0)
                .collect::<Vec<_>>();
            let Some(last) = page_records.last() else {
                return Err(Error::Internal(format!(
                    "journal stream made no progress while advancing verification session for tenant {tenant_id} from {} to {}",
                    cursor.0, target.0
                )));
            };
            let expected = cursor.0.saturating_add(1);
            if page_records[0].sequence.0 != expected
                || page_records
                    .windows(2)
                    .any(|pair| pair[1].sequence.0 != pair[0].sequence.0.saturating_add(1))
            {
                return Err(Error::Internal(format!(
                    "journal gap while advancing verification session for tenant {tenant_id} from {} to {}",
                    cursor.0, target.0
                )));
            }
            cursor = last.sequence;
            records.extend(page_records);
        }
        Ok(records)
    }

    #[cfg(test)]
    pub(crate) async fn corrupt_verification_shadow_for_testing(
        self: &Arc<Self>,
        tenant_id: &TenantId,
    ) {
        let slot = self
            .verification_sessions
            .acquire(tenant_id, self.monotonic_now());
        let mut guard = slot.lock().await;
        guard
            .as_mut()
            .expect("test must establish a verification session first")
            .corrupt_shadow_for_testing();
    }
}

fn report_from_session(
    tenant_id: &TenantId,
    session: &VerificationSession,
    mode: ConsistencyVerificationMode,
    escalation_reason: Option<ConsistencyEscalationReason>,
    event_count: u64,
    now: Instant,
    mismatches: Vec<ConsistencyMismatch>,
) -> Result<ConsistencyVerificationReport> {
    Ok(ConsistencyVerificationReport {
        tenant_id: tenant_id.to_string(),
        ok: mismatches.is_empty(),
        mode,
        anchor: VerificationAnchor {
            position: session.evidence.authoritative.position.clone(),
            age_millis: duration_millis_u64(
                now.saturating_duration_since(session.anchor_started_at),
            ),
        },
        event_count,
        escalation_reason,
        authoritative_root: verification_root_fingerprint(&session.authoritative)?,
        shadow_root: verification_root_fingerprint(&session.shadow)?,
        embedded_replica_root: verification_root_fingerprint(&session.embedded_replica)?,
        authoritative: session.evidence.authoritative.clone(),
        shadow: session.evidence.shadow.clone(),
        embedded_replica: session.evidence.embedded_replica.clone(),
        bootstrap: session.evidence.bootstrap.clone(),
        mismatches,
    })
}

fn compare_prior_roots_to_full_scrub(
    prior: &VerificationSession,
    fresh: &VerificationSession,
) -> Vec<ConsistencyMismatch> {
    let mut mismatches = Vec::new();
    for (scope, prior_tracker, fresh_tracker) in [
        (
            ConsistencyScope::AuthoritativeSnapshot,
            &prior.authoritative,
            &fresh.authoritative,
        ),
        (
            ConsistencyScope::ShadowMaterializer,
            &prior.shadow,
            &fresh.shadow,
        ),
        (
            ConsistencyScope::EmbeddedReplica,
            &prior.embedded_replica,
            &fresh.embedded_replica,
        ),
    ] {
        let (Some(prior_position), Some(fresh_position)) =
            (prior_tracker.position(), fresh_tracker.position())
        else {
            continue;
        };
        if prior_position.applied_sequence() == fresh_position.applied_sequence()
            && prior_position != fresh_position
        {
            mismatches.push(ConsistencyMismatch {
                invariant: "full_scrub_matches_incremental_root".to_string(),
                left_scope: ConsistencyScope::AuthoritativeSnapshot,
                right_scope: scope,
                path: "verification_position".to_string(),
                left_description: describe_tracker(fresh_tracker),
                right_description: describe_tracker(prior_tracker),
            });
        }
    }
    mismatches
}

fn compare_session_roots(session: &VerificationSession) -> Vec<ConsistencyMismatch> {
    let mut mismatches = Vec::new();
    if session.shadow.position() != session.authoritative.position() {
        mismatches.push(root_mismatch(
            ConsistencyScope::AuthoritativeSnapshot,
            &session.authoritative,
            ConsistencyScope::ShadowMaterializer,
            &session.shadow,
        ));
    }
    if session.embedded_replica.position() != session.authoritative.position() {
        mismatches.push(root_mismatch(
            ConsistencyScope::AuthoritativeSnapshot,
            &session.authoritative,
            ConsistencyScope::EmbeddedReplica,
            &session.embedded_replica,
        ));
    }
    mismatches
}

fn root_mismatch(
    left_scope: ConsistencyScope,
    left: &MaterializedVerificationTracker,
    right_scope: ConsistencyScope,
    right: &MaterializedVerificationTracker,
) -> ConsistencyMismatch {
    ConsistencyMismatch {
        invariant: "incremental_verification_roots_match".to_string(),
        left_scope,
        right_scope,
        path: "verification_position".to_string(),
        left_description: describe_tracker(left),
        right_description: describe_tracker(right),
    }
}

fn describe_tracker(tracker: &MaterializedVerificationTracker) -> String {
    match tracker.position() {
        Some(position) => format!(
            "sequence {} root {:02x?} (format v{})",
            position.applied_sequence().0,
            position.root_hash(),
            position.version().as_u16()
        ),
        None => "invalidated verification root".to_string(),
    }
}

fn duration_millis_u64(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn session_suffix_requires_rebuild(error: &Error) -> bool {
    match error {
        Error::InvalidInput(message) => message.contains("behind the retention floor"),
        Error::Internal(message) => {
            message.starts_with("journal stream made no progress while advancing verification")
                || message.starts_with("journal gap while advancing verification")
        }
        _ => false,
    }
}
