use super::*;

impl ConvexHostBridge {
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
            let mut check_cancel = || check_host_cancellation(cancellation);
            execute_query_result_cancellable(
                &self.service,
                &self.tenant_id,
                ConvexExecutableQuery::Query(query),
                &mut check_cancel,
            )
            .inspect(|value| self.record_result_documents(&builder.table, value))
        });
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
            let mut check_cancel = || check_host_cancellation(cancellation);
            self.service
                .paginate_documents_cancellable(
                    &self.tenant_id,
                    &PaginatedQuery {
                        query: query.clone(),
                        page_size: payload.page_size,
                        after: after.clone(),
                    },
                    &mut check_cancel,
                )
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
        });
        encode_runtime_core_result(response)
    }
}
