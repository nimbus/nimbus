use std::collections::BTreeSet;
use std::sync::Arc;

use nimbus_core::{Result, SequenceNumber, TableName, TenantEventRecord, TenantId};
use nimbus_storage::{
    ChangefeedBootstrap, ChangefeedCursor, ChangefeedPage, DurableJournal, DurableJournalBootstrap,
    DurableJournalPage, PointInTimeRestoreArchive, PointInTimeRestoreTarget, RetentionGcConfig,
};

use crate::engine::{
    DurableWriteOutcome, DurableWriteRoute, Engine, begin_durable_recovery_eviction,
    classify_durable_write_error,
};
use crate::persistence::TenantPersistence;

// Each helper below is generic over `S: DurableJournal` even though the
// surrounding `execute_journal_read_async`/`TenantPersistenceExecutor`
// plumbing still hands the closure a concrete `TenantPersistence` (SR7
// owns retiring that enum-dispatch layer). Routing the actual storage
// call through a capability-trait-bounded function proves the read
// logic itself only relies on `DurableJournal`, not a `TenantPersistence`
// inherent method.

fn read_durable_journal_from_for_store<S>(
    store: &S,
    from: SequenceNumber,
) -> Result<Vec<TenantEventRecord>>
where
    S: DurableJournal + ?Sized,
{
    store.read_durable_journal_from(from)
}

fn stream_durable_journal_for_store<S>(
    store: &S,
    after: SequenceNumber,
    limit: usize,
) -> Result<DurableJournalPage>
where
    S: DurableJournal + ?Sized,
{
    store.stream_durable_journal(after, limit)
}

fn export_durable_journal_bootstrap_for_store<S>(store: &S) -> Result<DurableJournalBootstrap>
where
    S: DurableJournal + ?Sized,
{
    store.export_durable_journal_bootstrap()
}

fn export_changefeed_bootstrap_for_store<S>(store: &S) -> Result<ChangefeedBootstrap>
where
    S: DurableJournal + ?Sized,
{
    store.export_changefeed_bootstrap()
}

fn stream_changefeed_for_store<S>(
    store: &S,
    cursor: &ChangefeedCursor,
    limit: usize,
) -> Result<ChangefeedPage>
where
    S: DurableJournal + ?Sized,
{
    store.stream_changefeed(cursor, limit)
}

fn latest_sequence_for_store<S>(store: &S) -> Result<SequenceNumber>
where
    S: DurableJournal + ?Sized,
{
    store.latest_sequence()
}

fn applied_sequence_for_store<S>(store: &S) -> Result<SequenceNumber>
where
    S: DurableJournal + ?Sized,
{
    store.applied_sequence()
}

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
            read_durable_journal_from_for_store(&store, from)
        })
        .await
    }

    /// Reads the materialized applied head without exporting tenant state.
    pub(super) async fn applied_sequence_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<SequenceNumber> {
        self.execute_journal_read_async(tenant_id, move |store| applied_sequence_for_store(&store))
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
            stream_durable_journal_for_store(&store, after, limit)
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
            export_durable_journal_bootstrap_for_store(&store)
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
        self.execute_journal_read_async(tenant_id, move |store| {
            export_changefeed_bootstrap_for_store(&store)
        })
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
            stream_changefeed_for_store(&store, &cursor, limit)
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
        let runtime_for_commit = runtime.clone();
        let archive = archive.clone();
        let (projection_token, restored_tables) = runtime.submit_internal_committer(move || {
            runtime_for_commit.ensure_committer_lease_for_assignment()?;
            let expected_previous = runtime_for_commit.durable_head();
            let progress = match runtime_for_commit
                .persist_point_in_time_restore_archive(expected_previous, &archive)
            {
                Ok(progress) => progress,
                Err(error) => {
                    return match classify_durable_write_error(
                        runtime_for_commit.as_ref(),
                        DurableWriteRoute::PointInTimeRestore,
                        expected_previous,
                        error,
                    ) {
                        DurableWriteOutcome::Definitive(error) => Err(error),
                        DurableWriteOutcome::Ambiguous(recovery_error) => {
                            runtime_for_commit.publisher_record_ambiguous_error();
                            begin_durable_recovery_eviction(
                                runtime_for_commit.as_ref(),
                                &recovery_error,
                            );
                            runtime_for_commit.fail_and_drain_mutation_queues(&recovery_error);
                            runtime_for_commit.close_committed_mutation_observers();
                            Err(recovery_error)
                        }
                    };
                }
            };
            runtime_for_commit.publish_mutation_journal_progress_in_actor(progress);
            let next_schema = runtime_for_commit.store().load_schema()?;
            crate::engine::schema::apply_loaded_schema_snapshot(&runtime_for_commit, next_schema)?;
            let projection_token = runtime_for_commit.projection_token()?;
            Ok((projection_token, restored_projection_tables(&archive)))
        })?;
        for table in restored_tables {
            self.notify_table_schema_change_observers(tenant_id, &table, projection_token);
        }
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
        self.execute_journal_read_async(tenant_id, move |store| latest_sequence_for_store(&store))
            .await
    }
}

fn restored_projection_tables(archive: &PointInTimeRestoreArchive) -> Vec<TableName> {
    let mut tables = archive
        .base_snapshot
        .schema
        .tables
        .keys()
        .cloned()
        .chain(
            archive
                .base_snapshot
                .documents
                .iter()
                .map(|document| document.table.clone()),
        )
        .collect::<BTreeSet<_>>();
    for record in &archive.journal_tail {
        tables.extend(TenantEventRecord::as_commit_entry(record).affected_tables());
        tables.extend(record.schema_epoch_tables());
    }
    tables.into_iter().collect()
}
