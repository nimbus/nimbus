use super::execution::next_runtime_server_request_id;
use super::*;

mod transforms;

pub use transforms::{RuntimeTransformContext, apply_subscription_transform};

pub fn next_runtime_subscription_server_request_id(prefix: &str) -> String {
    next_runtime_server_request_id(prefix)
}

pub(crate) fn is_scalar_filter_value(value: &Value) -> bool {
    transforms::is_scalar_filter_value(value)
}

pub(crate) fn should_replace_lower_bound(
    current: Option<&Value>,
    candidate: Option<&Value>,
    candidate_inclusive: bool,
) -> bool {
    transforms::should_replace_lower_bound(current, candidate, candidate_inclusive)
}

pub(crate) fn should_replace_upper_bound(
    current: Option<&Value>,
    candidate: Option<&Value>,
    candidate_inclusive: bool,
) -> bool {
    transforms::should_replace_upper_bound(current, candidate, candidate_inclusive)
}
