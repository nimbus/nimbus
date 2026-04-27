use super::*;

#[tokio::test]
async fn firebase_listen_add_target_bootstraps_documents_and_honors_explicit_target_id() {
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

    let mut client = firestore_grpc_client(&server).await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .listen(receiver)
        .await
        .expect("Firestore listen stream should open")
        .into_inner();

    sender
        .unbounded_send(grpc_listen_query_request(
            7,
            "projects/demo/databases/(default)/documents",
            "cities",
        ))
        .expect("Listen add_target should send");
    let (target_changes, document_changes) = collect_listen_bootstrap(&mut responses).await;

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
    for change in &target_changes {
        assert_eq!(change.target_ids, vec![7]);
    }
    assert_eq!(
        document_changes.len(),
        2,
        "bootstrap should stream both documents"
    );
    let names = document_changes
        .iter()
        .map(|change| {
            let document = change
                .document
                .as_ref()
                .expect("document change should include a document");
            assert_eq!(change.target_ids, vec![7]);
            document.name.clone()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from_iter([
            "projects/demo/databases/(default)/documents/cities/SF".to_string(),
            "projects/demo/databases/(default)/documents/cities/LA".to_string(),
        ])
    );

    drop(sender);
    assert!(
        responses
            .message()
            .await
            .expect("Listen stream should close cleanly")
            .is_none(),
        "Listen stream should terminate when the client closes it"
    );
}
#[tokio::test]
async fn firebase_listen_assigns_target_id_when_client_uses_zero() {
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
        .listen(receiver)
        .await
        .expect("Firestore listen stream should open")
        .into_inner();

    sender
        .unbounded_send(grpc_listen_query_request(
            0,
            "projects/demo/databases/(default)/documents",
            "cities",
        ))
        .expect("Listen add_target should send");
    let (target_changes, _documents) = collect_listen_bootstrap(&mut responses).await;
    let add_change = target_changes
        .first()
        .expect("bootstrap should include an ADD target change");

    assert_eq!(
        GrpcTargetChangeType::try_from(add_change.target_change_type)
            .expect("target change type should decode"),
        GrpcTargetChangeType::Add
    );
    assert_eq!(add_change.target_ids.len(), 1);
    assert!(
        add_change.target_ids[0] > 0,
        "server-assigned target IDs must be positive"
    );

    drop(sender);
    assert!(
        responses
            .message()
            .await
            .expect("Listen stream should close cleanly")
            .is_none(),
        "Listen stream should terminate when the client closes it"
    );
}

#[tokio::test]
async fn firebase_listen_remove_target_cleans_up_registration() {
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

    let mut client = firestore_grpc_client(&server).await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .listen(receiver)
        .await
        .expect("Firestore listen stream should open")
        .into_inner();

    sender
        .unbounded_send(grpc_listen_query_request(
            9,
            "projects/demo/databases/(default)/documents",
            "cities",
        ))
        .expect("Listen add_target should send");
    let _bootstrap = collect_listen_bootstrap(&mut responses).await;
    let active = wait_for_value(
        "firebase listen subscription registration",
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

    sender
        .unbounded_send(GrpcListenRequest {
            database: "projects/demo/databases/(default)".to_string(),
            target_change: Some(GrpcListenTargetChange::RemoveTarget(9)),
            labels: HashMap::new(),
        })
        .expect("Listen remove_target should send");
    let remove_response = responses
        .message()
        .await
        .expect("remove response should stream")
        .expect("remove response should be present");
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
    assert_eq!(remove_change.target_ids, vec![9]);

    let inactive = wait_for_value(
        "firebase listen subscription cleanup after remove_target",
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

    drop(sender);
    assert!(
        responses
            .message()
            .await
            .expect("Listen stream should close cleanly")
            .is_none(),
        "Listen stream should terminate when the client closes it"
    );
}

#[tokio::test]
async fn firebase_listen_stream_closure_cleans_up_active_registration() {
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

    let mut client = firestore_grpc_client(&server).await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .listen(receiver)
        .await
        .expect("Firestore listen stream should open")
        .into_inner();

    sender
        .unbounded_send(grpc_listen_query_request(
            11,
            "projects/demo/databases/(default)/documents",
            "cities",
        ))
        .expect("Listen add_target should send");
    let _bootstrap = collect_listen_bootstrap(&mut responses).await;
    let active = wait_for_value(
        "firebase listen subscription registration",
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

    drop(sender);
    assert!(
        responses
            .message()
            .await
            .expect("Listen stream should close cleanly")
            .is_none(),
        "Listen stream should terminate when the client closes it"
    );

    let inactive = wait_for_value(
        "firebase listen subscription cleanup after stream close",
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
async fn firebase_listen_once_target_auto_removes_after_current_snapshot() {
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

    let mut client = firestore_grpc_client(&server).await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .listen(receiver)
        .await
        .expect("Firestore listen stream should open")
        .into_inner();

    sender
        .unbounded_send(grpc_listen_once_query_request(
            31,
            "projects/demo/databases/(default)/documents",
            "cities",
        ))
        .expect("Listen once add_target should send");
    let (target_changes, document_changes) =
        collect_listen_until_target_change(&mut responses, GrpcTargetChangeType::Remove).await;

    assert_eq!(
        target_changes
            .iter()
            .map(
                |change| GrpcTargetChangeType::try_from(change.target_change_type)
                    .expect("target change type should decode")
            )
            .collect::<Vec<_>>(),
        vec![
            GrpcTargetChangeType::Add,
            GrpcTargetChangeType::Current,
            GrpcTargetChangeType::Remove,
        ]
    );
    assert_eq!(document_changes.len(), 1);
    let inactive = wait_for_value(
        "firebase listen once target cleanup after current snapshot",
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

    sender
        .unbounded_send(grpc_listen_query_request(
            32,
            "projects/demo/databases/(default)/documents",
            "cities",
        ))
        .expect("Listen add_target after once target should send");
    let (follow_up_target_changes, _follow_up_document_changes) =
        collect_listen_bootstrap(&mut responses).await;
    assert_eq!(
        follow_up_target_changes
            .first()
            .expect("follow-up bootstrap should include an ADD change")
            .target_ids,
        vec![32]
    );

    drop(sender);
    assert!(
        responses
            .message()
            .await
            .expect("Listen stream should close cleanly")
            .is_none(),
        "Listen stream should terminate when the client closes it"
    );
}

#[tokio::test]
async fn firebase_listen_resume_count_mismatch_emits_filter_then_resets() {
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

    let mut client = firestore_grpc_client(&server).await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .listen(receiver)
        .await
        .expect("Firestore listen stream should open")
        .into_inner();

    sender
        .unbounded_send(grpc_listen_query_request(
            33,
            "projects/demo/databases/(default)/documents",
            "cities",
        ))
        .expect("Listen add_target should send");
    let (target_changes, _document_changes) = collect_listen_bootstrap(&mut responses).await;
    let initial_resume_token = target_changes
        .last()
        .expect("bootstrap should include a CURRENT target change")
        .resume_token
        .clone();

    drop(sender);
    assert!(
        responses
            .message()
            .await
            .expect("Listen stream should close cleanly")
            .is_none(),
        "Listen stream should terminate when the client closes it"
    );

    let mut resumed_client = firestore_grpc_client(&server).await;
    let (resumed_sender, resumed_receiver) = mpsc::unbounded();
    let mut resumed_responses = resumed_client
        .listen(resumed_receiver)
        .await
        .expect("Firestore listen stream should reopen")
        .into_inner();

    resumed_sender
        .unbounded_send(
            grpc_listen_query_request_with_resume_token_and_expected_count(
                34,
                "projects/demo/databases/(default)/documents",
                "cities",
                initial_resume_token,
                1,
            ),
        )
        .expect("resume Listen add_target with stale expected_count should send");
    let (resumed_target_changes, resumed_document_changes, existence_filters) =
        collect_listen_until_target_change_with_filters(
            &mut resumed_responses,
            GrpcTargetChangeType::Current,
        )
        .await;

    assert_eq!(
        resumed_target_changes
            .iter()
            .map(
                |change| GrpcTargetChangeType::try_from(change.target_change_type)
                    .expect("target change type should decode")
            )
            .collect::<Vec<_>>(),
        vec![
            GrpcTargetChangeType::Add,
            GrpcTargetChangeType::Reset,
            GrpcTargetChangeType::Current,
        ]
    );
    assert_eq!(existence_filters.len(), 1);
    assert_eq!(existence_filters[0].target_id, 34);
    assert_eq!(existence_filters[0].count, 2);
    assert!(
        existence_filters[0].unchanged_names.is_none(),
        "phase 1 expected-count recovery should use the optional-bloom-filter fallback"
    );
    assert_eq!(
        resumed_document_changes.len(),
        2,
        "reset fallback should re-bootstrap the full result set"
    );

    drop(resumed_sender);
    assert!(
        resumed_responses
            .message()
            .await
            .expect("resumed Listen stream should close cleanly")
            .is_none(),
        "resumed Listen stream should terminate when the client closes it"
    );
}

#[tokio::test]
async fn firebase_listen_reports_resource_exhausted_when_client_falls_too_far_behind() {
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

    let mut client = firestore_grpc_client(&server).await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .listen(receiver)
        .await
        .expect("Firestore listen stream should open")
        .into_inner();

    sender
        .unbounded_send(grpc_listen_query_request(
            35,
            "projects/demo/databases/(default)/documents",
            "cities",
        ))
        .expect("Listen add_target should send");
    let _bootstrap = collect_listen_bootstrap(&mut responses).await;

    for index in 0..512 {
        seed_firebase_document(
            &service,
            &tenant_id,
            &["cities", "SF"],
            [("name", json!(format!("San Francisco {index}")))],
        );
    }

    let backpressure_error = loop {
        match timeout(Duration::from_secs(2), responses.message()).await {
            Ok(Ok(Some(_response))) => continue,
            Ok(Err(status)) => break status,
            Ok(Ok(None)) => panic!("Listen stream should fail with backpressure before ending"),
            Err(_) => panic!("expected Listen backpressure error"),
        }
    };
    assert_eq!(backpressure_error.code(), Code::ResourceExhausted);

    let inactive = wait_for_value(
        "firebase listen subscription cleanup after backpressure error",
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

    drop(sender);
}

#[tokio::test]
async fn firebase_listen_allows_multiple_targets_with_distinct_server_assigned_ids() {
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
        &["parks", "GGP"],
        [("name", json!("Golden Gate Park"))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let mut client = firestore_grpc_client(&server).await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .listen(receiver)
        .await
        .expect("Firestore listen stream should open")
        .into_inner();

    sender
        .unbounded_send(grpc_listen_query_request(
            0,
            "projects/demo/databases/(default)/documents",
            "cities",
        ))
        .expect("first Listen add_target should send");
    let (first_target_changes, _first_document_changes) =
        collect_listen_bootstrap(&mut responses).await;
    let first_target_id = first_target_changes
        .first()
        .expect("first bootstrap should include an ADD target change")
        .target_ids[0];

    sender
        .unbounded_send(grpc_listen_query_request(
            0,
            "projects/demo/databases/(default)/documents",
            "parks",
        ))
        .expect("second Listen add_target should send");
    let (second_target_changes, _second_document_changes) =
        collect_listen_bootstrap(&mut responses).await;
    let second_target_id = second_target_changes
        .first()
        .expect("second bootstrap should include an ADD target change")
        .target_ids[0];

    assert!(
        first_target_id > 0,
        "first assigned target ID must be positive"
    );
    assert!(
        second_target_id > 0,
        "second assigned target ID must be positive"
    );
    assert_ne!(
        first_target_id, second_target_id,
        "server-assigned IDs must stay distinct across multiple active targets"
    );

    drop(sender);
    assert!(
        responses
            .message()
            .await
            .expect("Listen stream should close cleanly")
            .is_none(),
        "Listen stream should terminate when the client closes it"
    );
}

#[tokio::test]
async fn firebase_listen_routes_updates_and_cleanup_per_target_across_overlapping_queries() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("name", json!("San Francisco")), ("region", json!("west"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "LA"],
        [("name", json!("Los Angeles")), ("region", json!("west"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "DEN"],
        [("name", json!("Denver")), ("region", json!("mountain"))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let mut client = firestore_grpc_client(&server).await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .listen(receiver)
        .await
        .expect("Firestore listen stream should open")
        .into_inner();

    sender
        .unbounded_send(grpc_listen_query_request(
            41,
            "projects/demo/databases/(default)/documents",
            "cities",
        ))
        .expect("first Listen add_target should send");
    let (all_target_changes, all_document_changes) = collect_listen_bootstrap(&mut responses).await;
    assert_eq!(
        all_target_changes
            .iter()
            .map(
                |change| GrpcTargetChangeType::try_from(change.target_change_type)
                    .expect("target change type should decode")
            )
            .collect::<Vec<_>>(),
        vec![GrpcTargetChangeType::Add, GrpcTargetChangeType::Current]
    );
    assert_eq!(all_document_changes.len(), 3);

    sender
        .unbounded_send(grpc_listen_filtered_query_request(
            42,
            "projects/demo/databases/(default)/documents",
            "cities",
            "region",
            grpc_string_value("west"),
        ))
        .expect("second Listen add_target should send");
    let (west_target_changes, west_document_changes) =
        collect_listen_bootstrap(&mut responses).await;
    assert_eq!(
        west_target_changes
            .iter()
            .map(
                |change| GrpcTargetChangeType::try_from(change.target_change_type)
                    .expect("target change type should decode")
            )
            .collect::<Vec<_>>(),
        vec![GrpcTargetChangeType::Add, GrpcTargetChangeType::Current]
    );
    assert_eq!(west_document_changes.len(), 2);

    let active = wait_for_value(
        "firebase listen concurrent subscription registration",
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
        |count| *count == 2,
    )
    .await;
    assert_eq!(active, 2);

    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "DEN"],
        [("name", json!("Denver")), ("region", json!("west"))],
    );
    let (update_target_changes, update_document_changes) =
        collect_listen_until_no_change_for_targets(&mut responses, &[41, 42]).await;
    let no_change_targets = update_target_changes
        .iter()
        .filter_map(|change| {
            (GrpcTargetChangeType::try_from(change.target_change_type).ok()
                == Some(GrpcTargetChangeType::NoChange))
            .then_some(change.target_ids.as_slice())
        })
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(no_change_targets, BTreeSet::from_iter([41, 42]));
    let den_target_ids = update_document_changes
        .iter()
        .filter_map(|change| {
            let document = change.document.as_ref()?;
            (document.name == "projects/demo/databases/(default)/documents/cities/DEN")
                .then_some(change.target_ids.as_slice())
        })
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        den_target_ids,
        BTreeSet::from_iter([41, 42]),
        "the same document should route to both overlapping targets"
    );

    sender
        .unbounded_send(GrpcListenRequest {
            database: "projects/demo/databases/(default)".to_string(),
            target_change: Some(GrpcListenTargetChange::RemoveTarget(42)),
            labels: HashMap::new(),
        })
        .expect("Listen remove_target should send");
    let remove_response = responses
        .message()
        .await
        .expect("remove response should stream")
        .expect("remove response should be present");
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
    assert_eq!(remove_change.target_ids, vec![42]);

    let one_remaining = wait_for_value(
        "firebase listen concurrent subscription cleanup after remove_target",
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
    assert_eq!(one_remaining, 1);

    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "LA"],
        [
            ("name", json!("Los Angeles Updated")),
            ("region", json!("west")),
        ],
    );
    let (_final_target_changes, final_document_changes) =
        collect_listen_until_target_change(&mut responses, GrpcTargetChangeType::NoChange).await;
    let la_target_ids = final_document_changes
        .iter()
        .filter_map(|change| {
            let document = change.document.as_ref()?;
            (document.name == "projects/demo/databases/(default)/documents/cities/LA")
                .then_some(change.target_ids.as_slice())
        })
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        la_target_ids,
        BTreeSet::from_iter([41]),
        "remaining updates should stay routed only to the still-active target"
    );

    drop(sender);
    assert!(
        responses
            .message()
            .await
            .expect("Listen stream should close cleanly")
            .is_none(),
        "Listen stream should terminate when the client closes it"
    );
    let inactive = wait_for_value(
        "firebase listen concurrent subscription cleanup after stream close",
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
async fn firebase_listen_resume_token_reconnects_with_delta_only() {
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

    let mut client = firestore_grpc_client(&server).await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .listen(receiver)
        .await
        .expect("Firestore listen stream should open")
        .into_inner();

    sender
        .unbounded_send(grpc_listen_query_request(
            15,
            "projects/demo/databases/(default)/documents",
            "cities",
        ))
        .expect("Listen add_target should send");
    let (target_changes, _document_changes) = collect_listen_bootstrap(&mut responses).await;
    let initial_current = target_changes
        .last()
        .expect("bootstrap should include a CURRENT target change");
    let initial_resume_token = initial_current.resume_token.clone();

    drop(sender);
    assert!(
        responses
            .message()
            .await
            .expect("Listen stream should close cleanly")
            .is_none(),
        "Listen stream should terminate when the client closes it"
    );
    let inactive = wait_for_value(
        "firebase listen subscription cleanup before resume reconnect",
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

    let mut resumed_client = firestore_grpc_client(&server).await;
    let (resumed_sender, resumed_receiver) = mpsc::unbounded();
    let mut resumed_responses = resumed_client
        .listen(resumed_receiver)
        .await
        .expect("Firestore listen stream should reopen")
        .into_inner();

    resumed_sender
        .unbounded_send(grpc_listen_query_request_with_resume_token(
            21,
            "projects/demo/databases/(default)/documents",
            "cities",
            initial_resume_token.clone(),
        ))
        .expect("resume Listen add_target should send");
    let (resumed_target_changes, resumed_document_changes) =
        collect_listen_bootstrap(&mut resumed_responses).await;

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
        assert_eq!(change.target_ids, vec![21]);
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

    drop(resumed_sender);
    assert!(
        resumed_responses
            .message()
            .await
            .expect("resumed Listen stream should close cleanly")
            .is_none(),
        "resumed Listen stream should terminate when the client closes it"
    );
}

#[tokio::test]
async fn firebase_listen_stale_resume_token_resets_before_full_bootstrap() {
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

    let mut client = firestore_grpc_client(&server).await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .listen(receiver)
        .await
        .expect("Firestore listen stream should open")
        .into_inner();

    sender
        .unbounded_send(grpc_listen_query_request(
            17,
            "projects/demo/databases/(default)/documents",
            "cities",
        ))
        .expect("Listen add_target should send");
    let (target_changes, _document_changes) = collect_listen_bootstrap(&mut responses).await;
    let initial_resume_token = target_changes
        .last()
        .expect("bootstrap should include a CURRENT target change")
        .resume_token
        .clone();

    drop(sender);
    assert!(
        responses
            .message()
            .await
            .expect("Listen stream should close cleanly")
            .is_none(),
        "Listen stream should terminate when the client closes it"
    );
    let inactive = wait_for_value(
        "firebase listen subscription cleanup before stale-token reconnect",
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

    let stale_resume_token =
        encode_grpc_resume_token(decode_grpc_resume_token(&initial_resume_token) + 999);
    let mut resumed_client = firestore_grpc_client(&server).await;
    let (resumed_sender, resumed_receiver) = mpsc::unbounded();
    let mut resumed_responses = resumed_client
        .listen(resumed_receiver)
        .await
        .expect("Firestore listen stream should reopen")
        .into_inner();

    resumed_sender
        .unbounded_send(grpc_listen_query_request_with_resume_token(
            23,
            "projects/demo/databases/(default)/documents",
            "cities",
            stale_resume_token,
        ))
        .expect("stale-token Listen add_target should send");
    let (resumed_target_changes, resumed_document_changes) =
        collect_listen_bootstrap(&mut resumed_responses).await;

    assert_eq!(
        resumed_target_changes
            .iter()
            .map(
                |change| GrpcTargetChangeType::try_from(change.target_change_type)
                    .expect("target change type should decode")
            )
            .collect::<Vec<_>>(),
        vec![
            GrpcTargetChangeType::Add,
            GrpcTargetChangeType::Reset,
            GrpcTargetChangeType::Current,
        ]
    );
    for change in &resumed_target_changes {
        assert_eq!(change.target_ids, vec![23]);
    }
    let names = resumed_document_changes
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
        ]),
        "stale resume reconnect should force a full bootstrap"
    );

    drop(resumed_sender);
    assert!(
        resumed_responses
            .message()
            .await
            .expect("resumed Listen stream should close cleanly")
            .is_none(),
        "resumed Listen stream should terminate when the client closes it"
    );
}

#[tokio::test]
async fn firebase_listen_no_change_tokens_and_read_times_advance_monotonically() {
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

    let mut client = firestore_grpc_client(&server).await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .listen(receiver)
        .await
        .expect("Firestore listen stream should open")
        .into_inner();

    sender
        .unbounded_send(grpc_listen_query_request(
            29,
            "projects/demo/databases/(default)/documents",
            "cities",
        ))
        .expect("Listen add_target should send");
    let (bootstrap_target_changes, _bootstrap_document_changes) =
        collect_listen_bootstrap(&mut responses).await;
    let initial_current = bootstrap_target_changes
        .last()
        .expect("bootstrap should include a CURRENT target change");
    let initial_sequence = decode_grpc_resume_token(&initial_current.resume_token);
    let initial_read_time = grpc_timestamp_millis(
        initial_current
            .read_time
            .as_ref()
            .expect("CURRENT target change should include a read time"),
    );

    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("name", json!("San Francisco One"))],
    );
    let (first_target_changes, first_document_changes) =
        collect_listen_until_target_change(&mut responses, GrpcTargetChangeType::NoChange).await;
    assert_eq!(
        first_document_changes.len(),
        1,
        "first write should produce one document change"
    );
    let first_no_change = first_target_changes
        .last()
        .expect("first update should end with a NO_CHANGE target change");
    let first_sequence = decode_grpc_resume_token(&first_no_change.resume_token);
    let first_read_time = grpc_timestamp_millis(
        first_no_change
            .read_time
            .as_ref()
            .expect("NO_CHANGE target change should include a read time"),
    );
    assert!(
        first_sequence > initial_sequence,
        "first NO_CHANGE resume token should advance after the first write"
    );
    assert!(
        first_read_time >= initial_read_time,
        "first NO_CHANGE read time should not move backwards"
    );

    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("name", json!("San Francisco Two"))],
    );
    let (second_target_changes, second_document_changes) =
        collect_listen_until_target_change(&mut responses, GrpcTargetChangeType::NoChange).await;
    assert_eq!(
        second_document_changes.len(),
        1,
        "second write should produce one document change"
    );
    let second_no_change = second_target_changes
        .last()
        .expect("second update should end with a NO_CHANGE target change");
    let second_sequence = decode_grpc_resume_token(&second_no_change.resume_token);
    let second_read_time = grpc_timestamp_millis(
        second_no_change
            .read_time
            .as_ref()
            .expect("NO_CHANGE target change should include a read time"),
    );
    assert!(
        second_sequence > first_sequence,
        "subsequent NO_CHANGE resume tokens should keep advancing"
    );
    assert!(
        second_read_time >= first_read_time,
        "subsequent NO_CHANGE read times should not move backwards"
    );

    drop(sender);
    assert!(
        responses
            .message()
            .await
            .expect("Listen stream should close cleanly")
            .is_none(),
        "Listen stream should terminate when the client closes it"
    );
}

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
