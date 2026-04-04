use std::sync::Arc;

use neovex_core::{CommitEntry, Document, Result};
use neovex_storage::TenantStore;

use crate::{Service, tenant::TenantRuntime};

impl Service {
    pub(super) fn run_store_mutation<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mutate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&TenantStore) -> Result<CommitEntry>,
    {
        let commit = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            mutate(&runtime.store)?
        };
        runtime.mark_durable_head(commit.sequence);
        runtime.invalidate_document_cache_for_commit(&commit);
        runtime.mark_applied_head(commit.sequence);
        self.process_commit(runtime, &commit);
        Ok(commit)
    }

    pub(super) fn run_store_mutation_once<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mutate: F,
    ) -> Result<bool>
    where
        F: FnOnce(&TenantStore) -> Result<Option<CommitEntry>>,
    {
        let commit = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            mutate(&runtime.store)?
        };
        let Some(commit) = commit else {
            return Ok(false);
        };
        runtime.mark_durable_head(commit.sequence);
        runtime.invalidate_document_cache_for_commit(&commit);
        runtime.mark_applied_head(commit.sequence);
        self.process_commit(runtime, &commit);
        Ok(true)
    }

    pub(super) fn run_store_delete_mutation<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mutate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&TenantStore) -> Result<(CommitEntry, Document)>,
    {
        let (commit, _deleted_document) = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            mutate(&runtime.store)?
        };
        runtime.mark_durable_head(commit.sequence);
        runtime.invalidate_document_cache_for_commit(&commit);
        runtime.mark_applied_head(commit.sequence);
        self.process_commit(runtime, &commit);
        Ok(commit)
    }

    pub(super) fn run_store_delete_mutation_once<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mutate: F,
    ) -> Result<bool>
    where
        F: FnOnce(&TenantStore) -> Result<Option<(CommitEntry, Document)>>,
    {
        let commit = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            mutate(&runtime.store)?
        };
        let Some((commit, _deleted_document)) = commit else {
            return Ok(false);
        };
        runtime.mark_durable_head(commit.sequence);
        runtime.invalidate_document_cache_for_commit(&commit);
        runtime.mark_applied_head(commit.sequence);
        self.process_commit(runtime, &commit);
        Ok(true)
    }
}
