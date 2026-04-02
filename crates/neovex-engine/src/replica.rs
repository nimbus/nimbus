use std::sync::Arc;

use neovex_core::{
    Document, DurableMutationRecord, Page, PaginatedQuery, PrincipalContext, Query, Result,
    SequenceNumber, TenantId,
};
use neovex_storage::{
    DEFAULT_DURABLE_JOURNAL_STREAM_LIMIT, DurableJournalBootstrap, MaterializedJournalSnapshot,
    TenantStore,
};

use crate::Service;
use crate::service::{
    paginate_documents_for_store_with_principal, query_documents_for_store_with_principal,
};

/// A narrow read-only replica for one tenant backed by the authoritative journal.
pub struct EmbeddedReplica {
    tenant_id: TenantId,
    store: TenantStore,
    sequence_cursor: SequenceNumber,
}

impl EmbeddedReplica {
    /// Bootstraps an in-memory replica for a tenant from the authoritative journal source.
    pub async fn bootstrap_in_memory(service: &Arc<Service>, tenant_id: TenantId) -> Result<Self> {
        Self::bootstrap_with_store(
            service,
            tenant_id,
            TenantStore::create_in_memory()?,
            DEFAULT_DURABLE_JOURNAL_STREAM_LIMIT,
        )
        .await
    }

    /// Bootstraps a replica into the provided local store.
    pub async fn bootstrap_with_store(
        service: &Arc<Service>,
        tenant_id: TenantId,
        store: TenantStore,
        stream_limit: usize,
    ) -> Result<Self> {
        let bootstrap = service
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await?;
        store.restore_materialized_journal_from_snapshot(&bootstrap.snapshot)?;

        let mut replica = Self {
            tenant_id,
            store,
            sequence_cursor: bootstrap.resume_after,
        };
        replica
            .catch_up_to_sequence(service, bootstrap.bootstrap_cut, stream_limit)
            .await?;
        Ok(replica)
    }

    /// Replays the latest durable journal suffix into the local replica.
    pub async fn catch_up(&mut self, service: &Arc<Service>, stream_limit: usize) -> Result<()> {
        let latest = service
            .latest_sequence_async(self.tenant_id.clone())
            .await?;
        self.catch_up_to_sequence(service, latest, stream_limit)
            .await?;
        self.refresh_schema(service).await
    }

    /// Evaluates a query locally against the replica store.
    pub fn query_documents(&self, query: &Query) -> Result<Vec<Document>> {
        self.query_documents_with_principal(query, &PrincipalContext::anonymous())
    }

    /// Evaluates a query locally against the replica store for the provided principal.
    pub fn query_documents_with_principal(
        &self,
        query: &Query,
        principal: &PrincipalContext,
    ) -> Result<Vec<Document>> {
        let schema = self.store.load_schema()?;
        query_documents_for_store_with_principal(&self.store, &schema, query, principal)
    }

    /// Evaluates a paginated query locally against the replica store.
    pub fn paginate_documents(&self, query: &PaginatedQuery) -> Result<Page> {
        self.paginate_documents_with_principal(query, &PrincipalContext::anonymous())
    }

    /// Evaluates a paginated query locally against the replica store for the provided principal.
    pub fn paginate_documents_with_principal(
        &self,
        query: &PaginatedQuery,
        principal: &PrincipalContext,
    ) -> Result<Page> {
        let schema = self.store.load_schema()?;
        paginate_documents_for_store_with_principal(&self.store, &schema, query, principal)
    }

    pub fn sequence_cursor(&self) -> SequenceNumber {
        self.sequence_cursor
    }

    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        self.store.export_materialized_journal_snapshot()
    }

    pub(crate) fn bootstrap_from_bootstrap(
        tenant_id: TenantId,
        store: TenantStore,
        bootstrap: DurableJournalBootstrap,
        records: Vec<DurableMutationRecord>,
    ) -> Result<Self> {
        store.restore_materialized_journal_from_snapshot(&bootstrap.snapshot)?;
        if !records.is_empty() {
            store.append_durable_records_batch(records)?;
            let progress = store.recover_durable_journal()?;
            return Ok(Self {
                tenant_id,
                store,
                sequence_cursor: progress.applied_head,
            });
        }

        Ok(Self {
            tenant_id,
            store,
            sequence_cursor: bootstrap.resume_after,
        })
    }

    async fn catch_up_to_sequence(
        &mut self,
        service: &Arc<Service>,
        target_sequence: SequenceNumber,
        stream_limit: usize,
    ) -> Result<()> {
        while self.sequence_cursor.0 < target_sequence.0 {
            let page = service
                .stream_durable_journal_async(
                    self.tenant_id.clone(),
                    self.sequence_cursor,
                    stream_limit,
                )
                .await?;

            let records = page
                .records
                .into_iter()
                .take_while(|record| record.sequence.0 <= target_sequence.0)
                .collect::<Vec<_>>();
            if records.is_empty() {
                return Err(neovex_core::Error::Internal(format!(
                    "journal stream made no progress while catching replica {} up to sequence {} from {}",
                    self.tenant_id, target_sequence.0, self.sequence_cursor.0
                )));
            }

            self.store.append_durable_records_batch(records)?;
            let progress = self.store.recover_durable_journal()?;
            self.sequence_cursor = progress.applied_head;
        }
        Ok(())
    }

    async fn refresh_schema(&self, service: &Arc<Service>) -> Result<()> {
        let schema = service.get_schema_async(self.tenant_id.clone()).await?;
        self.store.replace_schema(&schema)
    }
}
