use super::*;

/// Health endpoint.
pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

/// Returns the current Neovex license and entitlement status.
pub(crate) async fn license_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<crate::license::LicenseSnapshot>, AppError> {
    let service = state.service.clone();
    let usage = service.current_monthly_active_users_async().await?;
    Ok(Json(state.license_state.snapshot_with_usage(Some(usage))))
}

/// Returns runtime limits and live runtime metrics for diagnostics.
pub(crate) async fn runtime_diagnostics(
    State(state): State<Arc<AppState>>,
) -> Json<RuntimeDiagnosticsResponse> {
    let registry = state
        .convex_registry
        .clone()
        .expect("runtime diagnostics route requires Convex support state");
    let limits = registry.runtime_limits();
    Json(RuntimeDiagnosticsResponse {
        limits: RuntimeLimitsResponse {
            max_heap_mb: limits.max_heap_mb,
            initial_heap_mb: limits.initial_heap_mb,
            execution_timeout_ms: limits
                .execution_timeout
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
            max_concurrent_isolates: limits.max_concurrent_isolates,
            max_top_level_invocations_per_tenant: limits.max_top_level_invocations_per_tenant,
            max_queued_top_level_invocations_per_tenant: limits
                .max_queued_top_level_invocations_per_tenant,
            max_nested_runtime_invocations: limits.max_nested_runtime_invocations,
        },
        metrics: registry.runtime_metrics_snapshot(),
    })
}

/// Redirects to the repo-hosted demos index.
pub(crate) async fn demos_redirect() -> Redirect {
    Redirect::permanent("/demos/")
}
