use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use nimbus_core::{
    CommitEntry, PrincipalContext, Query, SequenceNumber, SubscriptionResultSnapshot,
};
use tokio::sync::mpsc;

use crate::service::evaluate_with_index_cancellable_for_principal;
use crate::tenant::TenantRuntime;

use super::dependencies::subscription_dependencies;
use super::queue::QueuedSubscriptionWork;

/// A subscription event emitted by the engine.
#[derive(Debug, Clone)]
pub enum SubscriptionUpdate {
    Result {
        subscription_id: u64,
        request_id: Option<String>,
        snapshot: SubscriptionResultSnapshot,
        // Keep the exact commit payload available for in-process optimizations.
        // Adapter-facing code should consume `snapshot` instead.
        commit_hint: Option<CommitEntry>,
    },
    Error {
        subscription_id: u64,
        request_id: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub(super) struct SubscriptionDelivery {
    pub(super) id: u64,
    pub(super) query: Query,
    pub(super) principal: PrincipalContext,
    pub(super) sender: mpsc::Sender<SubscriptionUpdate>,
    pub(super) last_delivered_sequence: Arc<AtomicU64>,
}

impl SubscriptionDelivery {
    pub(super) fn is_stale_for_sequence(&self, sequence: SequenceNumber) -> bool {
        self.last_delivered_sequence.load(Ordering::Acquire) >= sequence.0
    }

    pub(super) fn mark_delivered(&self, sequence: SequenceNumber) {
        let mut current = self.last_delivered_sequence.load(Ordering::Acquire);
        while current < sequence.0 {
            match self.last_delivered_sequence.compare_exchange_weak(
                current,
                sequence.0,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SubscriptionDispatchStats {
    pub reevaluation_count: u64,
    pub total_reevaluation_nanos: u64,
    pub coalesced_work_count: u64,
}

pub(crate) fn dispatch_subscription_work(
    runtime: &Arc<TenantRuntime>,
    work: &QueuedSubscriptionWork,
) -> SubscriptionDispatchStats {
    let sequence = work.delivery_sequence;
    let mut stats = SubscriptionDispatchStats::default();

    for subscription_id in &work.subscription_ids {
        let Some(subscription) = runtime.subscriptions.delivery(*subscription_id) else {
            continue;
        };
        if subscription.is_stale_for_sequence(sequence) {
            stats.coalesced_work_count += 1;
            continue;
        }

        let started_at = Instant::now();
        let mut check_cancel = || -> nimbus_core::Result<()> { Ok(()) };
        let result = evaluate_with_index_cancellable_for_principal(
            runtime,
            &subscription.query,
            &subscription.principal,
            sequence,
            &mut check_cancel,
        );
        stats.reevaluation_count += 1;
        let elapsed_nanos = started_at.elapsed().as_nanos();
        stats.total_reevaluation_nanos += elapsed_nanos.min(u128::from(u64::MAX)) as u64;

        if subscription.is_stale_for_sequence(sequence) {
            stats.coalesced_work_count += 1;
            continue;
        }

        match result {
            Ok(documents) => {
                let dependencies = subscription_dependencies(&subscription.query, &documents);
                let snapshot = SubscriptionResultSnapshot::from_delivery(
                    work.delivery_sequence,
                    work.commit.as_ref(),
                    documents,
                    work.deleted_documents.clone(),
                );
                let update = SubscriptionUpdate::Result {
                    subscription_id: subscription.id,
                    request_id: None,
                    snapshot,
                    commit_hint: work.commit.clone(),
                };
                if subscription.sender.try_send(update).is_ok() {
                    runtime
                        .subscriptions
                        .record_delivery(subscription.id, sequence, dependencies);
                } else {
                    runtime.subscriptions.remove(subscription.id);
                }
            }
            Err(error) => {
                tracing::warn!(
                    subscription_id = subscription.id,
                    error = %error,
                    "subscription re-evaluation failed"
                );
                if subscription
                    .sender
                    .try_send(SubscriptionUpdate::Error {
                        subscription_id: subscription.id,
                        request_id: None,
                        message: error.to_string(),
                    })
                    .is_ok()
                {
                    subscription.mark_delivered(sequence);
                } else {
                    runtime.subscriptions.remove(subscription.id);
                }
            }
        }
    }

    stats
}
