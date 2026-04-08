use super::*;

#[tokio::test]
async fn convex_http_demo_flow_matches_generated_app_behavior() {
    let registry = http_demo_registry(1_000);
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router_with_convex(service.clone(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let flow_author = "http-demo-flow";
    let action_body = "via-action";
    let scheduled_body = "via-schedule";
    let runtime_body = "via-runtime";
    let http_body = "via-http-action";
    let unique_author = "http-demo-unique";
    let unique_body = "only-one";

    let action = api
        .convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": flow_author, "body": action_body }),
        )
        .await;
    assert_eq!(action.status(), StatusCode::OK);
    let action_id = action
        .json::<serde_json::Value>()
        .await
        .expect("action response should parse");
    assert!(action_id.as_str().is_some());

    let filtered = wait_for_message(&api, flow_author, action_body).await;
    assert!(filtered.as_array().is_some_and(|items| {
        items.iter().any(|message| {
            message["author"] == json!(flow_author) && message["body"] == json!(action_body)
        })
    }));

    let by_id = api
        .convex_named_query("demo", "messages:byId", json!({ "id": action_id }))
        .await;
    assert_eq!(by_id.status(), StatusCode::OK);
    let by_id_body = by_id
        .json::<serde_json::Value>()
        .await
        .expect("byId response should parse");
    assert_eq!(by_id_body["author"], json!(flow_author));
    assert_eq!(by_id_body["body"], json!(action_body));

    let scheduled = api
        .convex_named_mutation(
            "demo",
            "messages:scheduleSend",
            json!({
                "author": flow_author,
                "body": scheduled_body,
                "delayMs": 0
            }),
        )
        .await;
    assert_eq!(scheduled.status(), StatusCode::OK);
    let scheduled_job = scheduled
        .json::<serde_json::Value>()
        .await
        .expect("scheduleSend response should parse");
    assert!(scheduled_job.as_str().is_some());
    let scheduled_messages = wait_for_message(&api, flow_author, scheduled_body).await;
    assert!(scheduled_messages.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|message| message["body"] == json!(scheduled_body))
    }));

    let runtime = api
        .convex_named_mutation(
            "demo",
            "messages:sendAndSchedule",
            json!({ "author": flow_author, "body": runtime_body }),
        )
        .await;
    assert_eq!(runtime.status(), StatusCode::OK);
    let runtime_id = runtime
        .json::<serde_json::Value>()
        .await
        .expect("sendAndSchedule response should parse");
    assert!(runtime_id.as_str().is_some());
    let runtime_messages = wait_for_message(&api, flow_author, runtime_body).await;
    assert!(runtime_messages.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|message| message["body"] == json!(runtime_body))
    }));
    let runtime_scheduled_messages =
        wait_for_message(&api, flow_author, "via-runtime (scheduled)").await;
    assert!(runtime_scheduled_messages.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|message| message["body"] == json!("via-runtime (scheduled)"))
    }));

    let http_post = api
        .convex_http_json(
            "demo",
            reqwest::Method::POST,
            "/messages",
            json!({ "author": flow_author, "body": http_body }),
        )
        .await;
    assert_eq!(http_post.status(), StatusCode::CREATED);
    let http_post_body = http_post
        .json::<serde_json::Value>()
        .await
        .expect("httpAction post response should parse");
    assert!(http_post_body["id"].as_str().is_some());
    wait_for_message(&api, flow_author, http_body).await;

    let http_get = api
        .convex_http(
            "demo",
            reqwest::Method::GET,
            "/messages/by-author?author=http-demo-flow",
        )
        .await;
    assert_eq!(http_get.status(), StatusCode::OK);
    let http_get_body = http_get
        .json::<serde_json::Value>()
        .await
        .expect("httpAction get response should parse");
    assert!(http_get_body.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|message| message["body"] == json!(http_body))
    }));

    assert_eq!(
        api.convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": unique_author, "body": unique_body }),
        )
        .await
        .status(),
        StatusCode::OK
    );

    let unique = api
        .convex_named_query(
            "demo",
            "messages:uniqueByAuthor",
            json!({ "author": unique_author }),
        )
        .await;
    assert_eq!(unique.status(), StatusCode::OK);
    let unique_body_json = unique
        .json::<serde_json::Value>()
        .await
        .expect("unique query should parse");
    assert_eq!(unique_body_json["author"], json!(unique_author));
    assert_eq!(unique_body_json["body"], json!(unique_body));

    let exact = api
        .convex_named_query(
            "demo",
            "messages:exactByAuthorAndBody",
            json!({ "author": unique_author, "body": unique_body }),
        )
        .await;
    assert_eq!(exact.status(), StatusCode::OK);
    let exact_body_json = exact
        .json::<serde_json::Value>()
        .await
        .expect("exact query should parse");
    assert_eq!(exact_body_json["author"], json!(unique_author));
    assert_eq!(exact_body_json["body"], json!(unique_body));

    assert_eq!(
        api.convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": unique_author, "body": "second" }),
        )
        .await
        .status(),
        StatusCode::OK
    );

    let unique_conflict = api
        .convex_named_query(
            "demo",
            "messages:uniqueByAuthor",
            json!({ "author": unique_author }),
        )
        .await;
    assert_eq!(unique_conflict.status(), StatusCode::BAD_REQUEST);
    let unique_conflict_body = unique_conflict
        .json::<serde_json::Value>()
        .await
        .expect("duplicate unique query error should parse");
    assert!(
        unique_conflict_body["error"]
            .as_str()
            .is_some_and(|message| message.contains("multiple documents")),
        "{unique_conflict_body}"
    );

    let all_messages = query_messages_by_author(&api, None).await;
    assert!(all_messages.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|message| message["author"] == json!(flow_author))
    }));

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn convex_http_demo_action_then_http_post_and_follow_up_action_all_complete() {
    let registry = http_demo_registry(1_000);
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let author = "http-demo-probe";
    let action_body = "via-action";
    let http_body = "via-http-action";
    let second_action_body = "via-second-action";

    let action = api
        .convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": author, "body": action_body }),
        )
        .await;
    assert_eq!(action.status(), StatusCode::OK);
    wait_for_message(&api, author, action_body).await;

    let http_post = server
        .client()
        .request(
            reqwest::Method::POST,
            api.convex_http_url("demo", "/messages"),
        )
        .json(&json!({ "author": author, "body": http_body }))
        .send()
        .await
        .expect("httpAction post should resolve");
    assert_eq!(http_post.status(), StatusCode::CREATED);
    wait_for_message(&api, author, http_body).await;

    let second_action = timeout(
        Duration::from_secs(1),
        api.convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": author, "body": second_action_body }),
        ),
    )
    .await
    .expect("second action should resolve");
    assert_eq!(second_action.status(), StatusCode::OK);
    wait_for_message(&api, author, second_action_body).await;
}

#[tokio::test]
async fn convex_http_demo_faulted_overlap_still_completes_http_post_and_follow_up_action() {
    let faults = neovex_testing::BlockingFaultInjector::new(
        neovex_storage::FaultPoint::JournalDurableAppendBeforeApply,
    );
    let harness = DeterministicHarness::with_fault_injector(
        ScenarioMetadata::new("convex-http-demo-faulted-overlap", 61),
        Arc::new(neovex_storage::ManualClock::new(neovex_core::Timestamp(
            61_000,
        ))),
        faults.clone(),
    );
    let registry = http_demo_registry(1_000);
    let fixture = ServiceFixture::new_with_harness(harness, |path, harness| {
        Service::new_with_simulation(path, harness.clock(), harness.fault_injector())
    });
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_convex(service.clone(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let author = "faulted-http-demo";
    let action_body = "first-action";
    let http_body = "follow-up-http";
    let second_action_body = "follow-up-action";

    let mut action = tokio::spawn({
        let client = server.client().clone();
        let url = api.convex_url("demo", "/action");
        async move {
            client
                .post(url)
                .json(&json!({
                    "name": "messages:sendViaAction",
                    "args": { "author": author, "body": action_body }
                }))
                .send()
                .await
                .expect("runtime-backed action should resolve")
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut action)
            .await
            .is_err(),
        "blocked runtime-backed action should remain pending until apply resumes"
    );

    let mut blocked_query = tokio::spawn({
        let client = server.client().clone();
        let url = api.convex_url("demo", "/query");
        async move {
            client
                .post(url)
                .json(&json!({
                    "name": "messages:byAuthor",
                    "args": { "author": author }
                }))
                .send()
                .await
                .expect("blocked query should resolve")
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut blocked_query)
            .await
            .is_err(),
        "query should remain pending while the first durable write is not yet applied"
    );

    let mut http_post = tokio::spawn({
        let client = server.client().clone();
        let url = api.convex_http_url("demo", "/messages");
        async move {
            client
                .post(url)
                .json(&json!({ "author": author, "body": http_body }))
                .send()
                .await
                .expect("httpAction post should resolve")
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut http_post)
            .await
            .is_err(),
        "follow-up httpAction post should remain pending while apply is blocked"
    );

    faults.release();

    let action = timeout(Duration::from_secs(1), action)
        .await
        .expect("runtime-backed action should resolve after apply resumes")
        .expect("action task should join");
    assert_eq!(action.status(), StatusCode::OK);

    let blocked_query = timeout(Duration::from_secs(1), blocked_query)
        .await
        .expect("blocked query should resolve after apply resumes")
        .expect("blocked query task should join");
    assert_eq!(blocked_query.status(), StatusCode::OK);
    let blocked_query_body = blocked_query
        .json::<serde_json::Value>()
        .await
        .expect("blocked query response should parse");
    assert!(blocked_query_body.as_array().is_some_and(|items| {
        items.iter().any(|message| {
            message["author"] == json!(author) && message["body"] == json!(action_body)
        })
    }));

    let http_post = timeout(Duration::from_secs(1), &mut http_post)
        .await
        .expect("follow-up httpAction post should resolve after apply resumes")
        .expect("httpAction post task should join");
    assert_eq!(http_post.status(), StatusCode::CREATED);
    wait_for_message(&api, author, http_body).await;

    let second_action = timeout(
        Duration::from_secs(1),
        api.convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": author, "body": second_action_body }),
        ),
    )
    .await
    .expect("follow-up runtime-backed action should resolve after the faulted overlap");
    assert_eq!(second_action.status(), StatusCode::OK);
    wait_for_message(&api, author, second_action_body).await;
}
