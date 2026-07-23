use super::*;
use nimbus_storage::TriggerInvocationTransitionStore;

impl TenantPersistence {
    pub(crate) fn materialize_trigger_invocations(
        &self,
        records: &[TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> Result<()> {
        match_tenant_persistence!(self, |store| {
            store.materialize_trigger_invocations(records, cursor)
        })
    }

    pub(crate) fn list_trigger_invocations(&self) -> Result<Vec<TriggerInvocationRecord>> {
        match_tenant_persistence!(self, |store| store.list_trigger_invocations())
    }

    pub(crate) fn trigger_invocation(
        &self,
        key: &nimbus_core::TriggerInvocationKey,
    ) -> Result<Option<TriggerInvocationRecord>> {
        match_tenant_persistence!(self, |store| store.trigger_invocation(key))
    }

    #[cfg(test)]
    pub(crate) fn save_trigger_invocation(&self, record: &TriggerInvocationRecord) -> Result<()> {
        match_tenant_persistence!(self, |store| store.save_trigger_invocation(record))
    }

    pub(crate) fn persist_trigger_invocation_transition(
        &self,
        record: &TriggerInvocationRecord,
    ) -> Result<()> {
        match_tenant_persistence!(self, |store| {
            store.persist_trigger_invocation_transition(record)
        })
    }

    pub(crate) fn persist_fenced_trigger_invocation_transition(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_durable_sequence: nimbus_core::SequenceNumber,
        record: &TriggerInvocationRecord,
    ) -> nimbus_storage::CommitterLeaseResult<()> {
        match_tenant_persistence!(self, |store| {
            store.persist_fenced_trigger_invocation_transition(
                owner_id,
                epoch,
                expected_durable_sequence,
                record,
            )
        })
    }
}
