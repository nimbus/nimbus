use super::*;

#[tokio::test]
async fn firebase_grpc_commit_executes_atomic_batch_and_consumes_transaction_token() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let transaction = client
        .begin_transaction(GrpcBeginTransactionRequest {
            database: "projects/demo/databases/(default)".to_string(),
            options: None,
        })
        .await
        .expect("Firestore BeginTransaction should succeed")
        .into_inner()
        .transaction;

    let response = client
        .commit(GrpcCommitRequest {
            database: "projects/demo/databases/(default)".to_string(),
            writes: vec![grpc_update_write(
                "projects/demo/databases/(default)/documents/cities/SF",
                [
                    ("name", grpc_string_value("San Francisco")),
                    ("state", grpc_string_value("CA")),
                ],
            )],
            transaction: transaction.clone(),
        })
        .await
        .expect("Firestore Commit should succeed")
        .into_inner();
    assert_eq!(response.write_results.len(), 1);
    assert!(response.write_results[0].update_time.is_some());
    assert!(response.commit_time.is_some());

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let document = service
        .get_document(&tenant_id, &locator.table, locator.id.clone())
        .expect("gRPC commit should persist the document");
    assert_eq!(document.get_field("name"), Some(&json!("San Francisco")));
    assert_eq!(document.get_field("state"), Some(&json!("CA")));

    let error = client
        .commit(GrpcCommitRequest {
            database: "projects/demo/databases/(default)".to_string(),
            writes: vec![grpc_delete_write(
                "projects/demo/databases/(default)/documents/cities/SF",
            )],
            transaction,
        })
        .await
        .expect_err("consumed transaction tokens should not be reusable");
    assert_eq!(error.code(), Code::InvalidArgument);
}

#[tokio::test]
async fn firebase_grpc_batch_get_documents_reads_found_missing_and_rolls_back_sessions() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [
            ("name", json!("San Francisco")),
            ("population", json!(884363)),
            ("state", json!("CA")),
        ],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let transaction = client
        .begin_transaction(GrpcBeginTransactionRequest {
            database: "projects/demo/databases/(default)".to_string(),
            options: Some(GrpcTransactionOptions {
                mode: Some(GrpcTransactionMode::ReadOnly(
                    GrpcReadOnlyTransactionOptions {
                        consistency_selector: None,
                    },
                )),
            }),
        })
        .await
        .expect("read-only BeginTransaction should succeed")
        .into_inner()
        .transaction;

    let mut request = grpc_batch_get_request([
        "projects/demo/databases/(default)/documents/cities/SF",
        "projects/demo/databases/(default)/documents/cities/SF",
        "projects/demo/databases/(default)/documents/cities/LA",
    ]);
    request.mask = Some(grpc_document_mask(["name", "population", "population"]));
    request.consistency_selector = Some(GrpcBatchGetConsistencySelector::Transaction(
        transaction.clone(),
    ));
    let mut responses = client
        .batch_get_documents(request)
        .await
        .expect("BatchGetDocuments should succeed")
        .into_inner();
    let mut entries = Vec::new();
    while let Some(response) = responses
        .message()
        .await
        .expect("BatchGetDocuments responses should stream cleanly")
    {
        entries.push(response);
    }

    assert_eq!(
        entries.len(),
        2,
        "duplicate document names should be elided"
    );
    assert!(entries[0].read_time.is_some());
    match entries[0]
        .result
        .clone()
        .expect("first BatchGet result should be present")
    {
        GrpcBatchGetResult::Found(document) => {
            assert_eq!(
                document.name,
                "projects/demo/databases/(default)/documents/cities/SF"
            );
            assert!(document.fields.contains_key("name"));
            assert!(document.fields.contains_key("population"));
            assert!(
                !document.fields.contains_key("state"),
                "document mask should omit non-requested fields: {document:?}"
            );
        }
        other => panic!("expected found BatchGet result, got {other:?}"),
    }
    match entries[1]
        .result
        .clone()
        .expect("second BatchGet result should be present")
    {
        GrpcBatchGetResult::Missing(document_name) => {
            assert_eq!(
                document_name,
                "projects/demo/databases/(default)/documents/cities/LA"
            );
        }
        other => panic!("expected missing BatchGet result, got {other:?}"),
    }

    client
        .rollback(GrpcRollbackRequest {
            database: "projects/demo/databases/(default)".to_string(),
            transaction: transaction.clone(),
        })
        .await
        .expect("Rollback should succeed");

    let error = client
        .batch_get_documents(GrpcBatchGetDocumentsRequest {
            consistency_selector: Some(GrpcBatchGetConsistencySelector::Transaction(transaction)),
            ..grpc_batch_get_request(["projects/demo/databases/(default)/documents/cities/SF"])
        })
        .await
        .expect_err("rolled back transaction tokens should become inactive");
    assert_eq!(error.code(), Code::InvalidArgument);
}

#[tokio::test]
async fn firebase_grpc_run_query_supports_transaction_selector_with_pinned_snapshot() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("name", json!("San Francisco")), ("visits", json!(1))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let transaction = client
        .begin_transaction(GrpcBeginTransactionRequest {
            database: "projects/demo/databases/(default)".to_string(),
            options: Some(GrpcTransactionOptions {
                mode: Some(GrpcTransactionMode::ReadOnly(
                    GrpcReadOnlyTransactionOptions {
                        consistency_selector: None,
                    },
                )),
            }),
        })
        .await
        .expect("read-only BeginTransaction should succeed")
        .into_inner()
        .transaction;

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    service
        .update_document(
            &tenant_id,
            locator.table.clone(),
            locator.id.clone(),
            serde_json::Map::from_iter([("visits".to_string(), json!(99))]),
        )
        .expect("outside update should commit");

    let mut responses = client
        .run_query(GrpcRunQueryRequest {
            consistency_selector: Some(GrpcRunQueryConsistencySelector::Transaction(
                transaction.clone(),
            )),
            ..grpc_run_query_request(
                "projects/demo/databases/(default)/documents",
                GrpcStructuredQuery {
                    from: vec![GrpcCollectionSelector {
                        collection_id: "cities".to_string(),
                        all_descendants: false,
                    }],
                    r#where: Some(GrpcListenFilter {
                        filter_type: Some(GrpcListenFilterType::FieldFilter(
                            GrpcListenFieldFilter {
                                field: Some(GrpcListenFieldReference {
                                    field_path: "name".to_string(),
                                }),
                                op: GrpcListenFieldFilterOperator::Equal as i32,
                                value: Some(grpc_string_value("San Francisco")),
                            },
                        )),
                    }),
                    ..Default::default()
                },
            )
        })
        .await
        .expect("transactional RunQuery should succeed")
        .into_inner();

    let response = responses
        .message()
        .await
        .expect("RunQuery response should stream")
        .expect("matching document should exist");
    assert_eq!(
        response
            .document
            .as_ref()
            .expect("RunQuery should include a document")
            .fields["visits"]
            .value_type,
        Some(GrpcValueType::IntegerValue(1))
    );
}

#[tokio::test]
async fn firebase_grpc_batch_write_reports_partial_success_and_rejects_duplicates() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let response = client
        .batch_write(GrpcBatchWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            writes: vec![
                grpc_update_write(
                    "projects/demo/databases/(default)/documents/cities/SF",
                    [("name", grpc_string_value("San Francisco"))],
                ),
                GrpcWrite {
                    operation: Some(GrpcWriteOperation::Delete(
                        "projects/demo/databases/(default)/documents/cities/LA".to_string(),
                    )),
                    update_mask: None,
                    update_transforms: Vec::new(),
                    current_document: Some(GrpcPrecondition {
                        condition_type: Some(GrpcConditionType::Exists(true)),
                    }),
                },
            ],
            labels: HashMap::from_iter([("sdk".to_string(), "grpc".to_string())]),
        })
        .await
        .expect("BatchWrite should succeed")
        .into_inner();

    assert_eq!(response.write_results.len(), 2);
    assert_eq!(response.status.len(), 2);
    assert_eq!(response.status[0].code, Code::Ok as i32);
    assert!(response.write_results[0].update_time.is_some());
    assert_eq!(response.status[1].code, Code::NotFound as i32);
    assert!(response.write_results[1].update_time.is_none());

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let stored = service
        .get_document(&tenant_id, &locator.table, locator.id.clone())
        .expect("successful BatchWrite entries should persist");
    assert_eq!(stored.get_field("name"), Some(&json!("San Francisco")));

    let duplicate_error = client
        .batch_write(GrpcBatchWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            writes: vec![
                grpc_update_write(
                    "projects/demo/databases/(default)/documents/cities/SF",
                    [("name", grpc_string_value("San Francisco"))],
                ),
                grpc_delete_write("projects/demo/databases/(default)/documents/cities/SF"),
            ],
            labels: HashMap::new(),
        })
        .await
        .expect_err("duplicate document targets should be rejected");
    assert_eq!(duplicate_error.code(), Code::InvalidArgument);
}

#[tokio::test]
async fn firebase_grpc_run_query_streams_documents_and_empty_results() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("name", json!("San Francisco")), ("state", json!("CA"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SEA"],
        [("name", json!("Seattle")), ("state", json!("WA"))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let mut responses = client
        .run_query(grpc_run_query_request(
            "projects/demo/databases/(default)/documents",
            GrpcStructuredQuery {
                from: vec![GrpcCollectionSelector {
                    collection_id: "cities".to_string(),
                    all_descendants: false,
                }],
                r#where: Some(GrpcListenFilter {
                    filter_type: Some(GrpcListenFilterType::FieldFilter(GrpcListenFieldFilter {
                        field: Some(GrpcListenFieldReference {
                            field_path: "state".to_string(),
                        }),
                        op: GrpcListenFieldFilterOperator::Equal as i32,
                        value: Some(grpc_string_value("CA")),
                    })),
                }),
                ..Default::default()
            },
        ))
        .await
        .expect("RunQuery should succeed")
        .into_inner();
    let first = responses
        .message()
        .await
        .expect("RunQuery response should stream")
        .expect("RunQuery should return one matching document");
    assert!(first.read_time.is_some());
    let document = first.document.expect("RunQuery should include a document");
    assert_eq!(
        document.name,
        "projects/demo/databases/(default)/documents/cities/SF"
    );
    assert!(
        responses
            .message()
            .await
            .expect("RunQuery stream should close cleanly")
            .is_none(),
        "single-result RunQuery should end after the matching document"
    );

    let mut empty_responses = client
        .run_query(grpc_run_query_request(
            "projects/demo/databases/(default)/documents",
            GrpcStructuredQuery {
                from: vec![GrpcCollectionSelector {
                    collection_id: "cities".to_string(),
                    all_descendants: false,
                }],
                r#where: Some(GrpcListenFilter {
                    filter_type: Some(GrpcListenFilterType::FieldFilter(GrpcListenFieldFilter {
                        field: Some(GrpcListenFieldReference {
                            field_path: "state".to_string(),
                        }),
                        op: GrpcListenFieldFilterOperator::Equal as i32,
                        value: Some(grpc_string_value("NV")),
                    })),
                }),
                ..Default::default()
            },
        ))
        .await
        .expect("empty RunQuery should still succeed")
        .into_inner();
    let empty = empty_responses
        .message()
        .await
        .expect("empty RunQuery response should stream")
        .expect("empty RunQuery should return a read_time-only response");
    assert!(empty.read_time.is_some());
    assert!(empty.document.is_none());
}

#[tokio::test]
async fn firebase_grpc_run_query_supports_document_id_filters_and_implicit_name_ordering() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "bravo"],
        [("rank", json!(1))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "alpha"],
        [("rank", json!(1))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "charlie"],
        [("rank", json!(2))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let mut responses = client
        .run_query(grpc_run_query_request(
            "projects/demo/databases/(default)/documents",
            GrpcStructuredQuery {
                from: vec![GrpcCollectionSelector {
                    collection_id: "cities".to_string(),
                    all_descendants: false,
                }],
                r#where: Some(GrpcListenFilter {
                    filter_type: Some(GrpcListenFilterType::FieldFilter(GrpcListenFieldFilter {
                        field: Some(GrpcListenFieldReference {
                            field_path: "__name__".to_string(),
                        }),
                        op: GrpcListenFieldFilterOperator::In as i32,
                        value: Some(grpc_array_value([
                            grpc_reference_value(
                                "projects/demo/databases/(default)/documents/cities/bravo",
                            ),
                            grpc_reference_value(
                                "projects/demo/databases/(default)/documents/cities/alpha",
                            ),
                        ])),
                    })),
                }),
                ..Default::default()
            },
        ))
        .await
        .expect("RunQuery with document ID filter should succeed")
        .into_inner();

    let first = responses
        .message()
        .await
        .expect("first RunQuery response should stream")
        .expect("first matching document should exist");
    let second = responses
        .message()
        .await
        .expect("second RunQuery response should stream")
        .expect("second matching document should exist");
    assert_eq!(
        first
            .document
            .as_ref()
            .expect("first response should include document")
            .name,
        "projects/demo/databases/(default)/documents/cities/alpha"
    );
    assert_eq!(
        second
            .document
            .as_ref()
            .expect("second response should include document")
            .name,
        "projects/demo/databases/(default)/documents/cities/bravo"
    );
    assert!(
        responses
            .message()
            .await
            .expect("RunQuery stream should close cleanly")
            .is_none()
    );
}

#[tokio::test]
async fn firebase_grpc_run_query_supports_collection_group_cursors_with_full_document_names() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF", "districts", "1", "landmarks", "zz-top"],
        [("rank", json!(1))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF", "landmarks", "aa-top"],
        [("rank", json!(1))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF", "landmarks", "bb-top"],
        [("rank", json!(2))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let mut responses = client
        .run_query(grpc_run_query_request(
            "projects/demo/databases/(default)/documents/cities/SF",
            GrpcStructuredQuery {
                from: vec![GrpcCollectionSelector {
                    collection_id: "landmarks".to_string(),
                    all_descendants: true,
                }],
                order_by: vec![GrpcListenOrder {
                    field: Some(GrpcListenFieldReference {
                        field_path: "__name__".to_string(),
                    }),
                    direction: GrpcListenDirection::Ascending as i32,
                }],
                start_at: Some(GrpcCursor {
                    values: vec![grpc_reference_value(
                        "projects/demo/databases/(default)/documents/cities/SF/landmarks/aa-top",
                    )],
                    before: true,
                }),
                ..Default::default()
            },
        ))
        .await
        .expect("collection-group gRPC RunQuery should succeed")
        .into_inner();

    let first = responses
        .message()
        .await
        .expect("first collection-group response should stream")
        .expect("first matching document should exist");
    let second = responses
        .message()
        .await
        .expect("second collection-group response should stream")
        .expect("second matching document should exist");
    assert_eq!(
        first
            .document
            .as_ref()
            .expect("first response should include document")
            .name,
        "projects/demo/databases/(default)/documents/cities/SF/landmarks/aa-top"
    );
    assert_eq!(
        second
            .document
            .as_ref()
            .expect("second response should include document")
            .name,
        "projects/demo/databases/(default)/documents/cities/SF/landmarks/bb-top"
    );
    assert!(
        responses
            .message()
            .await
            .expect("collection-group RunQuery stream should close cleanly")
            .is_none(),
        "the deeper descendant document should be filtered out by the full-path start cursor"
    );
}

#[tokio::test]
async fn firebase_grpc_run_query_reports_missing_index_for_compound_query() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let cities_table = crate::adapters::firebase::storage_table_for_collection_path(
        &CollectionPath::root(CollectionName::new("cities").expect("collection name should parse")),
    )
    .expect("cities collection table should derive");
    service
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: cities_table,
                fields: vec![
                    FieldSchema {
                        name: "state".to_string(),
                        field_type: FieldType::String,
                        required: false,
                    },
                    FieldSchema {
                        name: "rank".to_string(),
                        field_type: FieldType::Number,
                        required: false,
                    },
                ],
                indexes: vec![
                    IndexDefinition {
                        name: "by_state".to_string(),
                        fields: vec!["state".to_string()],
                    },
                    IndexDefinition {
                        name: "by_rank".to_string(),
                        fields: vec!["rank".to_string()],
                    },
                ],
                access_policy: None,
            },
        )
        .expect("single-field cities schema should install");
    let mut client = firestore_grpc_client(&server).await;

    let error = client
        .run_query(grpc_run_query_request(
            "projects/demo/databases/(default)/documents",
            GrpcStructuredQuery {
                from: vec![GrpcCollectionSelector {
                    collection_id: "cities".to_string(),
                    all_descendants: false,
                }],
                r#where: Some(GrpcListenFilter {
                    filter_type: Some(GrpcListenFilterType::FieldFilter(
                        GrpcListenFieldFilter {
                            field: Some(GrpcListenFieldReference {
                                field_path: "state".to_string(),
                            }),
                            op: GrpcListenFieldFilterOperator::Equal as i32,
                            value: Some(grpc_string_value("CA")),
                        },
                    )),
                }),
                order_by: vec![
                    crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_query::Order {
                        field: Some(GrpcListenFieldReference {
                            field_path: "rank".to_string(),
                        }),
                        direction: crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_query::Direction::Ascending as i32,
                    },
                ],
                ..Default::default()
            },
        ))
        .await
        .expect_err("compound RunQuery without a matching index should fail");
    assert_eq!(error.code(), Code::FailedPrecondition);
    assert!(
        error.message().contains("state") && error.message().contains("rank"),
        "missing-index error should mention the required fields: {error:?}"
    );
}

#[tokio::test]
async fn firebase_grpc_run_aggregation_query_counts_filtered_results_with_aliases() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("state", json!("CA"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "LA"],
        [("state", json!("CA"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SEA"],
        [("state", json!("WA"))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let mut responses = client
        .run_aggregation_query(grpc_run_aggregation_query_request(
            "projects/demo/databases/(default)/documents",
            GrpcStructuredQuery {
                from: vec![GrpcCollectionSelector {
                    collection_id: "cities".to_string(),
                    all_descendants: false,
                }],
                r#where: Some(GrpcListenFilter {
                    filter_type: Some(GrpcListenFilterType::FieldFilter(GrpcListenFieldFilter {
                        field: Some(GrpcListenFieldReference {
                            field_path: "state".to_string(),
                        }),
                        op: GrpcListenFieldFilterOperator::Equal as i32,
                        value: Some(grpc_string_value("CA")),
                    })),
                }),
                ..Default::default()
            },
            vec![grpc_count_aggregation("total", None)],
        ))
        .await
        .expect("RunAggregationQuery should succeed")
        .into_inner();

    let response = responses
        .message()
        .await
        .expect("RunAggregationQuery response should stream")
        .expect("RunAggregationQuery should return one aggregate result");
    assert!(response.read_time.is_some());
    assert_eq!(
        response
            .result
            .expect("aggregation response should include a result")
            .aggregate_fields["total"]
            .value_type,
        Some(GrpcValueType::IntegerValue(2))
    );
    assert!(
        responses
            .message()
            .await
            .expect("RunAggregationQuery stream should close cleanly")
            .is_none()
    );
}

#[tokio::test]
async fn firebase_grpc_unary_requests_reject_deferred_selectors() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let batch_get_error = client
        .batch_get_documents(GrpcBatchGetDocumentsRequest {
            consistency_selector: Some(GrpcBatchGetConsistencySelector::NewTransaction(
                GrpcTransactionOptions { mode: None },
            )),
            ..grpc_batch_get_request(["projects/demo/databases/(default)/documents/cities/SF"])
        })
        .await
        .expect_err("BatchGetDocuments new_transaction should be rejected");
    assert_eq!(batch_get_error.code(), Code::InvalidArgument);
    assert!(batch_get_error.message().contains("new_transaction"));

    let begin_error = client
        .begin_transaction(GrpcBeginTransactionRequest {
            database: "projects/demo/databases/(default)".to_string(),
            options: Some(GrpcTransactionOptions {
                mode: Some(GrpcTransactionMode::ReadOnly(
                    GrpcReadOnlyTransactionOptions {
                        consistency_selector: Some(GrpcReadOnlyConsistencySelector::ReadTime(
                            ProstTimestamp {
                                seconds: 1,
                                nanos: 0,
                            },
                        )),
                    },
                )),
            }),
        })
        .await
        .expect_err("read_only.read_time transactions should be rejected");
    assert_eq!(begin_error.code(), Code::InvalidArgument);
    assert!(begin_error.message().contains("read_only.read_time"));

    let run_query_error = client
        .run_query(GrpcRunQueryRequest {
            consistency_selector: Some(GrpcRunQueryConsistencySelector::ReadTime(ProstTimestamp {
                seconds: 1,
                nanos: 0,
            })),
            ..grpc_run_query_request(
                "projects/demo/databases/(default)/documents",
                GrpcStructuredQuery {
                    from: vec![GrpcCollectionSelector {
                        collection_id: "cities".to_string(),
                        all_descendants: false,
                    }],
                    ..Default::default()
                },
            )
        })
        .await
        .expect_err("RunQuery read_time should be rejected");
    assert_eq!(run_query_error.code(), Code::InvalidArgument);
    assert!(run_query_error.message().contains("read_time"));

    let run_aggregation_error = client
        .run_aggregation_query(GrpcRunAggregationQueryRequest {
            consistency_selector: Some(GrpcRunAggregationConsistencySelector::ReadTime(
                ProstTimestamp {
                    seconds: 1,
                    nanos: 0,
                },
            )),
            ..grpc_run_aggregation_query_request(
                "projects/demo/databases/(default)/documents",
                GrpcStructuredQuery {
                    from: vec![GrpcCollectionSelector {
                        collection_id: "cities".to_string(),
                        all_descendants: false,
                    }],
                    ..Default::default()
                },
                vec![grpc_count_aggregation("total", None)],
            )
        })
        .await
        .expect_err("RunAggregationQuery read_time should be rejected");
    assert_eq!(run_aggregation_error.code(), Code::InvalidArgument);
    assert!(run_aggregation_error.message().contains("read_time"));
}

#[tokio::test]
async fn firebase_grpc_get_document_returns_masked_fields_and_honors_transaction_selector() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [
            ("name", json!("San Francisco")),
            ("population", json!(884363)),
            ("state", json!("CA")),
        ],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;
    let transaction = client
        .begin_transaction(GrpcBeginTransactionRequest {
            database: "projects/demo/databases/(default)".to_string(),
            options: Some(GrpcTransactionOptions {
                mode: Some(GrpcTransactionMode::ReadOnly(
                    GrpcReadOnlyTransactionOptions {
                        consistency_selector: None,
                    },
                )),
            }),
        })
        .await
        .expect("read-only BeginTransaction should succeed")
        .into_inner()
        .transaction;

    let response = client
        .get_document(GrpcGetDocumentRequest {
            name: "projects/demo/databases/(default)/documents/cities/SF".to_string(),
            mask: Some(grpc_document_mask(["name", "population"])),
            consistency_selector: Some(
                crate::adapters::firebase::grpc::generated::google::firestore::v1::get_document_request::ConsistencySelector::Transaction(
                    transaction,
                ),
            ),
        })
        .await
        .expect("GetDocument should succeed")
        .into_inner();

    assert_eq!(
        response.name,
        "projects/demo/databases/(default)/documents/cities/SF"
    );
    assert!(response.fields.contains_key("name"));
    assert!(response.fields.contains_key("population"));
    assert!(
        !response.fields.contains_key("state"),
        "field mask should omit non-requested fields: {response:?}"
    );
}

#[tokio::test]
async fn firebase_grpc_point_crud_handles_explicit_and_generated_ids_masks_and_preconditions() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let explicit = client
        .create_document(GrpcCreateDocumentRequest {
            parent: "projects/demo/databases/(default)/documents".to_string(),
            collection_id: "cities".to_string(),
            document_id: "SF".to_string(),
            document: Some(GrpcDocument {
                name: String::new(),
                fields: HashMap::from_iter([
                    ("name".to_string(), grpc_string_value("San Francisco")),
                    ("state".to_string(), grpc_string_value("CA")),
                ]),
                create_time: None,
                update_time: None,
            }),
            mask: Some(grpc_document_mask(["name"])),
        })
        .await
        .expect("CreateDocument with explicit id should succeed")
        .into_inner();
    assert_eq!(
        explicit.name,
        "projects/demo/databases/(default)/documents/cities/SF"
    );
    assert!(explicit.fields.contains_key("name"));
    assert!(
        !explicit.fields.contains_key("state"),
        "response mask should omit non-requested fields: {explicit:?}"
    );

    let generated = client
        .create_document(GrpcCreateDocumentRequest {
            parent: "projects/demo/databases/(default)/documents".to_string(),
            collection_id: "cities".to_string(),
            document_id: String::new(),
            document: Some(GrpcDocument {
                name: String::new(),
                fields: HashMap::from_iter([(
                    "name".to_string(),
                    grpc_string_value("Los Angeles"),
                )]),
                create_time: None,
                update_time: None,
            }),
            mask: None,
        })
        .await
        .expect("CreateDocument without an explicit id should succeed")
        .into_inner();
    let parsed_generated =
        crate::adapters::firebase::resource_names::parse_document_name(&generated.name)
            .expect("generated document name should parse");
    assert_eq!(
        parsed_generated.document_path.collection_path().to_string(),
        "cities"
    );
    assert!(
        !parsed_generated
            .document_path
            .document_id()
            .as_str()
            .is_empty(),
        "generated document id should be non-empty"
    );

    let updated = client
        .update_document(GrpcUpdateDocumentRequest {
            document: Some(GrpcDocument {
                name: "projects/demo/databases/(default)/documents/cities/SF".to_string(),
                fields: HashMap::from_iter([(
                    "name".to_string(),
                    grpc_string_value("San Francisco Updated"),
                )]),
                create_time: None,
                update_time: None,
            }),
            update_mask: Some(grpc_document_mask(["name"])),
            mask: Some(grpc_document_mask(["name"])),
            current_document: None,
        })
        .await
        .expect("UpdateDocument should succeed")
        .into_inner();
    assert!(updated.fields.contains_key("name"));
    assert!(
        !updated.fields.contains_key("state"),
        "response mask should omit non-requested fields: {updated:?}"
    );

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let stored = service
        .get_document(&tenant_id, &locator.table, locator.id.clone())
        .expect("updated document should be readable");
    assert_eq!(
        stored.get_field("name"),
        Some(&json!("San Francisco Updated"))
    );
    assert_eq!(stored.get_field("state"), Some(&json!("CA")));

    client
        .delete_document(GrpcDeleteDocumentRequest {
            name: "projects/demo/databases/(default)/documents/cities/SF".to_string(),
            current_document: Some(GrpcPrecondition {
                condition_type: Some(GrpcConditionType::Exists(true)),
            }),
        })
        .await
        .expect("DeleteDocument should succeed for an existing document");

    let delete_error = client
        .delete_document(GrpcDeleteDocumentRequest {
            name: "projects/demo/databases/(default)/documents/cities/SF".to_string(),
            current_document: Some(GrpcPrecondition {
                condition_type: Some(GrpcConditionType::Exists(true)),
            }),
        })
        .await
        .expect_err("DeleteDocument preconditions should fail on missing documents");
    assert_eq!(delete_error.code(), Code::NotFound);
}

#[tokio::test]
async fn firebase_grpc_list_documents_lists_root_and_nested_collections_with_masks() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "LA"],
        [("name", json!("Los Angeles")), ("state", json!("CA"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("name", json!("San Francisco")), ("state", json!("CA"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF", "landmarks", "bridge"],
        [("label", json!("Golden Gate Bridge"))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let root = client
        .list_documents(GrpcListDocumentsRequest {
            parent: "projects/demo/databases/(default)/documents".to_string(),
            collection_id: "cities".to_string(),
            page_size: 0,
            page_token: String::new(),
            order_by: String::new(),
            mask: Some(grpc_document_mask(["name"])),
            show_missing: false,
            consistency_selector: None,
        })
        .await
        .expect("root ListDocuments should succeed")
        .into_inner();
    assert_eq!(root.documents.len(), 2);
    assert_eq!(
        root.documents[0].name,
        "projects/demo/databases/(default)/documents/cities/LA"
    );
    assert_eq!(
        root.documents[1].name,
        "projects/demo/databases/(default)/documents/cities/SF"
    );
    assert!(root.documents[0].fields.contains_key("name"));
    assert!(!root.documents[0].fields.contains_key("state"));
    assert!(root.next_page_token.is_empty());

    let nested = client
        .list_documents(GrpcListDocumentsRequest {
            parent: "projects/demo/databases/(default)/documents/cities/SF".to_string(),
            collection_id: "landmarks".to_string(),
            page_size: 0,
            page_token: String::new(),
            order_by: String::new(),
            mask: None,
            show_missing: false,
            consistency_selector: None,
        })
        .await
        .expect("nested ListDocuments should succeed")
        .into_inner();
    assert_eq!(nested.documents.len(), 1);
    assert_eq!(
        nested.documents[0].name,
        "projects/demo/databases/(default)/documents/cities/SF/landmarks/bridge"
    );
}

#[tokio::test]
async fn firebase_grpc_list_documents_rejects_deferred_selectors() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let page_size_error = client
        .list_documents(GrpcListDocumentsRequest {
            parent: "projects/demo/databases/(default)/documents".to_string(),
            collection_id: "cities".to_string(),
            page_size: 1,
            page_token: String::new(),
            order_by: String::new(),
            mask: None,
            show_missing: false,
            consistency_selector: None,
        })
        .await
        .expect_err("ListDocuments page_size should be rejected");
    assert_eq!(page_size_error.code(), Code::InvalidArgument);
    assert!(page_size_error.message().contains("page_size"));

    let read_time_error = client
        .list_documents(GrpcListDocumentsRequest {
            parent: "projects/demo/databases/(default)/documents".to_string(),
            collection_id: "cities".to_string(),
            page_size: 0,
            page_token: String::new(),
            order_by: String::new(),
            mask: None,
            show_missing: false,
            consistency_selector: Some(GrpcListDocumentsConsistencySelector::ReadTime(
                prost_types::Timestamp {
                    seconds: 1,
                    nanos: 0,
                },
            )),
        })
        .await
        .expect_err("ListDocuments read_time should be rejected");
    assert_eq!(read_time_error.code(), Code::InvalidArgument);
    assert!(read_time_error.message().contains("read_time"));
}

#[tokio::test]
async fn firebase_grpc_list_collection_ids_lists_root_and_nested_parents_with_pagination() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("name", json!("San Francisco"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["countries", "JP"],
        [("name", json!("Japan"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["regions", "west"],
        [("name", json!("West"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF", "landmarks", "bridge"],
        [("label", json!("Golden Gate Bridge"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF", "neighborhoods", "soma"],
        [("label", json!("SoMa"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF", "landmarks", "bridge", "photos", "p1"],
        [("label", json!("Photo"))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let root_first = client
        .list_collection_ids(GrpcListCollectionIdsRequest {
            parent: "projects/demo/databases/(default)/documents".to_string(),
            page_size: 2,
            page_token: String::new(),
            consistency_selector: None,
        })
        .await
        .expect("root ListCollectionIds should succeed")
        .into_inner();
    assert_eq!(root_first.collection_ids, vec!["cities", "countries"]);
    assert!(!root_first.next_page_token.is_empty());

    let root_second = client
        .list_collection_ids(GrpcListCollectionIdsRequest {
            parent: "projects/demo/databases/(default)/documents".to_string(),
            page_size: 2,
            page_token: root_first.next_page_token.clone(),
            consistency_selector: None,
        })
        .await
        .expect("paged ListCollectionIds should succeed")
        .into_inner();
    assert_eq!(root_second.collection_ids, vec!["regions"]);
    assert!(root_second.next_page_token.is_empty());

    let nested = client
        .list_collection_ids(GrpcListCollectionIdsRequest {
            parent: "projects/demo/databases/(default)/documents/cities/SF".to_string(),
            page_size: 0,
            page_token: String::new(),
            consistency_selector: None,
        })
        .await
        .expect("nested ListCollectionIds should succeed")
        .into_inner();
    assert_eq!(nested.collection_ids, vec!["landmarks", "neighborhoods"]);

    let deep = client
        .list_collection_ids(GrpcListCollectionIdsRequest {
            parent: "projects/demo/databases/(default)/documents/cities/SF/landmarks/bridge"
                .to_string(),
            page_size: 0,
            page_token: String::new(),
            consistency_selector: None,
        })
        .await
        .expect("deep ListCollectionIds should succeed")
        .into_inner();
    assert_eq!(deep.collection_ids, vec!["photos"]);
}

#[tokio::test]
async fn firebase_grpc_list_collection_ids_rejects_invalid_page_tokens_and_read_time() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let invalid_page_token_error = client
        .list_collection_ids(GrpcListCollectionIdsRequest {
            parent: "projects/demo/databases/(default)/documents".to_string(),
            page_size: 1,
            page_token: "not-base64!".to_string(),
            consistency_selector: None,
        })
        .await
        .expect_err("invalid page tokens should be rejected");
    assert_eq!(invalid_page_token_error.code(), Code::InvalidArgument);
    assert!(invalid_page_token_error.message().contains("pageToken"));

    let read_time_error = client
        .list_collection_ids(GrpcListCollectionIdsRequest {
            parent: "projects/demo/databases/(default)/documents".to_string(),
            page_size: 0,
            page_token: String::new(),
            consistency_selector: Some(GrpcListCollectionIdsConsistencySelector::ReadTime(
                prost_types::Timestamp {
                    seconds: 1,
                    nanos: 0,
                },
            )),
        })
        .await
        .expect_err("ListCollectionIds read_time should be rejected");
    assert_eq!(read_time_error.code(), Code::InvalidArgument);
    assert!(read_time_error.message().contains("read_time"));
}
