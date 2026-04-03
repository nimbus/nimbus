use super::*;
use neovex_storage::{FaultPoint, ManualClock};
use reqwest::StatusCode;
use std::sync::Arc;
use tokio::time::timeout;

#[tokio::test]
async fn websocket_unsubscribe_stops_receiving_updates() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("4", "tasks").await;

    let initial = socket.next_json().await;
    let subscription_id = initial["subscription_id"]
        .as_u64()
        .expect("subscription id should be present");

    socket.unsubscribe(subscription_id).await;
    assert!(
        api.insert_document("demo", "tasks", json!({ "title": "Hello" }))
            .await
            .status()
            .is_success()
    );

    let next = socket
        .next_json_with_timeout(Duration::from_millis(150))
        .await;
    assert!(next.is_none(), "unsubscribe should stop reactive pushes");
}

#[tokio::test]
async fn websocket_multiple_subscriptions_share_a_connection() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("tasks", "tasks").await;
    socket.subscribe_all("users", "users").await;

    let first = socket.next_json().await;
    let second = socket.next_json().await;
    assert_eq!(first["type"], json!("subscription_result"));
    assert_eq!(second["type"], json!("subscription_result"));

    assert!(
        api.insert_document("demo", "tasks", json!({ "title": "Hello" }))
            .await
            .status()
            .is_success()
    );

    let update = socket.next_json().await;
    assert_eq!(update["type"], json!("subscription_result"));
    assert_eq!(update["data"][0]["title"], json!("Hello"));

    let maybe_extra = socket
        .next_json_with_timeout(Duration::from_millis(150))
        .await;
    assert!(
        maybe_extra.is_none(),
        "unrelated subscription should stay idle"
    );
}

#[tokio::test]
async fn websocket_disconnect_drops_subscription_without_explicit_unsubscribe() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server = ServerFixture::start(build_router(service.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());
    let tenant_id = neovex_core::TenantId::new("demo").expect("tenant id should be valid");

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("disconnect", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(
        service
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        1
    );

    drop(socket);

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if service
                .active_subscription_count(&tenant_id)
                .expect("subscription count should load")
                == 0
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("connection teardown should release subscription handles");
}

#[tokio::test]
async fn websocket_disconnect_before_bootstrap_activation_cancels_pending_subscription_and_reconnects_cleanly()
 {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server = ServerFixture::start(build_router(service.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());
    let tenant_id = neovex_core::TenantId::new("demo").expect("tenant id should be valid");
    service
        .arm_subscription_bootstrap_pause_for_testing(&tenant_id)
        .expect("bootstrap pause handle should arm");

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("bootstrap-disconnect", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("bootstrap-disconnect"));
    assert_eq!(initial["data"], json!([]));
    assert!(
        service
            .wait_for_subscription_bootstrap_pause_for_testing(&tenant_id, Duration::from_secs(1),)
            .expect("bootstrap pause waiter should succeed"),
        "subscription bootstrap should pause before activation"
    );

    drop(socket);

    timeout(Duration::from_secs(2), async {
        loop {
            if service
                .active_subscription_count(&tenant_id)
                .expect("subscription count should load")
                == 0
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("disconnect should cancel the pending bootstrap without waiting for activation");

    service
        .release_subscription_bootstrap_pause_for_testing(&tenant_id)
        .expect("bootstrap pause should release");

    assert_eq!(
        api.insert_document("demo", "tasks", json!({ "title": "after-disconnect" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let mut reconnected = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    reconnected
        .subscribe_all("bootstrap-reconnected", "tasks")
        .await;

    let reconnected_initial = reconnected.next_json().await;
    assert_eq!(reconnected_initial["type"], json!("subscription_result"));
    assert_eq!(
        reconnected_initial["request_id"],
        json!("bootstrap-reconnected")
    );
    let reconnected_current = if reconnected_initial["data"]
        .as_array()
        .is_some_and(|documents| documents.is_empty())
    {
        let caught_up = reconnected.next_json().await;
        assert_eq!(caught_up["type"], json!("subscription_result"));
        caught_up
    } else {
        reconnected_initial
    };
    assert_eq!(
        reconnected_current["data"][0]["title"],
        json!("after-disconnect")
    );
}

#[tokio::test]
async fn websocket_unsubscribe_during_bootstrap_activation_keeps_subscription_gone() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server = ServerFixture::start(build_router(service.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());
    let tenant_id = neovex_core::TenantId::new("demo").expect("tenant id should be valid");
    service
        .arm_subscription_bootstrap_pause_for_testing(&tenant_id)
        .expect("bootstrap pause handle should arm");

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("bootstrap-unsubscribe", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    let subscription_id = initial["subscription_id"]
        .as_u64()
        .expect("subscription id should be present");
    assert!(
        service
            .wait_for_subscription_bootstrap_pause_for_testing(&tenant_id, Duration::from_secs(1))
            .expect("bootstrap pause waiter should succeed"),
        "subscription bootstrap should pause before activation",
    );

    socket.unsubscribe(subscription_id).await;

    timeout(Duration::from_secs(2), async {
        loop {
            if service
                .active_subscription_count(&tenant_id)
                .expect("subscription count should load")
                == 0
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("unsubscribe should remove the still-bootstrapping subscription");

    service
        .release_subscription_bootstrap_pause_for_testing(&tenant_id)
        .expect("bootstrap pause should release");

    assert_eq!(
        api.insert_document("demo", "tasks", json!({ "title": "after-unsubscribe" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    assert!(
        socket
            .next_json_with_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "a subscription cancelled during bootstrap should not reactivate later",
    );
}

#[tokio::test]
async fn websocket_reconnect_and_resubscribe_catches_up_after_apply_lag_and_keeps_pushing() {
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let harness = DeterministicHarness::with_fault_injector(
        ScenarioMetadata::new("websocket-reconnect-resubscribe", 74),
        Arc::new(ManualClock::new(neovex_core::Timestamp(74_000))),
        faults.clone(),
    );
    let fixture = ServiceFixture::new_with_harness(harness.clone(), |path, harness| {
        Service::new_with_simulation(path, harness.clock(), harness.fault_injector())
    });
    let service = fixture.service();
    let server = ServerFixture::start(build_router(service.clone())).await;
    let api = HttpApiFixture::new(&server);
    let tenant_id = neovex_core::TenantId::new("demo").expect("tenant id should be valid");

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("initial", "tasks").await;
    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("initial"));
    assert_eq!(initial["data"], json!([]));

    let mut insert_task = tokio::spawn({
        let client = server.client().clone();
        let url = server.http_url("/api/tenants/demo/documents");
        async move {
            client
                .post(url)
                .json(&json!({
                    "table": "tasks",
                    "fields": { "title": "during-lag" }
                }))
                .send()
                .await
                .expect("lagged insert request should succeed")
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("write should block after durable append but before apply");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_task)
            .await
            .is_err(),
        "insert should remain pending until journal apply resumes"
    );

    drop(socket);

    timeout(Duration::from_secs(2), async {
        loop {
            if service
                .active_subscription_count(&tenant_id)
                .expect("subscription count should load")
                == 0
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("disconnect should release the original subscription");

    let mut reconnected = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    reconnected.subscribe_all("reconnected", "tasks").await;
    let maybe_bootstrap = reconnected
        .next_json_with_timeout(Duration::from_millis(150))
        .await;
    if let Some(message) = maybe_bootstrap {
        assert_eq!(message["type"], json!("subscription_result"));
        assert_eq!(message["request_id"], json!("reconnected"));
        assert_eq!(message["data"], json!([]));
        assert!(
            reconnected
                .next_json_with_timeout(Duration::from_millis(150))
                .await
                .is_none(),
            "resubscribe should stay idle after an empty bootstrap while apply is blocked"
        );
    }

    faults.release();
    let insert_response = timeout(Duration::from_secs(1), insert_task)
        .await
        .expect("insert should complete after journal apply resumes")
        .expect("insert task should join");
    assert_eq!(insert_response.status(), StatusCode::CREATED);

    let caught_up = reconnected
        .next_json_with_timeout(Duration::from_secs(2))
        .await
        .expect("reconnected subscription should catch up after apply resumes");
    assert_eq!(caught_up["type"], json!("subscription_result"));
    assert!(
        caught_up.get("request_id").is_none() || caught_up["request_id"] == json!("reconnected"),
        "catch-up after apply resumes may arrive as the delayed bootstrap or as a reactive push"
    );
    assert_eq!(caught_up["data"][0]["title"], json!("during-lag"));

    assert_eq!(
        api.insert_document("demo", "tasks", json!({ "title": "after-reconnect" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let pushed = reconnected
        .next_json_with_timeout(Duration::from_secs(2))
        .await
        .expect("reconnected subscription should continue receiving pushes");
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert!(pushed.get("request_id").is_none());
    let titles = pushed["data"]
        .as_array()
        .expect("reactive payload should be an array")
        .iter()
        .map(|document| {
            document["title"]
                .as_str()
                .expect("title should be a string")
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(titles, vec!["during-lag", "after-reconnect"]);
}
