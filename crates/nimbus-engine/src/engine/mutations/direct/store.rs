use std::sync::Arc;

use nimbus_core::{CommitEntry, Document, Result};

use crate::persistence::TenantPersistence;
use crate::{Engine, tenant::TenantRuntime};

use super::super::prepared::PreparedCommit;

impl Engine {
    pub(super) fn run_store_mutation<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        prepared_commit: PreparedCommit,
        mutate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&TenantPersistence, &PreparedCommit) -> Result<CommitEntry>,
    {
        let commit = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            let commit = mutate(runtime.store(), &prepared_commit)?;
            runtime.mark_durable_head(commit.sequence);
            runtime.invalidate_document_cache_for_commit(&commit);
            runtime.mark_applied_head(commit.sequence);
            commit
        };
        self.process_commit(runtime, &commit);
        Ok(commit)
    }

    pub(super) fn run_store_mutation_once<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        prepared_commit: PreparedCommit,
        mutate: F,
    ) -> Result<bool>
    where
        F: FnOnce(&TenantPersistence, &PreparedCommit) -> Result<Option<CommitEntry>>,
    {
        let commit = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            let commit = mutate(runtime.store(), &prepared_commit)?;
            if let Some(commit) = &commit {
                runtime.mark_durable_head(commit.sequence);
                runtime.invalidate_document_cache_for_commit(commit);
                runtime.mark_applied_head(commit.sequence);
            }
            commit
        };
        let Some(commit) = commit else {
            return Ok(false);
        };
        self.process_commit(runtime, &commit);
        Ok(true)
    }

    pub(super) fn run_store_delete_mutation<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        prepared_commit: PreparedCommit,
        mutate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&TenantPersistence, &PreparedCommit) -> Result<(CommitEntry, Document)>,
    {
        let (commit, _deleted_document) = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            let (commit, deleted_document) = mutate(runtime.store(), &prepared_commit)?;
            runtime.mark_durable_head(commit.sequence);
            runtime.invalidate_document_cache_for_commit(&commit);
            runtime.mark_applied_head(commit.sequence);
            (commit, deleted_document)
        };
        self.process_commit(runtime, &commit);
        Ok(commit)
    }

    pub(super) fn run_store_delete_mutation_once<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        prepared_commit: PreparedCommit,
        mutate: F,
    ) -> Result<bool>
    where
        F: FnOnce(&TenantPersistence, &PreparedCommit) -> Result<Option<(CommitEntry, Document)>>,
    {
        let commit = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            let commit = mutate(runtime.store(), &prepared_commit)?;
            if let Some((commit, _deleted_document)) = &commit {
                runtime.mark_durable_head(commit.sequence);
                runtime.invalidate_document_cache_for_commit(commit);
                runtime.mark_applied_head(commit.sequence);
            }
            commit
        };
        let Some((commit, _deleted_document)) = commit else {
            return Ok(false);
        };
        self.process_commit(runtime, &commit);
        Ok(true)
    }
}
