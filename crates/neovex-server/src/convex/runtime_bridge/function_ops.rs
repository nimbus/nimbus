use super::*;

impl ConvexRuntimeBridge {
    pub(in crate::convex) fn invoke_http_route(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_http_route_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_http_route_cancellable(
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
            cancellation,
        )
        .and_then(|parts| {
            serde_json::to_value(parts).map_err(|error| Error::Serialization(error.to_string()))
        })
        .map(ConvexRuntimeResponseEnvelope::ok)
        .unwrap_or_else(ConvexRuntimeResponseEnvelope::from_core_error);

        serde_json::to_value(response).map_err(NeovexRuntimeError::from)
    }

    pub(in crate::convex) fn invoke_ctx_query(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_query_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        self.record_executable_query_read(&payload.query);
        let query = payload.query;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let mut check_cancel = || check_host_cancellation(cancellation);
        let response = execute_query_result_cancellable(
            &self.service,
            &self.tenant_id,
            query.clone(),
            &mut check_cancel,
        )
        .inspect(|value| self.record_query_result_value(&query, value));
        encode_runtime_core_result(response)
    }

    pub(in crate::convex) fn invoke_ctx_paginated_query(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_paginated_query_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_paginated_query_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimePaginatedQueryPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let table = payload.query.table.clone();
        let query = payload.query;
        let after = payload.cursor.map(Cursor);
        let page_size = payload.page_size;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let mut check_cancel = || check_host_cancellation(cancellation);
        let response = self
            .service
            .paginate_documents_cancellable(
                &self.tenant_id,
                &PaginatedQuery {
                    query: query.clone(),
                    page_size,
                    after: after.clone(),
                },
                &mut check_cancel,
            )
            .and_then(|page| {
                self.record_paginated_window_read(&query, page_size, after.as_ref(), &page);
                let value = serde_json::to_value(page)
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                self.record_result_documents(&table, &value);
                Ok(value)
            });
        encode_runtime_core_result(response)
    }

    pub(in crate::convex) fn invoke_ctx_mutation(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_mutation_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_mutation_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeMutationPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = dispatch_convex_mutation_cancellable(
            &self.service,
            &self.registry,
            &self.tenant_id,
            payload.mutation,
            cancellation,
        );
        encode_runtime_core_result(response)
    }

    pub(in crate::convex) fn invoke_ctx_action(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_action_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_action_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeActionPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = execute_convex_action_cancellable(
            &self.service,
            &self.registry,
            &self.tenant_id,
            payload.action,
            cancellation,
        );
        encode_runtime_core_result(response)
    }

    pub(in crate::convex) fn invoke_ctx_run_query(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_run_query_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_run_query_cancellable(
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

    pub(in crate::convex) fn invoke_ctx_runtime_enter_nested_call(
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

    pub(in crate::convex) fn invoke_ctx_run_mutation(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_run_mutation_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_run_mutation_cancellable(
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

    pub(in crate::convex) fn invoke_ctx_run_action(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_run_action_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_run_action_cancellable(
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

    pub(in crate::convex) fn execute_runtime_function_call_cancellable(
        &self,
        kind: InvocationKind,
        name: &str,
        args: &Value,
        visibility: ConvexFunctionVisibility,
        auth: Option<InvocationAuth>,
        cancellation: &HostCallCancellation,
    ) -> Result<Value, Error> {
        if self.should_use_nested_runtime(kind.clone(), name, visibility)? {
            return self.invoke_nested_runtime_function_cancellable(
                kind,
                name,
                args,
                visibility,
                auth,
                cancellation,
            );
        }

        ensure_runtime_host_not_cancelled(cancellation).map_err(runtime_error_to_core)?;

        match kind {
            InvocationKind::Query => {
                let query = self
                    .registry
                    .resolve_query_for_visibility(name, args, visibility)?;
                let mut check_cancel = || check_host_cancellation(cancellation);
                execute_query_result_cancellable(
                    &self.service,
                    &self.tenant_id,
                    query,
                    &mut check_cancel,
                )
            }
            InvocationKind::PaginatedQuery => Err(Error::InvalidInput(
                "ctx.runQuery does not support paginated queries".to_string(),
            )),
            InvocationKind::Mutation => {
                let mutation = self
                    .registry
                    .resolve_mutation_for_visibility(name, args, visibility)?;
                dispatch_convex_mutation_cancellable(
                    &self.service,
                    &self.registry,
                    &self.tenant_id,
                    mutation,
                    cancellation,
                )
            }
            InvocationKind::Action => {
                let action = self
                    .registry
                    .resolve_action_for_visibility(name, args, visibility)?;
                execute_convex_action_cancellable(
                    &self.service,
                    &self.registry,
                    &self.tenant_id,
                    action,
                    cancellation,
                )
            }
        }
    }

    pub(in crate::convex) fn should_use_nested_runtime(
        &self,
        kind: InvocationKind,
        name: &str,
        visibility: ConvexFunctionVisibility,
    ) -> Result<bool, Error> {
        let Some(bundle) = self.registry.runtime_bundle() else {
            return Ok(false);
        };
        let _ = bundle;
        let definition = self
            .registry
            .functions
            .get(name)
            .ok_or_else(|| Error::InvalidInput(format!("convex function not found: {name}")))?;
        if definition.visibility != visibility {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is {}, not {}",
                definition.visibility.as_str(),
                visibility.as_str()
            )));
        }
        let expected_kind = match kind {
            InvocationKind::Query => ConvexFunctionKind::Query,
            InvocationKind::PaginatedQuery => ConvexFunctionKind::PaginatedQuery,
            InvocationKind::Mutation => ConvexFunctionKind::Mutation,
            InvocationKind::Action => ConvexFunctionKind::Action,
        };
        if definition.kind != expected_kind {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is a {}, not a {}",
                definition.kind.as_str(),
                expected_kind.as_str()
            )));
        }
        Ok(definition.runtime_handler.is_some() && definition.plan.is_null())
    }

    pub(in crate::convex) fn invoke_nested_runtime_function_cancellable(
        &self,
        kind: InvocationKind,
        name: &str,
        args: &Value,
        visibility: ConvexFunctionVisibility,
        auth: Option<InvocationAuth>,
        cancellation: &HostCallCancellation,
    ) -> Result<Value, Error> {
        self.consume_nested_runtime_invocation_budget()?;
        self.registry
            .runtime_policy()
            .metrics()
            .record_fallback_cross_isolate_dispatch();
        tracing::debug!(
            tenant = %self.tenant_id,
            function = %name,
            kind = kind.as_str(),
            visibility = %visibility.as_str(),
            "convex runtime using cross-isolate fallback dispatch"
        );
        let definition = self
            .registry
            .functions
            .get(name)
            .ok_or_else(|| Error::InvalidInput(format!("convex function not found: {name}")))?;
        if definition.visibility != visibility {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is {}, not {}",
                definition.visibility.as_str(),
                visibility.as_str()
            )));
        }
        let bundle = self
            .registry
            .runtime_bundle()
            .cloned()
            .ok_or_else(|| Error::Internal("convex runtime bundle not loaded".to_string()))?;
        let runtime = NeovexRuntime::with_policy_bypassing_limit(
            Arc::new(self.clone()),
            self.registry.runtime_policy(),
        );
        let request = InvocationRequest {
            kind,
            function_name: name.to_string(),
            args: args.clone(),
            page_size: None,
            cursor: None,
            auth,
        };
        let response = self
            .registry
            .runtime_executor()
            .invoke_blocking_with_cancellation(
                runtime,
                bundle,
                request.clone(),
                match self.server_request_id.as_deref() {
                    Some(server_request_id) => {
                        RuntimeInvocationContext::top_level_for_tenant_and_request(
                            &request,
                            self.tenant_id.to_string(),
                            server_request_id,
                        )
                    }
                    None => RuntimeInvocationContext::top_level_for_tenant(
                        &request,
                        self.tenant_id.to_string(),
                    ),
                },
                Some(cancellation.clone()),
            )
            .map_err(runtime_error_to_core)?;
        let envelope: ConvexRuntimeResponseEnvelope = serde_json::from_value(response)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        envelope.into_core_result()
    }
}
