use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn invoke_ctx_query_collect_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryTerminalPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = match self.take_builder(&payload.builder_id) {
            Ok(builder) => {
                let query = builder.clone().into_query(None);
                self.record_builder_read(&builder, &query);
                self.execute_query_with_execution_context_async_cancellable(
                    ConvexExecutableQuery::Query(query),
                    self.auth.as_ref(),
                    cancellation,
                )
                .await
                .inspect(|value| self.record_result_documents(&builder.table, value))
            }
            Err(error) => Err(error),
        };
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_collect(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_collect_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_collect_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryTerminalPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self.take_builder(&payload.builder_id).and_then(|builder| {
            let query = builder.clone().into_query(None);
            self.record_builder_read(&builder, &query);
            self.execute_query_with_execution_context_cancellable(
                ConvexExecutableQuery::Query(query),
                self.auth.as_ref(),
                cancellation,
            )
            .inspect(|value| self.record_result_documents(&builder.table, value))
        });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_query_paginate_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryPaginatePayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = match self.take_builder(&payload.builder_id) {
            Ok(builder) => {
                let query = builder.clone().into_query(None);
                let after = payload.cursor.map(Cursor);
                self.paginate_query_with_execution_context_async_cancellable(
                    query.clone(),
                    payload.page_size,
                    after.clone(),
                    &self.principal,
                    cancellation,
                )
                .await
                .and_then(|page| {
                    self.record_paginated_window_read(
                        &query,
                        payload.page_size,
                        after.as_ref(),
                        &page,
                    );
                    let value = serde_json::to_value(page)
                        .map_err(|error| Error::Serialization(error.to_string()))?;
                    self.record_result_documents(&builder.table, &value);
                    Ok(value)
                })
            }
            Err(error) => Err(error),
        };
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_paginate(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_paginate_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_paginate_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryPaginatePayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self.take_builder(&payload.builder_id).and_then(|builder| {
            let query = builder.clone().into_query(None);
            let after = payload.cursor.map(Cursor);
            self.paginate_query_with_execution_context_cancellable(
                query.clone(),
                payload.page_size,
                after.clone(),
                &self.principal,
                cancellation,
            )
            .and_then(|page| {
                self.record_paginated_window_read(&query, payload.page_size, after.as_ref(), &page);
                let value = serde_json::to_value(page)
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                self.record_result_documents(&builder.table, &value);
                Ok(value)
            })
        });
        encode_runtime_core_result(response)
    }
}
