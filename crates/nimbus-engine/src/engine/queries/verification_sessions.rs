use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use nimbus_core::{Error, Result, TenantEventRecord, TenantId};
use nimbus_storage::{
    MaterializedDeltaApplyOutcome, MaterializedVerificationMetrics,
    MaterializedVerificationMetricsSnapshot, MaterializedVerificationObservation,
    MaterializedVerificationTracker,
};
use tokio::sync::Mutex as AsyncMutex;

use crate::verification::{BootstrapFingerprint, ConsistencyEscalationReason, SnapshotFingerprint};

const DEFAULT_MAX_SESSIONS: usize = 64;
const DEFAULT_MAX_RESIDENT_INDEX_BYTES: usize = 256 * 1024 * 1024;
const DEFAULT_MAX_IDLE: Duration = Duration::from_secs(5 * 60);
const DEFAULT_MAX_ANCHOR_AGE: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Copy)]
pub(super) struct VerificationSessionConfig {
    max_sessions: usize,
    max_resident_index_bytes: usize,
    max_idle: Duration,
    max_anchor_age: Duration,
}

impl Default for VerificationSessionConfig {
    fn default() -> Self {
        Self {
            max_sessions: DEFAULT_MAX_SESSIONS,
            // The IMV2 repeated-verification gate caps extra peak RSS at
            // 256 MiB. Session admission uses the same ceiling for the three
            // storage-owned indexes and never retains three document copies.
            max_resident_index_bytes: DEFAULT_MAX_RESIDENT_INDEX_BYTES,
            // The approved check interval is one minute. Five missed checks
            // retire an idle session; fifteen checks force a new full anchor.
            max_idle: DEFAULT_MAX_IDLE,
            max_anchor_age: DEFAULT_MAX_ANCHOR_AGE,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct FullScrubEvidence {
    pub authoritative: SnapshotFingerprint,
    pub shadow: SnapshotFingerprint,
    pub embedded_replica: SnapshotFingerprint,
    pub bootstrap: BootstrapFingerprint,
}

#[derive(Debug, Clone)]
pub(super) struct VerificationSession {
    pub anchor_started_at: Instant,
    pub last_used_at: Instant,
    pub evidence: FullScrubEvidence,
    pub authoritative: MaterializedVerificationTracker,
    pub shadow: MaterializedVerificationTracker,
    pub embedded_replica: MaterializedVerificationTracker,
    pub requires_full_scrub: bool,
}

impl VerificationSession {
    pub fn expiry_reason(
        &self,
        now: Instant,
        config: VerificationSessionConfig,
    ) -> Option<ConsistencyEscalationReason> {
        if now.saturating_duration_since(self.anchor_started_at) >= config.max_anchor_age {
            return Some(ConsistencyEscalationReason::AnchorExpired);
        }
        if now.saturating_duration_since(self.last_used_at) >= config.max_idle {
            return Some(ConsistencyEscalationReason::IdleSessionExpired);
        }
        None
    }

    pub fn applied_sequence(&self) -> Option<u64> {
        let authoritative = self.authoritative.position()?.applied_sequence().0;
        let shadow = self.shadow.position()?.applied_sequence().0;
        let replica = self.embedded_replica.position()?.applied_sequence().0;
        (authoritative == shadow && authoritative == replica).then_some(authoritative)
    }

    pub fn positions_match(&self) -> bool {
        let Some(authoritative) = self.authoritative.position() else {
            return false;
        };
        self.shadow.position() == Some(authoritative)
            && self.embedded_replica.position() == Some(authoritative)
    }

    pub fn apply_records(&mut self, records: &[TenantEventRecord]) -> FastPathOutcome {
        let mut roots_mismatched = false;
        for record in records {
            for tracker in [
                &mut self.authoritative,
                &mut self.shadow,
                &mut self.embedded_replica,
            ] {
                if matches!(
                    tracker.apply_applied_record(record),
                    MaterializedDeltaApplyOutcome::Invalidated
                ) {
                    return FastPathOutcome::Escalate(
                        ConsistencyEscalationReason::IndexInvalidated,
                    );
                }
            }
            if !self.positions_match() {
                roots_mismatched = true;
            }
        }
        if roots_mismatched {
            FastPathOutcome::Escalate(ConsistencyEscalationReason::RootMismatch)
        } else {
            FastPathOutcome::Applied
        }
    }

    pub fn resident_index_bytes(&self) -> usize {
        self.authoritative
            .resident_bytes()
            .saturating_add(self.shadow.resident_bytes())
            .saturating_add(self.embedded_replica.resident_bytes())
    }

    #[cfg(test)]
    pub fn corrupt_shadow_for_testing(&mut self) {
        let sequence = self
            .authoritative
            .position()
            .expect("test session should have an authoritative position")
            .applied_sequence();
        let empty = nimbus_storage::MaterializedJournalSnapshot {
            version: nimbus_storage::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
            applied_sequence: sequence,
            durable_head: sequence,
            table_identities: Vec::new(),
            schema: nimbus_core::Schema::default(),
            documents: Vec::new(),
            resource_path_bindings: Vec::new(),
            scheduled_execution_ids: Vec::new(),
            trigger_delivery_cursor: nimbus_core::TriggerDeliveryCursor::default(),
        };
        self.shadow = MaterializedVerificationTracker::from_snapshot(&empty)
            .expect("empty test snapshot should build a tracker");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FastPathOutcome {
    Applied,
    Escalate(ConsistencyEscalationReason),
}

#[derive(Debug)]
struct RegistryEntry {
    slot: Arc<AsyncMutex<Option<VerificationSession>>>,
    resident_index_bytes: usize,
    last_used_at: Instant,
    access_order: u64,
}

#[derive(Debug, Default)]
struct RegistryState {
    entries: HashMap<TenantId, RegistryEntry>,
    next_access_order: u64,
}

#[derive(Debug)]
pub(crate) struct VerificationSessionRegistry {
    state: StdMutex<RegistryState>,
    config: VerificationSessionConfig,
    metrics: MaterializedVerificationMetrics,
}

impl Default for VerificationSessionRegistry {
    fn default() -> Self {
        Self::new(VerificationSessionConfig::default())
    }
}

impl VerificationSessionRegistry {
    fn new(config: VerificationSessionConfig) -> Self {
        Self {
            state: StdMutex::new(RegistryState::default()),
            config,
            metrics: MaterializedVerificationMetrics::default(),
        }
    }

    pub(super) fn config(&self) -> VerificationSessionConfig {
        self.config
    }

    pub(super) fn acquire(
        &self,
        tenant_id: &TenantId,
        now: Instant,
    ) -> Result<Arc<AsyncMutex<Option<VerificationSession>>>> {
        let mut state = self
            .state
            .lock()
            .expect("verification session registry lock should not be poisoned");
        let access_order = state.next_access_order;
        state.next_access_order = state.next_access_order.wrapping_add(1);
        if let Some(entry) = state.entries.get_mut(tenant_id) {
            entry.last_used_at = now;
            entry.access_order = access_order;
            return Ok(Arc::clone(&entry.slot));
        }

        let mut evictions = 0;
        while state.entries.len() >= self.config.max_sessions {
            let candidate = state
                .entries
                .iter()
                .filter(|(_, entry)| Arc::strong_count(&entry.slot) == 1)
                .min_by_key(|(_, entry)| (entry.access_order, entry.last_used_at))
                .map(|(tenant_id, _)| tenant_id.clone());
            let Some(candidate) = candidate else {
                return Err(Error::ResourceExhausted(format!(
                    "materialized verification has {} active sessions; the limit is {}",
                    state.entries.len(),
                    self.config.max_sessions
                )));
            };
            state.entries.remove(&candidate);
            evictions += 1;
        }

        let slot = Arc::new(AsyncMutex::new(None));
        state.entries.insert(
            tenant_id.clone(),
            RegistryEntry {
                slot: Arc::clone(&slot),
                resident_index_bytes: 0,
                last_used_at: now,
                access_order,
            },
        );
        evictions += self.evict_to_bounds(&mut state, tenant_id);
        self.metrics.record_evictions(evictions);
        self.record_registry_usage(&state);
        Ok(slot)
    }

    pub(super) fn record_usage(
        &self,
        tenant_id: &TenantId,
        slot: &Arc<AsyncMutex<Option<VerificationSession>>>,
        resident_index_bytes: usize,
        now: Instant,
    ) {
        let mut state = self
            .state
            .lock()
            .expect("verification session registry lock should not be poisoned");
        let access_order = state.next_access_order;
        state.next_access_order = state.next_access_order.wrapping_add(1);
        let Some(entry) = state.entries.get_mut(tenant_id) else {
            return;
        };
        if !Arc::ptr_eq(&entry.slot, slot) {
            return;
        }
        entry.resident_index_bytes = resident_index_bytes;
        entry.last_used_at = now;
        entry.access_order = access_order;

        if resident_index_bytes > self.config.max_resident_index_bytes {
            state.entries.remove(tenant_id);
            self.metrics.record_evictions(1);
            self.record_registry_usage(&state);
            return;
        }
        let evictions = self.evict_to_bounds(&mut state, tenant_id);
        self.metrics.record_evictions(evictions);
        self.record_registry_usage(&state);
    }

    pub(crate) fn invalidate(&self, tenant_id: &TenantId) -> bool {
        let mut state = self
            .state
            .lock()
            .expect("verification session registry lock should not be poisoned");
        let removed = state.entries.remove(tenant_id).is_some();
        self.record_registry_usage(&state);
        removed
    }

    pub(super) fn record_verification(&self, observation: MaterializedVerificationObservation) {
        self.metrics.record(observation);
    }

    pub(crate) fn metrics_snapshot(&self) -> MaterializedVerificationMetricsSnapshot {
        self.metrics.snapshot()
    }

    fn evict_to_bounds(&self, state: &mut RegistryState, protected_tenant: &TenantId) -> usize {
        let mut evictions = 0;
        loop {
            let total_bytes = state.entries.values().fold(0usize, |total, entry| {
                total.saturating_add(entry.resident_index_bytes)
            });
            if state.entries.len() <= self.config.max_sessions
                && total_bytes <= self.config.max_resident_index_bytes
            {
                break;
            }

            let candidate = state
                .entries
                .iter()
                .filter(|(tenant_id, entry)| {
                    *tenant_id != protected_tenant && Arc::strong_count(&entry.slot) == 1
                })
                .min_by_key(|(_, entry)| (entry.access_order, entry.last_used_at))
                .map(|(tenant_id, _)| tenant_id.clone());
            let Some(candidate) = candidate else {
                if state.entries.remove(protected_tenant).is_some() {
                    evictions += 1;
                }
                break;
            };
            state.entries.remove(&candidate);
            evictions += 1;
        }
        evictions
    }

    fn record_registry_usage(&self, state: &RegistryState) {
        let resident_index_bytes = state.entries.values().fold(0usize, |total, entry| {
            total.saturating_add(entry.resident_index_bytes)
        });
        self.metrics
            .set_registry_usage(state.entries.len(), resident_index_bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_session(now: Instant) -> VerificationSession {
        let snapshot = nimbus_storage::MaterializedJournalSnapshot {
            version: nimbus_storage::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
            applied_sequence: nimbus_core::SequenceNumber(0),
            durable_head: nimbus_core::SequenceNumber(0),
            table_identities: Vec::new(),
            schema: nimbus_core::Schema::default(),
            documents: Vec::new(),
            resource_path_bindings: Vec::new(),
            scheduled_execution_ids: Vec::new(),
            trigger_delivery_cursor: nimbus_core::TriggerDeliveryCursor::default(),
        };
        let bootstrap = nimbus_storage::DurableJournalBootstrap {
            snapshot: snapshot.clone(),
            resume_after: nimbus_core::SequenceNumber(0),
            bootstrap_cut: nimbus_core::SequenceNumber(0),
            cursor_floor: nimbus_core::SequenceNumber(0),
        };
        let tracker = MaterializedVerificationTracker::from_snapshot(&snapshot)
            .expect("empty snapshot should build a tracker");
        VerificationSession {
            anchor_started_at: now,
            last_used_at: now,
            evidence: FullScrubEvidence {
                authoritative: crate::verification::snapshot_fingerprint(&snapshot)
                    .expect("snapshot should fingerprint"),
                shadow: crate::verification::snapshot_fingerprint(&snapshot)
                    .expect("snapshot should fingerprint"),
                embedded_replica: crate::verification::snapshot_fingerprint(&snapshot)
                    .expect("snapshot should fingerprint"),
                bootstrap: crate::verification::bootstrap_fingerprint(&bootstrap)
                    .expect("bootstrap should fingerprint"),
            },
            authoritative: tracker.clone(),
            shadow: tracker.clone(),
            embedded_replica: tracker,
            requires_full_scrub: false,
        }
    }

    #[test]
    fn bounded_sessions_evict_least_recently_used() {
        let config = VerificationSessionConfig {
            max_sessions: 2,
            max_resident_index_bytes: usize::MAX,
            max_idle: Duration::MAX,
            max_anchor_age: Duration::MAX,
        };
        let registry = VerificationSessionRegistry::new(config);
        let now = Instant::now();
        let tenant_a = TenantId::new("tenant-a").expect("tenant should validate");
        let tenant_b = TenantId::new("tenant-b").expect("tenant should validate");
        let tenant_c = TenantId::new("tenant-c").expect("tenant should validate");

        drop(
            registry
                .acquire(&tenant_a, now)
                .expect("tenant A should admit"),
        );
        drop(
            registry
                .acquire(&tenant_b, now)
                .expect("tenant B should admit"),
        );
        drop(
            registry
                .acquire(&tenant_b, now)
                .expect("tenant B should reuse"),
        );
        drop(
            registry
                .acquire(&tenant_c, now)
                .expect("tenant C should admit"),
        );

        let state = registry.state.lock().expect("registry should lock");
        assert!(!state.entries.contains_key(&tenant_a));
        assert!(state.entries.contains_key(&tenant_b));
        assert!(state.entries.contains_key(&tenant_c));
        drop(state);
        let metrics = registry.metrics_snapshot();
        assert_eq!(metrics.sessions_current, 2);
        assert_eq!(metrics.evictions_total, 1);
    }

    #[test]
    fn verification_session_byte_budget_evicts_least_recently_used() {
        let config = VerificationSessionConfig {
            max_sessions: usize::MAX,
            max_resident_index_bytes: 100,
            max_idle: Duration::MAX,
            max_anchor_age: Duration::MAX,
        };
        let registry = VerificationSessionRegistry::new(config);
        let now = Instant::now();
        let tenant_a = TenantId::new("tenant-a").expect("tenant should validate");
        let tenant_b = TenantId::new("tenant-b").expect("tenant should validate");

        let slot_a = registry
            .acquire(&tenant_a, now)
            .expect("tenant A should admit");
        registry.record_usage(&tenant_a, &slot_a, 60, now);
        drop(slot_a);
        let slot_b = registry
            .acquire(&tenant_b, now)
            .expect("tenant B should admit");
        registry.record_usage(&tenant_b, &slot_b, 60, now);

        let state = registry.state.lock().expect("registry should lock");
        assert!(!state.entries.contains_key(&tenant_a));
        assert!(state.entries.contains_key(&tenant_b));
        drop(state);
        let metrics = registry.metrics_snapshot();
        assert_eq!(metrics.sessions_current, 1);
        assert_eq!(metrics.resident_index_bytes_current, 60);
        assert_eq!(metrics.resident_index_bytes_peak, 60);
        assert_eq!(metrics.evictions_total, 1);
    }

    #[test]
    fn verification_session_limit_refuses_when_every_slot_is_active() {
        let config = VerificationSessionConfig {
            max_sessions: 1,
            max_resident_index_bytes: usize::MAX,
            max_idle: Duration::MAX,
            max_anchor_age: Duration::MAX,
        };
        let registry = VerificationSessionRegistry::new(config);
        let now = Instant::now();
        let tenant_a = TenantId::new("tenant-a").expect("tenant should validate");
        let tenant_b = TenantId::new("tenant-b").expect("tenant should validate");

        let active = registry
            .acquire(&tenant_a, now)
            .expect("first tenant should admit");
        let error = registry
            .acquire(&tenant_b, now)
            .expect_err("a second active tenant must not exceed the count bound");

        assert!(matches!(error, Error::ResourceExhausted(_)));
        assert_eq!(registry.metrics_snapshot().sessions_current, 1);
        drop(active);
    }

    #[test]
    fn verification_session_byte_limit_does_not_retain_an_over_budget_active_result() {
        let config = VerificationSessionConfig {
            max_sessions: usize::MAX,
            max_resident_index_bytes: 100,
            max_idle: Duration::MAX,
            max_anchor_age: Duration::MAX,
        };
        let registry = VerificationSessionRegistry::new(config);
        let now = Instant::now();
        let tenant_a = TenantId::new("tenant-a").expect("tenant should validate");
        let tenant_b = TenantId::new("tenant-b").expect("tenant should validate");

        let slot_a = registry
            .acquire(&tenant_a, now)
            .expect("tenant A should admit");
        registry.record_usage(&tenant_a, &slot_a, 60, now);
        let slot_b = registry
            .acquire(&tenant_b, now)
            .expect("tenant B should admit");
        registry.record_usage(&tenant_b, &slot_b, 60, now);

        let state = registry.state.lock().expect("registry should lock");
        assert!(state.entries.contains_key(&tenant_a));
        assert!(!state.entries.contains_key(&tenant_b));
        drop(state);
        let metrics = registry.metrics_snapshot();
        assert_eq!(metrics.sessions_current, 1);
        assert_eq!(metrics.resident_index_bytes_current, 60);
        assert_eq!(metrics.evictions_total, 1);
    }

    #[test]
    fn verification_session_expires_by_idle_time_and_anchor_age() {
        let now = Instant::now();
        let session = empty_session(now);
        let config = VerificationSessionConfig {
            max_sessions: usize::MAX,
            max_resident_index_bytes: usize::MAX,
            max_idle: Duration::from_secs(5),
            max_anchor_age: Duration::from_secs(15),
        };
        assert_eq!(
            session.expiry_reason(now + Duration::from_secs(5), config),
            Some(ConsistencyEscalationReason::IdleSessionExpired)
        );

        let mut recently_used = session;
        recently_used.last_used_at = now + Duration::from_secs(14);
        assert_eq!(
            recently_used.expiry_reason(now + Duration::from_secs(15), config),
            Some(ConsistencyEscalationReason::AnchorExpired)
        );
    }
}
