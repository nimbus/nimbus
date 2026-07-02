use std::sync::Arc;

use serde_json::Value;

use crate::RuntimeInvocationContext;
use crate::egress::{EgressGateway, RuntimeEgressGatewayBinding};
use crate::error::Result;
use crate::executor::RuntimeExecutor;
use crate::host::{HostBridge, HostCallCancellation};
use crate::limits::{RuntimeLimits, RuntimePolicy};

use super::{InvocationRequest, NimbusRuntime, RuntimeBundle, RuntimeHost};

impl NimbusRuntime {
    pub fn new(host: Arc<dyn HostBridge>) -> Self {
        Self::with_policy(host, Arc::new(RuntimePolicy::default()))
    }

    pub fn with_limits(host: Arc<dyn HostBridge>, limits: RuntimeLimits) -> Self {
        Self::with_policy(host, Arc::new(RuntimePolicy::new(limits)))
    }

    pub fn with_policy(host: Arc<dyn HostBridge>, policy: Arc<RuntimePolicy>) -> Self {
        Self {
            host,
            policy,
            egress_gateway: RuntimeEgressGatewayBinding::coarse_permissions(),
            owned_executor: Arc::default(),
        }
    }

    pub fn with_egress_gateway(mut self, gateway: Arc<dyn EgressGateway>) -> Self {
        self.egress_gateway = RuntimeEgressGatewayBinding::gateway(gateway);
        self
    }

    pub(crate) fn with_egress_gateway_binding(
        mut self,
        binding: RuntimeEgressGatewayBinding,
    ) -> Self {
        self.egress_gateway = binding;
        self
    }

    pub(crate) fn invocation_host(&self) -> RuntimeHost {
        RuntimeHost::from_runtime(self)
    }

    /// Returns the stable executor handle that powers this runtime's public
    /// convenience invocation APIs.
    pub fn executor(&self) -> RuntimeExecutor {
        self.owned_executor
            .get_or_init(|| RuntimeExecutor::new(self.policy.clone()))
            .clone()
    }

    pub async fn invoke_bundle(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
    ) -> Result<Value> {
        self.invoke_bundle_with_cancellation(bundle, request, None)
            .await
    }

    pub async fn invoke_bundle_with_cancellation(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        self.executor()
            .invoke_on_worker(
                self.clone(),
                bundle.clone(),
                request.clone(),
                RuntimeInvocationContext::top_level(request),
                cancellation,
            )
            .await
    }

    pub async fn invoke_bundle_for_tenant(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
    ) -> Result<Value> {
        self.invoke_bundle_for_tenant_with_cancellation(bundle, request, tenant_label, None)
            .await
    }

    pub async fn invoke_bundle_for_tenant_with_cancellation(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        self.executor()
            .invoke_on_worker(
                self.clone(),
                bundle.clone(),
                request.clone(),
                RuntimeInvocationContext::top_level_for_tenant(request, tenant_label),
                cancellation,
            )
            .await
    }

    pub fn invoke_bundle_blocking(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
    ) -> Result<Value> {
        self.invoke_bundle_blocking_with_cancellation(bundle, request, None)
    }

    pub fn invoke_bundle_blocking_with_cancellation(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        self.executor().invoke_blocking_with_cancellation(
            self.clone(),
            bundle.clone(),
            request.clone(),
            RuntimeInvocationContext::top_level(request),
            cancellation,
        )
    }

    pub fn invoke_bundle_blocking_for_tenant(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
    ) -> Result<Value> {
        self.invoke_bundle_blocking_for_tenant_with_cancellation(
            bundle,
            request,
            tenant_label,
            None,
        )
    }

    pub fn invoke_bundle_blocking_for_tenant_with_cancellation(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        self.executor().invoke_blocking_with_cancellation(
            self.clone(),
            bundle.clone(),
            request.clone(),
            RuntimeInvocationContext::top_level_for_tenant(request, tenant_label),
            cancellation,
        )
    }

    pub(crate) fn policy(&self) -> Arc<RuntimePolicy> {
        self.policy.clone()
    }

    pub(crate) fn egress_gateway_binding(&self) -> RuntimeEgressGatewayBinding {
        self.egress_gateway.clone()
    }
}
