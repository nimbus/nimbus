use super::*;

pub(in crate::adapters::convex) fn execute_schedule_command(
    service: &nimbus_engine::Engine,
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
            let job_id =
                service.schedule_mutation_at(tenant_id, Timestamp(timestamp_ms), mutation)?;
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
    service: &nimbus_engine::Engine,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    command: ConvexScheduledCommand,
    cancellation: &HostCallCancellation,
) -> Result<Value, Error> {
    check_host_cancellation(cancellation)?;
    execute_schedule_command(service, registry, tenant_id, command)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nimbus_core::{ManualWallClock, Timestamp};
    use nimbus_storage::NoopFaultInjector;
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    fn registry() -> ConvexRegistry {
        let root = tempdir().expect("registry tempdir should build");
        let convex_dir = root.path().join(".nimbus/convex");
        std::fs::create_dir_all(&convex_dir).expect("registry directory should build");
        std::fs::write(
            convex_dir.join("functions.json"),
            serde_json::to_vec(&json!({
                "functions": [{
                    "name": "messages:send",
                    "kind": "mutation",
                    "visibility": "public",
                    "schedulable": true,
                    "plan": { "type": "insert", "table": "messages", "fields": {} }
                }]
            }))
            .expect("registry should serialize"),
        )
        .expect("registry should write");
        ConvexRegistry::from_app_dir(root.path()).expect("registry should load")
    }

    #[test]
    fn convex_sync_run_at_uses_engine_wall_clock() {
        let data_dir = tempdir().expect("engine tempdir should build");
        let engine = nimbus_engine::Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualWallClock::new(Timestamp(50_000))),
            Arc::new(NoopFaultInjector),
        )
        .expect("engine should build");
        let tenant_id = TenantId::new("clock-sync").expect("tenant id should parse");
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");

        execute_schedule_command(
            &engine,
            &registry(),
            &tenant_id,
            ConvexScheduledCommand::RunAt {
                timestamp_ms: 1_000,
                name: "messages:send".to_string(),
                visibility: None,
                args: json!({}),
            },
        )
        .expect("sync runAt should schedule");

        let jobs = engine
            .list_scheduled_jobs(&tenant_id)
            .expect("scheduled jobs should list");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].run_at, Timestamp(1_000));
        assert_eq!(jobs[0].created_at, Timestamp(50_000));
    }
}
