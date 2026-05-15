use super::*;

pub(in crate::adapters::convex) async fn execute_schedule_command_async(
    service: &Arc<nimbus_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    command: ConvexScheduledCommand,
    cancellation: Option<HostCallCancellation>,
) -> Result<Value, Error> {
    if let Some(cancellation) = cancellation.as_ref() {
        check_host_cancellation(cancellation)?;
    }

    match command {
        ConvexScheduledCommand::RunAfter {
            delay_ms,
            name,
            visibility,
            args,
        } => {
            let mutation = registry.resolve_scheduled_mutation_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            let job_id = match cancellation {
                Some(cancellation) => {
                    let check_cancellation = cancellation.clone();
                    service
                        .schedule_mutation_async_cancellable(
                            tenant_id.clone(),
                            ScheduleRequest {
                                run_after_ms: delay_ms,
                                mutation,
                            },
                            cancellation.cancelled(),
                            move || check_host_cancellation(&check_cancellation),
                        )
                        .await?
                }
                None => {
                    service
                        .schedule_mutation_async(
                            tenant_id.clone(),
                            ScheduleRequest {
                                run_after_ms: delay_ms,
                                mutation,
                            },
                        )
                        .await?
                }
            };
            crate::system_tenant::sync_scheduler_state_for_tenant_async(service, tenant_id).await?;
            Ok(Value::String(job_id.to_string()))
        }
        ConvexScheduledCommand::RunAt {
            timestamp_ms,
            name,
            visibility,
            args,
        } => {
            let mutation = registry.resolve_scheduled_mutation_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            let delay_ms = timestamp_ms.saturating_sub(Timestamp::now().0);
            let job_id = match cancellation {
                Some(cancellation) => {
                    let check_cancellation = cancellation.clone();
                    service
                        .schedule_mutation_async_cancellable(
                            tenant_id.clone(),
                            ScheduleRequest {
                                run_after_ms: delay_ms,
                                mutation,
                            },
                            cancellation.cancelled(),
                            move || check_host_cancellation(&check_cancellation),
                        )
                        .await?
                }
                None => {
                    service
                        .schedule_mutation_async(
                            tenant_id.clone(),
                            ScheduleRequest {
                                run_after_ms: delay_ms,
                                mutation,
                            },
                        )
                        .await?
                }
            };
            crate::system_tenant::sync_scheduler_state_for_tenant_async(service, tenant_id).await?;
            Ok(Value::String(job_id.to_string()))
        }
        ConvexScheduledCommand::Cancel { job_id } => {
            let job_id: nimbus_core::DocumentId = job_id
                .parse()
                .map_err(|error| Error::InvalidInput(format!("invalid document id: {error}")))?;
            let job_id_for_projection = job_id.clone();
            match cancellation {
                Some(cancellation) => {
                    let check_cancellation = cancellation.clone();
                    service
                        .cancel_scheduled_job_async_cancellable(
                            tenant_id.clone(),
                            job_id,
                            cancellation.cancelled(),
                            move || check_host_cancellation(&check_cancellation),
                        )
                        .await?
                }
                None => {
                    service
                        .cancel_scheduled_job_async(tenant_id.clone(), job_id)
                        .await?
                }
            }
            crate::system_tenant::delete_scheduled_job_state_async(
                service,
                tenant_id,
                &job_id_for_projection,
            )
            .await?;
            Ok(Value::Null)
        }
    }
}
