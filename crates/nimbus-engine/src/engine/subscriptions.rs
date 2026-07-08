mod bootstrap;

use std::future::{Future, Pending, pending};
use std::sync::Arc;

use nimbus_core::{
    DependencySet, Document, Error, PrincipalContext, Query, Result, SequenceNumber,
    SubscriptionResultSnapshot, TenantId,
};
use tokio::sync::mpsc;

use crate::subscriptions::{
    QueuedSubscriptionWork, SubscriptionRegistration, SubscriptionUpdate, subscription_dependencies,
};
use crate::tenant::TenantRuntime;

use super::Engine;
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

/// Options controlling subscription registration: which principal evaluates
/// policy for the bootstrap read, and (for the async path) a cancellation
/// signal that aborts in-flight bootstrap work if it resolves first. The
/// sync `subscribe` path and non-cancellable async subscriptions use the
/// default `Fut`/`Check` (a never-resolving wait paired with an always-ok
/// check), so cancellation is effectively disabled.
pub struct SubscribeOptions<Fut = Pending<()>, Check = fn() -> Result<()>> {
    pub principal: PrincipalContext,
    pub cancellation: SubscriptionBootstrapCancellation<Fut, Check>,
}

impl SubscribeOptions<Pending<()>, fn() -> Result<()>> {
    /// Anonymous principal, no cancellation support.
    pub fn anonymous() -> Self {
        Self::for_principal(PrincipalContext::anonymous())
    }

    /// Explicit principal, no cancellation support.
    pub fn for_principal(principal: PrincipalContext) -> Self {
        Self {
            principal,
            cancellation: SubscriptionBootstrapCancellation::new(pending(), || Ok(())),
        }
    }
}

impl<Fut, Check> SubscribeOptions<Fut, Check> {
    /// Explicit principal plus a cancellation signal that aborts the async
    /// bootstrap read if it resolves first.
    pub fn cancellable(
        principal: PrincipalContext,
        cancellation: SubscriptionBootstrapCancellation<Fut, Check>,
    ) -> Self {
        Self {
            principal,
            cancellation,
        }
    }
}

impl Engine {
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
        Ok(runtime.subscription_registry().register(
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
        let dependencies =
            subscription_dependencies(query, runtime.store().table_id(&query.table)?, &documents);
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
            runtime
                .subscription_registry()
                .remove(publication.subscription_id);
            return Err(subscription_send_failure(error));
        }
        Ok(dependencies)
    }

    fn activate_bootstrapped_subscription(
        &self,
        runtime: Arc<TenantRuntime>,
        subscription_id: u64,
        covered_sequence: SequenceNumber,
        dependencies: nimbus_core::DependencySet,
    ) {
        runtime.subscription_registry().activate_with_dependencies(
            subscription_id,
            covered_sequence,
            dependencies,
        );
        let current_applied = runtime.applied_head();
        if current_applied.0 <= covered_sequence.0 {
            return;
        }

        // Something advanced the applied head between the bootstrap read and
        // activation above -- but `applied_head` also advances through
        // zero-write commits (e.g. the trigger-candidate feed's own
        // delivery-cursor advance, which shares this tenant's commit log and
        // sequence space). If nothing document-bearing landed in that gap,
        // re-evaluating now would only reproduce the bootstrap read's own
        // result: dispatching a catch-up would be a spurious duplicate, not
        // a real update. This is the uncommon "something happened during
        // bootstrap" branch, not the hot path, so a direct commit-log read
        // is cheap here; fail open (assume a catch-up is needed) if the read
        // itself errors.
        let gap_has_document_bearing_commit = runtime
            .store()
            .read_commit_log_from(SequenceNumber(covered_sequence.0.saturating_add(1)))
            .map(|commits| commits.iter().any(|commit| !commit.writes.is_empty()))
            .unwrap_or(true);
        if !gap_has_document_bearing_commit {
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
        opts: SubscribeOptions,
    ) -> Result<SubscriptionRegistration> {
        let SubscribeOptions { principal, .. } = opts;
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let registration =
            self.register_pending_subscription(&runtime, &query, &principal, &sender)?;
        let subscription_id = registration.id();
        let mut check_cancel = || Ok(());
        match evaluate_subscription_bootstrap_cancellable_for_principal(
            &runtime,
            &query,
            &principal,
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
                runtime.subscription_registry().remove(subscription_id);
                Err(error)
            }
        }
    }

    /// Registers a new subscription asynchronously, sends the initial result,
    /// and returns the stable id plus a cleanup handle owned by the caller.
    /// Aborts the in-flight bootstrap read if `opts.cancellation` resolves
    /// first.
    pub async fn subscribe_async<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        query: Query,
        request_id: String,
        sender: mpsc::Sender<SubscriptionUpdate>,
        opts: SubscribeOptions<Fut, Check>,
    ) -> Result<SubscriptionRegistration>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + Sync + 'static,
    {
        let SubscribeOptions {
            principal,
            cancellation,
        } = opts;
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
            runtime.subscription_registry().remove(subscription_id);
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
            runtime.subscription_registry().remove(subscription_id);
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
        runtime.subscription_registry().remove(subscription_id);
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
        runtime.subscription_registry().remove(subscription_id);
        Ok(())
    }

    /// Returns the current number of registered in-memory subscriptions for a
    /// tenant. This is a diagnostic snapshot of the live registry.
    pub fn active_subscription_count(&self, tenant_id: &TenantId) -> Result<usize> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.subscription_registry().len())
    }
}
