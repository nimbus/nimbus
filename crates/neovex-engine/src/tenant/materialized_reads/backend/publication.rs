use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use neovex_core::{CommitEntry, SequenceNumber, TableName};

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
        let mut writes_by_table = HashMap::<&TableName, Vec<&neovex_core::WriteOp>>::new();
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
        let mut writes_by_table = HashMap::<&TableName, Vec<&neovex_core::WriteOp>>::new();
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

    fn apply_writes_to_current_version(
        table_state: &mut RetainedMaterializedTable,
        covered_sequence: SequenceNumber,
        writes: &[&neovex_core::WriteOp],
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
    write: &neovex_core::WriteOp,
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
