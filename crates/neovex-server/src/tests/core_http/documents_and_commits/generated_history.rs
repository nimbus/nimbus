use super::*;

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
