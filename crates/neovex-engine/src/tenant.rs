use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use neovex_core::{
    CommitEntry, Document, DocumentId, Error, Mutation, PrincipalContext, Result, Schema,
    SequenceNumber, TableName, TenantId,
};
use neovex_storage::{JournalProgress, RedbTenantStorage, TenantStore};
use serde::Serialize;
use tokio::sync::{Notify, oneshot};

use crate::subscriptions::{
    QueuedSubscriptionWork, SubscriptionDispatchStats, SubscriptionRegistry,
    dispatch_subscription_work, merge_queued_subscription_work,
};

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DocumentCacheStats {
    pub hits: usize,
    pub misses: usize,
    pub entries: usize,
    pub evictions: usize,
}

pub(crate) const DOCUMENT_CACHE_CAPACITY: usize = 256;
pub(crate) const DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY: usize = 256;
pub(crate) const DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY: usize = 256;
pub(crate) const DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY: usize = 256;
const SUBSCRIPTION_DELIVERY_DRAIN_BATCH_SIZE: usize = 8;
pub(crate) const DEFAULT_MATERIALIZED_SURFACE_TABLE_CAPACITY: usize = 8;
pub(crate) const DEFAULT_MATERIALIZED_SURFACE_BYTE_CAPACITY: usize = 16 * 1024 * 1024;
pub(crate) const DEFAULT_MATERIALIZED_SURFACE_VERSION_CAPACITY: usize = 4;

type DocumentCacheKey = (TableName, DocumentId);

struct CachedDocumentEntry {
    document: Document,
    access_stamp: u64,
}

#[derive(Default)]
struct TenantDocumentCacheState {
    documents: HashMap<DocumentCacheKey, CachedDocumentEntry>,
    access_order: VecDeque<(DocumentCacheKey, u64)>,
    next_access_stamp: u64,
}

struct TenantDocumentCache {
    state: Mutex<TenantDocumentCacheState>,
    hits: AtomicUsize,
    misses: AtomicUsize,
    evictions: AtomicUsize,
}

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
// `access -> tables -> snapshots.state`. Keep that order when touching more
// than one of these locks in the same path.
struct TenantMaterializedReadSurface {
    tables: RwLock<HashMap<TableName, RetainedMaterializedTable>>,
    access: Mutex<MaterializedReadAccessState>,
    snapshots: ServingSnapshotManager,
    warm_loads: MaterializedWarmLoadCoordinator,
    next_generation: AtomicU64,
    table_capacity: AtomicUsize,
    byte_capacity: AtomicUsize,
    version_capacity: AtomicUsize,
    table_load_count: AtomicU64,
    evaluation_count: AtomicU64,
    paginated_count: AtomicU64,
    get_hit_count: AtomicU64,
    bypass_count: AtomicU64,
    eviction_count: AtomicU64,
    in_flight_load_count: AtomicU64,
    #[cfg(test)]
    pause_before_publish: Arc<MaterializedReadPublishPauseState>,
}

struct MaterializedWarmLoadGuard<'a> {
    surface: &'a TenantMaterializedReadSurface,
}

struct MaterializedWarmLoadOwner<'a> {
    surface: &'a TenantMaterializedReadSurface,
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

impl Drop for MaterializedWarmLoadGuard<'_> {
    fn drop(&mut self) {
        self.surface
            .in_flight_load_count
            .fetch_sub(1, Ordering::Relaxed);
    }
}

impl Drop for MaterializedWarmLoadOwner<'_> {
    fn drop(&mut self) {
        let wait_state = self
            .surface
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
    async fn wait_for_snapshot_covering_cancellable<Fut>(
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

impl TenantDocumentCacheState {
    fn next_access_stamp(&mut self) -> u64 {
        self.next_access_stamp = self.next_access_stamp.wrapping_add(1);
        if self.next_access_stamp == 0 {
            self.next_access_stamp = 1;
        }
        self.next_access_stamp
    }

    fn touch(&mut self, key: &DocumentCacheKey) {
        let stamp = self.next_access_stamp();
        if let Some(entry) = self.documents.get_mut(key) {
            entry.access_stamp = stamp;
            self.access_order.push_back((key.clone(), stamp));
        }
    }

    fn insert(&mut self, document: Document) {
        let key = (document.table.clone(), document.id);
        let stamp = self.next_access_stamp();
        self.documents.insert(
            key.clone(),
            CachedDocumentEntry {
                document,
                access_stamp: stamp,
            },
        );
        self.access_order.push_back((key, stamp));
    }

    fn evict_if_needed(&mut self, capacity: usize) -> usize {
        let mut evicted = 0;
        while self.documents.len() > capacity {
            let Some((key, stamp)) = self.access_order.pop_front() else {
                break;
            };
            let should_evict = self
                .documents
                .get(&key)
                .is_some_and(|entry| entry.access_stamp == stamp);
            if should_evict {
                self.documents.remove(&key);
                evicted += 1;
            }
        }
        evicted
    }
}

impl TenantDocumentCache {
    fn new() -> Self {
        Self {
            state: Mutex::new(TenantDocumentCacheState::default()),
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
            evictions: AtomicUsize::new(0),
        }
    }

    fn get(&self, table: &TableName, document_id: DocumentId) -> Option<Document> {
        let key = (table.clone(), document_id);
        let mut state = self
            .state
            .lock()
            .expect("document cache lock should not be poisoned");
        let document = state
            .documents
            .get(&key)
            .map(|entry| entry.document.clone());
        match document {
            Some(document) => {
                state.touch(&key);
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(document)
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    fn insert(&self, document: &Document) {
        let mut state = self
            .state
            .lock()
            .expect("document cache lock should not be poisoned");
        state.insert(document.clone());
        let evictions = state.evict_if_needed(DOCUMENT_CACHE_CAPACITY);
        if evictions != 0 {
            self.evictions.fetch_add(evictions, Ordering::Relaxed);
        }
    }

    fn insert_documents<'a>(&self, documents: impl IntoIterator<Item = &'a Document>) {
        let mut state = self
            .state
            .lock()
            .expect("document cache lock should not be poisoned");
        for document in documents {
            state.insert(document.clone());
        }
        let evictions = state.evict_if_needed(DOCUMENT_CACHE_CAPACITY);
        if evictions != 0 {
            self.evictions.fetch_add(evictions, Ordering::Relaxed);
        }
    }

    fn invalidate_commit(&self, commit: &CommitEntry) {
        let mut state = self
            .state
            .lock()
            .expect("document cache lock should not be poisoned");
        for write in &commit.writes {
            state.documents.remove(&(write.table.clone(), write.doc_id));
        }
    }

    fn invalidate_commits<'a>(&self, commits: impl IntoIterator<Item = &'a CommitEntry>) {
        let mut state = self
            .state
            .lock()
            .expect("document cache lock should not be poisoned");
        for commit in commits {
            for write in &commit.writes {
                state.documents.remove(&(write.table.clone(), write.doc_id));
            }
        }
    }

    fn clear(&self) {
        *self
            .state
            .lock()
            .expect("document cache lock should not be poisoned") =
            TenantDocumentCacheState::default();
    }

    #[cfg(test)]
    fn stats(&self) -> DocumentCacheStats {
        let entries = self
            .state
            .lock()
            .expect("document cache lock should not be poisoned")
            .documents
            .len();
        DocumentCacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            entries,
            evictions: self.evictions.load(Ordering::Relaxed),
        }
    }
}

impl TenantMaterializedReadSurface {
    fn new() -> Self {
        Self {
            tables: RwLock::new(HashMap::new()),
            access: Mutex::new(MaterializedReadAccessState::default()),
            snapshots: ServingSnapshotManager::new(),
            warm_loads: MaterializedWarmLoadCoordinator::default(),
            next_generation: AtomicU64::new(0),
            table_capacity: AtomicUsize::new(DEFAULT_MATERIALIZED_SURFACE_TABLE_CAPACITY),
            byte_capacity: AtomicUsize::new(DEFAULT_MATERIALIZED_SURFACE_BYTE_CAPACITY),
            version_capacity: AtomicUsize::new(DEFAULT_MATERIALIZED_SURFACE_VERSION_CAPACITY),
            table_load_count: AtomicU64::new(0),
            evaluation_count: AtomicU64::new(0),
            paginated_count: AtomicU64::new(0),
            get_hit_count: AtomicU64::new(0),
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
        MaterializedWarmLoadGuard { surface: self }
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
            surface: self,
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
    ) {
        let Some(snapshot) = Self::current_serving_snapshot_from_locked_tables(tables) else {
            self.snapshots.clear();
            return;
        };
        self.snapshots
            .publish(snapshot, self.current_version_capacity());
    }

    #[cfg(test)]
    fn serving_snapshot_covering(
        &self,
        required_sequence: SequenceNumber,
    ) -> Option<ServingSnapshot> {
        self.snapshots.snapshot_covering(required_sequence)
    }

    fn serving_snapshot_for_table_with_mode(
        &self,
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
        self.snapshots
            .snapshot_covering_table(table, required_sequence)
    }

    fn publish_table_snapshot(
        &self,
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
        self.publish_serving_snapshot_locked(&tables);
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

    fn apply_commit(&self, commit: &CommitEntry) {
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
        self.publish_serving_snapshot_locked(&tables);
    }

    fn apply_commits<'a>(&self, commits: impl IntoIterator<Item = &'a CommitEntry>) {
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
            self.publish_serving_snapshot_locked(&tables);
        }
    }

    fn record_evaluation(&self) {
        self.evaluation_count.fetch_add(1, Ordering::Relaxed);
    }

    fn record_paginated(&self) {
        self.paginated_count.fetch_add(1, Ordering::Relaxed);
    }

    fn record_get_hit(&self) {
        self.get_hit_count.fetch_add(1, Ordering::Relaxed);
    }

    fn clear(&self) {
        self.tables
            .write()
            .expect("materialized read surface lock should not be poisoned")
            .clear();
        self.snapshots.clear();
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

    fn stats(&self) -> MaterializedReadSurfaceStats {
        let tables = self
            .tables
            .read()
            .expect("materialized read surface lock should not be poisoned");
        MaterializedReadSurfaceStats {
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
            evaluation_count: self.evaluation_count.load(Ordering::Relaxed),
            paginated_count: self.paginated_count.load(Ordering::Relaxed),
            get_hit_count: self.get_hit_count.load(Ordering::Relaxed),
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

/// Runtime state for a loaded tenant.
pub struct TenantRuntime {
    pub store: Arc<TenantStore>,
    pub read_storage: Arc<RedbTenantStorage>,
    pub subscriptions: SubscriptionRegistry,
    pub schema: RwLock<Arc<Schema>>,
    document_cache: TenantDocumentCache,
    materialized_reads: TenantMaterializedReadSurface,
    query_planning: QueryPlanningMetrics,
    subscription_delivery: SubscriptionDeliveryQueue,
    lifecycle: Arc<TenantLifecycle>,
    mutation_admission: Arc<MutationAdmissionGate>,
    mutation_journal: Arc<MutationJournalState>,
    #[cfg(any(test, feature = "test-hooks"))]
    subscription_bootstrap_pause: Arc<MutationJournalPauseState>,
}

pub struct TenantOperationGuard {
    lifecycle: Arc<TenantLifecycle>,
}

pub struct TenantDeletionGuard;

pub(crate) enum QueuedMutationResult {
    Immediate(Option<DocumentId>),
    Scheduled(bool),
}

pub(crate) struct QueuedMutationRequest {
    pub mutation: Mutation,
    pub principal: PrincipalContext,
    pub scheduled_execution_id: Option<String>,
    pub cancelled: Arc<AtomicBool>,
    pub _operation: TenantOperationGuard,
    pub response: oneshot::Sender<Result<QueuedMutationResult>>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub enqueued_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MutationAdmissionPhase {
    Idle,
    Dropping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MutationAdmissionStats {
    pub queue_depth: usize,
    pub queue_capacity: usize,
    pub oldest_queue_age_nanos: u64,
    pub admitted_count: u64,
    pub shed_count: u64,
    pub queue_rejection_count: u64,
    pub codel_phase: MutationAdmissionPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MutationJournalStats {
    pub durable_head: SequenceNumber,
    pub applied_head: SequenceNumber,
    pub apply_lag: u64,
    pub queue_depth: usize,
    pub queue_capacity: usize,
    pub oldest_queue_age_nanos: u64,
    pub pending_response_count: u64,
    pub worker_running: bool,
    pub worker_start_count: u64,
    pub worker_restart_count: u64,
    pub queue_rejection_count: u64,
    pub worker_failure_count: u64,
    pub read_wait_count: u64,
    pub total_read_wait_nanos: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SubscriptionDeliveryStats {
    pub queue_depth: usize,
    pub queue_capacity: usize,
    pub oldest_queue_age_nanos: u64,
    pub worker_running: bool,
    pub worker_start_count: u64,
    pub worker_restart_count: u64,
    pub overflow_sync_fallback_count: u64,
    pub coalesced_batch_count: u64,
    pub coalesced_commit_count: u64,
    pub merged_subscription_wakeup_count: u64,
    pub queue_level_merge_count: u64,
    pub coalesced_work_count: u64,
    pub reevaluation_count: u64,
    pub total_reevaluation_nanos: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct QueryPlanningStats {
    pub query_full_scan_count: u64,
    pub query_single_field_index_count: u64,
    pub query_composite_index_count: u64,
    pub paginated_full_scan_count: u64,
    pub paginated_single_field_index_count: u64,
    pub paginated_composite_index_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TenantEngineDiagnosticsSnapshot {
    pub mutation_admission: MutationAdmissionStats,
    pub mutation_journal: MutationJournalStats,
    pub subscription_delivery: SubscriptionDeliveryStats,
    pub materialized_read_surface: MaterializedReadSurfaceStats,
    pub serving_snapshot_manager: ServingSnapshotManagerStats,
    pub query_planning: QueryPlanningStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryPlanMetricKind {
    FullScan,
    SingleFieldIndex,
    CompositeIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryPlanMetricOperation {
    Query,
    Paginated,
}

struct QueryPlanningMetrics {
    query_full_scan_count: AtomicU64,
    query_single_field_index_count: AtomicU64,
    query_composite_index_count: AtomicU64,
    paginated_full_scan_count: AtomicU64,
    paginated_single_field_index_count: AtomicU64,
    paginated_composite_index_count: AtomicU64,
}

struct MutationAdmissionGate {
    state: Mutex<MutationAdmissionGateState>,
    capacity: AtomicUsize,
    admitted_count: AtomicU64,
    shed_count: AtomicU64,
    queue_rejection_count: AtomicU64,
}

struct MutationAdmissionGateState {
    queue: VecDeque<QueuedMutationRequest>,
    codel: CoDelState,
}

struct CoDelState {
    target: Duration,
    interval: Duration,
    phase: CoDelPhase,
    first_above_time: Option<Instant>,
}

enum CoDelPhase {
    Idle,
    Dropping { drop_next: Instant, drop_count: u32 },
}

enum MutationAdmissionDecision {
    Admit(QueuedMutationRequest),
    Reject {
        request: QueuedMutationRequest,
        error: Error,
    },
    Empty,
}

impl QueryPlanningMetrics {
    fn new() -> Self {
        Self {
            query_full_scan_count: AtomicU64::new(0),
            query_single_field_index_count: AtomicU64::new(0),
            query_composite_index_count: AtomicU64::new(0),
            paginated_full_scan_count: AtomicU64::new(0),
            paginated_single_field_index_count: AtomicU64::new(0),
            paginated_composite_index_count: AtomicU64::new(0),
        }
    }

    fn record(&self, operation: QueryPlanMetricOperation, kind: QueryPlanMetricKind) {
        let counter = match (operation, kind) {
            (QueryPlanMetricOperation::Query, QueryPlanMetricKind::FullScan) => {
                &self.query_full_scan_count
            }
            (QueryPlanMetricOperation::Query, QueryPlanMetricKind::SingleFieldIndex) => {
                &self.query_single_field_index_count
            }
            (QueryPlanMetricOperation::Query, QueryPlanMetricKind::CompositeIndex) => {
                &self.query_composite_index_count
            }
            (QueryPlanMetricOperation::Paginated, QueryPlanMetricKind::FullScan) => {
                &self.paginated_full_scan_count
            }
            (QueryPlanMetricOperation::Paginated, QueryPlanMetricKind::SingleFieldIndex) => {
                &self.paginated_single_field_index_count
            }
            (QueryPlanMetricOperation::Paginated, QueryPlanMetricKind::CompositeIndex) => {
                &self.paginated_composite_index_count
            }
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn stats(&self) -> QueryPlanningStats {
        QueryPlanningStats {
            query_full_scan_count: self.query_full_scan_count.load(Ordering::Relaxed),
            query_single_field_index_count: self
                .query_single_field_index_count
                .load(Ordering::Relaxed),
            query_composite_index_count: self.query_composite_index_count.load(Ordering::Relaxed),
            paginated_full_scan_count: self.paginated_full_scan_count.load(Ordering::Relaxed),
            paginated_single_field_index_count: self
                .paginated_single_field_index_count
                .load(Ordering::Relaxed),
            paginated_composite_index_count: self
                .paginated_composite_index_count
                .load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct SubscriptionDeliveryPauseHandle {
    state: Arc<SubscriptionDeliveryPauseState>,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct SubscriptionDeliveryPauseControl {
    armed: bool,
    entered: bool,
    released: bool,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct SubscriptionDeliveryPauseState {
    control: Mutex<SubscriptionDeliveryPauseControl>,
    condvar: Condvar,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone)]
pub(crate) struct MutationJournalPauseHandle {
    state: Arc<MutationJournalPauseState>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct MaterializedReadPublishPauseHandle {
    state: Arc<MaterializedReadPublishPauseState>,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Default)]
struct MutationJournalPauseControl {
    armed: bool,
    entered: bool,
    released: bool,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Default)]
struct MutationJournalPauseState {
    control: Mutex<MutationJournalPauseControl>,
    entered: Condvar,
    released: Notify,
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

#[cfg(test)]
impl SubscriptionDeliveryPauseHandle {
    pub(crate) fn arm(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("subscription delivery pause lock should not be poisoned");
        *control = SubscriptionDeliveryPauseControl {
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
            .expect("subscription delivery pause lock should not be poisoned");
        while !control.entered {
            let now = Instant::now();
            let Some(remaining) = deadline.checked_duration_since(now) else {
                return false;
            };
            let (next, result) = self
                .state
                .condvar
                .wait_timeout(control, remaining)
                .expect("subscription delivery pause wait should not be poisoned");
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
            .expect("subscription delivery pause lock should not be poisoned");
        control.released = true;
        self.state.condvar.notify_all();
    }
}

#[cfg(any(test, feature = "test-hooks"))]
impl MutationJournalPauseState {
    async fn wait_if_armed(&self) {
        {
            let mut control = self
                .control
                .lock()
                .expect("mutation journal pause lock should not be poisoned");
            if !control.armed {
                return;
            }
            control.entered = true;
            self.entered.notify_all();
            if control.released {
                *control = MutationJournalPauseControl::default();
                return;
            }
        }

        loop {
            let notified = self.released.notified();
            {
                let mut control = self
                    .control
                    .lock()
                    .expect("mutation journal pause lock should not be poisoned");
                if control.released {
                    *control = MutationJournalPauseControl::default();
                    return;
                }
            }
            notified.await;
        }
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

#[cfg(any(test, feature = "test-hooks"))]
impl MutationJournalPauseHandle {
    pub(crate) fn arm(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("mutation journal pause lock should not be poisoned");
        *control = MutationJournalPauseControl {
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
            .expect("mutation journal pause lock should not be poisoned");
        while !control.entered {
            let now = Instant::now();
            let Some(remaining) = deadline.checked_duration_since(now) else {
                return false;
            };
            let (next, result) = self
                .state
                .entered
                .wait_timeout(control, remaining)
                .expect("mutation journal pause wait should not be poisoned");
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
            .expect("mutation journal pause lock should not be poisoned");
        control.released = true;
        self.state.released.notify_waiters();
    }
}

struct MutationJournalState {
    queue: Mutex<VecDeque<QueuedMutationRequest>>,
    capacity: AtomicUsize,
    worker_running: AtomicBool,
    worker_start_count: AtomicU64,
    queue_rejection_count: AtomicU64,
    worker_failure_count: AtomicU64,
    pending_response_count: AtomicU64,
    sequence_gate: Mutex<()>,
    applied_wait_lock: Mutex<()>,
    applied_wait: Condvar,
    durable_head: AtomicU64,
    applied_head: AtomicU64,
    read_wait_count: AtomicU64,
    total_read_wait_nanos: AtomicU64,
    applied_notify: Notify,
    #[cfg(test)]
    pause_before_drain: Arc<MutationJournalPauseState>,
}

struct SubscriptionDeliveryState {
    queue: Mutex<VecDeque<QueuedSubscriptionWork>>,
    queue_ready: Condvar,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    shutdown: AtomicBool,
    capacity: AtomicUsize,
    worker_start_count: AtomicU64,
    overflow_sync_fallback_count: AtomicU64,
    coalesced_batch_count: AtomicU64,
    coalesced_commit_count: AtomicU64,
    merged_subscription_wakeup_count: AtomicU64,
    queue_level_merge_count: AtomicU64,
    coalesced_work_count: AtomicU64,
    reevaluation_count: AtomicU64,
    total_reevaluation_nanos: AtomicU64,
    #[cfg(test)]
    pause: Arc<SubscriptionDeliveryPauseState>,
}

struct SubscriptionDeliveryQueue {
    state: Arc<SubscriptionDeliveryState>,
}

// Tenant lifecycle is a close-then-drain protocol:
// once deletion begins we first mark the tenant deleted so no new operations
// can enter, then we wait for the in-flight operation count to drain to zero.
// Sync callers block on the condvar path while async callers await Notify,
// but both are driven by the same atomic state and RAII operation guards.
struct TenantLifecycle {
    deleted: AtomicBool,
    active_operations: AtomicUsize,
    zero_active_lock: Mutex<()>,
    zero_active: Condvar,
    zero_active_notify: Notify,
}

impl TenantLifecycle {
    fn new() -> Self {
        Self {
            deleted: AtomicBool::new(false),
            active_operations: AtomicUsize::new(0),
            zero_active_lock: Mutex::new(()),
            zero_active: Condvar::new(),
            zero_active_notify: Notify::new(),
        }
    }

    fn enter_operation(&self, tenant_id: &TenantId) -> Result<()> {
        if self.deleted.load(Ordering::Acquire) {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        }
        self.active_operations.fetch_add(1, Ordering::AcqRel);
        if self.deleted.load(Ordering::Acquire) {
            self.release_operation();
            return Err(Error::TenantNotFound(tenant_id.clone()));
        }
        Ok(())
    }

    fn release_operation(&self) {
        if self.active_operations.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.zero_active.notify_all();
            self.zero_active_notify.notify_waiters();
        }
    }

    fn begin_delete_blocking(&self) {
        self.deleted.store(true, Ordering::Release);
        let mut guard = self
            .zero_active_lock
            .lock()
            .expect("tenant lifecycle wait lock should not be poisoned");
        while self.active_operations.load(Ordering::Acquire) != 0 {
            guard = self
                .zero_active
                .wait(guard)
                .expect("tenant lifecycle wait should not be poisoned");
        }
    }

    async fn begin_delete_async(&self) {
        self.deleted.store(true, Ordering::Release);
        loop {
            if self.active_operations.load(Ordering::Acquire) == 0 {
                return;
            }
            let notified = self.zero_active_notify.notified();
            if self.active_operations.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }
}

impl Drop for TenantOperationGuard {
    fn drop(&mut self) {
        self.lifecycle.release_operation();
    }
}

impl MutationAdmissionGate {
    fn new() -> Self {
        Self {
            state: Mutex::new(MutationAdmissionGateState {
                queue: VecDeque::new(),
                codel: CoDelState::new(Duration::from_millis(5), Duration::from_millis(100)),
            }),
            capacity: AtomicUsize::new(DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY),
            admitted_count: AtomicU64::new(0),
            shed_count: AtomicU64::new(0),
            queue_rejection_count: AtomicU64::new(0),
        }
    }

    fn enqueue(&self, request: QueuedMutationRequest) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .expect("mutation admission gate lock should not be poisoned");
        let capacity = self.capacity.load(Ordering::Acquire).max(1);
        if state.queue.len() >= capacity {
            self.queue_rejection_count.fetch_add(1, Ordering::Relaxed);
            return Err(Error::ResourceExhausted(format!(
                "mutation admission gate full (capacity {capacity})"
            )));
        }
        state.queue.push_back(request);
        self.admitted_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn pop_next_at(&self, now: Instant) -> MutationAdmissionDecision {
        let mut state = self
            .state
            .lock()
            .expect("mutation admission gate lock should not be poisoned");
        let Some(request) = state.queue.pop_front() else {
            state.codel.reset();
            return MutationAdmissionDecision::Empty;
        };

        let should_drop = state.codel.should_drop(now, request.enqueued_at);
        if state.queue.is_empty() {
            state.codel.reset();
        }

        if should_drop {
            self.shed_count.fetch_add(1, Ordering::Relaxed);
            return MutationAdmissionDecision::Reject {
                request,
                error: Error::ResourceExhausted("mutation shed by admission gate".to_string()),
            };
        }

        MutationAdmissionDecision::Admit(request)
    }

    fn has_pending(&self) -> bool {
        !self
            .state
            .lock()
            .expect("mutation admission gate lock should not be poisoned")
            .queue
            .is_empty()
    }

    fn stats(&self) -> MutationAdmissionStats {
        let state = self
            .state
            .lock()
            .expect("mutation admission gate lock should not be poisoned");
        let oldest_queue_age_nanos = state
            .queue
            .front()
            .map(|request| request.enqueued_at.elapsed().as_nanos())
            .unwrap_or(0)
            .min(u128::from(u64::MAX)) as u64;
        MutationAdmissionStats {
            queue_depth: state.queue.len(),
            queue_capacity: self.capacity.load(Ordering::Relaxed),
            oldest_queue_age_nanos,
            admitted_count: self.admitted_count.load(Ordering::Relaxed),
            shed_count: self.shed_count.load(Ordering::Relaxed),
            queue_rejection_count: self.queue_rejection_count.load(Ordering::Relaxed),
            codel_phase: state.codel.phase_stats(),
        }
    }

    #[cfg(test)]
    fn set_codel_for_testing(&self, target: Duration, interval: Duration) {
        let mut state = self
            .state
            .lock()
            .expect("mutation admission gate lock should not be poisoned");
        state.codel = CoDelState::new(target, interval);
    }
}

impl CoDelState {
    fn new(target: Duration, interval: Duration) -> Self {
        Self {
            target,
            interval,
            phase: CoDelPhase::Idle,
            first_above_time: None,
        }
    }

    fn should_drop(&mut self, now: Instant, enqueued_at: Instant) -> bool {
        let sojourn = now.saturating_duration_since(enqueued_at);
        if sojourn < self.target {
            self.reset();
            return false;
        }

        match &mut self.phase {
            CoDelPhase::Idle => match self.first_above_time {
                None => {
                    self.first_above_time = Some(now + self.interval);
                    false
                }
                Some(first_above_time) if now < first_above_time => false,
                Some(_) => {
                    self.phase = CoDelPhase::Dropping {
                        drop_next: now + codel_drop_interval(self.interval, 1),
                        drop_count: 1,
                    };
                    true
                }
            },
            CoDelPhase::Dropping {
                drop_next,
                drop_count,
            } => {
                if sojourn < self.target {
                    self.reset();
                    return false;
                }
                if now < *drop_next {
                    return false;
                }
                *drop_count = drop_count.saturating_add(1);
                *drop_next = now + codel_drop_interval(self.interval, *drop_count);
                true
            }
        }
    }

    fn reset(&mut self) {
        self.phase = CoDelPhase::Idle;
        self.first_above_time = None;
    }

    fn phase_stats(&self) -> MutationAdmissionPhase {
        match self.phase {
            CoDelPhase::Idle => MutationAdmissionPhase::Idle,
            CoDelPhase::Dropping { .. } => MutationAdmissionPhase::Dropping,
        }
    }
}

fn codel_drop_interval(interval: Duration, drop_count: u32) -> Duration {
    let divisor = f64::from(drop_count.max(1)).sqrt();
    Duration::from_secs_f64((interval.as_secs_f64() / divisor).max(0.000_001))
}

type MutationJournalEnqueueError = Box<(QueuedMutationRequest, Error)>;

impl MutationJournalState {
    fn new(progress: JournalProgress) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            capacity: AtomicUsize::new(DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY),
            worker_running: AtomicBool::new(false),
            worker_start_count: AtomicU64::new(0),
            queue_rejection_count: AtomicU64::new(0),
            worker_failure_count: AtomicU64::new(0),
            pending_response_count: AtomicU64::new(0),
            sequence_gate: Mutex::new(()),
            applied_wait_lock: Mutex::new(()),
            applied_wait: Condvar::new(),
            durable_head: AtomicU64::new(progress.durable_head.0),
            applied_head: AtomicU64::new(progress.applied_head.0),
            read_wait_count: AtomicU64::new(0),
            total_read_wait_nanos: AtomicU64::new(0),
            applied_notify: Notify::new(),
            #[cfg(test)]
            pause_before_drain: Arc::new(MutationJournalPauseState::default()),
        }
    }

    fn enqueue(
        &self,
        request: QueuedMutationRequest,
    ) -> std::result::Result<(), MutationJournalEnqueueError> {
        let mut queue = self
            .queue
            .lock()
            .expect("mutation journal queue lock should not be poisoned");
        let capacity = self.capacity.load(Ordering::Acquire).max(1);
        if queue.len() >= capacity {
            self.queue_rejection_count.fetch_add(1, Ordering::Relaxed);
            return Err(Box::new((
                request,
                Error::ResourceExhausted(format!(
                    "mutation journal queue full (capacity {capacity})"
                )),
            )));
        }
        queue.push_back(request);
        Ok(())
    }

    async fn drain_batch(&self, max_batch_size: usize) -> Vec<QueuedMutationRequest> {
        let mut queue = self
            .queue
            .lock()
            .expect("mutation journal queue lock should not be poisoned");
        let batch_size = queue.len().min(max_batch_size.max(1));
        queue.drain(..batch_size).collect()
    }

    #[cfg(test)]
    async fn wait_before_drain(&self) {
        self.pause_before_drain.wait_if_armed().await;
    }

    fn release_worker(&self, gate_has_more: bool) -> bool {
        self.worker_running.store(false, Ordering::Release);
        let queue_has_more = gate_has_more
            || !self
                .queue
                .lock()
                .expect("mutation journal queue lock should not be poisoned")
                .is_empty();
        queue_has_more
            && self
                .worker_running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    fn try_start_worker(&self) -> bool {
        self.worker_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn record_worker_start(&self) {
        self.worker_start_count.fetch_add(1, Ordering::Relaxed);
    }

    fn record_worker_failure(&self) {
        self.worker_failure_count.fetch_add(1, Ordering::Relaxed);
    }

    fn begin_pending_response(&self) {
        self.pending_response_count.fetch_add(1, Ordering::Relaxed);
    }

    fn finish_pending_response(&self) {
        self.pending_response_count.fetch_sub(1, Ordering::Relaxed);
    }

    fn durable_head(&self) -> SequenceNumber {
        SequenceNumber(self.durable_head.load(Ordering::Acquire))
    }

    fn lock_sequence_gate(&self) -> std::sync::MutexGuard<'_, ()> {
        self.sequence_gate
            .lock()
            .expect("mutation journal sequence gate should not be poisoned")
    }

    fn applied_head(&self) -> SequenceNumber {
        SequenceNumber(self.applied_head.load(Ordering::Acquire))
    }

    fn mark_durable_head(&self, sequence: SequenceNumber) {
        self.durable_head.store(sequence.0, Ordering::Release);
    }

    fn mark_applied_head(&self, sequence: SequenceNumber) {
        let _guard = self
            .applied_wait_lock
            .lock()
            .expect("mutation journal applied wait lock should not be poisoned");
        self.applied_head.store(sequence.0, Ordering::Release);
        self.applied_wait.notify_all();
        self.applied_notify.notify_waiters();
    }

    async fn wait_for_applied_sequence_cancellable<Fut>(
        &self,
        required: SequenceNumber,
        cancel_wait: Fut,
    ) -> Result<()>
    where
        Fut: Future<Output = ()>,
    {
        if self.applied_head().0 >= required.0 {
            return Ok(());
        }

        let started = Instant::now();
        tokio::pin!(cancel_wait);
        loop {
            if self.applied_head().0 >= required.0 {
                self.record_read_wait(started);
                return Ok(());
            }
            let notified = self.applied_notify.notified();
            tokio::pin!(notified);
            tokio::select! {
                _ = &mut cancel_wait => {
                    self.record_read_wait(started);
                    return Err(Error::Cancelled);
                }
                _ = &mut notified => {}
            }
        }
    }

    fn wait_for_applied_sequence_blocking(&self, required: SequenceNumber) {
        if self.applied_head().0 >= required.0 {
            return;
        }

        let started = Instant::now();
        let mut guard = self
            .applied_wait_lock
            .lock()
            .expect("mutation journal applied wait lock should not be poisoned");
        while self.applied_head().0 < required.0 {
            guard = self
                .applied_wait
                .wait(guard)
                .expect("mutation journal applied wait should not be poisoned");
        }
        drop(guard);
        self.record_read_wait(started);
    }

    fn record_read_wait(&self, started: Instant) {
        self.read_wait_count.fetch_add(1, Ordering::Relaxed);
        self.total_read_wait_nanos.fetch_add(
            started.elapsed().as_nanos().try_into().unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }

    fn stats(&self) -> MutationJournalStats {
        let durable_head = self.durable_head();
        let applied_head = self.applied_head();
        let queue = self
            .queue
            .lock()
            .expect("mutation journal queue lock should not be poisoned");
        let oldest_queue_age_nanos = queue
            .front()
            .map(|request| request.enqueued_at.elapsed().as_nanos())
            .unwrap_or(0)
            .min(u128::from(u64::MAX)) as u64;
        let worker_start_count = self.worker_start_count.load(Ordering::Relaxed);
        MutationJournalStats {
            durable_head,
            applied_head,
            apply_lag: durable_head.0.saturating_sub(applied_head.0),
            queue_depth: queue.len(),
            queue_capacity: self.capacity.load(Ordering::Relaxed),
            oldest_queue_age_nanos,
            pending_response_count: self.pending_response_count.load(Ordering::Relaxed),
            worker_running: self.worker_running.load(Ordering::Relaxed),
            worker_start_count,
            worker_restart_count: worker_start_count.saturating_sub(1),
            queue_rejection_count: self.queue_rejection_count.load(Ordering::Relaxed),
            worker_failure_count: self.worker_failure_count.load(Ordering::Relaxed),
            read_wait_count: self.read_wait_count.load(Ordering::Relaxed),
            total_read_wait_nanos: self.total_read_wait_nanos.load(Ordering::Relaxed),
        }
    }

    #[cfg(test)]
    fn set_capacity_for_testing(&self, capacity: usize) {
        self.capacity.store(capacity.max(1), Ordering::Release);
    }
}

#[cfg(test)]
impl SubscriptionDeliveryPauseState {
    fn wait_if_armed(&self) {
        let mut control = self
            .control
            .lock()
            .expect("subscription delivery pause lock should not be poisoned");
        if !control.armed {
            return;
        }
        control.entered = true;
        self.condvar.notify_all();
        while !control.released {
            control = self
                .condvar
                .wait(control)
                .expect("subscription delivery pause wait should not be poisoned");
        }
        *control = SubscriptionDeliveryPauseControl::default();
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

impl SubscriptionDeliveryQueue {
    fn new() -> Self {
        Self {
            state: Arc::new(SubscriptionDeliveryState {
                queue: Mutex::new(VecDeque::new()),
                queue_ready: Condvar::new(),
                worker: Mutex::new(None),
                shutdown: AtomicBool::new(false),
                capacity: AtomicUsize::new(DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY),
                worker_start_count: AtomicU64::new(0),
                overflow_sync_fallback_count: AtomicU64::new(0),
                coalesced_batch_count: AtomicU64::new(0),
                coalesced_commit_count: AtomicU64::new(0),
                merged_subscription_wakeup_count: AtomicU64::new(0),
                queue_level_merge_count: AtomicU64::new(0),
                coalesced_work_count: AtomicU64::new(0),
                reevaluation_count: AtomicU64::new(0),
                total_reevaluation_nanos: AtomicU64::new(0),
                #[cfg(test)]
                pause: Arc::new(SubscriptionDeliveryPauseState::default()),
            }),
        }
    }

    fn start_worker(&self, runtime: &Arc<TenantRuntime>) {
        let mut worker = self
            .state
            .worker
            .lock()
            .expect("subscription delivery worker lock should not be poisoned");
        if worker.is_some() {
            return;
        }
        self.state.shutdown.store(false, Ordering::Release);
        self.state
            .worker_start_count
            .fetch_add(1, Ordering::Relaxed);
        let state = self.state.clone();
        // Delivery intentionally uses a tenant-owned dedicated thread instead of
        // the shared Tokio background runtime. The key invariant is ownership:
        // this worker must outlive any request/task that enqueues delivery work,
        // remain explicitly bounded, and shut down via the tenant lifecycle.
        // The worker should not keep a tenant alive during deletion; the
        // explicit shutdown path joins first, and the weak upgrade lets the
        // worker exit cleanly if teardown wins the race.
        let runtime = Arc::downgrade(runtime);
        *worker =
            Some(
                std::thread::Builder::new()
                    .name("neovex-subscription-delivery".to_string())
                    .spawn(move || {
                        loop {
                            let first_work = {
                                let mut queue = state.queue.lock().expect(
                                    "subscription delivery queue lock should not be poisoned",
                                );
                                loop {
                                    if state.shutdown.load(Ordering::Acquire) {
                                        queue.clear();
                                        return;
                                    }
                                    if let Some(work) = queue.pop_front() {
                                        break work;
                                    }
                                    queue = state.queue_ready.wait(queue).expect(
                                        "subscription delivery wait should not be poisoned",
                                    );
                                }
                            };

                            #[cfg(test)]
                            state.pause.wait_if_armed();

                            let mut work_batch = vec![first_work];
                            {
                                let mut queue = state.queue.lock().expect(
                                    "subscription delivery queue lock should not be poisoned",
                                );
                                if state.shutdown.load(Ordering::Acquire) {
                                    queue.clear();
                                    return;
                                }
                                while work_batch.len() < SUBSCRIPTION_DELIVERY_DRAIN_BATCH_SIZE {
                                    let Some(work) = queue.pop_front() else {
                                        break;
                                    };
                                    work_batch.push(work);
                                }
                            }
                            let (work, merged_count) = merge_queued_subscription_work(work_batch);
                            if merged_count != 0 {
                                state
                                    .queue_level_merge_count
                                    .fetch_add(merged_count, Ordering::Relaxed);
                            }

                            let Some(runtime) = runtime.upgrade() else {
                                return;
                            };
                            let stats = dispatch_subscription_work(&runtime, &work);
                            state.record_dispatch_stats(stats);
                        }
                    })
                    .expect("subscription delivery worker should spawn"),
            );
    }

    fn enqueue(
        &self,
        work: QueuedSubscriptionWork,
    ) -> std::result::Result<(), QueuedSubscriptionWork> {
        let mut queue = self
            .state
            .queue
            .lock()
            .expect("subscription delivery queue lock should not be poisoned");
        if queue.len() >= self.state.capacity.load(Ordering::Acquire).max(1) {
            return Err(work);
        }
        queue.push_back(work);
        self.state.queue_ready.notify_one();
        Ok(())
    }

    fn record_overflow_sync_fallback(&self) {
        self.state
            .overflow_sync_fallback_count
            .fetch_add(1, Ordering::Relaxed);
    }

    fn record_coalesced_batch(&self, commit_count: u64, merged_subscription_wakeup_count: u64) {
        self.state
            .coalesced_batch_count
            .fetch_add(1, Ordering::Relaxed);
        self.state
            .coalesced_commit_count
            .fetch_add(commit_count, Ordering::Relaxed);
        self.state
            .merged_subscription_wakeup_count
            .fetch_add(merged_subscription_wakeup_count, Ordering::Relaxed);
    }

    fn record_dispatch_stats(&self, stats: SubscriptionDispatchStats) {
        self.state
            .coalesced_work_count
            .fetch_add(stats.coalesced_work_count, Ordering::Relaxed);
        self.state
            .reevaluation_count
            .fetch_add(stats.reevaluation_count, Ordering::Relaxed);
        self.state
            .total_reevaluation_nanos
            .fetch_add(stats.total_reevaluation_nanos, Ordering::Relaxed);
    }

    fn shutdown(&self) {
        self.state.shutdown.store(true, Ordering::Release);
        self.state.queue_ready.notify_all();
        if let Some(worker) = self
            .state
            .worker
            .lock()
            .expect("subscription delivery worker lock should not be poisoned")
            .take()
        {
            // Cleanup can be triggered from inside delivery paths; skip
            // joining ourselves and let the thread return naturally instead.
            if worker.thread().id() == std::thread::current().id() {
                return;
            }
            let _ = worker.join();
        }
    }

    fn stats(&self) -> SubscriptionDeliveryStats {
        let queue = self
            .state
            .queue
            .lock()
            .expect("subscription delivery queue lock should not be poisoned");
        let oldest_queue_age_nanos = queue
            .front()
            .map(|work| work.enqueued_at.elapsed().as_nanos())
            .unwrap_or(0)
            .min(u128::from(u64::MAX)) as u64;
        let worker_running = self
            .state
            .worker
            .lock()
            .expect("subscription delivery worker lock should not be poisoned")
            .is_some();
        let worker_start_count = self.state.worker_start_count.load(Ordering::Relaxed);
        SubscriptionDeliveryStats {
            queue_depth: queue.len(),
            queue_capacity: self.state.capacity.load(Ordering::Relaxed),
            oldest_queue_age_nanos,
            worker_running,
            worker_start_count,
            worker_restart_count: worker_start_count.saturating_sub(1),
            overflow_sync_fallback_count: self
                .state
                .overflow_sync_fallback_count
                .load(Ordering::Relaxed),
            coalesced_batch_count: self.state.coalesced_batch_count.load(Ordering::Relaxed),
            coalesced_commit_count: self.state.coalesced_commit_count.load(Ordering::Relaxed),
            merged_subscription_wakeup_count: self
                .state
                .merged_subscription_wakeup_count
                .load(Ordering::Relaxed),
            queue_level_merge_count: self.state.queue_level_merge_count.load(Ordering::Relaxed),
            coalesced_work_count: self.state.coalesced_work_count.load(Ordering::Relaxed),
            reevaluation_count: self.state.reevaluation_count.load(Ordering::Relaxed),
            total_reevaluation_nanos: self.state.total_reevaluation_nanos.load(Ordering::Relaxed),
        }
    }

    #[cfg(test)]
    fn set_capacity_for_testing(&self, capacity: usize) {
        self.state
            .capacity
            .store(capacity.max(1), Ordering::Release);
    }

    #[cfg(test)]
    fn pause_handle(&self) -> SubscriptionDeliveryPauseHandle {
        SubscriptionDeliveryPauseHandle {
            state: self.state.pause.clone(),
        }
    }
}

impl SubscriptionDeliveryState {
    fn record_dispatch_stats(&self, stats: SubscriptionDispatchStats) {
        self.coalesced_work_count
            .fetch_add(stats.coalesced_work_count, Ordering::Relaxed);
        self.reevaluation_count
            .fetch_add(stats.reevaluation_count, Ordering::Relaxed);
        self.total_reevaluation_nanos
            .fetch_add(stats.total_reevaluation_nanos, Ordering::Relaxed);
    }
}

impl Drop for SubscriptionDeliveryQueue {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl TenantRuntime {
    /// Creates a tenant runtime from a store.
    pub fn from_parts(
        store: Arc<TenantStore>,
        read_storage: Arc<RedbTenantStorage>,
    ) -> Result<Self> {
        let schema = store.load_schema()?;
        let progress = store.journal_progress()?;
        Ok(Self {
            store,
            read_storage,
            subscriptions: SubscriptionRegistry::new(),
            schema: RwLock::new(Arc::new(schema)),
            document_cache: TenantDocumentCache::new(),
            materialized_reads: TenantMaterializedReadSurface::new(),
            query_planning: QueryPlanningMetrics::new(),
            subscription_delivery: SubscriptionDeliveryQueue::new(),
            lifecycle: Arc::new(TenantLifecycle::new()),
            mutation_admission: Arc::new(MutationAdmissionGate::new()),
            mutation_journal: Arc::new(MutationJournalState::new(progress)),
            #[cfg(any(test, feature = "test-hooks"))]
            subscription_bootstrap_pause: Arc::new(MutationJournalPauseState::default()),
        })
    }

    /// Returns the current schema snapshot.
    pub fn schema(&self) -> Arc<Schema> {
        self.schema
            .read()
            .expect("schema lock should not be poisoned")
            .clone()
    }

    /// Enters a tenant operation, preventing deletion while the operation is active.
    pub fn enter_operation(&self, tenant_id: &TenantId) -> Result<TenantOperationGuard> {
        self.lifecycle.enter_operation(tenant_id)?;
        Ok(TenantOperationGuard {
            lifecycle: self.lifecycle.clone(),
        })
    }

    /// Begins tenant deletion and blocks until all in-flight operations complete.
    pub fn begin_delete(&self) -> TenantDeletionGuard {
        self.lifecycle.begin_delete_blocking();
        TenantDeletionGuard
    }

    /// Begins tenant deletion asynchronously and waits until all in-flight operations complete.
    pub async fn begin_delete_async(&self) -> TenantDeletionGuard {
        self.lifecycle.begin_delete_async().await;
        TenantDeletionGuard
    }

    pub(crate) fn get_cached_document(
        &self,
        table: &TableName,
        document_id: DocumentId,
    ) -> Option<Document> {
        self.document_cache.get(table, document_id)
    }

    pub(crate) fn cache_document(&self, document: &Document) {
        self.document_cache.insert(document);
    }

    pub(crate) fn cache_documents<'a>(&self, documents: impl IntoIterator<Item = &'a Document>) {
        self.document_cache.insert_documents(documents);
    }

    #[cfg(test)]
    pub(crate) fn materialized_serving_snapshot(
        &self,
        required_sequence: SequenceNumber,
    ) -> Option<ServingSnapshot> {
        self.materialized_reads
            .serving_snapshot_covering(required_sequence)
    }

    pub(crate) fn materialized_serving_snapshot_for_table(
        &self,
        table: &TableName,
        required_sequence: SequenceNumber,
    ) -> Option<ServingSnapshot> {
        self.materialized_reads
            .serving_snapshot_for_table_with_mode(table, required_sequence, true)
    }

    pub(crate) fn load_materialized_serving_snapshot_cancellable(
        &self,
        store: &TenantStore,
        table: &TableName,
        required_sequence: SequenceNumber,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<ServingSnapshot> {
        loop {
            if let Some(snapshot) = self
                .materialized_reads
                .serving_snapshot_for_table_with_mode(table, required_sequence, true)
            {
                return Ok(snapshot);
            }

            match self.materialized_reads.begin_or_wait_for_warm_load(table) {
                MaterializedWarmLoadDecision::Wait(wait_state) => {
                    wait_state.wait_cancellable(check_cancel)?;
                    continue;
                }
                MaterializedWarmLoadDecision::Load(_owner) => {
                    let _warm_load = self.materialized_reads.start_warm_load();
                    if let Some(snapshot) = self
                        .materialized_reads
                        .serving_snapshot_for_table_with_mode(table, required_sequence, false)
                    {
                        return Ok(snapshot);
                    }

                    let generation = self.materialized_reads.next_generation();
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
                            self.materialized_reads.wait_if_publish_pause_armed();
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

                    self.materialized_reads.publish_table_snapshot(
                        table.clone(),
                        generation,
                        replayed_sequence,
                        materialized_by_id,
                    );
                    return self
                        .materialized_serving_snapshot_for_table(table, required_sequence)
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

    #[cfg(test)]
    pub(crate) async fn wait_for_materialized_serving_snapshot_cancellable<Fut>(
        &self,
        required_sequence: SequenceNumber,
        cancel_wait: Fut,
    ) -> Result<ServingSnapshot>
    where
        Fut: Future<Output = ()>,
    {
        self.materialized_reads
            .snapshots
            .wait_for_snapshot_covering_cancellable(required_sequence, cancel_wait)
            .await
    }

    pub(crate) fn record_materialized_query_evaluation(&self) {
        self.materialized_reads.record_evaluation();
    }

    pub(crate) fn record_materialized_paginated_evaluation(&self) {
        self.materialized_reads.record_paginated();
    }

    pub(crate) fn record_materialized_get_hit(&self) {
        self.materialized_reads.record_get_hit();
    }

    pub(crate) fn invalidate_document_cache_for_commit(&self, commit: &CommitEntry) {
        self.document_cache.invalidate_commit(commit);
        self.materialized_reads.apply_commit(commit);
    }

    pub(crate) fn invalidate_document_cache_for_commits<'a>(
        &self,
        commits: impl IntoIterator<Item = &'a CommitEntry>,
    ) {
        let commits = commits.into_iter().collect::<Vec<_>>();
        self.document_cache
            .invalidate_commits(commits.iter().copied());
        self.materialized_reads.apply_commits(commits);
    }

    pub(crate) fn clear_document_cache(&self) {
        self.document_cache.clear();
        self.materialized_reads.clear();
    }

    pub(crate) fn record_query_plan_metric(
        &self,
        operation: QueryPlanMetricOperation,
        kind: QueryPlanMetricKind,
    ) {
        self.query_planning.record(operation, kind);
    }

    pub(crate) fn ensure_subscription_delivery_worker_started(self: &Arc<Self>) {
        self.subscription_delivery.start_worker(self);
    }

    pub(crate) fn enqueue_subscription_work(
        &self,
        work: QueuedSubscriptionWork,
    ) -> std::result::Result<(), QueuedSubscriptionWork> {
        self.subscription_delivery.enqueue(work)
    }

    pub(crate) fn record_subscription_dispatch_stats(&self, stats: SubscriptionDispatchStats) {
        self.subscription_delivery.record_dispatch_stats(stats);
    }

    pub(crate) fn record_subscription_overflow_sync_fallback(&self) {
        self.subscription_delivery.record_overflow_sync_fallback();
    }

    pub(crate) fn record_subscription_coalesced_batch(
        &self,
        commit_count: u64,
        merged_subscription_wakeup_count: u64,
    ) {
        self.subscription_delivery
            .record_coalesced_batch(commit_count, merged_subscription_wakeup_count);
    }

    pub(crate) fn shutdown_subscription_delivery(&self) {
        self.subscription_delivery.shutdown();
    }

    pub(crate) fn enqueue_mutation_admission_request(
        &self,
        request: QueuedMutationRequest,
    ) -> Result<bool> {
        self.mutation_admission.enqueue(request)?;
        Ok(self.mutation_journal.try_start_worker())
    }

    pub(crate) fn drain_mutation_admission_queue(&self) {
        loop {
            match self.mutation_admission.pop_next_at(Instant::now()) {
                MutationAdmissionDecision::Admit(request) => {
                    if let Err(enqueue_error) = self.mutation_journal.enqueue(request) {
                        let (request, error) = *enqueue_error;
                        let _ = request.response.send(Err(error));
                    }
                }
                MutationAdmissionDecision::Reject { request, error } => {
                    let _ = request.response.send(Err(error));
                }
                MutationAdmissionDecision::Empty => break,
            }
        }
    }

    pub(crate) async fn drain_mutation_batch(
        &self,
        max_batch_size: usize,
    ) -> Vec<QueuedMutationRequest> {
        #[cfg(test)]
        self.mutation_journal.wait_before_drain().await;
        let mut batch = self.mutation_journal.drain_batch(max_batch_size).await;
        let batch_limit = max_batch_size.max(1);
        while batch.len() < batch_limit {
            match self.mutation_admission.pop_next_at(Instant::now()) {
                MutationAdmissionDecision::Admit(request) => batch.push(request),
                MutationAdmissionDecision::Reject { request, error } => {
                    let _ = request.response.send(Err(error));
                }
                MutationAdmissionDecision::Empty => break,
            }
        }
        batch
    }

    pub(crate) fn release_mutation_worker(&self) -> bool {
        self.mutation_journal
            .release_worker(self.mutation_admission.has_pending())
    }

    pub(crate) fn record_mutation_worker_start(&self) {
        self.mutation_journal.record_worker_start();
    }

    pub(crate) fn record_mutation_worker_failure(&self) {
        self.mutation_journal.record_worker_failure();
    }

    pub(crate) fn begin_pending_mutation_response(&self) {
        self.mutation_journal.begin_pending_response();
    }

    pub(crate) fn finish_pending_mutation_response(&self) {
        self.mutation_journal.finish_pending_response();
    }

    pub(crate) fn durable_head(&self) -> SequenceNumber {
        self.mutation_journal.durable_head()
    }

    pub(crate) fn applied_head(&self) -> SequenceNumber {
        self.mutation_journal.applied_head()
    }

    pub(crate) fn lock_mutation_sequence(&self) -> std::sync::MutexGuard<'_, ()> {
        self.mutation_journal.lock_sequence_gate()
    }

    pub(crate) fn mark_durable_head(&self, sequence: SequenceNumber) {
        self.mutation_journal.mark_durable_head(sequence);
    }

    pub(crate) fn mark_applied_head(&self, sequence: SequenceNumber) {
        self.mutation_journal.mark_applied_head(sequence);
    }

    pub(crate) async fn wait_for_applied_sequence_cancellable<Fut>(
        &self,
        sequence: SequenceNumber,
        cancel_wait: Fut,
    ) -> Result<()>
    where
        Fut: Future<Output = ()>,
    {
        self.mutation_journal
            .wait_for_applied_sequence_cancellable(sequence, cancel_wait)
            .await
    }

    pub(crate) fn wait_for_applied_sequence_blocking(&self, sequence: SequenceNumber) {
        self.mutation_journal
            .wait_for_applied_sequence_blocking(sequence);
    }

    pub(crate) fn sync_mutation_journal_progress(&self, progress: JournalProgress) {
        self.mark_durable_head(progress.durable_head);
        self.mark_applied_head(progress.applied_head);
    }

    #[cfg(test)]
    pub(crate) fn document_cache_stats(&self) -> DocumentCacheStats {
        self.document_cache.stats()
    }

    pub(crate) fn materialized_read_surface_stats(&self) -> MaterializedReadSurfaceStats {
        self.materialized_reads.stats()
    }

    #[cfg(test)]
    pub(crate) fn materialized_table_publication_stats(
        &self,
        table: &TableName,
    ) -> Option<MaterializedTablePublicationStats> {
        self.materialized_reads.table_publication_stats(table)
    }

    #[cfg(test)]
    pub(crate) fn materialized_serving_snapshot_for_testing(
        &self,
        required_sequence: SequenceNumber,
    ) -> Option<ServingSnapshot> {
        self.materialized_serving_snapshot(required_sequence)
    }

    pub(crate) fn serving_snapshot_manager_stats(&self) -> ServingSnapshotManagerStats {
        self.materialized_reads.snapshots.stats()
    }

    #[cfg(test)]
    pub(crate) fn set_materialized_read_surface_limits_for_testing(
        &self,
        table_capacity: usize,
        byte_capacity: usize,
    ) {
        self.materialized_reads
            .set_limits_for_testing(table_capacity, byte_capacity);
    }

    #[cfg(test)]
    pub(crate) fn set_materialized_read_surface_version_capacity_for_testing(
        &self,
        version_capacity: usize,
    ) {
        self.materialized_reads
            .set_version_capacity_for_testing(version_capacity);
    }

    pub(crate) fn mutation_admission_stats(&self) -> MutationAdmissionStats {
        self.mutation_admission.stats()
    }

    pub(crate) fn mutation_journal_stats(&self) -> MutationJournalStats {
        self.mutation_journal.stats()
    }

    pub(crate) fn subscription_delivery_stats(&self) -> SubscriptionDeliveryStats {
        self.subscription_delivery.stats()
    }

    pub(crate) fn query_planning_stats(&self) -> QueryPlanningStats {
        self.query_planning.stats()
    }

    pub(crate) fn engine_diagnostics_snapshot(&self) -> TenantEngineDiagnosticsSnapshot {
        TenantEngineDiagnosticsSnapshot {
            mutation_admission: self.mutation_admission_stats(),
            mutation_journal: self.mutation_journal_stats(),
            subscription_delivery: self.subscription_delivery_stats(),
            materialized_read_surface: self.materialized_read_surface_stats(),
            serving_snapshot_manager: self.serving_snapshot_manager_stats(),
            query_planning: self.query_planning_stats(),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_subscription_delivery_queue_capacity_for_testing(&self, capacity: usize) {
        self.subscription_delivery
            .set_capacity_for_testing(capacity);
    }

    #[cfg(test)]
    pub(crate) fn set_mutation_journal_queue_capacity_for_testing(&self, capacity: usize) {
        self.mutation_journal.set_capacity_for_testing(capacity);
    }

    #[cfg(test)]
    pub(crate) fn set_mutation_admission_codel_for_testing(
        &self,
        target: Duration,
        interval: Duration,
    ) {
        self.mutation_admission
            .set_codel_for_testing(target, interval);
    }

    #[cfg(test)]
    pub(crate) fn subscription_delivery_pause_handle_for_testing(
        &self,
    ) -> SubscriptionDeliveryPauseHandle {
        self.subscription_delivery.pause_handle()
    }

    #[cfg(test)]
    pub(crate) fn materialized_read_publish_pause_handle_for_testing(
        &self,
    ) -> MaterializedReadPublishPauseHandle {
        self.materialized_reads.publish_pause_handle()
    }

    #[cfg(test)]
    pub(crate) fn mutation_journal_pause_handle_for_testing(&self) -> MutationJournalPauseHandle {
        MutationJournalPauseHandle {
            state: self.mutation_journal.pause_before_drain.clone(),
        }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn subscription_bootstrap_pause_handle_for_testing(
        &self,
    ) -> MutationJournalPauseHandle {
        MutationJournalPauseHandle {
            state: self.subscription_bootstrap_pause.clone(),
        }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) async fn wait_if_subscription_bootstrap_pause_armed(&self) {
        self.subscription_bootstrap_pause.wait_if_armed().await;
    }
}

#[cfg(test)]
mod mutation_admission_tests;
