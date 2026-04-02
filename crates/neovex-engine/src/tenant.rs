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

use crate::subscriptions::SubscriptionRegistry;

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DocumentCacheStats {
    pub hits: usize,
    pub misses: usize,
    pub entries: usize,
    pub evictions: usize,
}

pub(crate) const DOCUMENT_CACHE_CAPACITY: usize = 256;

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
        }
    }

    async fn enqueue(&self, request: QueuedMutationRequest) -> bool {
        self.queue.lock().await.push_back(request);
        self.worker_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    async fn drain_batch(&self, max_batch_size: usize) -> Vec<QueuedMutationRequest> {
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

    pub(crate) fn clear_document_cache(&self) {
        self.document_cache.clear();
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
}
