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
    TenantReadSnapshot, TenantReadStorage, TenantStore,
};
use serde_json::Value;

use crate::EmbeddedReplica;
use crate::evaluator::matches_filters;
use crate::evaluator::{
    evaluate_paginated_cancellable_with_predicate,
    evaluate_paginated_with_docs_cancellable_and_predicate,
    evaluate_query_cancellable_with_predicate, evaluate_query_with_docs_cancellable_and_predicate,
};
use crate::tenant::{QueryPlanMetricKind, QueryPlanMetricOperation, TenantRuntime};
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
        let required_sequence = wait_for_latest_applied_visibility_blocking(&runtime);
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

        if let Some(snapshot) =
            runtime.materialized_serving_snapshot_for_table(table, required_sequence)
        {
            debug_assert!(snapshot.covered_sequence().0 >= required_sequence.0);
            if let Some(document) = snapshot.document(table, document_id) {
                if !authorization.allows_document(principal, &document)? {
                    return Err(Error::DocumentNotFound(document_id));
                }
                runtime.record_materialized_get_hit();
                runtime.cache_document(&document);
                return Ok(document);
            }
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
        let required_sequence = runtime.durable_head();
        runtime
            .wait_for_applied_sequence_cancellable(required_sequence, cancel_wait.clone())
            .await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
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
            if !authorization.allows_document(&principal, &document)? {
                return Err(Error::DocumentNotFound(document_id));
            }
            return Ok(document);
        }

        if let Some(snapshot) =
            runtime.materialized_serving_snapshot_for_table(&table, required_sequence)
        {
            debug_assert!(snapshot.covered_sequence().0 >= required_sequence.0);
            if let Some(document) = snapshot.document(&table, document_id) {
                if cancel_wait.clone().now_or_never().is_some() {
                    return Err(Error::Cancelled);
                }
                check_cancel()?;
                if !authorization.allows_document(&principal, &document)? {
                    return Err(Error::DocumentNotFound(document_id));
                }
                runtime.record_materialized_get_hit();
                runtime.cache_document(&document);
                return Ok(document);
            }
        }

        let table_for_task = table.clone();
        let document = runtime
            .read_storage
            .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
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
        let required_sequence = runtime.durable_head();
        runtime
            .wait_for_applied_sequence_cancellable(required_sequence, cancel_wait.clone())
            .await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        let schema = runtime.schema();
        match prepare_query_execution(schema.get_table(&query.table), &query, &principal)? {
            None => Ok(Vec::new()),
            Some(prepared) if matches!(prepared.plan, QueryPlan::FullScan) => {
                let (plan_kind, documents) = evaluate_with_materialized_surface_async_prepared(
                    runtime.clone(),
                    &query,
                    prepared,
                    principal.clone(),
                    required_sequence,
                    cancel_wait.clone(),
                    check_cancel,
                )
                .await?;
                runtime.record_query_plan_metric(QueryPlanMetricOperation::Query, plan_kind);
                runtime.cache_documents(&documents);
                Ok(documents)
            }
            Some(prepared) => {
                let (plan_kind, documents) = evaluate_with_index_async_prepared(
                    runtime.clone(),
                    prepared,
                    principal,
                    cancel_wait,
                    check_cancel,
                )
                .await?;
                runtime.record_query_plan_metric(QueryPlanMetricOperation::Query, plan_kind);
                runtime.cache_documents(&documents);
                Ok(documents)
            }
        }
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
        let required_sequence = wait_for_latest_applied_visibility_blocking(&runtime);
        let _operation = runtime.enter_operation(tenant_id)?;
        let schema = runtime.schema();
        match prepare_query_execution(schema.get_table(&query.table), query, principal)? {
            None => Ok(Vec::new()),
            Some(prepared) if matches!(prepared.plan, QueryPlan::FullScan) => {
                let (plan_kind, documents) =
                    evaluate_with_materialized_surface_cancellable_prepared(
                        &runtime,
                        query,
                        &prepared,
                        principal,
                        required_sequence,
                        QueryPlanMetricOperation::Query,
                        check_cancel,
                    )?;
                runtime.record_query_plan_metric(QueryPlanMetricOperation::Query, plan_kind);
                runtime.cache_documents(&documents);
                Ok(documents)
            }
            Some(prepared) => {
                let plan_kind = query_plan_metric_kind(&prepared.plan);
                let documents = query_documents_for_store_prepared_cancellable(
                    runtime.store.as_ref(),
                    &prepared,
                    principal,
                    check_cancel,
                )?;
                runtime.record_query_plan_metric(QueryPlanMetricOperation::Query, plan_kind);
                runtime.cache_documents(&documents);
                Ok(documents)
            }
        }
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
        let required_sequence = runtime.durable_head();
        runtime
            .wait_for_applied_sequence_cancellable(required_sequence, cancel_wait.clone())
            .await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        let schema = runtime.schema();
        match prepare_paginated_execution(schema.get_table(&query.query.table), &query, &principal)?
        {
            None => Ok(Page {
                data: Vec::new(),
                next_cursor: None,
                has_more: false,
            }),
            Some(prepared) if matches!(prepared.plan, QueryPlan::FullScan) => {
                let (plan_kind, page) = paginate_with_materialized_surface_async_prepared(
                    runtime.clone(),
                    &query,
                    prepared,
                    principal.clone(),
                    required_sequence,
                    cancel_wait.clone(),
                    check_cancel,
                )
                .await?;
                runtime.record_query_plan_metric(QueryPlanMetricOperation::Paginated, plan_kind);
                Ok(page)
            }
            Some(prepared) => {
                let (plan_kind, page) = paginate_with_index_async_prepared(
                    runtime.clone(),
                    prepared,
                    principal,
                    cancel_wait,
                    check_cancel,
                )
                .await?;
                runtime.record_query_plan_metric(QueryPlanMetricOperation::Paginated, plan_kind);
                Ok(page)
            }
        }
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
        let required_sequence = wait_for_latest_applied_visibility_blocking(&runtime);
        let _operation = runtime.enter_operation(tenant_id)?;
        let schema = runtime.schema();
        match prepare_paginated_execution(schema.get_table(&query.query.table), query, principal)? {
            None => Ok(Page {
                data: Vec::new(),
                next_cursor: None,
                has_more: false,
            }),
            Some(prepared) if matches!(prepared.plan, QueryPlan::FullScan) => {
                let (plan_kind, page) = paginate_with_materialized_surface_cancellable_prepared(
                    &runtime,
                    query,
                    &prepared,
                    principal,
                    required_sequence,
                    check_cancel,
                )?;
                runtime.record_query_plan_metric(QueryPlanMetricOperation::Paginated, plan_kind);
                Ok(page)
            }
            Some(prepared) => {
                let plan_kind = query_plan_metric_kind(&prepared.plan);
                let page = paginate_documents_for_store_prepared_cancellable(
                    runtime.store.as_ref(),
                    &prepared,
                    principal,
                    check_cancel,
                )?;
                runtime.record_query_plan_metric(QueryPlanMetricOperation::Paginated, plan_kind);
                Ok(page)
            }
        }
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

    #[cfg(test)]
    pub(crate) fn mutation_admission_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::MutationAdmissionStats> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.mutation_admission_stats())
    }

    #[cfg(test)]
    pub(crate) fn subscription_delivery_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::SubscriptionDeliveryStats> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.subscription_delivery_stats())
    }

    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn query_planning_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::QueryPlanningStats> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.query_planning_stats())
    }

    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn materialized_read_surface_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::MaterializedReadSurfaceStats> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.materialized_read_surface_stats())
    }

    #[cfg(test)]
    pub(crate) fn materialized_table_publication_stats_for_testing(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
    ) -> Result<Option<crate::tenant::MaterializedTablePublicationStats>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.materialized_table_publication_stats(table))
    }

    #[cfg(test)]
    pub(crate) fn materialized_serving_snapshot_for_testing(
        &self,
        tenant_id: &TenantId,
        required_sequence: SequenceNumber,
    ) -> Result<Option<crate::tenant::ServingSnapshot>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.materialized_serving_snapshot_for_testing(required_sequence))
    }

    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn serving_snapshot_manager_stats_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::ServingSnapshotManagerStats> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.serving_snapshot_manager_stats())
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_materialized_serving_snapshot_for_testing<Fut>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        required_sequence: SequenceNumber,
        cancel_wait: Fut,
    ) -> Result<crate::tenant::ServingSnapshot>
    where
        Fut: Future<Output = ()> + Send,
    {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        runtime
            .wait_for_materialized_serving_snapshot_cancellable(required_sequence, cancel_wait)
            .await
    }

    #[cfg(test)]
    pub(crate) fn set_subscription_delivery_queue_capacity_for_testing(
        &self,
        tenant_id: &TenantId,
        capacity: usize,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.set_subscription_delivery_queue_capacity_for_testing(capacity);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_mutation_journal_queue_capacity_for_testing(
        &self,
        tenant_id: &TenantId,
        capacity: usize,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.set_mutation_journal_queue_capacity_for_testing(capacity);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_mutation_admission_codel_for_testing(
        &self,
        tenant_id: &TenantId,
        target: std::time::Duration,
        interval: std::time::Duration,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.set_mutation_admission_codel_for_testing(target, interval);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_materialized_read_surface_limits_for_testing(
        &self,
        tenant_id: &TenantId,
        table_capacity: usize,
        byte_capacity: usize,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.set_materialized_read_surface_limits_for_testing(table_capacity, byte_capacity);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_materialized_read_surface_version_capacity_for_testing(
        &self,
        tenant_id: &TenantId,
        version_capacity: usize,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.set_materialized_read_surface_version_capacity_for_testing(version_capacity);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn subscription_delivery_pause_handle_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::SubscriptionDeliveryPauseHandle> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.subscription_delivery_pause_handle_for_testing())
    }

    #[cfg(test)]
    pub(crate) fn mutation_journal_pause_handle_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::MutationJournalPauseHandle> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.mutation_journal_pause_handle_for_testing())
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn subscription_bootstrap_pause_handle_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::MutationJournalPauseHandle> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.subscription_bootstrap_pause_handle_for_testing())
    }

    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn arm_subscription_bootstrap_pause_for_testing(&self, tenant_id: &TenantId) -> Result<()> {
        let pause = self.subscription_bootstrap_pause_handle_for_testing(tenant_id)?;
        pause.arm();
        Ok(())
    }

    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn wait_for_subscription_bootstrap_pause_for_testing(
        &self,
        tenant_id: &TenantId,
        timeout: std::time::Duration,
    ) -> Result<bool> {
        let pause = self.subscription_bootstrap_pause_handle_for_testing(tenant_id)?;
        Ok(pause.wait_until_entered(timeout))
    }

    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn release_subscription_bootstrap_pause_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<()> {
        let pause = self.subscription_bootstrap_pause_handle_for_testing(tenant_id)?;
        pause.release();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn materialized_read_publish_pause_handle_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Result<crate::tenant::MaterializedReadPublishPauseHandle> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.materialized_read_publish_pause_handle_for_testing())
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
    let (_, documents) = query_documents_for_store_and_principal_cancellable(
        store,
        query,
        schema.get_table(&query.table),
        principal,
        &mut check_cancel,
    )?;
    Ok(documents)
}

#[derive(Debug, Clone)]
struct PreparedQueryExecution {
    authorization: ReadAuthorization,
    planned_query: Query,
    plan: QueryPlan,
}

#[derive(Debug, Clone)]
struct PreparedPaginatedExecution {
    authorization: ReadAuthorization,
    planned_paginated: PaginatedQuery,
    plan: QueryPlan,
}

fn prepare_query_execution(
    table_schema: Option<&TableSchema>,
    query: &Query,
    principal: &PrincipalContext,
) -> Result<Option<PreparedQueryExecution>> {
    let authorization = ReadAuthorization::for_table(table_schema, principal)?;
    if authorization.impossible {
        return Ok(None);
    }
    let planned_query = authorization.merge_query(query);
    let plan = plan_query(&planned_query, table_schema)?;
    Ok(Some(PreparedQueryExecution {
        authorization,
        planned_query,
        plan,
    }))
}

fn prepare_paginated_execution(
    table_schema: Option<&TableSchema>,
    query: &PaginatedQuery,
    principal: &PrincipalContext,
) -> Result<Option<PreparedPaginatedExecution>> {
    let authorization = ReadAuthorization::for_table(table_schema, principal)?;
    if authorization.impossible {
        return Ok(None);
    }
    let planned_paginated = PaginatedQuery {
        query: authorization.merge_query(&query.query),
        page_size: query.page_size,
        after: query.after.clone(),
    };
    let plan = plan_paginated_query(&planned_paginated.query, table_schema)?;
    Ok(Some(PreparedPaginatedExecution {
        authorization,
        planned_paginated,
        plan,
    }))
}

fn snapshot_table_documents(
    snapshot: &crate::tenant::ServingSnapshot,
    table: &TableName,
    context: &str,
) -> Result<Vec<Document>> {
    snapshot.table_documents(table).ok_or_else(|| {
        Error::Internal(format!(
            "materialized serving snapshot missing table {table} during {context}"
        ))
    })
}

fn query_documents_for_docs_prepared(
    documents: Vec<Document>,
    prepared: &PreparedQueryExecution,
    principal: &PrincipalContext,
) -> Result<Vec<Document>> {
    let mut check_cancel = || Ok(());
    let mut include_document =
        |document: &Document| prepared.authorization.allows_document(principal, document);
    if let Some(index_docs) = load_query_plan_documents_from_docs(&documents, &prepared.plan)? {
        let residual_query = prepared.plan.residual_query(&prepared.planned_query);
        evaluate_query_with_docs_cancellable_and_predicate(
            index_docs,
            &residual_query,
            &mut check_cancel,
            &mut include_document,
        )
    } else {
        evaluate_query_with_docs_cancellable_and_predicate(
            documents,
            &prepared.planned_query,
            &mut check_cancel,
            &mut include_document,
        )
    }
}

fn paginate_documents_for_docs_prepared(
    documents: Vec<Document>,
    prepared: &PreparedPaginatedExecution,
    principal: &PrincipalContext,
) -> Result<Page> {
    let mut check_cancel = || Ok(());
    let mut include_document =
        |document: &Document| prepared.authorization.allows_document(principal, document);
    if let Some(index_docs) = load_query_plan_documents_from_docs(&documents, &prepared.plan)? {
        let residual_paginated = PaginatedQuery {
            query: prepared
                .plan
                .residual_query(&prepared.planned_paginated.query),
            page_size: prepared.planned_paginated.page_size,
            after: prepared.planned_paginated.after.clone(),
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
            &prepared.planned_paginated,
            &mut check_cancel,
            &mut include_document,
        )
    }
}

pub(crate) fn query_documents_for_docs_with_principal(
    documents: Vec<Document>,
    schema: &Schema,
    query: &Query,
    principal: &PrincipalContext,
) -> Result<Vec<Document>> {
    match prepare_query_execution(schema.get_table(&query.table), query, principal)? {
        None => Ok(Vec::new()),
        Some(prepared) => query_documents_for_docs_prepared(documents, &prepared, principal),
    }
}

pub(crate) fn paginate_documents_for_store_with_principal(
    store: &TenantStore,
    schema: &Schema,
    query: &PaginatedQuery,
    principal: &PrincipalContext,
) -> Result<Page> {
    let mut check_cancel = || Ok(());
    let (_, page) = paginate_documents_for_store_and_principal(
        store,
        query,
        schema.get_table(&query.query.table),
        principal,
        &mut check_cancel,
    )?;
    Ok(page)
}

#[cfg(test)]
pub(crate) fn paginate_documents_for_docs_with_principal(
    documents: Vec<Document>,
    schema: &Schema,
    query: &PaginatedQuery,
    principal: &PrincipalContext,
) -> Result<Page> {
    match prepare_paginated_execution(schema.get_table(&query.query.table), query, principal)? {
        None => Ok(Page {
            data: Vec::new(),
            next_cursor: None,
            has_more: false,
        }),
        Some(prepared) => paginate_documents_for_docs_prepared(documents, &prepared, principal),
    }
}

fn wait_for_latest_applied_visibility_blocking(runtime: &TenantRuntime) -> SequenceNumber {
    let required_sequence = runtime.durable_head();
    runtime.wait_for_applied_sequence_blocking(required_sequence);
    required_sequence
}

#[derive(Debug)]
pub(crate) struct SubscriptionBootstrapResult {
    pub(crate) documents: Vec<Document>,
    pub(crate) covered_sequence: SequenceNumber,
}

pub(crate) fn evaluate_subscription_bootstrap_cancellable_for_principal(
    runtime: &TenantRuntime,
    query: &Query,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<SubscriptionBootstrapResult> {
    if let Some(result) =
        evaluate_subscription_bootstrap_with_materialized_surface_cancellable_for_principal(
            runtime,
            query,
            principal,
            check_cancel,
        )?
    {
        return Ok(result);
    }
    let schema = runtime.schema();
    let snapshot = runtime.store.read_snapshot()?;
    let covered_sequence = snapshot.applied_sequence()?;
    let (plan_kind, documents) = query_documents_for_snapshot_and_principal_cancellable(
        &snapshot,
        query,
        schema.get_table(&query.table),
        principal,
        check_cancel,
    )?;
    runtime.record_query_plan_metric(QueryPlanMetricOperation::Query, plan_kind);
    Ok(SubscriptionBootstrapResult {
        documents,
        covered_sequence,
    })
}

fn evaluate_subscription_bootstrap_with_materialized_surface_cancellable_for_principal(
    runtime: &TenantRuntime,
    query: &Query,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Option<SubscriptionBootstrapResult>> {
    let schema = runtime.schema();
    if !should_use_materialized_surface_for_query(schema.get_table(&query.table), query, principal)?
    {
        return Ok(None);
    }

    let required_sequence = runtime.applied_head();
    let snapshot = runtime.load_materialized_serving_snapshot_cancellable(
        runtime.store.as_ref(),
        &query.table,
        required_sequence,
        check_cancel,
    )?;
    runtime.record_query_plan_metric(
        QueryPlanMetricOperation::Query,
        QueryPlanMetricKind::FullScan,
    );
    runtime.record_materialized_query_evaluation();
    Ok(Some(SubscriptionBootstrapResult {
        documents: query_documents_for_docs_with_principal(
            snapshot_table_documents(
                &snapshot,
                &query.table,
                "subscription bootstrap materialized evaluation",
            )?,
            &schema,
            query,
            principal,
        )?,
        covered_sequence: snapshot.covered_sequence(),
    }))
}

pub(crate) async fn evaluate_subscription_bootstrap_async_for_principal<Fut, Check>(
    runtime: Arc<TenantRuntime>,
    tenant_id: TenantId,
    query: Query,
    principal: PrincipalContext,
    cancel_wait: Fut,
    check_cancel: Check,
) -> Result<SubscriptionBootstrapResult>
where
    Fut: Future<Output = ()> + Send,
    Check: Fn() -> Result<()> + Send + Sync + 'static,
{
    let cancel_wait = cancel_wait.shared();
    let check_cancel = Arc::new(check_cancel);
    let _operation = runtime.enter_operation(&tenant_id)?;
    if let Some(result) =
        evaluate_subscription_bootstrap_with_materialized_surface_async_for_principal(
            runtime.clone(),
            query.clone(),
            principal.clone(),
            cancel_wait.clone(),
            check_cancel.clone(),
        )
        .await?
    {
        return Ok(result);
    }
    let schema = runtime.schema();
    let table_schema = schema.get_table(&query.table).cloned();
    let query_for_task = query.clone();
    let principal_for_task = principal.clone();
    let (plan_kind, result) = runtime
        .read_storage
        .execute_cancellable(
            cancel_wait,
            {
                let check_cancel = check_cancel.clone();
                move || check_cancel()
            },
            move |store, check_cancel| {
                let snapshot = store.read_snapshot()?;
                let covered_sequence = snapshot.applied_sequence()?;
                let (plan_kind, documents) =
                    query_documents_for_snapshot_and_principal_cancellable(
                        &snapshot,
                        &query_for_task,
                        table_schema.as_ref(),
                        &principal_for_task,
                        check_cancel,
                    )?;
                Ok((
                    plan_kind,
                    SubscriptionBootstrapResult {
                        documents,
                        covered_sequence,
                    },
                ))
            },
        )
        .await?;
    runtime.record_query_plan_metric(QueryPlanMetricOperation::Query, plan_kind);
    Ok(result)
}

async fn evaluate_subscription_bootstrap_with_materialized_surface_async_for_principal<Fut, Check>(
    runtime: Arc<TenantRuntime>,
    query: Query,
    principal: PrincipalContext,
    cancel_wait: Fut,
    check_cancel: Arc<Check>,
) -> Result<Option<SubscriptionBootstrapResult>>
where
    Fut: Future<Output = ()> + Clone + Send,
    Check: Fn() -> Result<()> + Send + Sync + 'static,
{
    let required_sequence = runtime.applied_head();
    let schema = runtime.schema();
    if !should_use_materialized_surface_for_query(
        schema.get_table(&query.table),
        &query,
        &principal,
    )? {
        return Ok(None);
    }

    let snapshot = if let Some(snapshot) =
        runtime.materialized_serving_snapshot_for_table(&query.table, required_sequence)
    {
        snapshot
    } else {
        let runtime_for_task = runtime.clone();
        let table_for_task = query.table.clone();
        runtime
            .read_storage
            .execute_cancellable(
                cancel_wait,
                {
                    let check_cancel = check_cancel.clone();
                    move || check_cancel()
                },
                move |store, check_cancel| {
                    runtime_for_task.load_materialized_serving_snapshot_cancellable(
                        store.as_ref(),
                        &table_for_task,
                        required_sequence,
                        check_cancel,
                    )
                },
            )
            .await?
    };
    runtime.record_query_plan_metric(
        QueryPlanMetricOperation::Query,
        QueryPlanMetricKind::FullScan,
    );
    runtime.record_materialized_query_evaluation();
    Ok(Some(SubscriptionBootstrapResult {
        documents: query_documents_for_docs_with_principal(
            snapshot_table_documents(
                &snapshot,
                &query.table,
                "subscription bootstrap materialized evaluation",
            )?,
            &schema,
            &query,
            &principal,
        )?,
        covered_sequence: snapshot.covered_sequence(),
    }))
}

pub(crate) fn evaluate_with_index_cancellable_for_principal(
    runtime: &TenantRuntime,
    query: &Query,
    principal: &PrincipalContext,
    required_sequence: SequenceNumber,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let schema = runtime.schema();
    match prepare_query_execution(schema.get_table(&query.table), query, principal)? {
        None => Ok(Vec::new()),
        Some(prepared) if matches!(prepared.plan, QueryPlan::FullScan) => {
            let (plan_kind, documents) = evaluate_with_materialized_surface_cancellable_prepared(
                runtime,
                query,
                &prepared,
                principal,
                required_sequence,
                QueryPlanMetricOperation::Query,
                check_cancel,
            )?;
            runtime.record_query_plan_metric(QueryPlanMetricOperation::Query, plan_kind);
            runtime.cache_documents(&documents);
            Ok(documents)
        }
        Some(prepared) => {
            let plan_kind = query_plan_metric_kind(&prepared.plan);
            let documents = query_documents_for_store_prepared_cancellable(
                runtime.store.as_ref(),
                &prepared,
                principal,
                check_cancel,
            )?;
            runtime.record_query_plan_metric(QueryPlanMetricOperation::Query, plan_kind);
            runtime.cache_documents(&documents);
            Ok(documents)
        }
    }
}

fn evaluate_with_materialized_surface_cancellable_prepared(
    runtime: &TenantRuntime,
    query: &Query,
    prepared: &PreparedQueryExecution,
    principal: &PrincipalContext,
    required_sequence: SequenceNumber,
    operation: QueryPlanMetricOperation,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<(QueryPlanMetricKind, Vec<Document>)> {
    let snapshot = runtime.load_materialized_serving_snapshot_cancellable(
        runtime.store.as_ref(),
        &query.table,
        required_sequence,
        check_cancel,
    )?;
    debug_assert!(snapshot.covered_sequence().0 >= required_sequence.0);
    record_materialized_surface_operation(runtime, operation);
    Ok((
        QueryPlanMetricKind::FullScan,
        query_documents_for_docs_prepared(
            snapshot_table_documents(&snapshot, &query.table, "materialized query evaluation")?,
            prepared,
            principal,
        )?,
    ))
}

async fn evaluate_with_index_async_prepared<Fut, Check>(
    runtime: Arc<TenantRuntime>,
    prepared: PreparedQueryExecution,
    principal: PrincipalContext,
    cancel_wait: Fut,
    check_cancel: Check,
) -> Result<(QueryPlanMetricKind, Vec<Document>)>
where
    Fut: Future<Output = ()> + Send,
    Check: Fn() -> Result<()> + Send + 'static,
{
    let plan_kind = query_plan_metric_kind(&prepared.plan);
    let principal_for_task = principal.clone();
    let prepared_for_task = prepared.clone();
    let documents = runtime
        .read_storage
        .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
            query_documents_for_store_prepared_cancellable(
                store.as_ref(),
                &prepared_for_task,
                &principal_for_task,
                check_cancel,
            )
        })
        .await?;
    Ok((plan_kind, documents))
}

async fn evaluate_with_materialized_surface_async_prepared<Fut, Check>(
    runtime: Arc<TenantRuntime>,
    query: &Query,
    prepared: PreparedQueryExecution,
    principal: PrincipalContext,
    required_sequence: SequenceNumber,
    cancel_wait: Fut,
    check_cancel: Check,
) -> Result<(QueryPlanMetricKind, Vec<Document>)>
where
    Fut: Future<Output = ()> + Send,
    Check: Fn() -> Result<()> + Send + 'static,
{
    let snapshot = if let Some(snapshot) =
        runtime.materialized_serving_snapshot_for_table(&query.table, required_sequence)
    {
        snapshot
    } else {
        let runtime_for_task = runtime.clone();
        let table_for_task = query.table.clone();
        runtime
            .read_storage
            .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                runtime_for_task.load_materialized_serving_snapshot_cancellable(
                    store.as_ref(),
                    &table_for_task,
                    required_sequence,
                    check_cancel,
                )
            })
            .await?
    };
    debug_assert!(snapshot.covered_sequence().0 >= required_sequence.0);
    runtime.record_materialized_query_evaluation();
    Ok((
        QueryPlanMetricKind::FullScan,
        query_documents_for_docs_prepared(
            snapshot_table_documents(
                &snapshot,
                &query.table,
                "async materialized query evaluation",
            )?,
            &prepared,
            &principal,
        )?,
    ))
}

async fn paginate_with_index_async_prepared<Fut, Check>(
    runtime: Arc<TenantRuntime>,
    prepared: PreparedPaginatedExecution,
    principal: PrincipalContext,
    cancel_wait: Fut,
    check_cancel: Check,
) -> Result<(QueryPlanMetricKind, Page)>
where
    Fut: Future<Output = ()> + Send,
    Check: Fn() -> Result<()> + Send + 'static,
{
    let plan_kind = query_plan_metric_kind(&prepared.plan);
    let principal_for_task = principal.clone();
    let prepared_for_task = prepared.clone();
    let page = runtime
        .read_storage
        .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
            paginate_documents_for_store_prepared_cancellable(
                store.as_ref(),
                &prepared_for_task,
                &principal_for_task,
                check_cancel,
            )
        })
        .await?;
    Ok((plan_kind, page))
}

fn query_documents_for_store_prepared_cancellable(
    store: &TenantStore,
    prepared: &PreparedQueryExecution,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let mut include_document =
        |document: &Document| prepared.authorization.allows_document(principal, document);
    if let Some(documents) = load_query_plan_documents_cancellable(
        store,
        &prepared.planned_query,
        &prepared.plan,
        check_cancel,
    )? {
        let residual_query = prepared.plan.residual_query(&prepared.planned_query);
        evaluate_query_with_docs_cancellable_and_predicate(
            documents,
            &residual_query,
            check_cancel,
            &mut include_document,
        )
    } else {
        evaluate_query_cancellable_with_predicate(
            store,
            &prepared.planned_query,
            check_cancel,
            &mut include_document,
        )
    }
}

fn query_documents_for_snapshot_prepared_cancellable(
    snapshot: &TenantReadSnapshot,
    prepared: &PreparedQueryExecution,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let mut include_document =
        |document: &Document| prepared.authorization.allows_document(principal, document);
    if let Some(documents) = load_query_plan_documents_from_snapshot_cancellable(
        snapshot,
        &prepared.planned_query,
        &prepared.plan,
        check_cancel,
    )? {
        let residual_query = prepared.plan.residual_query(&prepared.planned_query);
        evaluate_query_with_docs_cancellable_and_predicate(
            documents,
            &residual_query,
            check_cancel,
            &mut include_document,
        )
    } else {
        let filtered = snapshot.scan_table_matching_with_filters_cancellable(
            &prepared.planned_query.table,
            &prepared.planned_query.filters,
            check_cancel,
            |document| {
                Ok(matches_filters(document, &prepared.planned_query.filters)?
                    && include_document(document)?)
            },
        )?;
        evaluate_query_with_docs_cancellable_and_predicate(
            filtered,
            &prepared.planned_query,
            check_cancel,
            &mut |_| Ok(true),
        )
    }
}

fn paginate_documents_for_store_prepared_cancellable(
    store: &TenantStore,
    prepared: &PreparedPaginatedExecution,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Page> {
    let mut include_document =
        |document: &Document| prepared.authorization.allows_document(principal, document);
    if let Some(index_docs) = load_query_plan_documents_cancellable(
        store,
        &prepared.planned_paginated.query,
        &prepared.plan,
        check_cancel,
    )? {
        let residual_paginated = PaginatedQuery {
            query: prepared
                .plan
                .residual_query(&prepared.planned_paginated.query),
            page_size: prepared.planned_paginated.page_size,
            after: prepared.planned_paginated.after.clone(),
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
            &prepared.planned_paginated,
            check_cancel,
            &mut include_document,
        )
    }
}

fn query_documents_for_store_and_principal_cancellable(
    store: &TenantStore,
    query: &Query,
    table_schema: Option<&TableSchema>,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<(QueryPlanMetricKind, Vec<Document>)> {
    match prepare_query_execution(table_schema, query, principal)? {
        None => Ok((QueryPlanMetricKind::FullScan, Vec::new())),
        Some(prepared) => Ok((
            query_plan_metric_kind(&prepared.plan),
            query_documents_for_store_prepared_cancellable(
                store,
                &prepared,
                principal,
                check_cancel,
            )?,
        )),
    }
}

fn query_documents_for_snapshot_and_principal_cancellable(
    snapshot: &TenantReadSnapshot,
    query: &Query,
    table_schema: Option<&TableSchema>,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<(QueryPlanMetricKind, Vec<Document>)> {
    match prepare_query_execution(table_schema, query, principal)? {
        None => Ok((QueryPlanMetricKind::FullScan, Vec::new())),
        Some(prepared) => Ok((
            query_plan_metric_kind(&prepared.plan),
            query_documents_for_snapshot_prepared_cancellable(
                snapshot,
                &prepared,
                principal,
                check_cancel,
            )?,
        )),
    }
}

fn paginate_documents_for_store_and_principal(
    store: &TenantStore,
    query: &PaginatedQuery,
    table_schema: Option<&TableSchema>,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<(QueryPlanMetricKind, Page)> {
    match prepare_paginated_execution(table_schema, query, principal)? {
        None => Ok((
            QueryPlanMetricKind::FullScan,
            Page {
                data: Vec::new(),
                next_cursor: None,
                has_more: false,
            },
        )),
        Some(prepared) => Ok((
            query_plan_metric_kind(&prepared.plan),
            paginate_documents_for_store_prepared_cancellable(
                store,
                &prepared,
                principal,
                check_cancel,
            )?,
        )),
    }
}

fn paginate_with_materialized_surface_cancellable_prepared(
    runtime: &TenantRuntime,
    query: &PaginatedQuery,
    prepared: &PreparedPaginatedExecution,
    principal: &PrincipalContext,
    required_sequence: SequenceNumber,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<(QueryPlanMetricKind, Page)> {
    let snapshot = runtime.load_materialized_serving_snapshot_cancellable(
        runtime.store.as_ref(),
        &query.query.table,
        required_sequence,
        check_cancel,
    )?;
    debug_assert!(snapshot.covered_sequence().0 >= required_sequence.0);
    runtime.record_materialized_paginated_evaluation();
    Ok((
        QueryPlanMetricKind::FullScan,
        paginate_documents_for_docs_prepared(
            snapshot_table_documents(
                &snapshot,
                &query.query.table,
                "materialized paginated evaluation",
            )?,
            prepared,
            principal,
        )?,
    ))
}

async fn paginate_with_materialized_surface_async_prepared<Fut, Check>(
    runtime: Arc<TenantRuntime>,
    query: &PaginatedQuery,
    prepared: PreparedPaginatedExecution,
    principal: PrincipalContext,
    required_sequence: SequenceNumber,
    cancel_wait: Fut,
    check_cancel: Check,
) -> Result<(QueryPlanMetricKind, Page)>
where
    Fut: Future<Output = ()> + Send,
    Check: Fn() -> Result<()> + Send + 'static,
{
    let snapshot = if let Some(snapshot) =
        runtime.materialized_serving_snapshot_for_table(&query.query.table, required_sequence)
    {
        snapshot
    } else {
        let runtime_for_task = runtime.clone();
        let table_for_task = query.query.table.clone();
        runtime
            .read_storage
            .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                runtime_for_task.load_materialized_serving_snapshot_cancellable(
                    store.as_ref(),
                    &table_for_task,
                    required_sequence,
                    check_cancel,
                )
            })
            .await?
    };
    debug_assert!(snapshot.covered_sequence().0 >= required_sequence.0);
    runtime.record_materialized_paginated_evaluation();
    Ok((
        QueryPlanMetricKind::FullScan,
        paginate_documents_for_docs_prepared(
            snapshot_table_documents(
                &snapshot,
                &query.query.table,
                "async materialized paginated evaluation",
            )?,
            &prepared,
            &principal,
        )?,
    ))
}

fn record_materialized_surface_operation(
    runtime: &TenantRuntime,
    operation: QueryPlanMetricOperation,
) {
    match operation {
        QueryPlanMetricOperation::Query => runtime.record_materialized_query_evaluation(),
        QueryPlanMetricOperation::Paginated => runtime.record_materialized_paginated_evaluation(),
    }
}

fn should_use_materialized_surface_for_query(
    table_schema: Option<&TableSchema>,
    query: &Query,
    principal: &PrincipalContext,
) -> Result<bool> {
    let authorization = ReadAuthorization::for_table(table_schema, principal)?;
    if authorization.impossible {
        return Ok(false);
    }
    let planned_query = authorization.merge_query(query);
    Ok(matches!(
        plan_query(&planned_query, table_schema)?,
        QueryPlan::FullScan
    ))
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
        is_composite_index: bool,
        exact_prefix: Vec<PlannedExactMatch>,
        residual_filters: Vec<Filter>,
    },
    RangeIndex(Box<RangeIndexPlan>),
}

impl QueryPlan {
    fn residual_query(&self, query: &Query) -> Query {
        match self {
            Self::FullScan => query.clone(),
            Self::ExactIndex {
                residual_filters, ..
            } => {
                let mut residual_query = query.clone();
                residual_query.filters = residual_filters.clone();
                residual_query
            }
            Self::RangeIndex(plan) => {
                let mut residual_query = query.clone();
                residual_query.filters = plan.residual_filters.clone();
                residual_query
            }
        }
    }
}

fn query_plan_metric_kind(plan: &QueryPlan) -> QueryPlanMetricKind {
    match plan {
        QueryPlan::FullScan => QueryPlanMetricKind::FullScan,
        QueryPlan::ExactIndex {
            is_composite_index, ..
        } => {
            if *is_composite_index {
                QueryPlanMetricKind::CompositeIndex
            } else {
                QueryPlanMetricKind::SingleFieldIndex
            }
        }
        QueryPlan::RangeIndex(plan) => {
            if plan.is_composite_index {
                QueryPlanMetricKind::CompositeIndex
            } else {
                QueryPlanMetricKind::SingleFieldIndex
            }
        }
    }
}

fn plan_query(query: &Query, table_schema: Option<&neovex_core::TableSchema>) -> Result<QueryPlan> {
    plan_query_inner(query, table_schema)
}

fn plan_paginated_query(
    query: &Query,
    table_schema: Option<&neovex_core::TableSchema>,
) -> Result<QueryPlan> {
    plan_query_inner(query, table_schema)
}

fn plan_query_inner(
    query: &Query,
    table_schema: Option<&neovex_core::TableSchema>,
) -> Result<QueryPlan> {
    let Some(table_schema) = table_schema else {
        return Ok(QueryPlan::FullScan);
    };

    let exact = plan_exact_index_scan(query, table_schema);
    let range = plan_range_index_scan(query, table_schema)?;
    Ok(match (exact, range) {
        (Some(exact), Some(range)) => {
            if range.score() > exact.score() {
                range.plan
            } else {
                exact.plan
            }
        }
        (Some(exact), None) => exact.plan,
        (None, Some(range)) => range.plan,
        (None, None) => QueryPlan::FullScan,
    })
}

fn plan_exact_index_scan(
    query: &Query,
    table_schema: &neovex_core::TableSchema,
) -> Option<PlanCandidate> {
    let mut best = None;
    for index in &table_schema.indexes {
        let exact_prefix = collect_exact_prefix(query, index);
        if exact_prefix.is_empty() {
            continue;
        }

        let residual_filters = query
            .filters
            .iter()
            .filter(|candidate| {
                !exact_prefix
                    .iter()
                    .any(|satisfied| matches_exact_prefix_filter(candidate, satisfied))
            })
            .cloned()
            .collect();
        let candidate = PlanCandidate {
            plan: QueryPlan::ExactIndex {
                index_name: index.name.clone(),
                is_composite_index: index.fields.len() > 1,
                exact_prefix: exact_prefix.clone(),
                residual_filters,
            },
            consumed_fields: exact_prefix.len(),
            supports_requested_order: index_supports_requested_order(
                index,
                exact_prefix.len(),
                query,
            ),
            exact_prefix_len: exact_prefix.len(),
            prefer_exact: true,
        };
        choose_better_plan(&mut best, candidate);
    }

    best
}

fn plan_range_index_scan(
    query: &Query,
    table_schema: &neovex_core::TableSchema,
) -> Result<Option<PlanCandidate>> {
    let mut best = None;
    for index in &table_schema.indexes {
        let exact_prefix = collect_exact_prefix(query, index);
        let Some(range_field) = index.fields.get(exact_prefix.len()) else {
            continue;
        };
        let mut kind = None;
        let mut lower = None;
        let mut upper = None;
        let mut unusable = false;

        for filter in query
            .filters
            .iter()
            .filter(|filter| filter.field == *range_field)
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

        let residual_filters = query.filters.iter().try_fold(
            Vec::new(),
            |mut residual_filters, candidate| -> Result<Vec<Filter>> {
                let exact_satisfied = exact_prefix
                    .iter()
                    .any(|satisfied| matches_exact_prefix_filter(candidate, satisfied));
                let range_satisfied = filter_satisfied_by_range_plan(
                    candidate,
                    range_field,
                    lower.as_ref(),
                    upper.as_ref(),
                )?;
                if !exact_satisfied && !range_satisfied {
                    residual_filters.push(candidate.clone());
                }
                Ok(residual_filters)
            },
        )?;
        let candidate = PlanCandidate {
            plan: QueryPlan::RangeIndex(Box::new(RangeIndexPlan {
                index_name: index.name.clone(),
                is_composite_index: index.fields.len() > 1,
                exact_prefix: exact_prefix.clone(),
                range_field: range_field.clone(),
                lower,
                upper,
                residual_filters,
            })),
            consumed_fields: exact_prefix.len() + 1,
            supports_requested_order: index_supports_requested_order(
                index,
                exact_prefix.len(),
                query,
            ),
            exact_prefix_len: exact_prefix.len(),
            prefer_exact: false,
        };
        choose_better_plan(&mut best, candidate);
    }

    Ok(best)
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
            index_name,
            exact_prefix,
            ..
        } => {
            let exact_values: Vec<Value> = exact_prefix
                .iter()
                .map(|match_| match_.value.clone())
                .collect();
            Ok(Some(if exact_values.len() == 1 {
                store.index_scan_eq_cancellable(
                    &query.table,
                    index_name,
                    &exact_values[0],
                    check_cancel,
                )?
            } else {
                store.index_scan_prefix_cancellable(
                    &query.table,
                    index_name,
                    &exact_values,
                    check_cancel,
                )?
            }))
        }
        QueryPlan::RangeIndex(plan) => {
            let exact_values: Vec<Value> = plan
                .exact_prefix
                .iter()
                .map(|match_| match_.value.clone())
                .collect();
            Ok(Some(if exact_values.is_empty() {
                store.index_scan_range_cancellable(
                    &query.table,
                    &plan.index_name,
                    plan.lower.as_ref().map(|bound| &bound.value),
                    plan.upper.as_ref().map(|bound| &bound.value),
                    plan.lower.as_ref().is_none_or(|bound| bound.inclusive),
                    plan.upper.as_ref().is_none_or(|bound| bound.inclusive),
                    check_cancel,
                )?
            } else {
                store.index_scan_composite_range_cancellable(
                    &query.table,
                    &plan.index_name,
                    &exact_values,
                    plan.lower.as_ref().map(|bound| &bound.value),
                    plan.upper.as_ref().map(|bound| &bound.value),
                    plan.lower.as_ref().is_none_or(|bound| bound.inclusive),
                    plan.upper.as_ref().is_none_or(|bound| bound.inclusive),
                    check_cancel,
                )?
            }))
        }
    }
}

fn load_query_plan_documents_from_snapshot_cancellable(
    snapshot: &TenantReadSnapshot,
    query: &Query,
    plan: &QueryPlan,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Option<Vec<Document>>> {
    match plan {
        QueryPlan::FullScan => Ok(None),
        QueryPlan::ExactIndex {
            index_name,
            exact_prefix,
            ..
        } => {
            let exact_values: Vec<Value> = exact_prefix
                .iter()
                .map(|match_| match_.value.clone())
                .collect();
            Ok(Some(if exact_values.len() == 1 {
                snapshot.index_scan_eq_cancellable(
                    &query.table,
                    index_name,
                    &exact_values[0],
                    check_cancel,
                )?
            } else {
                snapshot.index_scan_prefix_cancellable(
                    &query.table,
                    index_name,
                    &exact_values,
                    check_cancel,
                )?
            }))
        }
        QueryPlan::RangeIndex(plan) => {
            let exact_values: Vec<Value> = plan
                .exact_prefix
                .iter()
                .map(|match_| match_.value.clone())
                .collect();
            Ok(Some(if exact_values.is_empty() {
                snapshot.index_scan_range_cancellable(
                    &query.table,
                    &plan.index_name,
                    plan.lower.as_ref().map(|bound| &bound.value),
                    plan.upper.as_ref().map(|bound| &bound.value),
                    plan.lower.as_ref().is_none_or(|bound| bound.inclusive),
                    plan.upper.as_ref().is_none_or(|bound| bound.inclusive),
                    check_cancel,
                )?
            } else {
                snapshot.index_scan_composite_range_cancellable(
                    &query.table,
                    &plan.index_name,
                    &exact_values,
                    plan.lower.as_ref().map(|bound| &bound.value),
                    plan.upper.as_ref().map(|bound| &bound.value),
                    plan.lower.as_ref().is_none_or(|bound| bound.inclusive),
                    plan.upper.as_ref().is_none_or(|bound| bound.inclusive),
                    check_cancel,
                )?
            }))
        }
    }
}

fn load_query_plan_documents_from_docs(
    documents: &[Document],
    plan: &QueryPlan,
) -> Result<Option<Vec<Document>>> {
    let filtered = match plan {
        QueryPlan::FullScan => return Ok(None),
        QueryPlan::ExactIndex { exact_prefix, .. } => documents
            .iter()
            .filter(|document| document_matches_exact_prefix(document, exact_prefix))
            .cloned()
            .collect(),
        QueryPlan::RangeIndex(plan) => {
            let mut filtered = Vec::new();
            for document in documents {
                if document_matches_exact_prefix(document, &plan.exact_prefix)
                    && document_matches_range_bounds(
                        document,
                        &plan.range_field,
                        plan.lower.as_ref(),
                        plan.upper.as_ref(),
                    )?
                {
                    filtered.push(document.clone());
                }
            }
            filtered
        }
    };
    Ok(Some(filtered))
}

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

fn document_matches_exact_prefix(document: &Document, exact_prefix: &[PlannedExactMatch]) -> bool {
    exact_prefix
        .iter()
        .all(|entry| document.get_field(&entry.field) == Some(&entry.value))
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

#[derive(Debug, Clone, PartialEq)]
struct RangeIndexPlan {
    index_name: String,
    is_composite_index: bool,
    exact_prefix: Vec<PlannedExactMatch>,
    range_field: String,
    lower: Option<PlannedRangeBound>,
    upper: Option<PlannedRangeBound>,
    residual_filters: Vec<Filter>,
}

#[derive(Debug, Clone, PartialEq)]
struct PlannedExactMatch {
    field: String,
    value: Value,
}

#[derive(Debug, Clone)]
struct PlanCandidate {
    plan: QueryPlan,
    consumed_fields: usize,
    supports_requested_order: bool,
    exact_prefix_len: usize,
    prefer_exact: bool,
}

impl PlanCandidate {
    fn score(&self) -> (usize, bool, usize, bool) {
        (
            self.consumed_fields,
            self.supports_requested_order,
            self.exact_prefix_len,
            self.prefer_exact,
        )
    }
}

fn choose_better_plan(current: &mut Option<PlanCandidate>, candidate: PlanCandidate) {
    if current
        .as_ref()
        .is_none_or(|existing| candidate.score() > existing.score())
    {
        *current = Some(candidate);
    }
}

fn collect_exact_prefix(
    query: &Query,
    index: &neovex_core::IndexDefinition,
) -> Vec<PlannedExactMatch> {
    let mut exact_prefix = Vec::new();
    for field in &index.fields {
        let Some(filter) = query.filters.iter().find(|filter| {
            filter.field == *field
                && filter.op == FilterOp::Eq
                && is_scalar_index_value(&filter.value)
        }) else {
            break;
        };
        exact_prefix.push(PlannedExactMatch {
            field: field.clone(),
            value: filter.value.clone(),
        });
    }
    exact_prefix
}

fn index_supports_requested_order(
    index: &neovex_core::IndexDefinition,
    exact_prefix_len: usize,
    query: &Query,
) -> bool {
    let Some(order) = &query.order else {
        return false;
    };
    index
        .fields
        .get(exact_prefix_len)
        .is_some_and(|field| field == &order.field)
}

fn matches_exact_prefix_filter(candidate: &Filter, satisfied: &PlannedExactMatch) -> bool {
    candidate.op == FilterOp::Eq
        && candidate.field == satisfied.field
        && candidate.value == satisfied.value
}

fn filter_satisfied_by_range_plan(
    candidate: &Filter,
    range_field: &str,
    lower: Option<&PlannedRangeBound>,
    upper: Option<&PlannedRangeBound>,
) -> Result<bool> {
    if candidate.field != range_field {
        return Ok(false);
    }
    let Some(bound) = range_bound_from_filter(candidate)? else {
        return Ok(false);
    };

    Ok(match bound.side {
        RangeSide::Lower => lower.is_some_and(|selected| {
            compare_lower_bounds(selected.as_ref(), bound.as_ref()) != Ordering::Less
        }),
        RangeSide::Upper => upper.is_some_and(|selected| {
            compare_upper_bounds(selected.as_ref(), bound.as_ref()) != Ordering::Greater
        }),
    })
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

    fn schema_with_indexes(indexes: &[(&str, &[&str])]) -> TableSchema {
        TableSchema {
            table: tasks_table(),
            fields: Vec::new(),
            indexes: indexes
                .iter()
                .map(|(name, fields)| IndexDefinition {
                    name: (*name).to_string(),
                    fields: fields.iter().map(|field| (*field).to_string()).collect(),
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
            Some(&schema_with_indexes(&[("by_status", &["status"])])),
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
                ("by_status", &["status"]),
                ("by_rank", &["rank"]),
            ])),
        )
        .expect("planning should succeed");

        match &plan {
            QueryPlan::ExactIndex {
                index_name,
                is_composite_index,
                exact_prefix,
                residual_filters,
            } => {
                assert_eq!(index_name, "by_status");
                assert!(!is_composite_index);
                assert_eq!(
                    exact_prefix,
                    &vec![PlannedExactMatch {
                        field: "status".to_string(),
                        value: json!("active"),
                    }]
                );
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
        let plan = plan_query(
            &query,
            Some(&schema_with_indexes(&[("by_rank", &["rank"])])),
        )
        .expect("planning should succeed");

        match &plan {
            QueryPlan::RangeIndex(plan) => {
                assert_eq!(plan.index_name, "by_rank");
                assert!(!plan.is_composite_index);
                assert!(plan.exact_prefix.is_empty());
                assert_eq!(plan.range_field, "rank");
                assert_eq!(
                    plan.lower.as_ref().map(|bound| &bound.value),
                    Some(&json!(2))
                );
                assert_eq!(
                    plan.upper.as_ref().map(|bound| &bound.value),
                    Some(&json!(10))
                );
                assert_eq!(
                    &plan.residual_filters,
                    &vec![filter("status", FilterOp::Eq, json!("active"))]
                );
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }

    #[test]
    fn plan_query_selects_composite_exact_prefix_when_it_supports_requested_order() {
        let query = Query {
            table: tasks_table(),
            filters: vec![filter("status", FilterOp::Eq, json!("active"))],
            order: Some(neovex_core::OrderBy {
                field: "rank".to_string(),
                direction: neovex_core::OrderDirection::Asc,
            }),
            limit: None,
        };

        let plan = plan_query(
            &query,
            Some(&schema_with_indexes(&[
                ("by_status", &["status"]),
                ("by_status_rank", &["status", "rank"]),
            ])),
        )
        .expect("planning should succeed");

        match &plan {
            QueryPlan::ExactIndex {
                index_name,
                is_composite_index,
                exact_prefix,
                residual_filters,
            } => {
                assert_eq!(index_name, "by_status_rank");
                assert!(*is_composite_index);
                assert_eq!(
                    exact_prefix,
                    &vec![PlannedExactMatch {
                        field: "status".to_string(),
                        value: json!("active"),
                    }]
                );
                assert!(residual_filters.is_empty());
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }

    #[test]
    fn plan_query_selects_composite_range_after_exact_prefix() {
        let query = Query {
            table: tasks_table(),
            filters: vec![
                filter("status", FilterOp::Eq, json!("active")),
                filter("rank", FilterOp::Gte, json!(2)),
                filter("rank", FilterOp::Lt, json!(10)),
                filter("title", FilterOp::Eq, json!("important")),
            ],
            order: Some(neovex_core::OrderBy {
                field: "rank".to_string(),
                direction: neovex_core::OrderDirection::Asc,
            }),
            limit: None,
        };

        let plan = plan_query(
            &query,
            Some(&schema_with_indexes(&[
                ("by_status", &["status"]),
                ("by_status_rank", &["status", "rank"]),
            ])),
        )
        .expect("planning should succeed");

        match &plan {
            QueryPlan::RangeIndex(plan) => {
                assert_eq!(plan.index_name, "by_status_rank");
                assert!(plan.is_composite_index);
                assert_eq!(
                    &plan.exact_prefix,
                    &vec![PlannedExactMatch {
                        field: "status".to_string(),
                        value: json!("active"),
                    }]
                );
                assert_eq!(plan.range_field, "rank");
                assert_eq!(
                    plan.lower.as_ref().map(|bound| &bound.value),
                    Some(&json!(2))
                );
                assert_eq!(
                    plan.upper.as_ref().map(|bound| &bound.value),
                    Some(&json!(10))
                );
                assert_eq!(
                    &plan.residual_filters,
                    &vec![filter("title", FilterOp::Eq, json!("important"))]
                );
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }

    #[test]
    fn plan_paginated_query_selects_composite_exact_prefix_when_supported() {
        let query = Query {
            table: tasks_table(),
            filters: vec![filter("status", FilterOp::Eq, json!("active"))],
            order: Some(neovex_core::OrderBy {
                field: "rank".to_string(),
                direction: neovex_core::OrderDirection::Asc,
            }),
            limit: None,
        };

        let plan = plan_paginated_query(
            &query,
            Some(&schema_with_indexes(&[(
                "by_status_rank",
                &["status", "rank"],
            )])),
        )
        .expect("planning should succeed");

        match &plan {
            QueryPlan::ExactIndex {
                index_name,
                is_composite_index,
                exact_prefix,
                residual_filters,
            } => {
                assert_eq!(index_name, "by_status_rank");
                assert!(*is_composite_index);
                assert_eq!(
                    exact_prefix,
                    &vec![PlannedExactMatch {
                        field: "status".to_string(),
                        value: json!("active"),
                    }]
                );
                assert!(residual_filters.is_empty());
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }
}
