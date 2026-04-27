use serde::de::DeserializeOwned;

use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn invoke_ctx_query_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryPayload =
            prepare_direct_ctx_payload(self, payload, cancellation)?;
        self.record_executable_query_read(&payload.query);
        let query = payload.query;
        let response = self
            .execute_query_with_execution_context_async_cancellable(
                query.clone(),
                self.auth(),
                cancellation,
            )
            .await
            .inspect(|value| self.record_query_result_value(&query, value));
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        invoke_with_default_cancellation(self, payload, Self::invoke_ctx_query_cancellable)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryPayload =
            prepare_direct_ctx_payload(self, payload, cancellation)?;
        self.record_executable_query_read(&payload.query);
        let query = payload.query;
        let response = self
            .execute_query_with_execution_context_cancellable(
                query.clone(),
                self.auth(),
                cancellation,
            )
            .inspect(|value| self.record_query_result_value(&query, value));
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_paginated_query_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimePaginatedQueryPayload =
            prepare_direct_ctx_payload(self, payload, cancellation)?;
        let query = payload.query;
        let after = payload.cursor.map(Cursor);
        let page_size = payload.page_size;
        let response = self
            .paginate_query_with_execution_context_async_cancellable(
                query.clone(),
                page_size,
                after.clone(),
                self.principal(),
                cancellation,
            )
            .await
            .and_then(|page| {
                finalize_paginated_runtime_response(self, &query, page_size, after.as_ref(), page)
            });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_paginated_query(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        invoke_with_default_cancellation(
            self,
            payload,
            Self::invoke_ctx_paginated_query_cancellable,
        )
    }

    pub(in crate::adapters::convex) fn invoke_ctx_paginated_query_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimePaginatedQueryPayload =
            prepare_direct_ctx_payload(self, payload, cancellation)?;
        let query = payload.query;
        let after = payload.cursor.map(Cursor);
        let page_size = payload.page_size;
        let response = self
            .paginate_query_with_execution_context_cancellable(
                query.clone(),
                page_size,
                after.clone(),
                self.principal(),
                cancellation,
            )
            .and_then(|page| {
                finalize_paginated_runtime_response(self, &query, page_size, after.as_ref(), page)
            });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_mutation_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeMutationPayload =
            prepare_direct_ctx_payload(self, payload, cancellation)?;
        let response = self
            .dispatch_convex_mutation_with_execution_context_async_cancellable(
                payload.mutation,
                self.auth(),
                cancellation,
            )
            .await;
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_mutation(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        invoke_with_default_cancellation(self, payload, Self::invoke_ctx_mutation_cancellable)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_mutation_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeMutationPayload =
            prepare_direct_ctx_payload(self, payload, cancellation)?;
        let response = self.dispatch_convex_mutation_with_execution_context_cancellable(
            payload.mutation,
            self.auth(),
            cancellation,
        );
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_action_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeActionPayload =
            prepare_direct_ctx_payload(self, payload, cancellation)?;
        let response = execute_convex_action_async(
            self.service(),
            self.registry(),
            self.tenant_id(),
            payload.action,
            self.auth(),
            Some(cancellation.clone()),
        )
        .await;
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_action(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        invoke_with_default_cancellation(self, payload, Self::invoke_ctx_action_cancellable)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_action_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeActionPayload =
            prepare_direct_ctx_payload(self, payload, cancellation)?;
        let response = execute_convex_action_cancellable_with_auth(
            self.service(),
            self.registry(),
            self.tenant_id(),
            payload.action,
            self.auth(),
            cancellation,
        );
        encode_runtime_core_result(response)
    }
}

trait DirectCtxPayload {
    fn session_id(&self) -> Option<&str>;
}

impl DirectCtxPayload for ConvexRuntimeQueryPayload {
    fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }
}

impl DirectCtxPayload for ConvexRuntimePaginatedQueryPayload {
    fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }
}

impl DirectCtxPayload for ConvexRuntimeMutationPayload {
    fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }
}

impl DirectCtxPayload for ConvexRuntimeActionPayload {
    fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }
}

fn invoke_with_default_cancellation(
    bridge: &ConvexHostBridge,
    payload: Value,
    invoke: impl FnOnce(
        &ConvexHostBridge,
        Value,
        &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError>,
) -> std::result::Result<Value, NeovexRuntimeError> {
    let cancellation = HostCallCancellation::default();
    invoke(bridge, payload, &cancellation)
}

fn prepare_direct_ctx_payload<P>(
    bridge: &ConvexHostBridge,
    payload: Value,
    cancellation: &HostCallCancellation,
) -> std::result::Result<P, NeovexRuntimeError>
where
    P: DeserializeOwned + DirectCtxPayload,
{
    let payload: P = serde_json::from_value(payload)?;
    bridge.validate_session(payload.session_id())?;
    ensure_runtime_host_not_cancelled(cancellation)?;
    Ok(payload)
}

fn finalize_paginated_runtime_response(
    bridge: &ConvexHostBridge,
    query: &Query,
    page_size: usize,
    after: Option<&Cursor>,
    mut page: neovex_core::Page,
) -> Result<Value, Error> {
    synthesize_runtime_paginate_cursor(query, page_size, &mut page)?;
    bridge.record_paginated_window_read(query, page_size, after, &page);
    let value =
        serde_json::to_value(page).map_err(|error| Error::Serialization(error.to_string()))?;
    bridge.record_result_documents(&query.table, &value);
    Ok(value)
}
