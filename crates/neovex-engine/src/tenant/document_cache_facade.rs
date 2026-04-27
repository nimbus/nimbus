use neovex_core::{CommitEntry, Document, DocumentId, TableName};

use super::*;

impl TenantRuntime {
    pub(crate) fn get_cached_document(
        &self,
        table: &TableName,
        document_id: &DocumentId,
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

    #[cfg(test)]
    pub(crate) fn document_cache_stats(&self) -> DocumentCacheStats {
        self.document_cache.stats()
    }
}
