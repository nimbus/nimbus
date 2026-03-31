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

    /// Fetches a single document in a logical table.
    pub fn get_document(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        document_id: DocumentId,
    ) -> Result<Document> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime
            .store
            .get(table, &document_id)?
            .ok_or(Error::DocumentNotFound(document_id))
    }

    /// Evaluates a query for a tenant.
    pub fn query_documents(&self, tenant_id: &TenantId, query: &Query) -> Result<Vec<Document>> {
        self.query_documents_cancellable(tenant_id, query, &mut || Ok(()))
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

    /// Evaluates a paginated query for a tenant while checking for cancellation between rows.
    pub fn paginate_documents_cancellable(
        &self,
        tenant_id: &TenantId,
        query: &PaginatedQuery,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Page> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        if let Some(index_docs) = try_index_scan(&runtime, &query.query, check_cancel)? {
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

    /// Returns the latest committed sequence number for a tenant.
    pub fn latest_sequence(&self, tenant_id: &TenantId) -> Result<SequenceNumber> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.latest_sequence()
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
    if let Some(plan) = try_index_query_plan(runtime, query, check_cancel)? {
        evaluate_query_with_docs_cancellable(plan.documents, &plan.query, check_cancel)
    } else {
        evaluate_query_cancellable(&runtime.store, query, check_cancel)
    }
}

fn try_index_scan(
    runtime: &TenantRuntime,
    query: &Query,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Option<Vec<Document>>> {
    let schema = runtime.schema();
    let Some(table_schema) = schema.get_table(&query.table) else {
        return Ok(None);
    };

    if let Some(documents) = try_index_eq_scan(runtime, query, table_schema, check_cancel)? {
        return Ok(Some(documents));
    }

    try_index_range_scan(runtime, query, table_schema, check_cancel)
}

fn try_index_query_plan(
    runtime: &TenantRuntime,
    query: &Query,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Option<PlannedQuery>> {
    let schema = runtime.schema();
    let Some(table_schema) = schema.get_table(&query.table) else {
        return Ok(None);
    };

    if let Some(plan) = try_index_eq_query_plan(runtime, query, table_schema, check_cancel)? {
        return Ok(Some(plan));
    }

    if let Some(documents) = try_index_range_scan(runtime, query, table_schema, check_cancel)? {
        return Ok(Some(PlannedQuery {
            documents,
            query: query.clone(),
        }));
    }

    Ok(None)
}

fn try_index_eq_scan(
    runtime: &TenantRuntime,
    query: &Query,
    table_schema: &neovex_core::TableSchema,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Option<Vec<Document>>> {
    for filter in &query.filters {
        let is_scalar_value = filter.value.is_null()
            || filter.value.is_boolean()
            || filter.value.is_number()
            || filter.value.is_string();
        if filter.op == FilterOp::Eq
            && is_scalar_value
            && let Some(index) = table_schema
                .indexes
                .iter()
                .find(|index| index.field == filter.field)
        {
            let documents = runtime.store.index_scan_eq_cancellable(
                &query.table,
                &index.name,
                &filter.value,
                check_cancel,
            )?;
            return Ok(Some(documents));
        }
    }

    Ok(None)
}

fn try_index_eq_query_plan(
    runtime: &TenantRuntime,
    query: &Query,
    table_schema: &neovex_core::TableSchema,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Option<PlannedQuery>> {
    for filter in &query.filters {
        let is_scalar_value = filter.value.is_null()
            || filter.value.is_boolean()
            || filter.value.is_number()
            || filter.value.is_string();
        if filter.op == FilterOp::Eq
            && is_scalar_value
            && let Some(index) = table_schema
                .indexes
                .iter()
                .find(|index| index.field == filter.field)
        {
            let documents = runtime.store.index_scan_eq_cancellable(
                &query.table,
                &index.name,
                &filter.value,
                check_cancel,
            )?;
            let mut residual_query = query.clone();
            residual_query
                .filters
                .retain(|candidate| !matches_eq_filter(candidate, filter));
            return Ok(Some(PlannedQuery {
                documents,
                query: residual_query,
            }));
        }
    }

    Ok(None)
}

fn try_index_range_scan(
    runtime: &TenantRuntime,
    query: &Query,
    table_schema: &neovex_core::TableSchema,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Option<Vec<Document>>> {
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

        let documents = runtime.store.index_scan_range_cancellable(
            &query.table,
            &index.name,
            lower.as_ref().map(|bound| &bound.value),
            upper.as_ref().map(|bound| &bound.value),
            lower.as_ref().is_none_or(|bound| bound.inclusive),
            upper.as_ref().is_none_or(|bound| bound.inclusive),
            check_cancel,
        )?;
        return Ok(Some(documents));
    }

    Ok(None)
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

#[derive(Debug, Clone)]
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

struct PlannedQuery {
    documents: Vec<Document>,
    query: Query,
}

fn matches_eq_filter(candidate: &Filter, satisfied: &Filter) -> bool {
    candidate.op == FilterOp::Eq
        && satisfied.op == FilterOp::Eq
        && candidate.field == satisfied.field
        && candidate.value == satisfied.value
}
