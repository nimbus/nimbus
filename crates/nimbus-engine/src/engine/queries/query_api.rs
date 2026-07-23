use std::future::{Future, pending};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::FutureExt;
use nimbus_core::{
    Document, Error, Page, PaginatedQuery, PrincipalContext, Query, Result, SequenceNumber,
    TableId, TableName, TenantId,
};

use super::materialized::{
    evaluate_with_materialized_surface_async_prepared,
    evaluate_with_materialized_surface_cancellable_prepared,
    paginate_with_materialized_surface_async_prepared,
    paginate_with_materialized_surface_cancellable_prepared,
    wait_for_latest_applied_visibility_blocking,
};
use super::planner::{QueryPlan, query_plan_metric_kind};
use super::prepared::{
    evaluate_with_index_async_prepared, paginate_documents_for_read_surface_prepared_cancellable,
    paginate_with_index_async_prepared, prepare_paginated_execution, prepare_query_execution,
    query_documents_for_read_surface_prepared_cancellable,
};
use crate::engine::Engine;
use crate::engine::latency::{LatencySegment, budgeted_segment};
use crate::tenant::{QueryPlanMetricKind, QueryPlanMetricOperation, TenantRuntime};

struct LoadedQueryRuntime {
    runtime: Arc<TenantRuntime>,
    required_sequence: SequenceNumber,
    tenant_load: Duration,
    wait_visibility: Duration,
}

impl Engine {
    /// Resolves the active stable table identity for a tenant/table, if it exists.
    pub fn table_id(&self, tenant_id: &TenantId, table: &TableName) -> Result<Option<TableId>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        runtime.store().table_id(table)
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
        let total_started = Instant::now();
        let LoadedQueryRuntime {
            runtime,
            required_sequence,
            tenant_load,
            wait_visibility,
        } = load_query_runtime_async(self, &tenant_id, cancel_wait.clone()).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        let schema = runtime.schema();
        let prepare_timer = budgeted_segment(LatencySegment::QueryPrepare);
        let prepared = prepare_query_execution(schema.get_table(&query.table), &query, &principal)?;
        let prepare_elapsed = prepare_timer.finish();
        match prepared {
            None => {
                maybe_emit_query_profile(QueryProfileSample {
                    tenant_id: &tenant_id,
                    plan: "none",
                    tenant_load,
                    wait_visibility,
                    prepare: prepare_elapsed,
                    execute: Duration::ZERO,
                    cache: Duration::ZERO,
                    total: total_started.elapsed(),
                });
                Ok(Vec::new())
            }
            Some(prepared) if matches!(prepared.plan, QueryPlan::FullScan) => {
                let execute_timer = budgeted_segment(LatencySegment::QueryExecute);
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
                let execute_elapsed = execute_timer.finish();
                let cache_timer = budgeted_segment(LatencySegment::QueryCache);
                runtime.record_query_plan_metric(QueryPlanMetricOperation::Query, plan_kind);
                runtime.cache_documents(&documents);
                let cache_elapsed = cache_timer.finish();
                maybe_emit_query_profile(QueryProfileSample {
                    tenant_id: &tenant_id,
                    plan: query_plan_metric_kind_label(plan_kind),
                    tenant_load,
                    wait_visibility,
                    prepare: prepare_elapsed,
                    execute: execute_elapsed,
                    cache: cache_elapsed,
                    total: total_started.elapsed(),
                });
                Ok(documents)
            }
            Some(prepared) => {
                let execute_timer = budgeted_segment(LatencySegment::QueryExecute);
                let (plan_kind, documents) = evaluate_with_index_async_prepared(
                    runtime.clone(),
                    prepared,
                    principal,
                    cancel_wait,
                    check_cancel,
                )
                .await?;
                let execute_elapsed = execute_timer.finish();
                let cache_timer = budgeted_segment(LatencySegment::QueryCache);
                runtime.record_query_plan_metric(QueryPlanMetricOperation::Query, plan_kind);
                runtime.cache_documents(&documents);
                let cache_elapsed = cache_timer.finish();
                maybe_emit_query_profile(QueryProfileSample {
                    tenant_id: &tenant_id,
                    plan: query_plan_metric_kind_label(plan_kind),
                    tenant_load,
                    wait_visibility,
                    prepare: prepare_elapsed,
                    execute: execute_elapsed,
                    cache: cache_elapsed,
                    total: total_started.elapsed(),
                });
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
        let LoadedQueryRuntime {
            runtime,
            required_sequence,
            ..
        } = load_query_runtime(self, tenant_id)?;
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
                let documents = query_documents_for_read_surface_prepared_cancellable(
                    &runtime.store,
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

    /// Reads documents whose `DocumentId` begins with `id_prefix`.
    pub fn scan_documents_by_id_prefix_cancellable(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        id_prefix: &str,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let LoadedQueryRuntime { runtime, .. } = load_query_runtime(self, tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let documents =
            runtime
                .store()
                .scan_table_id_prefix_cancellable(table, id_prefix, check_cancel)?;
        runtime.cache_documents(&documents);
        Ok(documents)
    }

    /// Reads at most `limit` documents from `table` whose `DocumentId` is at or
    /// after `start_id`, in document-id order.
    pub fn scan_documents_by_id_starting_at_cancellable(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        start_id: &str,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let LoadedQueryRuntime { runtime, .. } = load_query_runtime(self, tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let documents = runtime.store().scan_table_id_starting_at_cancellable(
            table,
            start_id,
            limit,
            check_cancel,
        )?;
        runtime.cache_documents(&documents);
        Ok(documents)
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
        let LoadedQueryRuntime {
            runtime,
            required_sequence,
            ..
        } = load_query_runtime_async(self, &tenant_id, cancel_wait.clone()).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        let schema = runtime.schema();
        match prepare_paginated_execution(schema.get_table(&query.query.table), &query, &principal)?
        {
            None => Ok(empty_page()),
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
        let LoadedQueryRuntime {
            runtime,
            required_sequence,
            ..
        } = load_query_runtime(self, tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let schema = runtime.schema();
        match prepare_paginated_execution(schema.get_table(&query.query.table), query, principal)? {
            None => Ok(empty_page()),
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
                let page = paginate_documents_for_read_surface_prepared_cancellable(
                    &runtime.store,
                    &prepared,
                    principal,
                    check_cancel,
                )?;
                runtime.record_query_plan_metric(QueryPlanMetricOperation::Paginated, plan_kind);
                Ok(page)
            }
        }
    }
}

struct QueryProfileSample<'a> {
    tenant_id: &'a TenantId,
    plan: &'a str,
    tenant_load: Duration,
    wait_visibility: Duration,
    prepare: Duration,
    execute: Duration,
    cache: Duration,
    total: Duration,
}

fn maybe_emit_query_profile(sample: QueryProfileSample<'_>) {
    if std::env::var_os("NIMBUS_QUERY_PROFILE").is_none() {
        return;
    }

    eprintln!(
        "query-profile tenant={} plan={} tenant_load={:?} wait_visibility={:?} prepare={:?} execute={:?} cache={:?} total={:?}",
        sample.tenant_id,
        sample.plan,
        sample.tenant_load,
        sample.wait_visibility,
        sample.prepare,
        sample.execute,
        sample.cache,
        sample.total,
    );
}

fn query_plan_metric_kind_label(kind: QueryPlanMetricKind) -> &'static str {
    match kind {
        QueryPlanMetricKind::FullScan => "full_scan",
        QueryPlanMetricKind::SingleFieldIndex => "single_field_index",
        QueryPlanMetricKind::CompositeIndex => "composite_index",
    }
}

fn load_query_runtime(engine: &Engine, tenant_id: &TenantId) -> Result<LoadedQueryRuntime> {
    let runtime = engine.get_existing_tenant(tenant_id)?;
    let visibility_started = Instant::now();
    let required_sequence = wait_for_latest_applied_visibility_blocking(&runtime)?;
    Ok(LoadedQueryRuntime {
        runtime,
        required_sequence,
        tenant_load: Duration::ZERO,
        wait_visibility: visibility_started.elapsed(),
    })
}

async fn load_query_runtime_async<Fut>(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    cancel_wait: Fut,
) -> Result<LoadedQueryRuntime>
where
    Fut: Future<Output = ()> + Clone + Send,
{
    loop {
        let tenant_load_timer = budgeted_segment(LatencySegment::TenantLoad);
        let runtime = engine.get_existing_tenant_async(tenant_id).await?;
        let tenant_load = tenant_load_timer.finish();
        let required_sequence = runtime.durable_head();
        let visibility_timer = budgeted_segment(LatencySegment::WaitVisibility);
        let eviction_completion = runtime.eviction_completion();
        tokio::select! {
            result = runtime.wait_for_applied_sequence_cancellable(
                required_sequence,
                cancel_wait.clone(),
            ) => {
                if let Err(error) = result {
                    if runtime.eviction_started() {
                        let completion = runtime.eviction_completion();
                        drop(runtime);
                        tokio::select! {
                            () = completion.wait() => continue,
                            () = cancel_wait.clone() => return Err(Error::Cancelled),
                        }
                    }
                    return Err(error);
                }
                let wait_visibility = visibility_timer.finish();
                return Ok(LoadedQueryRuntime {
                    runtime,
                    required_sequence,
                    tenant_load,
                    wait_visibility,
                });
            }
            () = eviction_completion.wait() => {
                drop(runtime);
                // The durable head may have advanced while the failed runtime's
                // applied head cannot. Retry against the replayed replacement.
            }
        }
    }
}

fn empty_page() -> Page {
    Page {
        data: Vec::new(),
        next_cursor: None,
        has_more: false,
    }
}
