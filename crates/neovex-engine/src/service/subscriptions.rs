use std::sync::Arc;

use neovex_core::{Error, Query, Result, TenantId};
use tokio::sync::mpsc;

use crate::subscriptions::{SubscriptionRegistration, SubscriptionUpdate};

use super::{Service, documents_to_json, queries::evaluate_with_index};

impl Service {
    /// Registers a new subscription, sends the initial result, and returns the
    /// stable id plus a cleanup handle owned by the caller.
    pub fn subscribe(
        &self,
        tenant_id: &TenantId,
        query: Query,
        request_id: String,
        sender: mpsc::UnboundedSender<SubscriptionUpdate>,
    ) -> Result<SubscriptionRegistration> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let registration = runtime
            .subscriptions
            .register(query.clone(), sender.clone());
        let subscription_id = registration.id();
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
                    return Err(Error::Internal("subscription channel closed".to_string()));
                }
                Ok(registration)
            }
            Err(error) => Err(error),
        }
    }

    /// Registers a new subscription asynchronously, sends the initial result,
    /// and returns the stable id plus a cleanup handle owned by the caller.
    pub async fn subscribe_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: Query,
        request_id: String,
        sender: mpsc::UnboundedSender<SubscriptionUpdate>,
    ) -> Result<SubscriptionRegistration> {
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

    /// Returns the current number of registered in-memory subscriptions for a
    /// tenant. This is a diagnostic snapshot of the live registry.
    pub fn active_subscription_count(&self, tenant_id: &TenantId) -> Result<usize> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.subscriptions.len())
    }
}
