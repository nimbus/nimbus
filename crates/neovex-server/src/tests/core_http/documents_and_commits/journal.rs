use super::faults::BlockingFaultInjector;
use super::*;

#[tokio::test]
async fn journal_route_returns_ordered_cursor_pages() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let first_insert = api
        .insert_document("demo", "tasks", serde_json::json!({ "title": "first" }))
        .await;
    let first_document_id = first_insert
        .json::<serde_json::Value>()
        .await
        .expect("first insert response should parse")["id"]
        .as_str()
        .expect("first document id should be a string")
        .to_string();
    assert_eq!(
        api.update_document(
            "demo",
            "tasks",
            &first_document_id,
            serde_json::json!({ "title": "first-updated" }),
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        api.insert_document("demo", "tasks", serde_json::json!({ "title": "second" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let first_page = api
        .journal("demo", Some(0), Some(1))
        .await
        .json::<serde_json::Value>()
        .await
        .expect("first journal page should parse");
    assert_eq!(first_page["cursor_floor"], serde_json::json!(0));
    assert_eq!(first_page["latest_sequence"], serde_json::json!(3));
    assert_eq!(first_page["next_cursor"], serde_json::json!(1));
    assert_eq!(first_page["has_more"], serde_json::json!(true));
    assert_eq!(first_page["records"][0]["sequence"], serde_json::json!(1));

    let second_page = api
        .journal("demo", Some(1), Some(1))
        .await
        .json::<serde_json::Value>()
        .await
        .expect("second journal page should parse");
    assert_eq!(second_page["next_cursor"], serde_json::json!(2));
    assert_eq!(second_page["has_more"], serde_json::json!(true));
    assert_eq!(second_page["records"][0]["sequence"], serde_json::json!(2));

    let third_page = api
        .journal("demo", Some(2), Some(1))
        .await
        .json::<serde_json::Value>()
        .await
        .expect("third journal page should parse");
    assert_eq!(third_page["next_cursor"], serde_json::json!(3));
    assert_eq!(third_page["has_more"], serde_json::json!(false));
    assert_eq!(third_page["records"][0]["sequence"], serde_json::json!(3));
}

#[tokio::test]
async fn journal_bootstrap_route_returns_snapshot_and_durable_cut() {
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let harness = DeterministicHarness::with_fault_injector(
        ScenarioMetadata::new("journal-bootstrap-route", 40),
        Arc::new(ManualClock::new(neovex_core::Timestamp(40_000))),
        faults.clone(),
    );
    let fixture = ServiceFixture::new_with_harness(harness.clone(), |path, harness| {
        Service::new_with_simulation(path, harness.clock(), harness.fault_injector())
    });
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let mut insert_task = tokio::spawn({
        let client = server.client().clone();
        let url = server.http_url("/api/tenants/demo/documents");
        async move {
            client
                .post(url)
                .json(&serde_json::json!({
                    "table": "tasks",
                    "fields": { "title": "bootstrap" }
                }))
                .send()
                .await
                .expect("bootstrap insert request should succeed")
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_task)
            .await
            .is_err(),
        "document insert should remain pending until the journal apply step completes"
    );

    let bootstrap = api
        .journal_bootstrap("demo")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("bootstrap response should parse");
    assert_eq!(bootstrap["resume_after_sequence"], serde_json::json!(0));
    assert_eq!(bootstrap["bootstrap_cut_sequence"], serde_json::json!(1));
    assert_eq!(bootstrap["cursor_floor_sequence"], serde_json::json!(0));
    assert_eq!(
        bootstrap["snapshot"]["applied_sequence"],
        serde_json::json!(0)
    );
    assert_eq!(bootstrap["snapshot"]["durable_head"], serde_json::json!(1));
    assert_eq!(bootstrap["snapshot"]["documents"], serde_json::json!([]));

    let tail = api
        .journal("demo", Some(0), Some(10))
        .await
        .json::<serde_json::Value>()
        .await
        .expect("journal tail should parse");
    assert_eq!(tail["records"][0]["sequence"], serde_json::json!(1));
    assert_eq!(tail["next_cursor"], serde_json::json!(1));

    faults.release();
    let insert_response = timeout(Duration::from_secs(1), insert_task)
        .await
        .expect("insert should complete after journal apply unblocks")
        .expect("insert task should join");
    assert_eq!(insert_response.status(), StatusCode::CREATED);
    assert_eq!(harness.describe(), "journal-bootstrap-route (seed 40)");
}
