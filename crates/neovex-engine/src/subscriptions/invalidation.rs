use super::SubscriptionRegistry;
use super::delivery::SubscriptionUpdate;

impl SubscriptionRegistry {
    /// Sends a terminal error to subscriptions on the provided table that were
    /// registered under an outdated access-policy revision, then removes them.
    pub fn terminate_policy_revision_mismatches(
        &self,
        table: &neovex_core::TableName,
        current_policy_revision: &str,
        message: impl Into<String>,
    ) {
        let message = message.into();
        let mut removed = Vec::new();
        {
            let mut subscriptions = self
                .state
                .subscriptions
                .write()
                .expect("subscription lock should not be poisoned");
            subscriptions.retain(|_, subscription| {
                let is_stale = &subscription.query.table == table
                    && subscription.policy_revision != current_policy_revision;
                if is_stale {
                    removed.push((subscription.id, subscription.sender.clone()));
                }
                !is_stale
            });
        }

        for (subscription_id, sender) in removed {
            let _ = sender.try_send(SubscriptionUpdate::Error {
                subscription_id,
                request_id: None,
                message: message.clone(),
            });
        }
    }

    /// Sends a terminal error to all subscriptions and removes them.
    pub fn shutdown_all(&self, message: impl Into<String>) {
        let message = message.into();
        let subscriptions = std::mem::take(
            &mut *self
                .state
                .subscriptions
                .write()
                .expect("subscription lock should not be poisoned"),
        );

        for subscription in subscriptions.into_values() {
            let _ = subscription.sender.try_send(SubscriptionUpdate::Error {
                subscription_id: subscription.id,
                request_id: None,
                message: message.clone(),
            });
        }
    }
}
