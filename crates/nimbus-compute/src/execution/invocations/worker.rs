use std::sync::Arc;

use nimbus_runtime::{
    EgressGateway, HostBridge, InvocationRequest, NimbusRuntimeError, RuntimeBundle,
    RuntimeExecutor, RuntimePolicy,
};

use super::{
    RuntimeBundleInvocationOptions, runtime_for_host_with_egress_gateway,
    runtime_invocation_context,
};

pub async fn invoke_runtime_bundle_on_worker_with_host_state<H, S>(
    runtime_executor: &RuntimeExecutor,
    runtime_policy: Arc<RuntimePolicy>,
    host_bridge: Arc<H>,
    bundle: RuntimeBundle,
    request: InvocationRequest,
    options: RuntimeBundleInvocationOptions<'_>,
    snapshot: impl FnOnce(&H) -> S,
) -> std::result::Result<(serde_json::Value, S), NimbusRuntimeError>
where
    H: HostBridge + EgressGateway + 'static,
{
    let response = invoke_runtime_bundle_on_worker_with_egress_gateway(
        runtime_executor,
        runtime_policy,
        host_bridge.clone(),
        bundle,
        request,
        options,
    )
    .await?;
    Ok((response, snapshot(host_bridge.as_ref())))
}

pub async fn invoke_runtime_bundle_on_worker_with_egress_gateway<H>(
    runtime_executor: &RuntimeExecutor,
    runtime_policy: Arc<RuntimePolicy>,
    host_bridge: Arc<H>,
    bundle: RuntimeBundle,
    request: InvocationRequest,
    options: RuntimeBundleInvocationOptions<'_>,
) -> std::result::Result<serde_json::Value, NimbusRuntimeError>
where
    H: HostBridge + EgressGateway + 'static,
{
    options.admit_runtime_bundle_artifact(&bundle)?;
    let runtime = runtime_for_host_with_egress_gateway(host_bridge, runtime_policy)?;
    runtime_executor
        .invoke_on_worker_response_ready(
            runtime,
            bundle,
            request.clone(),
            runtime_invocation_context(
                &request,
                options.tenant_id,
                options.server_request_id,
                options.concurrency_mode,
                options.scope,
                options.authority,
            ),
            options.cancellation,
        )
        .await?
        .wait_until_complete()
        .await
}
