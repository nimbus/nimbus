use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use neovex_core::{CommitEntry, Document, DocumentId, Error, Result, Schema, TableName, TenantId};
use neovex_storage::TenantStore;

use crate::subscriptions::SubscriptionRegistry;

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DocumentCacheStats {
    pub hits: usize,
    pub misses: usize,
}

struct TenantDocumentCache {
    documents: RwLock<HashMap<(TableName, DocumentId), Document>>,
    hits: AtomicUsize,
    misses: AtomicUsize,
}

impl TenantDocumentCache {
    fn new() -> Self {
        Self {
            documents: RwLock::new(HashMap::new()),
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
        }
    }

    fn get(&self, table: &TableName, document_id: DocumentId) -> Option<Document> {
        let document = self
            .documents
            .read()
            .expect("document cache lock should not be poisoned")
            .get(&(table.clone(), document_id))
            .cloned();
        match document {
            Some(document) => {
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
        self.documents
            .write()
            .expect("document cache lock should not be poisoned")
            .insert((document.table.clone(), document.id), document.clone());
    }

    fn insert_documents<'a>(&self, documents: impl IntoIterator<Item = &'a Document>) {
        let mut cache = self
            .documents
            .write()
            .expect("document cache lock should not be poisoned");
        for document in documents {
            cache.insert((document.table.clone(), document.id), document.clone());
        }
    }

    fn invalidate_commit(&self, commit: &CommitEntry) {
        let mut cache = self
            .documents
            .write()
            .expect("document cache lock should not be poisoned");
        for write in &commit.writes {
            cache.remove(&(write.table.clone(), write.doc_id));
        }
    }

    fn clear(&self) {
        self.documents
            .write()
            .expect("document cache lock should not be poisoned")
            .clear();
    }

    #[cfg(test)]
    fn stats(&self) -> DocumentCacheStats {
        DocumentCacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
        }
    }
}

/// Runtime state for a loaded tenant.
pub struct TenantRuntime {
    pub store: Arc<TenantStore>,
    pub subscriptions: SubscriptionRegistry,
    pub schema: RwLock<Schema>,
    document_cache: TenantDocumentCache,
    lifecycle: RwLock<()>,
    deleted: AtomicBool,
}

pub struct TenantOperationGuard<'a> {
    _guard: RwLockReadGuard<'a, ()>,
}

pub struct TenantDeletionGuard<'a> {
    _guard: RwLockWriteGuard<'a, ()>,
}

impl TenantRuntime {
    /// Creates a tenant runtime from a store.
    pub fn new(store: TenantStore) -> Result<Self> {
        let schema = store.load_schema()?;
        Ok(Self {
            store: Arc::new(store),
            subscriptions: SubscriptionRegistry::new(),
            schema: RwLock::new(schema),
            document_cache: TenantDocumentCache::new(),
            lifecycle: RwLock::new(()),
            deleted: AtomicBool::new(false),
        })
    }

    /// Returns the current schema snapshot.
    pub fn schema(&self) -> Schema {
        self.schema
            .read()
            .expect("schema lock should not be poisoned")
            .clone()
    }

    /// Enters a tenant operation, preventing deletion while the operation is active.
    pub fn enter_operation(&self, tenant_id: &TenantId) -> Result<TenantOperationGuard<'_>> {
        let guard = self
            .lifecycle
            .read()
            .expect("tenant lifecycle lock should not be poisoned");
        if self.deleted.load(Ordering::SeqCst) {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        }

        Ok(TenantOperationGuard { _guard: guard })
    }

    /// Begins tenant deletion and blocks until all in-flight operations complete.
    pub fn begin_delete(&self) -> TenantDeletionGuard<'_> {
        let guard = self
            .lifecycle
            .write()
            .expect("tenant lifecycle lock should not be poisoned");
        self.deleted.store(true, Ordering::SeqCst);
        TenantDeletionGuard { _guard: guard }
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

    #[cfg(test)]
    pub(crate) fn document_cache_stats(&self) -> DocumentCacheStats {
        self.document_cache.stats()
    }
}
