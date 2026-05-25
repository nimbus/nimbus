use super::*;

/// Health endpoint.
pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

/// Returns the current Nimbus license and entitlement status.
pub(crate) async fn license_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<crate::license::LicenseSnapshot>, AppError> {
    let service = state.service.clone();
    let usage = service.current_monthly_active_users_async().await?;
    Ok(Json(state.license_state.snapshot_with_usage(Some(usage))))
}

/// Returns runtime limits and live runtime metrics for diagnostics.
///
/// Always returns 200 with a stable shape so the operator settings UI never
/// sees a 4xx on the default `nimbus start` (no app generation yet). The
/// limits/metrics/reset_capabilities fields are null until a deployment is
/// active.
pub(crate) async fn runtime_diagnostics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<RuntimeDiagnosticsResponse>, AppError> {
    let deployment = state.current_deployment();
    let Some(registry) = deployment.convex_registry() else {
        return Ok(Json(RuntimeDiagnosticsResponse {
            limits: None,
            reset_capabilities: None,
            metrics: None,
            lanes: Vec::new(),
        }));
    };
    let limits = registry.runtime_limits();
    Ok(Json(RuntimeDiagnosticsResponse {
        limits: Some(runtime_limits_response(&limits)),
        reset_capabilities: Some(limits.reset_capabilities()),
        metrics: Some(registry.runtime_metrics_snapshot()),
        lanes: registry
            .runtime_lane_diagnostics()
            .into_iter()
            .map(|lane| RuntimeLaneDiagnosticsResponse {
                lane_name: lane.lane_name.to_string(),
                default_lane: lane.default_lane,
                executor_started: lane.executor_started,
                execution_adapter_state: lane.execution_adapter_state,
                execution_adapter_artifact: lane.execution_adapter_artifact,
                limits: runtime_limits_response(&lane.limits),
                reset_capabilities: lane.reset_capabilities,
                metrics: lane.metrics,
            })
            .collect(),
    }))
}

fn runtime_limits_response(limits: &nimbus_runtime::RuntimeLimits) -> RuntimeLimitsResponse {
    let tenant_budget = limits.tenant_budget();
    RuntimeLimitsResponse {
        runtime_backend: limits.backend_kind,
        runtime_backend_trust_tier: limits.backend_trust_tier,
        runtime_backend_lockdown_profile: limits.backend_lockdown_profile,
        runtime_backend_lifecycle_policy: limits.backend_lifecycle_policy,
        bundle_content_kind: limits.bundle_content_kind,
        javascript_evaluation_format: limits.javascript_evaluation_format,
        compatibility_target: limits.compatibility_target,
        execution_model: limits.execution_model,
        runtime_mode: limits.mode,
        runtime_language: limits.language,
        runtime_preset: limits.preset,
        runtime_grants: limits.grants.clone(),
        runtime_pool_kind: limits.runtime_pool_kind,
        memory_enforcement: limits.memory_enforcement,
        module_state_semantics: limits.module_state_semantics(),
        routing_affinity: limits.routing_affinity,
        routing_affinity_max_entries: limits.routing_affinity_max_entries,
        max_warm_pool_entries_per_worker: limits.max_warm_pool_entries_per_worker,
        max_warm_reuses: limits.max_warm_reuses,
        max_heap_mb: limits.max_heap_mb,
        initial_heap_mb: limits.initial_heap_mb,
        execution_timeout_ms: duration_millis_u64(limits.execution_timeout),
        max_concurrent_runtime_instances: limits.max_concurrent_runtime_instances,
        worker_threads: limits.worker_threads,
        max_active_top_level_invocations_per_tenant: limits
            .max_active_top_level_invocations_per_tenant,
        max_in_flight_top_level_invocations_per_tenant: limits
            .max_in_flight_top_level_invocations_per_tenant,
        max_queued_top_level_invocations_per_tenant: limits
            .max_queued_top_level_invocations_per_tenant,
        max_nested_runtime_invocations: limits.max_nested_runtime_invocations,
        tenant_budget: RuntimeTenantBudgetResponse {
            max_active_runtime_slots: tenant_budget.max_active_runtime_slots,
            max_in_flight_top_level_invocations: tenant_budget.max_in_flight_top_level_invocations,
            max_queued_top_level_invocations: tenant_budget.max_queued_top_level_invocations,
            max_worker_thread_slots: tenant_budget.max_worker_thread_slots,
            max_heap_mb_per_runtime: tenant_budget.max_heap_mb_per_runtime,
            memory_enforcement: tenant_budget.memory_enforcement,
            max_active_heap_mb: tenant_budget.max_active_heap_mb,
            execution_timeout_ms: duration_millis_u64(tenant_budget.execution_timeout),
            max_nested_runtime_invocations_per_top_level: tenant_budget
                .max_nested_runtime_invocations_per_top_level,
        },
    }
}

fn duration_millis_u64(duration: std::time::Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

/// Returns per-tenant engine durability, worker, and serving diagnostics.
pub(crate) async fn tenant_engine_diagnostics(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<Json<TenantEngineDiagnosticsResponse>, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.metadata.engine")?;
    let diagnostics = state
        .service
        .clone()
        .tenant_engine_diagnostics_async(tenant.tenant_id().clone())
        .await?;
    Ok(Json(TenantEngineDiagnosticsResponse {
        tenant_id: tenant.tenant_id().to_string(),
        diagnostics,
    }))
}

/// Runs the on-demand tenant consistency verifier and returns the diagnostic report.
pub(crate) async fn tenant_consistency_report(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<Json<nimbus_engine::ConsistencyVerificationReport>, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.metadata.consistency")?;
    let report = state
        .service
        .clone()
        .verify_consistency_async(tenant.tenant_id().clone())
        .await?;
    Ok(Json(report))
}

/// Redirects to the repo-hosted demos index.
pub(crate) async fn demos_redirect() -> Redirect {
    Redirect::permanent("/demos/")
}

/// Returns the service encryption status for diagnostics.
pub(crate) async fn encryption_status(
    State(state): State<Arc<AppState>>,
) -> Json<nimbus_engine::EncryptionStatus> {
    let status = state
        .service
        .encryption_status()
        .cloned()
        .unwrap_or_else(|| nimbus_engine::EncryptionStatus {
            enabled: false,
            encrypted_families: Vec::new(),
            descriptor: nimbus_engine::EncryptionConfigDescriptor::Disabled,
        });
    Json(status)
}
