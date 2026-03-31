use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use neovex_core::{CommitEntry, Document, Query, TableName};
use serde_json::Value;
use tokio::sync::mpsc;

/// A subscription event emitted by the engine.
#[derive(Debug, Clone)]
pub enum SubscriptionUpdate {
    Result {
        subscription_id: u64,
        request_id: Option<String>,
        commit: Option<CommitEntry>,
        deleted_documents: Vec<Document>,
        data: Vec<Value>,
    },
    Error {
        subscription_id: u64,
        request_id: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone)]
struct Subscription {
    id: u64,
    query: Query,
    table: TableName,
    sender: mpsc::UnboundedSender<SubscriptionUpdate>,
}

#[derive(Debug, Clone)]
pub struct SubscriptionDelivery {
    pub id: u64,
    pub query: Query,
    pub sender: mpsc::UnboundedSender<SubscriptionUpdate>,
}

/// In-memory subscription registry for a tenant.
#[derive(Debug)]
pub struct SubscriptionRegistry {
    next_id: AtomicU64,
    subscriptions: RwLock<HashMap<u64, Subscription>>,
}

impl SubscriptionRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            subscriptions: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a subscription and returns its id.
    pub fn register(&self, query: Query, sender: mpsc::UnboundedSender<SubscriptionUpdate>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let subscription = Subscription {
            id,
            table: query.table.clone(),
            query,
            sender,
        };
        self.subscriptions
            .write()
            .expect("subscription lock should not be poisoned")
            .insert(id, subscription);
        id
    }

    /// Removes a subscription if present.
    pub fn remove(&self, id: u64) {
        self.subscriptions
            .write()
            .expect("subscription lock should not be poisoned")
            .remove(&id);
    }

    /// Returns the subscriptions affected by the provided table set.
    pub fn affected(&self, tables: &HashSet<TableName>) -> Vec<SubscriptionDelivery> {
        self.subscriptions
            .read()
            .expect("subscription lock should not be poisoned")
            .values()
            .filter(|subscription| tables.contains(&subscription.table))
            .map(|subscription| SubscriptionDelivery {
                id: subscription.id,
                query: subscription.query.clone(),
                sender: subscription.sender.clone(),
            })
            .collect()
    }

    /// Sends a terminal error to all subscriptions and removes them.
    pub fn shutdown_all(&self, message: impl Into<String>) {
        let message = message.into();
        let subscriptions = std::mem::take(
            &mut *self
                .subscriptions
                .write()
                .expect("subscription lock should not be poisoned"),
        );

        for subscription in subscriptions.into_values() {
            let _ = subscription.sender.send(SubscriptionUpdate::Error {
                subscription_id: subscription.id,
                request_id: None,
                message: message.clone(),
            });
        }
    }
}

impl Default for SubscriptionRegistry {
    fn default() -> Self {
        Self::new()
    }
}
