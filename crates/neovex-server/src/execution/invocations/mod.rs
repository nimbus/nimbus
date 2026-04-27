use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use neovex_core::TenantId;
use neovex_runtime::{
    HostBridge, HostCallCancellation, InvocationRequest, NeovexRuntime, NeovexRuntimeError,
    RuntimeBundle, RuntimeExecutor, RuntimeInvocationContext, RuntimePolicy,
};

mod blocking;
mod worker;

#[derive(Clone, Copy)]
pub(crate) enum RuntimeConcurrencyMode {
    EnforcePolicyLimit,
    BypassPolicyLimit,
}

pub(crate) struct RuntimeBundleInvocationOptions<'a> {
    pub(crate) tenant_id: &'a TenantId,
    pub(crate) server_request_id: Option<&'a str>,
    pub(crate) cancellation: Option<HostCallCancellation>,
    pub(crate) concurrency_mode: RuntimeConcurrencyMode,
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
        }
    }

    pub(crate) fn bypassing_policy_limit(
        tenant_id: &'a TenantId,
        server_request_id: Option<&'a str>,
        cancellation: Option<HostCallCancellation>,
    ) -> Self {
        Self {
            tenant_id,
            server_request_id,
            cancellation,
            concurrency_mode: RuntimeConcurrencyMode::BypassPolicyLimit,
        }
    }
}

pub(crate) fn next_runtime_server_request_id(prefix: &str) -> String {
    static NEXT_RUNTIME_SERVER_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
    format!(
        "{prefix}-{}",
        NEXT_RUNTIME_SERVER_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
    )
}

pub(crate) fn top_level_runtime_invocation_context(
    request: &InvocationRequest,
    tenant_id: &TenantId,
    server_request_id: Option<&str>,
    concurrency_mode: RuntimeConcurrencyMode,
) -> RuntimeInvocationContext {
    let context = match server_request_id {
        Some(server_request_id) => RuntimeInvocationContext::top_level_for_tenant_and_request(
            request,
            tenant_id.to_string(),
            server_request_id,
        ),
        None => RuntimeInvocationContext::top_level_for_tenant(request, tenant_id.to_string()),
    };
    match concurrency_mode {
        RuntimeConcurrencyMode::EnforcePolicyLimit => context,
        RuntimeConcurrencyMode::BypassPolicyLimit => context.with_bypassed_concurrency_limit(),
    }
}

fn runtime_for_host(
    host_bridge: Arc<dyn HostBridge>,
    runtime_policy: Arc<RuntimePolicy>,
) -> NeovexRuntime {
    NeovexRuntime::with_policy(host_bridge, runtime_policy)
}

pub(crate) use blocking::invoke_runtime_bundle_blocking_with_host;
#[cfg(test)]
pub(crate) use blocking::invoke_runtime_bundle_blocking_with_host_state;
pub(crate) use worker::invoke_runtime_bundle_on_worker_with_host;
pub(crate) use worker::invoke_runtime_bundle_on_worker_with_host_state;
