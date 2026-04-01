use super::*;

impl ConvexRuntimeBridge {
    pub(in crate::convex) fn invoke_ctx_db_get(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_get_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_db_get_cancellable(
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

    pub(in crate::convex) fn invoke_ctx_db_insert(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_insert_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_db_insert_cancellable(
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

    pub(in crate::convex) fn invoke_ctx_db_patch(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_patch_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_db_patch_cancellable(
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

    pub(in crate::convex) fn invoke_ctx_db_delete(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_delete_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_db_delete_cancellable(
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

    pub(in crate::convex) fn invoke_ctx_query_start(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
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
            .map_err(NeovexRuntimeError::from)
    }

    pub(in crate::convex) fn invoke_ctx_query_with_index(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
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
        serde_json::to_value(response).map_err(NeovexRuntimeError::from)
    }

    pub(in crate::convex) fn invoke_ctx_query_filter(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
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
        serde_json::to_value(response).map_err(NeovexRuntimeError::from)
    }

    pub(in crate::convex) fn invoke_ctx_query_order(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
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
        serde_json::to_value(response).map_err(NeovexRuntimeError::from)
    }

    pub(in crate::convex) fn invoke_ctx_query_collect(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_collect_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_query_collect_cancellable(
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

    pub(in crate::convex) fn invoke_ctx_query_take(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_take_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_query_take_cancellable(
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
            let mut check_cancel = || check_host_cancellation(cancellation);
            execute_query_result_cancellable(
                &self.service,
                &self.tenant_id,
                ConvexExecutableQuery::Query(query),
                &mut check_cancel,
            )
            .and_then(|value| {
                self.record_limited_query_window(&tracked_query, payload.limit, &value)?;
                self.record_result_documents(&builder.table, &value);
                Ok(value)
            })
        });
        encode_runtime_core_result(response)
    }

    pub(in crate::convex) fn invoke_ctx_query_paginate(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_paginate_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_query_paginate_cancellable(
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

    pub(in crate::convex) fn invoke_ctx_query_first(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_first_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_query_first_cancellable(
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
            let mut check_cancel = || check_host_cancellation(cancellation);
            execute_query_result_cancellable(
                &self.service,
                &self.tenant_id,
                ConvexExecutableQuery::Read(ConvexReadCommand::First { query }),
                &mut check_cancel,
            )
            .and_then(|value| {
                self.record_limited_query_window(&tracked_query, 1, &value)?;
                self.record_result_documents(&builder.table, &value);
                Ok(value)
            })
        });
        encode_runtime_core_result(response)
    }

    pub(in crate::convex) fn invoke_ctx_query_unique(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_unique_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_query_unique_cancellable(
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
            let mut check_cancel = || check_host_cancellation(cancellation);
            execute_query_result_cancellable(
                &self.service,
                &self.tenant_id,
                ConvexExecutableQuery::Read(ConvexReadCommand::Unique { query }),
                &mut check_cancel,
            )
            .and_then(|value| {
                self.record_limited_query_window(&tracked_query, 2, &value)?;
                self.record_result_documents(&builder.table, &value);
                Ok(value)
            })
        });
        encode_runtime_core_result(response)
    }

    pub(in crate::convex) fn invoke_ctx_scheduler_run_after(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_scheduler_run_after_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_scheduler_run_after_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeSchedulerRunAfterPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = execute_schedule_command(
            &self.service,
            &self.registry,
            &self.tenant_id,
            ConvexScheduledCommand::RunAfter {
                delay_ms: payload.delay_ms,
                name: payload.name,
                visibility: payload.visibility,
                args: payload.args,
            },
        );
        encode_runtime_core_result(response)
    }

    pub(in crate::convex) fn invoke_ctx_scheduler_run_at(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_scheduler_run_at_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_scheduler_run_at_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeSchedulerRunAtPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = execute_schedule_command(
            &self.service,
            &self.registry,
            &self.tenant_id,
            ConvexScheduledCommand::RunAt {
                timestamp_ms: payload.timestamp_ms,
                name: payload.name,
                visibility: payload.visibility,
                args: payload.args,
            },
        );
        encode_runtime_core_result(response)
    }

    pub(in crate::convex) fn invoke_ctx_scheduler_cancel(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_scheduler_cancel_cancellable(payload, &cancellation)
    }

    pub(in crate::convex) fn invoke_ctx_scheduler_cancel_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeSchedulerCancelPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = execute_schedule_command(
            &self.service,
            &self.registry,
            &self.tenant_id,
            ConvexScheduledCommand::Cancel {
                job_id: payload.job_id,
            },
        );
        encode_runtime_core_result(response)
    }
}
