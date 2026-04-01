use std::sync::RwLock;

use super::super::super::types::{ConvexSubscriptionTransform, ConvexSubscriptionTransforms};
use crate::adapters::convex::execution::ConvexSubscriptionEvent;

pub(in crate::adapters::convex::subscriptions) fn resolve_subscription_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    event: &ConvexSubscriptionEvent<'_>,
) -> ConvexSubscriptionTransform {
    let mut transforms = transforms
        .write()
        .expect("convex subscription transform lock should not be poisoned");
    if let Some(transform) = transforms.by_id.get(&event.subscription_id).cloned() {
        transform
    } else if let Some(request_id) = event.request_id {
        if let Some(transform) = transforms.by_request.remove(request_id) {
            transforms
                .by_id
                .insert(event.subscription_id, transform.clone());
            transform
        } else {
            ConvexSubscriptionTransform::Identity
        }
    } else {
        ConvexSubscriptionTransform::Identity
    }
}
