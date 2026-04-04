use neovex_core::{Document, Filter, FilterOp, Query, Result};
use neovex_storage::{TenantReadSnapshot, TenantStore};
use serde_json::Value;

use crate::evaluator::matches_filters;

use super::{PlannedExactMatch, PlannedRangeBound, QueryPlan};

pub(super) fn load_query_plan_documents_cancellable(
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

pub(super) fn load_query_plan_documents_from_snapshot_cancellable(
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

pub(super) fn load_query_plan_documents_from_docs(
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
