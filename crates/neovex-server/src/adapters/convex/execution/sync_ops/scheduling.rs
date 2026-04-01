use super::*;

pub(in crate::adapters::convex) fn execute_schedule_command(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    command: ConvexScheduledCommand,
) -> Result<Value, Error> {
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
            let job_id = service.schedule_mutation(
                tenant_id,
                ScheduleRequest {
                    run_after_ms: delay_ms,
                    mutation,
                },
            )?;
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
            let job_id = service.schedule_mutation(
                tenant_id,
                ScheduleRequest {
                    run_after_ms: delay_ms,
                    mutation,
                },
            )?;
            Ok(Value::String(job_id.to_string()))
        }
        ConvexScheduledCommand::Cancel { job_id } => {
            let job_id = job_id
                .parse()
                .map_err(|error| Error::InvalidInput(format!("invalid document id: {error}")))?;
            service.cancel_scheduled_job(tenant_id, &job_id)?;
            Ok(Value::Null)
        }
    }
}

pub(super) fn execute_schedule_command_cancellable(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    command: ConvexScheduledCommand,
    cancellation: &HostCallCancellation,
) -> Result<Value, Error> {
    check_host_cancellation(cancellation)?;
    execute_schedule_command(service, registry, tenant_id, command)
}
