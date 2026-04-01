use super::*;

#[derive(Debug, Default)]
pub(in crate::adapters::convex) struct ConvexRuntimeQueryBuilders {
    pub(in crate::adapters::convex) next_builder_id: u64,
    pub(in crate::adapters::convex) builders: HashMap<String, ConvexRuntimeQueryBuilderState>,
}

#[derive(Debug, Clone)]
pub(in crate::adapters::convex) struct ConvexRuntimeQueryBuilderState {
    pub(in crate::adapters::convex) table: TableName,
    pub(in crate::adapters::convex) filters: Vec<Filter>,
    pub(in crate::adapters::convex) order: Option<OrderBy>,
    pub(in crate::adapters::convex) order_field_hint: Option<String>,
    pub(in crate::adapters::convex) index_name: Option<String>,
}
