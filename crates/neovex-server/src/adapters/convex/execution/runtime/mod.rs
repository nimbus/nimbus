use super::*;
use crate::adapters::convex::subscriptions::types::{
    ConvexRuntimeSubscriptionSetup, ConvexSubscriptionTransform,
};

mod invoke;
mod subscriptions;

pub(in crate::adapters::convex) use invoke::{
    invoke_named_convex_function_async_cancellable,
    invoke_named_convex_function_with_trace_async_cancellable,
};
pub(in crate::adapters::convex) use subscriptions::bootstrap_runtime_named_subscription_async;

fn required_runtime_bundle(registry: &Arc<ConvexRegistry>) -> Result<RuntimeBundle, Error> {
    registry
        .runtime_bundle()
        .cloned()
        .ok_or_else(|| Error::Internal("convex runtime bundle not loaded".to_string()))
}
