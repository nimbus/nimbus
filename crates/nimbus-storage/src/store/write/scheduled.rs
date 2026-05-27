use nimbus_core::{Error, Result, TenantEventKind};

use crate::scheduler::{cancel_scheduled_job_in_write_txn, insert_scheduled_job_in_write_txn};

use super::super::journal::begin_scheduled_execution as begin_scheduled_execution_in_journal;
use super::super::{ResolvedScheduleOp, TenantWriteTransaction};

impl TenantWriteTransaction {
    pub fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool> {
        self.check_cancel()?;
        let inserted = begin_scheduled_execution_in_journal(self.write_txn()?, execution_id)?;
        if inserted && let Some(execution_id) = execution_id {
            self.record_tenant_event(TenantEventKind::ScheduledExecution {
                execution_id: execution_id.to_string(),
            });
        }
        Ok(inserted)
    }
}

pub(super) fn apply_schedule_ops(
    write_txn: &redb::WriteTransaction,
    schedule_ops: &[ResolvedScheduleOp],
) -> Result<()> {
    for schedule_op in schedule_ops {
        match schedule_op {
            ResolvedScheduleOp::Insert { job } => {
                insert_scheduled_job_in_write_txn(write_txn, job)?;
            }
            ResolvedScheduleOp::Cancel { job_id } => {
                if !cancel_scheduled_job_in_write_txn(write_txn, job_id)? {
                    return Err(Error::ScheduledJobNotFound(job_id.clone()));
                }
            }
        }
    }

    Ok(())
}
