use super::encryption::{apply_encryption_key, harden_temp_storage, verify_encryption_key};
use super::*;

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
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
            max_read_connections,
        )
    }

    pub fn open_with_simulation(
        path: impl AsRef<Path>,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        Self::open_with_simulation_and_max_read_connections(
            path,
            clock,
            fault_injector,
            default_sqlite_read_connection_limit(),
        )
    }

    pub(crate) fn open_with_simulation_and_max_read_connections(
        path: impl AsRef<Path>,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
        max_read_connections: usize,
    ) -> Result<Self> {
        Self::open_internal(path, None, clock, fault_injector, max_read_connections)
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
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
            max_read_connections,
        )
    }

    /// Opens or creates an encrypted SQLite tenant store with simulation support.
    pub fn open_encrypted_with_simulation(
        path: impl AsRef<Path>,
        dek: &[u8; 32],
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        Self::open_encrypted_with_simulation_and_max_read_connections(
            path,
            dek,
            clock,
            fault_injector,
            default_sqlite_read_connection_limit(),
        )
    }

    pub(crate) fn open_encrypted_with_simulation_and_max_read_connections(
        path: impl AsRef<Path>,
        dek: &[u8; 32],
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
        max_read_connections: usize,
    ) -> Result<Self> {
        Self::open_internal(
            path,
            Some(DataEncryptionKey::new(*dek)),
            clock,
            fault_injector,
            max_read_connections,
        )
    }

    fn open_internal(
        path: impl AsRef<Path>,
        dek: Option<DataEncryptionKey>,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
        max_read_connections: usize,
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
        let conn = self.open_connection()?;
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_sqlite_error)?;
        Ok(SqliteWriteTransaction {
            conn: Some(conn),
            clock: self.clock.clone(),
            fault_injector: self.fault_injector.clone(),
            commit_writes: Vec::new(),
            tenant_events: Vec::new(),
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
