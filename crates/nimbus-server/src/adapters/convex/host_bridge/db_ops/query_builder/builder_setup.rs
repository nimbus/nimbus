use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn invoke_ctx_query_start(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeQueryStartPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let builder_id = self.new_builder_id();
        self.insert_builder(
            builder_id.clone(),
            ConvexRuntimeQueryBuilderState {
                table: payload.table,
                filters: Vec::new(),
                order: None,
                order_field_hint: None,
                index_name: None,
            },
        );
        serde_json::to_value(ConvexRuntimeResponseEnvelope::ok(Value::String(builder_id)))
            .map_err(NimbusRuntimeError::from)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_with_index(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeQueryWithIndexPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self
            .with_builder_mut(&payload.builder_id, |builder| {
                let order_field_hint = self
                    .lookup_index_primary_field(&builder.table, &payload.index_name)?
                    .or_else(|| payload.filters.first().map(|filter| filter.field.clone()));
                builder.filters.extend(payload.filters);
                builder.index_name = Some(payload.index_name);
                if builder.order_field_hint.is_none() {
                    builder.order_field_hint = order_field_hint;
                }
                Ok(())
            })
            .map(|_| Value::Null)
            .map(ConvexRuntimeResponseEnvelope::ok)
            .unwrap_or_else(ConvexRuntimeResponseEnvelope::from_core_error);
        serde_json::to_value(response).map_err(NimbusRuntimeError::from)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_filter(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeQueryFilterPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self
            .with_builder_mut(&payload.builder_id, |builder| {
                if builder.order_field_hint.is_none() {
                    builder.order_field_hint =
                        payload.filters.first().map(|filter| filter.field.clone());
                }
                builder.filters.extend(payload.filters);
                Ok(())
            })
            .map(|_| Value::Null)
            .map(ConvexRuntimeResponseEnvelope::ok)
            .unwrap_or_else(ConvexRuntimeResponseEnvelope::from_core_error);
        serde_json::to_value(response).map_err(NimbusRuntimeError::from)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_order(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeQueryOrderPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self
            .with_builder_mut(&payload.builder_id, |builder| {
                let field = builder.order_field_hint.clone().ok_or_else(|| {
                    Error::InvalidInput(
                        "ctx.db.query(...).order(...) requires withIndex(...) or filter(...)"
                            .to_string(),
                    )
                })?;
                builder.order = Some(OrderBy {
                    field,
                    direction: payload.direction,
                });
                Ok(())
            })
            .map(|_| Value::Null)
            .map(ConvexRuntimeResponseEnvelope::ok)
            .unwrap_or_else(ConvexRuntimeResponseEnvelope::from_core_error);
        serde_json::to_value(response).map_err(NimbusRuntimeError::from)
    }
}
