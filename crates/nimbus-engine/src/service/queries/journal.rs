use std::sync::Arc;

use nimbus_core::{DurableMutationRecord, Result, SequenceNumber, TenantId};
use nimbus_storage::{DurableJournalBootstrap, DurableJournalPage};

use crate::persistence::TenantPersistence;
use crate::service::Service;

impl Service {
    async fn execute_journal_read_async<T, F>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        read: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(TenantPersistence) -> Result<T> + Send + 'static,
    {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let tenant_id_for_task = tenant_id.clone();
        let runtime_for_task = runtime.clone();
        runtime
            .read_storage
            .execute(move |store| {
                let _operation = runtime_for_task.enter_operation(&tenant_id_for_task)?;
                read(store)
            })
            .await
    }

    /// Reads durable journal records committed after the provided sequence number.
    pub fn read_durable_journal(
        &self,
        tenant_id: &TenantId,
        after: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let from = SequenceNumber(after.0.saturating_add(1));
        runtime.store.read_durable_journal_from(from)
    }

    /// Reads durable journal records asynchronously.
    pub async fn read_durable_journal_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        after: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        self.execute_journal_read_async(tenant_id, move |store| {
            let from = SequenceNumber(after.0.saturating_add(1));
            store.read_durable_journal_from(from)
        })
        .await
    }

    /// Streams durable journal records using an ordered sequence cursor.
    pub fn stream_durable_journal(
        &self,
        tenant_id: &TenantId,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.stream_durable_journal(after, limit)
    }

    /// Streams durable journal records asynchronously using an ordered sequence cursor.
    pub async fn stream_durable_journal_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        self.execute_journal_read_async(tenant_id, move |store| {
            store.stream_durable_journal(after, limit)
        })
        .await
    }

    /// Exports snapshot metadata for bootstrapping a journal consumer.
    pub fn export_durable_journal_bootstrap(
        &self,
        tenant_id: &TenantId,
    ) -> Result<DurableJournalBootstrap> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.export_durable_journal_bootstrap()
    }

    /// Exports snapshot metadata for bootstrapping a journal consumer asynchronously.
    pub async fn export_durable_journal_bootstrap_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<DurableJournalBootstrap> {
        self.execute_journal_read_async(tenant_id, move |store| {
            store.export_durable_journal_bootstrap()
        })
        .await
    }

    /// Returns the latest committed sequence number for a tenant.
    pub fn latest_sequence(&self, tenant_id: &TenantId) -> Result<SequenceNumber> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.latest_sequence()
    }

    /// Returns the latest committed sequence number for a tenant asynchronously.
    pub async fn latest_sequence_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<SequenceNumber> {
        self.execute_journal_read_async(tenant_id, move |store| store.latest_sequence())
            .await
    }
}
