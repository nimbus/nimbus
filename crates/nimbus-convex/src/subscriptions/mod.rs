use crate::*;

mod transforms;
mod types;

pub use transforms::{
    activate_transform, apply_builtin_transform, clear_pending_transform, is_scalar_filter_value,
    remove_subscription_transform, resolve_subscription_transform, set_pending_transform,
    should_replace_lower_bound, should_replace_upper_bound, subscription_plan_for_named_query,
    update_runtime_transform_read_set,
};
pub use types::*;

#[derive(Debug, Clone)]
pub struct ConvexSubscriptionEvent<'a> {
    pub subscription_id: u64,
    pub request_id: Option<&'a str>,
    pub commit: Option<&'a CommitEntry>,
    pub deleted_documents: &'a [nimbus_core::Document],
}
