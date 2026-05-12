use std::future::Future;
use std::sync::Arc;

use nimbus_core::{
    Document, Page, PaginatedQuery, PrincipalContext, Query, Result, Schema, TableSchema,
};
use nimbus_storage::QueryReadStore;

use crate::evaluator::{
    evaluate_paginated_cancellable_with_predicate,
    evaluate_paginated_with_docs_cancellable_and_predicate,
    evaluate_query_cancellable_with_predicate, evaluate_query_with_docs_cancellable_and_predicate,
};
use crate::tenant::{QueryPlanMetricKind, TenantRuntime};

use super::authorization::ReadAuthorization;
use super::planner::{
    QueryPlan, load_query_plan_documents_cancellable, load_query_plan_documents_from_docs,
    plan_paginated_query, plan_query, query_plan_metric_kind,
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

pub(crate) fn query_documents_for_store_with_principal<S>(
    store: &S,
    schema: &Schema,
    query: &Query,
    principal: &PrincipalContext,
) -> Result<Vec<Document>>
where
    S: QueryReadStore + ?Sized,
{
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

pub(crate) fn paginate_documents_for_store_with_principal<S>(
    store: &S,
    schema: &Schema,
    query: &PaginatedQuery,
    principal: &PrincipalContext,
) -> Result<Page>
where
    S: QueryReadStore + ?Sized,
{
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
            query_documents_for_read_surface_prepared_cancellable(
                &store,
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
            paginate_documents_for_read_surface_prepared_cancellable(
                &store,
                &prepared_for_task,
                &principal_for_task,
                check_cancel,
            )
        })
        .await?;
    Ok((plan_kind, page))
}

pub(super) fn query_documents_for_read_surface_prepared_cancellable<S>(
    store: &S,
    prepared: &PreparedQueryExecution,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>>
where
    S: QueryReadStore + ?Sized,
{
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

pub(super) fn paginate_documents_for_read_surface_prepared_cancellable<S>(
    store: &S,
    prepared: &PreparedPaginatedExecution,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Page>
where
    S: QueryReadStore + ?Sized,
{
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

fn query_documents_for_store_and_principal_cancellable<S>(
    store: &S,
    query: &Query,
    table_schema: Option<&TableSchema>,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<(QueryPlanMetricKind, Vec<Document>)>
where
    S: QueryReadStore + ?Sized,
{
    match prepare_query_execution(table_schema, query, principal)? {
        None => Ok((QueryPlanMetricKind::FullScan, Vec::new())),
        Some(prepared) => Ok((
            query_plan_metric_kind(&prepared.plan),
            query_documents_for_read_surface_prepared_cancellable(
                store,
                &prepared,
                principal,
                check_cancel,
            )?,
        )),
    }
}

pub(crate) fn query_documents_for_snapshot_and_principal_cancellable<S>(
    snapshot: &S,
    query: &Query,
    table_schema: Option<&TableSchema>,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<(QueryPlanMetricKind, Vec<Document>)>
where
    S: QueryReadStore + ?Sized,
{
    match prepare_query_execution(table_schema, query, principal)? {
        None => Ok((QueryPlanMetricKind::FullScan, Vec::new())),
        Some(prepared) => Ok((
            query_plan_metric_kind(&prepared.plan),
            query_documents_for_read_surface_prepared_cancellable(
                snapshot,
                &prepared,
                principal,
                check_cancel,
            )?,
        )),
    }
}

fn paginate_documents_for_store_and_principal<S>(
    store: &S,
    query: &PaginatedQuery,
    table_schema: Option<&TableSchema>,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<(QueryPlanMetricKind, Page)>
where
    S: QueryReadStore + ?Sized,
{
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
            paginate_documents_for_read_surface_prepared_cancellable(
                store,
                &prepared,
                principal,
                check_cancel,
            )?,
        )),
    }
}

#[cfg(test)]
mod tests {
    use nimbus_core::{
        Document, FieldSchema, FieldType, Filter, FilterOp, IndexDefinition, OrderBy,
        OrderDirection, TableName,
    };
    use nimbus_storage::SqliteTenantStore;
    use serde_json::{Map, json};
    use tempfile::tempdir;

    use super::*;

    fn tasks_table() -> TableName {
        TableName::new("tasks").expect("table should be valid")
    }

    fn tasks_schema() -> TableSchema {
        TableSchema {
            table: tasks_table(),
            fields: vec![
                FieldSchema {
                    name: "status".to_string(),
                    field_type: FieldType::String,
                    required: false,
                },
                FieldSchema {
                    name: "rank".to_string(),
                    field_type: FieldType::Number,
                    required: false,
                },
                FieldSchema {
                    name: "title".to_string(),
                    field_type: FieldType::String,
                    required: false,
                },
            ],
            indexes: vec![IndexDefinition {
                name: "by_status_rank".to_string(),
                fields: vec!["status".to_string(), "rank".to_string()],
            }],
            access_policy: None,
        }
    }

    fn task_document(status: &str, rank: i64, title: &str) -> Document {
        Document::new(
            tasks_table(),
            Map::from_iter([
                ("status".to_string(), json!(status)),
                ("rank".to_string(), json!(rank)),
                ("title".to_string(), json!(title)),
            ]),
        )
    }

    fn seeded_sqlite_store() -> (tempfile::TempDir, SqliteTenantStore, Schema) {
        let dir = tempdir().expect("temporary dir should create");
        let path = dir.path().join("tenant.sqlite3");
        let store = SqliteTenantStore::open(&path).expect("sqlite store should open");
        store
            .replace_table_schema(&tasks_schema())
            .expect("schema should save");
        for document in [
            task_document("open", 1, "alpha"),
            task_document("open", 2, "beta"),
            task_document("closed", 3, "gamma"),
            task_document("open", 4, "delta"),
        ] {
            store
                .insert_document_for_testing(&document)
                .expect("document should insert");
        }
        let schema = store.load_schema().expect("schema should load");
        (dir, store, schema)
    }

    #[test]
    fn sqlite_query_read_surface_supports_store_and_snapshot_paths() {
        let (_dir, store, schema) = seeded_sqlite_store();
        let query = Query {
            table: tasks_table(),
            filters: vec![
                Filter {
                    field: "status".to_string(),
                    op: FilterOp::Eq,
                    value: json!("open"),
                },
                Filter {
                    field: "rank".to_string(),
                    op: FilterOp::Gte,
                    value: json!(2),
                },
            ],
            order: Some(OrderBy {
                field: "rank".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        };
        let principal = PrincipalContext::anonymous();
        let prepared = prepare_query_execution(schema.get_table(&query.table), &query, &principal)
            .expect("query preparation should succeed")
            .expect("query should remain authorized");
        assert_eq!(
            query_plan_metric_kind(&prepared.plan),
            QueryPlanMetricKind::CompositeIndex
        );

        let store_documents = query_documents_for_read_surface_prepared_cancellable(
            &store,
            &prepared,
            &principal,
            &mut || Ok(()),
        )
        .expect("query should evaluate against sqlite store");
        assert_eq!(
            store_documents
                .iter()
                .map(|document| document.fields["rank"].clone())
                .collect::<Vec<_>>(),
            vec![json!(2), json!(4)]
        );

        let snapshot = store.read_snapshot().expect("sqlite snapshot should open");
        let (plan_kind, snapshot_documents) =
            query_documents_for_snapshot_and_principal_cancellable(
                &snapshot,
                &query,
                schema.get_table(&query.table),
                &principal,
                &mut || Ok(()),
            )
            .expect("query should evaluate against sqlite snapshot");
        assert_eq!(plan_kind, QueryPlanMetricKind::CompositeIndex);
        assert_eq!(
            snapshot_documents
                .iter()
                .map(|document| document.fields["rank"].clone())
                .collect::<Vec<_>>(),
            vec![json!(2), json!(4)]
        );
    }

    #[test]
    fn sqlite_paginated_query_uses_generic_read_surface() {
        let (_dir, store, schema) = seeded_sqlite_store();
        let principal = PrincipalContext::anonymous();
        let paginated = PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: vec![Filter {
                    field: "status".to_string(),
                    op: FilterOp::Eq,
                    value: json!("open"),
                }],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 1,
            after: None,
        };

        let first_page =
            paginate_documents_for_store_with_principal(&store, &schema, &paginated, &principal)
                .expect("first page should evaluate");
        assert_eq!(first_page.data.len(), 1);
        assert_eq!(first_page.data[0]["rank"], json!(1));
        assert!(first_page.has_more);
        let cursor = first_page
            .next_cursor
            .clone()
            .expect("cursor should be present");

        let second_page = paginate_documents_for_store_with_principal(
            &store,
            &schema,
            &PaginatedQuery {
                after: Some(cursor),
                ..paginated
            },
            &principal,
        )
        .expect("second page should evaluate");
        assert_eq!(second_page.data.len(), 1);
        assert_eq!(second_page.data[0]["rank"], json!(2));
    }
}
