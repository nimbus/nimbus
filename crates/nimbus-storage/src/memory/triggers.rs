use nimbus_core::{
    Result, TenantEventKind, TriggerDeliveryCursor, TriggerInvocationKey, TriggerInvocationRecord,
};

use super::MemoryTenantStore;

impl MemoryTenantStore {
    pub fn trigger_delivery_cursor(&self) -> Result<TriggerDeliveryCursor> {
        Ok(self.read_state()?.trigger_delivery_cursor)
    }

    pub fn set_trigger_delivery_cursor(&self, cursor: TriggerDeliveryCursor) -> Result<()> {
        let timestamp = self.now();
        self.transact(|state| {
            state.trigger_delivery_cursor = cursor;
            state.append_events(
                timestamp,
                Vec::new(),
                vec![TenantEventKind::TriggerDelivery { cursor }],
            )?;
            Ok(())
        })
    }

    pub fn materialize_trigger_invocations(
        &self,
        records: &[TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> Result<()> {
        let timestamp = self.now();
        self.transact(|state| {
            for record in records {
                state
                    .trigger_invocations
                    .insert(record.key.clone(), record.clone());
            }
            state.trigger_delivery_cursor = cursor;
            state.append_events(
                timestamp,
                Vec::new(),
                vec![TenantEventKind::TriggerDelivery { cursor }],
            )?;
            Ok(())
        })
    }

    pub fn list_trigger_invocations(&self) -> Result<Vec<TriggerInvocationRecord>> {
        let mut records = self
            .read_state()?
            .trigger_invocations
            .values()
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            left.commit_sequence
                .cmp(&right.commit_sequence)
                .then(left.key.cmp(&right.key))
        });
        Ok(records)
    }

    pub fn trigger_invocation(
        &self,
        key: &TriggerInvocationKey,
    ) -> Result<Option<TriggerInvocationRecord>> {
        Ok(self.read_state()?.trigger_invocations.get(key).cloned())
    }

    pub fn save_trigger_invocation(&self, record: &TriggerInvocationRecord) -> Result<()> {
        self.transact(|state| {
            state
                .trigger_invocations
                .insert(record.key.clone(), record.clone());
            Ok(())
        })
    }
}
