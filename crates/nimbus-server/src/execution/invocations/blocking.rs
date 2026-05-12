use super::*;

pub(crate) fn invoke_runtime_bundle_blocking_with_cancellation(
    runtime_executor: &RuntimeExecutor,
    runtime: NimbusRuntime,
    bundle: RuntimeBundle,
    request: InvocationRequest,
    options: RuntimeBundleInvocationOptions<'_>,
) -> std::result::Result<serde_json::Value, NimbusRuntimeError> {
    runtime_executor.invoke_blocking_with_cancellation(
        runtime,
        bundle,
        request.clone(),
        top_level_runtime_invocation_context(
            &request,
            options.tenant_id,
            options.server_request_id,
            options.concurrency_mode,
        ),
        options.cancellation,
    )
}

pub(crate) fn invoke_runtime_bundle_blocking_with_host(
    runtime_executor: &RuntimeExecutor,
    runtime_policy: Arc<RuntimePolicy>,
    host_bridge: Arc<dyn HostBridge>,
    bundle: RuntimeBundle,
    request: InvocationRequest,
    options: RuntimeBundleInvocationOptions<'_>,
) -> std::result::Result<serde_json::Value, NimbusRuntimeError> {
    invoke_runtime_bundle_blocking_with_cancellation(
        runtime_executor,
        runtime_for_host(host_bridge, runtime_policy),
        bundle,
        request,
        options,
    )
}

#[cfg(test)]
pub(crate) fn invoke_runtime_bundle_blocking_with_host_state<H, S>(
    runtime_executor: &RuntimeExecutor,
    runtime_policy: Arc<RuntimePolicy>,
    host_bridge: Arc<H>,
    bundle: RuntimeBundle,
    request: InvocationRequest,
    options: RuntimeBundleInvocationOptions<'_>,
    snapshot: impl FnOnce(&H) -> S,
) -> std::result::Result<(serde_json::Value, S), NimbusRuntimeError>
where
    H: HostBridge + 'static,
{
    let response = invoke_runtime_bundle_blocking_with_host(
        runtime_executor,
        runtime_policy,
        host_bridge.clone(),
        bundle,
        request,
        options,
    )?;
    Ok((response, snapshot(host_bridge.as_ref())))
}
