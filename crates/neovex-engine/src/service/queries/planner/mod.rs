mod exact;
mod loading;
mod range;
mod scoring;

use neovex_core::{Document, Filter, Query, Result, TableSchema};
use neovex_storage::{TenantReadSnapshot, TenantStore};

use crate::tenant::QueryPlanMetricKind;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum QueryPlan {
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
    pub(super) fn residual_query(&self, query: &Query) -> Query {
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

pub(super) fn query_plan_metric_kind(plan: &QueryPlan) -> QueryPlanMetricKind {
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

pub(super) fn plan_query(query: &Query, table_schema: Option<&TableSchema>) -> Result<QueryPlan> {
    plan_query_inner(query, table_schema)
}

pub(super) fn plan_paginated_query(
    query: &Query,
    table_schema: Option<&TableSchema>,
) -> Result<QueryPlan> {
    plan_query_inner(query, table_schema)
}

fn plan_query_inner(query: &Query, table_schema: Option<&TableSchema>) -> Result<QueryPlan> {
    let Some(table_schema) = table_schema else {
        return Ok(QueryPlan::FullScan);
    };

    let exact = exact::plan_exact_index_scan(query, table_schema);
    let range = range::plan_range_index_scan(query, table_schema)?;
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
    value: serde_json::Value,
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
pub(super) struct RangeIndexPlan {
    index_name: String,
    is_composite_index: bool,
    exact_prefix: Vec<PlannedExactMatch>,
    range_field: String,
    lower: Option<PlannedRangeBound>,
    upper: Option<PlannedRangeBound>,
    residual_filters: Vec<Filter>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PlannedExactMatch {
    field: String,
    value: serde_json::Value,
}

pub(super) fn load_query_plan_documents_cancellable(
    store: &TenantStore,
    query: &Query,
    plan: &QueryPlan,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Option<Vec<Document>>> {
    loading::load_query_plan_documents_cancellable(store, query, plan, check_cancel)
}

pub(super) fn load_query_plan_documents_from_snapshot_cancellable(
    snapshot: &TenantReadSnapshot,
    query: &Query,
    plan: &QueryPlan,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Option<Vec<Document>>> {
    loading::load_query_plan_documents_from_snapshot_cancellable(
        snapshot,
        query,
        plan,
        check_cancel,
    )
}

pub(super) fn load_query_plan_documents_from_docs(
    documents: &[Document],
    plan: &QueryPlan,
) -> Result<Option<Vec<Document>>> {
    loading::load_query_plan_documents_from_docs(documents, plan)
}

#[cfg(test)]
mod tests {
    use neovex_core::{FilterOp, IndexDefinition, Query, TableName, TableSchema};
    use serde_json::{Value, json};

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
