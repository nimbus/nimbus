mod bootstrap;

use std::future::{Future, pending};
use std::sync::Arc;

use neovex_core::{
    DependencySet, Document, Error, PrincipalContext, Query, Result, SequenceNumber,
    SubscriptionResultSnapshot, TenantId,
};
use tokio::sync::mpsc;

use crate::subscriptions::{
    QueuedSubscriptionWork, SubscriptionRegistration, SubscriptionUpdate, subscription_dependencies,
};
use crate::tenant::TenantRuntime;

use super::Service;
pub use bootstrap::SubscriptionBootstrapCancellation;
use bootstrap::{
    evaluate_subscription_bootstrap_async_for_principal,
    evaluate_subscription_bootstrap_cancellable_for_principal, table_policy_revision,
};

fn subscription_send_failure(error: mpsc::error::TrySendError<SubscriptionUpdate>) -> Error {
    match error {
        mpsc::error::TrySendError::Full(_) => {
            Error::Internal("subscription channel full".to_string())
        }
        mpsc::error::TrySendError::Closed(_) => {
            Error::Internal("subscription channel closed".to_string())
        }
    }
}

struct SubscriptionBootstrapPublication<'a> {
    subscription_id: u64,
    request_id: String,
    sender: &'a mpsc::Sender<SubscriptionUpdate>,
    covered_sequence: SequenceNumber,
}

impl Service {
    fn register_pending_subscription(
        &self,
        runtime: &Arc<TenantRuntime>,
        query: &Query,
        principal: &PrincipalContext,
        sender: &mpsc::Sender<SubscriptionUpdate>,
    ) -> Result<SubscriptionRegistration> {
        let schema = runtime.schema();
        principal.snapshot()?;
        let policy_revision = table_policy_revision(schema.get_table(&query.table))?;
        Ok(runtime.subscriptions.register(
            query.clone(),
            principal.clone(),
            policy_revision,
            sender.clone(),
            false,
        ))
    }

    fn publish_subscription_bootstrap(
        &self,
        runtime: &Arc<TenantRuntime>,
        query: &Query,
        publication: SubscriptionBootstrapPublication<'_>,
        documents: Vec<Document>,
    ) -> Result<DependencySet> {
        runtime.cache_documents(&documents);
        let dependencies = subscription_dependencies(query, &documents);
        let update = SubscriptionUpdate::Result {
            subscription_id: publication.subscription_id,
            request_id: Some(publication.request_id),
            snapshot: SubscriptionResultSnapshot::bootstrap(
                publication.covered_sequence,
                documents,
            ),
            commit_hint: None,
        };
        if let Err(error) = publication.sender.try_send(update) {
            runtime.subscriptions.remove(publication.subscription_id);
            return Err(subscription_send_failure(error));
        }
        Ok(dependencies)
    }

    fn activate_bootstrapped_subscription(
        &self,
        runtime: Arc<TenantRuntime>,
        subscription_id: u64,
        covered_sequence: SequenceNumber,
        dependencies: neovex_core::DependencySet,
    ) {
        runtime.subscriptions.activate_with_dependencies(
            subscription_id,
            covered_sequence,
            dependencies,
        );
        let current_applied = runtime.applied_head();
        if current_applied.0 <= covered_sequence.0 {
            return;
        }

        let work = QueuedSubscriptionWork::new_coalesced(
            vec![subscription_id],
            current_applied,
            None,
            Vec::new(),
        );
        self.dispatch_or_enqueue_subscription_work(runtime, work);
    }

    /// Registers a new subscription, sends the initial result, and returns the
    /// stable id plus a cleanup handle owned by the caller.
    pub fn subscribe(
        &self,
        tenant_id: &TenantId,
        query: Query,
        request_id: String,
        sender: mpsc::Sender<SubscriptionUpdate>,
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
        sender: mpsc::Sender<SubscriptionUpdate>,
    ) -> Result<SubscriptionRegistration> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let registration =
            self.register_pending_subscription(&runtime, &query, principal, &sender)?;
        let subscription_id = registration.id();
        let mut check_cancel = || Ok(());
        match evaluate_subscription_bootstrap_cancellable_for_principal(
            &runtime,
            &query,
            principal,
            &mut check_cancel,
        ) {
            Ok((documents, covered_sequence)) => {
                let dependencies = self.publish_subscription_bootstrap(
                    &runtime,
                    &query,
                    SubscriptionBootstrapPublication {
                        subscription_id,
                        request_id,
                        sender: &sender,
                        covered_sequence,
                    },
                    documents,
                )?;
                self.activate_bootstrapped_subscription(
                    runtime,
                    subscription_id,
                    covered_sequence,
                    dependencies,
                );
                Ok(registration)
            }
            Err(error) => {
                runtime.subscriptions.remove(subscription_id);
                Err(error)
            }
        }
    }

    /// Registers a new subscription asynchronously, sends the initial result,
    /// and returns the stable id plus a cleanup handle owned by the caller.
    pub async fn subscribe_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: Query,
        request_id: String,
        sender: mpsc::Sender<SubscriptionUpdate>,
    ) -> Result<SubscriptionRegistration> {
        self.subscribe_async_cancellable_with_principal(
            tenant_id,
            query,
            PrincipalContext::anonymous(),
            request_id,
            sender,
            SubscriptionBootstrapCancellation::new(pending(), || Ok(())),
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
        sender: mpsc::Sender<SubscriptionUpdate>,
    ) -> Result<SubscriptionRegistration> {
        self.subscribe_async_cancellable_with_principal(
            tenant_id,
            query,
            principal,
            request_id,
            sender,
            SubscriptionBootstrapCancellation::new(pending(), || Ok(())),
        )
        .await
    }

    /// Registers a new subscription asynchronously and aborts bootstrap work if
    /// the provided cancellation future resolves first.
    pub async fn subscribe_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: Query,
        request_id: String,
        sender: mpsc::Sender<SubscriptionUpdate>,
        cancellation: SubscriptionBootstrapCancellation<Fut, Check>,
    ) -> Result<SubscriptionRegistration>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + Sync + 'static,
    {
        self.subscribe_async_cancellable_with_principal(
            tenant_id,
            query,
            PrincipalContext::anonymous(),
            request_id,
            sender,
            cancellation,
        )
        .await
    }

    /// Registers a new subscription asynchronously for the provided principal
    /// and aborts bootstrap work if the provided cancellation future resolves
    /// first.
    pub async fn subscribe_async_cancellable_with_principal<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: Query,
        principal: PrincipalContext,
        request_id: String,
        sender: mpsc::Sender<SubscriptionUpdate>,
        cancellation: SubscriptionBootstrapCancellation<Fut, Check>,
    ) -> Result<SubscriptionRegistration>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + Sync + 'static,
    {
        let (cancel_wait, check_cancel) = cancellation.into_parts();
        let check_cancel = Arc::new(check_cancel);
        let query_for_bootstrap = query.clone();
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let registration =
            self.register_pending_subscription(&runtime, &query, &principal, &sender)?;
        let subscription_id = registration.id();
        let (documents, covered_sequence) = evaluate_subscription_bootstrap_async_for_principal(
            runtime.clone(),
            tenant_id,
            query_for_bootstrap,
            principal,
            cancel_wait,
            {
                let check_cancel = check_cancel.clone();
                move || (check_cancel.as_ref())()
            },
        )
        .await?;
        if let Err(error) = (check_cancel.as_ref())() {
            runtime.subscriptions.remove(subscription_id);
            return Err(error);
        }
        let dependencies = self.publish_subscription_bootstrap(
            &runtime,
            &query,
            SubscriptionBootstrapPublication {
                subscription_id,
                request_id,
                sender: &sender,
                covered_sequence,
            },
            documents,
        )?;
        #[cfg(any(test, feature = "test-hooks"))]
        runtime.wait_if_subscription_bootstrap_pause_armed().await;
        if let Err(error) = (check_cancel.as_ref())() {
            runtime.subscriptions.remove(subscription_id);
            return Err(error);
        }
        self.activate_bootstrapped_subscription(
            runtime,
            subscription_id,
            covered_sequence,
            dependencies,
        );
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
