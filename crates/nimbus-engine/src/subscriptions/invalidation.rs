use std::sync::atomic::Ordering;

use super::SubscriptionRegistry;
use super::delivery::SubscriptionUpdate;

#[derive(Debug)]
struct PendingPolicyRevisionTerminationRecord {
    subscription_id: u64,
    previous_last_delivered_sequence: u64,
}

/// Subscriptions marked terminal before a policy-revision commit is published.
///
/// The mark phase makes stale-policy subscriptions invisible to new dependency
/// scans and makes in-flight delivery workers observe their work as stale. The
/// schema owner either restores these subscriptions if the commit fails, or
/// finalizes them with a terminal error after the runtime schema is replaced.
#[derive(Debug, Default)]
pub struct PendingPolicyRevisionTermination {
    records: Vec<PendingPolicyRevisionTerminationRecord>,
}

impl PendingPolicyRevisionTermination {
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

impl SubscriptionRegistry {
    /// Marks subscriptions on the provided table stale before publishing a
    /// policy-revision change.
    pub fn begin_policy_revision_mismatches(
        &self,
        table: &nimbus_core::TableName,
        current_policy_revision: &str,
    ) -> PendingPolicyRevisionTermination {
        let mut records = Vec::new();
        let mut subscriptions = self
            .state
            .subscriptions
            .write()
            .expect("subscription lock should not be poisoned");
        for subscription in subscriptions.values_mut() {
            let is_stale = subscription.active
                && &subscription.query.table == table
                && subscription.policy_revision != current_policy_revision;
            if is_stale {
                records.push(PendingPolicyRevisionTerminationRecord {
                    subscription_id: subscription.id,
                    previous_last_delivered_sequence: subscription
                        .last_delivered_sequence
                        .swap(u64::MAX, Ordering::AcqRel),
                });
                subscription.active = false;
            }
        }
        PendingPolicyRevisionTermination { records }
    }

    /// Restores subscriptions that were pre-marked for a policy change that did
    /// not commit.
    pub fn restore_policy_revision_mismatches(&self, pending: PendingPolicyRevisionTermination) {
        if pending.is_empty() {
            return;
        }
        let mut subscriptions = self
            .state
            .subscriptions
            .write()
            .expect("subscription lock should not be poisoned");
        for record in pending.records {
            if let Some(subscription) = subscriptions.get_mut(&record.subscription_id) {
                subscription.active = true;
                subscription
                    .last_delivered_sequence
                    .store(record.previous_last_delivered_sequence, Ordering::Release);
            }
        }
    }

    /// Removes pre-marked policy-revision mismatches and sends their terminal
    /// authorization error.
    pub fn finish_policy_revision_mismatches(
        &self,
        pending: PendingPolicyRevisionTermination,
        message: impl Into<String>,
    ) {
        if pending.is_empty() {
            return;
        }
        let message = message.into();
        let mut removed = Vec::new();
        {
            let mut subscriptions = self
                .state
                .subscriptions
                .write()
                .expect("subscription lock should not be poisoned");
            for record in pending.records {
                if let Some(subscription) = subscriptions.remove(&record.subscription_id) {
                    removed.push((subscription.id, subscription.sender));
                }
            }
        }

        for (subscription_id, sender) in removed {
            let _ = sender.try_send(SubscriptionUpdate::Error {
                subscription_id,
                request_id: None,
                message: message.clone(),
            });
        }
    }

    /// Sends a terminal error to subscriptions on the provided table that were
    /// registered under an outdated access-policy revision, then removes them.
    pub fn terminate_policy_revision_mismatches(
        &self,
        table: &nimbus_core::TableName,
        current_policy_revision: &str,
        message: impl Into<String>,
    ) {
        let pending = self.begin_policy_revision_mismatches(table, current_policy_revision);
        self.finish_policy_revision_mismatches(pending, message);
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
