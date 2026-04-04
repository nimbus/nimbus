use std::collections::{BTreeMap, HashMap, VecDeque};
#[cfg(test)]
use std::future::Future;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
#[cfg(test)]
use std::time::Instant;

use neovex_core::{CommitEntry, Document, DocumentId, Error, Result, SequenceNumber, TableName};
use neovex_storage::TenantStore;
use serde::Serialize;
use tokio::sync::Notify;

pub(super) const DEFAULT_MATERIALIZED_SURFACE_TABLE_CAPACITY: usize = 8;
pub(super) const DEFAULT_MATERIALIZED_SURFACE_BYTE_CAPACITY: usize = 16 * 1024 * 1024;
pub(super) const DEFAULT_MATERIALIZED_SURFACE_VERSION_CAPACITY: usize = 4;

type MaterializedTableDocuments = HashMap<DocumentId, Document>;

#[derive(Clone)]
pub(crate) struct ServingSnapshot {
    inner: Arc<ServingSnapshotInner>,
}

struct ServingSnapshotInner {
    covered_sequence: SequenceNumber,
    tables: Arc<HashMap<TableName, Arc<MaterializedTableDocuments>>>,
}

impl ServingSnapshot {
    pub(crate) fn covered_sequence(&self) -> SequenceNumber {
        self.inner.covered_sequence
    }

    pub(crate) fn table_documents(&self, table: &TableName) -> Option<Vec<Document>> {
        self.inner
            .tables
            .get(table)
            .map(|documents| documents.values().cloned().collect())
    }

    pub(crate) fn document(&self, table: &TableName, document_id: DocumentId) -> Option<Document> {
        self.inner
            .tables
            .get(table)
            .and_then(|documents| documents.get(&document_id))
            .cloned()
    }

    fn contains_table(&self, table: &TableName) -> bool {
        self.inner.tables.contains_key(table)
    }

    fn pin_count(&self) -> usize {
        Arc::strong_count(&self.inner)
    }
}

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

#[derive(Default)]
struct ServingSnapshotManagerState {
    versions: VecDeque<ServingSnapshot>,
    waiters: BTreeMap<u64, Vec<Arc<Notify>>>,
}

struct ServingSnapshotManager {
    state: Mutex<ServingSnapshotManagerState>,
    pruned_version_count: AtomicU64,
    discarded_out_of_order_count: AtomicU64,
}

// Lock ordering for multi-lock materialized-read operations is
// `backend.access -> backend.tables -> snapshots.state`. Keep that order when
// touching more than one of these locks in the same path.
struct MaterializedServingBackend {
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

pub(super) struct TenantMaterializedReadSurface {
    backend: MaterializedServingBackend,
    snapshots: ServingSnapshotManager,
    evaluation_count: AtomicU64,
    paginated_count: AtomicU64,
    get_hit_count: AtomicU64,
}

struct MaterializedWarmLoadGuard<'a> {
    backend: &'a MaterializedServingBackend,
}

struct MaterializedWarmLoadOwner<'a> {
    backend: &'a MaterializedServingBackend,
    table: TableName,
}

#[derive(Default)]
struct MaterializedWarmLoadCoordinator {
    tables: Mutex<HashMap<TableName, Arc<MaterializedWarmLoadWaitState>>>,
}

#[derive(Default)]
struct MaterializedWarmLoadWaitState {
    completed: Mutex<bool>,
    condvar: Condvar,
}

enum MaterializedWarmLoadDecision<'a> {
    Load(MaterializedWarmLoadOwner<'a>),
    Wait(Arc<MaterializedWarmLoadWaitState>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MaterializedReadSurfaceStats {
    pub loaded_table_count: usize,
    pub resident_document_count: usize,
    pub resident_estimated_bytes: usize,
    pub retained_version_count: usize,
    pub retained_estimated_bytes: usize,
    pub table_capacity: usize,
    pub byte_capacity: usize,
    pub version_capacity: usize,
    pub table_load_count: u64,
    pub evaluation_count: u64,
    pub paginated_count: u64,
    pub get_hit_count: u64,
    pub bypass_count: u64,
    pub eviction_count: u64,
    pub in_flight_load_count: u64,
    pub earliest_covered_sequence: Option<SequenceNumber>,
    pub latest_covered_sequence: Option<SequenceNumber>,
    pub earliest_retained_sequence: Option<SequenceNumber>,
    pub latest_retained_sequence: Option<SequenceNumber>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MaterializedServingBackendStats {
    loaded_table_count: usize,
    resident_document_count: usize,
    resident_estimated_bytes: usize,
    retained_version_count: usize,
    retained_estimated_bytes: usize,
    table_capacity: usize,
    byte_capacity: usize,
    version_capacity: usize,
    table_load_count: u64,
    bypass_count: u64,
    eviction_count: u64,
    in_flight_load_count: u64,
    earliest_covered_sequence: Option<SequenceNumber>,
    latest_covered_sequence: Option<SequenceNumber>,
    earliest_retained_sequence: Option<SequenceNumber>,
    latest_retained_sequence: Option<SequenceNumber>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterializedTablePublicationStats {
    pub generation: u64,
    pub covered_sequence: SequenceNumber,
    pub document_count: usize,
    pub estimated_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ServingSnapshotManagerStats {
    pub retained_snapshot_count: usize,
    pub earliest_retained_sequence: Option<SequenceNumber>,
    pub latest_retained_sequence: Option<SequenceNumber>,
    pub pinned_snapshot_count: usize,
    pub waiter_count: usize,
    pub pruned_snapshot_count: u64,
    pub discarded_out_of_order_count: u64,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct MaterializedReadPublishPauseHandle {
    state: Arc<MaterializedReadPublishPauseState>,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct MaterializedReadPublishPauseControl {
    armed: bool,
    entered: bool,
    released: bool,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct MaterializedReadPublishPauseState {
    control: Mutex<MaterializedReadPublishPauseControl>,
    condvar: Condvar,
}

impl Drop for MaterializedWarmLoadGuard<'_> {
    fn drop(&mut self) {
        self.backend
            .in_flight_load_count
            .fetch_sub(1, Ordering::Relaxed);
    }
}

impl Drop for MaterializedWarmLoadOwner<'_> {
    fn drop(&mut self) {
        let wait_state = self
            .backend
            .warm_loads
            .tables
            .lock()
            .expect("materialized warm load coordinator lock should not be poisoned")
            .remove(&self.table);
        if let Some(wait_state) = wait_state {
            *wait_state
                .completed
                .lock()
                .expect("materialized warm load wait state lock should not be poisoned") = true;
            wait_state.condvar.notify_all();
        }
    }
}

impl MaterializedWarmLoadWaitState {
    fn wait_cancellable(&self, check_cancel: &mut dyn FnMut() -> Result<()>) -> Result<()> {
        let mut completed = self
            .completed
            .lock()
            .expect("materialized warm load wait state lock should not be poisoned");
        while !*completed {
            check_cancel()?;
            let (next_completed, _) = self
                .condvar
                .wait_timeout(completed, std::time::Duration::from_millis(10))
                .expect("materialized warm load wait state lock should not be poisoned");
            completed = next_completed;
        }
        check_cancel()?;
        Ok(())
    }
}

impl ServingSnapshotManager {
    fn new() -> Self {
        Self {
            state: Mutex::new(ServingSnapshotManagerState::default()),
            pruned_version_count: AtomicU64::new(0),
            discarded_out_of_order_count: AtomicU64::new(0),
        }
    }

    fn publish(&self, snapshot: ServingSnapshot, version_capacity: usize) {
        let mut state = self
            .state
            .lock()
            .expect("serving snapshot manager lock should not be poisoned");
        let sequence = snapshot.covered_sequence();
        match state.versions.back() {
            Some(latest) if latest.covered_sequence().0 > sequence.0 => {
                self.discarded_out_of_order_count
                    .fetch_add(1, Ordering::Relaxed);
                return;
            }
            Some(latest) if latest.covered_sequence().0 == sequence.0 => {
                state.versions.pop_back();
                state.versions.push_back(snapshot);
            }
            _ => state.versions.push_back(snapshot),
        }
        self.prune_locked(&mut state, version_capacity.max(1));
        let ready_waiters = self.take_ready_waiters_locked(&mut state, sequence);
        drop(state);
        for waiter in ready_waiters {
            waiter.notify_waiters();
        }
    }

    #[cfg(test)]
    fn snapshot_covering(&self, required_sequence: SequenceNumber) -> Option<ServingSnapshot> {
        self.state
            .lock()
            .expect("serving snapshot manager lock should not be poisoned")
            .versions
            .iter()
            .find(|snapshot| snapshot.covered_sequence().0 >= required_sequence.0)
            .cloned()
    }

    fn snapshot_covering_table(
        &self,
        table: &TableName,
        required_sequence: SequenceNumber,
    ) -> Option<ServingSnapshot> {
        self.state
            .lock()
            .expect("serving snapshot manager lock should not be poisoned")
            .versions
            .iter()
            .find(|snapshot| {
                snapshot.covered_sequence().0 >= required_sequence.0
                    && snapshot.contains_table(table)
            })
            .cloned()
    }

    #[cfg(test)]
    pub(super) async fn wait_for_snapshot_covering_cancellable<Fut>(
        &self,
        required_sequence: SequenceNumber,
        cancel_wait: Fut,
    ) -> Result<ServingSnapshot>
    where
        Fut: Future<Output = ()>,
    {
        tokio::pin!(cancel_wait);
        loop {
            let notify = {
                let mut state = self
                    .state
                    .lock()
                    .expect("serving snapshot manager lock should not be poisoned");
                if let Some(snapshot) = state
                    .versions
                    .iter()
                    .find(|snapshot| snapshot.covered_sequence().0 >= required_sequence.0)
                    .cloned()
                {
                    return Ok(snapshot);
                }
                let notify = Arc::new(Notify::new());
                state
                    .waiters
                    .entry(required_sequence.0)
                    .or_default()
                    .push(notify.clone());
                notify
            };

            tokio::select! {
                _ = notify.notified() => {}
                _ = &mut cancel_wait => return Err(Error::Cancelled),
            }
        }
    }

    fn clear(&self) {
        let mut state = self
            .state
            .lock()
            .expect("serving snapshot manager lock should not be poisoned");
        state.versions.clear();
        let waiters = std::mem::take(&mut state.waiters);
        drop(state);
        for waiter_group in waiters.into_values() {
            for waiter in waiter_group {
                waiter.notify_waiters();
            }
        }
    }

    fn take_ready_waiters_locked(
        &self,
        state: &mut ServingSnapshotManagerState,
        covered_sequence: SequenceNumber,
    ) -> Vec<Arc<Notify>> {
        let ready_keys = state
            .waiters
            .keys()
            .copied()
            .take_while(|required| *required <= covered_sequence.0)
            .collect::<Vec<_>>();
        let mut ready_waiters = Vec::new();
        for key in ready_keys {
            if let Some(waiters) = state.waiters.remove(&key) {
                ready_waiters.extend(waiters);
            }
        }
        ready_waiters
    }

    fn prune_locked(&self, state: &mut ServingSnapshotManagerState, version_capacity: usize) {
        while state.versions.len() > version_capacity {
            let Some(front) = state.versions.front() else {
                break;
            };
            if front.pin_count() > 1 {
                break;
            }
            state.versions.pop_front();
            self.pruned_version_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn stats(&self) -> ServingSnapshotManagerStats {
        let state = self
            .state
            .lock()
            .expect("serving snapshot manager lock should not be poisoned");
        ServingSnapshotManagerStats {
            retained_snapshot_count: state.versions.len(),
            earliest_retained_sequence: state
                .versions
                .front()
                .map(ServingSnapshot::covered_sequence),
            latest_retained_sequence: state.versions.back().map(ServingSnapshot::covered_sequence),
            pinned_snapshot_count: state
                .versions
                .iter()
                .filter(|snapshot| snapshot.pin_count() > 1)
                .count(),
            waiter_count: state.waiters.values().map(Vec::len).sum(),
            pruned_snapshot_count: self.pruned_version_count.load(Ordering::Relaxed),
            discarded_out_of_order_count: self.discarded_out_of_order_count.load(Ordering::Relaxed),
        }
    }
}

impl MaterializedServingBackend {
    fn new() -> Self {
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

    fn start_warm_load(&self) -> MaterializedWarmLoadGuard<'_> {
        self.in_flight_load_count.fetch_add(1, Ordering::Relaxed);
        MaterializedWarmLoadGuard { backend: self }
    }

    fn begin_or_wait_for_warm_load(&self, table: &TableName) -> MaterializedWarmLoadDecision<'_> {
        let mut loading_tables = self
            .warm_loads
            .tables
            .lock()
            .expect("materialized warm load coordinator lock should not be poisoned");
        if let Some(wait_state) = loading_tables.get(table) {
            return MaterializedWarmLoadDecision::Wait(wait_state.clone());
        }
        loading_tables.insert(
            table.clone(),
            Arc::new(MaterializedWarmLoadWaitState::default()),
        );
        MaterializedWarmLoadDecision::Load(MaterializedWarmLoadOwner {
            backend: self,
            table: table.clone(),
        })
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
        for (table, table_state) in tables.iter() {
            snapshot_tables.insert(table.clone(), table_state.current.documents.clone());
        }
        Some(ServingSnapshot {
            inner: Arc::new(ServingSnapshotInner {
                covered_sequence,
                tables: Arc::new(snapshot_tables),
            }),
        })
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

    fn serving_snapshot_for_table_with_mode(
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

    fn load_serving_snapshot_cancellable(
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

            match self.begin_or_wait_for_warm_load(table) {
                MaterializedWarmLoadDecision::Wait(wait_state) => {
                    wait_state.wait_cancellable(check_cancel)?;
                    continue;
                }
                MaterializedWarmLoadDecision::Load(_owner) => {
                    let _warm_load = self.start_warm_load();
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
                            return Err(Error::Internal(format!(
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
                            Error::Internal(format!(
                                "materialized serving snapshot for sequence {} should be available after loading table {}",
                                required_sequence.0, table
                            ))
                        });
                }
            }
        }
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

    fn apply_commit(&self, snapshots: &ServingSnapshotManager, commit: &CommitEntry) {
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

    fn apply_commits<'a>(
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

    fn clear_publications(&self) {
        self.tables
            .write()
            .expect("materialized read surface lock should not be poisoned")
            .clear();
        let wait_states = self
            .warm_loads
            .tables
            .lock()
            .expect("materialized warm load coordinator lock should not be poisoned")
            .drain()
            .map(|(_, wait_state)| wait_state)
            .collect::<Vec<_>>();
        for wait_state in wait_states {
            *wait_state
                .completed
                .lock()
                .expect("materialized warm load wait state lock should not be poisoned") = true;
            wait_state.condvar.notify_all();
        }
        *self
            .access
            .lock()
            .expect("materialized read surface access lock should not be poisoned") =
            MaterializedReadAccessState::default();
    }

    fn stats(&self) -> MaterializedServingBackendStats {
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
    fn table_publication_stats(
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
    fn publish_pause_handle(&self) -> MaterializedReadPublishPauseHandle {
        MaterializedReadPublishPauseHandle {
            state: self.pause_before_publish.clone(),
        }
    }

    #[cfg(test)]
    fn wait_if_publish_pause_armed(&self) {
        self.pause_before_publish.wait_if_armed();
    }

    #[cfg(test)]
    fn set_limits_for_testing(&self, table_capacity: usize, byte_capacity: usize) {
        self.table_capacity
            .store(table_capacity.max(1), Ordering::Relaxed);
        self.byte_capacity
            .store(byte_capacity.max(1), Ordering::Relaxed);
    }

    #[cfg(test)]
    fn set_version_capacity_for_testing(&self, version_capacity: usize) {
        self.version_capacity
            .store(version_capacity.max(1), Ordering::Relaxed);
    }
}

impl TenantMaterializedReadSurface {
    pub(super) fn new() -> Self {
        Self {
            backend: MaterializedServingBackend::new(),
            snapshots: ServingSnapshotManager::new(),
            evaluation_count: AtomicU64::new(0),
            paginated_count: AtomicU64::new(0),
            get_hit_count: AtomicU64::new(0),
        }
    }

    #[cfg(test)]
    pub(super) fn serving_snapshot_covering(
        &self,
        required_sequence: SequenceNumber,
    ) -> Option<ServingSnapshot> {
        self.snapshots.snapshot_covering(required_sequence)
    }

    pub(super) fn serving_snapshot_for_table_with_mode(
        &self,
        table: &TableName,
        required_sequence: SequenceNumber,
        count_bypass: bool,
    ) -> Option<ServingSnapshot> {
        self.backend.serving_snapshot_for_table_with_mode(
            &self.snapshots,
            table,
            required_sequence,
            count_bypass,
        )
    }

    pub(super) fn load_serving_snapshot_cancellable(
        &self,
        store: &TenantStore,
        table: &TableName,
        required_sequence: SequenceNumber,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<ServingSnapshot> {
        self.backend.load_serving_snapshot_cancellable(
            &self.snapshots,
            store,
            table,
            required_sequence,
            check_cancel,
        )
    }

    #[cfg(test)]
    pub(super) async fn wait_for_snapshot_covering_cancellable<Fut>(
        &self,
        required_sequence: SequenceNumber,
        cancel_wait: Fut,
    ) -> Result<ServingSnapshot>
    where
        Fut: Future<Output = ()>,
    {
        self.snapshots
            .wait_for_snapshot_covering_cancellable(required_sequence, cancel_wait)
            .await
    }

    pub(super) fn apply_commit(&self, commit: &CommitEntry) {
        self.backend.apply_commit(&self.snapshots, commit);
    }

    pub(super) fn apply_commits<'a>(&self, commits: impl IntoIterator<Item = &'a CommitEntry>) {
        self.backend.apply_commits(&self.snapshots, commits);
    }

    pub(super) fn record_evaluation(&self) {
        self.evaluation_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_paginated(&self) {
        self.paginated_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_get_hit(&self) {
        self.get_hit_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn clear(&self) {
        self.backend.clear_publications();
        self.snapshots.clear();
    }

    pub(super) fn stats(&self) -> MaterializedReadSurfaceStats {
        let backend = self.backend.stats();
        MaterializedReadSurfaceStats {
            loaded_table_count: backend.loaded_table_count,
            resident_document_count: backend.resident_document_count,
            resident_estimated_bytes: backend.resident_estimated_bytes,
            retained_version_count: backend.retained_version_count,
            retained_estimated_bytes: backend.retained_estimated_bytes,
            table_capacity: backend.table_capacity,
            byte_capacity: backend.byte_capacity,
            version_capacity: backend.version_capacity,
            table_load_count: backend.table_load_count,
            evaluation_count: self.evaluation_count.load(Ordering::Relaxed),
            paginated_count: self.paginated_count.load(Ordering::Relaxed),
            get_hit_count: self.get_hit_count.load(Ordering::Relaxed),
            bypass_count: backend.bypass_count,
            eviction_count: backend.eviction_count,
            in_flight_load_count: backend.in_flight_load_count,
            earliest_covered_sequence: backend.earliest_covered_sequence,
            latest_covered_sequence: backend.latest_covered_sequence,
            earliest_retained_sequence: backend.earliest_retained_sequence,
            latest_retained_sequence: backend.latest_retained_sequence,
        }
    }

    pub(super) fn serving_snapshot_manager_stats(&self) -> ServingSnapshotManagerStats {
        self.snapshots.stats()
    }

    #[cfg(test)]
    pub(super) fn table_publication_stats(
        &self,
        table: &TableName,
    ) -> Option<MaterializedTablePublicationStats> {
        self.backend.table_publication_stats(table)
    }

    #[cfg(test)]
    pub(super) fn publish_pause_handle(&self) -> MaterializedReadPublishPauseHandle {
        self.backend.publish_pause_handle()
    }

    #[cfg(test)]
    pub(super) fn set_limits_for_testing(&self, table_capacity: usize, byte_capacity: usize) {
        self.backend
            .set_limits_for_testing(table_capacity, byte_capacity);
    }

    #[cfg(test)]
    pub(super) fn set_version_capacity_for_testing(&self, version_capacity: usize) {
        self.backend
            .set_version_capacity_for_testing(version_capacity);
    }
}

#[cfg(test)]
impl MaterializedReadPublishPauseHandle {
    pub(crate) fn arm(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("materialized publish pause lock should not be poisoned");
        *control = MaterializedReadPublishPauseControl {
            armed: true,
            entered: false,
            released: false,
        };
    }

    pub(crate) fn wait_until_entered(&self, timeout: std::time::Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut control = self
            .state
            .control
            .lock()
            .expect("materialized publish pause lock should not be poisoned");
        while !control.entered {
            let now = Instant::now();
            let Some(remaining) = deadline.checked_duration_since(now) else {
                return false;
            };
            let (next, result) = self
                .state
                .condvar
                .wait_timeout(control, remaining)
                .expect("materialized publish pause wait should not be poisoned");
            control = next;
            if result.timed_out() && !control.entered {
                return false;
            }
        }
        true
    }

    pub(crate) fn release(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("materialized publish pause lock should not be poisoned");
        control.released = true;
        self.state.condvar.notify_all();
    }
}

#[cfg(test)]
impl MaterializedReadPublishPauseState {
    fn wait_if_armed(&self) {
        let mut control = self
            .control
            .lock()
            .expect("materialized publish pause lock should not be poisoned");
        if !control.armed {
            return;
        }
        control.armed = false;
        control.entered = true;
        self.condvar.notify_all();
        while !control.released {
            control = self
                .condvar
                .wait(control)
                .expect("materialized publish pause wait should not be poisoned");
        }
        *control = MaterializedReadPublishPauseControl::default();
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
