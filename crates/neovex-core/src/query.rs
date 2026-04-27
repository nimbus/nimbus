use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

use crate::resource_path::CollectionName;
use crate::types::TableName;

/// A single-table query.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Query {
    pub table: TableName,
    pub filters: Vec<Filter>,
    pub order: Option<OrderBy>,
    pub limit: Option<usize>,
}

/// Opaque pagination cursor.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cursor(pub String);

/// A paginated query request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Filter {
    pub field: String,
    pub op: FilterOp,
    pub value: Value,
}

/// Supported filter operators for phase 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrderBy {
    pub field: String,
    pub direction: OrderDirection,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderDirection {
    Asc,
    Desc,
}

/// Parser-facing structured query surface for Firestore-style query metadata.
///
/// This intentionally lives beside the legacy planner `Query` type instead of
/// replacing it in one pass. The engine still consumes the narrower legacy
/// query during `F0.4a`; `F0.4b` will own the translation/adoption and
/// unsupported-feature behavior.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<Projection>,
    #[serde(default)]
    pub from: Vec<CollectionSelector>,
    #[serde(rename = "where", default, skip_serializing_if = "Option::is_none")]
    pub where_filter: Option<QueryFilter>,
    #[serde(default)]
    pub order_by: Vec<StructuredOrder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_at: Option<StructuredCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_at: Option<StructuredCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub find_nearest: Option<FindNearest>,
}

/// Parser-facing structured aggregation query for Firestore-style aggregate
/// execution over a structured query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredAggregationQuery {
    pub structured_query: StructuredQuery,
    #[serde(default)]
    pub aggregations: Vec<StructuredAggregation>,
}

/// One aggregation entry within a structured aggregation query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredAggregation {
    pub alias: String,
    pub operator: AggregationOperator,
}

/// Shared aggregation operator surface for Firestore compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregationOperator {
    Count(CountAggregation),
    Sum(FieldReference),
    Avg(FieldReference),
}

/// Optional `COUNT_UP_TO(...)` bound for count aggregations.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountAggregation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_to: Option<u64>,
}

/// Shared aggregation result map keyed by normalized aggregation alias.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredAggregationResult {
    #[serde(default)]
    pub aggregate_fields: Map<String, Value>,
}

/// Firestore `from` selector metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSelector {
    pub collection_id: CollectionName,
    #[serde(default)]
    pub all_descendants: bool,
}

impl CollectionSelector {
    pub fn collection(collection_id: CollectionName) -> Self {
        Self {
            collection_id,
            all_descendants: false,
        }
    }

    pub fn collection_group(collection_id: CollectionName) -> Self {
        Self {
            collection_id,
            all_descendants: true,
        }
    }

    pub fn is_collection_group(&self) -> bool {
        self.all_descendants
    }
}

/// Optional projection mask for structured queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Projection {
    #[serde(default)]
    pub fields: Vec<FieldReference>,
}

/// Dot-delimited field reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldReference {
    pub field_path: String,
}

impl FieldReference {
    pub fn new(field_path: impl Into<String>) -> Self {
        Self {
            field_path: field_path.into(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.field_path
    }
}

/// Structured query filter tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QueryFilter {
    #[serde(rename = "compositeFilter")]
    CompositeFilter(CompositeFilter),
    #[serde(rename = "fieldFilter")]
    FieldFilter(FieldFilter),
    #[serde(rename = "unaryFilter")]
    UnaryFilter(UnaryFilter),
}

/// Composite filter metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositeFilter {
    pub op: CompositeOperator,
    #[serde(default)]
    pub filters: Vec<QueryFilter>,
}

/// Composite filter operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompositeOperator {
    And,
    Or,
}

/// Structured field filter metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldFilter {
    pub field: FieldReference,
    pub op: FieldFilterOperator,
    pub value: Value,
}

/// Firestore field filter operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FieldFilterOperator {
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    Equal,
    NotEqual,
    ArrayContains,
    In,
    ArrayContainsAny,
    NotIn,
}

/// Structured unary filter metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnaryFilter {
    pub op: UnaryFilterOperator,
    pub field: FieldReference,
}

/// Firestore unary filter operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UnaryFilterOperator {
    IsNan,
    IsNull,
    IsNotNan,
    IsNotNull,
}

/// Repeated ordering metadata for structured queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredOrder {
    pub field: FieldReference,
    pub direction: QueryDirection,
}

/// Firestore structured-query sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QueryDirection {
    Ascending,
    Descending,
}

/// Structured query cursor bound.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StructuredCursor {
    #[serde(default)]
    pub values: Vec<Value>,
    #[serde(default)]
    pub before: bool,
}

/// Explicit placeholder for deferred vector-search semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindNearest {
    pub vector_field: FieldReference,
    pub query_vector: Value,
    pub distance_measure: DistanceMeasure,
    pub limit: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance_result_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance_threshold: Option<Number>,
}

/// Vector distance measure placeholder for deferred nearest-neighbor support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DistanceMeasure {
    Euclidean,
    Cosine,
    DotProduct,
}

#[cfg(test)]
mod tests {
    use serde_json::{Number, json};

    use super::{
        AggregationOperator, CollectionSelector, CompositeFilter, CompositeOperator,
        CountAggregation, DistanceMeasure, FieldFilter, FieldFilterOperator, FieldReference,
        FindNearest, Projection, QueryDirection, QueryFilter, StructuredAggregation,
        StructuredAggregationQuery, StructuredAggregationResult, StructuredCursor, StructuredOrder,
        StructuredQuery, UnaryFilter, UnaryFilterOperator,
    };
    use crate::CollectionName;

    fn field(path: &str) -> FieldReference {
        FieldReference::new(path)
    }

    #[test]
    fn structured_query_roundtrips_collection_sources() {
        let query = StructuredQuery {
            from: vec![
                CollectionSelector::collection(
                    CollectionName::new("cities.v2").expect("collection id should parse"),
                ),
                CollectionSelector::collection_group(
                    CollectionName::new("日本語").expect("collection group should parse"),
                ),
            ],
            ..StructuredQuery::default()
        };

        let encoded = serde_json::to_value(&query).expect("structured query should serialize");
        assert_eq!(encoded["from"][0]["collectionId"], json!("cities.v2"));
        assert_eq!(encoded["from"][0]["allDescendants"], json!(false));
        assert_eq!(encoded["from"][1]["collectionId"], json!("日本語"));
        assert_eq!(encoded["from"][1]["allDescendants"], json!(true));

        let decoded: StructuredQuery =
            serde_json::from_value(encoded).expect("structured query should deserialize");
        assert!(!decoded.from[0].is_collection_group());
        assert!(decoded.from[1].is_collection_group());
    }

    #[test]
    fn structured_query_roundtrips_projection_order_cursors_offset_and_limit() {
        let query = StructuredQuery {
            select: Some(Projection {
                fields: vec![field("__name__"), field("stats.rank")],
            }),
            from: vec![CollectionSelector::collection(
                CollectionName::new("cities").expect("collection id should parse"),
            )],
            order_by: vec![
                StructuredOrder {
                    field: field("population"),
                    direction: QueryDirection::Ascending,
                },
                StructuredOrder {
                    field: field("__name__"),
                    direction: QueryDirection::Descending,
                },
            ],
            start_at: Some(StructuredCursor {
                values: vec![json!(1000), json!("cities/SF")],
                before: false,
            }),
            end_at: Some(StructuredCursor {
                values: vec![json!(2000), json!("cities/SEA")],
                before: true,
            }),
            offset: Some(25),
            limit: Some(10),
            ..StructuredQuery::default()
        };

        let encoded = serde_json::to_value(&query).expect("structured query should serialize");
        assert_eq!(
            encoded["select"]["fields"][0]["fieldPath"],
            json!("__name__")
        );
        assert_eq!(
            encoded["orderBy"][0]["field"]["fieldPath"],
            json!("population")
        );
        assert_eq!(encoded["orderBy"][1]["direction"], json!("DESCENDING"));
        assert_eq!(encoded["startAt"]["values"][0], json!(1000));
        assert_eq!(encoded["endAt"]["before"], json!(true));
        assert_eq!(encoded["offset"], json!(25));
        assert_eq!(encoded["limit"], json!(10));

        let decoded: StructuredQuery =
            serde_json::from_value(encoded).expect("structured query should deserialize");
        assert_eq!(decoded, query);
    }

    #[test]
    fn structured_query_roundtrips_composite_and_unary_filters() {
        let query = StructuredQuery {
            where_filter: Some(QueryFilter::CompositeFilter(CompositeFilter {
                op: CompositeOperator::And,
                filters: vec![
                    QueryFilter::FieldFilter(FieldFilter {
                        field: field("state"),
                        op: FieldFilterOperator::Equal,
                        value: json!("CA"),
                    }),
                    QueryFilter::UnaryFilter(UnaryFilter {
                        op: UnaryFilterOperator::IsNotNull,
                        field: field("population"),
                    }),
                ],
            })),
            ..StructuredQuery::default()
        };

        let encoded = serde_json::to_value(&query).expect("structured query should serialize");
        assert_eq!(encoded["where"]["compositeFilter"]["op"], json!("AND"));
        assert_eq!(
            encoded["where"]["compositeFilter"]["filters"][0]["fieldFilter"]["field"]["fieldPath"],
            json!("state")
        );
        assert_eq!(
            encoded["where"]["compositeFilter"]["filters"][1]["unaryFilter"]["op"],
            json!("IS_NOT_NULL")
        );

        let decoded: StructuredQuery =
            serde_json::from_value(encoded).expect("structured query should deserialize");
        assert_eq!(decoded, query);
    }

    #[test]
    fn structured_query_roundtrips_find_nearest_placeholder() {
        let query = StructuredQuery {
            from: vec![CollectionSelector::collection_group(
                CollectionName::new("landmarks").expect("collection group should parse"),
            )],
            find_nearest: Some(FindNearest {
                vector_field: field("embedding"),
                query_vector: json!([0.1, 0.2, 0.3]),
                distance_measure: DistanceMeasure::Cosine,
                limit: 5,
                distance_result_field: Some("distance".to_string()),
                distance_threshold: Some(
                    Number::from_f64(0.42).expect("distance threshold should be finite"),
                ),
            }),
            ..StructuredQuery::default()
        };

        let encoded = serde_json::to_value(&query).expect("structured query should serialize");
        assert_eq!(encoded["findNearest"]["distanceMeasure"], json!("COSINE"));
        assert_eq!(encoded["findNearest"]["limit"], json!(5));
        assert_eq!(
            encoded["findNearest"]["distanceResultField"],
            json!("distance")
        );

        let decoded: StructuredQuery =
            serde_json::from_value(encoded).expect("structured query should deserialize");
        assert_eq!(decoded, query);
    }

    #[test]
    fn structured_aggregation_query_roundtrips_count_aliases_and_results() {
        let aggregation_query = StructuredAggregationQuery {
            structured_query: StructuredQuery {
                from: vec![CollectionSelector::collection(
                    CollectionName::new("cities").expect("collection id should parse"),
                )],
                ..StructuredQuery::default()
            },
            aggregations: vec![
                StructuredAggregation {
                    alias: "total".to_string(),
                    operator: AggregationOperator::Count(CountAggregation { up_to: Some(25) }),
                },
                StructuredAggregation {
                    alias: "field_1".to_string(),
                    operator: AggregationOperator::Count(CountAggregation { up_to: None }),
                },
            ],
        };

        let encoded = serde_json::to_value(&aggregation_query)
            .expect("structured aggregation query should serialize");
        let decoded: StructuredAggregationQuery = serde_json::from_value(encoded)
            .expect("structured aggregation query should deserialize");
        assert_eq!(decoded, aggregation_query);

        let result = StructuredAggregationResult {
            aggregate_fields: serde_json::Map::from_iter([
                ("total".to_string(), json!(2)),
                ("field_1".to_string(), json!(5)),
            ]),
        };
        let encoded_result =
            serde_json::to_value(&result).expect("structured aggregation result should serialize");
        assert_eq!(encoded_result["aggregateFields"]["total"], json!(2));
    }
}
