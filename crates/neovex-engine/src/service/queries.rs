use std::future::{Future, pending};
use std::sync::Arc;

use std::cmp::Ordering;

use futures::FutureExt;
use neovex_core::{
    AccessAction, AccessRule, CommitEntry, Document, DocumentId, DurableMutationRecord, Error,
    Filter, FilterOp, Page, PaginatedQuery, PrincipalContext, Query, Result, Schema,
    SequenceNumber, TableName, TableSchema, TenantId, policy_revision_id,
};
use neovex_storage::index::encode_index_value;
use neovex_storage::{
    DurableJournalBootstrap, DurableJournalPage, ShadowMaterializer, ShadowMaterializerConfig,
    TenantReadStorage, TenantStore,
};
use serde_json::Value;

use crate::EmbeddedReplica;
#[cfg(test)]
use crate::evaluator::matches_filters;
use crate::evaluator::{
    evaluate_paginated_cancellable_with_predicate,
    evaluate_paginated_with_docs_cancellable_and_predicate,
    evaluate_query_cancellable_with_predicate, evaluate_query_with_docs_cancellable_and_predicate,
};
use crate::tenant::TenantRuntime;
use crate::verification::{
    ConsistencyScope, ConsistencyVerificationReport, bootstrap_fingerprint,
    collect_durable_journal_bootstrap_mismatches, compare_materialized_journal_snapshots,
    snapshot_fingerprint,
};

use super::Service;

impl Service {
    /// Lists documents in a logical table.
    pub fn list_documents(&self, tenant_id: &TenantId, table: &TableName) -> Result<Vec<Document>> {
        self.list_documents_with_principal(tenant_id, table, &PrincipalContext::anonymous())
    }

    /// Lists documents in a logical table for the provided principal.
    pub fn list_documents_with_principal(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        principal: &PrincipalContext,
    ) -> Result<Vec<Document>> {
        self.query_documents_with_principal_cancellable(
            tenant_id,
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            principal,
            &mut || Ok(()),
        )
    }

    /// Lists documents in a logical table asynchronously.
    pub async fn list_documents_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
    ) -> Result<Vec<Document>> {
        self.list_documents_async_with_principal(tenant_id, table, PrincipalContext::anonymous())
            .await
    }

    /// Lists documents in a logical table asynchronously for the provided principal.
    pub async fn list_documents_async_with_principal(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        principal: PrincipalContext,
    ) -> Result<Vec<Document>> {
        self.query_documents_async_cancellable_with_principal(
            tenant_id,
            Query {
                table,
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            principal,
            pending(),
            || Ok(()),
        )
        .await
    }

    /// Lists documents in a logical table asynchronously with cooperative cancellation.
    pub async fn list_documents_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Vec<Document>>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        self.list_documents_async_cancellable_with_principal(
            tenant_id,
            table,
            PrincipalContext::anonymous(),
            cancel_wait,
            check_cancel,
        )
        .await
    }

    /// Lists documents asynchronously for a principal with cooperative cancellation.
    pub async fn list_documents_async_cancellable_with_principal<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        principal: PrincipalContext,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Vec<Document>>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        self.query_documents_async_cancellable_with_principal(
            tenant_id,
            Query {
                table,
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            principal,
            cancel_wait,
            check_cancel,
        )
        .await
    }

    /// Fetches a single document in a logical table.
    pub fn get_document(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        document_id: DocumentId,
    ) -> Result<Document> {
        self.get_document_with_principal(
            tenant_id,
            table,
            document_id,
            &PrincipalContext::anonymous(),
        )
    }

    /// Fetches a single document in a logical table for the provided principal.
    pub fn get_document_with_principal(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        document_id: DocumentId,
        principal: &PrincipalContext,
    ) -> Result<Document> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        wait_for_latest_applied_visibility_blocking(&runtime);
        let _operation = runtime.enter_operation(tenant_id)?;
        let schema = runtime.schema();
        let authorization = ReadAuthorization::for_table(schema.get_table(table), principal)?;
        if authorization.impossible {
            return Err(Error::DocumentNotFound(document_id));
        }

        if let Some(document) = runtime.get_cached_document(table, document_id) {
            if !authorization.allows_document(principal, &document)? {
                return Err(Error::DocumentNotFound(document_id));
            }
            return Ok(document);
        }

        let document = runtime
            .store
            .get(table, &document_id)?
            .ok_or(Error::DocumentNotFound(document_id))?;
        if !authorization.allows_document(principal, &document)? {
            return Err(Error::DocumentNotFound(document_id));
        }
        runtime.cache_document(&document);
        Ok(document)
    }

    /// Fetches a single document in a logical table asynchronously.
    pub async fn get_document_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
    ) -> Result<Document> {
        self.get_document_async_cancellable_with_principal(
            tenant_id,
            table,
            document_id,
            PrincipalContext::anonymous(),
            pending(),
            || Ok(()),
        )
        .await
    }

    /// Fetches a single document asynchronously for the provided principal.
    pub async fn get_document_async_with_principal(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
        principal: PrincipalContext,
    ) -> Result<Document> {
        self.get_document_async_cancellable_with_principal(
            tenant_id,
            table,
            document_id,
            principal,
            pending(),
            || Ok(()),
        )
        .await
    }

    /// Fetches a single document asynchronously with cooperative cancellation.
    pub async fn get_document_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Document>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        self.get_document_async_cancellable_with_principal(
            tenant_id,
            table,
            document_id,
            PrincipalContext::anonymous(),
            cancel_wait,
            check_cancel,
        )
        .await
    }

    /// Fetches a single document asynchronously for the provided principal with cooperative cancellation.
    pub async fn get_document_async_cancellable_with_principal<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
        principal: PrincipalContext,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Document>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let cancel_wait = cancel_wait.shared();
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        runtime
            .wait_for_applied_sequence_cancellable(runtime.durable_head(), cancel_wait.clone())
            .await?;
        let schema = runtime.schema();
        let authorization = ReadAuthorization::for_table(schema.get_table(&table), &principal)?;
        if authorization.impossible {
            return Err(Error::DocumentNotFound(document_id));
        }

        if let Some(document) = runtime.get_cached_document(&table, document_id) {
            if cancel_wait.clone().now_or_never().is_some() {
                return Err(Error::Cancelled);
            }
            check_cancel()?;
            let _operation = runtime.enter_operation(&tenant_id)?;
            if !authorization.allows_document(&principal, &document)? {
                return Err(Error::DocumentNotFound(document_id));
            }
            return Ok(document);
        }

        let runtime_for_task = runtime.clone();
        let tenant_id_for_task = tenant_id.clone();
        let table_for_task = table.clone();
        let document = runtime
            .read_storage
            .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                let _operation = runtime_for_task.enter_operation(&tenant_id_for_task)?;
                check_cancel()?;
                store.get(&table_for_task, &document_id)
            })
            .await?
            .ok_or(Error::DocumentNotFound(document_id))?;
        if !authorization.allows_document(&principal, &document)? {
            return Err(Error::DocumentNotFound(document_id));
        }
        runtime.cache_document(&document);
        Ok(document)
    }

    /// Evaluates a query for a tenant.
    pub fn query_documents(&self, tenant_id: &TenantId, query: &Query) -> Result<Vec<Document>> {
        self.query_documents_with_principal_cancellable(
            tenant_id,
            query,
            &PrincipalContext::anonymous(),
            &mut || Ok(()),
        )
    }

    /// Evaluates a query for a tenant and principal.
    pub fn query_documents_with_principal(
        &self,
        tenant_id: &TenantId,
        query: &Query,
        principal: &PrincipalContext,
    ) -> Result<Vec<Document>> {
        self.query_documents_with_principal_cancellable(tenant_id, query, principal, &mut || Ok(()))
    }

    /// Evaluates a query for a tenant asynchronously.
    pub async fn query_documents_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: Query,
    ) -> Result<Vec<Document>> {
        self.query_documents_async_cancellable(tenant_id, query, pending(), || Ok(()))
            .await
    }

    /// Evaluates a query asynchronously for a specific principal.
    pub async fn query_documents_async_with_principal(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: Query,
        principal: PrincipalContext,
    ) -> Result<Vec<Document>> {
        self.query_documents_async_cancellable_with_principal(
            tenant_id,
            query,
            principal,
            pending(),
            || Ok(()),
        )
        .await
    }

    /// Evaluates a query for a tenant asynchronously with cooperative cancellation.
    pub async fn query_documents_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: Query,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Vec<Document>>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        self.query_documents_async_cancellable_with_principal(
            tenant_id,
            query,
            PrincipalContext::anonymous(),
            cancel_wait,
            check_cancel,
        )
        .await
    }

    /// Evaluates a query asynchronously for a principal with cooperative cancellation.
    pub async fn query_documents_async_cancellable_with_principal<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: Query,
        principal: PrincipalContext,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Vec<Document>>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let cancel_wait = cancel_wait.shared();
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        runtime
            .wait_for_applied_sequence_cancellable(runtime.durable_head(), cancel_wait.clone())
            .await?;
        evaluate_with_index_async_for_principal(
            runtime,
            tenant_id,
            query,
            principal,
            cancel_wait,
            check_cancel,
        )
        .await
    }

    /// Evaluates a query for a tenant while checking for cancellation between rows.
    pub fn query_documents_cancellable(
        &self,
        tenant_id: &TenantId,
        query: &Query,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.query_documents_with_principal_cancellable(
            tenant_id,
            query,
            &PrincipalContext::anonymous(),
            check_cancel,
        )
    }

    /// Evaluates a query for a tenant and principal while checking for cancellation between rows.
    pub fn query_documents_with_principal_cancellable(
        &self,
        tenant_id: &TenantId,
        query: &Query,
        principal: &PrincipalContext,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        wait_for_latest_applied_visibility_blocking(&runtime);
        let _operation = runtime.enter_operation(tenant_id)?;
        evaluate_with_index_cancellable_for_principal(&runtime, query, principal, check_cancel)
    }

    /// Evaluates a paginated query for a tenant.
    pub fn paginate_documents(&self, tenant_id: &TenantId, query: &PaginatedQuery) -> Result<Page> {
        self.paginate_documents_with_principal_cancellable(
            tenant_id,
            query,
            &PrincipalContext::anonymous(),
            &mut || Ok(()),
        )
    }

    /// Evaluates a paginated query for a tenant and principal.
    pub fn paginate_documents_with_principal(
        &self,
        tenant_id: &TenantId,
        query: &PaginatedQuery,
        principal: &PrincipalContext,
    ) -> Result<Page> {
        self.paginate_documents_with_principal_cancellable(tenant_id, query, principal, &mut || {
            Ok(())
        })
    }

    /// Evaluates a paginated query for a tenant asynchronously.
    pub async fn paginate_documents_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: PaginatedQuery,
    ) -> Result<Page> {
        self.paginate_documents_async_cancellable(tenant_id, query, pending(), || Ok(()))
            .await
    }

    /// Evaluates a paginated query asynchronously for a principal.
    pub async fn paginate_documents_async_with_principal(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: PaginatedQuery,
        principal: PrincipalContext,
    ) -> Result<Page> {
        self.paginate_documents_async_cancellable_with_principal(
            tenant_id,
            query,
            principal,
            pending(),
            || Ok(()),
        )
        .await
    }

    /// Evaluates a paginated query for a tenant asynchronously with cooperative cancellation.
    pub async fn paginate_documents_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: PaginatedQuery,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Page>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        self.paginate_documents_async_cancellable_with_principal(
            tenant_id,
            query,
            PrincipalContext::anonymous(),
            cancel_wait,
            check_cancel,
        )
        .await
    }

    /// Evaluates a paginated query asynchronously for a principal with cooperative cancellation.
    pub async fn paginate_documents_async_cancellable_with_principal<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: PaginatedQuery,
        principal: PrincipalContext,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Page>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let cancel_wait = cancel_wait.shared();
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        runtime
            .wait_for_applied_sequence_cancellable(runtime.durable_head(), cancel_wait.clone())
            .await?;
        paginate_with_index_async_for_principal(
            runtime,
            tenant_id,
            query,
            principal,
            cancel_wait,
            check_cancel,
        )
        .await
    }

    /// Evaluates a paginated query for a tenant while checking for cancellation between rows.
    pub fn paginate_documents_cancellable(
        &self,
        tenant_id: &TenantId,
        query: &PaginatedQuery,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Page> {
        self.paginate_documents_with_principal_cancellable(
            tenant_id,
            query,
            &PrincipalContext::anonymous(),
            check_cancel,
        )
    }

    /// Evaluates a paginated query for a tenant and principal while checking cancellation.
    pub fn paginate_documents_with_principal_cancellable(
        &self,
        tenant_id: &TenantId,
        query: &PaginatedQuery,
        principal: &PrincipalContext,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Page> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        wait_for_latest_applied_visibility_blocking(&runtime);
        let _operation = runtime.enter_operation(tenant_id)?;
        let schema = runtime.schema();
        paginate_documents_for_store_and_principal(
            runtime.store.as_ref(),
            query,
            schema.get_table(&query.query.table),
            principal,
            check_cancel,
        )
    }

    /// Reads durable journal records committed after the provided sequence number.
    pub fn read_durable_journal(
        &self,
        tenant_id: &TenantId,
        after: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let from = SequenceNumber(after.0.saturating_add(1));
        runtime.store.read_durable_journal_from(from)
    }

    /// Reads durable journal records asynchronously.
    pub async fn read_durable_journal_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        after: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let tenant_id_for_task = tenant_id.clone();
        let runtime_for_task = runtime.clone();
        runtime
            .read_storage
            .execute(move |store| {
                let _operation = runtime_for_task.enter_operation(&tenant_id_for_task)?;
                let from = SequenceNumber(after.0.saturating_add(1));
                store.read_durable_journal_from(from)
            })
            .await
    }

    /// Streams durable journal records using an ordered sequence cursor.
    pub fn stream_durable_journal(
        &self,
        tenant_id: &TenantId,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.stream_durable_journal(after, limit)
    }

    /// Streams durable journal records asynchronously using an ordered sequence cursor.
    pub async fn stream_durable_journal_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let tenant_id_for_task = tenant_id.clone();
        let runtime_for_task = runtime.clone();
        runtime
            .read_storage
            .execute(move |store| {
                let _operation = runtime_for_task.enter_operation(&tenant_id_for_task)?;
                store.stream_durable_journal(after, limit)
            })
            .await
    }

    /// Exports snapshot metadata for bootstrapping a journal consumer.
    pub fn export_durable_journal_bootstrap(
        &self,
        tenant_id: &TenantId,
    ) -> Result<DurableJournalBootstrap> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.export_durable_journal_bootstrap()
    }

    /// Exports snapshot metadata for bootstrapping a journal consumer asynchronously.
    pub async fn export_durable_journal_bootstrap_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<DurableJournalBootstrap> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let tenant_id_for_task = tenant_id.clone();
        let runtime_for_task = runtime.clone();
        runtime
            .read_storage
            .execute(move |store| {
                let _operation = runtime_for_task.enter_operation(&tenant_id_for_task)?;
                store.export_durable_journal_bootstrap()
            })
            .await
    }

    /// Builds a shadow materializer from the current authoritative journal state.
    pub async fn build_shadow_materializer_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        config: ShadowMaterializerConfig,
    ) -> Result<ShadowMaterializer> {
        let bootstrap = self
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await?;
        let mut after = bootstrap.resume_after;
        let mut tail = Vec::new();
        while after.0 < bootstrap.bootstrap_cut.0 {
            let page = self
                .stream_durable_journal_async(tenant_id.clone(), after, 256)
                .await?;
            let page_records = page
                .records
                .into_iter()
                .take_while(|record| record.sequence.0 <= bootstrap.bootstrap_cut.0)
                .collect::<Vec<_>>();
            let Some(last_record) = page_records.last() else {
                return Err(Error::Internal(format!(
                    "journal stream made no progress while building shadow materializer for tenant {} up to sequence {} from {}",
                    tenant_id, bootstrap.bootstrap_cut.0, after.0
                )));
            };
            after = last_record.sequence;
            tail.extend(page_records);
        }
        ShadowMaterializer::from_checkpoint_and_journal(bootstrap.snapshot, tail, config)
    }

    /// Verifies authoritative and derived tenant state against one bootstrap cut.
    pub async fn verify_consistency_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<ConsistencyVerificationReport> {
        let bootstrap = self
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await?;
        let journal_tail = self
            .read_durable_journal_suffix_to_sequence_async(&tenant_id, &bootstrap)
            .await?;
        let authoritative_snapshot = rebuild_authoritative_snapshot(&bootstrap, &journal_tail)?;

        let shadow = ShadowMaterializer::from_checkpoint_and_journal(
            bootstrap.snapshot.clone(),
            journal_tail.clone(),
            ShadowMaterializerConfig::default(),
        )?;
        let shadow_snapshot = shadow.current_snapshot();

        let replica = EmbeddedReplica::bootstrap_from_bootstrap(
            tenant_id.clone(),
            TenantStore::create_in_memory()?,
            bootstrap.clone(),
            journal_tail,
        )?;
        let replica_snapshot = replica.export_materialized_journal_snapshot()?;

        let mut mismatches = Vec::new();
        if let Some(mismatch) = compare_materialized_journal_snapshots(
            ConsistencyScope::AuthoritativeSnapshot,
            &authoritative_snapshot,
            ConsistencyScope::ShadowMaterializer,
            &shadow_snapshot,
        ) {
            mismatches.push(mismatch);
        }
        if let Some(mismatch) = compare_materialized_journal_snapshots(
            ConsistencyScope::AuthoritativeSnapshot,
            &authoritative_snapshot,
            ConsistencyScope::EmbeddedReplica,
            &replica_snapshot,
        ) {
            mismatches.push(mismatch);
        }
        mismatches.extend(collect_durable_journal_bootstrap_mismatches(
            &bootstrap.snapshot,
            &bootstrap,
        ));

        Ok(ConsistencyVerificationReport {
            tenant_id: tenant_id.to_string(),
            ok: mismatches.is_empty(),
            authoritative: snapshot_fingerprint(&authoritative_snapshot)?,
            shadow: snapshot_fingerprint(&shadow_snapshot)?,
            embedded_replica: snapshot_fingerprint(&replica_snapshot)?,
            bootstrap: bootstrap_fingerprint(&bootstrap)?,
            mismatches,
        })
    }

    /// Reads commit log entries committed after the provided sequence number.
    /// This is a compatibility wrapper over the authoritative durable journal.
    pub fn read_commit_log(
        &self,
        tenant_id: &TenantId,
        after: SequenceNumber,
    ) -> Result<Vec<CommitEntry>> {
        Ok(commit_entries_from_durable_records(
            self.read_durable_journal(tenant_id, after)?,
        ))
    }

    /// Reads commit log entries asynchronously.
    /// This is a compatibility wrapper over the authoritative durable journal.
    pub async fn read_commit_log_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        after: SequenceNumber,
    ) -> Result<Vec<CommitEntry>> {
        Ok(commit_entries_from_durable_records(
            self.read_durable_journal_async(tenant_id, after).await?,
        ))
    }

    /// Returns the latest committed sequence number for a tenant.
    pub fn latest_sequence(&self, tenant_id: &TenantId) -> Result<SequenceNumber> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.latest_sequence()
    }

    /// Returns the latest committed sequence number for a tenant asynchronously.
    pub async fn latest_sequence_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<SequenceNumber> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let tenant_id_for_task = tenant_id.clone();
        let runtime_for_task = runtime.clone();
        runtime
            .read_storage
            .execute(move |store| {
                let _operation = runtime_for_task.enter_operation(&tenant_id_for_task)?;
                store.latest_sequence()
            })
            .await
    }

    #[cfg(test)]
    pub(crate) fn document_cache_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::DocumentCacheStats> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.document_cache_stats())
    }

    #[cfg(test)]
    pub(crate) fn mutation_journal_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::MutationJournalStats> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.mutation_journal_stats())
    }
}

impl Service {
    async fn read_durable_journal_suffix_to_sequence_async(
        self: &Arc<Self>,
        tenant_id: &TenantId,
        bootstrap: &DurableJournalBootstrap,
    ) -> Result<Vec<DurableMutationRecord>> {
        let mut after = bootstrap.resume_after;
        let mut tail = Vec::new();
        while after.0 < bootstrap.bootstrap_cut.0 {
            let page = self
                .stream_durable_journal_async(tenant_id.clone(), after, 256)
                .await?;
            let page_records = page
                .records
                .into_iter()
                .take_while(|record| record.sequence.0 <= bootstrap.bootstrap_cut.0)
                .collect::<Vec<_>>();
            let Some(last_record) = page_records.last() else {
                return Err(Error::Internal(format!(
                    "journal stream made no progress while verifying consistency for tenant {} up to sequence {} from {}",
                    tenant_id, bootstrap.bootstrap_cut.0, after.0
                )));
            };
            after = last_record.sequence;
            tail.extend(page_records);
        }
        Ok(tail)
    }
}

fn commit_entries_from_durable_records(records: Vec<DurableMutationRecord>) -> Vec<CommitEntry> {
    records
        .into_iter()
        .map(|record| record.as_commit_entry())
        .collect()
}

fn rebuild_authoritative_snapshot(
    bootstrap: &DurableJournalBootstrap,
    journal_tail: &[DurableMutationRecord],
) -> Result<crate::MaterializedJournalSnapshot> {
    let store = TenantStore::create_in_memory()?;
    store.rebuild_materialized_journal_from_snapshot(
        &bootstrap.snapshot,
        journal_tail,
        Some(bootstrap.bootstrap_cut),
    )?;
    store.export_materialized_journal_snapshot()
}

pub(crate) fn query_documents_for_store_with_principal(
    store: &TenantStore,
    schema: &Schema,
    query: &Query,
    principal: &PrincipalContext,
) -> Result<Vec<Document>> {
    let mut check_cancel = || Ok(());
    query_documents_for_store_and_principal_cancellable(
        store,
        query,
        schema.get_table(&query.table),
        principal,
        &mut check_cancel,
    )
}

#[cfg(test)]
pub(crate) fn query_documents_for_docs_with_principal(
    documents: Vec<Document>,
    schema: &Schema,
    query: &Query,
    principal: &PrincipalContext,
) -> Result<Vec<Document>> {
    let table_schema = schema.get_table(&query.table);
    let authorization = ReadAuthorization::for_table(table_schema, principal)?;
    if authorization.impossible {
        return Ok(Vec::new());
    }

    let planned_query = authorization.merge_query(query);
    let plan = plan_query(&planned_query, table_schema)?;
    let mut check_cancel = || Ok(());
    let mut include_document =
        |document: &Document| authorization.allows_document(principal, document);
    if let Some(index_docs) = load_query_plan_documents_from_docs(&documents, table_schema, &plan)?
    {
        let residual_query = plan.residual_query(&planned_query);
        evaluate_query_with_docs_cancellable_and_predicate(
            index_docs,
            &residual_query,
            &mut check_cancel,
            &mut include_document,
        )
    } else {
        evaluate_query_with_docs_cancellable_and_predicate(
            documents,
            &planned_query,
            &mut check_cancel,
            &mut include_document,
        )
    }
}

pub(crate) fn paginate_documents_for_store_with_principal(
    store: &TenantStore,
    schema: &Schema,
    query: &PaginatedQuery,
    principal: &PrincipalContext,
) -> Result<Page> {
    let mut check_cancel = || Ok(());
    paginate_documents_for_store_and_principal(
        store,
        query,
        schema.get_table(&query.query.table),
        principal,
        &mut check_cancel,
    )
}

#[cfg(test)]
pub(crate) fn paginate_documents_for_docs_with_principal(
    documents: Vec<Document>,
    schema: &Schema,
    query: &PaginatedQuery,
    principal: &PrincipalContext,
) -> Result<Page> {
    let table_schema = schema.get_table(&query.query.table);
    let authorization = ReadAuthorization::for_table(table_schema, principal)?;
    if authorization.impossible {
        return Ok(Page {
            data: Vec::new(),
            next_cursor: None,
            has_more: false,
        });
    }

    let planned_paginated = PaginatedQuery {
        query: authorization.merge_query(&query.query),
        page_size: query.page_size,
        after: query.after.clone(),
    };
    let plan = plan_query(&planned_paginated.query, table_schema)?;
    let mut check_cancel = || Ok(());
    let mut include_document =
        |document: &Document| authorization.allows_document(principal, document);
    if let Some(index_docs) = load_query_plan_documents_from_docs(&documents, table_schema, &plan)?
    {
        let residual_paginated = PaginatedQuery {
            query: plan.residual_query(&planned_paginated.query),
            page_size: planned_paginated.page_size,
            after: planned_paginated.after.clone(),
        };
        evaluate_paginated_with_docs_cancellable_and_predicate(
            index_docs,
            &residual_paginated,
            &mut check_cancel,
            &mut include_document,
        )
    } else {
        evaluate_paginated_with_docs_cancellable_and_predicate(
            documents,
            &planned_paginated,
            &mut check_cancel,
            &mut include_document,
        )
    }
}

fn wait_for_latest_applied_visibility_blocking(runtime: &TenantRuntime) {
    runtime.wait_for_applied_sequence_blocking(runtime.durable_head());
}

pub(super) fn evaluate_with_index_cancellable_for_principal(
    runtime: &TenantRuntime,
    query: &Query,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let schema = runtime.schema();
    let documents = query_documents_for_store_and_principal_cancellable(
        runtime.store.as_ref(),
        query,
        schema.get_table(&query.table),
        principal,
        check_cancel,
    )?;
    runtime.cache_documents(&documents);
    Ok(documents)
}

async fn evaluate_with_index_async_for_principal<Fut, Check>(
    runtime: Arc<TenantRuntime>,
    tenant_id: TenantId,
    query: Query,
    principal: PrincipalContext,
    cancel_wait: Fut,
    check_cancel: Check,
) -> Result<Vec<Document>>
where
    Fut: Future<Output = ()> + Send,
    Check: Fn() -> Result<()> + Send + 'static,
{
    let schema = runtime.schema();
    let table_schema = schema.get_table(&query.table).cloned();
    let runtime_for_task = runtime.clone();
    let tenant_id_for_task = tenant_id.clone();
    let query_for_task = query.clone();
    let principal_for_task = principal.clone();
    let documents = runtime
        .read_storage
        .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
            let _operation = runtime_for_task.enter_operation(&tenant_id_for_task)?;
            query_documents_for_store_and_principal_cancellable(
                store.as_ref(),
                &query_for_task,
                table_schema.as_ref(),
                &principal_for_task,
                check_cancel,
            )
        })
        .await?;
    runtime.cache_documents(&documents);
    Ok(documents)
}

async fn paginate_with_index_async_for_principal<Fut, Check>(
    runtime: Arc<TenantRuntime>,
    tenant_id: TenantId,
    query: PaginatedQuery,
    principal: PrincipalContext,
    cancel_wait: Fut,
    check_cancel: Check,
) -> Result<Page>
where
    Fut: Future<Output = ()> + Send,
    Check: Fn() -> Result<()> + Send + 'static,
{
    let schema = runtime.schema();
    let table_schema = schema.get_table(&query.query.table).cloned();
    let runtime_for_task = runtime.clone();
    let tenant_id_for_task = tenant_id.clone();
    let query_for_task = query.clone();
    let principal_for_task = principal.clone();
    runtime
        .read_storage
        .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
            let _operation = runtime_for_task.enter_operation(&tenant_id_for_task)?;
            paginate_documents_for_store_and_principal(
                store.as_ref(),
                &query_for_task,
                table_schema.as_ref(),
                &principal_for_task,
                check_cancel,
            )
        })
        .await
}

fn query_documents_for_store_and_principal_cancellable(
    store: &TenantStore,
    query: &Query,
    table_schema: Option<&TableSchema>,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let authorization = ReadAuthorization::for_table(table_schema, principal)?;
    if authorization.impossible {
        return Ok(Vec::new());
    }

    let planned_query = authorization.merge_query(query);
    let plan = plan_query(&planned_query, table_schema)?;
    let mut include_document =
        |document: &Document| authorization.allows_document(principal, document);
    if let Some(documents) =
        load_query_plan_documents_cancellable(store, &planned_query, &plan, check_cancel)?
    {
        let residual_query = plan.residual_query(&planned_query);
        evaluate_query_with_docs_cancellable_and_predicate(
            documents,
            &residual_query,
            check_cancel,
            &mut include_document,
        )
    } else {
        evaluate_query_cancellable_with_predicate(
            store,
            &planned_query,
            check_cancel,
            &mut include_document,
        )
    }
}

fn paginate_documents_for_store_and_principal(
    store: &TenantStore,
    query: &PaginatedQuery,
    table_schema: Option<&TableSchema>,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Page> {
    let authorization = ReadAuthorization::for_table(table_schema, principal)?;
    if authorization.impossible {
        return Ok(Page {
            data: Vec::new(),
            next_cursor: None,
            has_more: false,
        });
    }

    let planned_query = authorization.merge_query(&query.query);
    let planned_paginated = PaginatedQuery {
        query: planned_query.clone(),
        page_size: query.page_size,
        after: query.after.clone(),
    };
    let plan = plan_query(&planned_paginated.query, table_schema)?;
    let mut include_document =
        |document: &Document| authorization.allows_document(principal, document);
    if let Some(index_docs) =
        load_query_plan_documents_cancellable(store, &planned_paginated.query, &plan, check_cancel)?
    {
        let residual_paginated = PaginatedQuery {
            query: plan.residual_query(&planned_paginated.query),
            page_size: planned_paginated.page_size,
            after: planned_paginated.after.clone(),
        };
        evaluate_paginated_with_docs_cancellable_and_predicate(
            index_docs,
            &residual_paginated,
            check_cancel,
            &mut include_document,
        )
    } else {
        evaluate_paginated_cancellable_with_predicate(
            store,
            &planned_paginated,
            check_cancel,
            &mut include_document,
        )
    }
}

pub(super) fn table_policy_revision(table_schema: Option<&TableSchema>) -> Result<String> {
    match table_schema {
        Some(table_schema) => table_schema.access_policy_revision(),
        None => policy_revision_id(None),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ReadAuthorization {
    rule: Option<AccessRule>,
    planner_filters: Vec<Filter>,
    pub(crate) impossible: bool,
}

impl ReadAuthorization {
    pub(crate) fn for_table(
        table_schema: Option<&TableSchema>,
        principal: &PrincipalContext,
    ) -> Result<Self> {
        let rule = table_schema
            .and_then(|table_schema| table_schema.access_policy.as_ref())
            .map(|policy| policy.rule_for(AccessAction::Read).clone())
            .filter(|rule| !rule.is_unrestricted());
        let Some(rule) = rule else {
            return Ok(Self {
                rule: None,
                planner_filters: Vec::new(),
                impossible: false,
            });
        };

        let compiled = rule.compile_read_filters(principal)?;
        Ok(Self {
            rule: Some(rule),
            planner_filters: compiled.planner_filters,
            impossible: compiled.impossible,
        })
    }

    pub(crate) fn merge_query(&self, query: &Query) -> Query {
        if self.planner_filters.is_empty() {
            return query.clone();
        }

        let mut merged = query.clone();
        merged.filters.extend(self.planner_filters.clone());
        merged
    }

    pub(crate) fn allows_document(
        &self,
        principal: &PrincipalContext,
        document: &Document,
    ) -> Result<bool> {
        match &self.rule {
            Some(rule) => rule.allows(principal, Some(document), None),
            None => Ok(true),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum QueryPlan {
    FullScan,
    ExactIndex {
        index_name: String,
        value: Value,
        residual_filters: Vec<Filter>,
    },
    RangeIndex {
        index_name: String,
        lower: Option<PlannedRangeBound>,
        upper: Option<PlannedRangeBound>,
        residual_filters: Vec<Filter>,
    },
}

impl QueryPlan {
    fn residual_query(&self, query: &Query) -> Query {
        match self {
            Self::FullScan => query.clone(),
            Self::ExactIndex {
                residual_filters, ..
            }
            | Self::RangeIndex {
                residual_filters, ..
            } => {
                let mut residual_query = query.clone();
                residual_query.filters = residual_filters.clone();
                residual_query
            }
        }
    }
}

fn plan_query(query: &Query, table_schema: Option<&neovex_core::TableSchema>) -> Result<QueryPlan> {
    let Some(table_schema) = table_schema else {
        return Ok(QueryPlan::FullScan);
    };

    if let Some(plan) = plan_exact_index_scan(query, table_schema) {
        return Ok(plan);
    }

    if let Some(plan) = plan_range_index_scan(query, table_schema)? {
        return Ok(plan);
    }

    Ok(QueryPlan::FullScan)
}

fn plan_exact_index_scan(
    query: &Query,
    table_schema: &neovex_core::TableSchema,
) -> Option<QueryPlan> {
    for filter in &query.filters {
        if filter.op == FilterOp::Eq
            && is_scalar_index_value(&filter.value)
            && let Some(index) = table_schema
                .indexes
                .iter()
                .find(|index| index.field == filter.field)
        {
            let residual_filters = query
                .filters
                .iter()
                .filter(|candidate| !matches_eq_filter(candidate, filter))
                .cloned()
                .collect();
            return Some(QueryPlan::ExactIndex {
                index_name: index.name.clone(),
                value: filter.value.clone(),
                residual_filters,
            });
        }
    }

    None
}

fn plan_range_index_scan(
    query: &Query,
    table_schema: &neovex_core::TableSchema,
) -> Result<Option<QueryPlan>> {
    for index in &table_schema.indexes {
        let mut kind = None;
        let mut lower = None;
        let mut upper = None;
        let mut unusable = false;

        for filter in query
            .filters
            .iter()
            .filter(|filter| filter.field == index.field)
        {
            let Some(bound) = range_bound_from_filter(filter)? else {
                continue;
            };

            if let Some(existing_kind) = kind {
                if existing_kind != bound.kind {
                    unusable = true;
                    break;
                }
            } else {
                kind = Some(bound.kind);
            }

            match bound.side {
                RangeSide::Lower => update_lower_bound(&mut lower, bound),
                RangeSide::Upper => update_upper_bound(&mut upper, bound),
            }
        }

        if unusable || (lower.is_none() && upper.is_none()) {
            continue;
        }

        return Ok(Some(QueryPlan::RangeIndex {
            index_name: index.name.clone(),
            lower,
            upper,
            residual_filters: query.filters.clone(),
        }));
    }

    Ok(None)
}

fn load_query_plan_documents_cancellable(
    store: &TenantStore,
    query: &Query,
    plan: &QueryPlan,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Option<Vec<Document>>> {
    match plan {
        QueryPlan::FullScan => Ok(None),
        QueryPlan::ExactIndex {
            index_name, value, ..
        } => Ok(Some(store.index_scan_eq_cancellable(
            &query.table,
            index_name,
            value,
            check_cancel,
        )?)),
        QueryPlan::RangeIndex {
            index_name,
            lower,
            upper,
            ..
        } => Ok(Some(store.index_scan_range_cancellable(
            &query.table,
            index_name,
            lower.as_ref().map(|bound| &bound.value),
            upper.as_ref().map(|bound| &bound.value),
            lower.as_ref().is_none_or(|bound| bound.inclusive),
            upper.as_ref().is_none_or(|bound| bound.inclusive),
            check_cancel,
        )?)),
    }
}

#[cfg(test)]
fn load_query_plan_documents_from_docs(
    documents: &[Document],
    table_schema: Option<&TableSchema>,
    plan: &QueryPlan,
) -> Result<Option<Vec<Document>>> {
    let Some(table_schema) = table_schema else {
        return Ok(None);
    };

    let filtered = match plan {
        QueryPlan::FullScan => return Ok(None),
        QueryPlan::ExactIndex {
            index_name, value, ..
        } => {
            let field = index_field_for_plan(table_schema, index_name)?;
            documents
                .iter()
                .filter(|document| document.get_field(field) == Some(value))
                .cloned()
                .collect()
        }
        QueryPlan::RangeIndex {
            index_name,
            lower,
            upper,
            ..
        } => {
            let field = index_field_for_plan(table_schema, index_name)?;
            let mut filtered = Vec::new();
            for document in documents {
                if document_matches_range_bounds(document, field, lower.as_ref(), upper.as_ref())? {
                    filtered.push(document.clone());
                }
            }
            filtered
        }
    };
    Ok(Some(filtered))
}

#[cfg(test)]
fn index_field_for_plan<'a>(table_schema: &'a TableSchema, index_name: &str) -> Result<&'a str> {
    table_schema
        .indexes
        .iter()
        .find(|index| index.name == index_name)
        .map(|index| index.field.as_str())
        .ok_or_else(|| {
            Error::Internal(format!("query plan referenced missing index: {index_name}"))
        })
}

#[cfg(test)]
fn document_matches_range_bounds(
    document: &Document,
    field: &str,
    lower: Option<&PlannedRangeBound>,
    upper: Option<&PlannedRangeBound>,
) -> Result<bool> {
    let mut filters = Vec::new();
    if let Some(lower) = lower {
        filters.push(Filter {
            field: field.to_string(),
            op: if lower.inclusive {
                FilterOp::Gte
            } else {
                FilterOp::Gt
            },
            value: lower.value.clone(),
        });
    }
    if let Some(upper) = upper {
        filters.push(Filter {
            field: field.to_string(),
            op: if upper.inclusive {
                FilterOp::Lte
            } else {
                FilterOp::Lt
            },
            value: upper.value.clone(),
        });
    }
    matches_filters(document, &filters)
}

fn is_scalar_index_value(value: &Value) -> bool {
    value.is_null() || value.is_boolean() || value.is_number() || value.is_string()
}

fn range_bound_from_filter(filter: &Filter) -> Result<Option<PlannedRangeBound>> {
    let (side, inclusive) = match filter.op {
        FilterOp::Gt => (RangeSide::Lower, false),
        FilterOp::Gte => (RangeSide::Lower, true),
        FilterOp::Lt => (RangeSide::Upper, false),
        FilterOp::Lte => (RangeSide::Upper, true),
        FilterOp::Eq | FilterOp::Neq => return Ok(None),
    };

    let kind = match &filter.value {
        Value::Number(number) if number.as_f64().is_some() => RangeValueKind::Number,
        Value::String(_) => RangeValueKind::String,
        _ => return Ok(None),
    };

    Ok(Some(PlannedRangeBound {
        value: filter.value.clone(),
        encoded: encode_index_value(&filter.value)?,
        inclusive,
        kind,
        side,
    }))
}

fn update_lower_bound(current: &mut Option<PlannedRangeBound>, candidate: PlannedRangeBound) {
    match current {
        Some(existing)
            if compare_lower_bounds(candidate.as_ref(), existing.as_ref()) == Ordering::Greater =>
        {
            *current = Some(candidate);
        }
        None => *current = Some(candidate),
        Some(_) => {}
    }
}

fn update_upper_bound(current: &mut Option<PlannedRangeBound>, candidate: PlannedRangeBound) {
    match current {
        Some(existing)
            if compare_upper_bounds(candidate.as_ref(), existing.as_ref()) == Ordering::Less =>
        {
            *current = Some(candidate);
        }
        None => *current = Some(candidate),
        Some(_) => {}
    }
}

fn compare_lower_bounds(
    left: PlannedRangeBoundRef<'_>,
    right: PlannedRangeBoundRef<'_>,
) -> Ordering {
    left.encoded
        .cmp(right.encoded)
        .then_with(|| match (left.inclusive, right.inclusive) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            _ => Ordering::Equal,
        })
}

fn compare_upper_bounds(
    left: PlannedRangeBoundRef<'_>,
    right: PlannedRangeBoundRef<'_>,
) -> Ordering {
    left.encoded
        .cmp(right.encoded)
        .then_with(|| match (left.inclusive, right.inclusive) {
            (false, true) => Ordering::Less,
            (true, false) => Ordering::Greater,
            _ => Ordering::Equal,
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeValueKind {
    Number,
    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeSide {
    Lower,
    Upper,
}

#[derive(Debug, Clone, PartialEq)]
struct PlannedRangeBound {
    value: Value,
    encoded: Vec<u8>,
    inclusive: bool,
    kind: RangeValueKind,
    side: RangeSide,
}

impl PlannedRangeBound {
    fn as_ref(&self) -> PlannedRangeBoundRef<'_> {
        PlannedRangeBoundRef {
            encoded: &self.encoded,
            inclusive: self.inclusive,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PlannedRangeBoundRef<'a> {
    encoded: &'a [u8],
    inclusive: bool,
}

fn matches_eq_filter(candidate: &Filter, satisfied: &Filter) -> bool {
    candidate.op == FilterOp::Eq
        && satisfied.op == FilterOp::Eq
        && candidate.field == satisfied.field
        && candidate.value == satisfied.value
}

#[cfg(test)]
mod tests {
    use neovex_core::{FilterOp, IndexDefinition, Query, TableName, TableSchema};
    use serde_json::json;

    use super::*;

    fn tasks_table() -> TableName {
        TableName::new("tasks").expect("table name should be valid")
    }

    fn filter(field: &str, op: FilterOp, value: Value) -> Filter {
        Filter {
            field: field.to_string(),
            op,
            value,
        }
    }

    fn schema_with_indexes(indexes: &[(&str, &str)]) -> TableSchema {
        TableSchema {
            table: tasks_table(),
            fields: Vec::new(),
            indexes: indexes
                .iter()
                .map(|(name, field)| IndexDefinition {
                    name: (*name).to_string(),
                    field: (*field).to_string(),
                })
                .collect(),
            access_policy: None,
        }
    }

    #[test]
    fn plan_query_returns_full_scan_without_a_usable_index() {
        let query = Query {
            table: tasks_table(),
            filters: vec![filter("status", FilterOp::Eq, json!(["active"]))],
            order: None,
            limit: None,
        };
        let plan = plan_query(
            &query,
            Some(&schema_with_indexes(&[("by_status", "status")])),
        )
        .expect("planning should succeed");

        assert!(matches!(plan, QueryPlan::FullScan));
    }

    #[test]
    fn plan_query_selects_exact_index_scan_and_retains_residual_filters() {
        let query = Query {
            table: tasks_table(),
            filters: vec![
                filter("status", FilterOp::Eq, json!("active")),
                filter("rank", FilterOp::Gte, json!(2)),
            ],
            order: None,
            limit: None,
        };
        let plan = plan_query(
            &query,
            Some(&schema_with_indexes(&[
                ("by_status", "status"),
                ("by_rank", "rank"),
            ])),
        )
        .expect("planning should succeed");

        match &plan {
            QueryPlan::ExactIndex {
                index_name,
                value,
                residual_filters,
            } => {
                assert_eq!(index_name, "by_status");
                assert_eq!(value, &json!("active"));
                assert_eq!(
                    residual_filters,
                    &vec![filter("rank", FilterOp::Gte, json!(2))]
                );
            }
            other => panic!("unexpected plan: {other:?}"),
        }

        let residual_query = plan.residual_query(&query);
        assert_eq!(
            residual_query.filters,
            vec![filter("rank", FilterOp::Gte, json!(2))]
        );
    }

    #[test]
    fn plan_query_selects_range_index_scan_when_no_exact_index_matches() {
        let query = Query {
            table: tasks_table(),
            filters: vec![
                filter("rank", FilterOp::Gte, json!(2)),
                filter("rank", FilterOp::Lt, json!(10)),
                filter("status", FilterOp::Eq, json!("active")),
            ],
            order: None,
            limit: None,
        };
        let plan = plan_query(&query, Some(&schema_with_indexes(&[("by_rank", "rank")])))
            .expect("planning should succeed");

        match &plan {
            QueryPlan::RangeIndex {
                index_name,
                lower,
                upper,
                residual_filters,
            } => {
                assert_eq!(index_name, "by_rank");
                assert_eq!(lower.as_ref().map(|bound| &bound.value), Some(&json!(2)));
                assert_eq!(upper.as_ref().map(|bound| &bound.value), Some(&json!(10)));
                assert_eq!(residual_filters, &query.filters);
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }
}
