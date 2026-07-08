use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use nimbus_core::{CommitEntry, SequenceNumber, TableName};

use super::state::{
    PublishedMaterializedTable, RetainedMaterializedTable, estimate_document_bytes,
};
use super::{MaterializedServingBackend, MaterializedTableDocuments, ServingSnapshotManager};

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
            if let Some(writes) = writes_by_table.get(table_name) {
                Self::apply_writes_to_current_version(table_state, commit.sequence, writes);
            } else {
                Self::advance_current_coverage_without_retention(table_state, commit.sequence);
            }
        }
        self.prune_retained_versions_locked(&mut tables);
        self.publish_serving_snapshot_locked(&tables, snapshots);
    }

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
        let mut writes_by_table = HashMap::<&TableName, Vec<&nimbus_core::WriteOp>>::new();
        for commit in commits {
            applied_through = Some(commit.sequence);
            for write in &commit.writes {
                writes_by_table.entry(&write.table).or_default().push(write);
            }
        }
        if let Some(applied_through) = applied_through {
            for (table_name, table_state) in tables.iter_mut() {
                if let Some(writes) = writes_by_table.get(table_name) {
                    Self::apply_writes_to_current_version(table_state, applied_through, writes);
                } else {
                    Self::advance_current_coverage_without_retention(table_state, applied_through);
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

    pub(super) fn publish_table_snapshot(
        &self,
        snapshots: &ServingSnapshotManager,
        table: TableName,
        generation: u64,
        covered_sequence: SequenceNumber,
        documents: MaterializedTableDocuments,
    ) {
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
        let should_publish = match tables.get(&table) {
            Some(existing) => {
                covered_sequence.0 > existing.current.covered_sequence.0
                    || (covered_sequence.0 == existing.current.covered_sequence.0
                        && generation > existing.current.generation)
            }
            None => true,
        };
        if !should_publish {
            return;
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
    use nimbus_core::TableName;

    use super::{MaterializedServingBackend, SequenceNumber, ServingSnapshotManager};

    fn seed_table(
        backend: &MaterializedServingBackend,
        snapshots: &ServingSnapshotManager,
        table: &str,
        covered_sequence: u64,
    ) {
        backend.publish_table_snapshot(
            snapshots,
            TableName::new(table).expect("table name should be valid"),
            1,
            SequenceNumber(covered_sequence),
            Default::default(),
        );
    }

    #[test]
    fn advance_coverage_for_zero_write_commit_widens_only_tables_at_or_above_floor() {
        let backend = MaterializedServingBackend::new();
        let snapshots = ServingSnapshotManager::new();
        // "caught_up" folded the same commit the trigger-delivery cursor was
        // materialized against (covered_sequence == floor): the caller has
        // already proven `(floor, head]` is inert, so it is sound to widen
        // this table's coverage straight through to `head`.
        seed_table(&backend, &snapshots, "caught_up", 5);
        // "lagging" has not folded a real commit that landed before `floor`
        // (covered_sequence < floor): a provider catch-up pass, not this
        // zero-write widening, owns bringing it forward. Widening it here
        // would mark writes it never folded as covered.
        seed_table(&backend, &snapshots, "lagging", 3);

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
}
