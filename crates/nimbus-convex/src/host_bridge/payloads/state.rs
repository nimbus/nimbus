use super::*;

#[derive(Debug, Default)]
pub struct ConvexRuntimeQueryBuilders {
    pub next_builder_id: u64,
    pub builders: HashMap<String, ConvexRuntimeQueryBuilderState>,
}

#[derive(Debug, Clone)]
pub struct ConvexRuntimeQueryBuilderState {
    pub table: TableName,
    pub filters: Vec<Filter>,
    pub order: Option<OrderBy>,
    pub order_field_hint: Option<String>,
    pub index_name: Option<String>,
}

impl ConvexRuntimeQueryBuilderState {
    pub fn into_query(self, limit: Option<usize>) -> Query {
        Query {
            table: self.table,
            filters: self.filters,
            order: self.order,
            limit,
        }
    }
}
