use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use neovex_core::{CommitEntry, Document, Result, SequenceNumber, TableName};
use neovex_storage::TenantStore;

#[cfg(test)]
use super::pause::{MaterializedReadPublishPauseHandle, MaterializedReadPublishPauseState};
use super::snapshot::{MaterializedTableDocuments, ServingSnapshot, ServingSnapshotManager};
use super::stats::MaterializedServingBackendStats;
#[cfg(test)]
use super::stats::MaterializedTablePublicationStats;
use super::warm_load::{
    MaterializedWarmLoadCoordinator, MaterializedWarmLoadDecision, MaterializedWarmLoadPermit,
};
use super::{
    DEFAULT_MATERIALIZED_SURFACE_BYTE_CAPACITY, DEFAULT_MATERIALIZED_SURFACE_TABLE_CAPACITY,
    DEFAULT_MATERIALIZED_SURFACE_VERSION_CAPACITY,
};

struct PublishedMaterializedTable {
    generation: u64,
    covered_sequence: SequenceNumber,
    document_count: usize,
    estimated_bytes: usize,
    documents: Arc<MaterializedTableDocuments>,
}

struct RetainedMaterializedTable {
    access_stamp: u64,
    current: PublishedMaterializedTable,
    retained: VecDeque<PublishedMaterializedTable>,
}

#[derive(Default)]
struct MaterializedReadAccessState {
    access_order: VecDeque<(TableName, u64)>,
    next_access_stamp: u64,
}

// Lock ordering for multi-lock materialized-read operations is
// `backend.access -> backend.tables -> snapshots.state`. Keep that order when
// touching more than one of these locks in the same path.
pub(super) struct MaterializedServingBackend {
    tables: RwLock<HashMap<TableName, RetainedMaterializedTable>>,
    access: Mutex<MaterializedReadAccessState>,
    warm_loads: MaterializedWarmLoadCoordinator,
    next_generation: AtomicU64,
    table_capacity: AtomicUsize,
    byte_capacity: AtomicUsize,
    version_capacity: AtomicUsize,
    table_load_count: AtomicU64,
    bypass_count: AtomicU64,
    eviction_count: AtomicU64,
    in_flight_load_count: AtomicU64,
    #[cfg(test)]
    pause_before_publish: Arc<MaterializedReadPublishPauseState>,
}

impl MaterializedServingBackend {
    pub(super) fn new() -> Self {
        Self {
            tables: RwLock::new(HashMap::new()),
            access: Mutex::new(MaterializedReadAccessState::default()),
            warm_loads: MaterializedWarmLoadCoordinator::default(),
            next_generation: AtomicU64::new(0),
            table_capacity: AtomicUsize::new(DEFAULT_MATERIALIZED_SURFACE_TABLE_CAPACITY),
            byte_capacity: AtomicUsize::new(DEFAULT_MATERIALIZED_SURFACE_BYTE_CAPACITY),
            version_capacity: AtomicUsize::new(DEFAULT_MATERIALIZED_SURFACE_VERSION_CAPACITY),
            table_load_count: AtomicU64::new(0),
            bypass_count: AtomicU64::new(0),
            eviction_count: AtomicU64::new(0),
            in_flight_load_count: AtomicU64::new(0),
            #[cfg(test)]
            pause_before_publish: Arc::new(MaterializedReadPublishPauseState::default()),
        }
    }

    pub(super) fn serving_snapshot_for_table_with_mode(
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

    pub(super) fn load_serving_snapshot_cancellable(
        &self,
        snapshots: &ServingSnapshotManager,
        store: &TenantStore,
        table: &TableName,
        required_sequence: SequenceNumber,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<ServingSnapshot> {
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
                        .map(|document| (document.id, document))
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
                            return Err(neovex_core::Error::Internal(format!(
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

                    self.publish_table_snapshot(
                        snapshots,
                        table.clone(),
                        generation,
                        replayed_sequence,
                        materialized_by_id,
                    );
                    return self
                        .serving_snapshot_for_table_with_mode(
                            snapshots,
                            table,
                            required_sequence,
                            true,
                        )
                        .ok_or_else(|| {
                            neovex_core::Error::Internal(format!(
                                "materialized serving snapshot for sequence {} should be available after loading table {}",
                                required_sequence.0, table
                            ))
                        });
                }
            }
        }
    }

    pub(super) fn apply_commit(&self, snapshots: &ServingSnapshotManager, commit: &CommitEntry) {
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

    pub(super) fn apply_commits<'a>(
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

    pub(super) fn clear_publications(&self) {
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
            MaterializedReadAccessState::default();
    }

    pub(super) fn stats(&self) -> MaterializedServingBackendStats {
        let tables = self
            .tables
            .read()
            .expect("materialized read surface lock should not be poisoned");
        MaterializedServingBackendStats {
            loaded_table_count: tables.len(),
            resident_document_count: tables
                .values()
                .map(|state| state.current.document_count)
                .sum(),
            resident_estimated_bytes: tables
                .values()
                .map(|state| state.current.estimated_bytes)
                .sum(),
            retained_version_count: tables.values().map(|state| state.retained.len()).sum(),
            retained_estimated_bytes: tables.values().map(Self::retained_bytes).sum(),
            table_capacity: self.table_capacity.load(Ordering::Relaxed),
            byte_capacity: self.byte_capacity.load(Ordering::Relaxed),
            version_capacity: self.version_capacity.load(Ordering::Relaxed),
            table_load_count: self.table_load_count.load(Ordering::Relaxed),
            bypass_count: self.bypass_count.load(Ordering::Relaxed),
            eviction_count: self.eviction_count.load(Ordering::Relaxed),
            in_flight_load_count: self.in_flight_load_count.load(Ordering::Relaxed),
            earliest_covered_sequence: tables
                .values()
                .map(|state| state.current.covered_sequence)
                .min_by_key(|sequence| sequence.0),
            latest_covered_sequence: tables
                .values()
                .map(|state| state.current.covered_sequence)
                .max_by_key(|sequence| sequence.0),
            earliest_retained_sequence: tables
                .values()
                .flat_map(|state| {
                    state
                        .retained
                        .iter()
                        .map(|version| version.covered_sequence)
                })
                .min_by_key(|sequence| sequence.0),
            latest_retained_sequence: tables
                .values()
                .flat_map(|state| {
                    state
                        .retained
                        .iter()
                        .map(|version| version.covered_sequence)
                })
                .max_by_key(|sequence| sequence.0),
        }
    }

    #[cfg(test)]
    pub(super) fn table_publication_stats(
        &self,
        table: &TableName,
    ) -> Option<MaterializedTablePublicationStats> {
        self.tables
            .read()
            .expect("materialized read surface lock should not be poisoned")
            .get(table)
            .map(|state| MaterializedTablePublicationStats {
                generation: state.current.generation,
                covered_sequence: state.current.covered_sequence,
                document_count: state.current.documents.len(),
                estimated_bytes: state.current.estimated_bytes,
            })
    }

    #[cfg(test)]
    pub(super) fn publish_pause_handle(&self) -> MaterializedReadPublishPauseHandle {
        MaterializedReadPublishPauseHandle {
            state: self.pause_before_publish.clone(),
        }
    }

    #[cfg(test)]
    pub(super) fn set_limits_for_testing(&self, table_capacity: usize, byte_capacity: usize) {
        self.table_capacity
            .store(table_capacity.max(1), Ordering::Relaxed);
        self.byte_capacity
            .store(byte_capacity.max(1), Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(super) fn set_version_capacity_for_testing(&self, version_capacity: usize) {
        self.version_capacity
            .store(version_capacity.max(1), Ordering::Relaxed);
    }

    fn next_generation(&self) -> u64 {
        self.next_generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn current_limits(&self) -> (usize, usize) {
        (
            self.table_capacity.load(Ordering::Relaxed).max(1),
            self.byte_capacity.load(Ordering::Relaxed).max(1),
        )
    }

    fn current_version_capacity(&self) -> usize {
        self.version_capacity.load(Ordering::Relaxed).max(1)
    }

    fn touch_locked(
        access: &mut MaterializedReadAccessState,
        table: &TableName,
        table_state: &mut RetainedMaterializedTable,
    ) {
        access.next_access_stamp = access.next_access_stamp.wrapping_add(1);
        if access.next_access_stamp == 0 {
            access.next_access_stamp = 1;
        }
        table_state.access_stamp = access.next_access_stamp;
        access
            .access_order
            .push_back((table.clone(), table_state.access_stamp));
    }

    fn compact_access_order_locked(
        access: &mut MaterializedReadAccessState,
        tables: &HashMap<TableName, RetainedMaterializedTable>,
    ) {
        let threshold = tables.len().max(1).saturating_mul(8).max(64);
        if access.access_order.len() <= threshold {
            return;
        }

        access.access_order.retain(|(table, stamp)| {
            tables
                .get(table)
                .is_some_and(|state| state.access_stamp == *stamp)
        });
    }

    fn retained_bytes(table_state: &RetainedMaterializedTable) -> usize {
        table_state
            .retained
            .iter()
            .map(|version| version.estimated_bytes)
            .sum::<usize>()
    }

    fn total_version_bytes(tables: &HashMap<TableName, RetainedMaterializedTable>) -> usize {
        tables
            .values()
            .map(|state| state.current.estimated_bytes + Self::retained_bytes(state))
            .sum()
    }

    fn prune_retained_versions_locked(
        &self,
        tables: &mut HashMap<TableName, RetainedMaterializedTable>,
    ) {
        let version_capacity = self.current_version_capacity();
        for table_state in tables.values_mut() {
            while table_state.retained.len().saturating_add(1) > version_capacity {
                table_state.retained.pop_front();
            }
        }

        let (_, byte_capacity) = self.current_limits();
        while Self::total_version_bytes(tables) > byte_capacity {
            let mut oldest_table = None;
            let mut oldest_sequence: Option<SequenceNumber> = None;
            for (table, state) in tables.iter() {
                let Some(candidate) = state.retained.front() else {
                    continue;
                };
                let candidate_sequence = candidate.covered_sequence;
                if oldest_sequence
                    .map(|sequence| candidate_sequence.0 < sequence.0)
                    .unwrap_or(true)
                {
                    oldest_sequence = Some(candidate_sequence);
                    oldest_table = Some(table.clone());
                }
            }
            let Some(oldest_table) = oldest_table else {
                break;
            };
            if let Some(state) = tables.get_mut(&oldest_table) {
                state.retained.pop_front();
            }
        }
    }

    fn evict_if_needed_locked(
        &self,
        tables: &mut HashMap<TableName, RetainedMaterializedTable>,
        access: &mut MaterializedReadAccessState,
    ) {
        self.prune_retained_versions_locked(tables);
        let (table_capacity, byte_capacity) = self.current_limits();
        loop {
            let resident_bytes = Self::total_version_bytes(tables);
            let over_tables = tables.len() > table_capacity;
            let over_bytes = resident_bytes > byte_capacity && tables.len() > 1;
            if !over_tables && !over_bytes {
                break;
            }

            let Some((table, stamp)) = access.access_order.pop_front() else {
                break;
            };
            let should_evict = tables
                .get(&table)
                .is_some_and(|state| state.access_stamp == stamp);
            if !should_evict {
                continue;
            }
            tables.remove(&table);
            self.eviction_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn current_serving_snapshot_from_locked_tables(
        tables: &HashMap<TableName, RetainedMaterializedTable>,
    ) -> Option<ServingSnapshot> {
        let covered_sequence = tables
            .values()
            .map(|state| state.current.covered_sequence)
            .min_by_key(|sequence| sequence.0)?;
        let mut snapshot_tables = HashMap::new();
        for (table, table_state) in tables {
            snapshot_tables.insert(table.clone(), table_state.current.documents.clone());
        }
        Some(ServingSnapshot::from_tables(
            covered_sequence,
            snapshot_tables,
        ))
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

    fn publish_table_snapshot(
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

    #[cfg(test)]
    fn wait_if_publish_pause_armed(&self) {
        self.pause_before_publish.wait_if_armed();
    }
}

fn estimate_document_bytes(document: &Document) -> usize {
    document
        .to_msgpack()
        .map(|bytes| bytes.len())
        // Sizing is advisory; fall back to a coarse JSON estimate instead of
        // poisoning the materialized-read locks on an unexpected serialization
        // failure.
        .unwrap_or_else(|_| document.to_json().to_string().len())
}

fn apply_write_to_materialized_documents(
    documents: &mut MaterializedTableDocuments,
    document_count: &mut usize,
    estimated_bytes: &mut usize,
    write: &neovex_core::WriteOp,
) {
    match &write.current {
        Some(document) => {
            let next_size = estimate_document_bytes(document);
            match documents.insert(write.doc_id, document.clone()) {
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
