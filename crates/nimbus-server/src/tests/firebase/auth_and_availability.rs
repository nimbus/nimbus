use super::*;

#[tokio::test]
async fn firebase_emulator_token_verification_bypass_requires_explicit_server_opt_in() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
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
    // A dev-mode emulator token carrying the Firebase project issuer. Without the
    // opt-in it is treated as an ordinary (unverifiable) bearer; with the opt-in
    // the bypass fabricates a verified principal from it.
    let bypass_token = firebase_verified_token("mock-user-123", "demo");

    // Without the opt-in the bypass is off: the JSON token is just a bearer with
    // no configured verifier, so the route-layer auth middleware refuses it
    // (401) before any handler runs.
    let without_opt_in =
        ServerFixture::start(router_for_firebase(service.clone(), FirebaseConfig::new())).await;
    let rejected = without_opt_in
        .client()
        .post(without_opt_in.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::AUTHORIZATION, format!("Bearer {bypass_token}"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(request_body.clone())
        .send()
        .await
        .expect("ungated bypass firebase request should send");
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);

    let with_opt_in =
        ServerFixture::start(router_for_firebase(service, firebase_verified_config())).await;

    // Even with the opt-in, an anonymous request (no bearer, hence no verified
    // project) is refused by the #24 verified-project gate.
    assert_firebase_rest_anonymous_refused(
        &with_opt_in,
        "/v1/projects/demo/databases/(default)/documents:commit",
        &request_body,
    )
    .await;

    let accepted = with_opt_in
        .client()
        .post(with_opt_in.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::AUTHORIZATION, format!("Bearer {bypass_token}"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(request_body)
        .send()
        .await
        .expect("gated bypass firebase request should send");
    assert_eq!(accepted.status(), StatusCode::OK);
}

/// #24 close (fail-closed default): a deployment that never configured project
/// bindings — `NIMBUS_FIREBASE_PROJECTS` unset, so `FirebaseConfig` carries its
/// default empty *strict* registry — refuses ALL Firestore traffic, even a fully
/// verified token, because every project is unregistered. The most common
/// deployment (operator sets nothing) must refuse, never fall back to permissive.
#[tokio::test]
async fn firebase_unconfigured_project_registry_refuses_all_traffic_even_with_a_verified_token() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();

    // Bypass ON (so the token fabricates a verified project "demo"), but NO
    // project registry installed: the default is the empty strict (refuse-all)
    // registry — the fail-closed state of an unconfigured deployment.
    let config = FirebaseConfig::new().with_emulator_token_verification_bypass();
    assert!(
        config.project_registry().is_strict_empty(),
        "an unconfigured FirebaseConfig must default to the empty strict (refuse-all) registry"
    );
    let server = ServerFixture::start(router_for_firebase(service, config)).await;

    let commit_body = json!({
        "database": "projects/demo/databases/(default)",
        "writes": []
    })
    .to_string();

    // A fully VERIFIED token for project "demo" — the authorized case in every
    // other test — is still REFUSED here, because "demo" is unregistered. This is
    // the close: the empty-strict default refuses on the registry resolution, not
    // on a missing token.
    let refused = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(
            header::AUTHORIZATION,
            firebase_verified_bearer("user-1", "demo"),
        )
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(commit_body.clone())
        .send()
        .await
        .expect("verified firebase commit should send");
    assert_eq!(
        refused.status(),
        StatusCode::FORBIDDEN,
        "an unconfigured (empty-strict) registry must refuse even a verified token"
    );

    // Anonymous is refused too — belt-and-suspenders, keeps the test non-vacuous.
    assert_firebase_rest_anonymous_refused(
        &server,
        "/v1/projects/demo/databases/(default)/documents:commit",
        &commit_body,
    )
    .await;
}

#[tokio::test]
async fn firebase_rest_commit_and_batch_get_respect_bearer_principal() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    service
        .set_table_schema(
            &tenant_id,
            firebase_owner_schema_for_collection(
                "secureCities",
                firebase_owner_read_write_policy(),
            ),
        )
        .expect("secureCities schema should install");
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

    let commit_body = json!({
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
    .to_string();

    // Anonymous (no verified project) is refused outright by the #24 gate.
    assert_firebase_rest_anonymous_refused(
        &server,
        "/v1/projects/demo/databases/(default)/documents:commit",
        &commit_body,
    )
    .await;

    let commit_response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::AUTHORIZATION, &bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(commit_body)
        .send()
        .await
        .expect("authenticated firebase commit should send");
    assert_eq!(commit_response.status(), StatusCode::OK);

    let batch_get_body = json!({
        "documents": [
            "projects/demo/databases/(default)/documents/secureCities/SF"
        ]
    })
    .to_string();

    // Anonymous batchGet is refused at the gate (it can no longer reach the
    // access-policy filter as an unauthenticated caller).
    assert_firebase_rest_anonymous_refused(
        &server,
        "/v1/projects/demo/databases/(default)/documents:batchGet",
        &batch_get_body,
    )
    .await;

    let authenticated_batch_get = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::AUTHORIZATION, &bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(batch_get_body)
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
async fn firebase_rest_batch_get_rejects_bearer_for_different_tenant() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("tenant-a", Engine::create_tenant);
    fixture.create_tenant("tenant-b", Engine::create_tenant);
    let service = fixture.engine();
    let server =
        ServerFixture::start(router_for_firebase(service, firebase_verified_config())).await;
    // A token verified for project (tenant) tenant-b.
    let tenant_b_bearer = firebase_verified_bearer("user-123", "tenant-b");

    let authorized = server
        .client()
        .post(server.http_url("/v1/projects/tenant-b/databases/(default)/documents:batchGet"))
        .header(header::AUTHORIZATION, &tenant_b_bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "documents": [
                    "projects/tenant-b/databases/(default)/documents/cities/SF"
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("same-tenant firebase batchGet should send");
    let authorized_status = authorized.status();
    let authorized_body = authorized
        .text()
        .await
        .expect("same-tenant firebase batchGet body should read");
    assert_eq!(
        authorized_status,
        StatusCode::OK,
        "same-tenant firebase batchGet body: {authorized_body}"
    );

    let rejected = server
        .client()
        .post(server.http_url("/v1/projects/tenant-a/databases/(default)/documents:batchGet"))
        .header(header::AUTHORIZATION, &tenant_b_bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "documents": [
                    "projects/tenant-a/databases/(default)/documents/cities/SF"
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("swapped-tenant firebase batchGet should send");
    let rejected_status = rejected.status();
    let rejected_body = rejected
        .text()
        .await
        .expect("swapped-tenant firebase batchGet body should read");
    assert_eq!(
        rejected_status,
        StatusCode::FORBIDDEN,
        "swapped-tenant firebase batchGet body: {rejected_body}"
    );
    assert!(
        rejected_body.contains("verified Firebase project `tenant-b`"),
        "swapped-tenant Firebase error should name the verified project: {rejected_body}"
    );
    assert!(
        rejected_body.contains("project `tenant-a`"),
        "swapped-tenant Firebase error should name the rejected target project: {rejected_body}"
    );
}

#[tokio::test]
async fn firebase_grpc_get_document_respects_bearer_principal() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
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
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    let document_name = "projects/demo/databases/(default)/documents/secureGrpcReads/SF";

    // Anonymous (no metadata) is refused at the #24 gate.
    let anonymous_error = client
        .get_document(GrpcGetDocumentRequest {
            name: document_name.to_string(),
            mask: None,
            consistency_selector: None,
        })
        .await
        .expect_err("anonymous gRPC GetDocument should be refused");
    assert_eq!(anonymous_error.code(), Code::PermissionDenied);

    let authenticated = client
        .get_document(firebase_grpc_request(
            GrpcGetDocumentRequest {
                name: document_name.to_string(),
                mask: None,
                consistency_selector: None,
            },
            "user-123",
            "demo",
        ))
        .await
        .expect("authenticated gRPC GetDocument should succeed")
        .into_inner();
    assert_eq!(
        authenticated.fields["name"],
        grpc_string_value("San Francisco")
    );
}

#[tokio::test]
async fn firebase_grpc_get_document_rejects_bearer_for_different_tenant() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("tenant-a", Engine::create_tenant);
    let tenant_b = fixture.create_tenant("tenant-b", Engine::create_tenant);
    let service = fixture.engine();
    seed_firebase_document(
        &service,
        &tenant_b,
        &["cities", "SF"],
        [("name", json!("San Francisco"))],
    );
    let server =
        ServerFixture::start(router_for_firebase(service, firebase_verified_config())).await;
    let mut client = firestore_grpc_client(&server).await;

    let authorized = client
        .get_document(firebase_grpc_request(
            GrpcGetDocumentRequest {
                name: "projects/tenant-b/databases/(default)/documents/cities/SF".to_string(),
                mask: None,
                consistency_selector: None,
            },
            "user-123",
            "tenant-b",
        ))
        .await
        .expect("same-tenant gRPC GetDocument should succeed")
        .into_inner();
    assert_eq!(
        authorized.fields["name"],
        grpc_string_value("San Francisco")
    );

    // The same tenant-b token addressing tenant-a is refused by the gate.
    let rejected = client
        .get_document(firebase_grpc_request(
            GrpcGetDocumentRequest {
                name: "projects/tenant-a/databases/(default)/documents/cities/SF".to_string(),
                mask: None,
                consistency_selector: None,
            },
            "user-123",
            "tenant-b",
        ))
        .await
        .expect_err("swapped-tenant gRPC GetDocument should be rejected");
    assert_eq!(rejected.code(), Code::PermissionDenied);
    assert!(
        rejected
            .message()
            .contains("verified Firebase project `tenant-b`"),
        "swapped-tenant gRPC error should name the verified project: {rejected}"
    );
    assert!(
        rejected.message().contains("project `tenant-a`"),
        "swapped-tenant gRPC error should name the rejected target project: {rejected}"
    );
}

#[tokio::test]
async fn firebase_grpc_write_stream_respects_bearer_principal() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    service
        .set_table_schema(
            &tenant_id,
            firebase_owner_schema_for_collection(
                "secureWriteStream",
                firebase_owner_read_write_policy(),
            ),
        )
        .expect("secureWriteStream schema should install");
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let mut client = firestore_grpc_client(&server).await;

    // Anonymous write-stream handshake is refused at the #24 gate.
    assert_firebase_grpc_write_stream_anonymous_refused(
        &server,
        "projects/demo/databases/(default)",
    )
    .await;

    let (auth_sender, auth_receiver) = mpsc::unbounded();
    let mut auth_responses = client
        .write(firebase_grpc_request(auth_receiver, "user-123", "demo"))
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
}

#[tokio::test]
async fn firebase_listen_websocket_auth_offer_controls_bootstrap_visibility() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
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
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;

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
        axum::http::HeaderValue::from_str(&firebase_listen_ws_auth_protocol("user-123", "demo"))
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

    // Anonymous (no auth offer) has no verified project: the #24 gate refuses the
    // add-target and the socket closes with a policy frame.
    assert_firebase_listen_ws_anonymous_refused(
        &server,
        "projects/demo/databases/(default)/documents",
        "secureListen",
    )
    .await;
}

#[tokio::test]
async fn firebase_listen_websocket_rejects_bearer_for_different_tenant() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("tenant-a", Engine::create_tenant);
    let tenant_b = fixture.create_tenant("tenant-b", Engine::create_tenant);
    let service = fixture.engine();
    seed_firebase_document(
        &service,
        &tenant_b,
        &["listenTenantProof", "mine"],
        [("name", json!("Visible"))],
    );
    let server =
        ServerFixture::start(router_for_firebase(service, firebase_verified_config())).await;

    // The auth offer carries a token verified for tenant-b.
    let auth_protocol = firebase_listen_ws_auth_protocol("user-123", "tenant-b");

    let mut authorized_request = server
        .ws_url("/google.firestore.v1.Firestore/Listen")
        .into_client_request()
        .expect("same-tenant listen websocket request should build");
    authorized_request.headers_mut().insert(
        header::ORIGIN,
        axum::http::HeaderValue::from_static("http://localhost:5173"),
    );
    authorized_request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        axum::http::HeaderValue::from_str(&auth_protocol)
            .expect("listen auth subprotocol header should build"),
    );
    let mut authorized_socket = WebSocketFixture::connect_request(authorized_request)
        .await
        .expect("same-tenant listen websocket should connect");
    authorized_socket
        .send_binary(
            firebase_tenant_listen_query_request(
                31,
                "projects/tenant-b/databases/(default)",
                "projects/tenant-b/databases/(default)/documents",
                "listenTenantProof",
            )
            .encode_to_vec(),
        )
        .await;
    let (_authorized_target_changes, authorized_document_changes) =
        collect_listen_websocket_bootstrap(&mut authorized_socket).await;
    assert_eq!(authorized_document_changes.len(), 1);
    assert_eq!(
        authorized_document_changes[0]
            .document
            .as_ref()
            .expect("same-tenant listen bootstrap should include a document")
            .name,
        "projects/tenant-b/databases/(default)/documents/listenTenantProof/mine"
    );

    // The same tenant-b token addressing tenant-a is refused by the gate.
    let mut rejected_request = server
        .ws_url("/google.firestore.v1.Firestore/Listen")
        .into_client_request()
        .expect("swapped-tenant listen websocket request should build");
    rejected_request.headers_mut().insert(
        header::ORIGIN,
        axum::http::HeaderValue::from_static("http://localhost:5173"),
    );
    rejected_request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        axum::http::HeaderValue::from_str(&auth_protocol)
            .expect("listen auth subprotocol header should build"),
    );
    let mut rejected_socket = WebSocketFixture::connect_request(rejected_request)
        .await
        .expect("swapped-tenant listen websocket should connect before target admission");
    rejected_socket
        .send_binary(
            firebase_tenant_listen_query_request(
                32,
                "projects/tenant-a/databases/(default)",
                "projects/tenant-a/databases/(default)/documents",
                "listenTenantProof",
            )
            .encode_to_vec(),
        )
        .await;
    let close = rejected_socket.next_message().await;
    let WsMessage::Close(Some(frame)) = close else {
        panic!("expected swapped-tenant listen to close with a policy frame, got {close:?}");
    };
    assert_eq!(frame.code, WsCloseCode::Policy);
    // The WebSocket close reason is bounded to 123 bytes, so the gate message is
    // middle-truncated; assert the (preserved) verified and rejected tenant ids.
    assert!(
        frame.reason.contains("tenant-b"),
        "swapped-tenant Listen close reason should name the verified tenant: {frame:?}"
    );
    assert!(
        frame.reason.contains("tenant-a"),
        "swapped-tenant Listen close reason should name the rejected target tenant: {frame:?}"
    );
}

fn firebase_tenant_listen_query_request(
    target_id: i32,
    database: &str,
    parent: &str,
    collection_id: &str,
) -> GrpcListenRequest {
    GrpcListenRequest {
        database: database.to_string(),
        target_change: Some(GrpcListenTargetChange::AddTarget(GrpcTarget {
            target_id,
            once: false,
            expected_count: None,
            target_type: Some(GrpcTargetType::Query(
                crate::adapters::firebase::grpc::generated::google::firestore::v1::target::QueryTarget {
                    parent: parent.to_string(),
                    query_type: Some(GrpcListenQueryType::StructuredQuery(GrpcStructuredQuery {
                        from: vec![GrpcCollectionSelector {
                            collection_id: collection_id.to_string(),
                            all_descendants: false,
                        }],
                        ..Default::default()
                    })),
                },
            )),
            resume_type: None,
        })),
        labels: HashMap::new(),
    }
}

#[tokio::test]
async fn firebase_listen_websocket_mock_user_token_requires_explicit_server_opt_in() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
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

    let auth_protocol = firebase_listen_ws_auth_protocol("mock-user-123", "demo");

    // Without the opt-in the bypass is off: the offered token cannot be verified,
    // so the listen add-target is refused and the socket closes with a policy
    // frame.
    let without_opt_in =
        ServerFixture::start(router_for_firebase(service.clone(), FirebaseConfig::new())).await;
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
        axum::http::HeaderValue::from_str(&auth_protocol)
            .expect("listen auth subprotocol header should build"),
    );
    let mut rejected_socket = WebSocketFixture::connect_request(rejected_request)
        .await
        .expect("ungated websocket handshake should still complete");
    rejected_socket
        .send_binary(
            grpc_listen_query_request(
                22,
                "projects/demo/databases/(default)/documents",
                "mockTokenListen",
            )
            .encode_to_vec(),
        )
        .await;
    let close_code = websocket_close_code(rejected_socket.next_message().await);
    assert_eq!(close_code, WsCloseCode::Policy);

    let with_opt_in =
        ServerFixture::start(router_for_firebase(service, firebase_verified_config())).await;
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
        axum::http::HeaderValue::from_str(&auth_protocol)
            .expect("listen auth subprotocol header should build"),
    );
    let mut accepted_socket = WebSocketFixture::connect_request(accepted_request)
        .await
        .expect("gated websocket bypass auth should connect");
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;

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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let server = ServerFixture::start(router_for_firebase(
        fixture.engine(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

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

        // An enabled route is reachable (not 404) but still gated: an anonymous
        // request reaches the #24 gate and is refused with 403, never 404.
        assert_firebase_rest_anonymous_refused(&server, path, &body).await;

        let response = server
            .client()
            .post(server.http_url(path))
            .header(header::AUTHORIZATION, &bearer)
            .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
            .body(body)
            .send()
            .await
            .expect("enabled firebase request should send");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "unexpected status for {path}"
        );
    }
}

#[tokio::test]
async fn firebase_rest_routes_reject_invalid_bearer_uniformly() {
    // The shared route-layer middleware is what makes Firestore REST auth
    // structural: every REST route — including the unknown-action
    // fallthrough under /documents/{*...} that would otherwise 404 —
    // answers an unverifiable bearer with 401 before any handler runs.
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let server =
        ServerFixture::start(router_for_firebase(fixture.engine(), FirebaseConfig::new())).await;

    for path in [
        "/v1/projects/demo/databases/(default)/documents:commit",
        "/v1/projects/demo/databases/(default)/documents:batchWrite",
        "/v1/projects/demo/databases/(default)/documents:batchGet",
        "/v1/projects/demo/databases/(default)/documents:beginTransaction",
        "/v1/projects/demo/databases/(default)/documents:rollback",
        "/v1/projects/demo/databases/(default)/documents:listCollectionIds",
        "/v1/projects/demo/databases/(default)/documents:runQuery",
        "/v1/projects/demo/databases/(default)/documents:runAggregationQuery",
        "/v1/projects/demo/databases/(default)/documents/cities/SF:runQuery",
        "/v1/projects/demo/databases/(default)/documents/cities/SF:listCollectionIds",
        "/v1/projects/demo/databases/(default)/documents/cities/SF:unknownAction",
    ] {
        let response = server
            .client()
            .post(server.http_url(path))
            .header(header::AUTHORIZATION, "Bearer not-a-real-token")
            .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
            .body("{}")
            .send()
            .await
            .expect("invalid-bearer firebase request should send");
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "every Firestore REST route must reject an invalid bearer before its handler runs: {path}"
        );
    }
}

#[tokio::test]
async fn firebase_commit_rejects_malformed_commit_json() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let server = ServerFixture::start(router_for_firebase(
        fixture.engine(),
        firebase_verified_config(),
    ))
    .await;

    // Even malformed commits must clear the #24 gate first: an anonymous
    // malformed commit is refused with 403, not 400.
    assert_firebase_rest_anonymous_refused(
        &server,
        "/v1/projects/demo/databases/(default)/documents:commit",
        "{}",
    )
    .await;

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(
            header::AUTHORIZATION,
            firebase_verified_bearer("user-123", "demo"),
        )
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body("{}")
        .send()
        .await
        .expect("malformed firebase commit request should send");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
