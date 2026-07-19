use nimbus_core::{Result, Schema, SequenceNumber, TenantEventRecord};
use nimbus_storage::{DurableJournalPage, JournalProgress};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TenantProviderRefreshPlan {
    pub refresh_schema: bool,
    pub refresh_journal: bool,
}

impl TenantPersistence {
    /// True when this runtime is the only process that can assign tenant
    /// sequences. Shared provider backends receive foreign commits through
    /// asynchronous catch-up, so their local write log cannot be authoritative
    /// without a storage watermark read (PPSC5 owns that publisher protocol).
    pub(crate) fn has_process_local_sequence_authority(&self) -> bool {
        match self {
            Self::Redb(_) | Self::Sqlite(_) => true,
            Self::Postgres(_) | Self::LibsqlReplica(_) | Self::MySql(_) => false,
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => true,
        }
    }

    pub(crate) async fn load_schema_async(
        &self,
        read_storage: &TenantPersistenceExecutor,
    ) -> Result<Schema> {
        match self {
            Self::Postgres(store) => store.load_schema_async().await,
            Self::Redb(_) | Self::Sqlite(_) | Self::LibsqlReplica(_) | Self::MySql(_) => {
                read_storage.execute(|store| store.load_schema()).await
            }
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => read_storage.execute(|store| store.load_schema()).await,
        }
    }

    pub(crate) async fn journal_progress_async(
        &self,
        read_storage: &TenantPersistenceExecutor,
    ) -> Result<JournalProgress> {
        match self {
            Self::Postgres(store) => store.journal_progress_async().await,
            Self::Redb(_) | Self::Sqlite(_) | Self::LibsqlReplica(_) | Self::MySql(_) => {
                read_storage.execute(|store| store.journal_progress()).await
            }
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => read_storage.execute(|store| store.journal_progress()).await,
        }
    }

    pub(crate) async fn recover_durable_journal_async(
        &self,
        read_storage: &TenantPersistenceExecutor,
    ) -> Result<JournalProgress> {
        match self {
            Self::Postgres(store) => store.recover_durable_journal_async().await,
            Self::Redb(_) | Self::Sqlite(_) | Self::LibsqlReplica(_) | Self::MySql(_) => {
                read_storage
                    .execute(|store| store.recover_durable_journal())
                    .await
            }
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => {
                read_storage
                    .execute(|store| store.recover_durable_journal())
                    .await
            }
        }
    }

    pub(crate) async fn read_durable_journal_from_async(
        &self,
        read_storage: &TenantPersistenceExecutor,
        next_sequence: SequenceNumber,
    ) -> Result<Vec<TenantEventRecord>> {
        match self {
            Self::Postgres(store) => store.read_durable_journal_from_async(next_sequence).await,
            Self::Redb(_) | Self::Sqlite(_) | Self::LibsqlReplica(_) | Self::MySql(_) => {
                read_storage
                    .execute(move |store| store.read_durable_journal_from(next_sequence))
                    .await
            }
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => {
                read_storage
                    .execute(move |store| store.read_durable_journal_from(next_sequence))
                    .await
            }
        }
    }

    /// Reads one bounded page of the provider's journal after `after`.
    ///
    /// Callers that walk an unbounded tail must page through this instead of
    /// `read_durable_journal_from_async`, whose result grows with the whole
    /// remaining journal.
    pub(crate) async fn stream_durable_journal_async(
        &self,
        read_storage: &TenantPersistenceExecutor,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        match self {
            Self::Postgres(store) => store.stream_durable_journal_async(after, limit).await,
            Self::Redb(_) | Self::Sqlite(_) | Self::LibsqlReplica(_) | Self::MySql(_) => {
                read_storage
                    .execute(move |store| store.stream_durable_journal(after, limit))
                    .await
            }
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => {
                read_storage
                    .execute(move |store| store.stream_durable_journal(after, limit))
                    .await
            }
        }
    }

    /// Recovers a provider's raw journal tail as unflattened records, kept
    /// event-kind-aware for callers that need to tell a real document write
    /// apart from a zero-write commit that must still force re-evaluation
    /// (e.g. a `SchemaChange`/`TableLifecycle`). `read_commit_log_from_async`
    /// flattens away that distinction via `as_commit_entry` and must not be
    /// used here.
    pub(crate) async fn recover_journal_tail_async(
        &self,
        read_storage: &TenantPersistenceExecutor,
        next_sequence: SequenceNumber,
    ) -> Result<(JournalProgress, Vec<TenantEventRecord>)> {
        let progress = self.recover_durable_journal_async(read_storage).await?;
        let records = if progress.applied_head.0 >= next_sequence.0 {
            self.read_durable_journal_from_async(read_storage, next_sequence)
                .await?
        } else {
            Vec::new()
        };
        Ok((progress, records))
    }

    pub(crate) async fn has_scheduled_work_async(
        &self,
        read_storage: &TenantPersistenceExecutor,
    ) -> Result<bool> {
        match self {
            Self::Postgres(store) => store.has_scheduled_work_async().await,
            Self::Redb(_) | Self::Sqlite(_) | Self::LibsqlReplica(_) | Self::MySql(_) => {
                read_storage
                    .execute(|store| store.has_scheduled_work())
                    .await
            }
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => {
                read_storage
                    .execute(|store| store.has_scheduled_work())
                    .await
            }
        }
    }

    pub(crate) async fn plan_loaded_runtime_refresh_async(
        &self,
        read_storage: &TenantPersistenceExecutor,
        current_schema: &Schema,
        durable_head: SequenceNumber,
        applied_head: SequenceNumber,
    ) -> Result<TenantProviderRefreshPlan> {
        if matches!(self, Self::MySql(_)) {
            self.invalidate_schema_cache();
        }
        let store_schema = self.load_schema_async(read_storage).await?;
        let store_progress = self.journal_progress_async(read_storage).await?;
        Ok(TenantProviderRefreshPlan {
            refresh_schema: store_schema != *current_schema,
            refresh_journal: store_progress.durable_head.0 > durable_head.0
                || store_progress.applied_head.0 > applied_head.0,
        })
    }

    pub(crate) fn applied_head_after_durable_apply(
        &self,
        records: &[TenantEventRecord],
    ) -> Result<SequenceNumber> {
        if matches!(self, Self::LibsqlReplica(_)) {
            Ok(records
                .last()
                .expect("non-empty durable batch should have a last record")
                .sequence)
        } else {
            self.applied_sequence()
        }
    }
}
