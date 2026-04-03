use super::*;
use neovex_engine::EmbeddedReplica;
use std::sync::{Arc, Condvar, Mutex};

use neovex_storage::{FaultInjector, FaultPoint, ManualClock};
use tokio::sync::Notify;
use tokio::time::{Duration, timeout};

struct BlockingFaultInjector {
    point: FaultPoint,
    entered: Notify,
    release_gate: (Mutex<bool>, Condvar),
}

impl BlockingFaultInjector {
    fn new(point: FaultPoint) -> Arc<Self> {
        Arc::new(Self {
            point,
            entered: Notify::new(),
            release_gate: (Mutex::new(false), Condvar::new()),
        })
    }

    async fn wait_until_entered(&self) {
        self.entered.notified().await;
    }

    fn release(&self) {
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking fault injector should acquire release lock");
        *released = true;
        cvar.notify_all();
    }
}

impl FaultInjector for BlockingFaultInjector {
    fn check(&self, point: FaultPoint) -> neovex_core::Result<()> {
        if point != self.point {
            return Ok(());
        }
        self.entered.notify_one();
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking fault injector should acquire release lock");
        while !*released {
            released = cvar
                .wait(released)
                .expect("blocking fault injector should wait for release");
        }
        Ok(())
    }
}

fn normalize_generated_task_values(values: Vec<serde_json::Value>) -> Vec<GeneratedTaskRecord> {
    let mut records = values
        .iter()
        .map(GeneratedTaskRecord::from_json)
        .collect::<Vec<_>>();
    records.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.rank.cmp(&right.rank))
            .then_with(|| left.status.cmp(&right.status))
    });
    records
}

fn assert_generated_task_page_matches(
    page: &neovex_core::Page,
    expected: &GeneratedTaskPageExpectation,
    context: &str,
) {
    assert_eq!(
        normalize_generated_task_values(page.data.clone()),
        expected.data,
        "{context}: page data should match the generated-history oracle",
    );
    assert_eq!(
        page.has_more, expected.has_more,
        "{context}: has_more should match the generated-history oracle",
    );
    assert_eq!(
        page.next_cursor.is_some(),
        expected.has_more,
        "{context}: next_cursor presence should track has_more",
    );
}

async fn assert_generated_task_history_matches_model_on_native_http_surface(
    history: &GeneratedTaskHistory,
    case: Option<GeneratedTaskHistorySeedCase>,
    test_name: &str,
) {
    let context = |invariant: &str| {
        case.map(|case| case.failure_context("neovex-server", test_name, invariant))
            .unwrap_or_else(|| history.failure_context(invariant, None))
    };

    let model = history.model();
    let expected_query = model.query_result();
    assert!(
        expected_query.len() > history.page_size(),
        "history seed should produce at least two query pages: {}",
        context("generated-history seed should produce at least two query pages")
    );

    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);
    let table = history.table().to_string();

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    replay_generated_task_history_async(
        history,
        |_slot, record| {
            let api = &api;
            let table = table.clone();
            let fields = serde_json::Value::Object(record.fields());
            async move {
                let response = api.insert_document("demo", &table, fields).await;
                assert_eq!(
                    response.status(),
                    StatusCode::CREATED,
                    "generated-history insert should succeed"
                );
                Ok::<String, std::convert::Infallible>(
                    response
                        .json::<serde_json::Value>()
                        .await
                        .expect("insert response should parse")["id"]
                        .as_str()
                        .expect("insert response should include a document id")
                        .to_string(),
                )
            }
        },
        |_slot, document_id, record| {
            let api = &api;
            let table = table.clone();
            let fields = serde_json::Value::Object(record.fields());
            async move {
                let response = api
                    .update_document("demo", &table, &document_id, fields)
                    .await;
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "generated-history update should succeed"
                );
                Ok::<(), std::convert::Infallible>(())
            }
        },
        |_slot, document_id| {
            let api = &api;
            let table = table.clone();
            async move {
                let response = api.delete_document("demo", &table, &document_id).await;
                assert_eq!(
                    response.status(),
                    StatusCode::NO_CONTENT,
                    "generated-history delete should succeed"
                );
                Ok::<(), std::convert::Infallible>(())
            }
        },
    )
    .await
    .expect("generated history HTTP replay should succeed");

    let list_response = api.list_documents("demo", &table).await;
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body = list_response
        .json::<serde_json::Value>()
        .await
        .expect("list response should parse");
    let listed = normalize_generated_task_values(
        list_body["data"]
            .as_array()
            .expect("list response should contain a data array")
            .clone(),
    );
    assert_eq!(
        listed,
        model.final_documents(),
        "{}",
        context("HTTP list should match the generated-history oracle")
    );

    let query_response = api
        .query_documents(
            "demo",
            serde_json::to_value(history.ordered_query()).expect("query should serialize"),
        )
        .await;
    assert_eq!(query_response.status(), StatusCode::OK);
    let query_body = query_response
        .json::<serde_json::Value>()
        .await
        .expect("query response should parse");
    let queried = normalize_generated_task_values(
        query_body["data"]
            .as_array()
            .expect("query response should contain a data array")
            .clone(),
    );
    assert_eq!(
        queried,
        expected_query,
        "{}",
        context("HTTP query should match the generated-history oracle")
    );

    let first_page_response = api
        .query_documents_paginated(
            "demo",
            serde_json::to_value(history.paginated_query(None)).expect("page should serialize"),
        )
        .await;
    assert_eq!(first_page_response.status(), StatusCode::OK);
    let first_page = first_page_response
        .json::<neovex_core::Page>()
        .await
        .expect("first page should parse");
    assert_generated_task_page_matches(
        &first_page,
        &model.first_page(),
        &context("HTTP first page should match the generated-history oracle"),
    );

    let second_page_response = api
        .query_documents_paginated(
            "demo",
            serde_json::to_value(history.paginated_query(first_page.next_cursor.clone()))
                .expect("page should serialize"),
        )
        .await;
    assert_eq!(second_page_response.status(), StatusCode::OK);
    let second_page = second_page_response
        .json::<neovex_core::Page>()
        .await
        .expect("second page should parse");
    assert_generated_task_page_matches(
        &second_page,
        &model.second_page(),
        &context("HTTP second page should match the generated-history oracle"),
    );
}

#[tokio::test]
async fn create_tenant_and_run_document_lifecycle() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let create_response = api.create_tenant("demo").await;
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let insert_response = api
        .insert_document("demo", "tasks", serde_json::json!({ "title": "Hello" }))
        .await;
    assert_eq!(insert_response.status(), StatusCode::CREATED);
    let document_id = insert_response
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")["id"]
        .as_str()
        .expect("id should be a string")
        .to_string();

    let update_response = api
        .update_document(
            "demo",
            "tasks",
            &document_id,
            serde_json::json!({ "title": "Updated" }),
        )
        .await;
    assert_eq!(update_response.status(), StatusCode::OK);
    assert_eq!(
        update_response
            .json::<serde_json::Value>()
            .await
            .expect("update response should parse")["id"],
        serde_json::json!(document_id)
    );

    let list_response = api.list_documents("demo", "tasks").await;
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body = list_response
        .json::<serde_json::Value>()
        .await
        .expect("list response should parse");
    assert_eq!(list_body["data"][0]["title"], serde_json::json!("Updated"));

    let get_response = api.get_document("demo", "tasks", &document_id).await;
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_body = get_response
        .json::<serde_json::Value>()
        .await
        .expect("get response should parse");
    assert_eq!(get_body["document"]["title"], serde_json::json!("Updated"));

    let delete_response = api.delete_document("demo", "tasks", &document_id).await;
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn commit_log_route_returns_sequenced_commits() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let create_response = api.create_tenant("demo").await;
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let insert_response = api
        .insert_document("demo", "tasks", serde_json::json!({ "title": "Hello" }))
        .await;
    let document_id = insert_response
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")["id"]
        .as_str()
        .expect("id should be a string")
        .to_string();

    api.update_document(
        "demo",
        "tasks",
        &document_id,
        serde_json::json!({ "title": "Updated" }),
    )
    .await;

    let commit_log_response = api.commit_log("demo", None).await;
    assert_eq!(commit_log_response.status(), StatusCode::OK);
    let commit_log = commit_log_response
        .json::<serde_json::Value>()
        .await
        .expect("commit log response should parse");
    assert_eq!(commit_log["latest_sequence"], serde_json::json!(2));
    let commits = commit_log["commits"]
        .as_array()
        .expect("commits should be an array");
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0]["sequence"], serde_json::json!(1));
    assert_eq!(
        commits[0]["writes"][0]["op_type"],
        serde_json::json!("insert")
    );
    assert_eq!(commits[1]["sequence"], serde_json::json!(2));
    assert_eq!(
        commits[1]["writes"][0]["op_type"],
        serde_json::json!("update")
    );

    let filtered_response = api.commit_log("demo", Some(1)).await;
    assert_eq!(filtered_response.status(), StatusCode::OK);
    let filtered = filtered_response
        .json::<serde_json::Value>()
        .await
        .expect("filtered response should parse");
    let commits = filtered["commits"]
        .as_array()
        .expect("commits should be an array");
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0]["sequence"], serde_json::json!(2));
}

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

#[tokio::test]
async fn tenant_consistency_route_returns_green_report_for_live_state() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    for rank in [1, 2, 3] {
        let response = api
            .insert_document("demo", "tasks", serde_json::json!({ "rank": rank }))
            .await;
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    let report = api
        .tenant_consistency_report("demo")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("consistency report should parse");
    assert_eq!(report["ok"], serde_json::json!(true));
    assert_eq!(report["mismatches"], serde_json::json!([]));
    assert_eq!(
        report["authoritative"]["document_count"],
        serde_json::json!(3)
    );
    assert_eq!(
        report["authoritative"]["digest"],
        report["shadow"]["digest"]
    );
    assert_eq!(
        report["authoritative"]["digest"],
        report["embedded_replica"]["digest"]
    );
    assert_eq!(
        report["bootstrap"]["bootstrap_cut_sequence"],
        report["authoritative"]["durable_head"]
    );
}

#[tokio::test]
async fn embedded_replica_matches_server_results_and_catches_up_after_http_writes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server = ServerFixture::start(build_router(service.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    for (title, rank) in [("alpha", 1), ("beta", 2)] {
        assert_eq!(
            api.insert_document(
                "demo",
                "tasks",
                serde_json::json!({ "title": title, "rank": rank }),
            )
            .await
            .status(),
            StatusCode::CREATED
        );
    }

    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let mut replica = EmbeddedReplica::bootstrap_in_memory(&service, tenant_id.clone())
        .await
        .expect("replica should bootstrap");

    let ordered_query = neovex_core::Query {
        table: neovex_core::TableName::new("tasks").expect("table name should build"),
        filters: Vec::new(),
        order: Some(neovex_core::OrderBy {
            field: "rank".to_string(),
            direction: neovex_core::OrderDirection::Asc,
        }),
        limit: None,
    };
    let server_query = api
        .query_documents(
            "demo",
            serde_json::to_value(&ordered_query).expect("query should serialize"),
        )
        .await
        .json::<serde_json::Value>()
        .await
        .expect("server query should parse");
    let replica_query = replica
        .query_documents(&ordered_query)
        .expect("replica query should succeed")
        .into_iter()
        .map(|document| document.to_json())
        .collect::<Vec<_>>();
    assert_eq!(server_query["data"], serde_json::json!(replica_query));

    let paginated = neovex_core::PaginatedQuery {
        query: ordered_query.clone(),
        page_size: 1,
        after: None,
    };
    let server_page = api
        .query_documents_paginated(
            "demo",
            serde_json::to_value(&paginated).expect("page should serialize"),
        )
        .await
        .json::<serde_json::Value>()
        .await
        .expect("server page should parse");
    let replica_page = replica
        .paginate_documents(&paginated)
        .expect("replica page should succeed");
    assert_eq!(
        server_page,
        serde_json::to_value(&replica_page).expect("replica page should serialize")
    );

    assert_eq!(
        api.insert_document(
            "demo",
            "tasks",
            serde_json::json!({ "title": "gamma", "rank": 3 }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    replica
        .catch_up(&service, 1)
        .await
        .expect("replica catch-up should succeed");

    let updated_server_query = api
        .query_documents(
            "demo",
            serde_json::to_value(&ordered_query).expect("query should serialize"),
        )
        .await
        .json::<serde_json::Value>()
        .await
        .expect("updated server query should parse");
    let updated_replica_query = replica
        .query_documents(&ordered_query)
        .expect("updated replica query should succeed")
        .into_iter()
        .map(|document| document.to_json())
        .collect::<Vec<_>>();
    assert_eq!(
        updated_server_query["data"],
        serde_json::json!(updated_replica_query)
    );
}

#[tokio::test]
async fn generated_task_history_matches_model_on_native_http_surface() {
    let history = GeneratedTaskHistory::seeded("http-generated-history", 41, 48);
    assert_generated_task_history_matches_model_on_native_http_surface(
        &history,
        None,
        "generated_task_history_matches_model_on_native_http_surface",
    )
    .await;
}

#[tokio::test]
#[ignore = "run through verification harness pr mode"]
async fn verification_harness_pr_generated_history_seed_corpus_matches_model() {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::PullRequest)
        .expect("pull-request corpus should resolve")
    {
        let history = case.history("http-generated-history");
        assert_generated_task_history_matches_model_on_native_http_surface(
            &history,
            Some(case),
            "verification_harness_pr_generated_history_seed_corpus_matches_model",
        )
        .await;
    }
}

#[tokio::test]
#[ignore = "run through verification harness nightly mode"]
async fn verification_harness_nightly_generated_history_seed_corpus_matches_model() {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::Nightly)
        .expect("nightly corpus should resolve")
    {
        let history = case.history("http-generated-history");
        assert_generated_task_history_matches_model_on_native_http_surface(
            &history,
            Some(case),
            "verification_harness_nightly_generated_history_seed_corpus_matches_model",
        )
        .await;
    }
}

#[tokio::test]
async fn get_nonexistent_document_returns_not_found() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .get_document("demo", "tasks", &neovex_core::DocumentId::new().to_string())
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn dropped_http_insert_after_commit_still_persists_the_document() {
    let faults = BlockingFaultInjector::new(FaultPoint::StorageCommitAfterVisibilityBeforeReturn);
    let faults_for_builder = faults.clone();
    let fixture = ServiceFixture::new(move |path| {
        Service::new_with_simulation(
            path,
            Arc::new(ManualClock::new(neovex_core::Timestamp(30_000))),
            faults_for_builder,
        )
    });
    let service = fixture.service();
    let server = ServerFixture::start(build_router(service.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let request = open_json_post_stream(
        &server,
        "/api/tenants/demo/documents",
        &serde_json::json!({
            "table": "tasks",
            "fields": { "title": "after-disconnect" }
        }),
    )
    .await;
    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("write should block after the durable commit point");
    drop(request);
    faults.release();

    let started_at = tokio::time::Instant::now();
    loop {
        let documents = service
            .list_documents(
                &TenantId::new("demo").expect("tenant id should build"),
                &TableName::new("tasks").expect("table should build"),
            )
            .expect("query should succeed");
        if documents.len() == 1 {
            assert_eq!(
                documents[0].fields.get("title"),
                Some(&serde_json::json!("after-disconnect"))
            );
            break;
        }
        assert!(
            started_at.elapsed() < Duration::from_secs(2),
            "timed out waiting for the committed write to become observable"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
