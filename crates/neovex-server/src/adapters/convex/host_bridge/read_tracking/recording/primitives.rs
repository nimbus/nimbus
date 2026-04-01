use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn record_table_read(&self, table: &TableName) {
        self.state.record_table_read(table);
    }

    pub(in crate::adapters::convex) fn record_document_read(
        &self,
        table: &TableName,
        document_id: &DocumentId,
    ) {
        self.state.record_document_read(table, document_id);
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
        self.state
            .record_paginated_window_read(query, page_size, after, page);
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
        self.state.record_index_read(read);
    }

    pub(in crate::adapters::convex) fn record_predicate_read(
        &self,
        table: &TableName,
        filters: &[Filter],
    ) {
        self.state.record_predicate_read(table, filters);
    }
}
