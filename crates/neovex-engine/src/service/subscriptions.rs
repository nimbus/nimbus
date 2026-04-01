use std::future::pending;
use std::sync::Arc;

use neovex_core::{Error, PrincipalContext, Query, Result, TenantId};
use tokio::sync::mpsc;

use crate::subscriptions::{SubscriptionRegistration, SubscriptionUpdate};

use super::{
    Service, documents_to_json,
    queries::{evaluate_with_index_cancellable_for_principal, table_policy_revision},
};

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
        self.subscribe_with_principal(
            tenant_id,
            query,
            &PrincipalContext::anonymous(),
            request_id,
            sender,
        )
    }

    /// Registers a new subscription for the provided principal, sends the initial result,
    /// and returns the stable id plus a cleanup handle owned by the caller.
    pub fn subscribe_with_principal(
        &self,
        tenant_id: &TenantId,
        query: Query,
        principal: &PrincipalContext,
        request_id: String,
        sender: mpsc::UnboundedSender<SubscriptionUpdate>,
    ) -> Result<SubscriptionRegistration> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let schema = runtime.schema();
        let principal_snapshot = principal.snapshot()?;
        let policy_revision = table_policy_revision(schema.get_table(&query.table))?;
        let registration = runtime.subscriptions.register(
            query.clone(),
            principal.clone(),
            principal_snapshot,
            policy_revision,
            sender.clone(),
        );
        let subscription_id = registration.id();
        let mut check_cancel = || Ok(());
        match evaluate_with_index_cancellable_for_principal(
            &runtime,
            &query,
            principal,
            &mut check_cancel,
        ) {
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
        self.subscribe_async_with_principal(
            tenant_id,
            query,
            PrincipalContext::anonymous(),
            request_id,
            sender,
        )
        .await
    }

    /// Registers a new subscription asynchronously for the provided principal.
    pub async fn subscribe_async_with_principal(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: Query,
        principal: PrincipalContext,
        request_id: String,
        sender: mpsc::UnboundedSender<SubscriptionUpdate>,
    ) -> Result<SubscriptionRegistration> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let schema = runtime.schema();
        let principal_snapshot = principal.snapshot()?;
        let policy_revision = table_policy_revision(schema.get_table(&query.table))?;
        let registration = runtime.subscriptions.register(
            query.clone(),
            principal.clone(),
            principal_snapshot,
            policy_revision,
            sender.clone(),
        );
        let subscription_id = registration.id();
        let documents = self
            .query_documents_async_cancellable_with_principal(
                tenant_id,
                query,
                principal,
                pending(),
                || Ok(()),
            )
            .await?;
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
        let runtime = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .get(&tenant_id)
            .cloned()
            .ok_or(Error::TenantNotFound(tenant_id.clone()))?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        runtime.subscriptions.remove(subscription_id);
        Ok(())
    }

    /// Returns the current number of registered in-memory subscriptions for a
    /// tenant. This is a diagnostic snapshot of the live registry.
    pub fn active_subscription_count(&self, tenant_id: &TenantId) -> Result<usize> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.subscriptions.len())
    }
}
