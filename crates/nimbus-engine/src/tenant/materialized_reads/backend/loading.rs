use std::collections::HashMap;
use std::sync::atomic::Ordering;

use nimbus_core::{Error, Result, SequenceNumber, TableName};
use nimbus_storage::MaterializedRebuild;
use tracing::warn;

use super::publication::{TableSnapshotPublish, apply_write_to_materialized_documents};
use super::state::estimate_document_bytes;
use super::{MaterializedServingBackend, ServingSnapshot, ServingSnapshotManager};
use crate::tenant::materialized_reads::warm_load::{
    MaterializedWarmLoadDecision, MaterializedWarmLoadPermit,
};

/// Restarts of one table load before the surface reports the table as
/// contended.
///
/// A load restarts when a commit lands between its final applied-sequence read
/// and the publish that takes the `tables` write lock. That window is small, so
/// losing it a few times is ordinary under write load and needs no operator
/// attention. Losing it repeatedly is not ordinary: each restart discards a
/// full table scan, so a table whose write rate keeps closing the window burns
/// scan work without ever becoming serveable. The counter alone cannot say
/// that, because it aggregates every table; the warning names the one that is
/// stuck.
const LOAD_RESTART_WARN_THRESHOLD: u32 = 8;

impl MaterializedServingBackend {
    pub(crate) fn serving_snapshot_for_table_with_mode(
        &self,
        snapshots: &ServingSnapshotManager,
        table: &TableName,
        required_sequence: SequenceNumber,
        count_bypass: bool,
    ) -> Option<ServingSnapshot> {
        let mut access = self
            .access
            .lock()
            .expect("materialized read surface access lock should not be poisoned");
        let mut tables = self
            .tables
            .write()
            .expect("materialized read surface lock should not be poisoned");
        let table_state = tables.get_mut(table)?;
        if table_state.current.covered_sequence.0 < required_sequence.0 {
            if count_bypass {
                self.bypass_count.fetch_add(1, Ordering::Relaxed);
            }
            return None;
        }
        Self::touch_locked(&mut access, table, table_state);
        Self::compact_access_order_locked(&mut access, &tables);
        snapshots.snapshot_covering_table(table, required_sequence)
    }

    pub(crate) fn load_serving_snapshot_cancellable<S>(
        &self,
        snapshots: &ServingSnapshotManager,
        store: &S,
        table: &TableName,
        required_sequence: SequenceNumber,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<ServingSnapshot>
    where
        S: MaterializedRebuild + ?Sized,
    {
        let mut restarts: u32 = 0;
        loop {
            if let Some(snapshot) =
                self.serving_snapshot_for_table_with_mode(snapshots, table, required_sequence, true)
            {
                return Ok(snapshot);
            }

            match self.warm_loads.begin_or_wait_for_warm_load(table) {
                MaterializedWarmLoadDecision::Wait(wait_state) => {
                    wait_state.wait_cancellable(check_cancel)?;
                    continue;
                }
                MaterializedWarmLoadDecision::Load(_owner) => {
                    let _warm_load = MaterializedWarmLoadPermit::new(&self.in_flight_load_count);
                    if let Some(snapshot) = self.serving_snapshot_for_table_with_mode(
                        snapshots,
                        table,
                        required_sequence,
                        false,
                    ) {
                        return Ok(snapshot);
                    }

                    let generation = self.next_generation();
                    check_cancel()?;
                    let starting_sequence = store.applied_sequence()?;
                    let mut materialized_documents = store.scan_table_matching_cancellable(
                        table,
                        check_cancel,
                        |_document| Ok(true),
                    )?;
                    let mut materialized_by_id = materialized_documents
                        .drain(..)
                        .map(|document| (document.id.clone(), document))
                        .collect::<HashMap<_, _>>();
                    let mut document_count = materialized_by_id.len();
                    let mut estimated_bytes = materialized_by_id
                        .values()
                        .map(estimate_document_bytes)
                        .sum::<usize>();
                    let mut replayed_sequence = starting_sequence;

                    loop {
                        check_cancel()?;
                        let target_sequence = store.applied_sequence()?;
                        if replayed_sequence.0 >= target_sequence.0 {
                            #[cfg(test)]
                            self.wait_if_publish_pause_armed();
                            check_cancel()?;
                            let publish_target_sequence = store.applied_sequence()?;
                            if replayed_sequence.0 >= publish_target_sequence.0 {
                                break;
                            }
                            continue;
                        }

                        let commits = store.read_commit_log_from(SequenceNumber(
                            replayed_sequence.0.saturating_add(1),
                        ))?;
                        let commits = commits
                            .into_iter()
                            .take_while(|commit| commit.sequence.0 <= target_sequence.0)
                            .collect::<Vec<_>>();
                        let Some(last_commit) = commits.last() else {
                            return Err(nimbus_core::Error::Internal(format!(
                                "materialized read surface for table {} made no progress while catching up from sequence {} to {}",
                                table, replayed_sequence.0, target_sequence.0
                            )));
                        };
                        for commit in &commits {
                            for write in &commit.writes {
                                if &write.table == table {
                                    apply_write_to_materialized_documents(
                                        &mut materialized_by_id,
                                        &mut document_count,
                                        &mut estimated_bytes,
                                        write,
                                    );
                                }
                            }
                        }
                        replayed_sequence = last_commit.sequence;
                    }

                    self.catch_up_loaded_tables_before_publish(
                        snapshots,
                        store,
                        replayed_sequence,
                    )?;
                    if let TableSnapshotPublish::BehindServingFrontier(frontier) = self
                        .publish_table_snapshot(
                            snapshots,
                            table.clone(),
                            generation,
                            replayed_sequence,
                            materialized_by_id,
                        )
                    {
                        // The surface folded commits past this load between
                        // the last applied-sequence read above and the publish
                        // attempt. The scan is stale as of `frontier`, so
                        // start it over rather than publish a table the
                        // serving history would drop. The retry rescans from
                        // the store's current applied head, which is at or
                        // past `frontier`, so it converges as soon as the
                        // writer that raced this load pauses.
                        debug_assert!(frontier.0 > replayed_sequence.0);
                        self.load_restart_count.fetch_add(1, Ordering::Relaxed);
                        restarts = restarts.saturating_add(1);
                        // Report at the threshold and then on each doubling.
                        // A load that never converges stays visible, and the
                        // log volume it produces grows with the logarithm of
                        // the attempts rather than with the attempts.
                        if restarts >= LOAD_RESTART_WARN_THRESHOLD && restarts.is_power_of_two() {
                            warn!(
                                table = %table,
                                restarts,
                                replayed_sequence = replayed_sequence.0,
                                serving_frontier = frontier.0,
                                required_sequence = required_sequence.0,
                                "materialized table load keeps losing the publish race and is rescanning"
                            );
                        }
                        continue;
                    }
                    return self
                        .serving_snapshot_for_table_with_mode(
                            snapshots,
                            table,
                            required_sequence,
                            true,
                        )
                        .ok_or_else(|| {
                            nimbus_core::Error::Internal(format!(
                                "materialized serving snapshot for sequence {} should be available after loading table {}",
                                required_sequence.0, table
                            ))
                        });
                }
            }
        }
    }

    fn catch_up_loaded_tables_before_publish<S>(
        &self,
        snapshots: &ServingSnapshotManager,
        store: &S,
        target_sequence: SequenceNumber,
    ) -> Result<()>
    where
        S: MaterializedRebuild + ?Sized,
    {
        let earliest_loaded_sequence = {
            let tables = self
                .tables
                .read()
                .expect("materialized read surface lock should not be poisoned");
            tables
                .values()
                .map(|table_state| table_state.current.covered_sequence)
                .min_by_key(|sequence| sequence.0)
        };
        let Some(earliest_loaded_sequence) = earliest_loaded_sequence else {
            return Ok(());
        };
        if earliest_loaded_sequence.0 >= target_sequence.0 {
            return Ok(());
        }

        let commits = store
            .read_commit_log_from(SequenceNumber(earliest_loaded_sequence.0.saturating_add(1)))?;
        let commits = commits
            .into_iter()
            .take_while(|commit| commit.sequence.0 <= target_sequence.0)
            .collect::<Vec<_>>();
        let Some(last_commit) = commits.last() else {
            return Err(Error::Internal(format!(
                "materialized read surface made no progress while catching up loaded tables from sequence {} to {}",
                earliest_loaded_sequence.0, target_sequence.0
            )));
        };
        if last_commit.sequence.0 < target_sequence.0 {
            return Err(Error::Internal(format!(
                "materialized read surface caught up loaded tables only to sequence {}, expected {}",
                last_commit.sequence.0, target_sequence.0
            )));
        }

        self.apply_commits(snapshots, commits.iter());
        Ok(())
    }

    pub(crate) fn clear_publications(&self) {
        self.tables
            .write()
            .expect("materialized read surface lock should not be poisoned")
            .clear();
        for wait_state in self.warm_loads.clear() {
            wait_state.mark_completed();
        }
        *self
            .access
            .lock()
            .expect("materialized read surface access lock should not be poisoned") =
            super::state::MaterializedReadAccessState::default();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use nimbus_core::{
        CommitEntry, Document, Result, SequenceNumber, TableName, TenantEventRecord, Timestamp,
    };
    use nimbus_storage::{
        DurableJournal, DurableJournalBootstrap, DurableJournalPage, JournalProgress,
        MaterializedRebuild, PointInTimeRestoreArchive, PointInTimeRestoreTarget,
        RetentionGcConfig,
    };

    use super::super::ServingSnapshotManager;
    use super::super::publication::TableSnapshotPublish;
    use super::super::{MaterializedServingBackend, MaterializedTableDocuments};

    fn table_name(table: &str) -> TableName {
        TableName::new(table).expect("table name should be valid")
    }

    /// A store whose applied head stands still for a fixed number of reads.
    ///
    /// The publish race this reproduces is otherwise a timing accident: a
    /// commit has to land between a load's last applied-sequence read and the
    /// write lock the publish takes. Holding the head below an already-resident
    /// table's coverage makes the same rejection happen on demand, and
    /// releasing it after `stall_reads` proves the retry converges rather than
    /// spinning forever.
    struct StalledHeadStore {
        stalled_sequence: SequenceNumber,
        released_sequence: SequenceNumber,
        stall_reads: usize,
        applied_reads: AtomicUsize,
        scans: AtomicU64,
    }

    impl StalledHeadStore {
        fn new(stalled: u64, released: u64, stall_reads: usize) -> Self {
            Self {
                stalled_sequence: SequenceNumber(stalled),
                released_sequence: SequenceNumber(released),
                stall_reads,
                applied_reads: AtomicUsize::new(0),
                scans: AtomicU64::new(0),
            }
        }

        fn scan_count(&self) -> u64 {
            self.scans.load(Ordering::Relaxed)
        }
    }

    impl MaterializedRebuild for StalledHeadStore {
        fn scan_table_matching_cancellable<F>(
            &self,
            _table: &TableName,
            check_cancel: &mut dyn FnMut() -> Result<()>,
            _include_document: F,
        ) -> Result<Vec<Document>>
        where
            F: FnMut(&Document) -> Result<bool>,
        {
            check_cancel()?;
            self.scans.fetch_add(1, Ordering::Relaxed);
            Ok(Vec::new())
        }
    }

    impl DurableJournal for StalledHeadStore {
        fn applied_sequence(&self) -> Result<SequenceNumber> {
            let read = self.applied_reads.fetch_add(1, Ordering::Relaxed);
            Ok(if read < self.stall_reads {
                self.stalled_sequence
            } else {
                self.released_sequence
            })
        }

        fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>> {
            Ok((sequence.0..=self.released_sequence.0)
                .map(|sequence| CommitEntry {
                    sequence: SequenceNumber(sequence),
                    timestamp: Timestamp::from_unix_millis(sequence),
                    writes: Vec::new(),
                })
                .collect())
        }

        fn journal_progress(&self) -> Result<JournalProgress> {
            unimplemented!("the load path does not read journal progress")
        }

        fn read_durable_journal_from(
            &self,
            _sequence: SequenceNumber,
        ) -> Result<Vec<TenantEventRecord>> {
            unimplemented!("the load path does not read the durable journal")
        }

        fn stream_durable_journal(
            &self,
            _after: SequenceNumber,
            _limit: usize,
        ) -> Result<DurableJournalPage> {
            unimplemented!("the load path does not stream the durable journal")
        }

        fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
            unimplemented!("the load path does not export a bootstrap")
        }

        fn latest_sequence(&self) -> Result<SequenceNumber> {
            unimplemented!("the load path reads the applied head, not the durable head")
        }

        fn recover_durable_journal(&self) -> Result<JournalProgress> {
            unimplemented!("the load path does not recover")
        }

        fn append_durable_records_batch(&self, _records: &[TenantEventRecord]) -> Result<()> {
            unimplemented!("the load path does not write")
        }

        fn apply_durable_records_batch(&self, _records: &[TenantEventRecord]) -> Result<()> {
            unimplemented!("the load path does not write")
        }

        fn export_point_in_time_restore_archive(
            &self,
            _target: PointInTimeRestoreTarget,
            _retention_config: RetentionGcConfig,
        ) -> Result<PointInTimeRestoreArchive> {
            unimplemented!("the load path does not export an archive")
        }

        fn import_point_in_time_restore_archive(
            &self,
            _archive: &PointInTimeRestoreArchive,
        ) -> Result<JournalProgress> {
            unimplemented!("the load path does not import an archive")
        }
    }

    #[test]
    fn a_load_rejected_behind_the_serving_frontier_counts_its_restart_and_then_converges() {
        let backend = MaterializedServingBackend::new();
        let snapshots = ServingSnapshotManager::new();
        // A resident table already folded through 5, so it holds the serving
        // frontier there. Anything published below 5 is rejected.
        assert_eq!(
            backend.publish_table_snapshot(
                &snapshots,
                table_name("ahead"),
                1,
                SequenceNumber(5),
                MaterializedTableDocuments::default(),
            ),
            TableSnapshotPublish::Published,
        );
        assert_eq!(
            backend.stats().load_restart_count,
            0,
            "an accepted publication is not a restart"
        );

        // One load attempt reads the applied head three times: once for its
        // scan's starting sequence, then the catch-up loop's target read and
        // its confirming re-read. Holding all three at 3 makes that attempt
        // replay to 3 and offer a table behind the frontier at 5, so it is
        // rejected. The restart reads 5 and is accepted.
        let store = StalledHeadStore::new(3, 5, 3);
        let snapshot = backend
            .load_serving_snapshot_cancellable(
                &snapshots,
                &store,
                &table_name("behind"),
                SequenceNumber(5),
                &mut || Ok(()),
            )
            .expect("the load should converge once the applied head reaches the frontier");

        assert!(
            snapshot
                .table_document_count(&table_name("behind"))
                .is_some(),
            "the converged load should serve the table it loaded"
        );
        assert_eq!(
            backend.stats().load_restart_count,
            1,
            "the rejected attempt should be counted where an operator can see it"
        );
        assert_eq!(
            store.scan_count(),
            2,
            "a restart rescans the table -- the cost the counter exists to expose"
        );
        assert_eq!(
            backend.stats().table_load_count,
            2,
            "a restart is not a publication; only 'ahead' and the converged 'behind' publish"
        );
    }
}
