use super::*;

#[tokio::test]
async fn firebase_mock_user_token_requires_explicit_server_opt_in() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let request_body = json!({
        "database": "projects/demo/databases/(default)",
        "writes": [
            {
                "update": {
                    "name": "projects/demo/databases/(default)/documents/cities/SF",
                    "fields": {
                        "name": { "stringValue": "San Francisco" }
                    }
                }
            }
        ]
    })
    .to_string();
    let mock_user_token = json!({
        "sub": "mock-user-123"
    })
    .to_string();

    let without_opt_in = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let rejected = without_opt_in
        .client()
        .post(without_opt_in.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::AUTHORIZATION, format!("Bearer {mock_user_token}"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(request_body.clone())
        .send()
        .await
        .expect("ungated mock-user firebase request should send");
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);

    let with_opt_in = ServerFixture::start(build_router_with_firebase(
        service,
        FirebaseConfig::new().with_emulator_mock_user_token_auth(),
    ))
    .await;
    let accepted = with_opt_in
        .client()
        .post(with_opt_in.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::AUTHORIZATION, format!("Bearer {mock_user_token}"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(request_body)
        .send()
        .await
        .expect("gated mock-user firebase request should send");
    assert_eq!(accepted.status(), StatusCode::OK);
}

#[tokio::test]
async fn firebase_rest_commit_and_batch_get_respect_bearer_principal() {
    let _guard = auth::auth_test_guard().await;
    let issuer = "https://firebase-auth.example.com";
    let application_id = "neovex-firebase-test";
    let (token, jwks_data_url) =
        auth::issue_es256_test_token(issuer, application_id, "user-123", json!({}));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    service
        .set_table_schema(
            &tenant_id,
            firebase_owner_schema_for_collection(
                "secureCities",
                firebase_owner_read_write_policy(),
            ),
        )
        .expect("secureCities schema should install");
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([]),
        json!([]),
        None,
        Some(firebase_test_auth_config(
            issuer,
            application_id,
            &jwks_data_url,
        )),
    );
    let server = ServerFixture::start(
        RouterBuildConfig::core(service.clone())
            .with_convex(registry)
            .with_firebase(FirebaseConfig::new())
            .build(),
    )
    .await;

    let commit_response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [
                    {
                        "update": {
                            "name": "projects/demo/databases/(default)/documents/secureCities/SF",
                            "fields": {
                                "owner": { "stringValue": "user-123" },
                                "body": { "stringValue": "authenticated write" }
                            }
                        }
                    }
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("authenticated firebase commit should send");
    assert_eq!(commit_response.status(), StatusCode::OK);

    let anonymous_batch_get = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "documents": [
                    "projects/demo/databases/(default)/documents/secureCities/SF"
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("anonymous firebase batchGet should send");
    assert_eq!(anonymous_batch_get.status(), StatusCode::OK);
    let anonymous_entries = response_json_lines(anonymous_batch_get).await;
    assert_eq!(anonymous_entries.len(), 1);
    assert_eq!(
        anonymous_entries[0]["missing"],
        json!("projects/demo/databases/(default)/documents/secureCities/SF")
    );

    let authenticated_batch_get = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "documents": [
                    "projects/demo/databases/(default)/documents/secureCities/SF"
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("authenticated firebase batchGet should send");
    assert_eq!(authenticated_batch_get.status(), StatusCode::OK);
    let authenticated_entries = response_json_lines(authenticated_batch_get).await;
    assert_eq!(authenticated_entries.len(), 1);
    assert_eq!(
        authenticated_entries[0]["found"]["fields"]["body"],
        json!({ "stringValue": "authenticated write" })
    );
}
#[tokio::test]
async fn firebase_grpc_get_document_respects_bearer_principal() {
    let _guard = auth::auth_test_guard().await;
    let issuer = "https://firebase-auth.example.com";
    let application_id = "neovex-firebase-test";
    let (token, jwks_data_url) =
        auth::issue_es256_test_token(issuer, application_id, "user-123", json!({}));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    service
        .set_table_schema(
            &tenant_id,
            firebase_owner_schema_for_collection(
                "secureGrpcReads",
                firebase_owner_read_only_policy(),
            ),
        )
        .expect("secureGrpcReads schema should install");
    seed_firebase_document(
        &service,
        &tenant_id,
        &["secureGrpcReads", "SF"],
        [
            ("owner", json!("user-123")),
            ("name", json!("San Francisco")),
        ],
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([]),
        json!([]),
        None,
        Some(firebase_test_auth_config(
            issuer,
            application_id,
            &jwks_data_url,
        )),
    );
    let server = ServerFixture::start(
        RouterBuildConfig::core(service.clone())
            .with_convex(registry)
            .with_firebase(FirebaseConfig::new())
            .build(),
    )
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let mut authenticated_request = tonic::Request::new(GrpcGetDocumentRequest {
        name: "projects/demo/databases/(default)/documents/secureGrpcReads/SF".to_string(),
        mask: None,
        consistency_selector: None,
    });
    authenticated_request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}"))
            .expect("grpc authorization metadata should build"),
    );
    let authenticated = client
        .get_document(authenticated_request)
        .await
        .expect("authenticated gRPC GetDocument should succeed")
        .into_inner();
    assert_eq!(
        authenticated.fields["name"],
        grpc_string_value("San Francisco")
    );

    let anonymous_error = client
        .get_document(GrpcGetDocumentRequest {
            name: "projects/demo/databases/(default)/documents/secureGrpcReads/SF".to_string(),
            mask: None,
            consistency_selector: None,
        })
        .await
        .expect_err("anonymous gRPC GetDocument should be filtered");
    assert_eq!(anonymous_error.code(), Code::NotFound);
}

#[tokio::test]
async fn firebase_grpc_write_stream_respects_bearer_principal() {
    let _guard = auth::auth_test_guard().await;
    let issuer = "https://firebase-auth.example.com";
    let application_id = "neovex-firebase-test";
    let (token, jwks_data_url) =
        auth::issue_es256_test_token(issuer, application_id, "user-123", json!({}));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    service
        .set_table_schema(
            &tenant_id,
            firebase_owner_schema_for_collection(
                "secureWriteStream",
                firebase_owner_read_write_policy(),
            ),
        )
        .expect("secureWriteStream schema should install");
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([]),
        json!([]),
        None,
        Some(firebase_test_auth_config(
            issuer,
            application_id,
            &jwks_data_url,
        )),
    );
    let server = ServerFixture::start(
        RouterBuildConfig::core(service.clone())
            .with_convex(registry)
            .with_firebase(FirebaseConfig::new())
            .build(),
    )
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let (auth_sender, auth_receiver) = mpsc::unbounded();
    let mut auth_request = tonic::Request::new(auth_receiver);
    auth_request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}"))
            .expect("grpc authorization metadata should build"),
    );
    let mut auth_responses = client
        .write(auth_request)
        .await
        .expect("authenticated Firestore write stream should open")
        .into_inner();
    auth_sender
        .unbounded_send(GrpcWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            ..Default::default()
        })
        .expect("authenticated write handshake should send");
    let auth_handshake = auth_responses
        .message()
        .await
        .expect("authenticated handshake should stream")
        .expect("authenticated handshake should be present");
    auth_sender
        .unbounded_send(GrpcWriteRequest {
            stream_token: auth_handshake.stream_token.clone(),
            writes: vec![grpc_update_write(
                "projects/demo/databases/(default)/documents/secureWriteStream/SF",
                [
                    ("owner", grpc_string_value("user-123")),
                    ("name", grpc_string_value("San Francisco")),
                ],
            )],
            ..Default::default()
        })
        .expect("authenticated write request should send");
    let write_response = auth_responses
        .message()
        .await
        .expect("authenticated write response should stream")
        .expect("authenticated write response should be present");
    assert_eq!(write_response.write_results.len(), 1);

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["secureWriteStream", "SF"])
            .expect("secureWriteStream document path should parse"),
    )
    .expect("firebase locator should derive");
    let stored = service
        .get_document_with_principal(
            &tenant_id,
            &locator.table,
            locator.id,
            &PrincipalContext {
                authenticated: true,
                claims: serde_json::Map::from_iter([
                    ("subject".to_string(), json!("user-123")),
                    ("sub".to_string(), json!("user-123")),
                ]),
                verified_claims: serde_json::Map::new(),
            },
        )
        .expect("authenticated write should persist a document");
    assert_eq!(stored.get_field("owner"), Some(&json!("user-123")));

    let (anonymous_sender, anonymous_receiver) = mpsc::unbounded();
    let mut anonymous_responses = client
        .write(anonymous_receiver)
        .await
        .expect("anonymous Firestore write stream should open")
        .into_inner();
    anonymous_sender
        .unbounded_send(GrpcWriteRequest {
            database: "projects/demo/databases/(default)".to_string(),
            ..Default::default()
        })
        .expect("anonymous write handshake should send");
    let anonymous_handshake = anonymous_responses
        .message()
        .await
        .expect("anonymous handshake should stream")
        .expect("anonymous handshake should be present");
    anonymous_sender
        .unbounded_send(GrpcWriteRequest {
            stream_token: anonymous_handshake.stream_token.clone(),
            writes: vec![grpc_update_write(
                "projects/demo/databases/(default)/documents/secureWriteStream/denied",
                [
                    ("owner", grpc_string_value("user-123")),
                    ("name", grpc_string_value("Denied")),
                ],
            )],
            ..Default::default()
        })
        .expect("anonymous write request should send");
    let anonymous_error = anonymous_responses
        .message()
        .await
        .expect_err("anonymous write response should fail");
    assert_eq!(anonymous_error.code(), Code::PermissionDenied);
}

#[tokio::test]
async fn firebase_listen_websocket_auth_offer_controls_bootstrap_visibility() {
    let _guard = auth::auth_test_guard().await;
    let issuer = "https://firebase-auth.example.com";
    let application_id = "neovex-firebase-test";
    let (token, jwks_data_url) =
        auth::issue_es256_test_token(issuer, application_id, "user-123", json!({}));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    service
        .set_table_schema(
            &tenant_id,
            firebase_owner_schema_for_collection("secureListen", firebase_owner_read_only_policy()),
        )
        .expect("secureListen schema should install");
    seed_firebase_document(
        &service,
        &tenant_id,
        &["secureListen", "mine"],
        [("owner", json!("user-123")), ("name", json!("Visible"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["secureListen", "theirs"],
        [("owner", json!("user-999")), ("name", json!("Hidden"))],
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([]),
        json!([]),
        None,
        Some(firebase_test_auth_config(
            issuer,
            application_id,
            &jwks_data_url,
        )),
    );
    let server = ServerFixture::start(
        RouterBuildConfig::core(service.clone())
            .with_convex(registry)
            .with_firebase(FirebaseConfig::new())
            .build(),
    )
    .await;

    let encoded_token = URL_SAFE_NO_PAD.encode(token.as_bytes());
    let mut authenticated_request = server
        .ws_url("/google.firestore.v1.Firestore/Listen")
        .into_client_request()
        .expect("authenticated browser websocket request should build");
    authenticated_request.headers_mut().insert(
        header::ORIGIN,
        axum::http::HeaderValue::from_static("http://localhost:5173"),
    );
    authenticated_request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        axum::http::HeaderValue::from_str(&format!(
            "neovex.firebase.listen.v1,neovex.firebase.auth.{encoded_token}"
        ))
        .expect("listen auth subprotocol header should build"),
    );
    let mut authenticated_socket = WebSocketFixture::connect_request(authenticated_request)
        .await
        .expect("authenticated websocket should connect");
    authenticated_socket
        .send_binary(
            grpc_listen_query_request(
                17,
                "projects/demo/databases/(default)/documents",
                "secureListen",
            )
            .encode_to_vec(),
        )
        .await;
    let (_auth_target_changes, auth_document_changes) =
        collect_listen_websocket_bootstrap(&mut authenticated_socket).await;
    assert_eq!(auth_document_changes.len(), 1);
    assert_eq!(
        auth_document_changes[0]
            .document
            .as_ref()
            .expect("authenticated listen bootstrap should include a document")
            .name,
        "projects/demo/databases/(default)/documents/secureListen/mine"
    );

    let mut anonymous_request = server
        .ws_url("/google.firestore.v1.Firestore/Listen")
        .into_client_request()
        .expect("anonymous browser websocket request should build");
    anonymous_request.headers_mut().insert(
        header::ORIGIN,
        axum::http::HeaderValue::from_static("http://localhost:5173"),
    );
    anonymous_request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        axum::http::HeaderValue::from_static("neovex.firebase.listen.v1"),
    );
    let mut anonymous_socket = WebSocketFixture::connect_request(anonymous_request)
        .await
        .expect("anonymous websocket should connect");
    anonymous_socket
        .send_binary(
            grpc_listen_query_request(
                18,
                "projects/demo/databases/(default)/documents",
                "secureListen",
            )
            .encode_to_vec(),
        )
        .await;
    let (_anonymous_target_changes, anonymous_document_changes) =
        collect_listen_websocket_bootstrap(&mut anonymous_socket).await;
    assert!(
        anonymous_document_changes.is_empty(),
        "anonymous websocket bootstrap should not expose protected documents: {anonymous_document_changes:?}"
    );
}

#[tokio::test]
async fn firebase_listen_websocket_mock_user_token_requires_explicit_server_opt_in() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    service
        .set_table_schema(
            &tenant_id,
            firebase_owner_schema_for_collection(
                "mockTokenListen",
                firebase_owner_read_only_policy(),
            ),
        )
        .expect("mockTokenListen schema should install");
    seed_firebase_document(
        &service,
        &tenant_id,
        &["mockTokenListen", "mine"],
        [
            ("owner", json!("mock-user-123")),
            ("name", json!("Visible")),
        ],
    );

    let mock_user_token = json!({
        "sub": "mock-user-123"
    })
    .to_string();
    let encoded_token = URL_SAFE_NO_PAD.encode(mock_user_token.as_bytes());

    let without_opt_in = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let mut rejected_request = without_opt_in
        .ws_url("/google.firestore.v1.Firestore/Listen")
        .into_client_request()
        .expect("ungated websocket request should build");
    rejected_request.headers_mut().insert(
        header::ORIGIN,
        axum::http::HeaderValue::from_static("http://localhost:5173"),
    );
    rejected_request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        axum::http::HeaderValue::from_str(&format!(
            "neovex.firebase.listen.v1,neovex.firebase.auth.{encoded_token}"
        ))
        .expect("listen auth subprotocol header should build"),
    );
    let mut rejected_socket = WebSocketFixture::connect_request(rejected_request)
        .await
        .expect("ungated websocket handshake should still complete");
    let close_code = websocket_close_code(rejected_socket.next_message().await);
    assert_eq!(close_code, WsCloseCode::Policy);

    let with_opt_in = ServerFixture::start(build_router_with_firebase(
        service,
        FirebaseConfig::new().with_emulator_mock_user_token_auth(),
    ))
    .await;
    let mut accepted_request = with_opt_in
        .ws_url("/google.firestore.v1.Firestore/Listen")
        .into_client_request()
        .expect("gated websocket request should build");
    accepted_request.headers_mut().insert(
        header::ORIGIN,
        axum::http::HeaderValue::from_static("http://localhost:5173"),
    );
    accepted_request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        axum::http::HeaderValue::from_str(&format!(
            "neovex.firebase.listen.v1,neovex.firebase.auth.{encoded_token}"
        ))
        .expect("listen auth subprotocol header should build"),
    );
    let mut accepted_socket = WebSocketFixture::connect_request(accepted_request)
        .await
        .expect("gated websocket mock-user auth should connect");
    accepted_socket
        .send_binary(
            grpc_listen_query_request(
                23,
                "projects/demo/databases/(default)/documents",
                "mockTokenListen",
            )
            .encode_to_vec(),
        )
        .await;
    let (_target_changes, document_changes) =
        collect_listen_websocket_bootstrap(&mut accepted_socket).await;
    assert_eq!(document_changes.len(), 1);
    assert_eq!(
        document_changes[0]
            .document
            .as_ref()
            .expect("gated websocket bootstrap should include a document")
            .name,
        "projects/demo/databases/(default)/documents/mockTokenListen/mine"
    );
}

#[tokio::test]
async fn firebase_rest_routes_return_not_found_when_adapter_is_disabled() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;

    for path in [
        "/v1/projects/demo/databases/(default)/documents:commit",
        "/v1/projects/demo/databases/(default)/documents:batchGet",
        "/v1/projects/demo/databases/(default)/documents:runQuery",
        "/v1/projects/demo/databases/(default)/documents/cities/SF:runQuery",
    ] {
        let response = server
            .client()
            .post(server.http_url(path))
            .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
            .body("{}")
            .send()
            .await
            .expect("disabled firebase request should send");
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "disabled firebase route should 404 for {path}"
        );
    }
}

#[tokio::test]
async fn firebase_rest_routes_are_registered_when_adapter_is_enabled() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;

    for path in [
        "/v1/projects/demo/databases/(default)/documents:commit",
        "/v1/projects/demo/databases/(default)/documents:batchGet",
        "/v1/projects/demo/databases/(default)/documents:runQuery",
        "/v1/projects/demo/databases/(default)/documents/cities/SF:runQuery",
    ] {
        let body = if path.ends_with(":commit") {
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [
                    {
                        "update": {
                            "name": "projects/demo/databases/(default)/documents/cities/SF",
                            "fields": {
                                "name": { "stringValue": "San Francisco" }
                            }
                        }
                    }
                ]
            })
            .to_string()
        } else if path.ends_with(":batchGet") {
            json!({
                "documents": [
                    "projects/demo/databases/(default)/documents/cities/SF"
                ]
            })
            .to_string()
        } else if path.ends_with("cities/SF:runQuery") {
            json!({
                "structuredQuery": {
                    "from": [{ "collectionId": "landmarks" }],
                    "limit": 1
                }
            })
            .to_string()
        } else if path.ends_with(":runQuery") {
            json!({
                "structuredQuery": {
                    "from": [{ "collectionId": "cities" }],
                    "limit": 1
                }
            })
            .to_string()
        } else {
            "{}".to_string()
        };
        let response = server
            .client()
            .post(server.http_url(path))
            .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
            .body(body)
            .send()
            .await
            .expect("enabled firebase request should send");
        let expected = StatusCode::OK;
        assert_eq!(response.status(), expected, "unexpected status for {path}");
    }
}

#[tokio::test]
async fn firebase_commit_rejects_malformed_commit_json() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body("{}")
        .send()
        .await
        .expect("malformed firebase commit request should send");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
