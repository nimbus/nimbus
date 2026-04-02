use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Instant;

use neovex_core::{
    CommitEntry, Document, DocumentId, Error, Mutation, PrincipalContext, Result, Schema,
    SequenceNumber, TableName, TenantId,
};
use neovex_storage::{JournalProgress, RedbTenantStorage, TenantStore};
use tokio::sync::{Mutex as AsyncMutex, Notify, oneshot};

use crate::subscriptions::{
    QueuedSubscriptionWork, SubscriptionDispatchStats, SubscriptionRegistry,
    dispatch_subscription_work,
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
pub(crate) const DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY: usize = 256;

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

/// Runtime state for a loaded tenant.
pub struct TenantRuntime {
    pub store: Arc<TenantStore>,
    pub read_storage: Arc<RedbTenantStorage>,
    pub subscriptions: SubscriptionRegistry,
    pub schema: RwLock<Arc<Schema>>,
    document_cache: TenantDocumentCache,
    subscription_delivery: SubscriptionDeliveryQueue,
    lifecycle: Arc<TenantLifecycle>,
    mutation_journal: Arc<MutationJournalState>,
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
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MutationJournalStats {
    pub durable_head: SequenceNumber,
    pub applied_head: SequenceNumber,
    pub apply_lag: u64,
    pub read_wait_count: u64,
    pub total_read_wait_nanos: u64,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SubscriptionDeliveryStats {
    pub queue_depth: usize,
    pub oldest_queue_age_nanos: u64,
    pub overflow_sync_fallback_count: u64,
    pub coalesced_batch_count: u64,
    pub coalesced_commit_count: u64,
    pub merged_subscription_wakeup_count: u64,
    pub coalesced_work_count: u64,
    pub reevaluation_count: u64,
    pub total_reevaluation_nanos: u64,
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

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct MutationJournalPauseHandle {
    state: Arc<MutationJournalPauseState>,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct MutationJournalPauseControl {
    armed: bool,
    entered: bool,
    released: bool,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct MutationJournalPauseState {
    control: Mutex<MutationJournalPauseControl>,
    entered: Condvar,
    released: Notify,
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

#[cfg(test)]
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
    queue: AsyncMutex<VecDeque<QueuedMutationRequest>>,
    worker_running: AtomicBool,
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
    overflow_sync_fallback_count: AtomicU64,
    coalesced_batch_count: AtomicU64,
    coalesced_commit_count: AtomicU64,
    merged_subscription_wakeup_count: AtomicU64,
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

impl MutationJournalState {
    fn new(progress: JournalProgress) -> Self {
        Self {
            queue: AsyncMutex::new(VecDeque::new()),
            worker_running: AtomicBool::new(false),
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

    async fn enqueue(&self, request: QueuedMutationRequest) -> bool {
        self.queue.lock().await.push_back(request);
        self.worker_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    async fn drain_batch(&self, max_batch_size: usize) -> Vec<QueuedMutationRequest> {
        #[cfg(test)]
        self.pause_before_drain.wait_if_armed().await;
        let mut queue = self.queue.lock().await;
        let batch_size = queue.len().min(max_batch_size.max(1));
        queue.drain(..batch_size).collect()
    }

    async fn release_worker(&self) -> bool {
        self.worker_running.store(false, Ordering::Release);
        let queue_has_more = !self.queue.lock().await.is_empty();
        queue_has_more
            && self
                .worker_running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
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

    #[cfg(test)]
    fn stats(&self) -> MutationJournalStats {
        let durable_head = self.durable_head();
        let applied_head = self.applied_head();
        MutationJournalStats {
            durable_head,
            applied_head,
            apply_lag: durable_head.0.saturating_sub(applied_head.0),
            read_wait_count: self.read_wait_count.load(Ordering::Relaxed),
            total_read_wait_nanos: self.total_read_wait_nanos.load(Ordering::Relaxed),
        }
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

impl SubscriptionDeliveryQueue {
    fn new() -> Self {
        Self {
            state: Arc::new(SubscriptionDeliveryState {
                queue: Mutex::new(VecDeque::new()),
                queue_ready: Condvar::new(),
                worker: Mutex::new(None),
                shutdown: AtomicBool::new(false),
                capacity: AtomicUsize::new(DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY),
                overflow_sync_fallback_count: AtomicU64::new(0),
                coalesced_batch_count: AtomicU64::new(0),
                coalesced_commit_count: AtomicU64::new(0),
                merged_subscription_wakeup_count: AtomicU64::new(0),
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
        let state = self.state.clone();
        let runtime = Arc::downgrade(runtime);
        *worker = Some(
            std::thread::Builder::new()
                .name("neovex-subscription-delivery".to_string())
                .spawn(move || {
                    loop {
                        let work =
                            {
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

    fn enqueue(&self, work: QueuedSubscriptionWork) -> bool {
        let mut queue = self
            .state
            .queue
            .lock()
            .expect("subscription delivery queue lock should not be poisoned");
        if queue.len() >= self.state.capacity.load(Ordering::Acquire).max(1) {
            return false;
        }
        queue.push_back(work);
        self.state.queue_ready.notify_one();
        true
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
            let _ = worker.join();
        }
    }

    #[cfg(test)]
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
        SubscriptionDeliveryStats {
            queue_depth: queue.len(),
            oldest_queue_age_nanos,
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
            subscription_delivery: SubscriptionDeliveryQueue::new(),
            lifecycle: Arc::new(TenantLifecycle::new()),
            mutation_journal: Arc::new(MutationJournalState::new(progress)),
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

    pub(crate) fn invalidate_document_cache_for_commit(&self, commit: &CommitEntry) {
        self.document_cache.invalidate_commit(commit);
    }

    pub(crate) fn invalidate_document_cache_for_commits<'a>(
        &self,
        commits: impl IntoIterator<Item = &'a CommitEntry>,
    ) {
        self.document_cache.invalidate_commits(commits);
    }

    pub(crate) fn clear_document_cache(&self) {
        self.document_cache.clear();
    }

    pub(crate) fn ensure_subscription_delivery_worker_started(self: &Arc<Self>) {
        self.subscription_delivery.start_worker(self);
    }

    pub(crate) fn enqueue_subscription_work(&self, work: QueuedSubscriptionWork) -> bool {
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

    pub(crate) async fn enqueue_mutation_request(&self, request: QueuedMutationRequest) -> bool {
        self.mutation_journal.enqueue(request).await
    }

    pub(crate) async fn drain_mutation_batch(
        &self,
        max_batch_size: usize,
    ) -> Vec<QueuedMutationRequest> {
        self.mutation_journal.drain_batch(max_batch_size).await
    }

    pub(crate) async fn release_mutation_worker(&self) -> bool {
        self.mutation_journal.release_worker().await
    }

    pub(crate) fn durable_head(&self) -> SequenceNumber {
        self.mutation_journal.durable_head()
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

    #[cfg(test)]
    pub(crate) fn mutation_journal_stats(&self) -> MutationJournalStats {
        self.mutation_journal.stats()
    }

    #[cfg(test)]
    pub(crate) fn subscription_delivery_stats(&self) -> SubscriptionDeliveryStats {
        self.subscription_delivery.stats()
    }

    #[cfg(test)]
    pub(crate) fn set_subscription_delivery_queue_capacity_for_testing(&self, capacity: usize) {
        self.subscription_delivery
            .set_capacity_for_testing(capacity);
    }

    #[cfg(test)]
    pub(crate) fn subscription_delivery_pause_handle_for_testing(
        &self,
    ) -> SubscriptionDeliveryPauseHandle {
        self.subscription_delivery.pause_handle()
    }

    #[cfg(test)]
    pub(crate) fn mutation_journal_pause_handle_for_testing(&self) -> MutationJournalPauseHandle {
        MutationJournalPauseHandle {
            state: self.mutation_journal.pause_before_drain.clone(),
        }
    }
}
