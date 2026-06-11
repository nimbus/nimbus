use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(test)]
use std::sync::{Arc, Condvar};
#[cfg(test)]
use std::time::Instant;

use nimbus_core::{CommitEntry, Document, DocumentId, TableName};

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

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DocumentCacheStats {
    pub hits: usize,
    pub misses: usize,
    pub entries: usize,
    pub evictions: usize,
}

pub(super) struct TenantDocumentCache {
    state: Mutex<TenantDocumentCacheState>,
    hits: AtomicUsize,
    misses: AtomicUsize,
    evictions: AtomicUsize,
    #[cfg(test)]
    invalidation_pause: Arc<DocumentCacheInvalidationPauseState>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct DocumentCacheInvalidationPauseHandle {
    state: Arc<DocumentCacheInvalidationPauseState>,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct DocumentCacheInvalidationPauseControl {
    armed: bool,
    entered: bool,
    released: bool,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct DocumentCacheInvalidationPauseState {
    control: Mutex<DocumentCacheInvalidationPauseControl>,
    condvar: Condvar,
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
        let key = (document.table.clone(), document.id.clone());
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
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(TenantDocumentCacheState::default()),
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
            evictions: AtomicUsize::new(0),
            #[cfg(test)]
            invalidation_pause: Arc::new(DocumentCacheInvalidationPauseState::default()),
        }
    }

    pub(super) fn get(&self, table: &TableName, document_id: &DocumentId) -> Option<Document> {
        let key = (table.clone(), document_id.clone());
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

    pub(super) fn insert(&self, document: &Document) {
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

    pub(super) fn insert_documents<'a>(&self, documents: impl IntoIterator<Item = &'a Document>) {
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

    pub(super) fn invalidate_commit(&self, commit: &CommitEntry) {
        #[cfg(test)]
        self.invalidation_pause.wait_if_armed();

        let mut state = self
            .state
            .lock()
            .expect("document cache lock should not be poisoned");
        for write in &commit.writes {
            state
                .documents
                .remove(&(write.table.clone(), write.doc_id.clone()));
        }
    }

    pub(super) fn invalidate_commits<'a>(
        &self,
        commits: impl IntoIterator<Item = &'a CommitEntry>,
    ) {
        #[cfg(test)]
        self.invalidation_pause.wait_if_armed();

        let mut state = self
            .state
            .lock()
            .expect("document cache lock should not be poisoned");
        for commit in commits {
            for write in &commit.writes {
                state
                    .documents
                    .remove(&(write.table.clone(), write.doc_id.clone()));
            }
        }
    }

    pub(super) fn clear(&self) {
        *self
            .state
            .lock()
            .expect("document cache lock should not be poisoned") =
            TenantDocumentCacheState::default();
    }

    #[cfg(test)]
    pub(super) fn stats(&self) -> DocumentCacheStats {
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

    #[cfg(test)]
    pub(super) fn invalidation_pause_handle(&self) -> DocumentCacheInvalidationPauseHandle {
        DocumentCacheInvalidationPauseHandle {
            state: self.invalidation_pause.clone(),
        }
    }
}

#[cfg(test)]
impl DocumentCacheInvalidationPauseHandle {
    pub(crate) fn arm(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("document cache invalidation pause lock should not be poisoned");
        *control = DocumentCacheInvalidationPauseControl {
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
            .expect("document cache invalidation pause lock should not be poisoned");
        while !control.entered {
            let now = Instant::now();
            let Some(remaining) = deadline.checked_duration_since(now) else {
                return false;
            };
            let (next, result) = self
                .state
                .condvar
                .wait_timeout(control, remaining)
                .expect("document cache invalidation pause wait should not be poisoned");
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
            .expect("document cache invalidation pause lock should not be poisoned");
        control.released = true;
        self.state.condvar.notify_all();
    }
}

#[cfg(test)]
impl DocumentCacheInvalidationPauseState {
    fn wait_if_armed(&self) {
        let mut control = self
            .control
            .lock()
            .expect("document cache invalidation pause lock should not be poisoned");
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
                .expect("document cache invalidation pause wait should not be poisoned");
        }
        *control = DocumentCacheInvalidationPauseControl::default();
    }
}
