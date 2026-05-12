use nimbus_core::{CommitEntry, DurableMutationRecord, Result, Schema, SequenceNumber};
use nimbus_storage::JournalProgress;

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TenantProviderRefreshPlan {
    pub refresh_schema: bool,
    pub refresh_journal: bool,
}

impl TenantPersistence {
    pub(crate) async fn load_schema_async(
        &self,
        read_storage: &TenantPersistenceExecutor,
    ) -> Result<Schema> {
        match self {
            Self::Postgres(store) => store.load_schema_async().await,
            Self::Redb(_) | Self::Sqlite(_) | Self::LibsqlReplica(_) | Self::MySql(_) => {
                read_storage.execute(|store| store.load_schema()).await
            }
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
        }
    }

    pub(crate) async fn read_commit_log_from_async(
        &self,
        read_storage: &TenantPersistenceExecutor,
        next_sequence: SequenceNumber,
    ) -> Result<Vec<CommitEntry>> {
        match self {
            Self::Postgres(store) => store.read_commit_log_from_async(next_sequence).await,
            Self::Redb(_) | Self::Sqlite(_) | Self::LibsqlReplica(_) | Self::MySql(_) => {
                read_storage
                    .execute(move |store| store.read_commit_log_from(next_sequence))
                    .await
            }
        }
    }

    pub(crate) async fn recover_journal_tail_async(
        &self,
        read_storage: &TenantPersistenceExecutor,
        next_sequence: SequenceNumber,
    ) -> Result<(JournalProgress, Vec<CommitEntry>)> {
        let progress = self.recover_durable_journal_async(read_storage).await?;
        let commits = if progress.applied_head.0 >= next_sequence.0 {
            self.read_commit_log_from_async(read_storage, next_sequence)
                .await?
        } else {
            Vec::new()
        };
        Ok((progress, commits))
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
        records: &[DurableMutationRecord],
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
