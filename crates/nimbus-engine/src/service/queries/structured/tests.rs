use serde_json::{Value, json};

use super::{
    PreparedField, PreparedOrder, ProjectionMode, apply_structured_aggregation_limit,
    ensure_structured_query_index, finalize_structured_documents,
    prepare::required_structured_query_index_fields, prepare_structured_query,
    structured_aggregation_result_from_count, structured_base_query,
    validate_structured_aggregation_query,
};
use crate::tests::tasks_table;
use nimbus_core::{
    AggregationOperator, CollectionName, CollectionSelector, CompositeFilter, CompositeOperator,
    CountAggregation, Document, DocumentId, Error, FieldFilter, FieldFilterOperator,
    FieldReference, FindNearest, IndexDefinition, OrderDirection, Projection, Query,
    QueryDirection, QueryFilter, StructuredAggregation, StructuredAggregationQuery,
    StructuredCursor, StructuredOrder, StructuredQuery, TableSchema, UnaryFilter,
    UnaryFilterOperator,
};

fn field(path: &str) -> FieldReference {
    FieldReference::new(path)
}

#[test]
fn prepares_supported_structured_query_subset() {
    let prepared = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            from: vec![CollectionSelector::collection(
                CollectionName::new("tasks").expect("collection should parse"),
            )],
            where_filter: Some(QueryFilter::FieldFilter(FieldFilter {
                field: field("status"),
                op: FieldFilterOperator::Equal,
                value: json!("open"),
            })),
            order_by: vec![
                StructuredOrder {
                    field: field("rank"),
                    direction: QueryDirection::Descending,
                },
                StructuredOrder {
                    field: field("title"),
                    direction: QueryDirection::Ascending,
                },
            ],
            start_at: Some(StructuredCursor {
                values: vec![json!(4), json!("bravo")],
                before: false,
            }),
            end_at: Some(StructuredCursor {
                values: vec![json!(1)],
                before: false,
            }),
            offset: Some(1),
            limit: Some(3),
            select: Some(Projection {
                fields: vec![field("__name__"), field("title")],
            }),
            ..StructuredQuery::default()
        },
    )
    .expect("supported structured query should prepare");

    assert_eq!(
        structured_base_query(&tasks_table(), &prepared),
        Query {
            table: tasks_table(),
            filters: vec![nimbus_core::Filter {
                field: "status".to_string(),
                op: nimbus_core::FilterOp::Eq,
                value: json!("open"),
            }],
            order: None,
            limit: None,
        }
    );
    assert_eq!(
        prepared.order_by,
        vec![
            PreparedOrder {
                field: PreparedField::UserField("rank".to_string()),
                direction: OrderDirection::Desc,
            },
            PreparedOrder {
                field: PreparedField::UserField("title".to_string()),
                direction: OrderDirection::Asc,
            },
            PreparedOrder {
                field: PreparedField::DocumentName,
                direction: OrderDirection::Asc,
            },
        ]
    );
    assert_eq!(prepared.offset, 1);
    assert_eq!(prepared.limit, Some(3));
    assert_eq!(
        prepared.projection,
        ProjectionMode::SelectedFields(vec!["title".to_string()])
    );
}

#[test]
fn executes_repeated_order_cursors_offset_limit_and_projection() {
    let prepared = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            order_by: vec![
                StructuredOrder {
                    field: field("rank"),
                    direction: QueryDirection::Descending,
                },
                StructuredOrder {
                    field: field("title"),
                    direction: QueryDirection::Ascending,
                },
            ],
            start_at: Some(StructuredCursor {
                values: vec![json!(4), json!("bravo")],
                before: false,
            }),
            end_at: Some(StructuredCursor {
                values: vec![json!(1)],
                before: false,
            }),
            offset: Some(1),
            limit: Some(2),
            select: Some(Projection {
                fields: vec![field("__name__"), field("title")],
            }),
            ..StructuredQuery::default()
        },
    )
    .expect("structured query should prepare");

    let docs = vec![
        Document::with_id(
            DocumentId::from_key("alpha").expect("doc id should parse"),
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("alpha")),
                ("rank".to_string(), json!(5)),
                ("status".to_string(), json!("open")),
            ]),
        ),
        Document::with_id(
            DocumentId::from_key("bravo").expect("doc id should parse"),
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("bravo")),
                ("rank".to_string(), json!(4)),
                ("status".to_string(), json!("open")),
            ]),
        ),
        Document::with_id(
            DocumentId::from_key("charlie").expect("doc id should parse"),
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("charlie")),
                ("rank".to_string(), json!(4)),
                ("status".to_string(), json!("open")),
            ]),
        ),
        Document::with_id(
            DocumentId::from_key("delta").expect("doc id should parse"),
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("delta")),
                ("rank".to_string(), json!(2)),
                ("status".to_string(), json!("open")),
            ]),
        ),
        Document::with_id(
            DocumentId::from_key("echo").expect("doc id should parse"),
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("echo")),
                ("rank".to_string(), json!(1)),
                ("status".to_string(), json!("open")),
            ]),
        ),
    ];

    let mut check_cancel = || Ok(());
    let result = finalize_structured_documents(docs, &prepared, &mut check_cancel)
        .expect("structured query should finalize");

    assert_eq!(
        result
            .iter()
            .map(|document| document.id.to_string())
            .collect::<Vec<_>>(),
        vec!["delta".to_string(), "echo".to_string()]
    );
    assert!(
        result
            .iter()
            .all(|document| !document.fields.contains_key("status"))
    );
    assert!(
        result
            .iter()
            .all(|document| document.fields.contains_key("title"))
    );
}

#[test]
fn derives_required_index_fields_for_compound_structured_queries() {
    let prepared = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            where_filter: Some(QueryFilter::FieldFilter(FieldFilter {
                field: field("status"),
                op: FieldFilterOperator::Equal,
                value: json!("open"),
            })),
            order_by: vec![
                StructuredOrder {
                    field: field("rank"),
                    direction: QueryDirection::Ascending,
                },
                StructuredOrder {
                    field: field("title"),
                    direction: QueryDirection::Descending,
                },
            ],
            ..StructuredQuery::default()
        },
    )
    .expect("compound structured query should prepare");

    assert_eq!(
        required_structured_query_index_fields(&prepared),
        Some(vec![
            "status".to_string(),
            "rank".to_string(),
            "title".to_string(),
        ])
    );
}

#[test]
fn compound_structured_queries_require_matching_schema_indexes() {
    let prepared = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            where_filter: Some(QueryFilter::FieldFilter(FieldFilter {
                field: field("status"),
                op: FieldFilterOperator::Equal,
                value: json!("open"),
            })),
            order_by: vec![StructuredOrder {
                field: field("rank"),
                direction: QueryDirection::Ascending,
            }],
            ..StructuredQuery::default()
        },
    )
    .expect("compound structured query should prepare");

    let missing_index = TableSchema {
        table: tasks_table(),
        fields: Vec::new(),
        indexes: vec![IndexDefinition {
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    };
    let error = ensure_structured_query_index(Some(&missing_index), &prepared)
        .expect_err("compound query should require a matching composite index");
    assert!(matches!(error, Error::InvalidInput(message) if message.contains("requires an index")));

    let matching_index = TableSchema {
        table: tasks_table(),
        fields: Vec::new(),
        indexes: vec![IndexDefinition {
            name: "by_status_rank".to_string(),
            fields: vec!["status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    };
    ensure_structured_query_index(Some(&matching_index), &prepared)
        .expect("matching composite index should satisfy the query");
}

#[test]
fn rejects_nested_projection_fields() {
    let error = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            select: Some(Projection {
                fields: vec![field("stats.rank")],
            }),
            ..StructuredQuery::default()
        },
    )
    .expect_err("projection should be rejected");

    assert!(matches!(error, Error::InvalidInput(message) if message.contains("nested projection")));
}

#[test]
fn rejects_collection_group_sources() {
    let error = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            from: vec![CollectionSelector::collection_group(
                CollectionName::new("tasks").expect("collection should parse"),
            )],
            ..StructuredQuery::default()
        },
    )
    .expect_err("collection group should be rejected");

    assert!(matches!(error, Error::InvalidInput(message) if message.contains("collection group")));
}

#[test]
fn executes_composite_unary_and_membership_filters() {
    let docs = vec![
        Document::with_id(
            DocumentId::from_key("alpha").expect("doc id should parse"),
            tasks_table(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("pending")),
                ("state".to_string(), Value::Null),
                ("tags".to_string(), json!(["bridge", "west"])),
            ]),
        ),
        Document::with_id(
            DocumentId::from_key("bravo").expect("doc id should parse"),
            tasks_table(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("open")),
                ("state".to_string(), json!("CA")),
                ("tags".to_string(), json!(["park", "west"])),
            ]),
        ),
        Document::with_id(
            DocumentId::from_key("charlie").expect("doc id should parse"),
            tasks_table(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("closed")),
                ("state".to_string(), json!("NV")),
                ("tags".to_string(), json!(["east"])),
            ]),
        ),
    ];

    let composite_or = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            where_filter: Some(QueryFilter::CompositeFilter(CompositeFilter {
                op: CompositeOperator::Or,
                filters: vec![
                    QueryFilter::FieldFilter(FieldFilter {
                        field: field("tags"),
                        op: FieldFilterOperator::ArrayContains,
                        value: json!("west"),
                    }),
                    QueryFilter::UnaryFilter(UnaryFilter {
                        op: UnaryFilterOperator::IsNull,
                        field: field("state"),
                    }),
                ],
            })),
            ..StructuredQuery::default()
        },
    )
    .expect("composite OR query should prepare");
    let mut check_cancel = || Ok(());
    let composite_result =
        finalize_structured_documents(docs.clone(), &composite_or, &mut check_cancel)
            .expect("composite OR query should execute");
    assert_eq!(
        composite_result
            .iter()
            .map(|document| document.id.to_string())
            .collect::<Vec<_>>(),
        vec!["alpha".to_string(), "bravo".to_string()]
    );

    let in_query = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            where_filter: Some(QueryFilter::FieldFilter(FieldFilter {
                field: field("status"),
                op: FieldFilterOperator::In,
                value: json!(["open", "pending"]),
            })),
            ..StructuredQuery::default()
        },
    )
    .expect("IN query should prepare");
    let in_result = finalize_structured_documents(docs.clone(), &in_query, &mut check_cancel)
        .expect("IN query should execute");
    assert_eq!(
        in_result
            .iter()
            .map(|document| document.id.to_string())
            .collect::<Vec<_>>(),
        vec!["alpha".to_string(), "bravo".to_string()]
    );

    let array_any_query = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            where_filter: Some(QueryFilter::FieldFilter(FieldFilter {
                field: field("tags"),
                op: FieldFilterOperator::ArrayContainsAny,
                value: json!(["park", "bridge"]),
            })),
            ..StructuredQuery::default()
        },
    )
    .expect("ARRAY_CONTAINS_ANY query should prepare");
    let array_any_result =
        finalize_structured_documents(docs.clone(), &array_any_query, &mut check_cancel)
            .expect("ARRAY_CONTAINS_ANY query should execute");
    assert_eq!(
        array_any_result
            .iter()
            .map(|document| document.id.to_string())
            .collect::<Vec<_>>(),
        vec!["alpha".to_string(), "bravo".to_string()]
    );

    let not_in_query = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            where_filter: Some(QueryFilter::FieldFilter(FieldFilter {
                field: field("status"),
                op: FieldFilterOperator::NotIn,
                value: json!(["closed"]),
            })),
            ..StructuredQuery::default()
        },
    )
    .expect("NOT_IN query should prepare");
    let not_in_result = finalize_structured_documents(docs, &not_in_query, &mut check_cancel)
        .expect("NOT_IN query should execute");
    assert_eq!(
        not_in_result
            .iter()
            .map(|document| document.id.to_string())
            .collect::<Vec<_>>(),
        vec!["bravo".to_string(), "alpha".to_string()]
    );
}

#[test]
fn supports_document_id_filters_and_implicit_name_ordering() {
    let docs = vec![
        Document::with_id(
            DocumentId::from_key("bravo").expect("doc id should parse"),
            tasks_table(),
            serde_json::Map::from_iter([("rank".to_string(), json!(1))]),
        ),
        Document::with_id(
            DocumentId::from_key("alpha").expect("doc id should parse"),
            tasks_table(),
            serde_json::Map::from_iter([("rank".to_string(), json!(1))]),
        ),
        Document::with_id(
            DocumentId::from_key("charlie").expect("doc id should parse"),
            tasks_table(),
            serde_json::Map::from_iter([("rank".to_string(), json!(2))]),
        ),
    ];

    let document_id_query = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            where_filter: Some(QueryFilter::FieldFilter(FieldFilter {
                field: field("__name__"),
                op: FieldFilterOperator::In,
                value: json!([
                    "projects/demo/databases/(default)/documents/tasks/bravo",
                    "tasks/alpha"
                ]),
            })),
            ..StructuredQuery::default()
        },
    )
    .expect("document ID query should prepare");
    let mut check_cancel = || Ok(());
    let document_id_result =
        finalize_structured_documents(docs.clone(), &document_id_query, &mut check_cancel)
            .expect("document ID query should execute");
    assert_eq!(
        document_id_result
            .iter()
            .map(|document| document.id.to_string())
            .collect::<Vec<_>>(),
        vec!["alpha".to_string(), "bravo".to_string()]
    );

    let implicit_name_order = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            order_by: vec![StructuredOrder {
                field: field("rank"),
                direction: QueryDirection::Ascending,
            }],
            ..StructuredQuery::default()
        },
    )
    .expect("implicit __name__ ordering query should prepare");
    assert_eq!(
        implicit_name_order.order_by,
        vec![
            PreparedOrder {
                field: PreparedField::UserField("rank".to_string()),
                direction: OrderDirection::Asc,
            },
            PreparedOrder {
                field: PreparedField::DocumentName,
                direction: OrderDirection::Asc,
            },
        ]
    );
    let implicit_name_result =
        finalize_structured_documents(docs, &implicit_name_order, &mut check_cancel)
            .expect("implicit __name__ ordering query should execute");
    assert_eq!(
        implicit_name_result
            .iter()
            .map(|document| document.id.to_string())
            .collect::<Vec<_>>(),
        vec![
            "alpha".to_string(),
            "bravo".to_string(),
            "charlie".to_string(),
        ]
    );
}

#[test]
fn rejects_invalid_filter_combinations_and_keeps_find_nearest_deferred() {
    let multiple_negative = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            where_filter: Some(QueryFilter::CompositeFilter(CompositeFilter {
                op: CompositeOperator::And,
                filters: vec![
                    QueryFilter::FieldFilter(FieldFilter {
                        field: field("status"),
                        op: FieldFilterOperator::NotEqual,
                        value: json!("open"),
                    }),
                    QueryFilter::UnaryFilter(UnaryFilter {
                        op: UnaryFilterOperator::IsNotNull,
                        field: field("state"),
                    }),
                ],
            })),
            ..StructuredQuery::default()
        },
    )
    .expect_err("multiple negative filters should be rejected");
    assert!(matches!(
        multiple_negative,
        Error::InvalidInput(message) if message.contains("cannot combine multiple NOT_EQUAL")
    ));

    let duplicate_array_contains_any = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            where_filter: Some(QueryFilter::CompositeFilter(CompositeFilter {
                op: CompositeOperator::And,
                filters: vec![
                    QueryFilter::FieldFilter(FieldFilter {
                        field: field("tags"),
                        op: FieldFilterOperator::ArrayContainsAny,
                        value: json!(["west"]),
                    }),
                    QueryFilter::FieldFilter(FieldFilter {
                        field: field("labels"),
                        op: FieldFilterOperator::ArrayContainsAny,
                        value: json!(["gold"]),
                    }),
                ],
            })),
            ..StructuredQuery::default()
        },
    )
    .expect_err("multiple ARRAY_CONTAINS_ANY filters should be rejected");
    assert!(matches!(
        duplicate_array_contains_any,
        Error::InvalidInput(message) if message.contains("ARRAY_CONTAINS_ANY")
    ));

    let inequality_order_mismatch = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            where_filter: Some(QueryFilter::FieldFilter(FieldFilter {
                field: field("rank"),
                op: FieldFilterOperator::GreaterThan,
                value: json!(1),
            })),
            order_by: vec![StructuredOrder {
                field: field("title"),
                direction: QueryDirection::Ascending,
            }],
            ..StructuredQuery::default()
        },
    )
    .expect_err("inequality order mismatch should be rejected");
    assert!(matches!(
        inequality_order_mismatch,
        Error::InvalidInput(message) if message.contains("first order_by field")
    ));

    let find_nearest = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            find_nearest: Some(FindNearest {
                vector_field: field("embedding"),
                query_vector: json!([0.1, 0.2]),
                distance_measure: nimbus_core::DistanceMeasure::Cosine,
                limit: 5,
                distance_result_field: None,
                distance_threshold: None,
            }),
            ..StructuredQuery::default()
        },
    )
    .expect_err("find_nearest should be rejected");
    assert!(
        matches!(find_nearest, Error::InvalidInput(message) if message.contains("find_nearest"))
    );
}

#[test]
fn rejects_nested_filter_and_order_paths_and_cursor_width_mismatches() {
    let nested_filter = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            where_filter: Some(QueryFilter::FieldFilter(FieldFilter {
                field: field("stats.rank"),
                op: FieldFilterOperator::Equal,
                value: json!(1),
            })),
            ..StructuredQuery::default()
        },
    )
    .expect_err("nested filter should be rejected");
    assert!(
        matches!(nested_filter, Error::InvalidInput(message) if message.contains("nested field paths in filters"))
    );

    let nested_order = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            order_by: vec![StructuredOrder {
                field: field("stats.rank"),
                direction: QueryDirection::Ascending,
            }],
            ..StructuredQuery::default()
        },
    )
    .expect_err("nested order should be rejected");
    assert!(
        matches!(nested_order, Error::InvalidInput(message) if message.contains("nested field paths in order_by"))
    );

    let too_wide_cursor = prepare_structured_query(
        &tasks_table(),
        &StructuredQuery {
            order_by: vec![StructuredOrder {
                field: field("rank"),
                direction: QueryDirection::Ascending,
            }],
            start_at: Some(StructuredCursor {
                values: vec![json!(1), json!("extra"), json!("overflow")],
                before: true,
            }),
            ..StructuredQuery::default()
        },
    )
    .expect_err("cursor width mismatch should be rejected");
    assert!(matches!(
        too_wide_cursor,
        Error::InvalidInput(message) if message.contains("cannot include more values")
    ));
}

#[test]
fn count_aggregations_apply_up_to_limits_and_trim_query_limit() {
    let aggregation_query = StructuredAggregationQuery {
        structured_query: StructuredQuery {
            limit: Some(50),
            ..StructuredQuery::default()
        },
        aggregations: vec![
            StructuredAggregation {
                alias: "small".to_string(),
                operator: AggregationOperator::Count(CountAggregation { up_to: Some(3) }),
            },
            StructuredAggregation {
                alias: "large".to_string(),
                operator: AggregationOperator::Count(CountAggregation { up_to: Some(8) }),
            },
        ],
    };

    validate_structured_aggregation_query(&aggregation_query)
        .expect("count aggregations should validate");
    let limited = apply_structured_aggregation_limit(
        &aggregation_query.structured_query,
        &aggregation_query.aggregations,
    );
    assert_eq!(limited.limit, Some(8));

    let result = structured_aggregation_result_from_count(&aggregation_query.aggregations, 5)
        .expect("count aggregations should execute");
    assert_eq!(result.aggregate_fields["small"], json!(3));
    assert_eq!(result.aggregate_fields["large"], json!(5));
}

#[test]
fn rejects_duplicate_aliases_and_deferred_sum_aggregations() {
    let duplicate_aliases = StructuredAggregationQuery {
        structured_query: StructuredQuery::default(),
        aggregations: vec![
            StructuredAggregation {
                alias: "total".to_string(),
                operator: AggregationOperator::Count(CountAggregation::default()),
            },
            StructuredAggregation {
                alias: "total".to_string(),
                operator: AggregationOperator::Count(CountAggregation::default()),
            },
        ],
    };
    assert!(matches!(
        validate_structured_aggregation_query(&duplicate_aliases),
        Err(Error::InvalidInput(message)) if message.contains("must be unique")
    ));

    let unsupported_sum = StructuredAggregationQuery {
        structured_query: StructuredQuery::default(),
        aggregations: vec![StructuredAggregation {
            alias: "sum_rank".to_string(),
            operator: AggregationOperator::Sum(field("rank")),
        }],
    };
    assert!(matches!(
        validate_structured_aggregation_query(&unsupported_sum),
        Err(Error::InvalidInput(message)) if message.contains("sum aggregations")
    ));
}
