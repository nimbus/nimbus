use std::sync::Arc;

use neovex_core::{CommitEntry, Error, Result, WriteOp};

use crate::simulation::{Clock, FaultInjector};

use super::super::TenantWriteTransaction;
use super::super::journal::{append_commit, commit_write_txn_cancellable};

impl TenantWriteTransaction {
    pub(super) fn new<Check>(
        write_txn: redb::WriteTransaction,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
        check_cancel: Check,
    ) -> Self
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        Self {
            write_txn: Some(write_txn),
            clock,
            fault_injector,
            commit_writes: Vec::new(),
            check_cancel: Box::new(check_cancel),
        }
    }

    pub(crate) fn write_txn(&self) -> Result<&redb::WriteTransaction> {
        self.write_txn
            .as_ref()
            .ok_or_else(|| Error::Internal("write transaction already closed".to_string()))
    }

    pub(crate) fn check_cancel(&self) -> Result<()> {
        (self.check_cancel.as_ref())()
    }

    pub(crate) fn record_commit_write(&mut self, write: WriteOp) {
        self.commit_writes.push(write);
    }

    pub fn commit(mut self) -> Result<Option<CommitEntry>> {
        self.check_cancel()?;
        let Some(write_txn) = self.write_txn.take() else {
            return Err(Error::Internal(
                "write transaction already closed".to_string(),
            ));
        };
        let clock = self.clock.clone();
        let fault_injector = self.fault_injector.clone();
        let commit_writes = std::mem::take(&mut self.commit_writes);
        let check_cancel = self.check_cancel;

        let commit = if commit_writes.is_empty() {
            None
        } else {
            Some(append_commit(&write_txn, clock.now(), commit_writes)?)
        };
        commit_write_txn_cancellable(&*fault_injector, || check_cancel.as_ref()(), write_txn)?;
        Ok(commit)
    }

    pub fn rollback(mut self) {
        let _ = self.write_txn.take();
    }
}
