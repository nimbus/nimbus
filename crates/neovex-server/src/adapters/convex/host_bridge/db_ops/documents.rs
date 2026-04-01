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
        let response = match self.service.get_document_with_principal(
            &self.tenant_id,
            &payload.table,
            payload.id,
            &self.principal,
        ) {
            Ok(document) => {
                self.record_document_read(&payload.table, &payload.id);
                ConvexRuntimeResponseEnvelope::ok(document.to_json())
            }
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
        let response = self
            .service
            .insert_document_with_principal(
                &self.tenant_id,
                payload.table,
                payload.fields,
                &self.principal,
            )
            .map(|id| Value::String(id.to_string()));
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
        let response = self
            .service
            .update_document_with_principal(
                &self.tenant_id,
                payload.table,
                payload.id,
                payload.patch,
                &self.principal,
            )
            .map(|id| Value::String(id.to_string()));
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
        let response = self
            .service
            .delete_document_with_principal(
                &self.tenant_id,
                payload.table,
                payload.id,
                &self.principal,
            )
            .map(|_| Value::Null);
        encode_runtime_core_result(response)
    }
}
