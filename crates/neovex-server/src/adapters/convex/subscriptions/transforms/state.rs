use std::sync::RwLock;

use super::super::types::{ConvexSubscriptionTransform, ConvexSubscriptionTransforms};

pub(in crate::adapters::convex::subscriptions) fn set_pending_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    request_id: String,
    transform: ConvexSubscriptionTransform,
) {
    transforms
        .write()
        .expect("convex subscription transform lock should not be poisoned")
        .by_request
        .insert(request_id, transform);
}

pub(in crate::adapters::convex::subscriptions) fn activate_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    subscription_id: u64,
    request_id: &str,
    transform: ConvexSubscriptionTransform,
) {
    let mut transforms = transforms
        .write()
        .expect("convex subscription transform lock should not be poisoned");
    transforms.by_request.remove(request_id);
    transforms.by_id.insert(subscription_id, transform);
}

pub(in crate::adapters::convex::subscriptions) fn clear_pending_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    request_id: &str,
) {
    transforms
        .write()
        .expect("convex subscription transform lock should not be poisoned")
        .by_request
        .remove(request_id);
}

pub(in crate::adapters::convex::subscriptions) fn remove_subscription_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    subscription_id: u64,
) {
    transforms
        .write()
        .expect("convex subscription transform lock should not be poisoned")
        .by_id
        .remove(&subscription_id);
}

pub(in crate::adapters::convex::subscriptions) fn update_runtime_transform_read_set(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    subscription_id: u64,
    transform: ConvexSubscriptionTransform,
) {
    transforms
        .write()
        .expect("convex subscription transform lock should not be poisoned")
        .by_id
        .insert(subscription_id, transform);
}
