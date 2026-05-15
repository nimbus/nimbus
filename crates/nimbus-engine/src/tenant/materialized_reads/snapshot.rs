use std::collections::{BTreeMap, HashMap, VecDeque};
#[cfg(test)]
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::{Document, DocumentId, SequenceNumber, TableName};
#[cfg(test)]
use nimbus_core::{Error, Result};
use tokio::sync::Notify;

use super::stats::ServingSnapshotManagerStats;

pub(super) type MaterializedTableDocuments = HashMap<DocumentId, Document>;

#[derive(Clone)]
pub(crate) struct ServingSnapshot {
    inner: Arc<ServingSnapshotInner>,
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
