use std::cmp::Ordering;

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
    pub(super) fn score(&self) -> PlanScore {
        PlanScore {
            consumed_fields: self.consumed_fields,
            supports_requested_order: self.supports_requested_order,
            exact_prefix_len: self.exact_prefix_len,
            prefer_exact: self.prefer_exact,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PlanScore {
    consumed_fields: usize,
    supports_requested_order: bool,
    exact_prefix_len: usize,
    prefer_exact: bool,
}

impl Ord for PlanScore {
    fn cmp(&self, other: &Self) -> Ordering {
        // Ranking priority: consume more indexed fields, preserve requested
        // order, prefer longer exact prefixes, then exact scans over range scans.
        self.consumed_fields
            .cmp(&other.consumed_fields)
            .then_with(|| {
                self.supports_requested_order
                    .cmp(&other.supports_requested_order)
            })
            .then_with(|| self.exact_prefix_len.cmp(&other.exact_prefix_len))
            .then_with(|| self.prefer_exact.cmp(&other.prefer_exact))
    }
}

impl PartialOrd for PlanScore {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn score(
        consumed_fields: usize,
        supports_requested_order: bool,
        exact_prefix_len: usize,
        prefer_exact: bool,
    ) -> PlanScore {
        PlanScore {
            consumed_fields,
            supports_requested_order,
            exact_prefix_len,
            prefer_exact,
        }
    }

    #[test]
    fn plan_score_orders_priority_fields_explicitly() {
        assert!(score(2, false, 0, false) > score(1, true, 99, true));
        assert!(score(1, true, 0, false) > score(1, false, 99, true));
        assert!(score(1, true, 2, false) > score(1, true, 1, true));
        assert!(score(1, true, 2, true) > score(1, true, 2, false));
    }
}
