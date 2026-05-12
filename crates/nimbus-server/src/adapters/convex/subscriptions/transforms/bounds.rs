use serde_json::Value;

fn compare_index_values(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Null, Value::Null) => Some(std::cmp::Ordering::Equal),
        (Value::Bool(left), Value::Bool(right)) => Some(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .zip(right.as_f64())
            .and_then(|(left, right)| left.partial_cmp(&right)),
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

pub(in crate::adapters::convex::subscriptions) fn is_scalar_filter_value(value: &Value) -> bool {
    value.is_null() || value.is_boolean() || value.is_number() || value.is_string()
}

pub(in crate::adapters::convex::subscriptions) fn should_replace_lower_bound(
    current: Option<&Value>,
    candidate: Option<&Value>,
    candidate_inclusive: bool,
) -> bool {
    let Some(candidate) = candidate else {
        return false;
    };
    let Some(current) = current else {
        return true;
    };
    match compare_index_values(candidate, current) {
        Some(std::cmp::Ordering::Greater) => true,
        Some(std::cmp::Ordering::Equal) => candidate_inclusive,
        Some(std::cmp::Ordering::Less) => false,
        None => true,
    }
}

pub(in crate::adapters::convex::subscriptions) fn should_replace_upper_bound(
    current: Option<&Value>,
    candidate: Option<&Value>,
    candidate_inclusive: bool,
) -> bool {
    let Some(candidate) = candidate else {
        return false;
    };
    let Some(current) = current else {
        return true;
    };
    match compare_index_values(candidate, current) {
        Some(std::cmp::Ordering::Less) => true,
        Some(std::cmp::Ordering::Equal) => candidate_inclusive,
        Some(std::cmp::Ordering::Greater) => false,
        None => true,
    }
}
