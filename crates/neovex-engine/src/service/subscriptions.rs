use std::sync::Arc;

use neovex_core::{Error, Query, Result, TenantId};
use tokio::sync::mpsc;

use crate::subscriptions::SubscriptionUpdate;

use super::{Service, documents_to_json, queries::evaluate_with_index};

impl Service {
    /// Registers a new subscription, sends the initial result, and returns the id.
    pub fn subscribe(
        &self,
        tenant_id: &TenantId,
        query: Query,
        request_id: String,
        sender: mpsc::UnboundedSender<SubscriptionUpdate>,
    ) -> Result<u64> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let subscription_id = runtime
            .subscriptions
            .register(query.clone(), sender.clone());
        match evaluate_with_index(&runtime, &query) {
            Ok(documents) => {
                let update = SubscriptionUpdate::Result {
                    subscription_id,
                    request_id: Some(request_id),
                    commit: None,
                    deleted_documents: Vec::new(),
                    data: documents_to_json(documents),
                };
                if sender.send(update).is_err() {
                    runtime.subscriptions.remove(subscription_id);
                    return Err(Error::Internal("subscription channel closed".to_string()));
                }
                Ok(subscription_id)
            }
            Err(error) => {
                runtime.subscriptions.remove(subscription_id);
                Err(error)
            }
        }
    }

    /// Registers a new subscription asynchronously, sends the initial result, and returns the id.
    pub async fn subscribe_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: Query,
        request_id: String,
        sender: mpsc::UnboundedSender<SubscriptionUpdate>,
    ) -> Result<u64> {
        self.call_blocking(move |service| service.subscribe(&tenant_id, query, request_id, sender))
            .await
    }

    /// Removes a subscription if present.
    pub fn unsubscribe(&self, tenant_id: &TenantId, subscription_id: u64) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.subscriptions.remove(subscription_id);
        Ok(())
    }

    /// Removes a subscription asynchronously if present.
    pub async fn unsubscribe_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        subscription_id: u64,
    ) -> Result<()> {
        self.call_blocking(move |service| service.unsubscribe(&tenant_id, subscription_id))
            .await
    }
}
