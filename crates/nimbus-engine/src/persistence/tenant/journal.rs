use super::*;

impl TenantPersistence {
    delegate_store_method!(fn latest_sequence(&self) -> Result<SequenceNumber>);
    delegate_store_method!(fn applied_sequence(&self) -> Result<SequenceNumber>);
    delegate_store_method!(fn journal_progress(&self) -> Result<JournalProgress>);
    delegate_store_method!(fn recover_durable_journal(&self) -> Result<JournalProgress>);
    delegate_store_method!(fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>>);
    delegate_store_method!(fn read_durable_journal_from(&self, sequence: SequenceNumber) -> Result<Vec<DurableMutationRecord>>);
    delegate_store_method!(fn stream_durable_journal(&self, after: SequenceNumber, limit: usize) -> Result<DurableJournalPage>);
    delegate_store_method!(fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap>);
    delegate_store_method!(fn export_changefeed_bootstrap(&self) -> Result<ChangefeedBootstrap>);
    delegate_store_method!(fn stream_changefeed(&self, cursor: &ChangefeedCursor, limit: usize) -> Result<ChangefeedPage>);

    pub(crate) fn export_point_in_time_restore_archive(
        &self,
        target: PointInTimeRestoreTarget,
        retention_config: RetentionGcConfig,
    ) -> Result<PointInTimeRestoreArchive> {
        match_tenant_persistence!(self, |store| {
            store.export_point_in_time_restore_archive(target, retention_config)
        })
    }

    pub(crate) fn import_point_in_time_restore_archive(
        &self,
        archive: &PointInTimeRestoreArchive,
    ) -> Result<JournalProgress> {
        match_tenant_persistence!(self, |store| {
            store.import_point_in_time_restore_archive(archive)
        })
    }

    pub(crate) fn append_durable_records_batch(
        &self,
        records: &[DurableMutationRecord],
    ) -> Result<()> {
        match_tenant_persistence!(self, |store| store.append_durable_records_batch(records))
    }

    pub(crate) fn apply_durable_records_batch(
        &self,
        records: &[DurableMutationRecord],
    ) -> Result<()> {
        match_tenant_persistence!(self, |store| store.apply_durable_records_batch(records))
    }
}
