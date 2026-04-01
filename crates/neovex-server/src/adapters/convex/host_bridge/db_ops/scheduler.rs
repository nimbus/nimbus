use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn invoke_ctx_scheduler_run_after(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_scheduler_run_after_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_scheduler_run_after_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeSchedulerRunAfterPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = execute_schedule_command(
            &self.service,
            &self.registry,
            &self.tenant_id,
            ConvexScheduledCommand::RunAfter {
                delay_ms: payload.delay_ms,
                name: payload.name,
                visibility: payload.visibility,
                args: payload.args,
            },
        );
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_scheduler_run_at(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_scheduler_run_at_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_scheduler_run_at_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeSchedulerRunAtPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = execute_schedule_command(
            &self.service,
            &self.registry,
            &self.tenant_id,
            ConvexScheduledCommand::RunAt {
                timestamp_ms: payload.timestamp_ms,
                name: payload.name,
                visibility: payload.visibility,
                args: payload.args,
            },
        );
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_scheduler_cancel(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_scheduler_cancel_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_scheduler_cancel_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeSchedulerCancelPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = execute_schedule_command(
            &self.service,
            &self.registry,
            &self.tenant_id,
            ConvexScheduledCommand::Cancel {
                job_id: payload.job_id,
            },
        );
        encode_runtime_core_result(response)
    }
}
