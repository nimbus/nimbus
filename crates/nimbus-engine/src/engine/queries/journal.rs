use std::sync::Arc;

use nimbus_core::{Result, SequenceNumber, TenantEventRecord, TenantId};
use nimbus_storage::{
    ChangefeedBootstrap, ChangefeedCursor, ChangefeedPage, DurableJournalBootstrap,
    DurableJournalPage, PointInTimeRestoreArchive, PointInTimeRestoreTarget, RetentionGcConfig,
};

use crate::engine::Engine;
use crate::persistence::TenantPersistence;

impl Engine {
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
    ) -> Result<Vec<TenantEventRecord>> {
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
    ) -> Result<Vec<TenantEventRecord>> {
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

    /// Exports a typed changefeed bootstrap with snapshot cut and resume cursor.
    pub fn export_changefeed_bootstrap(&self, tenant_id: &TenantId) -> Result<ChangefeedBootstrap> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.export_changefeed_bootstrap()
    }

    /// Exports a typed changefeed bootstrap asynchronously.
    pub async fn export_changefeed_bootstrap_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<ChangefeedBootstrap> {
        self.execute_journal_read_async(tenant_id, move |store| store.export_changefeed_bootstrap())
            .await
    }

    /// Streams typed changefeed events from a retained changefeed cursor.
    pub fn stream_changefeed(
        &self,
        tenant_id: &TenantId,
        cursor: &ChangefeedCursor,
        limit: usize,
    ) -> Result<ChangefeedPage> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.stream_changefeed(cursor, limit)
    }

    /// Streams typed changefeed events asynchronously from a retained cursor.
    pub async fn stream_changefeed_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        cursor: ChangefeedCursor,
        limit: usize,
    ) -> Result<ChangefeedPage> {
        self.execute_journal_read_async(tenant_id, move |store| {
            store.stream_changefeed(&cursor, limit)
        })
        .await
    }

    /// Exports a point-in-time restore archive of the tenant at its
    /// latest committed sequence — the unit `nimbus backup` writes per
    /// tenant. Rides the SEQ8 storage machinery; no new formats.
    pub fn export_latest_point_in_time_restore_archive(
        &self,
        tenant_id: &TenantId,
    ) -> Result<PointInTimeRestoreArchive> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let latest = runtime.store.latest_sequence()?;
        runtime.store.export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(latest),
            RetentionGcConfig::default(),
        )
    }

    /// Imports a point-in-time restore archive into a tenant. The
    /// storage layer fails closed unless the tenant's journal is empty
    /// and the restored fingerprint matches the archive's.
    pub fn import_point_in_time_restore_archive(
        &self,
        tenant_id: &TenantId,
        archive: &PointInTimeRestoreArchive,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime
            .store
            .import_point_in_time_restore_archive(archive)?;
        Ok(())
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
