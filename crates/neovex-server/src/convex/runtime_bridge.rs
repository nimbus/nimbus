use super::dispatch::{
    check_host_cancellation, dispatch_convex_mutation_cancellable, dispatch_mutation,
    encode_runtime_core_result, ensure_runtime_host_not_cancelled,
    execute_convex_action_cancellable, execute_query_result_cancellable, execute_schedule_command,
    runtime_error_to_core,
};
use super::http_actions::prepare_http_action_response_cancellable;
use super::registry::validate_runtime_http_route;
use super::subscriptions::{
    is_scalar_filter_value, should_replace_lower_bound, should_replace_upper_bound,
};
use super::*;

impl ConvexRuntimeResponseEnvelope {
    pub(super) fn ok(value: Value) -> Self {
        Self::Ok { value }
    }

    pub(super) fn from_core_error(error: Error) -> Self {
        Self::Error {
            error: ConvexRuntimeEncodedError::from_core_error(error),
        }
    }

    pub(super) fn into_core_result(self) -> Result<Value, Error> {
        match self {
            Self::Ok { value } => Ok(value),
            Self::Error { error } => Err(error.into_core_error()),
        }
    }
}

impl ConvexRuntimeEncodedError {
    pub(super) fn from_core_error(error: Error) -> Self {
        match error {
            Error::Cancelled => Self::Cancelled,
            Error::TenantNotFound(tenant_id) => Self::TenantNotFound {
                tenant_id: tenant_id.to_string(),
            },
            Error::DocumentNotFound(document_id) => Self::DocumentNotFound {
                document_id: document_id.to_string(),
            },
            Error::ScheduledJobNotFound(job_id) => Self::ScheduledJobNotFound {
                job_id: job_id.to_string(),
            },
            Error::AlreadyExists(message) => Self::AlreadyExists { message },
            Error::InvalidInput(message) => Self::InvalidInput { message },
            Error::SchemaValidation(message) => Self::SchemaValidation { message },
            Error::SchemaNotFound(table) => Self::SchemaNotFound {
                table: table.to_string(),
            },
            Error::Storage(message) => Self::Storage { message },
            Error::Serialization(message) => Self::Serialization { message },
            Error::Internal(message) => Self::Internal { message },
        }
    }

    pub(super) fn into_core_error(self) -> Error {
        match self {
            Self::Cancelled => Error::Cancelled,
            Self::TenantNotFound { tenant_id } => TenantId::new(tenant_id)
                .map(Error::TenantNotFound)
                .unwrap_or_else(|error| Error::Internal(error.to_string())),
            Self::DocumentNotFound { document_id } => document_id
                .parse()
                .map(Error::DocumentNotFound)
                .unwrap_or_else(|error| Error::Internal(error.to_string())),
            Self::ScheduledJobNotFound { job_id } => job_id
                .parse()
                .map(Error::ScheduledJobNotFound)
                .unwrap_or_else(|error| Error::Internal(error.to_string())),
            Self::AlreadyExists { message } => Error::AlreadyExists(message),
            Self::InvalidInput { message } => Error::InvalidInput(message),
            Self::SchemaValidation { message } => Error::SchemaValidation(message),
            Self::SchemaNotFound { table } => TableName::new(table)
                .map(Error::SchemaNotFound)
                .unwrap_or_else(|error| Error::Internal(error.to_string())),
            Self::Storage { message } => Error::Storage(message),
            Self::Serialization { message } => Error::Serialization(message),
            Self::Internal { message } => Error::Internal(message),
        }
    }
}

impl ConvexRuntimeBridge {
    pub(super) fn new(
        service: Arc<neovex_engine::Service>,
        registry: Arc<ConvexRegistry>,
        tenant_id: TenantId,
    ) -> Self {
        static NEXT_RUNTIME_SESSION_ID: AtomicU64 = AtomicU64::new(1);
        let max_nested_runtime_invocations = registry
            .runtime_policy()
            .limits()
            .max_nested_runtime_invocations;
        Self {
            service,
            registry,
            tenant_id,
            session_id: format!(
                "convex-runtime-session-{}",
                NEXT_RUNTIME_SESSION_ID.fetch_add(1, Ordering::Relaxed)
            ),
            max_nested_runtime_invocations,
            remaining_nested_runtime_invocations: Arc::new(AtomicUsize::new(
                max_nested_runtime_invocations,
            )),
            query_builders: Arc::new(Mutex::new(ConvexRuntimeQueryBuilders::default())),
            read_set: Arc::new(Mutex::new(ConvexRuntimeReadSet::default())),
        }
    }

    pub(super) fn snapshot_read_set(&self) -> ConvexRuntimeReadSet {
        self.read_set
            .lock()
            .expect("convex runtime read set lock should not be poisoned")
            .clone()
    }

    pub(super) fn validate_session(
        &self,
        session_id: Option<&str>,
    ) -> std::result::Result<(), NeovexRuntimeError> {
        if let Some(session_id) = session_id
            && session_id.is_empty()
        {
            return Err(NeovexRuntimeError::Contract(format!(
                "runtime session token must not be empty for tenant {}",
                self.tenant_id
            )));
        }
        Ok(())
    }

    pub(super) fn new_builder_id(&self) -> String {
        let mut builders = self
            .query_builders
            .lock()
            .expect("convex runtime query builder lock should not be poisoned");
        builders.next_builder_id += 1;
        format!("{}-builder-{}", self.session_id, builders.next_builder_id)
    }

    pub(super) fn consume_nested_runtime_invocation_budget(&self) -> Result<(), Error> {
        let mut remaining = self
            .remaining_nested_runtime_invocations
            .load(Ordering::SeqCst);
        loop {
            if remaining == 0 {
                return Err(runtime_error_to_core(
                    NeovexRuntimeError::NestedInvocationLimitExceeded(
                        self.max_nested_runtime_invocations,
                    ),
                ));
            }
            match self.remaining_nested_runtime_invocations.compare_exchange(
                remaining,
                remaining - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return Ok(()),
                Err(next_remaining) => remaining = next_remaining,
            }
        }
    }

    pub(super) fn insert_builder(&self, builder_id: String, state: ConvexRuntimeQueryBuilderState) {
        self.query_builders
            .lock()
            .expect("convex runtime query builder lock should not be poisoned")
            .builders
            .insert(builder_id, state);
    }

    pub(super) fn with_builder_mut<R>(
        &self,
        builder_id: &str,
        update: impl FnOnce(&mut ConvexRuntimeQueryBuilderState) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let mut builders = self
            .query_builders
            .lock()
            .expect("convex runtime query builder lock should not be poisoned");
        let state = builders.builders.get_mut(builder_id).ok_or_else(|| {
            Error::InvalidInput(format!(
                "convex runtime query builder not found: {builder_id}"
            ))
        })?;
        update(state)
    }

    pub(super) fn take_builder(
        &self,
        builder_id: &str,
    ) -> Result<ConvexRuntimeQueryBuilderState, Error> {
        self.query_builders
            .lock()
            .expect("convex runtime query builder lock should not be poisoned")
            .builders
            .remove(builder_id)
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "convex runtime query builder not found: {builder_id}"
                ))
            })
    }

    pub(super) fn record_table_read(&self, table: &TableName) {
        self.read_set
            .lock()
            .expect("convex runtime read set lock should not be poisoned")
            .record_table(table);
    }

    pub(super) fn record_document_read(&self, table: &TableName, document_id: &DocumentId) {
        self.read_set
            .lock()
            .expect("convex runtime read set lock should not be poisoned")
            .record_document(table, document_id);
    }

    pub(super) fn record_result_documents(&self, table: &TableName, value: &Value) {
        match value {
            Value::Array(items) => {
                for item in items {
                    self.record_result_documents(table, item);
                }
            }
            Value::Object(map) => {
                if let Some(document_id) = map
                    .get("_id")
                    .and_then(Value::as_str)
                    .and_then(|value| value.parse::<DocumentId>().ok())
                {
                    self.record_document_read(table, &document_id);
                }

                if let Some(data) = map.get("data") {
                    self.record_result_documents(table, data);
                }
            }
            _ => {}
        }
    }

    pub(super) fn record_query_result_value(&self, query: &ConvexExecutableQuery, value: &Value) {
        match query {
            ConvexExecutableQuery::Query(query) => {
                self.record_result_documents(&query.table, value)
            }
            ConvexExecutableQuery::Read(ConvexReadCommand::Get { table, .. }) => {
                self.record_result_documents(table, value);
            }
            ConvexExecutableQuery::Read(ConvexReadCommand::First { query })
            | ConvexExecutableQuery::Read(ConvexReadCommand::Unique { query }) => {
                self.record_result_documents(&query.table, value);
            }
        }
    }

    pub(super) fn record_paginated_window_read(
        &self,
        query: &Query,
        page_size: usize,
        after: Option<&Cursor>,
        page: &neovex_core::Page,
    ) {
        self.read_set
            .lock()
            .expect("convex runtime read set lock should not be poisoned")
            .record_paginated_window(query, page_size, after, page);
    }

    pub(super) fn record_limited_query_window(
        &self,
        query: &Query,
        limit: usize,
        value: &Value,
    ) -> Result<(), Error> {
        if query.order.is_none() {
            return Ok(());
        }

        let data = match value {
            Value::Array(items) => items.clone(),
            Value::Null => Vec::new(),
            other => vec![other.clone()],
        };
        let page = neovex_core::Page {
            data,
            has_more: false,
            next_cursor: None,
        };
        self.record_paginated_window_read(query, limit, None, &page);
        Ok(())
    }

    pub(super) fn record_index_read(&self, read: ConvexRuntimeIndexRangeRead) {
        self.read_set
            .lock()
            .expect("convex runtime read set lock should not be poisoned")
            .record_index_range(read);
    }

    pub(super) fn record_predicate_read(&self, table: &TableName, filters: &[Filter]) {
        self.read_set
            .lock()
            .expect("convex runtime read set lock should not be poisoned")
            .record_predicate(table, filters);
    }

    pub(super) fn lookup_index_primary_field(
        &self,
        table: &TableName,
        index_name: &str,
    ) -> Result<Option<String>, Error> {
        let schema = self.service.get_table_schema(&self.tenant_id, table)?;
        Ok(schema
            .indexes
            .iter()
            .find(|index| index.name == index_name)
            .map(|index| index.field.clone()))
    }

    pub(super) fn record_query_read(&self, query: &Query) {
        if !query.filters.is_empty() {
            self.record_predicate_read(&query.table, &query.filters);
        }
        if let Some(index_read) = self.derive_index_read(query, None) {
            self.record_index_read(index_read);
        } else if query.filters.is_empty() {
            self.record_table_read(&query.table);
        }
    }

    pub(super) fn record_executable_query_read(&self, query: &ConvexExecutableQuery) {
        match query {
            ConvexExecutableQuery::Query(query) => self.record_query_read(query),
            ConvexExecutableQuery::Read(ConvexReadCommand::Get { table, id }) => {
                self.record_document_read(table, id);
            }
            ConvexExecutableQuery::Read(ConvexReadCommand::First { query })
            | ConvexExecutableQuery::Read(ConvexReadCommand::Unique { query }) => {
                self.record_query_read(query);
            }
        }
    }

    pub(super) fn record_builder_read(
        &self,
        state: &ConvexRuntimeQueryBuilderState,
        query: &Query,
    ) {
        if !query.filters.is_empty() {
            self.record_predicate_read(&query.table, &query.filters);
        }
        if let Some(index_read) = self.derive_index_read(query, state.index_name.as_deref()) {
            self.record_index_read(index_read);
        } else if query.filters.is_empty() {
            self.record_table_read(&query.table);
        }
    }

    pub(super) fn derive_index_read(
        &self,
        query: &Query,
        preferred_index_name: Option<&str>,
    ) -> Option<ConvexRuntimeIndexRangeRead> {
        let table_schema = self
            .service
            .get_table_schema(&self.tenant_id, &query.table)
            .ok()?;
        let index = if let Some(index_name) = preferred_index_name {
            table_schema
                .indexes
                .iter()
                .find(|index| index.name == index_name)
        } else {
            table_schema.indexes.iter().find(|index| {
                query.filters.iter().any(|filter| {
                    filter.field == index.field && is_scalar_filter_value(&filter.value)
                })
            })
        }?;
        let field = index.field.clone();
        let mut start: Option<Value> = None;
        let mut end: Option<Value> = None;
        let mut start_inclusive = false;
        let mut end_inclusive = false;
        let mut has_bound = false;

        for filter in query.filters.iter().filter(|filter| filter.field == field) {
            match filter.op {
                FilterOp::Eq if is_scalar_filter_value(&filter.value) => {
                    start = Some(filter.value.clone());
                    end = Some(filter.value.clone());
                    start_inclusive = true;
                    end_inclusive = true;
                    has_bound = true;
                }
                FilterOp::Gt if is_scalar_filter_value(&filter.value) => {
                    if should_replace_lower_bound(start.as_ref(), Some(&filter.value), false) {
                        start = Some(filter.value.clone());
                        start_inclusive = false;
                        has_bound = true;
                    }
                }
                FilterOp::Gte if is_scalar_filter_value(&filter.value) => {
                    if should_replace_lower_bound(start.as_ref(), Some(&filter.value), true) {
                        start = Some(filter.value.clone());
                        start_inclusive = true;
                        has_bound = true;
                    }
                }
                FilterOp::Lt if is_scalar_filter_value(&filter.value) => {
                    if should_replace_upper_bound(end.as_ref(), Some(&filter.value), false) {
                        end = Some(filter.value.clone());
                        end_inclusive = false;
                        has_bound = true;
                    }
                }
                FilterOp::Lte if is_scalar_filter_value(&filter.value) => {
                    if should_replace_upper_bound(end.as_ref(), Some(&filter.value), true) {
                        end = Some(filter.value.clone());
                        end_inclusive = true;
                        has_bound = true;
                    }
                }
                _ => {}
            }
        }

        if !has_bound {
            return None;
        }

        Some(ConvexRuntimeIndexRangeRead {
            table: query.table.clone(),
            index_name: index.name.clone(),
            field,
            start,
            end,
            start_inclusive,
            end_inclusive,
        })
    }

    pub(super) fn invoke_ctx_db_get(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_get_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_db_get_cancellable(
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

    pub(super) fn invoke_ctx_db_insert(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_insert_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_db_insert_cancellable(
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

    pub(super) fn invoke_ctx_db_patch(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_patch_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_db_patch_cancellable(
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

    pub(super) fn invoke_ctx_db_delete(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_db_delete_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_db_delete_cancellable(
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

    pub(super) fn invoke_ctx_query_start(
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

    pub(super) fn invoke_ctx_query_with_index(
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

    pub(super) fn invoke_ctx_query_filter(
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

    pub(super) fn invoke_ctx_query_order(
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

    pub(super) fn invoke_ctx_query_collect(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_collect_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_query_collect_cancellable(
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

    pub(super) fn invoke_ctx_query_take(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_take_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_query_take_cancellable(
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

    pub(super) fn invoke_ctx_query_paginate(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_paginate_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_query_paginate_cancellable(
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

    pub(super) fn invoke_ctx_query_first(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_first_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_query_first_cancellable(
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

    pub(super) fn invoke_ctx_query_unique(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_unique_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_query_unique_cancellable(
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

    pub(super) fn invoke_ctx_scheduler_run_after(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_scheduler_run_after_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_scheduler_run_after_cancellable(
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

    pub(super) fn invoke_ctx_scheduler_run_at(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_scheduler_run_at_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_scheduler_run_at_cancellable(
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

    pub(super) fn invoke_ctx_scheduler_cancel(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_scheduler_cancel_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_scheduler_cancel_cancellable(
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

impl ConvexRuntimeQueryBuilderState {
    pub(super) fn into_query(self, limit: Option<usize>) -> Query {
        Query {
            table: self.table,
            filters: self.filters,
            order: self.order,
            limit,
        }
    }
}

fn record_host_operation_result(
    metrics: &neovex_runtime::RuntimeMetrics,
    operation: &str,
    result: &std::result::Result<Value, NeovexRuntimeError>,
) {
    match result {
        Ok(_) => metrics.record_host_operation_succeeded(operation),
        Err(NeovexRuntimeError::Cancelled) => {
            metrics.record_host_operation_canceled_in_flight(operation);
        }
        Err(_) => metrics.record_host_operation_failed(operation),
    }
}

#[derive(Clone)]
struct AsyncHostCallTrace {
    span: tracing::Span,
    enqueued_at: std::time::Instant,
}

impl AsyncHostCallTrace {
    fn new(bridge: &ConvexRuntimeBridge, operation: &str) -> Self {
        static NEXT_ASYNC_HOST_CALL_ID: AtomicU64 = AtomicU64::new(1);

        let span = tracing::debug_span!(
            "convex_runtime_async_host_call",
            tenant = %bridge.tenant_id,
            session_id = %bridge.session_id,
            operation,
            host_call_id = NEXT_ASYNC_HOST_CALL_ID.fetch_add(1, Ordering::Relaxed),
        );
        let trace = Self {
            span,
            enqueued_at: std::time::Instant::now(),
        };
        tracing::debug!(
            parent: &trace.span,
            "convex runtime async host call enqueued"
        );
        trace
    }

    fn record_canceled_before_start(&self) {
        tracing::debug!(
            parent: &self.span,
            queue_wait_ms = self.enqueued_at.elapsed().as_secs_f64() * 1000.0,
            "convex runtime async host call canceled before start"
        );
    }

    fn record_started(&self) -> std::time::Instant {
        let started_at = std::time::Instant::now();
        tracing::debug!(
            parent: &self.span,
            queue_wait_ms = started_at.duration_since(self.enqueued_at).as_secs_f64() * 1000.0,
            "convex runtime async host call started"
        );
        started_at
    }

    fn record_finished(
        &self,
        started_at: std::time::Instant,
        result: &std::result::Result<Value, NeovexRuntimeError>,
    ) {
        let execution_ms = started_at.elapsed().as_secs_f64() * 1000.0;
        match result {
            Ok(_) => tracing::debug!(
                parent: &self.span,
                execution_ms,
                "convex runtime async host call finished"
            ),
            Err(NeovexRuntimeError::Cancelled) => tracing::debug!(
                parent: &self.span,
                execution_ms,
                "convex runtime async host call canceled in flight"
            ),
            Err(error) => tracing::debug!(
                parent: &self.span,
                execution_ms,
                error = %error,
                "convex runtime async host call failed"
            ),
        }
    }

    fn record_join_failure(&self, error: &tokio::task::JoinError) {
        tracing::debug!(
            parent: &self.span,
            error = %error,
            "convex runtime async host call failed before completion"
        );
    }
}

async fn execute_async_blocking_host_call<F>(
    trace: AsyncHostCallTrace,
    metrics: Arc<neovex_runtime::RuntimeMetrics>,
    operation: String,
    cancellation: HostCallCancellation,
    task: F,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    F: FnOnce(HostCallCancellation) -> std::result::Result<Value, NeovexRuntimeError>
        + Send
        + 'static,
{
    if cancellation.is_cancelled() {
        metrics.record_host_operation_canceled_before_start(&operation);
        trace.record_canceled_before_start();
        return Err(NeovexRuntimeError::Cancelled);
    }

    let metrics_for_task = metrics.clone();
    let operation_for_task = operation.clone();
    let trace_for_task = trace.clone();
    let handle = tokio::task::spawn_blocking(move || {
        let started_at = trace_for_task.record_started();
        metrics_for_task.record_host_operation_started(&operation_for_task);
        let result = task(cancellation);
        (started_at, result)
    });
    let (started_at, result) = match handle.await {
        Ok(output) => output,
        Err(error) => {
            trace.record_join_failure(&error);
            metrics.record_host_operation_failed(&operation);
            return Err(NeovexRuntimeError::Contract(format!(
                "runtime host bridge task failed: {error}"
            )));
        }
    };
    trace.record_finished(started_at, &result);
    record_host_operation_result(&metrics, &operation, &result);
    result
}

impl HostBridge for ConvexRuntimeBridge {
    fn call(&self, request: HostCallRequest) -> std::result::Result<Value, NeovexRuntimeError> {
        let metrics = self.registry.runtime_policy().metrics();
        let operation = request.operation.clone();
        metrics.record_host_operation_started(&operation);
        let result = self.dispatch_host_call(request);
        record_host_operation_result(&metrics, &operation, &result);
        result
    }

    fn call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let metrics = self.registry.runtime_policy().metrics();
        let operation = request.operation.clone();
        if cancellation.is_cancelled() {
            metrics.record_host_operation_canceled_before_start(&operation);
            return Err(NeovexRuntimeError::Cancelled);
        }
        metrics.record_host_operation_started(&operation);
        let result = self.dispatch_host_call_cancellable(request, cancellation);
        record_host_operation_result(&metrics, &operation, &result);
        result
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        let bridge = self.clone();
        let trace = AsyncHostCallTrace::new(&bridge, &request.operation);
        let metrics = bridge.registry.runtime_policy().metrics();
        let operation = request.operation.clone();
        Box::pin(execute_async_blocking_host_call(
            trace,
            metrics,
            operation,
            cancellation,
            move |cancellation| bridge.dispatch_host_call_cancellable(request, &cancellation),
        ))
    }
}

impl ConvexRuntimeBridge {
    pub(super) fn dispatch_host_call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match request.operation.as_str() {
            "convex.ctx.db.query.start" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_start(request.payload)
            }
            "convex.ctx.db.query.with_index" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_with_index(request.payload)
            }
            "convex.ctx.db.query.filter" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_filter(request.payload)
            }
            "convex.ctx.db.query.order" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_order(request.payload)
            }
            "convex.http_route" => {
                self.invoke_http_route_cancellable(request.payload, cancellation)
            }
            "convex.ctx.query" => self.invoke_ctx_query_cancellable(request.payload, cancellation),
            "convex.ctx.paginated_query" => {
                self.invoke_ctx_paginated_query_cancellable(request.payload, cancellation)
            }
            "convex.ctx.mutation" => {
                self.invoke_ctx_mutation_cancellable(request.payload, cancellation)
            }
            "convex.ctx.action" => {
                self.invoke_ctx_action_cancellable(request.payload, cancellation)
            }
            "convex.ctx.run_query" => {
                self.invoke_ctx_run_query_cancellable(request.payload, cancellation)
            }
            "convex.ctx.run_mutation" => {
                self.invoke_ctx_run_mutation_cancellable(request.payload, cancellation)
            }
            "convex.ctx.run_action" => {
                self.invoke_ctx_run_action_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.get" => {
                self.invoke_ctx_db_get_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.insert" => {
                self.invoke_ctx_db_insert_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.patch" => {
                self.invoke_ctx_db_patch_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.delete" => {
                self.invoke_ctx_db_delete_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.collect" => {
                self.invoke_ctx_query_collect_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.take" => {
                self.invoke_ctx_query_take_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.paginate" => {
                self.invoke_ctx_query_paginate_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.first" => {
                self.invoke_ctx_query_first_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.unique" => {
                self.invoke_ctx_query_unique_cancellable(request.payload, cancellation)
            }
            "convex.ctx.scheduler.run_after" => {
                self.invoke_ctx_scheduler_run_after_cancellable(request.payload, cancellation)
            }
            "convex.ctx.scheduler.run_at" => {
                self.invoke_ctx_scheduler_run_at_cancellable(request.payload, cancellation)
            }
            "convex.ctx.scheduler.cancel" => {
                self.invoke_ctx_scheduler_cancel_cancellable(request.payload, cancellation)
            }
            "convex.ctx.runtime.enter_nested_call" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_runtime_enter_nested_call(request.payload)
            }
            other => Err(NeovexRuntimeError::Contract(format!(
                "unsupported convex runtime operation: {other}"
            ))),
        }
    }

    pub(super) fn dispatch_host_call(
        &self,
        request: HostCallRequest,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match request.operation.as_str() {
            "convex.http_route" => self.invoke_http_route(request.payload),
            "convex.ctx.query" => self.invoke_ctx_query(request.payload),
            "convex.ctx.paginated_query" => self.invoke_ctx_paginated_query(request.payload),
            "convex.ctx.mutation" => self.invoke_ctx_mutation(request.payload),
            "convex.ctx.action" => self.invoke_ctx_action(request.payload),
            "convex.ctx.runtime.enter_nested_call" => {
                self.invoke_ctx_runtime_enter_nested_call(request.payload)
            }
            "convex.ctx.run_query" => self.invoke_ctx_run_query(request.payload),
            "convex.ctx.run_mutation" => self.invoke_ctx_run_mutation(request.payload),
            "convex.ctx.run_action" => self.invoke_ctx_run_action(request.payload),
            "convex.ctx.db.get" => self.invoke_ctx_db_get(request.payload),
            "convex.ctx.db.query.start" => self.invoke_ctx_query_start(request.payload),
            "convex.ctx.db.query.with_index" => self.invoke_ctx_query_with_index(request.payload),
            "convex.ctx.db.query.filter" => self.invoke_ctx_query_filter(request.payload),
            "convex.ctx.db.query.order" => self.invoke_ctx_query_order(request.payload),
            "convex.ctx.db.query.collect" => self.invoke_ctx_query_collect(request.payload),
            "convex.ctx.db.query.take" => self.invoke_ctx_query_take(request.payload),
            "convex.ctx.db.query.paginate" => self.invoke_ctx_query_paginate(request.payload),
            "convex.ctx.db.query.first" => self.invoke_ctx_query_first(request.payload),
            "convex.ctx.db.query.unique" => self.invoke_ctx_query_unique(request.payload),
            "convex.ctx.db.insert" => self.invoke_ctx_db_insert(request.payload),
            "convex.ctx.db.patch" => self.invoke_ctx_db_patch(request.payload),
            "convex.ctx.db.delete" => self.invoke_ctx_db_delete(request.payload),
            "convex.ctx.scheduler.run_after" => {
                self.invoke_ctx_scheduler_run_after(request.payload)
            }
            "convex.ctx.scheduler.run_at" => self.invoke_ctx_scheduler_run_at(request.payload),
            "convex.ctx.scheduler.cancel" => self.invoke_ctx_scheduler_cancel(request.payload),
            other => Err(NeovexRuntimeError::Contract(format!(
                "unsupported convex runtime operation: {other}"
            ))),
        }
    }

    pub(super) fn invoke_http_route(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_http_route_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_http_route_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeHttpRouteInvokePayload = serde_json::from_value(payload)?;
        validate_runtime_http_route(&payload.request, &payload.route)?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let request_context: ConvexHttpRequestContext =
            serde_json::from_value(payload.request.args.clone())?;
        let response = prepare_http_action_response_cancellable(
            &self.service,
            &self.registry,
            &self.tenant_id,
            &payload.route.plan,
            &request_context,
            cancellation,
        )
        .and_then(|parts| {
            serde_json::to_value(parts).map_err(|error| Error::Serialization(error.to_string()))
        })
        .map(ConvexRuntimeResponseEnvelope::ok)
        .unwrap_or_else(ConvexRuntimeResponseEnvelope::from_core_error);

        serde_json::to_value(response).map_err(NeovexRuntimeError::from)
    }

    pub(super) fn invoke_ctx_query(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_query_cancellable(
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

    pub(super) fn invoke_ctx_paginated_query(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_paginated_query_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_paginated_query_cancellable(
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

    pub(super) fn invoke_ctx_mutation(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_mutation_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_mutation_cancellable(
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

    pub(super) fn invoke_ctx_action(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_action_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_action_cancellable(
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

    pub(super) fn invoke_ctx_run_query(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_run_query_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_run_query_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self.execute_runtime_function_call_cancellable(
            InvocationKind::Query,
            &payload.name,
            &payload.args,
            payload
                .visibility
                .unwrap_or(ConvexFunctionVisibility::Public),
            payload.auth,
            cancellation,
        );
        encode_runtime_core_result(response)
    }

    pub(super) fn invoke_ctx_runtime_enter_nested_call(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        self.registry
            .runtime_policy()
            .metrics()
            .record_nested_local_dispatch();
        tracing::debug!(
            tenant = %self.tenant_id,
            function = %payload.name,
            visibility = %payload.visibility.unwrap_or(ConvexFunctionVisibility::Public).as_str(),
            "convex runtime entered same-isolate nested local dispatch"
        );
        let response = self
            .consume_nested_runtime_invocation_budget()
            .map(|_| Value::Null)
            .map(ConvexRuntimeResponseEnvelope::ok)
            .unwrap_or_else(ConvexRuntimeResponseEnvelope::from_core_error);
        serde_json::to_value(response).map_err(NeovexRuntimeError::from)
    }

    pub(super) fn invoke_ctx_run_mutation(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_run_mutation_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_run_mutation_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self.execute_runtime_function_call_cancellable(
            InvocationKind::Mutation,
            &payload.name,
            &payload.args,
            payload
                .visibility
                .unwrap_or(ConvexFunctionVisibility::Public),
            payload.auth,
            cancellation,
        );
        encode_runtime_core_result(response)
    }

    pub(super) fn invoke_ctx_run_action(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_run_action_cancellable(payload, &cancellation)
    }

    pub(super) fn invoke_ctx_run_action_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeFunctionCallPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let response = self.execute_runtime_function_call_cancellable(
            InvocationKind::Action,
            &payload.name,
            &payload.args,
            payload
                .visibility
                .unwrap_or(ConvexFunctionVisibility::Public),
            payload.auth,
            cancellation,
        );
        encode_runtime_core_result(response)
    }

    pub(super) fn execute_runtime_function_call_cancellable(
        &self,
        kind: InvocationKind,
        name: &str,
        args: &Value,
        visibility: ConvexFunctionVisibility,
        auth: Option<InvocationAuth>,
        cancellation: &HostCallCancellation,
    ) -> Result<Value, Error> {
        if self.should_use_nested_runtime(kind.clone(), name, visibility)? {
            return self.invoke_nested_runtime_function_cancellable(
                kind,
                name,
                args,
                visibility,
                auth,
                cancellation,
            );
        }

        ensure_runtime_host_not_cancelled(cancellation).map_err(runtime_error_to_core)?;

        match kind {
            InvocationKind::Query => {
                let query = self
                    .registry
                    .resolve_query_for_visibility(name, args, visibility)?;
                let mut check_cancel = || check_host_cancellation(cancellation);
                execute_query_result_cancellable(
                    &self.service,
                    &self.tenant_id,
                    query,
                    &mut check_cancel,
                )
            }
            InvocationKind::PaginatedQuery => Err(Error::InvalidInput(
                "ctx.runQuery does not support paginated queries".to_string(),
            )),
            InvocationKind::Mutation => {
                let mutation = self
                    .registry
                    .resolve_mutation_for_visibility(name, args, visibility)?;
                dispatch_convex_mutation_cancellable(
                    &self.service,
                    &self.registry,
                    &self.tenant_id,
                    mutation,
                    cancellation,
                )
            }
            InvocationKind::Action => {
                let action = self
                    .registry
                    .resolve_action_for_visibility(name, args, visibility)?;
                execute_convex_action_cancellable(
                    &self.service,
                    &self.registry,
                    &self.tenant_id,
                    action,
                    cancellation,
                )
            }
        }
    }

    pub(super) fn should_use_nested_runtime(
        &self,
        kind: InvocationKind,
        name: &str,
        visibility: ConvexFunctionVisibility,
    ) -> Result<bool, Error> {
        let Some(bundle) = self.registry.runtime_bundle() else {
            return Ok(false);
        };
        let _ = bundle;
        let definition = self
            .registry
            .functions
            .get(name)
            .ok_or_else(|| Error::InvalidInput(format!("convex function not found: {name}")))?;
        if definition.visibility != visibility {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is {}, not {}",
                definition.visibility.as_str(),
                visibility.as_str()
            )));
        }
        let expected_kind = match kind {
            InvocationKind::Query => ConvexFunctionKind::Query,
            InvocationKind::PaginatedQuery => ConvexFunctionKind::PaginatedQuery,
            InvocationKind::Mutation => ConvexFunctionKind::Mutation,
            InvocationKind::Action => ConvexFunctionKind::Action,
        };
        if definition.kind != expected_kind {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is a {}, not a {}",
                definition.kind.as_str(),
                expected_kind.as_str()
            )));
        }
        Ok(definition.runtime_handler.is_some() && definition.plan.is_null())
    }

    pub(super) fn invoke_nested_runtime_function_cancellable(
        &self,
        kind: InvocationKind,
        name: &str,
        args: &Value,
        visibility: ConvexFunctionVisibility,
        auth: Option<InvocationAuth>,
        cancellation: &HostCallCancellation,
    ) -> Result<Value, Error> {
        self.consume_nested_runtime_invocation_budget()?;
        self.registry
            .runtime_policy()
            .metrics()
            .record_fallback_cross_isolate_dispatch();
        tracing::debug!(
            tenant = %self.tenant_id,
            function = %name,
            kind = kind.as_str(),
            visibility = %visibility.as_str(),
            "convex runtime using cross-isolate fallback dispatch"
        );
        let definition = self
            .registry
            .functions
            .get(name)
            .ok_or_else(|| Error::InvalidInput(format!("convex function not found: {name}")))?;
        if definition.visibility != visibility {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is {}, not {}",
                definition.visibility.as_str(),
                visibility.as_str()
            )));
        }
        let bundle = self
            .registry
            .runtime_bundle()
            .cloned()
            .ok_or_else(|| Error::Internal("convex runtime bundle not loaded".to_string()))?;
        let runtime = NeovexRuntime::with_policy_bypassing_limit(
            Arc::new(self.clone()),
            self.registry.runtime_policy(),
        );
        let response = runtime
            .invoke_bundle_blocking_with_cancellation(
                &bundle,
                &InvocationRequest {
                    kind,
                    function_name: name.to_string(),
                    args: args.clone(),
                    page_size: None,
                    cursor: None,
                    auth,
                },
                Some(cancellation.clone()),
            )
            .map_err(runtime_error_to_core)?;
        let envelope: ConvexRuntimeResponseEnvelope = serde_json::from_value(response)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        envelope.into_core_result()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Condvar, Mutex};

    use neovex_runtime::{RuntimeHostOperationMetricsSnapshot, RuntimeLimits, RuntimePolicy};
    use serde_json::json;
    use tokio::sync::Notify;
    use tokio::time::{Duration, timeout};

    use super::execute_async_blocking_host_call;
    use super::*;

    fn host_operation_metrics(
        policy: &RuntimePolicy,
        operation: &str,
    ) -> RuntimeHostOperationMetricsSnapshot {
        policy
            .metrics_snapshot()
            .host_operations
            .get(operation)
            .copied()
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn async_blocking_host_call_records_precancel_metric() {
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits::default()));
        let cancellation = HostCallCancellation::default();
        cancellation.cancel();

        let result = execute_async_blocking_host_call(
            AsyncHostCallTrace {
                span: tracing::Span::none(),
                enqueued_at: std::time::Instant::now(),
            },
            policy.metrics(),
            "convex.ctx.db.get".to_string(),
            cancellation,
            |_cancellation| Ok(json!("unexpected")),
        )
        .await;

        assert!(matches!(result, Err(NeovexRuntimeError::Cancelled)));
        let snapshot = policy.metrics_snapshot();
        assert_eq!(snapshot.canceled_host_ops, 1);
        assert_eq!(snapshot.precanceled_host_ops, 1);
        assert_eq!(snapshot.in_flight_canceled_host_ops, 0);
        assert_eq!(
            host_operation_metrics(&policy, "convex.ctx.db.get"),
            RuntimeHostOperationMetricsSnapshot {
                started: 0,
                succeeded: 0,
                failed: 0,
                canceled_before_start: 1,
                canceled_in_flight: 0,
            }
        );
    }

    #[tokio::test]
    async fn async_blocking_host_call_records_cooperative_read_cancellation() {
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits::default()));
        let cancellation = HostCallCancellation::default();
        let started = Arc::new(Notify::new());

        let task = tokio::spawn({
            let started = started.clone();
            let metrics = policy.metrics();
            let cancellation = cancellation.clone();
            async move {
                execute_async_blocking_host_call(
                    AsyncHostCallTrace {
                        span: tracing::Span::none(),
                        enqueued_at: std::time::Instant::now(),
                    },
                    metrics,
                    "convex.ctx.db.get".to_string(),
                    cancellation,
                    move |cancellation| {
                        started.notify_one();
                        while !cancellation.is_cancelled() {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(NeovexRuntimeError::Cancelled)
                    },
                )
                .await
            }
        });

        timeout(Duration::from_secs(1), started.notified())
            .await
            .expect("blocking host call should start");
        cancellation.cancel();

        let result = timeout(Duration::from_secs(1), task)
            .await
            .expect("canceled host call should resolve promptly")
            .expect("blocking host call task should join");
        assert!(matches!(result, Err(NeovexRuntimeError::Cancelled)));
        let snapshot = policy.metrics_snapshot();
        assert_eq!(snapshot.canceled_host_ops, 1);
        assert_eq!(snapshot.precanceled_host_ops, 0);
        assert_eq!(snapshot.in_flight_canceled_host_ops, 1);
        assert_eq!(
            host_operation_metrics(&policy, "convex.ctx.db.get"),
            RuntimeHostOperationMetricsSnapshot {
                started: 1,
                succeeded: 0,
                failed: 0,
                canceled_before_start: 0,
                canceled_in_flight: 1,
            }
        );
    }

    #[tokio::test]
    async fn async_blocking_host_call_waits_for_write_completion_after_cancellation() {
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits::default()));
        let cancellation = HostCallCancellation::default();
        let started = Arc::new(Notify::new());
        let release = Arc::new((Mutex::new(false), Condvar::new()));

        let task = tokio::spawn({
            let started = started.clone();
            let release = release.clone();
            let metrics = policy.metrics();
            let cancellation = cancellation.clone();
            async move {
                execute_async_blocking_host_call(
                    AsyncHostCallTrace {
                        span: tracing::Span::none(),
                        enqueued_at: std::time::Instant::now(),
                    },
                    metrics,
                    "convex.ctx.db.insert".to_string(),
                    cancellation,
                    move |_cancellation| {
                        started.notify_one();
                        let (lock, cvar) = &*release;
                        let mut released = lock
                            .lock()
                            .expect("write completion lock should not be poisoned");
                        while !*released {
                            released = cvar
                                .wait(released)
                                .expect("write completion wait should not be poisoned");
                        }
                        Ok(json!("committed"))
                    },
                )
                .await
            }
        });

        timeout(Duration::from_secs(1), started.notified())
            .await
            .expect("blocking write should start");
        cancellation.cancel();
        tokio::time::sleep(Duration::from_millis(25)).await;

        {
            let (lock, cvar) = &*release;
            let mut released = lock
                .lock()
                .expect("write completion lock should not be poisoned");
            *released = true;
            cvar.notify_all();
        }

        let result = timeout(Duration::from_secs(1), task)
            .await
            .expect("write host call should finish after release")
            .expect("write host call task should join")
            .expect("write host call should complete successfully");
        assert_eq!(result, json!("committed"));
        let snapshot = policy.metrics_snapshot();
        assert_eq!(snapshot.canceled_host_ops, 0);
        assert_eq!(
            host_operation_metrics(&policy, "convex.ctx.db.insert"),
            RuntimeHostOperationMetricsSnapshot {
                started: 1,
                succeeded: 1,
                failed: 0,
                canceled_before_start: 0,
                canceled_in_flight: 0,
            }
        );
    }
}
