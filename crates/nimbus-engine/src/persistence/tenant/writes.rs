use super::*;

impl TenantPersistence {
    pub(crate) fn apply_prepared_execution_unit_batch(
        &self,
        record: Option<&nimbus_core::TenantEventRecord>,
        schedule_ops: &[ResolvedScheduleOp],
    ) -> Result<Option<CommitEntry>> {
        match record {
            Some(record) => self.apply_prepared_write_batch(record, schedule_ops, None),
            // The established durable contract emits no TenantEventRecord for
            // a schedule-only unit. This branch applies only those already-
            // prepared scheduler effects and cannot construct a storage record.
            None => self.apply_execution_unit_batch_with_origin(&[], schedule_ops, None, None),
        }
    }

    pub(crate) fn apply_prepared_write_batch(
        &self,
        record: &nimbus_core::TenantEventRecord,
        schedule_ops: &[ResolvedScheduleOp],
        scheduled_execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        match_tenant_persistence!(self, |store| {
            store.apply_prepared_write_batch(record, schedule_ops, scheduled_execution_id)
        })
    }

    pub(crate) fn apply_execution_unit_batch_with_origin(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
        trigger_write_origin: Option<&nimbus_core::TriggerWriteOrigin>,
        commit_timestamp: Option<Timestamp>,
    ) -> Result<Option<CommitEntry>> {
        match_tenant_persistence!(self, |store| {
            store.apply_execution_unit_batch_with_origin(
                writes,
                schedule_ops,
                trigger_write_origin,
                commit_timestamp,
            )
        })
    }
}
