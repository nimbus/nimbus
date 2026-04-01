use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn invoke_ctx_db_get(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_get_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_get_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeDbGetPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        self.record_document_read(&payload.table, &payload.id);
        let response = match self
            .service
            .get_document(&self.tenant_id, &payload.table, payload.id)
        {
            Ok(document) => ConvexRuntimeResponseEnvelope::ok(document.to_json()),
            Err(Error::DocumentNotFound(_)) => ConvexRuntimeResponseEnvelope::ok(Value::Null),
            Err(error) => ConvexRuntimeResponseEnvelope::from_core_error(error),
        };
        serde_json::to_value(response).map_err(NeovexRuntimeError::from)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_insert(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_insert_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_insert_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeDbInsertPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = dispatch_mutation(
            &self.service,
            &self.tenant_id,
            Mutation::Insert {
                table: payload.table,
                fields: payload.fields,
            },
        );
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_patch(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_patch_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_patch_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeDbPatchPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = dispatch_mutation(
            &self.service,
            &self.tenant_id,
            Mutation::Update {
                table: payload.table,
                id: payload.id,
                patch: payload.patch,
            },
        );
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_delete(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_delete_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_delete_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeDbDeletePayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = dispatch_mutation(
            &self.service,
            &self.tenant_id,
            Mutation::Delete {
                table: payload.table,
                id: payload.id,
            },
        );
        encode_runtime_core_result(response)
    }
}
