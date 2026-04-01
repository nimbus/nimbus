use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn invoke_ctx_query(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_cancellable(
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

    pub(in crate::adapters::convex) fn invoke_ctx_paginated_query(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_paginated_query_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_paginated_query_cancellable(
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

    pub(in crate::adapters::convex) fn invoke_ctx_mutation(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_mutation_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_mutation_cancellable(
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

    pub(in crate::adapters::convex) fn invoke_ctx_action(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_action_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_action_cancellable(
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
}
