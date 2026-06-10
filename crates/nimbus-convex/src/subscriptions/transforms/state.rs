use std::sync::{RwLock, RwLockWriteGuard};

use super::super::types::{ConvexSubscriptionTransform, ConvexSubscriptionTransforms};

pub(crate) fn write_transform_state(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
) -> RwLockWriteGuard<'_, ConvexSubscriptionTransforms> {
    transforms
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn set_pending_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    request_id: String,
    transform: ConvexSubscriptionTransform,
) {
    write_transform_state(transforms)
        .by_request
        .insert(request_id, transform);
}

pub fn activate_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    subscription_id: u64,
    request_id: &str,
    transform: ConvexSubscriptionTransform,
) {
    let mut transforms = write_transform_state(transforms);
    transforms.by_request.remove(request_id);
    transforms.by_id.insert(subscription_id, transform);
}

pub fn clear_pending_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    request_id: &str,
) {
    write_transform_state(transforms)
        .by_request
        .remove(request_id);
}

pub fn remove_subscription_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    subscription_id: u64,
) {
    write_transform_state(transforms)
        .by_id
        .remove(&subscription_id);
}

pub fn update_runtime_transform_read_set(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    subscription_id: u64,
    transform: ConvexSubscriptionTransform,
) {
    write_transform_state(transforms)
        .by_id
        .insert(subscription_id, transform);
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::RwLockReadGuard;

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

    fn read_transform_state(
        transforms: &RwLock<ConvexSubscriptionTransforms>,
    ) -> RwLockReadGuard<'_, ConvexSubscriptionTransforms> {
        transforms
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn transform_state_mutators_recover_after_poisoned_lock() {
        let transforms = RwLock::new(ConvexSubscriptionTransforms::default());
        poison_transform_state(&transforms);

        set_pending_transform(
            &transforms,
            "request-1".to_string(),
            ConvexSubscriptionTransform::Identity,
        );
        {
            let transforms = read_transform_state(&transforms);
            assert!(matches!(
                transforms.by_request.get("request-1"),
                Some(ConvexSubscriptionTransform::Identity)
            ));
        }

        activate_transform(
            &transforms,
            41,
            "request-1",
            ConvexSubscriptionTransform::First,
        );
        {
            let transforms = read_transform_state(&transforms);
            assert!(!transforms.by_request.contains_key("request-1"));
            assert!(matches!(
                transforms.by_id.get(&41),
                Some(ConvexSubscriptionTransform::First)
            ));
        }

        set_pending_transform(
            &transforms,
            "request-2".to_string(),
            ConvexSubscriptionTransform::Unique,
        );
        clear_pending_transform(&transforms, "request-2");
        {
            let transforms = read_transform_state(&transforms);
            assert!(!transforms.by_request.contains_key("request-2"));
        }

        update_runtime_transform_read_set(&transforms, 41, ConvexSubscriptionTransform::Unique);
        {
            let transforms = read_transform_state(&transforms);
            assert!(matches!(
                transforms.by_id.get(&41),
                Some(ConvexSubscriptionTransform::Unique)
            ));
        }

        remove_subscription_transform(&transforms, 41);
        {
            let transforms = read_transform_state(&transforms);
            assert!(!transforms.by_id.contains_key(&41));
        }
    }
}
