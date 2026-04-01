use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn invoke_http_route(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_http_route_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_http_route_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeHttpRouteInvokePayload = serde_json::from_value(payload)?;
        validate_runtime_http_route(&payload.request, &payload.route)?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let request_context: ConvexHttpRequestContext =
            serde_json::from_value(payload.request.args.clone())?;
        let response = prepare_http_action_response_cancellable(
            &self.service,
            &self.registry,
            &self.tenant_id,
            &payload.route.plan,
            &request_context,
            self.auth.as_ref(),
            cancellation,
        )
        .and_then(|parts| {
            serde_json::to_value(parts).map_err(|error| Error::Serialization(error.to_string()))
        })
        .map(ConvexRuntimeResponseEnvelope::ok)
        .unwrap_or_else(ConvexRuntimeResponseEnvelope::from_core_error);

        serde_json::to_value(response).map_err(NeovexRuntimeError::from)
    }
}
