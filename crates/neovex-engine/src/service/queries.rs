mod authorization;
mod materialized;
mod planner;
mod prepared;
mod snapshot;

use std::future::{Future, pending};
use std::sync::Arc;

use futures::FutureExt;
use neovex_core::{
    Document, DocumentId, DurableMutationRecord, Error, Page, PaginatedQuery, PrincipalContext,
    Query, Result, SequenceNumber, TableName, TenantId,
};
use neovex_storage::{
    DurableJournalBootstrap, DurableJournalPage, ShadowMaterializer, ShadowMaterializerConfig,
    TenantReadStorage, TenantStore,
};

use crate::EmbeddedReplica;
use crate::tenant::QueryPlanMetricOperation;
use crate::verification::{
    ConsistencyScope, ConsistencyVerificationReport, bootstrap_fingerprint,
    collect_durable_journal_bootstrap_mismatches, compare_materialized_journal_snapshots,
    snapshot_fingerprint,
};

use super::Service;
pub(crate) use authorization::ReadAuthorization;
pub(crate) use materialized::{
    evaluate_with_index_cancellable_for_principal, should_use_materialized_surface_for_query,
};
use materialized::{
    evaluate_with_materialized_surface_async_prepared,
    evaluate_with_materialized_surface_cancellable_prepared,
    paginate_with_materialized_surface_async_prepared,
    paginate_with_materialized_surface_cancellable_prepared,
    wait_for_latest_applied_visibility_blocking,
};
use planner::{QueryPlan, query_plan_metric_kind};
#[cfg(test)]
pub(crate) use prepared::paginate_documents_for_docs_with_principal;
use prepared::{
    evaluate_with_index_async_prepared, paginate_documents_for_store_prepared_cancellable,
    paginate_with_index_async_prepared, prepare_paginated_execution, prepare_query_execution,
    query_documents_for_store_prepared_cancellable,
};
pub(crate) use prepared::{
    paginate_documents_for_store_with_principal, query_documents_for_docs_with_principal,
    query_documents_for_snapshot_and_principal_cancellable,
    query_documents_for_store_with_principal,
};
use snapshot::rebuild_authoritative_snapshot;
pub(crate) use snapshot::snapshot_table_documents;

fn full_table_query(table: TableName) -> Query {
    Query {
        table,
        filters: Vec::new(),
        order: None,
        limit: None,
    }
}

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
            &full_table_query(table.clone()),
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
            full_table_query(table),
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
            full_table_query(table),
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
