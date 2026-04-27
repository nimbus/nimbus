use std::sync::Arc;

use neovex_core::{CommitEntry, Document, Result};

use crate::persistence::TenantPersistence;
use crate::{Service, tenant::TenantRuntime};

impl Service {
    pub(super) fn run_store_mutation<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mutate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&TenantPersistence) -> Result<CommitEntry>,
    {
        let commit = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            let commit = mutate(runtime.store())?;
            runtime.mark_durable_head(commit.sequence);
            runtime.mark_applied_head(commit.sequence);
            commit
        };
        runtime.invalidate_document_cache_for_commit(&commit);
        self.process_commit(runtime, &commit);
        Ok(commit)
    }

    pub(super) fn run_store_mutation_once<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mutate: F,
    ) -> Result<bool>
    where
        F: FnOnce(&TenantPersistence) -> Result<Option<CommitEntry>>,
    {
        let commit = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            let commit = mutate(runtime.store())?;
            if let Some(commit) = &commit {
                runtime.mark_durable_head(commit.sequence);
                runtime.mark_applied_head(commit.sequence);
            }
            commit
        };
        let Some(commit) = commit else {
            return Ok(false);
        };
        runtime.invalidate_document_cache_for_commit(&commit);
        self.process_commit(runtime, &commit);
        Ok(true)
    }

    pub(super) fn run_store_delete_mutation<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mutate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&TenantPersistence) -> Result<(CommitEntry, Document)>,
    {
        let (commit, _deleted_document) = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            let (commit, deleted_document) = mutate(runtime.store())?;
            runtime.mark_durable_head(commit.sequence);
            runtime.mark_applied_head(commit.sequence);
            (commit, deleted_document)
        };
        runtime.invalidate_document_cache_for_commit(&commit);
        self.process_commit(runtime, &commit);
        Ok(commit)
    }

    pub(super) fn run_store_delete_mutation_once<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mutate: F,
    ) -> Result<bool>
    where
        F: FnOnce(&TenantPersistence) -> Result<Option<(CommitEntry, Document)>>,
    {
        let commit = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            let commit = mutate(runtime.store())?;
            if let Some((commit, _deleted_document)) = &commit {
                runtime.mark_durable_head(commit.sequence);
                runtime.mark_applied_head(commit.sequence);
            }
            commit
        };
        let Some((commit, _deleted_document)) = commit else {
            return Ok(false);
        };
        runtime.invalidate_document_cache_for_commit(&commit);
        self.process_commit(runtime, &commit);
        Ok(true)
    }
}
