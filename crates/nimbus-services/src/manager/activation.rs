use std::time::Instant;

use nimbus_core::{Error, TenantId};
use nimbus_node::{LocalEnforcementBinding, TenantEgressReloadRequest};
use nimbus_runtime::HostCallCancellation;
use nimbus_sandbox::{SandboxHandle, SandboxStatus};
use nimbus_tenant::{
    TenantIsolationContext, TenantIsolationDecision, TenantIsolationPolicyInput,
    TenantServiceGrantPolicyDecision, WorkloadAttributes,
};
use tokio::time::sleep;

use super::ServiceManager;
use super::types::{ActivationClaim, TenantServiceKey, sandbox_backend_error};

impl ServiceManager {
    pub(super) async fn claim_activation(&self, key: &TenantServiceKey) -> ActivationClaim {
        loop {
            let notified = self.activation_notify.notified();
            {
                let mut state = self
                    .state
                    .lock()
                    .expect("manager lock should not be poisoned");
                if state.handles.contains_key(key) {
                    return ActivationClaim::AlreadyActive;
                }
                if state.activations_in_progress.insert(key.clone()) {
                    return ActivationClaim::Claimed;
                }
            }
            notified.await;
        }
    }

    pub(super) fn release_activation(&self, key: &TenantServiceKey) {
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        state.activations_in_progress.remove(key);
        self.activation_notify.notify_waiters();
    }

    pub(super) async fn wait_for_ready_handle_async(
        &self,
        key: &TenantServiceKey,
        cancellation: &HostCallCancellation,
    ) -> Result<Option<SandboxHandle>, Error> {
        let deadline = Instant::now() + self.activation_timeout;
        loop {
            if cancellation.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let Some(handle) = self.refresh_handle_async(key).await? else {
                return Ok(None);
            };
            if handle.status == SandboxStatus::Ready {
                return Ok(Some(handle));
            }
            if matches!(
                handle.status,
                SandboxStatus::Stopped | SandboxStatus::Failed
            ) {
                return Ok(Some(handle));
            }
            if Instant::now() >= deadline {
                return Err(Error::ResourceExhausted(format!(
                    "sandbox-backed service {} for tenant {} did not become ready within {:?}",
                    key.service_name, key.tenant_id, self.activation_timeout
                )));
            }
            tokio::select! {
                _ = cancellation.cancelled() => return Err(Error::Cancelled),
                _ = sleep(self.activation_poll_interval) => {}
            }
        }
    }

    pub async fn start_service_async(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        cancellation: HostCallCancellation,
    ) -> Result<Option<SandboxHandle>, Error> {
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
        self.start_service_for_context_async(&isolation, service_name, cancellation)
            .await
    }

    pub async fn start_service_for_context_async(
        &self,
        isolation: &TenantIsolationContext,
        service_name: &str,
        cancellation: HostCallCancellation,
    ) -> Result<Option<SandboxHandle>, Error> {
        let decision = self.service_lifecycle_decision(isolation, service_name)?;
        self.start_service_for_decision_async(&decision, service_name, cancellation)
            .await
    }

    pub async fn start_service_for_decision_async(
        &self,
        decision: &TenantIsolationDecision,
        service_name: &str,
        cancellation: HostCallCancellation,
    ) -> Result<Option<SandboxHandle>, Error> {
        let tenant_id = decision.tenant_id();
        let binding = LocalEnforcementBinding::from_decision(decision)?;
        binding.service_access(service_name)?;
        let key = TenantServiceKey::new(tenant_id, service_name);
        if let Some(handle) = self.refresh_handle_async(&key).await?
            && !matches!(
                handle.status,
                SandboxStatus::Stopped | SandboxStatus::Failed
            )
        {
            return self.wait_for_ready_handle_async(&key, &cancellation).await;
        }

        match self.claim_activation(&key).await {
            ActivationClaim::AlreadyActive => {
                self.wait_for_ready_handle_async(&key, &cancellation).await
            }
            ActivationClaim::Claimed => {
                let Some(activation) = self.service_activation_for_tenant(tenant_id, service_name)
                else {
                    self.release_activation(&key);
                    return Ok(None);
                };
                let start_result = self
                    .start_sandbox_service_async(
                        &key,
                        decision,
                        activation.backend,
                        &activation.volume_policy,
                    )
                    .await;
                self.release_activation(&key);
                start_result?;
                self.wait_for_ready_handle_async(&key, &cancellation).await
            }
        }
    }

    pub async fn stop_service_for_context_async(
        &self,
        isolation: &TenantIsolationContext,
        service_name: &str,
    ) -> Result<Option<SandboxHandle>, Error> {
        let decision = self.service_lifecycle_decision(isolation, service_name)?;
        self.stop_service_for_decision_async(&decision, service_name)
            .await
    }

    pub async fn stop_service_for_decision_async(
        &self,
        decision: &TenantIsolationDecision,
        service_name: &str,
    ) -> Result<Option<SandboxHandle>, Error> {
        let tenant_id = decision.tenant_id();
        let binding = LocalEnforcementBinding::from_decision(decision)?;
        binding.service_access(service_name)?;
        let key = TenantServiceKey::new(tenant_id, service_name);
        let previous_handle = self.current_handle(&key);
        let refreshed_handle = self.refresh_handle_async(&key).await?;
        let handle_existed_in_backend = refreshed_handle.is_some();
        let Some(handle) = refreshed_handle.or(previous_handle) else {
            return Ok(None);
        };

        if handle_existed_in_backend
            && !matches!(
                handle.status,
                SandboxStatus::Stopped | SandboxStatus::Stopping
            )
        {
            self.sandbox_backend
                .stop(&handle.id)
                .await
                .map_err(|error| sandbox_backend_error(&key, "stop", &error))?;
        }

        let mut stopped_handle = handle;
        stopped_handle.status = SandboxStatus::Stopped;
        stopped_handle.published_endpoints.clear();

        {
            let mut state = self
                .state
                .lock()
                .expect("manager lock should not be poisoned");
            state.handles.remove(&key);
            state.activations_in_progress.remove(&key);
        }
        self.activation_notify.notify_waiters();
        self.record_service_handle(&key, &stopped_handle).await?;

        Ok(Some(stopped_handle))
    }

    pub async fn restart_service_for_context_async(
        &self,
        isolation: &TenantIsolationContext,
        service_name: &str,
        cancellation: HostCallCancellation,
    ) -> Result<Option<SandboxHandle>, Error> {
        let decision = self.service_lifecycle_decision(isolation, service_name)?;
        self.restart_service_for_decision_async(&decision, service_name, cancellation)
            .await
    }

    pub async fn restart_service_for_decision_async(
        &self,
        decision: &TenantIsolationDecision,
        service_name: &str,
        cancellation: HostCallCancellation,
    ) -> Result<Option<SandboxHandle>, Error> {
        self.stop_service_for_decision_async(decision, service_name)
            .await?;
        self.start_service_for_decision_async(decision, service_name, cancellation)
            .await
    }

    pub async fn reload_service_egress_for_decision_async(
        &self,
        tenant_id: &TenantId,
        decision: &TenantIsolationDecision,
        service_name: &str,
    ) -> Result<Option<SandboxHandle>, Error> {
        if decision.tenant_id() != tenant_id {
            return Err(Error::InvalidInput(format!(
                "egress reload decision tenant {} does not match requested tenant {}",
                decision.tenant_id(),
                tenant_id
            )));
        }
        let key = TenantServiceKey::new(tenant_id, service_name);
        let binding = LocalEnforcementBinding::from_decision(decision)?;
        binding
            .service_access(service_name)?
            .ensure_tenant_matches(tenant_id, "sandbox-backed service egress reload")?;
        binding.authorize_egress_reload(&TenantEgressReloadRequest::for_spec(binding.spec()))?;
        let Some(handle) = self.refresh_handle_async(&key).await? else {
            return Ok(None);
        };
        let egress = decision.network().sandbox_egress().clone();
        self.sandbox_backend
            .reload_egress_policy(&handle.id, egress)
            .await
            .map_err(|error| sandbox_backend_error(&key, "reload egress policy", &error))?;
        let refreshed = self.refresh_handle_async(&key).await?.unwrap_or(handle);
        Ok(Some(refreshed))
    }

    pub(super) fn service_lifecycle_decision(
        &self,
        isolation: &TenantIsolationContext,
        service_name: &str,
    ) -> Result<TenantIsolationDecision, Error> {
        isolation.admit_decision(
            TenantIsolationPolicyInput::new(WorkloadAttributes::service(service_name))
                .with_services(TenantServiceGrantPolicyDecision::new([service_name]))
                .with_image(self.manager_image_policy()),
        )
    }
}
