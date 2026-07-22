use super::*;

pub(in crate::adapters::convex) async fn execute_schedule_command_async(
    service: &Arc<nimbus_engine::Engine>,
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
            nimbus_system::sync_scheduler_state_for_tenant_async(service, tenant_id).await?;
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
            let job_id = match cancellation {
                Some(cancellation) => {
                    let check_cancellation = cancellation.clone();
                    service
                        .schedule_mutation_at_async_cancellable(
                            tenant_id.clone(),
                            Timestamp(timestamp_ms),
                            mutation,
                            cancellation.cancelled(),
                            move || check_host_cancellation(&check_cancellation),
                        )
                        .await?
                }
                None => {
                    service
                        .schedule_mutation_at_async(
                            tenant_id.clone(),
                            Timestamp(timestamp_ms),
                            mutation,
                        )
                        .await?
                }
            };
            nimbus_system::sync_scheduler_state_for_tenant_async(service, tenant_id).await?;
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
            nimbus_system::delete_scheduled_job_state_async(
                service,
                tenant_id,
                &job_id_for_projection,
            )
            .await?;
            Ok(Value::Null)
        }
    }
}

#[cfg(test)]
mod tests {
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

    #[tokio::test]
    async fn convex_async_run_at_uses_engine_wall_clock() {
        let data_dir = tempdir().expect("engine tempdir should build");
        let engine = Arc::new(
            nimbus_engine::Engine::new_with_simulation(
                data_dir.path(),
                Arc::new(ManualWallClock::new(Timestamp(50_000))),
                Arc::new(NoopFaultInjector),
            )
            .expect("engine should build"),
        );
        let tenant_id = TenantId::new("clock-async").expect("tenant id should parse");
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");

        execute_schedule_command_async(
            &engine,
            &Arc::new(registry()),
            &tenant_id,
            ConvexScheduledCommand::RunAt {
                timestamp_ms: 1_000,
                name: "messages:send".to_string(),
                visibility: None,
                args: json!({}),
            },
            None,
        )
        .await
        .expect("async runAt should schedule");

        let jobs = engine
            .list_scheduled_jobs_async(tenant_id)
            .await
            .expect("scheduled jobs should list");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].run_at, Timestamp(1_000));
        assert_eq!(jobs[0].created_at, Timestamp(50_000));
    }
}
