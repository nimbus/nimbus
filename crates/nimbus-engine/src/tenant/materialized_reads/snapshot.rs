use std::collections::{BTreeMap, HashMap, VecDeque};
#[cfg(test)]
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::{
    Document, DocumentId, Error, HistoricalReadErrorKind, HistoricalReadShape, Result,
    SequenceNumber, TableId, TableName,
};
use tokio::sync::Notify;

use super::stats::ServingSnapshotManagerStats;

pub(super) type MaterializedTableDocuments = HashMap<DocumentId, Document>;

#[derive(Clone)]
pub(crate) struct ServingSnapshot {
    inner: Arc<ServingSnapshotInner>,
}

#[derive(Clone)]
pub struct PinnedServingReadSnapshot {
    snapshot: ServingSnapshot,
    read_shape: HistoricalReadShape,
}

struct ServingSnapshotInner {
    covered_sequence: SequenceNumber,
    tables: Arc<HashMap<TableName, Arc<MaterializedTableDocuments>>>,
}

#[derive(Default)]
struct ServingSnapshotManagerState {
    versions: VecDeque<ServingSnapshot>,
    waiters: BTreeMap<u64, Vec<Arc<Notify>>>,
}

pub(super) struct ServingSnapshotManager {
    state: Mutex<ServingSnapshotManagerState>,
    pruned_version_count: AtomicU64,
    discarded_out_of_order_count: AtomicU64,
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

    pub(crate) fn table_document_count(&self, table: &TableName) -> Option<usize> {
        self.inner
            .tables
            .get(table)
            .map(|documents| documents.len())
    }

    pub(crate) fn document(&self, table: &TableName, document_id: &DocumentId) -> Option<Document> {
        self.inner
            .tables
            .get(table)
            .and_then(|documents| documents.get(document_id))
            .cloned()
    }

    pub(crate) fn pin_read_shape(
        &self,
        read_shape: HistoricalReadShape,
    ) -> Result<PinnedServingReadSnapshot> {
        let required_sequence = read_shape.read_snapshot().sequence().sequence();
        if self.covered_sequence().0 < required_sequence.0 {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::SnapshotUnavailable,
                format!(
                    "serving snapshot covers sequence {}, below historical read sequence {} for table {}",
                    self.covered_sequence().0,
                    required_sequence.0,
                    read_shape.table()
                ),
            ));
        }
        if !self.contains_table(read_shape.table()) {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::SnapshotUnavailable,
                format!(
                    "serving snapshot covering sequence {} does not include table {}",
                    self.covered_sequence().0,
                    read_shape.table()
                ),
            ));
        }
        Ok(PinnedServingReadSnapshot {
            snapshot: self.clone(),
            read_shape,
        })
    }

    pub(super) fn from_tables(
        covered_sequence: SequenceNumber,
        tables: HashMap<TableName, Arc<MaterializedTableDocuments>>,
    ) -> Self {
        Self {
            inner: Arc::new(ServingSnapshotInner {
                covered_sequence,
                tables: Arc::new(tables),
            }),
        }
    }

    pub(super) fn contains_table(&self, table: &TableName) -> bool {
        self.inner.tables.contains_key(table)
    }

    fn pin_count(&self) -> usize {
        Arc::strong_count(&self.inner)
    }
}

impl PinnedServingReadSnapshot {
    pub fn covered_sequence(&self) -> SequenceNumber {
        self.snapshot.covered_sequence()
    }

    pub fn read_shape(&self) -> &HistoricalReadShape {
        &self.read_shape
    }

    pub fn table_id(&self) -> &TableId {
        self.read_shape.table_id()
    }

    pub fn table_documents(&self) -> Option<Vec<Document>> {
        self.snapshot.table_documents(self.read_shape.table())
    }

    pub fn document(&self, document_id: &DocumentId) -> Option<Document> {
        self.snapshot.document(self.read_shape.table(), document_id)
    }
}

impl ServingSnapshotManager {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(ServingSnapshotManagerState::default()),
            pruned_version_count: AtomicU64::new(0),
            discarded_out_of_order_count: AtomicU64::new(0),
        }
    }

    pub(super) fn publish(&self, snapshot: ServingSnapshot, version_capacity: usize) {
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

    /// Widens the latest retained version's coverage to `snapshot`'s sequence
    /// in place, instead of retaining it as a new history entry. Callers use
    /// this exclusively for zero-write commits (the trigger-candidate feed's
    /// delivery-cursor advance): `snapshot`'s table contents are guaranteed
    /// byte-identical to the current latest version's, since nothing was
    /// written, so there is no distinct historical revision to keep around.
    /// Retaining one anyway would only inflate `retained_snapshot_count` with
    /// content-free duplicates and evict genuinely distinct versions sooner.
    ///
    /// The in-place replace is only safe while the current latest is
    /// unpinned. A pin clones the `Arc`-backed snapshot directly, so the pin
    /// itself stays valid either way -- but `prune_locked` also treats a
    /// pinned *front* of the deque as manager-level protection against
    /// pruning it away. Popping a pinned latest here would silently strip
    /// that protection: if it is also the front (the common case with a
    /// small `version_capacity`), the next `publish()` could prune it from
    /// history out from under a caller that still holds the pin. So when the
    /// current latest is pinned, push the widened snapshot alongside it
    /// instead of replacing it; the deque grows by one, bounded by how long
    /// the pin lives, and the next `publish()` prunes normally once it is
    /// released.
    pub(super) fn extend_latest_coverage(&self, snapshot: ServingSnapshot) {
        let mut state = self
            .state
            .lock()
            .expect("serving snapshot manager lock should not be poisoned");
        let sequence = snapshot.covered_sequence();
        match state.versions.back() {
            Some(latest) if latest.covered_sequence().0 >= sequence.0 => return,
            Some(latest) if latest.pin_count() > 1 => {
                state.versions.push_back(snapshot);
            }
            Some(_) => {
                state.versions.pop_back();
                state.versions.push_back(snapshot);
            }
            None => state.versions.push_back(snapshot),
        }
        let ready_waiters = self.take_ready_waiters_locked(&mut state, sequence);
        drop(state);
        for waiter in ready_waiters {
            waiter.notify_waiters();
        }
    }

    #[cfg(test)]
    pub(super) fn snapshot_covering(
        &self,
        required_sequence: SequenceNumber,
    ) -> Option<ServingSnapshot> {
        self.state
            .lock()
            .expect("serving snapshot manager lock should not be poisoned")
            .versions
            .iter()
            .find(|snapshot| snapshot.covered_sequence().0 >= required_sequence.0)
            .cloned()
    }

    /// The coverage of the newest retained version, or `None` before anything
    /// has been published.
    pub(super) fn latest_covered_sequence(&self) -> Option<SequenceNumber> {
        self.state
            .lock()
            .expect("serving snapshot manager lock should not be poisoned")
            .versions
            .back()
            .map(|snapshot| snapshot.covered_sequence())
    }

    pub(super) fn snapshot_covering_table(
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

    pub(super) fn clear(&self) {
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

    pub(super) fn stats(&self) -> ServingSnapshotManagerStats {
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
}

#[cfg(test)]
mod tests {
    use nimbus_core::{
        CommitSequence, CommitTimestamp, HistoricalReadSnapshot, ReadTimestamp, SchemaChangeEvent,
        TableId, TenantEventRecord, Timestamp, VersionedRegistry,
    };

    use super::*;

    fn table() -> TableName {
        TableName::new("tasks").expect("table name should build")
    }

    fn snapshot_at(sequence: SequenceNumber) -> ServingSnapshot {
        let mut tables = HashMap::new();
        tables.insert(table(), Arc::new(MaterializedTableDocuments::new()));
        ServingSnapshot::from_tables(sequence, tables)
    }

    fn read_shape_at(sequence: SequenceNumber) -> HistoricalReadShape {
        let table = table();
        let table_id = TableId::new();
        let registry = VersionedRegistry::from_records([TenantEventRecord::schema_change(
            SequenceNumber(1),
            Timestamp(100),
            SchemaChangeEvent::SetTable {
                table: table.clone(),
                table_id: table_id.clone(),
                previous: None,
                current: nimbus_core::TableSchema {
                    table: table.clone(),
                    fields: Vec::new(),
                    indexes: Vec::new(),
                    access_policy: None,
                },
            },
        )
        .expect("schema change event should build")])
        .expect("registry should build");
        let timestamp = Timestamp(sequence.0.saturating_mul(100));
        registry
            .read_shape_at(
                &table,
                HistoricalReadSnapshot::new(
                    ReadTimestamp::new(timestamp),
                    CommitSequence::new(sequence),
                    CommitTimestamp::new(timestamp),
                ),
            )
            .expect("read shape should load")
            .expect("table should exist at historical read")
    }

    /// Pins the latest retained snapshot (`pin_read_shape`), then widens it
    /// with a newer snapshot: the pinned original must stay retained by the
    /// manager -- not just independently valid through the pin's own `Arc`
    /// clone -- alongside the widened snapshot as the new latest.
    #[test]
    fn extend_latest_coverage_retains_pinned_latest_alongside_widened_snapshot() {
        let manager = ServingSnapshotManager::new();
        let original = snapshot_at(SequenceNumber(5));
        manager.publish(original.clone(), 10);

        let pinned = original
            .pin_read_shape(read_shape_at(SequenceNumber(5)))
            .expect("snapshot should pin the read shape it covers");
        drop(original);

        manager.extend_latest_coverage(snapshot_at(SequenceNumber(6)));

        let stats = manager.stats();
        assert_eq!(
            stats.retained_snapshot_count, 2,
            "a pinned latest must not be popped by extend_latest_coverage; the widened \
             snapshot should be retained alongside it instead of replacing it"
        );
        assert_eq!(stats.latest_retained_sequence, Some(SequenceNumber(6)));
        assert_eq!(
            manager
                .snapshot_covering(SequenceNumber(5))
                .map(|snapshot| snapshot.covered_sequence()),
            Some(SequenceNumber(5)),
            "the pinned original must still be retrievable from the manager's own history, \
             not just kept alive by the caller's independent pin"
        );
        assert_eq!(pinned.covered_sequence(), SequenceNumber(5));
    }

    /// The in-place replace behavior is unchanged when the current latest
    /// carries no outstanding pin.
    #[test]
    fn extend_latest_coverage_replaces_unpinned_latest_in_place() {
        let manager = ServingSnapshotManager::new();
        manager.publish(snapshot_at(SequenceNumber(1)), 10);

        manager.extend_latest_coverage(snapshot_at(SequenceNumber(2)));

        let stats = manager.stats();
        assert_eq!(
            stats.retained_snapshot_count, 1,
            "an unpinned latest should still be replaced in place, not retained alongside \
             the widened snapshot"
        );
        assert_eq!(stats.latest_retained_sequence, Some(SequenceNumber(2)));
    }
}
