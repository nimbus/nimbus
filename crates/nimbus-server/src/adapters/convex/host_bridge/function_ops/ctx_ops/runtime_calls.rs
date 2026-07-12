use super::*;
use nimbus_bridge::capabilities::RuntimeServiceCapabilityHost;
use nimbus_runtime::{RuntimeSyncNestedCallPayload, RuntimeSyncResolveCalleeLanePayload};

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn invoke_ctx_service_lookup_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeServiceLookupPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
        let Some(service_capabilities) = self.service_capabilities() else {
            return encode_runtime_core_result(Err(Error::PermissionDenied(
                "runtime service capability was not granted for this invocation".to_string(),
            )));
        };
        let service_access = match service_capabilities.service_access(&payload.service_name) {
            Ok(service_access) => service_access,
            Err(error) => return encode_runtime_core_result(Err(error)),
        };
        let response = self
            .runtime_service_registry()
            .ensure_service_binding_for_decision_async(&service_access, cancellation.clone())
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
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeServiceLookupPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
        let Some(service_capabilities) = self.service_capabilities() else {
            return encode_runtime_core_result(Err(Error::PermissionDenied(
                "runtime service capability was not granted for this invocation".to_string(),
            )));
        };
        let service_access = match service_capabilities.service_access(&payload.service_name) {
            Ok(service_access) => service_access,
            Err(error) => return encode_runtime_core_result(Err(error)),
        };
        let response = self
            .runtime_service_registry()
            .resolve_service_binding_for_decision(&service_access)
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
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
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
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_run_query_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_run_query_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
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
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: RuntimeSyncNestedCallPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
        self.registry()
            .runtime_policy()
            .metrics()
            .record_nested_local_dispatch();
        tracing::debug!(
            tenant = %self.tenant_id(),
            function = %payload.name,
            visibility = %payload.visibility,
            kind = payload.kind.as_deref().unwrap_or("unknown"),
            "convex runtime entered same-isolate nested local dispatch"
        );
        let response = self
            .consume_nested_runtime_invocation_budget()
            .map(|_| Value::Null)
            .map(ConvexRuntimeResponseEnvelope::ok)
            .unwrap_or_else(ConvexRuntimeResponseEnvelope::from_core_error);
        serde_json::to_value(response).map_err(NimbusRuntimeError::from)
    }

    /// Callee-lane oracle for the nested `ctx.run*` dispatcher (EX10R3.1). The
    /// runtime asks the host for the authoritative lane of `payload.name` and
    /// compares it against the isolate's frozen lane to choose local vs host
    /// dispatch. Resolving it here — from the registry the host alone owns —
    /// means no guest handler body or eagerly-imported dependency can influence
    /// that decision. `None` (unknown or non-locally-dispatchable callee) is
    /// returned as JSON null so the runtime fails safe to host dispatch.
    pub(in crate::adapters::convex) fn invoke_ctx_resolve_callee_lane(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: RuntimeSyncResolveCalleeLanePayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
        let lane = self
            .registry()
            .runtime_environment_for_function(&payload.name)
            .map(|lane| Value::String(lane.to_string()))
            .unwrap_or(Value::Null);
        encode_runtime_core_result(Ok(lane))
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_run_mutation_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
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
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_run_mutation_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_run_mutation_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
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
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
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
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_run_action_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_run_action_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
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
