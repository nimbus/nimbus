use super::common::registry_and_auth;
use super::*;

/// Schedules a public convex mutation by relative delay.
pub(crate) async fn schedule_after(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexScheduleAfterRequest>,
) -> Result<(StatusCode, Json<ScheduleResponse>), AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let (registry, _) = registry_and_auth(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexHttp,
        &tenant_id,
        &headers,
        "convex schedule-after route requires Convex support state",
    )
    .await?;
    let request = match request {
        ConvexScheduleAfterRequest::Named(request) => ScheduleRequest {
            run_after_ms: request.run_after_ms,
            mutation: registry.resolve_scheduled_mutation(&request.name, &request.args)?,
        },
        ConvexScheduleAfterRequest::Raw {
            mutation,
            run_after_ms,
        } => ScheduleRequest {
            run_after_ms,
            mutation,
        },
    };

    let job_id = service
        .schedule_mutation_async(tenant_id.clone(), request)
        .await?;
    crate::system_tenant::sync_scheduler_state_for_tenant_async(&service, &tenant_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(ScheduleResponse {
            job_id: job_id.to_string(),
        }),
    ))
}

/// Schedules a public convex mutation for an absolute timestamp.
pub(crate) async fn schedule_at(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexScheduleAtRequest>,
) -> Result<(StatusCode, Json<ScheduleResponse>), AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let (registry, _) = registry_and_auth(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexHttp,
        &tenant_id,
        &headers,
        "convex schedule-at route requires Convex support state",
    )
    .await?;
    let (run_at_ms, mutation) = match request {
        ConvexScheduleAtRequest::Named(request) => (
            request.run_at_ms,
            registry.resolve_scheduled_mutation(&request.name, &request.args)?,
        ),
        ConvexScheduleAtRequest::Raw {
            mutation,
            run_at_ms,
        } => (run_at_ms, mutation),
    };
    let delay_ms = run_at_ms.saturating_sub(Timestamp::now().0);
    let request = ScheduleRequest {
        run_after_ms: delay_ms,
        mutation,
    };

    let job_id = service
        .schedule_mutation_async(tenant_id.clone(), request)
        .await?;
    crate::system_tenant::sync_scheduler_state_for_tenant_async(&service, &tenant_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(ScheduleResponse {
            job_id: job_id.to_string(),
        }),
    ))
}

/// Cancels a pending convex scheduled job.
pub(crate) async fn cancel_scheduled_job(
    State(state): State<Arc<AppState>>,
    AxumPath((tenant_id, job_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let job_id = parse_job_id(&job_id)?;
    let service = state.service.clone();
    let _ = registry_and_auth(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexHttp,
        &tenant_id,
        &headers,
        "convex scheduled job cancel route requires Convex support state",
    )
    .await?;
    service
        .cancel_scheduled_job_async(tenant_id.clone(), job_id.clone())
        .await?;
    crate::system_tenant::delete_scheduled_job_state_async(&service, &tenant_id, &job_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
