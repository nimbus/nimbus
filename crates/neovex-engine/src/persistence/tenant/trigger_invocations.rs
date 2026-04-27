use super::*;

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
        key: &neovex_core::TriggerInvocationKey,
    ) -> Result<Option<TriggerInvocationRecord>> {
        match_tenant_persistence!(self, |store| store.trigger_invocation(key))
    }

    pub(crate) fn save_trigger_invocation(&self, record: &TriggerInvocationRecord) -> Result<()> {
        match_tenant_persistence!(self, |store| store.save_trigger_invocation(record))
    }
}
