use super::*;
use nimbus_bridge::capabilities::{
    delete_document, delete_document_async, get_document, get_document_async, insert_document,
    insert_document_async, update_document, update_document_async,
};
use nimbus_core::Document;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn invoke_ctx_db_get_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeDbGetPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let table = payload.table.clone();
        let document_id = match resolve_convex_document_id(&table, payload.id) {
            Ok(resolved) => resolved.into_document_id(),
            Err(error) => {
                let response = ConvexRuntimeResponseEnvelope::from_core_error(error);
                return serde_json::to_value(response).map_err(NimbusRuntimeError::from);
            }
        };
        let locator = nimbus_core::DocumentLocator::new(table, document_id);
        let response =
            convex_document_get_response(get_document_async(self, &locator, cancellation).await);
        serde_json::to_value(response).map_err(NimbusRuntimeError::from)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_get(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_get_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_get_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeDbGetPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let table = payload.table.clone();
        let document_id = match resolve_convex_document_id(&table, payload.id) {
            Ok(resolved) => resolved.into_document_id(),
            Err(error) => {
                let response = ConvexRuntimeResponseEnvelope::from_core_error(error);
                return serde_json::to_value(response).map_err(NimbusRuntimeError::from);
            }
        };
        let locator = nimbus_core::DocumentLocator::new(table, document_id);
        let response = convex_document_get_response(get_document(self, &locator));
        serde_json::to_value(response).map_err(NimbusRuntimeError::from)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_db_insert_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeDbInsertPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let table = payload.table;
        let table_for_id = table.clone();
        let fields = payload.fields;
        let response = insert_document_async(self, table, fields, cancellation)
            .await
            .and_then(|id| {
                encode_convex_document_id(&table_for_id, &id)
                    .map(|scoped| Value::String(scoped.to_string()))
            });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_insert(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_insert_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_insert_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeDbInsertPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let table = payload.table;
        let table_for_id = table.clone();
        let fields = payload.fields;
        let response = insert_document(self, table, fields).and_then(|id| {
            encode_convex_document_id(&table_for_id, &id)
                .map(|scoped| Value::String(scoped.to_string()))
        });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_db_patch_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeDbPatchPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let table = payload.table;
        let id = match resolve_convex_document_id(&table, payload.id) {
            Ok(resolved) => resolved.into_document_id(),
            Err(error) => return encode_runtime_core_result(Err(error)),
        };
        let table_for_id = table.clone();
        let patch = payload.patch;
        let response = update_document_async(self, table, id, patch, cancellation)
            .await
            .and_then(|id| {
                encode_convex_document_id(&table_for_id, &id)
                    .map(|scoped| Value::String(scoped.to_string()))
            });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_patch(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_patch_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_patch_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeDbPatchPayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let table = payload.table;
        let id = match resolve_convex_document_id(&table, payload.id) {
            Ok(resolved) => resolved.into_document_id(),
            Err(error) => return encode_runtime_core_result(Err(error)),
        };
        let table_for_id = table.clone();
        let patch = payload.patch;
        let response = update_document(self, table, id, patch).and_then(|id| {
            encode_convex_document_id(&table_for_id, &id)
                .map(|scoped| Value::String(scoped.to_string()))
        });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_db_delete_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeDbDeletePayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let table = payload.table;
        let id = match resolve_convex_document_id(&table, payload.id) {
            Ok(resolved) => resolved.into_document_id(),
            Err(error) => return encode_runtime_core_result(Err(error)),
        };
        let response = delete_document_async(self, table, id, cancellation)
            .await
            .map(|_| Value::Null);
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_delete(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_delete_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_db_delete_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let payload: ConvexRuntimeDbDeletePayload = serde_json::from_value(payload)?;
        self.validate_host_call_session(payload.host_call_session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let table = payload.table;
        let id = match resolve_convex_document_id(&table, payload.id) {
            Ok(resolved) => resolved.into_document_id(),
            Err(error) => return encode_runtime_core_result(Err(error)),
        };
        let response = delete_document(self, table, id).map(|_| Value::Null);
        encode_runtime_core_result(response)
    }
}

fn convex_document_get_response(
    result: nimbus_core::Result<Option<Document>>,
) -> ConvexRuntimeResponseEnvelope {
    match result {
        Ok(Some(document)) => match document_to_convex_json(document) {
            Ok(value) => ConvexRuntimeResponseEnvelope::ok(value),
            Err(error) => ConvexRuntimeResponseEnvelope::from_core_error(error),
        },
        Ok(None) => ConvexRuntimeResponseEnvelope::ok(Value::Null),
        Err(error) => ConvexRuntimeResponseEnvelope::from_core_error(error),
    }
}
