use super::*;

#[tokio::test]
async fn firebase_write_stream_handshakes_and_applies_ordered_writes() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    assert_firebase_grpc_write_stream_anonymous_refused(
        &server,
        "projects/demo/databases/(default)",
    )
    .await;
    let mut client = firestore_grpc_authed_client(&server, "user-123", "demo").await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .write(receiver)
        .await
        .expect("Firestore write stream should open")
        .into_inner();

    sender
        .unbounded_send(GrpcWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            ..Default::default()
        })
        .expect("handshake request should send");
    let handshake = responses
        .message()
        .await
        .expect("handshake response should stream")
        .expect("handshake response should be present");
    assert!(
        !handshake.stream_id.is_empty(),
        "handshake should allocate a stream id"
    );
    assert!(
        !handshake.stream_token.is_empty(),
        "handshake should allocate a stream token"
    );
    assert!(
        handshake.write_results.is_empty(),
        "handshake should not include write results"
    );
    assert!(
        handshake.commit_time.is_none(),
        "handshake should not include a commit time"
    );

    sender
        .unbounded_send(GrpcWriteRequest {
            stream_token: handshake.stream_token.clone(),
            writes: vec![
                grpc_update_write(
                    "projects/demo/databases/(default)/documents/cities/SF",
                    [
                        ("name", grpc_string_value("San Francisco")),
                        ("population", grpc_integer_value(884_363)),
                    ],
                ),
                grpc_delete_write("projects/demo/databases/(default)/documents/cities/LA"),
            ],
            ..Default::default()
        })
        .expect("write batch request should send");
    let write_response = responses
        .message()
        .await
        .expect("write response should stream")
        .expect("write response should be present");
    assert!(
        write_response.stream_id.is_empty(),
        "non-handshake responses should not repeat the stream id"
    );
    assert_eq!(
        write_response.write_results.len(),
        2,
        "ordered write response should include both results"
    );
    assert!(
        write_response.write_results[0].update_time.is_some(),
        "update write should expose update_time"
    );
    assert!(
        write_response.write_results[1].update_time.is_none(),
        "delete missing should not expose update_time"
    );
    assert!(
        write_response.commit_time.is_some(),
        "committed write batch should expose commit_time"
    );

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let document = service
        .get_document(&tenant_id, &locator.table, locator.id.clone())
        .expect("gRPC write stream should commit the document");
    assert_eq!(document.get_field("name"), Some(&json!("San Francisco")));
    assert_eq!(document.get_field("population"), Some(&json!(884363)));

    drop(sender);
    assert!(
        responses
            .message()
            .await
            .expect("closed write stream should not error")
            .is_none(),
        "write stream should end cleanly after the request sender closes"
    );
}

#[tokio::test]
async fn firebase_write_stream_roundtrips_lossless_firestore_field_types() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let server = ServerFixture::start(router_for_firebase(
        fixture.engine(),
        firebase_verified_config(),
    ))
    .await;
    let mut client = firestore_grpc_authed_client(&server, "user-123", "demo").await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .write(receiver)
        .await
        .expect("Firestore write stream should open")
        .into_inner();

    sender
        .unbounded_send(GrpcWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            ..Default::default()
        })
        .expect("handshake request should send");
    let handshake = responses
        .message()
        .await
        .expect("handshake should stream")
        .expect("handshake should be present");
    let document_name = "projects/demo/databases/(default)/documents/events/typed-grpc";
    sender
        .unbounded_send(GrpcWriteRequest {
            stream_token: handshake.stream_token,
            writes: vec![grpc_update_write(
                document_name,
                [
                    (
                        "createdAt",
                        grpc_timestamp_value(1_704_164_645, 123_456_789),
                    ),
                    ("payload", grpc_bytes_value([1, 2, 3, 4])),
                    (
                        "owner",
                        grpc_reference_value(
                            "projects/demo/databases/(default)/documents/users/ada",
                        ),
                    ),
                    ("location", grpc_geo_point_value(37.7749, -122.4194)),
                    ("score", grpc_double_value(f64::NEG_INFINITY)),
                    (
                        "nested",
                        grpc_map_value([
                            ("attachment", grpc_bytes_value([0xFF, 0x00])),
                            ("label", grpc_string_value("kept")),
                        ]),
                    ),
                ],
            )],
            ..Default::default()
        })
        .expect("typed write request should send");
    responses
        .message()
        .await
        .expect("typed write response should stream")
        .expect("typed write response should be present");
    drop(sender);
    drop(responses);

    let mut reads = client
        .batch_get_documents(grpc_batch_get_request([document_name]))
        .await
        .expect("typed batch get should succeed")
        .into_inner();
    let response = reads
        .message()
        .await
        .expect("typed read should stream")
        .expect("typed read should be present");
    let GrpcBatchGetResult::Found(document) = response
        .result
        .expect("typed batch get result should exist")
    else {
        panic!("typed document should be found")
    };

    assert!(matches!(
        document.fields["createdAt"].value_type,
        Some(GrpcValueType::TimestampValue(ProstTimestamp {
            seconds: 1_704_164_645,
            nanos: 123_456_789,
        }))
    ));
    assert!(matches!(
        &document.fields["payload"].value_type,
        Some(GrpcValueType::BytesValue(value)) if value == &[1, 2, 3, 4]
    ));
    assert!(matches!(
        &document.fields["owner"].value_type,
        Some(GrpcValueType::ReferenceValue(value))
            if value == "projects/demo/databases/(default)/documents/users/ada"
    ));
    assert!(matches!(
        document.fields["location"].value_type,
        Some(GrpcValueType::GeoPointValue(GrpcLatLng { latitude, longitude }))
            if latitude == 37.7749 && longitude == -122.4194
    ));
    assert!(matches!(
        document.fields["score"].value_type,
        Some(GrpcValueType::DoubleValue(value))
            if value.is_infinite() && value.is_sign_negative()
    ));
    let Some(GrpcValueType::MapValue(nested)) = &document.fields["nested"].value_type else {
        panic!("nested typed map should roundtrip")
    };
    assert!(matches!(
        &nested.fields["attachment"].value_type,
        Some(GrpcValueType::BytesValue(value)) if value == &[0xFF, 0x00]
    ));
    assert!(matches!(
        &nested.fields["label"].value_type,
        Some(GrpcValueType::StringValue(value)) if value == "kept"
    ));
}
#[tokio::test]
async fn firebase_write_stream_rejects_missing_post_handshake_token() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let server = ServerFixture::start(router_for_firebase(
        fixture.engine(),
        firebase_verified_config(),
    ))
    .await;
    assert_firebase_grpc_write_stream_anonymous_refused(
        &server,
        "projects/demo/databases/(default)",
    )
    .await;
    let mut client = firestore_grpc_authed_client(&server, "user-123", "demo").await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .write(receiver)
        .await
        .expect("Firestore write stream should open")
        .into_inner();

    sender
        .unbounded_send(GrpcWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            ..Default::default()
        })
        .expect("handshake request should send");
    let _handshake = responses
        .message()
        .await
        .expect("handshake response should stream")
        .expect("handshake response should be present");

    sender
        .unbounded_send(GrpcWriteRequest {
            writes: vec![grpc_update_write(
                "projects/demo/databases/(default)/documents/cities/SF",
                [("name", grpc_string_value("San Francisco"))],
            )],
            ..Default::default()
        })
        .expect("invalid write request should send");
    let error = responses
        .message()
        .await
        .expect_err("missing stream token should terminate the stream");
    assert_eq!(error.code(), Code::InvalidArgument);
}

#[tokio::test]
async fn firebase_write_stream_replays_unacknowledged_responses_on_resume() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let server = ServerFixture::start(router_for_firebase(
        fixture.engine(),
        firebase_verified_config(),
    ))
    .await;

    assert_firebase_grpc_write_stream_anonymous_refused(
        &server,
        "projects/demo/databases/(default)",
    )
    .await;
    let mut initial_client = firestore_grpc_authed_client(&server, "user-123", "demo").await;
    let (initial_sender, initial_receiver) = mpsc::unbounded();
    let mut initial_responses = initial_client
        .write(initial_receiver)
        .await
        .expect("initial write stream should open")
        .into_inner();

    initial_sender
        .unbounded_send(GrpcWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            ..Default::default()
        })
        .expect("initial handshake request should send");
    let handshake = initial_responses
        .message()
        .await
        .expect("initial handshake response should stream")
        .expect("initial handshake response should be present");

    initial_sender
        .unbounded_send(GrpcWriteRequest {
            stream_token: handshake.stream_token.clone(),
            writes: vec![grpc_update_write(
                "projects/demo/databases/(default)/documents/cities/SF",
                [("name", grpc_string_value("San Francisco"))],
            )],
            ..Default::default()
        })
        .expect("initial write request should send");
    let write_response = initial_responses
        .message()
        .await
        .expect("initial write response should stream")
        .expect("initial write response should be present");
    drop(initial_sender);
    assert!(
        initial_responses
            .message()
            .await
            .expect("initial stream should close cleanly")
            .is_none(),
        "initial write stream should end after the sender closes"
    );

    let mut resumed_client = firestore_grpc_authed_client(&server, "user-123", "demo").await;
    let (resume_sender, resume_receiver) = mpsc::unbounded();
    let mut resumed_responses = resumed_client
        .write(resume_receiver)
        .await
        .expect("resumed write stream should open")
        .into_inner();
    resume_sender
        .unbounded_send(GrpcWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            stream_id: handshake.stream_id.clone(),
            stream_token: handshake.stream_token.clone(),
            ..Default::default()
        })
        .expect("resume request should send");

    let replayed = resumed_responses
        .message()
        .await
        .expect("replayed response should stream")
        .expect("replayed response should be present");
    assert_eq!(
        replayed.write_results.len(),
        1,
        "resume should replay the unacknowledged write response"
    );
    assert_eq!(replayed.stream_token, write_response.stream_token);

    let current = resumed_responses
        .message()
        .await
        .expect("resume token marker should stream")
        .expect("resume token marker should be present");
    assert!(
        current.write_results.is_empty(),
        "final resume marker should only carry the current token"
    );
    assert_eq!(current.stream_token, write_response.stream_token);

    drop(resume_sender);
    assert!(
        resumed_responses
            .message()
            .await
            .expect("resumed stream should close cleanly")
            .is_none(),
        "resumed write stream should end after the sender closes"
    );
}

#[tokio::test]
async fn firebase_write_stream_executes_transform_only_writes_and_returns_transform_results() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    assert_firebase_grpc_write_stream_anonymous_refused(
        &server,
        "projects/demo/databases/(default)",
    )
    .await;
    let mut client = firestore_grpc_authed_client(&server, "user-123", "demo").await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .write(receiver)
        .await
        .expect("Firestore write stream should open")
        .into_inner();

    sender
        .unbounded_send(GrpcWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            ..Default::default()
        })
        .expect("handshake request should send");
    let handshake = responses
        .message()
        .await
        .expect("handshake response should stream")
        .expect("handshake response should be present");

    sender
        .unbounded_send(GrpcWriteRequest {
            stream_token: handshake.stream_token.clone(),
            writes: vec![grpc_transform_write(
                "projects/demo/databases/(default)/documents/cities/SF",
                vec![
                    grpc_increment_transform("count", grpc_integer_value(1)),
                    grpc_append_missing_elements_transform(
                        "tags",
                        [grpc_string_value("seed"), grpc_string_value("seed")],
                    ),
                ],
            )],
            ..Default::default()
        })
        .expect("transform write request should send");
    let response = responses
        .message()
        .await
        .expect("transform write response should stream")
        .expect("transform write response should be present");

    assert_eq!(response.write_results.len(), 1);
    assert_eq!(response.write_results[0].transform_results.len(), 2);
    assert!(matches!(
        response.write_results[0].transform_results[0].value_type,
        Some(GrpcValueType::IntegerValue(1))
    ));
    assert!(matches!(
        response.write_results[0].transform_results[1].value_type,
        Some(GrpcValueType::NullValue(_))
    ));

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let document = service
        .get_document(&tenant_id, &locator.table, locator.id)
        .expect("transform write should create the document");
    assert_eq!(document.get_field("count"), Some(&json!(1)));
    assert_eq!(document.get_field("tags"), Some(&json!(["seed"])));
}

#[tokio::test]
async fn firebase_write_stream_roundtrips_server_timestamp_transform_results_and_reads() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    assert_firebase_grpc_write_stream_anonymous_refused(
        &server,
        "projects/demo/databases/(default)",
    )
    .await;
    let mut client = firestore_grpc_authed_client(&server, "user-123", "demo").await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .write(receiver)
        .await
        .expect("Firestore write stream should open")
        .into_inner();

    sender
        .unbounded_send(GrpcWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            ..Default::default()
        })
        .expect("handshake request should send");
    let handshake = responses
        .message()
        .await
        .expect("handshake response should stream")
        .expect("handshake response should be present");

    sender
        .unbounded_send(GrpcWriteRequest {
            stream_token: handshake.stream_token.clone(),
            writes: vec![
                grpc_update_write(
                    "projects/demo/databases/(default)/documents/cities/SF",
                    [("name", grpc_string_value("San Francisco"))],
                ),
                grpc_transform_write(
                    "projects/demo/databases/(default)/documents/cities/SF",
                    vec![grpc_server_timestamp_transform("updatedAt")],
                ),
            ],
            ..Default::default()
        })
        .expect("transform write request should send");
    let response = responses
        .message()
        .await
        .expect("transform write response should stream")
        .expect("transform write response should be present");
    assert_eq!(response.write_results.len(), 2);
    assert!(matches!(
        response.write_results[1].transform_results[0].value_type,
        Some(GrpcValueType::TimestampValue(_))
    ));

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let document = service
        .get_document(&tenant_id, &locator.table, locator.id)
        .expect("server timestamp write should commit");
    assert_eq!(document.get_field("name"), Some(&json!("San Francisco")));
    assert!(matches!(
        document.typed_field("updatedAt"),
        Some(TypedScalarValue::Timestamp { .. })
    ));

    let fetched = client
        .get_document(GrpcGetDocumentRequest {
            name: "projects/demo/databases/(default)/documents/cities/SF".to_string(),
            ..Default::default()
        })
        .await
        .expect("gRPC get_document should succeed")
        .into_inner();
    assert!(matches!(
        fetched.fields["updatedAt"].value_type,
        Some(GrpcValueType::TimestampValue(_))
    ));
}

#[tokio::test]
async fn firebase_write_stream_roundtrips_special_double_transform_results_and_reads() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    assert_firebase_grpc_write_stream_anonymous_refused(
        &server,
        "projects/demo/databases/(default)",
    )
    .await;
    let mut client = firestore_grpc_authed_client(&server, "user-123", "demo").await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .write(receiver)
        .await
        .expect("Firestore write stream should open")
        .into_inner();

    sender
        .unbounded_send(GrpcWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            ..Default::default()
        })
        .expect("handshake request should send");
    let handshake = responses
        .message()
        .await
        .expect("handshake response should stream")
        .expect("handshake response should be present");

    sender
        .unbounded_send(GrpcWriteRequest {
            stream_token: handshake.stream_token.clone(),
            writes: vec![grpc_transform_write(
                "projects/demo/databases/(default)/documents/cities/LA",
                vec![grpc_maximum_transform(
                    "ceiling",
                    grpc_double_value(f64::INFINITY),
                )],
            )],
            ..Default::default()
        })
        .expect("transform write request should send");
    let response = responses
        .message()
        .await
        .expect("transform write response should stream")
        .expect("transform write response should be present");
    assert_eq!(response.write_results.len(), 1);
    assert!(matches!(
        response.write_results[0].transform_results[0].value_type,
        Some(GrpcValueType::DoubleValue(value)) if value.is_infinite() && value.is_sign_positive()
    ));

    let fetched = client
        .get_document(GrpcGetDocumentRequest {
            name: "projects/demo/databases/(default)/documents/cities/LA".to_string(),
            ..Default::default()
        })
        .await
        .expect("gRPC get_document should succeed")
        .into_inner();
    assert!(matches!(
        fetched.fields["ceiling"].value_type,
        Some(GrpcValueType::DoubleValue(value)) if value.is_infinite() && value.is_sign_positive()
    ));
}

#[tokio::test]
async fn firebase_commit_roundtrips_typed_scalar_transform_results_and_document_reads() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let client = firebase_rest_client("user-123", "demo");

    assert_firebase_rest_anonymous_refused(
        &server,
        "/v1/projects/demo/databases/(default)/documents:commit",
        "{}",
    )
    .await;

    let response = client
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [
                    {
                        "update": {
                            "name": "projects/demo/databases/(default)/documents/cities/SF",
                            "fields": {
                                "name": { "stringValue": "San Francisco" }
                            }
                        },
                        "updateTransforms": [
                            {
                                "fieldPath": "updatedAt",
                                "setToServerValue": "REQUEST_TIME"
                            },
                            {
                                "fieldPath": "ceiling",
                                "maximum": { "doubleValue": "Infinity" }
                            }
                        ]
                    }
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase commit should send");

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("firebase commit response should deserialize");
    assert!(
        body["writeResults"][0]["transformResults"][0]["timestampValue"]
            .as_str()
            .is_some()
    );
    assert_eq!(
        body["writeResults"][0]["transformResults"][1],
        json!({ "doubleValue": "Infinity" })
    );

    let get_response = client
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "documents": [
                    "projects/demo/databases/(default)/documents/cities/SF"
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase batch get should send");
    assert_eq!(get_response.status(), StatusCode::OK);
    let document_body = response_json_lines(get_response)
        .await
        .into_iter()
        .next()
        .expect("firebase batch get should return one document");
    assert!(
        document_body["found"]["fields"]["updatedAt"]["timestampValue"]
            .as_str()
            .is_some()
    );
    assert_eq!(
        document_body["found"]["fields"]["ceiling"],
        json!({ "doubleValue": "Infinity" })
    );

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let stored = service
        .get_document(&tenant_id, &locator.table, locator.id)
        .expect("committed document should exist");
    assert!(matches!(
        stored.typed_field("updatedAt"),
        Some(TypedScalarValue::Timestamp { .. })
    ));
    assert_eq!(
        stored.typed_field("ceiling"),
        Some(&TypedScalarValue::SpecialDouble {
            value: SpecialDouble::PositiveInfinity,
        })
    );
}

#[tokio::test]
async fn firebase_write_stream_closes_cleanly_after_handshake_when_sender_drops() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let server = ServerFixture::start(router_for_firebase(
        fixture.engine(),
        firebase_verified_config(),
    ))
    .await;
    assert_firebase_grpc_write_stream_anonymous_refused(
        &server,
        "projects/demo/databases/(default)",
    )
    .await;
    let mut client = firestore_grpc_authed_client(&server, "user-123", "demo").await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .write(receiver)
        .await
        .expect("Firestore write stream should open")
        .into_inner();

    sender
        .unbounded_send(GrpcWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            ..Default::default()
        })
        .expect("handshake request should send");
    let _handshake = responses
        .message()
        .await
        .expect("handshake response should stream")
        .expect("handshake response should be present");

    drop(sender);
    assert!(
        responses
            .message()
            .await
            .expect("closed write stream should not error")
            .is_none(),
        "write stream should terminate cleanly once the client half-closes it"
    );
}

#[tokio::test]
async fn firebase_write_stream_array_transforms_roundtrip_typed_values() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let server = ServerFixture::start(router_for_firebase(
        fixture.engine(),
        firebase_verified_config(),
    ))
    .await;
    let mut client = firestore_grpc_authed_client(&server, "user-123", "demo").await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .write(receiver)
        .await
        .expect("Firestore write stream should open")
        .into_inner();

    sender
        .unbounded_send(GrpcWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            ..Default::default()
        })
        .expect("handshake request should send");
    let handshake = responses
        .message()
        .await
        .expect("handshake should stream")
        .expect("handshake should be present");
    let document_name = "projects/demo/databases/(default)/documents/events/arrays-grpc";

    sender
        .unbounded_send(GrpcWriteRequest {
            stream_token: handshake.stream_token.clone(),
            writes: vec![grpc_transform_write(
                document_name,
                [grpc_append_missing_elements_transform(
                    "tags",
                    [
                        grpc_string_value("seed"),
                        grpc_timestamp_value(1_704_164_645, 123_456_789),
                        grpc_bytes_value([1, 2, 3, 4]),
                        grpc_reference_value(
                            "projects/demo/databases/(default)/documents/users/ada",
                        ),
                        grpc_geo_point_value(37.7749, -122.4194),
                    ],
                )],
            )],
            ..Default::default()
        })
        .expect("typed arrayUnion request should send");
    let append = responses
        .message()
        .await
        .expect("typed arrayUnion response should stream")
        .expect("typed arrayUnion response should be present");

    sender
        .unbounded_send(GrpcWriteRequest {
            stream_token: append.stream_token,
            writes: vec![grpc_transform_write(
                document_name,
                [grpc_remove_all_from_array_transform(
                    "tags",
                    [
                        grpc_timestamp_value(1_704_164_645, 123_456_789),
                        grpc_geo_point_value(37.7749, -122.4194),
                    ],
                )],
            )],
            ..Default::default()
        })
        .expect("typed arrayRemove request should send");
    responses
        .message()
        .await
        .expect("typed arrayRemove response should stream")
        .expect("typed arrayRemove response should be present");
    drop(sender);
    drop(responses);

    let mut reads = client
        .batch_get_documents(grpc_batch_get_request([document_name]))
        .await
        .expect("typed array batch get should succeed")
        .into_inner();
    let response = reads
        .message()
        .await
        .expect("typed array read should stream")
        .expect("typed array read should be present");
    let GrpcBatchGetResult::Found(document) = response
        .result
        .expect("typed array batch get result should exist")
    else {
        panic!("typed array document should be found")
    };

    let Some(GrpcValueType::ArrayValue(tags)) = &document.fields["tags"].value_type else {
        panic!("tags should read back as an array value")
    };
    assert_eq!(
        tags.values.len(),
        3,
        "arrayRemove should drop exactly the two typed elements it named"
    );
    assert!(matches!(
        &tags.values[0].value_type,
        Some(GrpcValueType::StringValue(value)) if value == "seed"
    ));
    assert!(
        matches!(
            &tags.values[1].value_type,
            Some(GrpcValueType::BytesValue(value)) if value == &[1, 2, 3, 4]
        ),
        "array elements must survive as bytes, not as their base64 projection"
    );
    assert!(matches!(
        &tags.values[2].value_type,
        Some(GrpcValueType::ReferenceValue(value))
            if value == "projects/demo/databases/(default)/documents/users/ada"
    ));
}
