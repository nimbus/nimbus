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
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| Error::Internal(error.to_string()))?;
        }
        let store = Self {
            path,
            clock,
            fault_injector,
            max_read_connections: max_read_connections.max(1),
            open_read_connections: Arc::new(AtomicUsize::new(0)),
            read_connections: Arc::new(Mutex::new(Vec::new())),
            schema_cache: Arc::new(RwLock::new(Schema::default())),
        };
        let conn = store.open_pooled_read_connection()?;
        let schema = load_schema_from_conn(&conn)?;
        store.replace_cached_schema(schema)?;
        store.lock_read_connections()?.push(conn);
        Ok(store)
    }

    pub fn max_read_connections(&self) -> usize {
        self.max_read_connections
    }

    pub fn read_snapshot(&self) -> Result<SqliteReadSnapshot> {
        Ok(SqliteReadSnapshot {
            conn: self.acquire_read_connection()?,
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
        let conn = Connection::open(&self.path).map_err(map_sqlite_error)?;
        initialize_connection(&conn)?;
        Ok(conn)
    }

    fn reserve_read_connection_slot(&self) -> Result<()> {
        let mut current = self.open_read_connections.load(Ordering::Acquire);
        loop {
            if current >= self.max_read_connections {
                return Err(Error::ResourceExhausted(format!(
                    "sqlite read connection pool exhausted at {} open connections",
                    self.max_read_connections
                )));
            }
            match self.open_read_connections.compare_exchange(
                current,
                current.saturating_add(1),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(()),
                Err(updated) => current = updated,
            }
        }
    }

    fn release_read_connection_slot(&self) {
        self.open_read_connections.fetch_sub(1, Ordering::AcqRel);
    }

    fn open_pooled_read_connection(&self) -> Result<Connection> {
        self.reserve_read_connection_slot()?;
        match self.open_connection() {
            Ok(conn) => Ok(conn),
            Err(error) => {
                self.release_read_connection_slot();
                Err(error)
            }
        }
    }

    fn acquire_read_connection(&self) -> Result<PooledSqliteConnection> {
        let conn = self
            .lock_read_connections()?
            .pop()
            .map(Ok)
            .unwrap_or_else(|| self.open_pooled_read_connection())?;
        Ok(PooledSqliteConnection {
            conn: Some(conn),
            open_read_connections: self.open_read_connections.clone(),
            pool: self.read_connections.clone(),
        })
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
        if let Ok(mut pool) = self.pool.lock() {
            pool.push(conn);
        } else {
            self.open_read_connections.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

pub(super) fn default_sqlite_read_connection_limit() -> usize {
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
