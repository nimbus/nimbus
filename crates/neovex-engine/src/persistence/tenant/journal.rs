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

    pub(crate) fn append_durable_records_batch(
        &self,
        records: &[DurableMutationRecord],
    ) -> Result<()> {
        match self {
            Self::Redb(store) => store.append_durable_records_batch(records),
            Self::Sqlite(store) => store.append_durable_records_batch(records),
            Self::LibsqlReplica(store) => store.append_durable_records_batch(records),
            Self::Postgres(store) => store.append_durable_records_batch(records),
            Self::MySql(store) => store.append_durable_records_batch(records),
        }
    }

    pub(crate) fn apply_durable_records_batch(
        &self,
        records: &[DurableMutationRecord],
    ) -> Result<()> {
        match self {
            Self::Redb(store) => store.apply_durable_records_batch(records),
            Self::Sqlite(store) => store.apply_durable_records_batch(records),
            Self::LibsqlReplica(store) => store.apply_durable_records_batch(records),
            Self::Postgres(store) => store.apply_durable_records_batch(records),
            Self::MySql(store) => store.apply_durable_records_batch(records),
        }
    }
}
