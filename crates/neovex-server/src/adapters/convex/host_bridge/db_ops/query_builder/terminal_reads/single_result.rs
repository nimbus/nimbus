use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn invoke_ctx_query_take_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryTakePayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = match self.take_builder(&payload.builder_id) {
            Ok(builder) => {
                let query = builder.clone().into_query(Some(payload.limit));
                let tracked_query = query.clone();
                if query.order.is_none() {
                    self.record_builder_read(&builder, &query);
                }
                self.execute_query_with_execution_context_async_cancellable(
                    ConvexExecutableQuery::Query(query),
                    self.auth(),
                    cancellation,
                )
                .await
                .and_then(|value| {
                    self.record_limited_query_window(&tracked_query, payload.limit, &value)?;
                    self.record_result_documents(&builder.table, &value);
                    Ok(value)
                })
            }
            Err(error) => Err(error),
        };
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_take(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_take_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_take_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryTakePayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self.take_builder(&payload.builder_id).and_then(|builder| {
            let query = builder.clone().into_query(Some(payload.limit));
            let tracked_query = query.clone();
            if query.order.is_none() {
                self.record_builder_read(&builder, &query);
            }
            self.execute_query_with_execution_context_cancellable(
                ConvexExecutableQuery::Query(query),
                self.auth(),
                cancellation,
            )
            .and_then(|value| {
                self.record_limited_query_window(&tracked_query, payload.limit, &value)?;
                self.record_result_documents(&builder.table, &value);
                Ok(value)
            })
        });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_query_first_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryTerminalPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = match self.take_builder(&payload.builder_id) {
            Ok(builder) => {
                let query = builder.clone().into_query(Some(1));
                let tracked_query = query.clone();
                if query.order.is_none() {
                    self.record_builder_read(&builder, &query);
                }
                self.execute_query_with_execution_context_async_cancellable(
                    ConvexExecutableQuery::Read(ConvexReadCommand::First { query }),
                    self.auth(),
                    cancellation,
                )
                .await
                .and_then(|value| {
                    self.record_limited_query_window(&tracked_query, 1, &value)?;
                    self.record_result_documents(&builder.table, &value);
                    Ok(value)
                })
            }
            Err(error) => Err(error),
        };
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_first(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_first_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_first_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryTerminalPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self.take_builder(&payload.builder_id).and_then(|builder| {
            let query = builder.clone().into_query(Some(1));
            let tracked_query = query.clone();
            if query.order.is_none() {
                self.record_builder_read(&builder, &query);
            }
            self.execute_query_with_execution_context_cancellable(
                ConvexExecutableQuery::Read(ConvexReadCommand::First { query }),
                self.auth(),
                cancellation,
            )
            .and_then(|value| {
                self.record_limited_query_window(&tracked_query, 1, &value)?;
                self.record_result_documents(&builder.table, &value);
                Ok(value)
            })
        });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_query_unique_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryTerminalPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = match self.take_builder(&payload.builder_id) {
            Ok(builder) => {
                let query = builder.clone().into_query(Some(2));
                let tracked_query = query.clone();
                if query.order.is_none() {
                    self.record_builder_read(&builder, &query);
                }
                self.execute_query_with_execution_context_async_cancellable(
                    ConvexExecutableQuery::Read(ConvexReadCommand::Unique { query }),
                    self.auth(),
                    cancellation,
                )
                .await
                .and_then(|value| {
                    self.record_limited_query_window(&tracked_query, 2, &value)?;
                    self.record_result_documents(&builder.table, &value);
                    Ok(value)
                })
            }
            Err(error) => Err(error),
        };
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_unique(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_unique_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_unique_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryTerminalPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self.take_builder(&payload.builder_id).and_then(|builder| {
            let query = builder.clone().into_query(Some(2));
            let tracked_query = query.clone();
            if query.order.is_none() {
                self.record_builder_read(&builder, &query);
            }
            self.execute_query_with_execution_context_cancellable(
                ConvexExecutableQuery::Read(ConvexReadCommand::Unique { query }),
                self.auth(),
                cancellation,
            )
            .and_then(|value| {
                self.record_limited_query_window(&tracked_query, 2, &value)?;
                self.record_result_documents(&builder.table, &value);
                Ok(value)
            })
        });
        encode_runtime_core_result(response)
    }
}
