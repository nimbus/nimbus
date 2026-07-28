use super::encryption::{apply_encryption_key, harden_temp_storage, verify_encryption_key};
use super::*;

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum SqliteWriteStatementConcept {
    JournalNextSequenceRead,
    JournalInsert,
    NextSequenceWrite,
    AppliedSequenceRead,
    AppliedSequenceWrite,
    DurableRecordRead,
    DocumentVersionFormatRead,
    DocumentVersionFormatWrite,
    DocumentVersionInsert,
    IndexSchemaRead,
    IndexVersionFormatRead,
    IndexVersionFormatWrite,
    IndexVersionClose,
    IndexVersionOpen,
    TableIdentityCheck,
    DocumentPreimageRead,
    LiveDocumentInsert,
    LiveDocumentUpdate,
    LiveDocumentDelete,
    ResourceBindingUpsert,
    ResourceBindingDelete,
}

#[cfg(test)]
impl SqliteWriteStatementConcept {
    const COUNT: usize = 21;
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SqliteWriteTestObservationSnapshot {
    pub writer_opens: u64,
    pub format_checks: u64,
    pub schema_checks: u64,
    pub table_identity_checks: u64,
    pub current_document_encodes: u64,
    statement_prepares: [u64; SqliteWriteStatementConcept::COUNT],
    statement_executes: [u64; SqliteWriteStatementConcept::COUNT],
}

#[cfg(test)]
impl Default for SqliteWriteTestObservationSnapshot {
    fn default() -> Self {
        Self {
            writer_opens: 0,
            format_checks: 0,
            schema_checks: 0,
            table_identity_checks: 0,
            current_document_encodes: 0,
            statement_prepares: [0; SqliteWriteStatementConcept::COUNT],
            statement_executes: [0; SqliteWriteStatementConcept::COUNT],
        }
    }
}

#[cfg(test)]
impl SqliteWriteTestObservationSnapshot {
    pub fn statement_prepares(&self, concept: SqliteWriteStatementConcept) -> u64 {
        self.statement_prepares[concept as usize]
    }

    pub fn statement_executes(&self, concept: SqliteWriteStatementConcept) -> u64 {
        self.statement_executes[concept as usize]
    }
}

#[cfg(test)]
#[derive(Default)]
struct SqliteWriteTestObservationState {
    target_path: Option<PathBuf>,
    snapshot: SqliteWriteTestObservationSnapshot,
    /// Concepts already used on the current writer connection. A fresh
    /// connection starts with an empty SQLite statement cache, so the first
    /// use of a concept per connection is the prepare bound SWT1 optimizes;
    /// later uses hit the connection's prepared-statement cache.
    cache_seen: [bool; SqliteWriteStatementConcept::COUNT],
}

#[cfg(test)]
static SQLITE_WRITE_TEST_OBSERVATION: std::sync::LazyLock<Mutex<SqliteWriteTestObservationState>> =
    std::sync::LazyLock::new(|| Mutex::new(SqliteWriteTestObservationState::default()));

#[cfg(test)]
fn lock_sqlite_write_test_observation() -> MutexGuard<'static, SqliteWriteTestObservationState> {
    SQLITE_WRITE_TEST_OBSERVATION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
pub(super) fn reset_sqlite_write_test_observation(path: &Path) {
    let mut observation = lock_sqlite_write_test_observation();
    observation.target_path = Some(path.to_path_buf());
    observation.snapshot = SqliteWriteTestObservationSnapshot::default();
    observation.cache_seen = [false; SqliteWriteStatementConcept::COUNT];
}

#[cfg(test)]
pub(super) fn sqlite_write_test_observation_snapshot(
    path: &Path,
) -> SqliteWriteTestObservationSnapshot {
    let observation = lock_sqlite_write_test_observation();
    if observation.target_path.as_deref() == Some(path) {
        observation.snapshot.clone()
    } else {
        SqliteWriteTestObservationSnapshot::default()
    }
}

#[cfg(test)]
pub(super) fn observe_sqlite_writer_open(path: &Path) {
    let mut observation = lock_sqlite_write_test_observation();
    if observation.target_path.as_deref() == Some(path) {
        observation.snapshot.writer_opens = observation.snapshot.writer_opens.saturating_add(1);
        // A new connection starts with an empty prepared-statement cache.
        observation.cache_seen = [false; SqliteWriteStatementConcept::COUNT];
    }
}

/// Records one execution of a cached statement: the execute counter always
/// advances, while the prepare counter advances only on the concept's first
/// use since the current writer connection opened. This is an upper bound on
/// real SQL parses because several concepts share one statement text and
/// therefore one prepared-statement cache entry.
#[cfg(test)]
pub(super) fn observe_sqlite_cached_statement(path: &Path, concept: SqliteWriteStatementConcept) {
    let mut observation = lock_sqlite_write_test_observation();
    if observation.target_path.as_deref() != Some(path) {
        return;
    }
    let index = concept as usize;
    if !observation.cache_seen[index] {
        observation.cache_seen[index] = true;
        observation.snapshot.statement_prepares[index] =
            observation.snapshot.statement_prepares[index].saturating_add(1);
    }
    observation.snapshot.statement_executes[index] =
        observation.snapshot.statement_executes[index].saturating_add(1);
}

#[cfg(test)]
pub(super) fn observe_sqlite_format_check(path: &Path) {
    let mut observation = lock_sqlite_write_test_observation();
    if observation.target_path.as_deref() == Some(path) {
        observation.snapshot.format_checks = observation.snapshot.format_checks.saturating_add(1);
    }
}

#[cfg(test)]
pub(super) fn observe_sqlite_schema_check(path: &Path) {
    let mut observation = lock_sqlite_write_test_observation();
    if observation.target_path.as_deref() == Some(path) {
        observation.snapshot.schema_checks = observation.snapshot.schema_checks.saturating_add(1);
    }
}

#[cfg(test)]
pub(super) fn observe_sqlite_table_identity_check(path: &Path) {
    let mut observation = lock_sqlite_write_test_observation();
    if observation.target_path.as_deref() == Some(path) {
        observation.snapshot.table_identity_checks =
            observation.snapshot.table_identity_checks.saturating_add(1);
    }
}

#[cfg(test)]
pub(super) fn observe_sqlite_current_document_encode(path: &Path) {
    let mut observation = lock_sqlite_write_test_observation();
    if observation.target_path.as_deref() == Some(path) {
        observation.snapshot.current_document_encodes = observation
            .snapshot
            .current_document_encodes
            .saturating_add(1);
    }
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct SqliteWalCheckpointObservationSnapshot {
    pub foreground_commit_count: u64,
    pub foreground_commit_nanos: u64,
    /// Post-COMMIT samples whose WAL frame count was at or beyond the
    /// connection's automatic-checkpoint threshold.
    ///
    /// This is sampled WAL state, not proven per-commit attribution: the probe
    /// runs after COMMIT releases the writer lock, so attribution to the
    /// sampled commit is exact only while writers to this database are
    /// externally serialized. Per-tenant Engine commits are serialized by the
    /// tenant committer, which holds for the canonical benchmark workloads;
    /// concurrent non-committer writers (object manifests, replica
    /// reconciliation) can shift a sample onto an adjacent commit. Threshold
    /// crossings observed by this path's own writers remain aggregate-accurate.
    pub automatic_checkpoint_count: u64,
    /// Total commit duration for the sampled threshold-crossing commits. An
    /// upper bound twice over: SQLite does not expose the checkpoint-only
    /// portion of COMMIT, and attribution is sampled as described on
    /// `automatic_checkpoint_count`.
    pub automatic_checkpoint_commit_upper_bound_nanos: u64,
    pub wal_high_water_frames: u64,
    pub checkpointed_high_water_frames: u64,
    pub auto_checkpoint_pages: u64,
    pub observation_probe_count: u64,
    pub observation_probe_nanos: u64,
    pub observation_probe_error_count: u64,
    pub post_run_passive_probe_count: u64,
    pub post_run_passive_probe_nanos: u64,
    pub post_run_passive_busy: u64,
    pub post_run_passive_wal_frames: u64,
    pub post_run_passive_checkpointed_frames: u64,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct SqlitePassiveCheckpointProbe {
    pub busy: u64,
    pub wal_frames: u64,
    pub checkpointed_frames: u64,
    pub elapsed_nanos: u64,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Default)]
struct SqliteWalCheckpointObservationState {
    generation: u64,
    target_path: Option<PathBuf>,
    snapshot: SqliteWalCheckpointObservationSnapshot,
}

#[cfg(any(test, feature = "test-hooks"))]
static SQLITE_WAL_CHECKPOINT_OBSERVATION_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(any(test, feature = "test-hooks"))]
static SQLITE_WAL_CHECKPOINT_OBSERVATION: std::sync::LazyLock<
    Mutex<SqliteWalCheckpointObservationState>,
> = std::sync::LazyLock::new(|| Mutex::new(SqliteWalCheckpointObservationState::default()));

#[cfg(any(test, feature = "test-hooks"))]
fn lock_sqlite_wal_checkpoint_observation()
-> MutexGuard<'static, SqliteWalCheckpointObservationState> {
    SQLITE_WAL_CHECKPOINT_OBSERVATION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Resets and enables the opt-in WAL/checkpoint observer for one SQLite path.
///
/// This surface is compiled only for tests and benchmark builds that enable
/// `test-hooks`. Normal release builds contain neither the counters nor the
/// post-COMMIT observation probes.
#[cfg(any(test, feature = "test-hooks"))]
pub fn reset_sqlite_wal_checkpoint_observation(path: impl AsRef<Path>) {
    let mut observation = lock_sqlite_wal_checkpoint_observation();
    observation.generation = observation.generation.wrapping_add(1);
    observation.target_path = Some(path.as_ref().to_path_buf());
    observation.snapshot = SqliteWalCheckpointObservationSnapshot::default();
    SQLITE_WAL_CHECKPOINT_OBSERVATION_ENABLED.store(true, Ordering::Release);
}

#[cfg(any(test, feature = "test-hooks"))]
pub fn disable_sqlite_wal_checkpoint_observation() {
    SQLITE_WAL_CHECKPOINT_OBSERVATION_ENABLED.store(false, Ordering::Release);
    let mut observation = lock_sqlite_wal_checkpoint_observation();
    observation.generation = observation.generation.wrapping_add(1);
    observation.target_path = None;
}

#[cfg(any(test, feature = "test-hooks"))]
pub fn sqlite_wal_checkpoint_observation_snapshot(
    path: impl AsRef<Path>,
) -> SqliteWalCheckpointObservationSnapshot {
    let observation = lock_sqlite_wal_checkpoint_observation();
    if observation.target_path.as_deref() == Some(path.as_ref()) {
        observation.snapshot
    } else {
        SqliteWalCheckpointObservationSnapshot::default()
    }
}

#[cfg(any(test, feature = "test-hooks"))]
pub(super) fn observe_sqlite_foreground_commit(
    path: &Path,
    conn: &Connection,
    commit_elapsed: Duration,
) {
    if !SQLITE_WAL_CHECKPOINT_OBSERVATION_ENABLED.load(Ordering::Acquire) {
        return;
    }
    let (generation, known_auto_checkpoint_pages) = {
        let observation = lock_sqlite_wal_checkpoint_observation();
        if observation.target_path.as_deref() != Some(path) {
            return;
        }
        (
            observation.generation,
            observation.snapshot.auto_checkpoint_pages,
        )
    };

    let probe_started = std::time::Instant::now();
    let result: Result<(u64, u64, u64)> = (|| {
        let auto_checkpoint_pages = if known_auto_checkpoint_pages == 0 {
            conn.query_row("PRAGMA wal_autocheckpoint", [], |row| row.get::<_, i64>(0))
                .map_err(map_sqlite_error)
                .and_then(|value| nonnegative_checkpoint_value("autocheckpoint pages", value))?
        } else {
            known_auto_checkpoint_pages
        };
        let (wal_frames, checkpointed_frames) = conn
            .query_row("PRAGMA wal_checkpoint(NOOP)", [], |row| {
                Ok((row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
            })
            .map_err(map_sqlite_error)?;
        Ok((
            auto_checkpoint_pages,
            nonnegative_checkpoint_value("WAL frames", wal_frames)?,
            nonnegative_checkpoint_value("checkpointed frames", checkpointed_frames)?,
        ))
    })();
    let probe_elapsed = probe_started.elapsed();

    let mut observation = lock_sqlite_wal_checkpoint_observation();
    if observation.generation != generation || observation.target_path.as_deref() != Some(path) {
        return;
    }
    let snapshot = &mut observation.snapshot;
    snapshot.foreground_commit_count = snapshot.foreground_commit_count.saturating_add(1);
    snapshot.foreground_commit_nanos = snapshot
        .foreground_commit_nanos
        .saturating_add(duration_nanos(commit_elapsed));
    snapshot.observation_probe_count = snapshot.observation_probe_count.saturating_add(1);
    snapshot.observation_probe_nanos = snapshot
        .observation_probe_nanos
        .saturating_add(duration_nanos(probe_elapsed));
    match result {
        Ok((auto_checkpoint_pages, wal_frames, checkpointed_frames)) => {
            snapshot.auto_checkpoint_pages = auto_checkpoint_pages;
            snapshot.wal_high_water_frames = snapshot.wal_high_water_frames.max(wal_frames);
            snapshot.checkpointed_high_water_frames = snapshot
                .checkpointed_high_water_frames
                .max(checkpointed_frames);
            if auto_checkpoint_pages > 0 && wal_frames >= auto_checkpoint_pages {
                snapshot.automatic_checkpoint_count =
                    snapshot.automatic_checkpoint_count.saturating_add(1);
                snapshot.automatic_checkpoint_commit_upper_bound_nanos = snapshot
                    .automatic_checkpoint_commit_upper_bound_nanos
                    .saturating_add(duration_nanos(commit_elapsed));
            }
        }
        Err(_) => {
            // A diagnostic failure after COMMIT must never turn a durable
            // success into an ambiguous business-operation result.
            snapshot.observation_probe_error_count =
                snapshot.observation_probe_error_count.saturating_add(1);
        }
    }
}

/// Runs the explicitly post-run passive checkpoint probe and records it
/// separately from automatic foreground checkpoint observations.
#[cfg(any(test, feature = "test-hooks"))]
pub fn probe_sqlite_passive_checkpoint(
    path: impl AsRef<Path>,
) -> Result<SqlitePassiveCheckpointProbe> {
    let path = path.as_ref();
    let generation = {
        let observation = lock_sqlite_wal_checkpoint_observation();
        if observation.target_path.as_deref() != Some(path) {
            return Err(Error::InvalidInput(format!(
                "SQLite WAL/checkpoint observation is not enabled for {}",
                path.display()
            )));
        }
        observation.generation
    };
    let conn = Connection::open(path).map_err(map_sqlite_error)?;
    let started = std::time::Instant::now();
    let (busy, wal_frames, checkpointed_frames) = conn
        .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(map_sqlite_error)?;
    let probe = SqlitePassiveCheckpointProbe {
        busy: nonnegative_checkpoint_value("passive checkpoint busy result", busy)?,
        wal_frames: nonnegative_checkpoint_value("passive checkpoint WAL frames", wal_frames)?,
        checkpointed_frames: nonnegative_checkpoint_value(
            "passive checkpointed frames",
            checkpointed_frames,
        )?,
        elapsed_nanos: duration_nanos(started.elapsed()),
    };

    let mut observation = lock_sqlite_wal_checkpoint_observation();
    if observation.generation == generation && observation.target_path.as_deref() == Some(path) {
        observation.snapshot.post_run_passive_probe_count = observation
            .snapshot
            .post_run_passive_probe_count
            .saturating_add(1);
        observation.snapshot.post_run_passive_probe_nanos = observation
            .snapshot
            .post_run_passive_probe_nanos
            .saturating_add(probe.elapsed_nanos);
        observation.snapshot.post_run_passive_busy = probe.busy;
        observation.snapshot.post_run_passive_wal_frames = probe.wal_frames;
        observation.snapshot.post_run_passive_checkpointed_frames = probe.checkpointed_frames;
    }
    Ok(probe)
}

#[cfg(any(test, feature = "test-hooks"))]
fn nonnegative_checkpoint_value(label: &str, value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!("SQLite {label} is negative: {value}"),
        )
    })
}

#[cfg(any(test, feature = "test-hooks"))]
fn duration_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

impl SqliteTenantStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_max_read_connections(path, default_sqlite_read_connection_limit())
    }

    pub(crate) fn open_with_max_read_connections(
        path: impl AsRef<Path>,
        max_read_connections: usize,
    ) -> Result<Self> {
        Self::open_with_simulation_and_max_read_connections(
            path,
            Arc::new(SystemWallClock),
            Arc::new(NoopFaultInjector),
            max_read_connections,
        )
    }

    pub fn open_with_simulation(
        path: impl AsRef<Path>,
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        Self::open_with_simulation_and_id_source(
            path,
            clock,
            fault_injector,
            Arc::new(SystemIdSource),
        )
    }

    pub fn open_with_simulation_and_id_source(
        path: impl AsRef<Path>,
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        id_source: Arc<dyn IdSource>,
    ) -> Result<Self> {
        Self::open_with_simulation_and_max_read_connections_and_id_source(
            path,
            clock,
            fault_injector,
            default_sqlite_read_connection_limit(),
            id_source,
        )
    }

    pub(crate) fn open_with_simulation_and_max_read_connections(
        path: impl AsRef<Path>,
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        max_read_connections: usize,
    ) -> Result<Self> {
        Self::open_with_simulation_and_max_read_connections_and_id_source(
            path,
            clock,
            fault_injector,
            max_read_connections,
            Arc::new(SystemIdSource),
        )
    }

    pub(crate) fn open_with_simulation_and_max_read_connections_and_id_source(
        path: impl AsRef<Path>,
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        max_read_connections: usize,
        id_source: Arc<dyn IdSource>,
    ) -> Result<Self> {
        Self::open_internal(
            path,
            None,
            clock,
            fault_injector,
            max_read_connections,
            id_source,
        )
    }

    /// Opens or creates an encrypted SQLite tenant store.
    ///
    /// The DEK must be a 32-byte key obtained from the key provider system.
    /// All connections will use SQLCipher encryption with this key.
    pub fn open_encrypted(path: impl AsRef<Path>, dek: &[u8; 32]) -> Result<Self> {
        Self::open_encrypted_with_max_read_connections(
            path,
            dek,
            default_sqlite_read_connection_limit(),
        )
    }

    pub(crate) fn open_encrypted_with_max_read_connections(
        path: impl AsRef<Path>,
        dek: &[u8; 32],
        max_read_connections: usize,
    ) -> Result<Self> {
        Self::open_encrypted_with_simulation_and_max_read_connections(
            path,
            dek,
            Arc::new(SystemWallClock),
            Arc::new(NoopFaultInjector),
            max_read_connections,
        )
    }

    /// Opens or creates an encrypted SQLite tenant store with simulation support.
    pub fn open_encrypted_with_simulation(
        path: impl AsRef<Path>,
        dek: &[u8; 32],
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        Self::open_encrypted_with_simulation_and_id_source(
            path,
            dek,
            clock,
            fault_injector,
            Arc::new(SystemIdSource),
        )
    }

    pub fn open_encrypted_with_simulation_and_id_source(
        path: impl AsRef<Path>,
        dek: &[u8; 32],
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        id_source: Arc<dyn IdSource>,
    ) -> Result<Self> {
        Self::open_encrypted_with_simulation_and_max_read_connections_and_id_source(
            path,
            dek,
            clock,
            fault_injector,
            default_sqlite_read_connection_limit(),
            id_source,
        )
    }

    pub(crate) fn open_encrypted_with_simulation_and_max_read_connections(
        path: impl AsRef<Path>,
        dek: &[u8; 32],
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        max_read_connections: usize,
    ) -> Result<Self> {
        Self::open_encrypted_with_simulation_and_max_read_connections_and_id_source(
            path,
            dek,
            clock,
            fault_injector,
            max_read_connections,
            Arc::new(SystemIdSource),
        )
    }

    pub(crate) fn open_encrypted_with_simulation_and_max_read_connections_and_id_source(
        path: impl AsRef<Path>,
        dek: &[u8; 32],
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        max_read_connections: usize,
        id_source: Arc<dyn IdSource>,
    ) -> Result<Self> {
        Self::open_internal(
            path,
            Some(DataEncryptionKey::new(*dek)),
            clock,
            fault_injector,
            max_read_connections,
            id_source,
        )
    }

    fn open_internal(
        path: impl AsRef<Path>,
        dek: Option<DataEncryptionKey>,
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        max_read_connections: usize,
        id_source: Arc<dyn IdSource>,
    ) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| Error::Internal(error.to_string()))?;
        }
        let store = Self {
            path,
            dek,
            clock,
            fault_injector,
            id_source,
            max_read_connections: max_read_connections.max(1),
            open_read_connections: Arc::new(AtomicUsize::new(0)),
            read_connections: Arc::new(Mutex::new(Vec::new())),
            schema_cache: Arc::new(RwLock::new(Schema::default())),
            retention_floor: RetentionFloor::new(),
        };
        let pooled_open_started = std::time::Instant::now();
        let conn = store.open_pooled_read_connection()?.ok_or_else(|| {
            Error::Internal(
                "fresh sqlite store could not reserve its first read connection".to_string(),
            )
        })?;
        let pooled_open_elapsed = pooled_open_started.elapsed();
        let schema_load_started = std::time::Instant::now();
        let schema = load_schema_from_conn(&conn)?;
        let schema_load_elapsed = schema_load_started.elapsed();
        store.replace_cached_schema(schema)?;
        if sqlite_open_profile_enabled(&store.path) {
            eprintln!(
                "sqlite-open-profile path={} encrypted={} pooled_open={:?} schema_load={:?} total={:?}",
                store.path.display(),
                store.dek.is_some(),
                pooled_open_elapsed,
                schema_load_elapsed,
                pooled_open_elapsed + schema_load_elapsed,
            );
        }
        store.lock_read_connections()?.push(conn);
        Ok(store)
    }

    /// Returns whether this store uses encryption.
    pub fn is_encrypted(&self) -> bool {
        self.dek.is_some()
    }

    pub fn max_read_connections(&self) -> usize {
        self.max_read_connections
    }

    pub fn read_snapshot(&self) -> Result<SqliteReadSnapshot> {
        let conn = self.acquire_read_connection()?;
        conn.execute_batch("BEGIN").map_err(map_sqlite_error)?;
        Ok(SqliteReadSnapshot {
            conn,
            schema_cache: self.schema_cache.clone(),
        })
    }

    pub fn begin_write_transaction(&self) -> Result<SqliteWriteTransaction> {
        self.begin_write_transaction_cancellable(|| Ok(()))
    }

    pub fn begin_write_transaction_cancellable<Check>(
        &self,
        check_cancel: Check,
    ) -> Result<SqliteWriteTransaction>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let conn = self.open_writer_connection()?;
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_sqlite_error)?;
        Ok(SqliteWriteTransaction {
            conn: Some(conn),
            #[cfg(any(test, feature = "test-hooks"))]
            observation_path: self.path.clone(),
            clock: self.clock.clone(),
            fault_injector: self.fault_injector.clone(),
            id_source: self.id_source.clone(),
            commit_writes: Vec::new(),
            tenant_events: Vec::new(),
            prepared_record: None,
            trigger_write_origin: None,
            commit_timestamp: None,
            check_cancel: Box::new(check_cancel),
            schema_cache: self.schema_cache.clone(),
            schema_cache_dirty: false,
        })
    }

    pub fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        F: FnOnce(&mut SqliteWriteTransaction) -> Result<T>,
    {
        self.execute_write_cancellable(|| Ok(()), task)
    }

    pub fn execute_write_cancellable<T, Check, F>(
        &self,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteCommit<T>>
    where
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut SqliteWriteTransaction) -> Result<T>,
    {
        let mut transaction = self.begin_write_transaction_cancellable(check_cancel)?;
        let value = match task(&mut transaction) {
            Ok(value) => value,
            Err(error) => {
                transaction.rollback();
                return Err(error);
            }
        };
        let commit = transaction.commit()?;
        Ok(TenantWriteCommit { value, commit })
    }

    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub fn check_fault(&self, point: FaultPoint) -> Result<()> {
        self.fault_injector.check(point)
    }

    pub(super) fn open_connection(&self) -> Result<Connection> {
        let total_started = std::time::Instant::now();
        let open_started = std::time::Instant::now();
        let conn = Connection::open(&self.path).map_err(map_sqlite_error)?;
        let open_elapsed = open_started.elapsed();
        let mut apply_key_elapsed = Duration::ZERO;
        let mut harden_elapsed = Duration::ZERO;
        let mut verify_elapsed = Duration::ZERO;
        if let Some(dek) = &self.dek {
            // For encrypted databases, the key must be set before any other operations
            let apply_key_started = std::time::Instant::now();
            apply_encryption_key(&conn, dek)?;
            apply_key_elapsed = apply_key_started.elapsed();
            let harden_started = std::time::Instant::now();
            harden_temp_storage(&conn)?;
            harden_elapsed = harden_started.elapsed();
            // Verify the key is valid before proceeding
            let verify_started = std::time::Instant::now();
            verify_encryption_key(&conn)?;
            verify_elapsed = verify_started.elapsed();
        }
        let initialize_started = std::time::Instant::now();
        initialize_connection(&conn)?;
        let initialize_elapsed = initialize_started.elapsed();
        if sqlite_open_profile_enabled(&self.path) {
            eprintln!(
                "sqlite-connection-profile path={} encrypted={} connection_open={:?} apply_key={:?} temp_hardening={:?} verify_key={:?} initialize={:?} total={:?}",
                self.path.display(),
                self.dek.is_some(),
                open_elapsed,
                apply_key_elapsed,
                harden_elapsed,
                verify_elapsed,
                initialize_elapsed,
                total_started.elapsed(),
            );
        }
        Ok(conn)
    }

    pub(super) fn open_writer_connection(&self) -> Result<Connection> {
        #[cfg(test)]
        observe_sqlite_writer_open(&self.path);
        let conn = self.open_connection()?;
        // Write batches re-execute a small fixed set of statements per record;
        // size the connection's prepared-statement cache so none of them is
        // evicted within a transaction. This is a rusqlite-level setting and
        // changes no SQL semantics.
        conn.set_prepared_statement_cache_capacity(64);
        Ok(conn)
    }

    #[cfg(test)]
    pub(crate) fn reset_write_test_observation(&self) {
        reset_sqlite_write_test_observation(&self.path);
    }

    #[cfg(test)]
    pub(crate) fn write_test_observation(&self) -> SqliteWriteTestObservationSnapshot {
        sqlite_write_test_observation_snapshot(&self.path)
    }

    /// Try to claim a pool slot without waiting; `false` means the pool
    /// is at its cap.
    fn try_reserve_read_connection_slot(&self) -> bool {
        let mut current = self.open_read_connections.load(Ordering::Acquire);
        loop {
            if current >= self.max_read_connections {
                return false;
            }
            match self.open_read_connections.compare_exchange(
                current,
                current.saturating_add(1),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(updated) => current = updated,
            }
        }
    }

    fn release_read_connection_slot(&self) {
        self.open_read_connections.fetch_sub(1, Ordering::AcqRel);
    }

    fn open_pooled_read_connection(&self) -> Result<Option<Connection>> {
        if !self.try_reserve_read_connection_slot() {
            return Ok(None);
        }
        match self.open_connection() {
            Ok(conn) => Ok(Some(conn)),
            Err(error) => {
                self.release_read_connection_slot();
                Err(error)
            }
        }
    }

    /// Acquire a pooled read connection, waiting up to
    /// [`READ_POOL_WAIT`] for one to free up when the pool is at its
    /// cap. The cap tracks `available_parallelism`, so a transient
    /// overlap between foreground reads and background readers (small
    /// CI runners) must wait for a returned connection instead of
    /// failing a correct operation; sustained exhaustion still fails
    /// closed with a typed error after the bounded wait.
    fn acquire_read_connection(&self) -> Result<PooledSqliteConnection> {
        let deadline = std::time::Instant::now() + READ_POOL_WAIT;
        loop {
            let cached = self.lock_read_connections()?.pop();
            let conn = match cached {
                Some(conn) => Some(conn),
                None => self.open_pooled_read_connection()?,
            };
            if let Some(conn) = conn {
                return Ok(PooledSqliteConnection {
                    conn: Some(conn),
                    open_read_connections: self.open_read_connections.clone(),
                    pool: self.read_connections.clone(),
                });
            }
            if std::time::Instant::now() >= deadline {
                return Err(Error::ResourceExhausted(format!(
                    "sqlite read connection pool exhausted at {} open connections \
                     (waited {READ_POOL_WAIT:?} for a free connection)",
                    self.max_read_connections
                )));
            }
            std::thread::sleep(READ_POOL_RETRY_INTERVAL);
        }
    }

    fn lock_read_connections(&self) -> Result<MutexGuard<'_, Vec<Connection>>> {
        self.read_connections
            .lock()
            .map_err(|_| Error::Internal("sqlite read connection pool lock poisoned".to_string()))
    }
}

impl Deref for PooledSqliteConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.conn
            .as_ref()
            .expect("pooled sqlite connection should not be empty while borrowed")
    }
}

impl Drop for PooledSqliteConnection {
    fn drop(&mut self) {
        let Some(conn) = self.conn.take() else {
            return;
        };
        let _ = conn.execute_batch("ROLLBACK");
        if let Ok(mut pool) = self.pool.lock() {
            pool.push(conn);
        } else {
            self.open_read_connections.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

/// Bounded wait for a pooled read connection before failing closed.
const READ_POOL_WAIT: Duration = Duration::from_secs(2);
/// Poll cadence while waiting for a pooled read connection.
const READ_POOL_RETRY_INTERVAL: Duration = Duration::from_millis(10);

pub(super) fn default_sqlite_read_connection_limit() -> usize {
    if let Ok(value) = std::env::var("NIMBUS_SQLITE_MAX_READ_CONNECTIONS")
        && let Ok(parsed) = value.parse::<usize>()
        && parsed > 0
    {
        return parsed;
    }
    std::thread::available_parallelism()
        .map(|parallelism| parallelism.get().max(MIN_SQLITE_READ_CONNECTIONS))
        .unwrap_or(MIN_SQLITE_READ_CONNECTIONS)
}

pub(super) fn initialize_connection(conn: &Connection) -> Result<()> {
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(map_sqlite_error)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(map_sqlite_error)?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(map_sqlite_error)?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(map_sqlite_error)?;
    conn.execute_batch(SQLITE_INIT_SQL)
        .map_err(map_sqlite_error)?;
    Ok(())
}

fn sqlite_open_profile_enabled(path: &Path) -> bool {
    std::env::var_os("NIMBUS_SQLITE_OPEN_PROFILE").is_some() && profile_scope_allows_path(path)
}

fn profile_scope_allows_path(path: &Path) -> bool {
    if std::env::var_os("NIMBUS_PROFILE_ONLY_COLD_SAMPLES").is_none() {
        return true;
    }

    path.to_string_lossy().contains("cold-sample")
}
