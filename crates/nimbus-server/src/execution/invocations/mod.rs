use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use nimbus_core::TenantId;
use nimbus_provenance::RuntimeBundleProvenanceConfig;
use nimbus_runtime::{
    HostBridge, HostCallCancellation, InvocationRequest, NimbusRuntime, RuntimeInvocationContext,
    RuntimePolicy,
};

mod blocking;
mod provenance;
mod worker;

#[derive(Clone, Copy)]
pub(crate) enum RuntimeConcurrencyMode {
    EnforcePolicyLimit,
    BudgetedNestedInvocationBypass,
}

#[derive(Clone, Copy)]
pub(crate) enum RuntimeInvocationScope {
    TopLevel,
    Nested,
}

pub(crate) struct RuntimeBundleInvocationOptions<'a> {
    pub(crate) tenant_id: &'a TenantId,
    pub(crate) server_request_id: Option<&'a str>,
    pub(crate) cancellation: Option<HostCallCancellation>,
    pub(crate) concurrency_mode: RuntimeConcurrencyMode,
    pub(crate) scope: RuntimeInvocationScope,
    provenance_gate: Option<&'a RuntimeBundleProvenanceConfig>,
}

impl<'a> RuntimeBundleInvocationOptions<'a> {
    pub(crate) fn enforcing_policy_limit(
        tenant_id: &'a TenantId,
        server_request_id: Option<&'a str>,
        cancellation: Option<HostCallCancellation>,
    ) -> Self {
        Self {
            tenant_id,
            server_request_id,
            cancellation,
            concurrency_mode: RuntimeConcurrencyMode::EnforcePolicyLimit,
            scope: RuntimeInvocationScope::TopLevel,
            provenance_gate: None,
        }
        .with_runtime_bundle_provenance_gate(None)
    }

    pub(crate) fn budgeted_nested_invocation_bypass(
        tenant_id: &'a TenantId,
        server_request_id: Option<&'a str>,
        cancellation: Option<HostCallCancellation>,
    ) -> Self {
        Self {
            tenant_id,
            server_request_id,
            cancellation,
            concurrency_mode: RuntimeConcurrencyMode::BudgetedNestedInvocationBypass,
            scope: RuntimeInvocationScope::Nested,
            provenance_gate: None,
        }
        .with_runtime_bundle_provenance_gate(None)
    }
}

pub(crate) fn next_runtime_server_request_id(prefix: &str) -> String {
    static NEXT_RUNTIME_SERVER_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
    format!(
        "{prefix}-{}",
        NEXT_RUNTIME_SERVER_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
    )
}

pub(crate) fn runtime_invocation_context(
    request: &InvocationRequest,
    tenant_id: &TenantId,
    server_request_id: Option<&str>,
    concurrency_mode: RuntimeConcurrencyMode,
    scope: RuntimeInvocationScope,
) -> RuntimeInvocationContext {
    let tenant_label = tenant_id.to_string();
    let context = match (scope, server_request_id) {
        (RuntimeInvocationScope::TopLevel, Some(server_request_id)) => {
            RuntimeInvocationContext::top_level_for_tenant_and_request(
                request,
                tenant_label,
                server_request_id,
            )
        }
        (RuntimeInvocationScope::TopLevel, None) => {
            RuntimeInvocationContext::top_level_for_tenant(request, tenant_label)
        }
        (RuntimeInvocationScope::Nested, Some(server_request_id)) => {
            RuntimeInvocationContext::nested_for_tenant_and_request(
                request,
                tenant_label,
                server_request_id,
            )
        }
        (RuntimeInvocationScope::Nested, None) => {
            RuntimeInvocationContext::nested_for_tenant(request, tenant_label)
        }
    };
    match concurrency_mode {
        RuntimeConcurrencyMode::EnforcePolicyLimit => context,
        RuntimeConcurrencyMode::BudgetedNestedInvocationBypass => {
            context.with_bypassed_concurrency_limit()
        }
    }
}

fn runtime_for_host(
    host_bridge: Arc<dyn HostBridge>,
    runtime_policy: Arc<RuntimePolicy>,
) -> NimbusRuntime {
    NimbusRuntime::with_policy(host_bridge, runtime_policy)
}

pub(crate) use blocking::invoke_runtime_bundle_blocking_with_host;
#[cfg(test)]
pub(crate) use blocking::invoke_runtime_bundle_blocking_with_host_state;
pub(crate) use worker::invoke_runtime_bundle_on_worker_with_host;
pub(crate) use worker::invoke_runtime_bundle_on_worker_with_host_state;
