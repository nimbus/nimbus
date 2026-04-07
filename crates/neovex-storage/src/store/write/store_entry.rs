use std::path::Path;
use std::sync::Arc;

use neovex_core::Result;
use redb::backends::InMemoryBackend;

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
        let db = redb::Database::create(path).map_err(map_redb_error)?;
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
