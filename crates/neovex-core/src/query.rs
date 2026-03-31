use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::TableName;

/// A single-table query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub table: TableName,
    pub filters: Vec<Filter>,
    pub order: Option<OrderBy>,
    pub limit: Option<usize>,
}

/// Opaque pagination cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor(pub String);

/// A paginated query request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaginatedQuery {
    pub query: Query,
    pub page_size: usize,
    pub after: Option<Cursor>,
}

/// A paginated query result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub data: Vec<Value>,
    pub next_cursor: Option<Cursor>,
    pub has_more: bool,
}

/// A field predicate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub field: String,
    pub op: FilterOp,
    pub value: Value,
}

/// Supported filter operators for phase 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
}

/// Ordering clause for a query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderBy {
    pub field: String,
    pub direction: OrderDirection,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderDirection {
    Asc,
    Desc,
}
