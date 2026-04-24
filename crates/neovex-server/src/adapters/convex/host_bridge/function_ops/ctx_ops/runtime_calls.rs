use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn invoke_ctx_service_lookup_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeServiceLookupPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self
            .runtime_service_registry
            .ensure_service_binding_async(
                &self.tenant_id,
                &payload.service_name,
                cancellation.clone(),
            )
            .await
            .and_then(|binding| {
                serde_json::to_value(binding)
                    .map_err(|error| Error::Serialization(error.to_string()))
            });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_service_lookup(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeServiceLookupPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self
            .runtime_service_registry
            .resolve_service_binding(&self.tenant_id, &payload.service_name)
            .and_then(|binding| {
                serde_json::to_value(binding)
                    .map_err(|error| Error::Serialization(error.to_string()))
            });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_run_query_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self
            .execute_runtime_function_call_async_cancellable(
                InvocationKind::Query,
                &payload.name,
                &payload.args,
                payload
                    .visibility
                    .unwrap_or(ConvexFunctionVisibility::Public),
                payload.auth,
                cancellation,
            )
            .await;
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_run_query(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_run_query_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_run_query_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self.execute_runtime_function_call_cancellable(
            InvocationKind::Query,
            &payload.name,
            &payload.args,
            payload
                .visibility
                .unwrap_or(ConvexFunctionVisibility::Public),
            payload.auth,
            cancellation,
        );
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_runtime_enter_nested_call(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        self.registry
            .runtime_policy()
            .metrics()
            .record_nested_local_dispatch();
        tracing::debug!(
            tenant = %self.tenant_id,
            function = %payload.name,
            visibility = %payload.visibility.unwrap_or(ConvexFunctionVisibility::Public).as_str(),
            "convex runtime entered same-isolate nested local dispatch"
        );
        let response = self
            .consume_nested_runtime_invocation_budget()
            .map(|_| Value::Null)
            .map(ConvexRuntimeResponseEnvelope::ok)
            .unwrap_or_else(ConvexRuntimeResponseEnvelope::from_core_error);
        serde_json::to_value(response).map_err(NeovexRuntimeError::from)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_run_mutation_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self
            .execute_runtime_function_call_async_cancellable(
                InvocationKind::Mutation,
                &payload.name,
                &payload.args,
                payload
                    .visibility
                    .unwrap_or(ConvexFunctionVisibility::Public),
                payload.auth,
                cancellation,
            )
            .await;
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_run_mutation(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_run_mutation_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_run_mutation_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self.execute_runtime_function_call_cancellable(
            InvocationKind::Mutation,
            &payload.name,
            &payload.args,
            payload
                .visibility
                .unwrap_or(ConvexFunctionVisibility::Public),
            payload.auth,
            cancellation,
        );
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_run_action_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self
            .execute_runtime_function_call_async_cancellable(
                InvocationKind::Action,
                &payload.name,
                &payload.args,
                payload
                    .visibility
                    .unwrap_or(ConvexFunctionVisibility::Public),
                payload.auth,
                cancellation,
            )
            .await;
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_run_action(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_run_action_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_run_action_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self.execute_runtime_function_call_cancellable(
            InvocationKind::Action,
            &payload.name,
            &payload.args,
            payload
                .visibility
                .unwrap_or(ConvexFunctionVisibility::Public),
            payload.auth,
            cancellation,
        );
        encode_runtime_core_result(response)
    }
}
