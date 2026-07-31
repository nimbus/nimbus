use std::sync::Arc;

use nimbus_core::{
    CommitEntry, Error, IdSource, Result, TableId, TableName, TenantEventKind, Timestamp,
    WallClock, WriteOp,
};

use crate::simulation::FaultInjector;

use super::super::TenantWriteTransaction;
use super::super::journal::{append_commit, append_prepared_commit, commit_write_txn_cancellable};

impl TenantWriteTransaction {
    pub(super) fn new<Check>(
        write_txn: redb::WriteTransaction,
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        id_source: Arc<dyn IdSource>,
        check_cancel: Check,
    ) -> Self
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        Self {
            write_txn: Some(write_txn),
            clock,
            fault_injector,
            id_source,
            commit_writes: Vec::new(),
            tenant_events: Vec::new(),
            prepared_record: None,
            durable_records_for_fault: Vec::new(),
            check_cancel: Box::new(check_cancel),
        }
    }

    pub(crate) fn write_txn(&self) -> Result<&redb::WriteTransaction> {
        self.write_txn
            .as_ref()
            .ok_or_else(|| Error::Internal("write transaction already closed".to_string()))
    }

    pub(crate) fn resolve_or_create_table_id(&self, table: &TableName) -> Result<TableId> {
        super::super::table_catalog::resolve_or_create_table_id_in_write_txn(
            self.write_txn()?,
            table,
            self.id_source.as_ref(),
        )
    }

    pub(crate) fn check_cancel(&self) -> Result<()> {
        (self.check_cancel.as_ref())()
    }

    pub(crate) fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub(crate) fn record_commit_write(&mut self, write: WriteOp) {
        self.commit_writes.push(write);
    }

    pub(crate) fn record_tenant_event(&mut self, event: TenantEventKind) {
        self.tenant_events.push(event);
    }

    pub(crate) fn set_prepared_record(&mut self, record: nimbus_core::TenantEventRecord) {
        self.commit_writes = record.writes.clone();
        self.tenant_events = record
            .events
            .iter()
            .filter(|event| !matches!(event, TenantEventKind::DocumentWrite { .. }))
            .cloned()
            .collect();
        // Retained past `prepared_record.take()` in `commit_with_timestamp` so
        // the commit-sequence fault checks can name the record this transaction
        // is making durable.
        self.durable_records_for_fault = vec![record.clone()];
        self.prepared_record = Some(record);
    }

    pub(crate) fn apply_prepared_record(
        &mut self,
        record: &nimbus_core::TenantEventRecord,
    ) -> Result<()> {
        super::super::journal::apply_durable_record_in_write_txn(self.write_txn()?, record)
    }

    pub fn commit(self) -> Result<Option<CommitEntry>> {
        self.commit_with_timestamp(None)
    }

    pub(crate) fn commit_with_timestamp(
        mut self,
        commit_timestamp: Option<Timestamp>,
    ) -> Result<Option<CommitEntry>> {
        self.check_cancel()?;
        let Some(write_txn) = self.write_txn.take() else {
            return Err(Error::Internal(
                "write transaction already closed".to_string(),
            ));
        };
        let clock = self.clock.clone();
        let fault_injector = self.fault_injector.clone();
        let commit_writes = std::mem::take(&mut self.commit_writes);
        let mut tenant_events = std::mem::take(&mut self.tenant_events);
        let prepared_record = self.prepared_record.take();
        let durable_records_for_fault = std::mem::take(&mut self.durable_records_for_fault);
        let check_cancel = self.check_cancel;

        if !commit_writes.is_empty() {
            tenant_events.insert(
                0,
                TenantEventKind::DocumentWrite {
                    writes: commit_writes.clone(),
                },
            );
        }

        let commit = if let Some(record) = prepared_record {
            crate::store::validate_prepared_record_shape(&record, &commit_writes, &tenant_events)?;
            Some(append_prepared_commit(&write_txn, &record)?)
        } else if tenant_events.is_empty() {
            None
        } else {
            Some(append_commit(
                &write_txn,
                commit_timestamp.unwrap_or_else(|| clock.now()),
                commit_writes,
                tenant_events,
            )?)
        };
        commit_write_txn_cancellable(
            &*fault_injector,
            &durable_records_for_fault,
            || check_cancel.as_ref()(),
            write_txn,
        )?;
        Ok(commit)
    }

    pub fn rollback(mut self) {
        let _ = self.write_txn.take();
    }
}
