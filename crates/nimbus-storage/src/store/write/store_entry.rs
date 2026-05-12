use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nimbus_core::Result;
use redb::backends::InMemoryBackend;

use crate::encrypted_redb::{
    EncryptedFileBackend, EncryptedMemoryBackend, EncryptedReadProfileSnapshot,
};
use crate::simulation::{Clock, FaultInjector, FaultPoint, NoopFaultInjector, SystemClock};

use super::super::scan::ScanMetrics;
use super::super::{TenantStore, TenantWriteCommit, TenantWriteTransaction, map_redb_error};

impl TenantStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_simulation(path, Arc::new(SystemClock), Arc::new(NoopFaultInjector))
    }

    pub fn open_with_simulation(
        path: impl AsRef<Path>,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        let path = path.as_ref();
        let total_started = Instant::now();
        let database_open_started = Instant::now();
        let db = redb::Database::builder()
            .create(path)
            .map_err(map_redb_error)?;
        maybe_emit_redb_open_profile(
            path,
            false,
            Duration::ZERO,
            database_open_started.elapsed(),
            total_started.elapsed(),
            None,
        );
        Ok(Self {
            db,
            clock,
            fault_injector,
            scan_metrics: Arc::new(ScanMetrics::new()),
        })
    }

    /// Opens or creates an encrypted tenant store.
    ///
    /// The DEK must be a 32-byte key obtained from the key provider system.
    /// If the file exists, it will be opened with the provided key.
    /// If it doesn't exist, a new encrypted database will be created.
    pub fn open_encrypted(path: impl AsRef<Path>, dek: &[u8; 32]) -> Result<Self> {
        Self::open_encrypted_with_simulation(
            path,
            dek,
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
        )
    }

    /// Opens or creates an encrypted tenant store with simulation support.
    pub fn open_encrypted_with_simulation(
        path: impl AsRef<Path>,
        dek: &[u8; 32],
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        let path = path.as_ref();
        let total_started = Instant::now();
        let backend_open_started = Instant::now();
        let backend = EncryptedFileBackend::create(path, dek).map_err(map_redb_error)?;
        let read_profile = backend.read_profile_handle();
        let backend_open_elapsed = backend_open_started.elapsed();
        let database_open_started = Instant::now();
        let db = redb::Database::builder()
            .create_with_backend(backend)
            .map_err(map_redb_error)?;
        maybe_emit_redb_open_profile(
            path,
            true,
            backend_open_elapsed,
            database_open_started.elapsed(),
            total_started.elapsed(),
            read_profile.as_ref().map(|handle| handle.snapshot()),
        );
        Ok(Self {
            db,
            clock,
            fault_injector,
            scan_metrics: Arc::new(ScanMetrics::new()),
        })
    }

    pub fn create_in_memory() -> Result<Self> {
        Self::create_in_memory_with_simulation(Arc::new(SystemClock), Arc::new(NoopFaultInjector))
    }

    pub fn create_in_memory_with_simulation(
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        let db = redb::Database::builder()
            .create_with_backend(InMemoryBackend::new())
            .map_err(map_redb_error)?;
        Ok(Self {
            db,
            clock,
            fault_injector,
            scan_metrics: Arc::new(ScanMetrics::new()),
        })
    }

    /// Creates an in-memory encrypted tenant store for testing.
    pub fn create_in_memory_encrypted(dek: &[u8; 32]) -> Result<Self> {
        Self::create_in_memory_encrypted_with_simulation(
            dek,
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
        )
    }

    /// Creates an in-memory encrypted tenant store with simulation support.
    pub fn create_in_memory_encrypted_with_simulation(
        dek: &[u8; 32],
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        let backend = EncryptedMemoryBackend::new(dek).map_err(map_redb_error)?;
        let db = redb::Database::builder()
            .create_with_backend(backend)
            .map_err(map_redb_error)?;
        Ok(Self {
            db,
            clock,
            fault_injector,
            scan_metrics: Arc::new(ScanMetrics::new()),
        })
    }

    pub fn begin_write_transaction(&self) -> Result<TenantWriteTransaction> {
        self.begin_write_transaction_cancellable(|| Ok(()))
    }

    pub fn begin_write_transaction_cancellable<Check>(
        &self,
        check_cancel: Check,
    ) -> Result<TenantWriteTransaction>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        Ok(TenantWriteTransaction::new(
            write_txn,
            self.clock.clone(),
            self.fault_injector.clone(),
            check_cancel,
        ))
    }

    pub fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        F: FnOnce(&mut TenantWriteTransaction) -> Result<T>,
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
        F: FnOnce(&mut TenantWriteTransaction) -> Result<T>,
    {
        let mut transaction = self.begin_write_transaction_cancellable(check_cancel)?;
        let value = task(&mut transaction)?;
        let commit = transaction.commit()?;
        Ok(TenantWriteCommit { value, commit })
    }

    pub(crate) fn commit_write_txn(&self, write_txn: redb::WriteTransaction) -> Result<()> {
        super::super::journal::commit_write_txn_cancellable(
            &*self.fault_injector,
            || Ok(()),
            write_txn,
        )
    }

    pub fn check_fault(&self, point: FaultPoint) -> Result<()> {
        self.fault_injector.check(point)
    }
}

fn maybe_emit_redb_open_profile(
    path: &Path,
    encrypted: bool,
    backend_open: Duration,
    database_open: Duration,
    total: Duration,
    encrypted_reads: Option<EncryptedReadProfileSnapshot>,
) {
    if std::env::var_os("NIMBUS_REDB_OPEN_PROFILE").is_none() {
        return;
    }
    if std::env::var_os("NIMBUS_PROFILE_ONLY_COLD_SAMPLES").is_some()
        && !path.to_string_lossy().contains("cold-sample")
    {
        return;
    }

    if let Some(reads) = encrypted_reads {
        eprintln!(
            "redb-open-profile path={} encrypted={} backend_open={:?} database_open={:?} total={:?} open_read_calls={} open_requested_bytes={} open_page_reads={} open_file_read={:?} open_decrypt={:?}",
            path.display(),
            encrypted,
            backend_open,
            database_open,
            total,
            reads.read_calls,
            reads.bytes_requested,
            reads.page_reads,
            reads.file_read,
            reads.decrypt,
        );
    } else {
        eprintln!(
            "redb-open-profile path={} encrypted={} backend_open={:?} database_open={:?} total={:?}",
            path.display(),
            encrypted,
            backend_open,
            database_open,
            total,
        );
    }
}
