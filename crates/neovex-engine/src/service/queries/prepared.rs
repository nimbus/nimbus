use std::future::Future;
use std::sync::Arc;

use neovex_core::{
    Document, Page, PaginatedQuery, PrincipalContext, Query, Result, Schema, TableSchema,
};
use neovex_storage::{TenantReadSnapshot, TenantReadStorage, TenantStore};

use crate::evaluator::{
    evaluate_paginated_cancellable_with_predicate,
    evaluate_paginated_with_docs_cancellable_and_predicate,
    evaluate_query_cancellable_with_predicate, evaluate_query_with_docs_cancellable_and_predicate,
    matches_filters,
};
use crate::tenant::{QueryPlanMetricKind, TenantRuntime};

use super::authorization::ReadAuthorization;
use super::planner::{
    QueryPlan, load_query_plan_documents_cancellable, load_query_plan_documents_from_docs,
    load_query_plan_documents_from_snapshot_cancellable, plan_paginated_query, plan_query,
    query_plan_metric_kind,
};

#[derive(Debug, Clone)]
pub(super) struct PreparedQueryExecution {
    pub(super) authorization: ReadAuthorization,
    pub(super) planned_query: Query,
    pub(super) plan: QueryPlan,
}

#[derive(Debug, Clone)]
pub(super) struct PreparedPaginatedExecution {
    pub(super) authorization: ReadAuthorization,
    pub(super) planned_paginated: PaginatedQuery,
    pub(super) plan: QueryPlan,
}

pub(super) fn prepare_query_execution(
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

pub(super) fn prepare_paginated_execution(
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

pub(super) fn query_documents_for_docs_prepared(
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

pub(super) fn paginate_documents_for_docs_prepared(
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

pub(super) async fn evaluate_with_index_async_prepared<Fut, Check>(
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

pub(super) async fn paginate_with_index_async_prepared<Fut, Check>(
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

pub(super) fn query_documents_for_store_prepared_cancellable(
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

pub(super) fn paginate_documents_for_store_prepared_cancellable(
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

pub(crate) fn query_documents_for_snapshot_and_principal_cancellable(
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
