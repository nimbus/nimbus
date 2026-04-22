use super::*;

struct NestedRuntimeInvocationPlan {
    bundle: RuntimeBundle,
    request: InvocationRequest,
}

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn execute_runtime_function_call_async_cancellable(
        &self,
        kind: InvocationKind,
        name: &str,
        args: &Value,
        visibility: ConvexFunctionVisibility,
        auth: Option<InvocationAuth>,
        cancellation: &HostCallCancellation,
    ) -> Result<Value, Error> {
        if self.should_use_nested_runtime(kind.clone(), name, visibility)? {
            return self
                .invoke_nested_runtime_function_async_cancellable(
                    kind,
                    name,
                    args,
                    visibility,
                    auth,
                    cancellation,
                )
                .await;
        }

        ensure_runtime_host_not_cancelled(cancellation).map_err(runtime_error_to_core)?;

        match kind {
            InvocationKind::Query => {
                let query = self
                    .registry
                    .resolve_query_for_visibility(name, args, visibility)?;
                self.execute_query_with_execution_context_async_cancellable(
                    query,
                    auth.as_ref(),
                    cancellation,
                )
                .await
            }
            InvocationKind::PaginatedQuery => Err(Error::InvalidInput(
                "ctx.runQuery does not support paginated queries".to_string(),
            )),
            InvocationKind::Mutation => {
                let mutation = self
                    .registry
                    .resolve_mutation_for_visibility(name, args, visibility)?;
                self.dispatch_convex_mutation_with_execution_context_async_cancellable(
                    mutation,
                    auth.as_ref(),
                    cancellation,
                )
                .await
            }
            InvocationKind::Action => {
                let action = self
                    .registry
                    .resolve_action_for_visibility(name, args, visibility)?;
                execute_convex_action_async(
                    &self.service,
                    &self.registry,
                    &self.tenant_id,
                    action,
                    auth.as_ref(),
                    Some(cancellation.clone()),
                )
                .await
            }
        }
    }

    pub(in crate::adapters::convex) fn execute_runtime_function_call_cancellable(
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
                self.execute_query_with_execution_context_cancellable(
                    query,
                    auth.as_ref(),
                    cancellation,
                )
            }
            InvocationKind::PaginatedQuery => Err(Error::InvalidInput(
                "ctx.runQuery does not support paginated queries".to_string(),
            )),
            InvocationKind::Mutation => {
                let mutation = self
                    .registry
                    .resolve_mutation_for_visibility(name, args, visibility)?;
                self.dispatch_convex_mutation_with_execution_context_cancellable(
                    mutation,
                    auth.as_ref(),
                    cancellation,
                )
            }
            InvocationKind::Action => {
                let action = self
                    .registry
                    .resolve_action_for_visibility(name, args, visibility)?;
                execute_convex_action_cancellable_with_auth(
                    &self.service,
                    &self.registry,
                    &self.tenant_id,
                    action,
                    auth.as_ref(),
                    cancellation,
                )
            }
        }
    }

    pub(in crate::adapters::convex) async fn invoke_nested_runtime_function_async_cancellable(
        &self,
        kind: InvocationKind,
        name: &str,
        args: &Value,
        visibility: ConvexFunctionVisibility,
        auth: Option<InvocationAuth>,
        cancellation: &HostCallCancellation,
    ) -> Result<Value, Error> {
        let NestedRuntimeInvocationPlan { bundle, request } =
            self.prepare_nested_runtime_invocation(kind, name, args, visibility, auth)?;
        let response = invoke_runtime_bundle_on_worker_with_host(
            &self.registry.runtime_executor(),
            self.registry.runtime_policy(),
            Arc::new(self.clone()),
            bundle,
            request,
            RuntimeBundleInvocationOptions::bypassing_policy_limit(
                &self.tenant_id,
                self.server_request_id(),
                Some(cancellation.clone()),
            ),
        )
        .await
        .map_err(runtime_error_to_core)?;
        let envelope: ConvexRuntimeResponseEnvelope = serde_json::from_value(response)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        envelope.into_core_result()
    }

    pub(in crate::adapters::convex) fn invoke_nested_runtime_function_cancellable(
        &self,
        kind: InvocationKind,
        name: &str,
        args: &Value,
        visibility: ConvexFunctionVisibility,
        auth: Option<InvocationAuth>,
        cancellation: &HostCallCancellation,
    ) -> Result<Value, Error> {
        let NestedRuntimeInvocationPlan { bundle, request } =
            self.prepare_nested_runtime_invocation(kind, name, args, visibility, auth)?;
        let response = invoke_runtime_bundle_blocking_with_host(
            &self.registry.runtime_executor(),
            self.registry.runtime_policy(),
            Arc::new(self.clone()),
            bundle,
            request,
            RuntimeBundleInvocationOptions::bypassing_policy_limit(
                &self.tenant_id,
                self.server_request_id(),
                Some(cancellation.clone()),
            ),
        )
        .map_err(runtime_error_to_core)?;
        let envelope: ConvexRuntimeResponseEnvelope = serde_json::from_value(response)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        envelope.into_core_result()
    }

    fn prepare_nested_runtime_invocation(
        &self,
        kind: InvocationKind,
        name: &str,
        args: &Value,
        visibility: ConvexFunctionVisibility,
        auth: Option<InvocationAuth>,
    ) -> Result<NestedRuntimeInvocationPlan, Error> {
        self.consume_nested_runtime_invocation_budget()?;
        self.registry
            .runtime_policy()
            .metrics()
            .record_fallback_cross_runtime_dispatch();
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
        Ok(NestedRuntimeInvocationPlan {
            bundle,
            request: InvocationRequest {
                kind,
                function_name: name.to_string(),
                args: args.clone(),
                page_size: None,
                cursor: None,
                auth,
                services: self.services.clone(),
            },
        })
    }
}
