use super::*;

impl TenantRuntime {
    pub(crate) fn record_query_plan_metric(
        &self,
        operation: QueryPlanMetricOperation,
        kind: QueryPlanMetricKind,
    ) {
        self.query_planning.record(operation, kind);
    }

    pub(crate) fn query_planning_stats(&self) -> QueryPlanningStats {
        self.query_planning.stats()
    }
}
