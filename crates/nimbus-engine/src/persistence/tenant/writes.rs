use super::*;

impl TenantPersistence {
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
}
