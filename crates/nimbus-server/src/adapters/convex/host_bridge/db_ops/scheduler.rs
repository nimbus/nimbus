use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn execute_schedule_command_with_execution_context_async(
        &self,
        command: ConvexScheduledCommand,
        cancellation: &HostCallCancellation,
    ) -> Result<Value, Error> {
        if let Some(execution_unit) = self.mutation_execution_unit() {
            return match command {
                ConvexScheduledCommand::RunAfter {
                    delay_ms,
                    name,
                    visibility,
                    args,
                } => {
                    let mutation = self.registry().resolve_scheduled_mutation_for_visibility(
                        &name,
                        &args,
                        visibility.unwrap_or(ConvexFunctionVisibility::Public),
                    )?;
                    execution_unit
                        .schedule_mutation_after(mutation, delay_ms)
                        .map(|job_id| Value::String(job_id.to_string()))
                }
                ConvexScheduledCommand::RunAt {
                    timestamp_ms,
                    name,
                    visibility,
                    args,
                } => {
                    let mutation = self.registry().resolve_scheduled_mutation_for_visibility(
                        &name,
                        &args,
                        visibility.unwrap_or(ConvexFunctionVisibility::Public),
                    )?;
                    execution_unit
                        .schedule_mutation_at(mutation, timestamp_ms)
                        .map(|job_id| Value::String(job_id.to_string()))
                }
                ConvexScheduledCommand::Cancel { job_id } => {
                    let job_id = job_id.parse().map_err(|error| {
                        Error::InvalidInput(format!("invalid document id: {error}"))
                    })?;
                    execution_unit.cancel_scheduled_job(job_id)?;
                    Ok(Value::Null)
                }
            };
        }

        execute_schedule_command_async(
            self.service(),
            self.registry(),
            self.tenant_id(),
            command,
            Some(cancellation.clone()),
        )
        .await
    }

    pub(in crate::adapters::convex) fn execute_schedule_command_with_execution_context(
        &self,
        command: ConvexScheduledCommand,
    ) -> Result<Value, Error> {
        if let Some(execution_unit) = self.mutation_execution_unit() {
            return match command {
                ConvexScheduledCommand::RunAfter {
                    delay_ms,
                    name,
                    visibility,
                    args,
                } => {
                    let mutation = self.registry().resolve_scheduled_mutation_for_visibility(
                        &name,
                        &args,
                        visibility.unwrap_or(ConvexFunctionVisibility::Public),
                    )?;
                    execution_unit
                        .schedule_mutation_after(mutation, delay_ms)
                        .map(|job_id| Value::String(job_id.to_string()))
                }
                ConvexScheduledCommand::RunAt {
                    timestamp_ms,
                    name,
                    visibility,
                    args,
                } => {
                    let mutation = self.registry().resolve_scheduled_mutation_for_visibility(
                        &name,
                        &args,
                        visibility.unwrap_or(ConvexFunctionVisibility::Public),
                    )?;
                    execution_unit
                        .schedule_mutation_at(mutation, timestamp_ms)
                        .map(|job_id| Value::String(job_id.to_string()))
                }
                ConvexScheduledCommand::Cancel { job_id } => {
                    let job_id = job_id.parse().map_err(|error| {
                        Error::InvalidInput(format!("invalid document id: {error}"))
                    })?;
                    execution_unit.cancel_scheduled_job(job_id)?;
                    Ok(Value::Null)
                }
            };
        }

        execute_schedule_command(self.service(), self.registry(), self.tenant_id(), command)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_scheduler_run_after_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeSchedulerRunAfterPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self
            .execute_schedule_command_with_execution_context_async(
                ConvexScheduledCommand::RunAfter {
                    delay_ms: payload.delay_ms,
                    name: payload.name,
                    visibility: payload.visibility,
                    args: payload.args,
                },
                cancellation,
            )
            .await;
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_scheduler_run_after(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_scheduler_run_after_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_scheduler_run_after_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeSchedulerRunAfterPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self.execute_schedule_command_with_execution_context(
            ConvexScheduledCommand::RunAfter {
                delay_ms: payload.delay_ms,
                name: payload.name,
                visibility: payload.visibility,
                args: payload.args,
            },
        );
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_scheduler_run_at_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeSchedulerRunAtPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self
            .execute_schedule_command_with_execution_context_async(
                ConvexScheduledCommand::RunAt {
                    timestamp_ms: payload.timestamp_ms,
                    name: payload.name,
                    visibility: payload.visibility,
                    args: payload.args,
                },
                cancellation,
            )
            .await;
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_scheduler_run_at(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_scheduler_run_at_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_scheduler_run_at_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeSchedulerRunAtPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response =
            self.execute_schedule_command_with_execution_context(ConvexScheduledCommand::RunAt {
                timestamp_ms: payload.timestamp_ms,
                name: payload.name,
                visibility: payload.visibility,
                args: payload.args,
            });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_scheduler_cancel_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeSchedulerCancelPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self
            .execute_schedule_command_with_execution_context_async(
                ConvexScheduledCommand::Cancel {
                    job_id: payload.job_id,
                },
                cancellation,
            )
            .await;
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_scheduler_cancel(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_scheduler_cancel_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_scheduler_cancel_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeSchedulerCancelPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response =
            self.execute_schedule_command_with_execution_context(ConvexScheduledCommand::Cancel {
                job_id: payload.job_id,
            });
        encode_runtime_core_result(response)
    }
}
