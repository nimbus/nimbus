use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryPlanMetricKind {
    FullScan,
    SingleFieldIndex,
    CompositeIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryPlanMetricOperation {
    Query,
    Paginated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct QueryPlanningStats {
    pub query_full_scan_count: u64,
    pub query_single_field_index_count: u64,
    pub query_composite_index_count: u64,
    pub paginated_full_scan_count: u64,
    pub paginated_single_field_index_count: u64,
    pub paginated_composite_index_count: u64,
}

pub(super) struct QueryPlanningMetrics {
    query_full_scan_count: AtomicU64,
    query_single_field_index_count: AtomicU64,
    query_composite_index_count: AtomicU64,
    paginated_full_scan_count: AtomicU64,
    paginated_single_field_index_count: AtomicU64,
    paginated_composite_index_count: AtomicU64,
}

impl QueryPlanningMetrics {
    pub(super) fn new() -> Self {
        Self {
            query_full_scan_count: AtomicU64::new(0),
            query_single_field_index_count: AtomicU64::new(0),
            query_composite_index_count: AtomicU64::new(0),
            paginated_full_scan_count: AtomicU64::new(0),
            paginated_single_field_index_count: AtomicU64::new(0),
            paginated_composite_index_count: AtomicU64::new(0),
        }
    }

    pub(super) fn record(&self, operation: QueryPlanMetricOperation, kind: QueryPlanMetricKind) {
        let counter = match (operation, kind) {
            (QueryPlanMetricOperation::Query, QueryPlanMetricKind::FullScan) => {
                &self.query_full_scan_count
            }
            (QueryPlanMetricOperation::Query, QueryPlanMetricKind::SingleFieldIndex) => {
                &self.query_single_field_index_count
            }
            (QueryPlanMetricOperation::Query, QueryPlanMetricKind::CompositeIndex) => {
                &self.query_composite_index_count
            }
            (QueryPlanMetricOperation::Paginated, QueryPlanMetricKind::FullScan) => {
                &self.paginated_full_scan_count
            }
            (QueryPlanMetricOperation::Paginated, QueryPlanMetricKind::SingleFieldIndex) => {
                &self.paginated_single_field_index_count
            }
            (QueryPlanMetricOperation::Paginated, QueryPlanMetricKind::CompositeIndex) => {
                &self.paginated_composite_index_count
            }
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn stats(&self) -> QueryPlanningStats {
        QueryPlanningStats {
            query_full_scan_count: self.query_full_scan_count.load(Ordering::Relaxed),
            query_single_field_index_count: self
                .query_single_field_index_count
                .load(Ordering::Relaxed),
            query_composite_index_count: self.query_composite_index_count.load(Ordering::Relaxed),
            paginated_full_scan_count: self.paginated_full_scan_count.load(Ordering::Relaxed),
            paginated_single_field_index_count: self
                .paginated_single_field_index_count
                .load(Ordering::Relaxed),
            paginated_composite_index_count: self
                .paginated_composite_index_count
                .load(Ordering::Relaxed),
        }
    }
}
