use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn invoke_http_route_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        ensure_runtime_host_not_cancelled(cancellation)?;
        let payload: ConvexRuntimeHttpRouteInvokePayload = serde_json::from_value(payload)?;
        let (request_context, route) = self.resolve_runtime_http_route(&payload)?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = prepare_http_action_response_async(
            self.engine(),
            self.registry(),
            self.tenant_id(),
            &route.plan,
            &request_context,
            self.auth(),
            Some(cancellation.clone()),
        )
        .await
        .and_then(|parts| {
            serde_json::to_value(parts).map_err(|error| Error::Serialization(error.to_string()))
        })
        .map(ConvexRuntimeResponseEnvelope::ok)
        .unwrap_or_else(ConvexRuntimeResponseEnvelope::from_core_error);

        serde_json::to_value(response).map_err(NimbusRuntimeError::from)
    }

    pub(in crate::adapters::convex) fn invoke_http_route(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_http_route_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_http_route_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        ensure_runtime_host_not_cancelled(cancellation)?;
        let payload: ConvexRuntimeHttpRouteInvokePayload = serde_json::from_value(payload)?;
        let (request_context, route) = self.resolve_runtime_http_route(&payload)?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = prepare_http_action_response_cancellable(
            self.engine(),
            self.registry(),
            self.tenant_id(),
            &route.plan,
            &request_context,
            self.auth(),
            cancellation,
        )
        .and_then(|parts| {
            serde_json::to_value(parts).map_err(|error| Error::Serialization(error.to_string()))
        })
        .map(ConvexRuntimeResponseEnvelope::ok)
        .unwrap_or_else(ConvexRuntimeResponseEnvelope::from_core_error);

        serde_json::to_value(response).map_err(NimbusRuntimeError::from)
    }

    fn resolve_runtime_http_route<'a>(
        &'a self,
        payload: &ConvexRuntimeHttpRouteInvokePayload,
    ) -> std::result::Result<
        (ConvexHttpRequestContext, &'a ConvexHttpRouteDefinition),
        NimbusRuntimeError,
    > {
        if &payload.request.kind != self.invocation_kind()
            || payload.request.function_name != self.current_function_name()
        {
            return Err(NimbusRuntimeError::Contract(format!(
                "runtime http route request {} ({:?}) does not match active host invocation {} ({:?})",
                payload.request.function_name,
                payload.request.kind,
                self.current_function_name(),
                self.invocation_kind()
            )));
        }

        let request_context: ConvexHttpRequestContext =
            serde_json::from_value(payload.request.args.clone())?;
        let method = Method::from_bytes(request_context.method.as_bytes()).map_err(|error| {
            NimbusRuntimeError::Contract(format!(
                "runtime http route request has invalid method {}: {error}",
                request_context.method
            ))
        })?;
        let route = self
            .registry()
            .resolve_http_route(&method, &request_context.pathname)
            .ok_or_else(|| {
                NimbusRuntimeError::Contract(format!(
                    "runtime http route is not registered for {} {}",
                    request_context.method, request_context.pathname
                ))
            })?;
        validate_runtime_http_route(&payload.request, route)?;
        Ok((request_context, route))
    }
}
