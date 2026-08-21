use std::sync::Arc;
use std::time::Instant;

use nimbus_core::{Error, Result, SequenceNumber, TenantEventRecord, TenantId};
use nimbus_storage::{
    MaterializedVerificationMetricMode, MaterializedVerificationObservation,
    MaterializedVerificationTracker, ShadowMaterializer, ShadowMaterializerConfig, TenantStore,
};

use super::snapshot::rebuild_authoritative_snapshot;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerificationRequestMode {
    ReuseAllowed,
    ForceFullScrub,
}

#[cfg(test)]
#[derive(Default)]
struct FullScrubPause {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[cfg(test)]
static FULL_SCRUB_PAUSES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<TenantId, Arc<FullScrubPause>>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
async fn pause_before_full_scrub_for_testing(tenant_id: &TenantId) {
    let pause = FULL_SCRUB_PAUSES
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("full-scrub pause registry should not be poisoned")
        .get(tenant_id)
        .cloned();
    if let Some(pause) = pause {
        pause.entered.notify_one();
        pause.release.notified().await;
    }
}

#[cfg(test)]
pub(crate) struct FullScrubPauseHandle {
    tenant_id: TenantId,
    pause: Arc<FullScrubPause>,
}

#[cfg(test)]
impl FullScrubPauseHandle {
    pub(crate) async fn wait_until_entered(&self) {
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.pause.entered.notified(),
        )
        .await
        .expect("verification should reach the full-scrub pause");
    }

    pub(crate) fn release(self) {
        drop(self);
    }
}

#[cfg(test)]
impl Drop for FullScrubPauseHandle {
    fn drop(&mut self) {
        let mut pauses = FULL_SCRUB_PAUSES
            .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
            .lock()
            .expect("full-scrub pause registry should not be poisoned");
        if pauses
            .get(&self.tenant_id)
            .is_some_and(|armed| Arc::ptr_eq(armed, &self.pause))
        {
            pauses.remove(&self.tenant_id);
        }
        self.pause.release.notify_one();
    }
}

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
        self.verify_consistency_with_mode(tenant_id, VerificationRequestMode::ReuseAllowed)
            .await
    }

    /// Runs a full provider-state scrub even when a reusable session exists.
    pub async fn verify_consistency_full_scrub_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<ConsistencyVerificationReport> {
        self.verify_consistency_with_mode(tenant_id, VerificationRequestMode::ForceFullScrub)
            .await
    }

    /// Discards one process-local verification session without changing data.
    pub fn clear_consistency_verification_session(&self, tenant_id: &TenantId) -> bool {
        self.verification_sessions.invalidate(tenant_id)
    }

    /// Returns fixed-shape process metrics for consistency verification.
    pub fn consistency_verification_metrics(
        &self,
    ) -> nimbus_storage::MaterializedVerificationMetricsSnapshot {
        self.verification_sessions.metrics_snapshot()
    }

    async fn verify_consistency_with_mode(
        self: &Arc<Self>,
        tenant_id: TenantId,
        request_mode: VerificationRequestMode,
    ) -> Result<ConsistencyVerificationReport> {
        let request_started_at = self.monotonic_now();
        let slot = self
            .verification_sessions
            .acquire(&tenant_id, self.monotonic_now())?;
        let mut session_guard = slot.lock().await;
        // Another check for this tenant can wait on the session lock. Read
        // time after admission so an older waiter cannot move last-used time
        // backward or defer an expiry decision.
        let now = self.monotonic_now();
        let config = self.verification_sessions.config();

        let expiry_reason = session_guard
            .as_ref()
            .and_then(|session| session.expiry_reason(now, config));
        let failed_scrub_retry = session_guard
            .as_ref()
            .is_some_and(|session| session.requires_full_scrub);
        let mut report = if request_mode == VerificationRequestMode::ForceFullScrub {
            let prior_session = session_guard.take();
            self.run_full_scrub(
                tenant_id.clone(),
                ConsistencyEscalationReason::OperatorForced,
                prior_session,
                0,
                now,
                &mut session_guard,
            )
            .await
        } else if failed_scrub_retry {
            let prior_session = session_guard.take();
            self.run_full_scrub(
                tenant_id.clone(),
                ConsistencyEscalationReason::RootMismatch,
                prior_session,
                0,
                now,
                &mut session_guard,
            )
            .await
        } else if session_guard.is_none() {
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
            let prior_session = session_guard.take();
            self.run_full_scrub(
                tenant_id.clone(),
                reason,
                prior_session,
                0,
                now,
                &mut session_guard,
            )
            .await
        } else {
            self.run_incremental_check(tenant_id.clone(), now, &mut session_guard)
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
        if let Ok(completed) = report.as_mut() {
            let mode = match completed.mode {
                ConsistencyVerificationMode::FullScrub => {
                    MaterializedVerificationMetricMode::FullScrub
                }
                ConsistencyVerificationMode::Incremental => {
                    MaterializedVerificationMetricMode::Incremental
                }
            };
            let rebuilt = completed.mode == ConsistencyVerificationMode::FullScrub
                && completed.escalation_reason != Some(ConsistencyEscalationReason::ColdStart);
            self.verification_sessions
                .record_verification(MaterializedVerificationObservation {
                    mode,
                    duration: self
                        .monotonic_now()
                        .saturating_duration_since(request_started_at),
                    verified_leaves: completed.authoritative_root.leaf_count,
                    rebuilt,
                    mismatch_count: completed.mismatches.len(),
                });
            completed.metrics = self.verification_sessions.metrics_snapshot();
        }
        report
    }

    async fn run_incremental_check(
        self: &Arc<Self>,
        tenant_id: TenantId,
        now: Instant,
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
        mut prior_session: Option<VerificationSession>,
        mut event_count: u64,
        now: Instant,
        session_guard: &mut Option<VerificationSession>,
    ) -> Result<ConsistencyVerificationReport> {
        let mut escalation_reason = reason;
        #[cfg(test)]
        pause_before_full_scrub_for_testing(&tenant_id).await;
        let bootstrap = self
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await?;

        // The bootstrap snapshot owns the full-scrub comparison cut. Align an
        // expected session to that captured applied head, not to a separate
        // metadata read that a concurrent commit can overtake.
        let mut prior_is_comparable = false;
        if let Some(prior) = prior_session.as_mut()
            && let Some(current) = prior.applied_sequence()
            && current <= bootstrap.snapshot.applied_sequence.0
        {
            let target = bootstrap.snapshot.applied_sequence;
            let records = if current == target.0 {
                Some(Vec::new())
            } else {
                match self
                    .read_contiguous_applied_suffix(&tenant_id, SequenceNumber(current), target)
                    .await
                {
                    Ok(records) => Some(records),
                    Err(error) if session_suffix_requires_rebuild(&error) => {
                        escalation_reason = ConsistencyEscalationReason::RetentionGap;
                        None
                    }
                    Err(error) => return Err(error),
                }
            };
            if let Some(records) = records {
                event_count = event_count.saturating_add(records.len() as u64);
                if let FastPathOutcome::Escalate(alignment_reason) = prior.apply_records(&records) {
                    escalation_reason = alignment_reason;
                }
                prior_is_comparable = prior.applied_sequence() == Some(target.0);
            }
        }

        // Keep the prior cross-implementation replay check. The anchor below
        // remains actual provider state at the applied head; this replay only
        // checks how the three materializers interpret the captured durable
        // tail through the bootstrap cut.
        let journal_tail = self
            .read_durable_journal_suffix_to_sequence_async(&tenant_id, &bootstrap)
            .await?;
        let replayed_authoritative = rebuild_authoritative_snapshot(&bootstrap, &journal_tail)?;
        let replayed_shadow = ShadowMaterializer::from_checkpoint_and_journal(
            bootstrap.snapshot.clone(),
            journal_tail.clone(),
            ShadowMaterializerConfig::default(),
        )?
        .current_snapshot();
        let replayed_replica = EmbeddedReplica::bootstrap_from_bootstrap(
            tenant_id.clone(),
            TenantStore::create_in_memory()?,
            bootstrap.clone(),
            journal_tail,
        )?
        .export_materialized_journal_snapshot()?;

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
            &replayed_authoritative,
            ConsistencyScope::ShadowMaterializer,
            &replayed_shadow,
        )? {
            mismatches.push(mismatch);
        }
        if let Some(mismatch) = compare_materialized_journal_snapshots(
            ConsistencyScope::AuthoritativeSnapshot,
            &replayed_authoritative,
            ConsistencyScope::EmbeddedReplica,
            &replayed_replica,
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
            requires_full_scrub: false,
        };

        let prior_was_consistent = prior_session
            .as_ref()
            .is_some_and(VerificationSession::positions_match);

        if let Some(prior) = prior_session.as_ref() {
            if escalation_reason == ConsistencyEscalationReason::RootMismatch {
                mismatches.extend(compare_session_roots(prior));
            }
            if prior_is_comparable {
                mismatches.extend(compare_prior_roots_to_full_scrub(prior, &fresh_session));
            }
        }
        mismatches.extend(compare_session_roots(&fresh_session));

        let report = report_from_session(
            &tenant_id,
            &fresh_session,
            ConsistencyVerificationMode::FullScrub,
            Some(escalation_reason),
            event_count,
            now,
            mismatches.clone(),
        )?;
        if mismatches.is_empty() {
            *session_guard = Some(fresh_session);
        } else {
            // A failed scrub must never become a warm success. Keep a
            // consistent prior witness when it disagrees with provider state;
            // otherwise keep the clean rebuilt witness. The marker forces the
            // next request through another full scrub.
            let mut retained = if prior_was_consistent {
                prior_session.expect("a consistent prior witness must exist")
            } else {
                fresh_session
            };
            retained.requires_full_scrub = true;
            *session_guard = Some(retained);
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
            .acquire(tenant_id, self.monotonic_now())
            .expect("test must admit the established verification session");
        let mut guard = slot.lock().await;
        guard
            .as_mut()
            .expect("test must establish a verification session first")
            .corrupt_shadow_for_testing();
    }

    #[cfg(test)]
    pub(crate) async fn tamper_materialized_document_for_testing(
        self: &Arc<Self>,
        tenant_id: &TenantId,
        document: nimbus_core::Document,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant_async(tenant_id).await?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.tamper_document_for_testing(document)
    }

    #[cfg(test)]
    pub(crate) fn pause_before_full_scrub_for_testing(
        &self,
        tenant_id: TenantId,
    ) -> FullScrubPauseHandle {
        let pause = Arc::new(FullScrubPause::default());
        let previous = FULL_SCRUB_PAUSES
            .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
            .lock()
            .expect("full-scrub pause registry should not be poisoned")
            .insert(tenant_id.clone(), pause.clone());
        assert!(previous.is_none(), "full-scrub pause already armed");
        FullScrubPauseHandle { tenant_id, pause }
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
        metrics: nimbus_storage::MaterializedVerificationMetricsSnapshot::default(),
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
