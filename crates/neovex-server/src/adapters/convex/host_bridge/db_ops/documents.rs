use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn invoke_ctx_db_get_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeDbGetPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = match self.mutation_execution_unit().map_or_else(
            || None,
            |execution_unit| {
                Some(
                    execution_unit
                        .get_document(&payload.table, payload.id)
                        .and_then(|document| document.ok_or(Error::DocumentNotFound(payload.id))),
                )
            },
        ) {
            Some(result) => match result {
                Ok(document) => {
                    self.record_document_read(&payload.table, &payload.id);
                    ConvexRuntimeResponseEnvelope::ok(document.into_json())
                }
                Err(Error::DocumentNotFound(_)) => ConvexRuntimeResponseEnvelope::ok(Value::Null),
                Err(error) => ConvexRuntimeResponseEnvelope::from_core_error(error),
            },
            None => {
                let check_cancellation = cancellation.clone();
                match self
                    .service
                    .get_document_async_cancellable_with_principal(
                        self.tenant_id.clone(),
                        payload.table.clone(),
                        payload.id,
                        self.principal.clone(),
                        cancellation.cancelled(),
                        move || check_host_cancellation(&check_cancellation),
                    )
                    .await
                {
                    Ok(document) => {
                        self.record_document_read(&payload.table, &payload.id);
                        ConvexRuntimeResponseEnvelope::ok(document.into_json())
                    }
                    Err(Error::DocumentNotFound(_)) => {
                        ConvexRuntimeResponseEnvelope::ok(Value::Null)
                    }
                    Err(error) => ConvexRuntimeResponseEnvelope::from_core_error(error),
                }
            }
        };
        serde_json::to_value(response).map_err(NeovexRuntimeError::from)
    }

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
        let response = match self.mutation_execution_unit().map_or_else(
            || {
                self.service.get_document_with_principal(
                    &self.tenant_id,
                    &payload.table,
                    payload.id,
                    &self.principal,
                )
            },
            |execution_unit| {
                execution_unit
                    .get_document(&payload.table, payload.id)
                    .and_then(|document| document.ok_or(Error::DocumentNotFound(payload.id)))
            },
        ) {
            Ok(document) => {
                self.record_document_read(&payload.table, &payload.id);
                ConvexRuntimeResponseEnvelope::ok(document.into_json())
            }
            Err(Error::DocumentNotFound(_)) => ConvexRuntimeResponseEnvelope::ok(Value::Null),
            Err(error) => ConvexRuntimeResponseEnvelope::from_core_error(error),
        };
        serde_json::to_value(response).map_err(NeovexRuntimeError::from)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_db_insert_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeDbInsertPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let table = payload.table;
        let fields = payload.fields;
        let response = if let Some(execution_unit) = self.mutation_execution_unit() {
            execution_unit.insert_document(table, fields)
        } else {
            let check_cancellation = cancellation.clone();
            let cancel_wait = {
                let cancellation = cancellation.clone();
                async move {
                    cancellation.cancelled().await;
                }
            };
            self.service
                .insert_document_async_cancellable_with_principal(
                    self.tenant_id.clone(),
                    table,
                    fields,
                    self.principal.clone(),
                    cancel_wait,
                    move || check_host_cancellation(&check_cancellation),
                )
                .await
        }
        .map(|id| Value::String(id.to_string()));
        encode_runtime_core_result(response)
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
        let table = payload.table;
        let fields = payload.fields;
        let response = if let Some(execution_unit) = self.mutation_execution_unit() {
            execution_unit.insert_document(table, fields)
        } else {
            self.service.insert_document_with_principal(
                &self.tenant_id,
                table,
                fields,
                &self.principal,
            )
        }
        .map(|id| Value::String(id.to_string()));
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_db_patch_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeDbPatchPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let table = payload.table;
        let id = payload.id;
        let patch = payload.patch;
        let response = if let Some(execution_unit) = self.mutation_execution_unit() {
            execution_unit.update_document(table, id, patch)
        } else {
            let check_cancellation = cancellation.clone();
            let cancel_wait = {
                let cancellation = cancellation.clone();
                async move {
                    cancellation.cancelled().await;
                }
            };
            self.service
                .update_document_async_cancellable_with_principal(
                    self.tenant_id.clone(),
                    table,
                    id,
                    patch,
                    self.principal.clone(),
                    cancel_wait,
                    move || check_host_cancellation(&check_cancellation),
                )
                .await
        }
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
        let table = payload.table;
        let id = payload.id;
        let patch = payload.patch;
        let response = if let Some(execution_unit) = self.mutation_execution_unit() {
            execution_unit.update_document(table, id, patch)
        } else {
            self.service.update_document_with_principal(
                &self.tenant_id,
                table,
                id,
                patch,
                &self.principal,
            )
        }
        .map(|id| Value::String(id.to_string()));
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_db_delete_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeDbDeletePayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let table = payload.table;
        let id = payload.id;
        let response = if let Some(execution_unit) = self.mutation_execution_unit() {
            execution_unit.delete_document(table, id)
        } else {
            let check_cancellation = cancellation.clone();
            let cancel_wait = {
                let cancellation = cancellation.clone();
                async move {
                    cancellation.cancelled().await;
                }
            };
            self.service
                .delete_document_async_cancellable_with_principal(
                    self.tenant_id.clone(),
                    table,
                    id,
                    self.principal.clone(),
                    cancel_wait,
                    move || check_host_cancellation(&check_cancellation),
                )
                .await
        }
        .map(|_| Value::Null);
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
        let table = payload.table;
        let id = payload.id;
        let response = if let Some(execution_unit) = self.mutation_execution_unit() {
            execution_unit.delete_document(table, id)
        } else {
            self.service
                .delete_document_with_principal(&self.tenant_id, table, id, &self.principal)
        }
        .map(|_| Value::Null);
        encode_runtime_core_result(response)
    }
}
