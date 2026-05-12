use std::cmp::Ordering;

use nimbus_core::{Filter, FilterOp, Query, Result, TableSchema};
use nimbus_storage::index::encode_index_value;
use serde_json::Value;

use super::exact::{collect_exact_prefix, matches_exact_prefix_filter};
use super::scoring::{PlanCandidate, choose_better_plan, index_supports_requested_order};
use super::{
    PlannedRangeBound, PlannedRangeBoundRef, QueryPlan, RangeIndexPlan, RangeSide, RangeValueKind,
};

pub(super) fn plan_range_index_scan(
    query: &Query,
    table_schema: &TableSchema,
) -> Result<Option<PlanCandidate>> {
    let mut best = None;
    for index in &table_schema.indexes {
        let exact_prefix = collect_exact_prefix(query, index);
        let Some(range_field) = index.fields.get(exact_prefix.len()) else {
            continue;
        };
        let mut kind = None;
        let mut lower = None;
        let mut upper = None;
        let mut unusable = false;

        for filter in query
            .filters
            .iter()
            .filter(|filter| filter.field == *range_field)
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

        let residual_filters = query.filters.iter().try_fold(
            Vec::new(),
            |mut residual_filters, candidate| -> Result<Vec<Filter>> {
                let exact_satisfied = exact_prefix
                    .iter()
                    .any(|satisfied| matches_exact_prefix_filter(candidate, satisfied));
                let range_satisfied = filter_satisfied_by_range_plan(
                    candidate,
                    range_field,
                    lower.as_ref(),
                    upper.as_ref(),
                )?;
                if !exact_satisfied && !range_satisfied {
                    residual_filters.push(candidate.clone());
                }
                Ok(residual_filters)
            },
        )?;
        let candidate = PlanCandidate {
            plan: QueryPlan::RangeIndex(Box::new(RangeIndexPlan {
                index_name: index.name.clone(),
                is_composite_index: index.fields.len() > 1,
                exact_prefix: exact_prefix.clone(),
                range_field: range_field.clone(),
                lower,
                upper,
                residual_filters,
            })),
            consumed_fields: exact_prefix.len() + 1,
            supports_requested_order: index_supports_requested_order(
                index,
                exact_prefix.len(),
                query,
            ),
            exact_prefix_len: exact_prefix.len(),
            prefer_exact: false,
        };
        choose_better_plan(&mut best, candidate);
    }

    Ok(best)
}

fn filter_satisfied_by_range_plan(
    candidate: &Filter,
    range_field: &str,
    lower: Option<&PlannedRangeBound>,
    upper: Option<&PlannedRangeBound>,
) -> Result<bool> {
    if candidate.field != range_field {
        return Ok(false);
    }
    let Some(bound) = range_bound_from_filter(candidate)? else {
        return Ok(false);
    };

    Ok(match bound.side {
        RangeSide::Lower => lower.is_some_and(|selected| {
            compare_lower_bounds(selected.as_ref(), bound.as_ref()) != Ordering::Less
        }),
        RangeSide::Upper => upper.is_some_and(|selected| {
            compare_upper_bounds(selected.as_ref(), bound.as_ref()) != Ordering::Greater
        }),
    })
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
