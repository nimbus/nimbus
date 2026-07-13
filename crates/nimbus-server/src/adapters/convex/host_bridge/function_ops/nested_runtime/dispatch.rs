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
                    .registry()
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
                    .registry()
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
                    .registry()
                    .resolve_action_for_visibility(name, args, visibility)?;
                execute_convex_action_async(
                    self.engine(),
                    self.registry(),
                    self.tenant_id(),
                    action,
                    auth.as_ref(),
                    Some(cancellation.clone()),
                )
                .await
            }
            InvocationKind::CloudflareWorkerFetch => Err(Error::InvalidInput(
                "ctx.runAction does not support Cloudflare Worker fetch invocations".to_string(),
            )),
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
                    .registry()
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
                    .registry()
                    .resolve_mutation_for_visibility(name, args, visibility)?;
                self.dispatch_convex_mutation_with_execution_context_cancellable(
                    mutation,
                    auth.as_ref(),
                    cancellation,
                )
            }
            InvocationKind::Action => {
                let action = self
                    .registry()
                    .resolve_action_for_visibility(name, args, visibility)?;
                execute_convex_action_cancellable_with_auth(
                    self.engine(),
                    self.registry(),
                    self.tenant_id(),
                    action,
                    auth.as_ref(),
                    cancellation,
                )
            }
            InvocationKind::CloudflareWorkerFetch => Err(Error::InvalidInput(
                "ctx.runAction does not support Cloudflare Worker fetch invocations".to_string(),
            )),
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
        let (runtime_executor, runtime_policy) = self.registry().runtime_lane_for_function(name)?;
        let _host_call_session = self
            .host_state()
            .enter_host_call_session(format!(
                "{}:{}",
                request.kind.as_str(),
                request.function_name
            ))
            .map_err(runtime_error_to_core)?;
        let response = invoke_runtime_bundle_on_worker_with_egress_gateway(
            &runtime_executor,
            runtime_policy,
            Arc::new(self.retargeted_for_nested_invocation(name)),
            bundle,
            request,
            RuntimeBundleInvocationOptions::budgeted_nested_invocation_bypass(
                self.tenant_id(),
                self.server_request_id(),
                Some(cancellation.clone()),
            )
            .with_optional_runtime_bundle_provenance_gate(
                self.registry().runtime_bundle_provenance(),
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
        let (runtime_executor, runtime_policy) = self.registry().runtime_lane_for_function(name)?;
        let _host_call_session = self
            .host_state()
            .enter_host_call_session(format!(
                "{}:{}",
                request.kind.as_str(),
                request.function_name
            ))
            .map_err(runtime_error_to_core)?;
        let response = invoke_runtime_bundle_blocking_with_egress_gateway(
            &runtime_executor,
            runtime_policy,
            Arc::new(self.retargeted_for_nested_invocation(name)),
            bundle,
            request,
            RuntimeBundleInvocationOptions::budgeted_nested_invocation_bypass(
                self.tenant_id(),
                self.server_request_id(),
                Some(cancellation.clone()),
            )
            .with_optional_runtime_bundle_provenance_gate(
                self.registry().runtime_bundle_provenance(),
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
        self.registry()
            .runtime_policy()
            .metrics()
            .record_fallback_cross_runtime_dispatch();
        tracing::debug!(
            tenant = %self.tenant_id(),
            function = %name,
            kind = kind.as_str(),
            visibility = %visibility.as_str(),
            "convex runtime using cross-isolate fallback dispatch"
        );
        let definition = self
            .registry()
            .function_definition(name)
            .ok_or_else(|| Error::InvalidInput(format!("convex function not found: {name}")))?;
        if definition.visibility != visibility {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is {}, not {}",
                definition.visibility.as_str(),
                visibility.as_str()
            )));
        }
        let bundle = self
            .registry()
            .runtime_bundle()
            .cloned()
            .ok_or_else(|| Error::Internal("convex runtime bundle not loaded".to_string()))?;
        self.decision()
            .ensure_runtime_bundle_matches(&bundle, "convex nested runtime bundle")?;
        Ok(NestedRuntimeInvocationPlan {
            bundle,
            request: InvocationRequest {
                kind,
                function_name: name.to_string(),
                args: args.clone(),
                page_size: None,
                cursor: None,
                auth: auth.map(InvocationAuth::into_runtime_payload),
                services: self.services().clone(),
            },
        })
    }
}
