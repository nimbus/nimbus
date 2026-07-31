use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use nimbus_core::{
    Error, IdSource, Result, SystemIdSource, SystemWallClock, TenantEventRecord, Timestamp,
    WallClock,
};

use crate::TenantWriteCommit;
use crate::async_storage::BlockingWriteStore;
use crate::simulation::{FaultInjector, FaultPoint, NoopFaultInjector};

use super::state::MemoryState;

/// A deterministic, process-local tenant store backed only by Rust data structures.
pub struct MemoryTenantStore {
    pub(super) state: Arc<RwLock<MemoryState>>,
    pub(super) clock: Arc<dyn WallClock>,
    pub(super) fault_injector: Arc<dyn FaultInjector>,
}

impl Default for MemoryTenantStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryTenantStore {
    pub fn new() -> Self {
        Self::with_simulation(Arc::new(SystemWallClock), Arc::new(NoopFaultInjector))
    }

    pub fn with_simulation(
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Self {
        Self::with_simulation_and_id_source(clock, fault_injector, Arc::new(SystemIdSource))
    }

    pub fn with_simulation_and_id_source(
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        id_source: Arc<dyn IdSource>,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(MemoryState::with_id_source(id_source))),
            clock,
            fault_injector,
        }
    }

    /// Recreates the process-local store from a cloned durable-state image.
    ///
    /// This models a restart boundary for deterministic tests. It does not
    /// claim persistence across a real process restart: callers must explicitly
    /// carry this volatile image into the replacement store.
    pub fn restart_from_durable_state(&self) -> Result<Self> {
        Ok(Self {
            state: Arc::new(RwLock::new(self.read_state()?.clone())),
            clock: self.clock.clone(),
            fault_injector: self.fault_injector.clone(),
        })
    }

    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub fn check_fault(&self, point: FaultPoint) -> Result<()> {
        self.fault_injector.check(point)
    }

    /// Fault check naming the durable journal records this boundary is making
    /// visible, so a records-scoped injector can target one specific batch. The
    /// store's tenant is already bound into `fault_injector`; see
    /// `crate::simulation::tenant_scoped_fault_injector`.
    pub(super) fn check_durable_records_fault(
        &self,
        point: FaultPoint,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        self.fault_injector.check_durable_records(point, records)
    }

    pub(super) fn read_state(&self) -> Result<RwLockReadGuard<'_, MemoryState>> {
        self.state
            .read()
            .map_err(|_| Error::Internal("memory tenant read lock is poisoned".to_string()))
    }

    pub(super) fn write_state(&self) -> Result<RwLockWriteGuard<'_, MemoryState>> {
        self.state
            .write()
            .map_err(|_| Error::Internal("memory tenant write lock is poisoned".to_string()))
    }

    pub(super) fn transact<T>(
        &self,
        apply: impl FnOnce(&mut MemoryState) -> Result<T>,
    ) -> Result<T> {
        self.transact_durable_records(&[], apply)
    }

    /// [`MemoryTenantStore::transact`] for a transaction that makes durable
    /// journal records visible. `records` reaches the commit-sequence fault
    /// checks so a fault armed at one batch is not consumed by an unrelated
    /// concurrent commit on the same tenant.
    pub(super) fn transact_durable_records<T>(
        &self,
        records: &[TenantEventRecord],
        apply: impl FnOnce(&mut MemoryState) -> Result<T>,
    ) -> Result<T> {
        let mut state = self.write_state()?;
        let mut next = state.clone();
        let value = apply(&mut next)?;
        self.check_durable_records_fault(FaultPoint::StorageCommitBeforeVisibility, records)?;
        next.revision = state.revision.saturating_add(1);
        *state = next;
        drop(state);
        self.check_durable_records_fault(
            FaultPoint::StorageCommitAfterVisibilityBeforeReturn,
            records,
        )?;
        Ok(value)
    }

    pub fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        F: FnOnce(&mut MemoryWriteTransaction) -> Result<T>,
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
        F: FnOnce(&mut MemoryWriteTransaction) -> Result<T>,
    {
        check_cancel()?;
        let state = self.read_state()?.clone();
        let mut transaction = MemoryWriteTransaction {
            base_revision: state.revision,
            state,
            check_cancel: Box::new(check_cancel),
        };
        let value = task(&mut transaction)?;
        transaction.check_cancel()?;
        let mut live = self.write_state()?;
        if live.revision != transaction.base_revision {
            return Err(Error::conflict(
                "memory write transaction observed concurrent state change".to_string(),
            ));
        }
        self.fault_injector
            .check(FaultPoint::StorageCommitBeforeVisibility)?;
        transaction.state.revision = live.revision.saturating_add(1);
        *live = transaction.state;
        drop(live);
        self.fault_injector
            .check(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)?;
        Ok(TenantWriteCommit {
            value,
            commit: None,
        })
    }
}

pub struct MemoryWriteTransaction {
    pub(super) base_revision: u64,
    pub(super) state: MemoryState,
    pub(super) check_cancel: Box<dyn Fn() -> Result<()> + Send>,
}

impl MemoryWriteTransaction {
    pub(super) fn check_cancel(&self) -> Result<()> {
        (self.check_cancel)()
    }
}

impl BlockingWriteStore for MemoryTenantStore {
    type WriteTransaction = MemoryWriteTransaction;

    fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut Self::WriteTransaction) -> Result<T> + Send + 'static,
    {
        Self::execute_write(self, task)
    }

    fn execute_write_cancellable<T, Check, F>(
        &self,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut Self::WriteTransaction) -> Result<T> + Send + 'static,
    {
        Self::execute_write_cancellable(self, check_cancel, task)
    }
}
