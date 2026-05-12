use std::future::Future;
use std::sync::Arc;

use nimbus_core::{
    Document, Page, PaginatedQuery, PrincipalContext, Query, Result, SequenceNumber, TableSchema,
};

use crate::tenant::{QueryPlanMetricKind, QueryPlanMetricOperation, TenantRuntime};

use super::authorization::ReadAuthorization;
use super::planner::{QueryPlan, plan_query, query_plan_metric_kind};
use super::prepared::{
    PreparedPaginatedExecution, PreparedQueryExecution, paginate_documents_for_docs_prepared,
    prepare_query_execution, query_documents_for_docs_prepared,
    query_documents_for_read_surface_prepared_cancellable,
};
use super::snapshot::snapshot_table_documents;

pub(super) fn wait_for_latest_applied_visibility_blocking(
    runtime: &TenantRuntime,
) -> SequenceNumber {
    let required_sequence = runtime.durable_head();
    runtime.wait_for_applied_sequence_blocking(required_sequence);
    required_sequence
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

pub(super) fn evaluate_with_materialized_surface_cancellable_prepared(
    runtime: &TenantRuntime,
    query: &Query,
    prepared: &PreparedQueryExecution,
    principal: &PrincipalContext,
    required_sequence: SequenceNumber,
    operation: QueryPlanMetricOperation,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<(QueryPlanMetricKind, Vec<Document>)> {
    let snapshot = runtime.load_materialized_serving_snapshot_cancellable(
        &runtime.store,
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

pub(super) async fn evaluate_with_materialized_surface_async_prepared<Fut, Check>(
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
                    &store,
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

pub(super) fn paginate_with_materialized_surface_cancellable_prepared(
    runtime: &TenantRuntime,
    query: &PaginatedQuery,
    prepared: &PreparedPaginatedExecution,
    principal: &PrincipalContext,
    required_sequence: SequenceNumber,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<(QueryPlanMetricKind, Page)> {
    let snapshot = runtime.load_materialized_serving_snapshot_cancellable(
        &runtime.store,
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

pub(super) async fn paginate_with_materialized_surface_async_prepared<Fut, Check>(
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
                    &store,
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

pub(crate) fn should_use_materialized_surface_for_query(
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
