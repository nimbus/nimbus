use std::sync::RwLock;

use super::super::super::types::{ConvexSubscriptionTransform, ConvexSubscriptionTransforms};
use super::super::state::write_transform_state;
use crate::ConvexSubscriptionEvent;

pub fn resolve_subscription_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    event: &ConvexSubscriptionEvent<'_>,
) -> ConvexSubscriptionTransform {
    let mut transforms = write_transform_state(transforms);
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

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    fn poison_transform_state(transforms: &RwLock<ConvexSubscriptionTransforms>) {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _guard = transforms
                .write()
                .expect("transform state should start unpoisoned");
            panic!("poison transform state");
        }));
        assert!(result.is_err());
        assert!(transforms.is_poisoned());
    }

    #[test]
    fn resolve_subscription_transform_recovers_poisoned_lock_and_promotes_pending_transform() {
        let transforms = RwLock::new(ConvexSubscriptionTransforms::default());
        transforms
            .write()
            .expect("transform state should start unpoisoned")
            .by_request
            .insert("request-1".to_string(), ConvexSubscriptionTransform::First);
        poison_transform_state(&transforms);

        let event = ConvexSubscriptionEvent {
            subscription_id: 42,
            request_id: Some("request-1"),
            commit: None,
            deleted_documents: &[],
        };
        let transform = resolve_subscription_transform(&transforms, &event);
        assert!(matches!(transform, ConvexSubscriptionTransform::First));

        let transforms = transforms
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(!transforms.by_request.contains_key("request-1"));
        assert!(matches!(
            transforms.by_id.get(&42),
            Some(ConvexSubscriptionTransform::First)
        ));
    }
}
