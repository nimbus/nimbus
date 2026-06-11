use std::cmp::Ordering;

use serde_json::{Number, Value};

fn compare_index_values(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Null, Value::Null) => Some(Ordering::Equal),
        (Value::Bool(left), Value::Bool(right)) => Some(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => compare_index_numbers(left, right),
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn compare_index_numbers(left: &Number, right: &Number) -> Option<Ordering> {
    match (json_integer(left), json_integer(right)) {
        (Some(left), Some(right)) => Some(left.cmp(&right)),
        (Some(left), None) => compare_integer_to_float(left, right.as_f64()?),
        (None, Some(right)) => {
            compare_integer_to_float(right, left.as_f64()?).map(Ordering::reverse)
        }
        (None, None) => left.as_f64()?.partial_cmp(&right.as_f64()?),
    }
}

fn json_integer(number: &Number) -> Option<i128> {
    number
        .as_i64()
        .map(i128::from)
        .or_else(|| number.as_u64().map(i128::from))
}

fn compare_integer_to_float(integer: i128, float: f64) -> Option<Ordering> {
    if !float.is_finite() {
        return None;
    }
    if float < i64::MIN as f64 {
        return Some(Ordering::Greater);
    }
    if float >= 18_446_744_073_709_551_616.0 {
        return Some(Ordering::Less);
    }
    if float.fract() == 0.0 {
        let float_integer = float.trunc() as i128;
        return Some(integer.cmp(&float_integer));
    }
    let float_floor = float.floor() as i128;
    if integer <= float_floor {
        Some(Ordering::Less)
    } else {
        Some(Ordering::Greater)
    }
}

pub fn is_scalar_filter_value(value: &Value) -> bool {
    value.is_null() || value.is_boolean() || value.is_number() || value.is_string()
}

pub fn should_replace_lower_bound(
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
        Some(Ordering::Greater) => true,
        Some(Ordering::Equal) => candidate_inclusive,
        Some(Ordering::Less) | None => false,
    }
}

pub fn should_replace_upper_bound(
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
        Some(Ordering::Less) => true,
        Some(Ordering::Equal) => candidate_inclusive,
        Some(Ordering::Greater) | None => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn incomparable_candidates_do_not_replace_existing_bounds() {
        let number = json!(10);
        let string = json!("10");

        assert!(!should_replace_lower_bound(
            Some(&number),
            Some(&string),
            true
        ));
        assert!(!should_replace_upper_bound(
            Some(&number),
            Some(&string),
            true
        ));
        assert!(should_replace_lower_bound(None, Some(&string), true));
        assert!(should_replace_upper_bound(None, Some(&string), true));
    }

    #[test]
    fn large_integer_bounds_compare_without_f64_precision_loss() {
        let lower = json!(9_007_199_254_740_992_i64);
        let higher = json!(9_007_199_254_740_993_i64);

        assert!(should_replace_lower_bound(
            Some(&lower),
            Some(&higher),
            true
        ));
        assert!(!should_replace_lower_bound(
            Some(&higher),
            Some(&lower),
            true
        ));
        assert!(should_replace_upper_bound(
            Some(&higher),
            Some(&lower),
            true
        ));
        assert!(!should_replace_upper_bound(
            Some(&lower),
            Some(&higher),
            true
        ));
    }

    #[test]
    fn mixed_integer_float_bounds_compare_by_numeric_value() {
        let integer = json!(9_007_199_254_740_993_i64);
        let rounded_float = json!(9_007_199_254_740_992.0_f64);
        let fractional_float = json!(41.5_f64);
        let integer_above_fraction = json!(42);

        assert!(should_replace_lower_bound(
            Some(&rounded_float),
            Some(&integer),
            true
        ));
        assert!(!should_replace_lower_bound(
            Some(&integer),
            Some(&rounded_float),
            true
        ));
        assert!(should_replace_lower_bound(
            Some(&fractional_float),
            Some(&integer_above_fraction),
            true
        ));
        assert!(should_replace_upper_bound(
            Some(&integer_above_fraction),
            Some(&fractional_float),
            true
        ));
    }
}
