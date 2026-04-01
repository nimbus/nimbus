use super::*;

impl ConvexRuntimeBridge {
    pub(in crate::adapters::convex) fn new_builder_id(&self) -> String {
        let mut builders = self
            .query_builders
            .lock()
            .expect("convex runtime query builder lock should not be poisoned");
        builders.next_builder_id += 1;
        format!("{}-builder-{}", self.session_id, builders.next_builder_id)
    }

    pub(in crate::adapters::convex) fn insert_builder(
        &self,
        builder_id: String,
        state: ConvexRuntimeQueryBuilderState,
    ) {
        self.query_builders
            .lock()
            .expect("convex runtime query builder lock should not be poisoned")
            .builders
            .insert(builder_id, state);
    }

    pub(in crate::adapters::convex) fn with_builder_mut<R>(
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

    pub(in crate::adapters::convex) fn take_builder(
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

    pub(in crate::adapters::convex) fn record_table_read(&self, table: &TableName) {
        self.read_set
            .lock()
            .expect("convex runtime read set lock should not be poisoned")
            .record_table(table);
    }

    pub(in crate::adapters::convex) fn record_document_read(
        &self,
        table: &TableName,
        document_id: &DocumentId,
    ) {
        self.read_set
            .lock()
            .expect("convex runtime read set lock should not be poisoned")
            .record_document(table, document_id);
    }

    pub(in crate::adapters::convex) fn record_result_documents(
        &self,
        table: &TableName,
        value: &Value,
    ) {
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

    pub(in crate::adapters::convex) fn record_query_result_value(
        &self,
        query: &ConvexExecutableQuery,
        value: &Value,
    ) {
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

    pub(in crate::adapters::convex) fn record_paginated_window_read(
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

    pub(in crate::adapters::convex) fn record_limited_query_window(
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

    pub(in crate::adapters::convex) fn record_index_read(&self, read: RuntimeIndexRangeRead) {
        self.read_set
            .lock()
            .expect("convex runtime read set lock should not be poisoned")
            .record_index_range(read);
    }

    pub(in crate::adapters::convex) fn record_predicate_read(
        &self,
        table: &TableName,
        filters: &[Filter],
    ) {
        self.read_set
            .lock()
            .expect("convex runtime read set lock should not be poisoned")
            .record_predicate(table, filters);
    }

    pub(in crate::adapters::convex) fn lookup_index_primary_field(
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

    pub(in crate::adapters::convex) fn record_query_read(&self, query: &Query) {
        if !query.filters.is_empty() {
            self.record_predicate_read(&query.table, &query.filters);
        }
        if let Some(index_read) = self.derive_index_read(query, None) {
            self.record_index_read(index_read);
        } else if query.filters.is_empty() {
            self.record_table_read(&query.table);
        }
    }

    pub(in crate::adapters::convex) fn record_executable_query_read(
        &self,
        query: &ConvexExecutableQuery,
    ) {
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

    pub(in crate::adapters::convex) fn record_builder_read(
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

    pub(in crate::adapters::convex) fn derive_index_read(
        &self,
        query: &Query,
        preferred_index_name: Option<&str>,
    ) -> Option<RuntimeIndexRangeRead> {
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

        Some(RuntimeIndexRangeRead {
            table: query.table.clone(),
            index_name: index.name.clone(),
            field,
            start,
            end,
            start_inclusive,
            end_inclusive,
        })
    }
}

impl ConvexRuntimeQueryBuilderState {
    pub(in crate::adapters::convex) fn into_query(self, limit: Option<usize>) -> Query {
        Query {
            table: self.table,
            filters: self.filters,
            order: self.order,
            limit,
        }
    }
}
