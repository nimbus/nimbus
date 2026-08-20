use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use nimbus_core::{CommitEntry, SequenceNumber, TableName};

use super::state::{
    PublishedMaterializedTable, RetainedMaterializedTable, estimate_document_bytes,
};
use super::{MaterializedServingBackend, MaterializedTableDocuments, ServingSnapshotManager};

/// What became of a freshly loaded table offered to the serving surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub(super) enum TableSnapshotPublish {
    /// The table joined the surface and is serveable at its covered sequence.
    Published,
    /// The surface had already folded commits past the sequence this load
    /// replayed to, so the table was rejected and nothing was mutated. The
    /// caller must reload from the carried frontier.
    BehindServingFrontier(SequenceNumber),
}

impl MaterializedServingBackend {
    pub(crate) fn apply_commit(&self, snapshots: &ServingSnapshotManager, commit: &CommitEntry) {
        let mut tables = self
            .tables
            .write()
            .expect("materialized read surface lock should not be poisoned");
        let mut writes_by_table = HashMap::<&TableName, Vec<&nimbus_core::WriteOp>>::new();
        for write in &commit.writes {
            writes_by_table.entry(&write.table).or_default().push(write);
        }
        for (table_name, table_state) in tables.iter_mut() {
            if table_state.current.covered_sequence.0 >= commit.sequence.0 {
                // A table load can publish at the store's applied head before
                // the pipeline invalidates the commits that head already
                // contains, so a resident table can legitimately be at or past
                // the commit being applied here. Folding it again would
                // re-apply writes it already contains -- resurrecting a
                // document a later commit deleted -- and reporting the
                // commit's sequence would drag its coverage backwards.
                continue;
            }
            if let Some(writes) = writes_by_table.get(table_name) {
                Self::apply_writes_to_current_version(table_state, commit.sequence, writes);
            } else {
                Self::advance_current_coverage_without_retention(table_state, commit.sequence);
            }
        }
        self.prune_retained_versions_locked(&mut tables);
        self.publish_serving_snapshot_locked(&tables, snapshots);
    }

    /// Folds a run of commits into every resident table.
    ///
    /// Each table folds only the suffix of the run that is past its own
    /// `covered_sequence`, and a table already at or past the run's end is
    /// left alone. Tables do not all sit at the same coverage: a warm load
    /// publishes its table at the sequence it replayed to, and
    /// `catch_up_loaded_tables_before_publish` replays from the *earliest*
    /// resident coverage, so a run routinely spans commits some tables have
    /// already folded. Applying the whole run to all of them would re-insert
    /// documents a later commit in the run deleted, and stamping every table
    /// with the run's end would move an already-further-ahead table's
    /// coverage backwards.
    pub(crate) fn apply_commits<'a>(
        &self,
        snapshots: &ServingSnapshotManager,
        commits: impl IntoIterator<Item = &'a CommitEntry>,
    ) {
        let mut tables = self
            .tables
            .write()
            .expect("materialized read surface lock should not be poisoned");
        let mut applied_through = None;
        let mut writes_by_table =
            HashMap::<&TableName, Vec<(SequenceNumber, &nimbus_core::WriteOp)>>::new();
        for commit in commits {
            applied_through = Some(commit.sequence);
            for write in &commit.writes {
                writes_by_table
                    .entry(&write.table)
                    .or_default()
                    .push((commit.sequence, write));
            }
        }
        if let Some(applied_through) = applied_through {
            for (table_name, table_state) in tables.iter_mut() {
                let covered_sequence = table_state.current.covered_sequence;
                if covered_sequence.0 >= applied_through.0 {
                    continue;
                }
                let writes = writes_by_table
                    .get(table_name)
                    .map(|writes| {
                        writes
                            .iter()
                            .filter(|(sequence, _)| sequence.0 > covered_sequence.0)
                            .map(|(_, write)| *write)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if writes.is_empty() {
                    Self::advance_current_coverage_without_retention(table_state, applied_through);
                } else {
                    Self::apply_writes_to_current_version(table_state, applied_through, &writes);
                }
            }
            self.prune_retained_versions_locked(&mut tables);
            self.publish_serving_snapshot_locked(&tables, snapshots);
        }
    }

    fn publish_serving_snapshot_locked(
        &self,
        tables: &HashMap<TableName, RetainedMaterializedTable>,
        snapshots: &ServingSnapshotManager,
    ) {
        let Some(snapshot) = Self::current_serving_snapshot_from_locked_tables(tables) else {
            snapshots.clear();
            return;
        };
        snapshots.publish(snapshot, self.current_version_capacity());
    }

    /// Publishes a freshly loaded table into the serving surface.
    ///
    /// Rejects a table that arrives behind the serving frontier instead of
    /// taking it. Such a table is unusable twice over: the snapshot built from
    /// it covers the new minimum across resident tables, which
    /// `ServingSnapshotManager::publish` drops as out of order -- leaving the
    /// table absent from every retained snapshot, so reads of it fail even
    /// though its own coverage satisfies them -- and it would sit in the
    /// surface missing the writes from the commits already folded before it
    /// arrived. The frontier check runs under the same `tables` write lock as
    /// the insert, so a commit applied concurrently with a load cannot slip
    /// between them.
    pub(super) fn publish_table_snapshot(
        &self,
        snapshots: &ServingSnapshotManager,
        table: TableName,
        generation: u64,
        covered_sequence: SequenceNumber,
        documents: MaterializedTableDocuments,
    ) -> TableSnapshotPublish {
        let document_count = documents.len();
        let estimated_bytes = documents
            .values()
            .map(estimate_document_bytes)
            .sum::<usize>();
        let mut access = self
            .access
            .lock()
            .expect("materialized read surface access lock should not be poisoned");
        let mut tables = self
            .tables
            .write()
            .expect("materialized read surface lock should not be poisoned");
        if let Some(frontier) = Self::serving_frontier_locked(&tables, snapshots)
            && covered_sequence.0 < frontier.0
        {
            return TableSnapshotPublish::BehindServingFrontier(frontier);
        }
        let should_publish = match tables.get(&table) {
            Some(existing) => {
                covered_sequence.0 > existing.current.covered_sequence.0
                    || (covered_sequence.0 == existing.current.covered_sequence.0
                        && generation > existing.current.generation)
            }
            None => true,
        };
        if !should_publish {
            return TableSnapshotPublish::Published;
        }
        access.next_access_stamp = access.next_access_stamp.wrapping_add(1);
        if access.next_access_stamp == 0 {
            access.next_access_stamp = 1;
        }
        let access_stamp = access.next_access_stamp;
        let next_current = PublishedMaterializedTable {
            generation,
            covered_sequence,
            document_count,
            estimated_bytes,
            documents: Arc::new(documents),
        };
        match tables.get_mut(&table) {
            Some(existing) => {
                if next_current.covered_sequence.0 > existing.current.covered_sequence.0 {
                    existing.retained.push_back(PublishedMaterializedTable {
                        generation: existing.current.generation,
                        covered_sequence: existing.current.covered_sequence,
                        document_count: existing.current.document_count,
                        estimated_bytes: existing.current.estimated_bytes,
                        documents: existing.current.documents.clone(),
                    });
                }
                existing.current = next_current;
                existing.access_stamp = access_stamp;
            }
            None => {
                tables.insert(
                    table.clone(),
                    RetainedMaterializedTable {
                        access_stamp,
                        current: next_current,
                        retained: VecDeque::new(),
                    },
                );
            }
        }
        access.access_order.push_back((table, access_stamp));
        Self::compact_access_order_locked(&mut access, &tables);
        self.evict_if_needed_locked(&mut tables, &mut access);
        self.publish_serving_snapshot_locked(&tables, snapshots);
        self.table_load_count.fetch_add(1, Ordering::Relaxed);
        TableSnapshotPublish::Published
    }

    /// The sequence a freshly loaded table has to reach to be serveable.
    ///
    /// Takes the higher of what the resident tables have folded and what the
    /// snapshot manager last published: the two can differ. Eviction can empty
    /// the resident set while the manager still retains snapshots covering
    /// later sequences, and a deliberately lagging table holds the published
    /// coverage below the folded frontier.
    fn serving_frontier_locked(
        tables: &HashMap<TableName, RetainedMaterializedTable>,
        snapshots: &ServingSnapshotManager,
    ) -> Option<SequenceNumber> {
        let folded = tables
            .values()
            .map(|table_state| table_state.current.covered_sequence)
            .max_by_key(|sequence| sequence.0);
        let published = snapshots.latest_covered_sequence();
        match (folded, published) {
            (Some(folded), Some(published)) if published.0 > folded.0 => Some(published),
            (Some(folded), _) => Some(folded),
            (None, published) => published,
        }
    }

    fn advance_current_coverage_without_retention(
        table_state: &mut RetainedMaterializedTable,
        covered_sequence: SequenceNumber,
    ) {
        table_state.current.covered_sequence = covered_sequence;
    }

    /// Carries a loaded table's coverage frontier through to `head` without
    /// touching a single document, PROVIDED the table is already known to
    /// have folded everything through `floor`. The trigger-candidate feed's
    /// own delivery-cursor advance is exactly such a widening commit: a
    /// zero-write entry appended to the same commit log and sequence space
    /// real document writes use, purely to record how far it has delivered.
    /// Because it carries no writes, it can never change what any
    /// materialized-serving snapshot serves -- but only for a table that
    /// provably has no *other*, unfolded write sitting in `(floor, head]`.
    /// `floor` is the sequence of the commit the caller just materialized
    /// invocations for; the caller has already verified every record in
    /// `(floor, head]` is provably inert before calling this, which makes
    /// widening sound for any table whose `covered_sequence` is already
    /// `>= floor` (the same way `apply_commit` already carries an untouched
    /// table through a real commit via
    /// `advance_current_coverage_without_retention`). A table whose
    /// `covered_sequence < floor` is lagging behind real commits a
    /// provider-catch-up pass owns folding in -- it must NOT be carried
    /// past writes it has not folded, even though the `(floor, head]` gap
    /// itself is inert, so it is left untouched here and simply reloads on
    /// its next query.
    ///
    /// Publishes the widened snapshot via `extend_latest_coverage` rather
    /// than `publish_serving_snapshot_locked`: a zero-write commit produces a
    /// snapshot whose table contents are identical to the one already
    /// published, so it extends the latest retained version's coverage in
    /// place instead of retaining a content-free duplicate history entry.
    pub(crate) fn advance_coverage_for_zero_write_commit(
        &self,
        snapshots: &ServingSnapshotManager,
        floor: SequenceNumber,
        head: SequenceNumber,
    ) {
        let mut tables = self
            .tables
            .write()
            .expect("materialized read surface lock should not be poisoned");
        if tables.is_empty() {
            return;
        }
        let mut advanced = false;
        for table_state in tables.values_mut() {
            let covered = table_state.current.covered_sequence.0;
            if covered >= floor.0 && head.0 > covered {
                Self::advance_current_coverage_without_retention(table_state, head);
                advanced = true;
            }
        }
        if advanced
            && let Some(snapshot) = Self::current_serving_snapshot_from_locked_tables(&tables)
        {
            snapshots.extend_latest_coverage(snapshot);
        }
    }

    fn apply_writes_to_current_version(
        table_state: &mut RetainedMaterializedTable,
        covered_sequence: SequenceNumber,
        writes: &[&nimbus_core::WriteOp],
    ) {
        let mut next_documents = table_state.current.documents.clone();
        let mut next_document_count = table_state.current.document_count;
        let mut next_estimated_bytes = table_state.current.estimated_bytes;
        for write in writes {
            let documents = Arc::make_mut(&mut next_documents);
            apply_write_to_materialized_documents(
                documents,
                &mut next_document_count,
                &mut next_estimated_bytes,
                write,
            );
        }
        table_state.retained.push_back(PublishedMaterializedTable {
            generation: table_state.current.generation,
            covered_sequence: table_state.current.covered_sequence,
            document_count: table_state.current.document_count,
            estimated_bytes: table_state.current.estimated_bytes,
            documents: table_state.current.documents.clone(),
        });
        table_state.current = PublishedMaterializedTable {
            generation: table_state.current.generation,
            covered_sequence,
            document_count: next_document_count,
            estimated_bytes: next_estimated_bytes,
            documents: next_documents,
        };
    }
}

pub(super) fn apply_write_to_materialized_documents(
    documents: &mut MaterializedTableDocuments,
    document_count: &mut usize,
    estimated_bytes: &mut usize,
    write: &nimbus_core::WriteOp,
) {
    match &write.current {
        Some(document) => {
            let next_size = estimate_document_bytes(document);
            match documents.insert(write.doc_id.clone(), document.clone()) {
                Some(previous) => {
                    *estimated_bytes = estimated_bytes
                        .saturating_sub(estimate_document_bytes(&previous))
                        .saturating_add(next_size);
                }
                None => {
                    *document_count = document_count.saturating_add(1);
                    *estimated_bytes = estimated_bytes.saturating_add(next_size);
                }
            }
        }
        None => {
            if let Some(previous) = documents.remove(&write.doc_id) {
                *document_count = document_count.saturating_sub(1);
                *estimated_bytes =
                    estimated_bytes.saturating_sub(estimate_document_bytes(&previous));
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use nimbus_core::{
        CommitEntry, Document, DocumentId, TableId, TableName, Timestamp, WriteOp, WriteOpType,
    };

    use super::{
        MaterializedServingBackend, MaterializedTableDocuments, SequenceNumber,
        ServingSnapshotManager, TableSnapshotPublish,
    };

    fn table_name(table: &str) -> TableName {
        TableName::new(table).expect("table name should be valid")
    }

    fn seed_table(
        backend: &MaterializedServingBackend,
        snapshots: &ServingSnapshotManager,
        table: &str,
        covered_sequence: u64,
    ) {
        seed_table_with_documents(
            backend,
            snapshots,
            table,
            covered_sequence,
            MaterializedTableDocuments::default(),
        );
    }

    fn seed_table_with_documents(
        backend: &MaterializedServingBackend,
        snapshots: &ServingSnapshotManager,
        table: &str,
        covered_sequence: u64,
        documents: MaterializedTableDocuments,
    ) {
        assert_eq!(
            backend.publish_table_snapshot(
                snapshots,
                table_name(table),
                1,
                SequenceNumber(covered_sequence),
                documents,
            ),
            TableSnapshotPublish::Published,
            "seeding {table} at sequence {covered_sequence} should be accepted"
        );
    }

    fn document(table: &str, id: &DocumentId) -> Document {
        Document::with_id(
            id.clone(),
            table_name(table),
            serde_json::Map::from_iter([("body".to_string(), serde_json::json!("seeded"))]),
        )
    }

    fn insert_commit(sequence: u64, table: &str, id: &DocumentId) -> CommitEntry {
        write_commit(
            sequence,
            table,
            id,
            WriteOpType::Insert,
            Some(document(table, id)),
        )
    }

    fn write_commit(
        sequence: u64,
        table: &str,
        id: &DocumentId,
        op_type: WriteOpType,
        current: Option<Document>,
    ) -> CommitEntry {
        CommitEntry {
            sequence: SequenceNumber(sequence),
            timestamp: Timestamp::from_unix_millis(sequence),
            writes: vec![WriteOp {
                table: table_name(table),
                table_id: TableId::new(),
                op_type,
                doc_id: id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current,
            }],
        }
    }

    #[test]
    fn advance_coverage_for_zero_write_commit_widens_only_tables_at_or_above_floor() {
        let backend = MaterializedServingBackend::new();
        let snapshots = ServingSnapshotManager::new();
        // "caught_up" folded the same commit the trigger-delivery cursor was
        // materialized against (covered_sequence == floor): the caller has
        // already proven `(floor, head]` is inert, so it is sound to widen
        // this table's coverage straight through to `head`.
        // "lagging" has not folded a real commit that landed before `floor`
        // (covered_sequence < floor): a provider catch-up pass, not this
        // zero-write widening, owns bringing it forward. Widening it here
        // would mark writes it never folded as covered. It is seeded first
        // because the surface refuses a table that arrives behind the serving
        // frontier.
        seed_table(&backend, &snapshots, "lagging", 3);
        seed_table(&backend, &snapshots, "caught_up", 5);

        let floor = SequenceNumber(5);
        let head = SequenceNumber(8);
        backend.advance_coverage_for_zero_write_commit(&snapshots, floor, head);

        let caught_up = backend
            .table_publication_stats(&TableName::new("caught_up").expect("valid table name"))
            .expect("caught_up table should be published");
        assert_eq!(caught_up.covered_sequence, head);

        let lagging = backend
            .table_publication_stats(&TableName::new("lagging").expect("valid table name"))
            .expect("lagging table should be published");
        assert_eq!(
            lagging.covered_sequence,
            SequenceNumber(3),
            "a table lagging behind the verified-inert floor must not be advanced"
        );
    }

    #[test]
    fn a_table_loaded_behind_the_serving_frontier_is_rejected_instead_of_published_unserveable() {
        let backend = MaterializedServingBackend::new();
        let snapshots = ServingSnapshotManager::new();
        seed_table(&backend, &snapshots, "ahead", 8);

        // A warm load that read the applied head as 7, then lost the race with
        // the commit that carried "ahead" to 8.
        let behind = table_name("behind");
        assert_eq!(
            backend.publish_table_snapshot(
                &snapshots,
                behind.clone(),
                1,
                SequenceNumber(7),
                MaterializedTableDocuments::default(),
            ),
            TableSnapshotPublish::BehindServingFrontier(SequenceNumber(8)),
        );
        assert!(
            backend.table_publication_stats(&behind).is_none(),
            "a rejected load must leave the surface untouched"
        );

        // Reloaded at the frontier it is accepted, and -- the point of the
        // rejection -- it is actually serveable. Taking it at 7 would have
        // published a snapshot covering 7, which the manager drops as out of
        // order behind the snapshot covering 8, leaving no retained snapshot
        // that contains the table at all.
        seed_table(&backend, &snapshots, "behind", 8);
        assert!(
            backend
                .serving_snapshot_for_table_with_mode(&snapshots, &behind, SequenceNumber(8), false)
                .is_some(),
            "a table published at the frontier must be serveable at it"
        );
    }

    #[test]
    fn folding_a_catch_up_run_leaves_a_table_that_is_past_it_untouched() {
        let backend = MaterializedServingBackend::new();
        let snapshots = ServingSnapshotManager::new();
        let doomed = DocumentId::new();
        let survivor = DocumentId::new();

        // "lagging" stopped at 3. It is what makes the catch-up run below
        // start before the commits "ahead" has already folded, and it is
        // seeded first because the surface refuses a table that arrives
        // behind the serving frontier.
        let mut lagging_documents = MaterializedTableDocuments::default();
        lagging_documents.insert(survivor.clone(), document("lagging", &survivor));
        seed_table_with_documents(&backend, &snapshots, "lagging", 3, lagging_documents);
        // "ahead" was loaded at 6: it folded the insert at 4 and the delete at
        // 6 that removed the same document again, so it holds nothing.
        seed_table_with_documents(
            &backend,
            &snapshots,
            "ahead",
            6,
            MaterializedTableDocuments::default(),
        );

        backend.apply_commits(
            &snapshots,
            [
                insert_commit(4, "ahead", &doomed),
                insert_commit(5, "lagging", &DocumentId::new()),
            ]
            .iter(),
        );

        let ahead = backend
            .table_publication_stats(&table_name("ahead"))
            .expect("ahead table should stay published");
        assert_eq!(
            ahead.covered_sequence,
            SequenceNumber(6),
            "a table past the end of the run must not be dragged backwards"
        );
        assert_eq!(
            ahead.document_count, 0,
            "re-folding the run would resurrect a document a later commit deleted"
        );

        let lagging = backend
            .table_publication_stats(&table_name("lagging"))
            .expect("lagging table should stay published");
        assert_eq!(
            lagging.covered_sequence,
            SequenceNumber(5),
            "a table behind the run must be carried to its end"
        );
        assert_eq!(lagging.document_count, 2);
    }
}
