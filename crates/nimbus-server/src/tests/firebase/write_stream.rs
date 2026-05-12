use super::*;

#[tokio::test]
async fn firebase_write_stream_handshakes_and_applies_ordered_writes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;
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
async fn firebase_write_stream_rejects_missing_post_handshake_token() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;

    let mut initial_client = firestore_grpc_client(&server).await;
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

    let mut resumed_client = firestore_grpc_client(&server).await;
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let response = server
        .client()
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

    let get_response = server
        .client()
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;
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
