use super::*;

#[tokio::test]
async fn firebase_listen_websocket_streams_binary_protobuf_frames_and_remove_target() {
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
        &["cities", "LA"],
        [("name", json!("Los Angeles"))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let mut socket =
        WebSocketFixture::connect_raw(&server.ws_url("/google.firestore.v1.Firestore/Listen"))
            .await
            .expect("Firestore Listen websocket should connect");

    socket
        .send_binary(
            grpc_listen_query_request(51, "projects/demo/databases/(default)/documents", "cities")
                .encode_to_vec(),
        )
        .await;
    let (target_changes, document_changes) = collect_listen_websocket_bootstrap(&mut socket).await;

    assert_eq!(
        target_changes
            .iter()
            .map(
                |change| GrpcTargetChangeType::try_from(change.target_change_type)
                    .expect("target change type should decode")
            )
            .collect::<Vec<_>>(),
        vec![GrpcTargetChangeType::Add, GrpcTargetChangeType::Current]
    );
    assert_eq!(document_changes.len(), 2);
    let names = document_changes
        .iter()
        .map(|change| {
            change
                .document
                .as_ref()
                .expect("document change should include a document")
                .name
                .clone()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from_iter([
            "projects/demo/databases/(default)/documents/cities/SF".to_string(),
            "projects/demo/databases/(default)/documents/cities/LA".to_string(),
        ])
    );

    let active = wait_for_value(
        "firebase websocket listen subscription registration",
        Duration::from_secs(2),
        Duration::from_millis(10),
        || {
            let service = service.clone();
            let tenant_id = tenant_id.clone();
            async move {
                service
                    .active_subscription_count(&tenant_id)
                    .expect("subscription count should load")
            }
        },
        |count| *count == 1,
    )
    .await;
    assert_eq!(active, 1);

    socket
        .send_binary(
            GrpcListenRequest {
                database: "projects/demo/databases/(default)".to_string(),
                target_change: Some(GrpcListenTargetChange::RemoveTarget(51)),
                labels: HashMap::new(),
            }
            .encode_to_vec(),
        )
        .await;
    let remove_response = next_listen_websocket_response(&mut socket).await;
    let GrpcListenResponseType::TargetChange(remove_change) = remove_response
        .response_type
        .expect("remove response should set a response_type")
    else {
        panic!("remove_target should return a target change response");
    };
    assert_eq!(
        GrpcTargetChangeType::try_from(remove_change.target_change_type)
            .expect("target change type should decode"),
        GrpcTargetChangeType::Remove
    );
    assert_eq!(remove_change.target_ids, vec![51]);

    let inactive = wait_for_value(
        "firebase websocket listen subscription cleanup after remove_target",
        Duration::from_secs(2),
        Duration::from_millis(10),
        || {
            let service = service.clone();
            let tenant_id = tenant_id.clone();
            async move {
                service
                    .active_subscription_count(&tenant_id)
                    .expect("subscription count should load")
            }
        },
        |count| *count == 0,
    )
    .await;
    assert_eq!(inactive, 0);
}

#[tokio::test]
async fn firebase_listen_websocket_resume_token_reconnects_via_shared_transport_state() {
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
        &["cities", "LA"],
        [("name", json!("Los Angeles"))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let mut socket =
        WebSocketFixture::connect_raw(&server.ws_url("/google.firestore.v1.Firestore/Listen"))
            .await
            .expect("Firestore Listen websocket should connect");
    socket
        .send_binary(
            grpc_listen_query_request(61, "projects/demo/databases/(default)/documents", "cities")
                .encode_to_vec(),
        )
        .await;
    let (target_changes, _document_changes) = collect_listen_websocket_bootstrap(&mut socket).await;
    let initial_resume_token = target_changes
        .last()
        .expect("bootstrap should include a CURRENT target change")
        .resume_token
        .clone();

    drop(socket);
    let inactive = wait_for_value(
        "firebase websocket listen cleanup before resume reconnect",
        Duration::from_secs(2),
        Duration::from_millis(10),
        || {
            let service = service.clone();
            let tenant_id = tenant_id.clone();
            async move {
                service
                    .active_subscription_count(&tenant_id)
                    .expect("subscription count should load")
            }
        },
        |count| *count == 0,
    )
    .await;
    assert_eq!(inactive, 0);

    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("name", json!("San Francisco Updated"))],
    );

    let mut resumed_socket =
        WebSocketFixture::connect_raw(&server.ws_url("/google.firestore.v1.Firestore/Listen"))
            .await
            .expect("Firestore Listen websocket should reconnect");
    resumed_socket
        .send_binary(
            grpc_listen_query_request_with_resume_token(
                62,
                "projects/demo/databases/(default)/documents",
                "cities",
                initial_resume_token.clone(),
            )
            .encode_to_vec(),
        )
        .await;
    let (resumed_target_changes, resumed_document_changes) =
        collect_listen_websocket_bootstrap(&mut resumed_socket).await;

    assert_eq!(
        resumed_target_changes
            .iter()
            .map(
                |change| GrpcTargetChangeType::try_from(change.target_change_type)
                    .expect("target change type should decode")
            )
            .collect::<Vec<_>>(),
        vec![GrpcTargetChangeType::Add, GrpcTargetChangeType::Current]
    );
    for change in &resumed_target_changes {
        assert_eq!(change.target_ids, vec![62]);
    }
    assert_eq!(
        resumed_document_changes.len(),
        1,
        "resume reconnect should only stream the changed document"
    );
    let resumed_document = resumed_document_changes[0]
        .document
        .as_ref()
        .expect("document change should include a document");
    assert_eq!(
        resumed_document.name,
        "projects/demo/databases/(default)/documents/cities/SF"
    );
    assert_eq!(
        resumed_document.fields["name"].value_type,
        Some(GrpcValueType::StringValue(
            "San Francisco Updated".to_string()
        ))
    );
    let resumed_resume_token = resumed_target_changes
        .last()
        .expect("resume bootstrap should include a CURRENT target change")
        .resume_token
        .clone();
    assert!(
        decode_grpc_resume_token(&resumed_resume_token)
            > decode_grpc_resume_token(&initial_resume_token),
        "resume reconnect should advance the covered sequence after a disconnected write"
    );
}

#[tokio::test]
async fn firebase_listen_websocket_accepts_loopback_browser_origin_and_bootstraps() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("name", json!("San Francisco"))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let mut request = server
        .ws_url("/google.firestore.v1.Firestore/Listen")
        .into_client_request()
        .expect("browser websocket request should build");
    request.headers_mut().insert(
        header::ORIGIN,
        axum::http::HeaderValue::from_static("http://localhost:5173"),
    );
    let mut socket = WebSocketFixture::connect_request(request)
        .await
        .expect("loopback browser websocket should connect");

    socket
        .send_binary(
            grpc_listen_query_request(71, "projects/demo/databases/(default)/documents", "cities")
                .encode_to_vec(),
        )
        .await;
    let (target_changes, document_changes) = collect_listen_websocket_bootstrap(&mut socket).await;

    assert_eq!(
        target_changes
            .iter()
            .map(
                |change| GrpcTargetChangeType::try_from(change.target_change_type)
                    .expect("target change type should decode")
            )
            .collect::<Vec<_>>(),
        vec![GrpcTargetChangeType::Add, GrpcTargetChangeType::Current]
    );
    assert_eq!(document_changes.len(), 1);
    assert_eq!(
        document_changes[0]
            .document
            .as_ref()
            .expect("document change should include a document")
            .name,
        "projects/demo/databases/(default)/documents/cities/SF"
    );
}

#[tokio::test]
async fn firebase_listen_websocket_text_frames_close_with_unsupported_code() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;

    let mut socket =
        WebSocketFixture::connect_raw(&server.ws_url("/google.firestore.v1.Firestore/Listen"))
            .await
            .expect("Firestore Listen websocket should connect");
    socket.send_text("not protobuf").await;

    let close_code = websocket_close_code(socket.next_message().await);
    assert_eq!(close_code, WsCloseCode::Unsupported);
}

#[tokio::test]
async fn firebase_listen_websocket_invalid_protobuf_frames_close_with_policy_code() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;

    let mut socket =
        WebSocketFixture::connect_raw(&server.ws_url("/google.firestore.v1.Firestore/Listen"))
            .await
            .expect("Firestore Listen websocket should connect");
    socket.send_binary(vec![0x80]).await;

    let close_code = websocket_close_code(socket.next_message().await);
    assert_eq!(close_code, WsCloseCode::Policy);
}

#[tokio::test]
async fn firebase_listen_websocket_backpressure_closes_with_error_code() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("name", json!("San Francisco"))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let mut socket =
        WebSocketFixture::connect_raw(&server.ws_url("/google.firestore.v1.Firestore/Listen"))
            .await
            .expect("Firestore Listen websocket should connect");
    socket
        .send_binary(
            grpc_listen_query_request(72, "projects/demo/databases/(default)/documents", "cities")
                .encode_to_vec(),
        )
        .await;
    let _bootstrap = collect_listen_websocket_bootstrap(&mut socket).await;

    for index in 0..512 {
        seed_firebase_document(
            &service,
            &tenant_id,
            &["cities", "SF"],
            [("name", json!(format!("San Francisco {index}")))],
        );
    }

    let close_code = loop {
        match timeout(Duration::from_secs(2), socket.next_message()).await {
            Ok(WsMessage::Binary(_)) => continue,
            Ok(message) => break websocket_close_code(message),
            Err(_) => panic!("expected websocket backpressure close frame"),
        }
    };
    assert_eq!(close_code, WsCloseCode::Error);
}
