use nimbus_core::{Filter, FilterOp, IndexDefinition, Query, TableSchema};
use serde_json::Value;

use super::scoring::{PlanCandidate, choose_better_plan, index_supports_requested_order};
use super::{PlannedExactMatch, QueryPlan};

pub(super) fn plan_exact_index_scan(
    query: &Query,
    table_schema: &TableSchema,
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

pub(super) fn collect_exact_prefix(
    query: &Query,
    index: &IndexDefinition,
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

pub(super) fn matches_exact_prefix_filter(
    candidate: &Filter,
    satisfied: &PlannedExactMatch,
) -> bool {
    candidate.op == FilterOp::Eq
        && candidate.field == satisfied.field
        && candidate.value == satisfied.value
}

fn is_scalar_index_value(value: &Value) -> bool {
    value.is_null() || value.is_boolean() || value.is_number() || value.is_string()
}
