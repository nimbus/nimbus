use super::*;

impl ConvexHostBridge {
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
}
