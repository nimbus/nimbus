use std::future::{Future, pending};
use std::sync::Arc;

use std::cmp::Ordering;

use neovex_core::{
    CommitEntry, Document, DocumentId, Error, Filter, FilterOp, Page, PaginatedQuery, Query,
    Result, SequenceNumber, TableName, TenantId,
};
use neovex_storage::index::encode_index_value;
use serde_json::Value;

use crate::evaluator::{
    evaluate_paginated_cancellable, evaluate_paginated_with_docs_cancellable,
    evaluate_query_cancellable, evaluate_query_with_docs_cancellable,
};
use crate::tenant::TenantRuntime;

use super::Service;

impl Service {
    /// Lists documents in a logical table.
    pub fn list_documents(&self, tenant_id: &TenantId, table: &TableName) -> Result<Vec<Document>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.scan_table(table)
    }

    /// Lists documents in a logical table asynchronously.
    pub async fn list_documents_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
    ) -> Result<Vec<Document>> {
        self.list_documents_async_cancellable(tenant_id, table, pending(), || Ok(()))
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
        self.call_blocking_cancellable(cancel_wait, move |service| {
            let runtime = service.get_existing_tenant(&tenant_id)?;
            let _operation = runtime.enter_operation(&tenant_id)?;
            let mut check_cancel = || check_cancel();
            runtime
                .store
                .scan_table_cancellable(&table, &mut check_cancel)
        })
        .await
    }

    /// Fetches a single document in a logical table.
    pub fn get_document(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        document_id: DocumentId,
    ) -> Result<Document> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        if let Some(document) = runtime.get_cached_document(table, document_id) {
            return Ok(document);
        }

        let document = runtime
            .store
            .get(table, &document_id)?
            .ok_or(Error::DocumentNotFound(document_id))?;
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
        self.call_blocking(move |service| service.get_document(&tenant_id, &table, document_id))
            .await
    }

    /// Evaluates a query for a tenant.
    pub fn query_documents(&self, tenant_id: &TenantId, query: &Query) -> Result<Vec<Document>> {
        self.query_documents_cancellable(tenant_id, query, &mut || Ok(()))
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
        self.call_blocking_cancellable(cancel_wait, move |service| {
            let mut check_cancel = || check_cancel();
            service.query_documents_cancellable(&tenant_id, &query, &mut check_cancel)
        })
        .await
    }

    /// Evaluates a query for a tenant while checking for cancellation between rows.
    pub fn query_documents_cancellable(
        &self,
        tenant_id: &TenantId,
        query: &Query,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        evaluate_with_index_cancellable(&runtime, query, check_cancel)
    }

    /// Evaluates a paginated query for a tenant.
    pub fn paginate_documents(&self, tenant_id: &TenantId, query: &PaginatedQuery) -> Result<Page> {
        self.paginate_documents_cancellable(tenant_id, query, &mut || Ok(()))
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
        self.call_blocking_cancellable(cancel_wait, move |service| {
            let mut check_cancel = || check_cancel();
            service.paginate_documents_cancellable(&tenant_id, &query, &mut check_cancel)
        })
        .await
    }

    /// Evaluates a paginated query for a tenant while checking for cancellation between rows.
    pub fn paginate_documents_cancellable(
        &self,
        tenant_id: &TenantId,
        query: &PaginatedQuery,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Page> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let plan = plan_query(&query.query, runtime.schema().get_table(&query.query.table))?;
        if let Some(index_docs) =
            load_query_plan_documents_cancellable(&runtime, &query.query, &plan, check_cancel)?
        {
            evaluate_paginated_with_docs_cancellable(index_docs, query, check_cancel)
        } else {
            evaluate_paginated_cancellable(&runtime.store, query, check_cancel)
        }
    }

    /// Reads commit log entries committed after the provided sequence number.
    pub fn read_commit_log(
        &self,
        tenant_id: &TenantId,
        after: SequenceNumber,
    ) -> Result<Vec<CommitEntry>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let from = SequenceNumber(after.0.saturating_add(1));
        runtime.store.read_commit_log_from(from)
    }

    /// Reads commit log entries asynchronously.
    pub async fn read_commit_log_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        after: SequenceNumber,
    ) -> Result<Vec<CommitEntry>> {
        self.call_blocking(move |service| service.read_commit_log(&tenant_id, after))
            .await
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
        self.call_blocking(move |service| service.latest_sequence(&tenant_id))
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
}

pub(super) fn evaluate_with_index(runtime: &TenantRuntime, query: &Query) -> Result<Vec<Document>> {
    evaluate_with_index_cancellable(runtime, query, &mut || Ok(()))
}

pub(super) fn evaluate_with_index_cancellable(
    runtime: &TenantRuntime,
    query: &Query,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let plan = plan_query(query, runtime.schema().get_table(&query.table))?;
    let documents = if let Some(documents) =
        load_query_plan_documents_cancellable(runtime, query, &plan, check_cancel)?
    {
        let residual_query = plan.residual_query(query);
        evaluate_query_with_docs_cancellable(documents, &residual_query, check_cancel)?
    } else {
        evaluate_query_cancellable(&runtime.store, query, check_cancel)?
    };
    runtime.cache_documents(&documents);
    Ok(documents)
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
    runtime: &TenantRuntime,
    query: &Query,
    plan: &QueryPlan,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Option<Vec<Document>>> {
    match plan {
        QueryPlan::FullScan => Ok(None),
        QueryPlan::ExactIndex {
            index_name, value, ..
        } => {
            let documents = runtime.store.index_scan_eq_cancellable(
                &query.table,
                index_name,
                value,
                check_cancel,
            )?;
            runtime.cache_documents(&documents);
            Ok(Some(documents))
        }
        QueryPlan::RangeIndex {
            index_name,
            lower,
            upper,
            ..
        } => {
            let documents = runtime.store.index_scan_range_cancellable(
                &query.table,
                index_name,
                lower.as_ref().map(|bound| &bound.value),
                upper.as_ref().map(|bound| &bound.value),
                lower.as_ref().is_none_or(|bound| bound.inclusive),
                upper.as_ref().is_none_or(|bound| bound.inclusive),
                check_cancel,
            )?;
            runtime.cache_documents(&documents);
            Ok(Some(documents))
        }
    }
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
