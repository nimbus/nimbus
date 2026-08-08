use std::time::Instant;

use nimbus_core::Error;

use super::ServiceManager;
use super::types::TenantServiceKey;

/// Cancellation-safe ownership of one definition-mutation claim.
///
/// Async callers must retain this guard across every awaited retirement
/// effect. Dropping the future, unwinding, or returning on an error releases
/// the exact claim and wakes queued mutations.
pub(super) struct DefinitionMutationClaim<'a> {
    manager: &'a ServiceManager,
    key: TenantServiceKey,
}

impl Drop for DefinitionMutationClaim<'_> {
    fn drop(&mut self) {
        self.manager.release_definition_mutation(&self.key);
    }
}

impl ServiceManager {
    pub(super) async fn claim_definition_mutation_guard(
        &self,
        key: &TenantServiceKey,
        wait_for_existing: bool,
    ) -> Result<DefinitionMutationClaim<'_>, Error> {
        self.claim_definition_mutation(key, wait_for_existing)
            .await?;
        Ok(DefinitionMutationClaim {
            manager: self,
            key: key.clone(),
        })
    }

    /// Claim one dynamic-definition mutation that spans asynchronous retirement.
    ///
    /// The gate deliberately has no callback and no access to the sandbox
    /// backend. It cannot provision, activate, inspect, or otherwise drive a
    /// workload. Its only purpose is serializing definition deletion with
    /// update and session snapshot reads.
    pub(super) async fn claim_definition_mutation(
        &self,
        key: &TenantServiceKey,
        wait_for_existing: bool,
    ) -> Result<(), Error> {
        let deadline = Instant::now() + self.definition_mutation_timeout;
        loop {
            let notified = self.definition_mutation_notify.notified();
            {
                let mut state = self
                    .state
                    .lock()
                    .expect("manager lock should not be poisoned");
                if state.definition_mutations_in_progress.insert(key.clone()) {
                    return Ok(());
                }
                if !wait_for_existing {
                    return Err(Error::conflict(format!(
                        "service `{}` for tenant `{}` has a definition mutation in progress",
                        key.service_name, key.tenant_id
                    )));
                }
            }
            self.notify_definition_mutation_wait_observer();

            let now = Instant::now();
            if now >= deadline {
                return Err(Error::ResourceExhausted(format!(
                    "service definition mutation for `{}` in tenant `{}` could not acquire its serialization gate before {:?}",
                    key.service_name, key.tenant_id, self.definition_mutation_timeout
                )));
            }
            let remaining = deadline.saturating_duration_since(now);
            if tokio::time::timeout(remaining, notified).await.is_err() {
                return Err(Error::ResourceExhausted(format!(
                    "service definition mutation for `{}` in tenant `{}` timed out waiting for the existing mutation",
                    key.service_name, key.tenant_id
                )));
            }
        }
    }

    pub(super) fn release_definition_mutation(&self, key: &TenantServiceKey) {
        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .definition_mutations_in_progress
            .remove(key);
        self.definition_mutation_notify.notify_waiters();
    }
}
