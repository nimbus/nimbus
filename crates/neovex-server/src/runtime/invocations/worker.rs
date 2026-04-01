use super::*;

pub(crate) async fn invoke_runtime_bundle_on_worker(
    runtime_executor: &RuntimeExecutor,
    runtime: NeovexRuntime,
    bundle: RuntimeBundle,
    request: InvocationRequest,
    tenant_id: &TenantId,
    server_request_id: Option<&str>,
    cancellation: Option<HostCallCancellation>,
) -> std::result::Result<serde_json::Value, NeovexRuntimeError> {
    runtime_executor
        .invoke_on_worker(
            runtime,
            bundle,
            request.clone(),
            top_level_runtime_invocation_context(&request, tenant_id, server_request_id),
            cancellation,
        )
        .await
}

pub(crate) async fn invoke_runtime_bundle_on_worker_with_host(
    runtime_executor: &RuntimeExecutor,
    runtime_policy: Arc<RuntimePolicy>,
    host_bridge: Arc<dyn HostBridge>,
    bundle: RuntimeBundle,
    request: InvocationRequest,
    options: RuntimeBundleInvocationOptions<'_>,
) -> std::result::Result<serde_json::Value, NeovexRuntimeError> {
    invoke_runtime_bundle_on_worker(
        runtime_executor,
        runtime_for_host(host_bridge, runtime_policy, options.concurrency_mode),
        bundle,
        request,
        options.tenant_id,
        options.server_request_id,
        options.cancellation,
    )
    .await
}

pub(crate) async fn invoke_runtime_bundle_on_worker_with_host_state<H, S>(
    runtime_executor: &RuntimeExecutor,
    runtime_policy: Arc<RuntimePolicy>,
    host_bridge: Arc<H>,
    bundle: RuntimeBundle,
    request: InvocationRequest,
    options: RuntimeBundleInvocationOptions<'_>,
    snapshot: impl FnOnce(&H) -> S,
) -> std::result::Result<(serde_json::Value, S), NeovexRuntimeError>
where
    H: HostBridge + 'static,
{
    let response = invoke_runtime_bundle_on_worker_with_host(
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
