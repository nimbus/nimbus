use nimbus_core::{IndexDefinition, Query};

use super::QueryPlan;

#[derive(Debug, Clone)]
pub(super) struct PlanCandidate {
    pub(super) plan: QueryPlan,
    pub(super) consumed_fields: usize,
    pub(super) supports_requested_order: bool,
    pub(super) exact_prefix_len: usize,
    pub(super) prefer_exact: bool,
}

impl PlanCandidate {
    pub(super) fn score(&self) -> (usize, bool, usize, bool) {
        (
            self.consumed_fields,
            self.supports_requested_order,
            self.exact_prefix_len,
            self.prefer_exact,
        )
    }
}

pub(super) fn choose_better_plan(current: &mut Option<PlanCandidate>, candidate: PlanCandidate) {
    if current
        .as_ref()
        .is_none_or(|existing| candidate.score() > existing.score())
    {
        *current = Some(candidate);
    }
}

pub(super) fn index_supports_requested_order(
    index: &IndexDefinition,
    exact_prefix_len: usize,
    query: &Query,
) -> bool {
    let Some(order) = &query.order else {
        return false;
    };
    index
        .fields
        .get(exact_prefix_len)
        .is_some_and(|field| field == &order.field)
}
