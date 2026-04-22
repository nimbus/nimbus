use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibsqlReplicaBarrierPath {
    Unknown,
    AlreadyCurrentCache,
    WaitedForBackgroundRefresh,
    IncrementalCatchUp,
    FullSnapshotRebuild,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibsqlReplicaRefreshCause {
    Unknown,
    CommitBarrier,
    DurableJournalReplay,
    SchemaMismatch,
    SchemaWrite,
    BootstrapExport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibsqlReplicaRefreshPath {
    Unknown,
    IncrementalCatchUp,
    FullSnapshotRebuild,
    IncrementalFallbackToSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibsqlReplicaFreshnessStats {
    pub required_sequence: SequenceNumber,
    pub local_durable_head: SequenceNumber,
    pub local_applied_sequence: SequenceNumber,
    pub refresh_needed: bool,
    pub refresh_requested: bool,
    pub refresh_inflight: bool,
    pub last_barrier_path: LibsqlReplicaBarrierPath,
    pub barrier_current_count: u64,
    pub barrier_waited_for_background_refresh_count: u64,
    pub barrier_incremental_catch_up_count: u64,
    pub barrier_full_snapshot_rebuild_count: u64,
    pub last_refresh_cause: LibsqlReplicaRefreshCause,
    pub last_refresh_path: LibsqlReplicaRefreshPath,
    pub incremental_refresh_count: u64,
    pub full_snapshot_refresh_count: u64,
    pub incremental_fallback_to_snapshot_count: u64,
    pub refresh_error_count: u64,
    pub last_refresh_duration_ms: u64,
    pub last_refresh_required_sequence: SequenceNumber,
    pub last_refresh_local_durable_head: SequenceNumber,
    pub last_refresh_applied_sequence: SequenceNumber,
    pub last_refresh_error: Option<String>,
}

pub(super) struct LibsqlReplicaFreshnessMetrics {
    requested_refresh_cause: AtomicU8,
    refresh_attempt_path: AtomicU8,
    last_barrier_path: AtomicU8,
    barrier_current_count: AtomicU64,
    barrier_waited_for_background_refresh_count: AtomicU64,
    barrier_incremental_catch_up_count: AtomicU64,
    barrier_full_snapshot_rebuild_count: AtomicU64,
    last_refresh_cause: AtomicU8,
    last_refresh_path: AtomicU8,
    incremental_refresh_count: AtomicU64,
    full_snapshot_refresh_count: AtomicU64,
    incremental_fallback_to_snapshot_count: AtomicU64,
    refresh_error_count: AtomicU64,
    last_refresh_duration_ms: AtomicU64,
    last_refresh_required_sequence: AtomicU64,
    last_refresh_local_durable_head: AtomicU64,
    last_refresh_applied_sequence: AtomicU64,
    last_refresh_error: RwLock<Option<String>>,
}

pub(super) struct ReplicaRefreshOutcome {
    pub(super) path: LibsqlReplicaRefreshPath,
    pub(super) progress: JournalProgress,
}

impl LibsqlReplicaBarrierPath {
    fn to_atomic(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::AlreadyCurrentCache => 1,
            Self::WaitedForBackgroundRefresh => 2,
            Self::IncrementalCatchUp => 3,
            Self::FullSnapshotRebuild => 4,
        }
    }

    fn from_atomic(value: u8) -> Self {
        match value {
            1 => Self::AlreadyCurrentCache,
            2 => Self::WaitedForBackgroundRefresh,
            3 => Self::IncrementalCatchUp,
            4 => Self::FullSnapshotRebuild,
            _ => Self::Unknown,
        }
    }

    pub(super) fn from_refresh_path(path: LibsqlReplicaRefreshPath) -> Self {
        match path {
            LibsqlReplicaRefreshPath::IncrementalCatchUp => Self::IncrementalCatchUp,
            LibsqlReplicaRefreshPath::FullSnapshotRebuild
            | LibsqlReplicaRefreshPath::IncrementalFallbackToSnapshot => Self::FullSnapshotRebuild,
            LibsqlReplicaRefreshPath::Unknown => Self::Unknown,
        }
    }
}

impl LibsqlReplicaRefreshCause {
    fn to_atomic(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::CommitBarrier => 1,
            Self::DurableJournalReplay => 2,
            Self::SchemaMismatch => 3,
            Self::SchemaWrite => 4,
            Self::BootstrapExport => 5,
        }
    }

    fn from_atomic(value: u8) -> Self {
        match value {
            1 => Self::CommitBarrier,
            2 => Self::DurableJournalReplay,
            3 => Self::SchemaMismatch,
            4 => Self::SchemaWrite,
            5 => Self::BootstrapExport,
            _ => Self::Unknown,
        }
    }
}

impl LibsqlReplicaRefreshPath {
    fn to_atomic(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::IncrementalCatchUp => 1,
            Self::FullSnapshotRebuild => 2,
            Self::IncrementalFallbackToSnapshot => 3,
        }
    }

    fn from_atomic(value: u8) -> Self {
        match value {
            1 => Self::IncrementalCatchUp,
            2 => Self::FullSnapshotRebuild,
            3 => Self::IncrementalFallbackToSnapshot,
            _ => Self::Unknown,
        }
    }
}

impl LibsqlReplicaFreshnessMetrics {
    pub(super) fn new() -> Self {
        Self {
            requested_refresh_cause: AtomicU8::new(LibsqlReplicaRefreshCause::Unknown.to_atomic()),
            refresh_attempt_path: AtomicU8::new(LibsqlReplicaRefreshPath::Unknown.to_atomic()),
            last_barrier_path: AtomicU8::new(LibsqlReplicaBarrierPath::Unknown.to_atomic()),
            barrier_current_count: AtomicU64::new(0),
            barrier_waited_for_background_refresh_count: AtomicU64::new(0),
            barrier_incremental_catch_up_count: AtomicU64::new(0),
            barrier_full_snapshot_rebuild_count: AtomicU64::new(0),
            last_refresh_cause: AtomicU8::new(LibsqlReplicaRefreshCause::Unknown.to_atomic()),
            last_refresh_path: AtomicU8::new(LibsqlReplicaRefreshPath::Unknown.to_atomic()),
            incremental_refresh_count: AtomicU64::new(0),
            full_snapshot_refresh_count: AtomicU64::new(0),
            incremental_fallback_to_snapshot_count: AtomicU64::new(0),
            refresh_error_count: AtomicU64::new(0),
            last_refresh_duration_ms: AtomicU64::new(0),
            last_refresh_required_sequence: AtomicU64::new(0),
            last_refresh_local_durable_head: AtomicU64::new(0),
            last_refresh_applied_sequence: AtomicU64::new(0),
            last_refresh_error: RwLock::new(None),
        }
    }

    pub(super) fn note_refresh_request(&self, cause: LibsqlReplicaRefreshCause) {
        self.requested_refresh_cause
            .store(cause.to_atomic(), Ordering::Release);
    }

    pub(super) fn requested_refresh_cause(&self) -> LibsqlReplicaRefreshCause {
        LibsqlReplicaRefreshCause::from_atomic(self.requested_refresh_cause.load(Ordering::Acquire))
    }

    pub(super) fn note_refresh_attempt_path(&self, path: LibsqlReplicaRefreshPath) {
        self.refresh_attempt_path
            .store(path.to_atomic(), Ordering::Release);
    }

    pub(super) fn refresh_attempt_path(&self) -> LibsqlReplicaRefreshPath {
        LibsqlReplicaRefreshPath::from_atomic(self.refresh_attempt_path.load(Ordering::Acquire))
    }

    pub(super) fn record_barrier_path(&self, path: LibsqlReplicaBarrierPath) {
        self.last_barrier_path
            .store(path.to_atomic(), Ordering::Release);
        match path {
            LibsqlReplicaBarrierPath::AlreadyCurrentCache => {
                self.barrier_current_count.fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaBarrierPath::WaitedForBackgroundRefresh => {
                self.barrier_waited_for_background_refresh_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaBarrierPath::IncrementalCatchUp => {
                self.barrier_incremental_catch_up_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaBarrierPath::FullSnapshotRebuild => {
                self.barrier_full_snapshot_rebuild_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaBarrierPath::Unknown => {}
        }
    }

    pub(super) fn record_refresh_success(
        &self,
        cause: LibsqlReplicaRefreshCause,
        outcome: &ReplicaRefreshOutcome,
        duration_ms: u64,
        required_sequence: SequenceNumber,
    ) {
        self.last_refresh_cause
            .store(cause.to_atomic(), Ordering::Release);
        self.last_refresh_path
            .store(outcome.path.to_atomic(), Ordering::Release);
        self.last_refresh_duration_ms
            .store(duration_ms, Ordering::Release);
        self.last_refresh_required_sequence
            .store(required_sequence.0, Ordering::Release);
        self.last_refresh_local_durable_head
            .store(outcome.progress.durable_head.0, Ordering::Release);
        self.last_refresh_applied_sequence
            .store(outcome.progress.applied_head.0, Ordering::Release);
        match outcome.path {
            LibsqlReplicaRefreshPath::IncrementalCatchUp => {
                self.incremental_refresh_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaRefreshPath::FullSnapshotRebuild => {
                self.full_snapshot_refresh_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaRefreshPath::IncrementalFallbackToSnapshot => {
                self.incremental_fallback_to_snapshot_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaRefreshPath::Unknown => {}
        }
        if let Ok(mut guard) = self.last_refresh_error.write() {
            *guard = None;
        }
    }

    pub(super) fn record_refresh_error(
        &self,
        cause: LibsqlReplicaRefreshCause,
        path: LibsqlReplicaRefreshPath,
        duration_ms: u64,
        required_sequence: SequenceNumber,
        local_progress: JournalProgress,
        error: &Error,
    ) {
        self.last_refresh_cause
            .store(cause.to_atomic(), Ordering::Release);
        self.last_refresh_path
            .store(path.to_atomic(), Ordering::Release);
        self.last_refresh_duration_ms
            .store(duration_ms, Ordering::Release);
        self.last_refresh_required_sequence
            .store(required_sequence.0, Ordering::Release);
        self.last_refresh_local_durable_head
            .store(local_progress.durable_head.0, Ordering::Release);
        self.last_refresh_applied_sequence
            .store(local_progress.applied_head.0, Ordering::Release);
        self.refresh_error_count.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut guard) = self.last_refresh_error.write() {
            *guard = Some(error.to_string());
        }
    }

    pub(super) fn snapshot(
        &self,
        required_sequence: SequenceNumber,
        local_progress: JournalProgress,
        refresh_needed: bool,
        refresh_requested: bool,
        refresh_inflight: bool,
    ) -> LibsqlReplicaFreshnessStats {
        let last_refresh_error = self
            .last_refresh_error
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| Some("libsql replica refresh error lock poisoned".to_string()));
        LibsqlReplicaFreshnessStats {
            required_sequence,
            local_durable_head: local_progress.durable_head,
            local_applied_sequence: local_progress.applied_head,
            refresh_needed,
            refresh_requested,
            refresh_inflight,
            last_barrier_path: LibsqlReplicaBarrierPath::from_atomic(
                self.last_barrier_path.load(Ordering::Acquire),
            ),
            barrier_current_count: self.barrier_current_count.load(Ordering::Relaxed),
            barrier_waited_for_background_refresh_count: self
                .barrier_waited_for_background_refresh_count
                .load(Ordering::Relaxed),
            barrier_incremental_catch_up_count: self
                .barrier_incremental_catch_up_count
                .load(Ordering::Relaxed),
            barrier_full_snapshot_rebuild_count: self
                .barrier_full_snapshot_rebuild_count
                .load(Ordering::Relaxed),
            last_refresh_cause: LibsqlReplicaRefreshCause::from_atomic(
                self.last_refresh_cause.load(Ordering::Acquire),
            ),
            last_refresh_path: LibsqlReplicaRefreshPath::from_atomic(
                self.last_refresh_path.load(Ordering::Acquire),
            ),
            incremental_refresh_count: self.incremental_refresh_count.load(Ordering::Relaxed),
            full_snapshot_refresh_count: self.full_snapshot_refresh_count.load(Ordering::Relaxed),
            incremental_fallback_to_snapshot_count: self
                .incremental_fallback_to_snapshot_count
                .load(Ordering::Relaxed),
            refresh_error_count: self.refresh_error_count.load(Ordering::Relaxed),
            last_refresh_duration_ms: self.last_refresh_duration_ms.load(Ordering::Acquire),
            last_refresh_required_sequence: SequenceNumber(
                self.last_refresh_required_sequence.load(Ordering::Acquire),
            ),
            last_refresh_local_durable_head: SequenceNumber(
                self.last_refresh_local_durable_head.load(Ordering::Acquire),
            ),
            last_refresh_applied_sequence: SequenceNumber(
                self.last_refresh_applied_sequence.load(Ordering::Acquire),
            ),
            last_refresh_error,
        }
    }
}
