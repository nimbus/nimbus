use super::*;

struct ArmedBlockingFaultInjector {
    armed: std::sync::atomic::AtomicBool,
    inner: std::sync::Arc<neovex_test_support::BlockingFaultInjector>,
}

impl ArmedBlockingFaultInjector {
    fn new(point: neovex_storage::FaultPoint) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            armed: std::sync::atomic::AtomicBool::new(false),
            inner: neovex_test_support::BlockingFaultInjector::new(point),
        })
    }

    fn arm(&self) {
        self.armed.store(true, std::sync::atomic::Ordering::Release);
    }

    async fn wait_until_entered(&self) {
        self.inner.wait_until_entered().await;
    }

    fn release(&self) {
        self.armed
            .store(false, std::sync::atomic::Ordering::Release);
        self.inner.release();
    }
}

impl neovex_storage::FaultInjector for ArmedBlockingFaultInjector {
    fn check(&self, point: neovex_storage::FaultPoint) -> neovex_core::Result<()> {
        if !self.armed.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(());
        }
        self.inner.check(point)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MessageSnapshot {
    author: String,
    body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CreatedMessage {
    id: String,
    author: String,
    body: String,
}

#[derive(Debug, Clone)]
enum SeededDemoOperation {
    SendViaAction {
        author: String,
        body: String,
    },
    SendViaHttpAction {
        author: String,
        body: String,
    },
    ScheduleSend {
        author: String,
        body: String,
    },
    RuntimeSendAndSchedule {
        author: String,
        body: String,
    },
    QueryByAuthor {
        author: Option<String>,
    },
    LoadViaHttpAction {
        author: String,
    },
    LoadById {
        message_index: usize,
    },
    CheckUnique {
        author: String,
    },
    CheckExact {
        author: String,
        body: String,
        expect_match: bool,
    },
}

fn scenario_message_budget() -> usize {
    12
}

fn seeded_convex_demo_request_timeout() -> Duration {
    Duration::from_secs(3)
}

fn seeded_convex_demo_operation_count(step_count: usize) -> usize {
    (6 + step_count / 12).min(14)
}

fn seeded_convex_demo_faulted_overlap_step(operation_count: usize) -> usize {
    operation_count.saturating_sub(1).min(2)
}

fn seeded_convex_demo_draw(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut draw = *state;
    draw = (draw ^ (draw >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    draw = (draw ^ (draw >> 27)).wrapping_mul(0x94d049bb133111eb);
    draw ^ (draw >> 31)
}

fn seeded_convex_demo_author(state: &mut u64) -> String {
    const AUTHORS: [&str; 4] = ["Ada", "Byron", "Curie", "Dijkstra"];
    AUTHORS[(seeded_convex_demo_draw(state) as usize) % AUTHORS.len()].to_string()
}

fn seeded_convex_demo_body(seed: u64, step_index: usize, state: &mut u64) -> String {
    format!(
        "seed-{seed}-step-{step_index}-{:04x}",
        seeded_convex_demo_draw(state) & 0xffff
    )
}

fn normalize_message_snapshots(values: &[serde_json::Value]) -> Vec<MessageSnapshot> {
    let mut snapshots = values
        .iter()
        .map(|value| MessageSnapshot {
            author: value["author"]
                .as_str()
                .expect("message author should be a string")
                .to_string(),
            body: value["body"]
                .as_str()
                .expect("message body should be a string")
                .to_string(),
        })
        .collect::<Vec<_>>();
    snapshots.sort();
    snapshots
}

fn expected_message_snapshots(
    created: &[CreatedMessage],
    author: Option<&str>,
) -> Vec<MessageSnapshot> {
    let mut snapshots = created
        .iter()
        .filter(|message| author.is_none_or(|expected| message.author == expected))
        .map(|message| MessageSnapshot {
            author: message.author.clone(),
            body: message.body.clone(),
        })
        .collect::<Vec<_>>();
    snapshots.sort();
    snapshots
}

fn message_from_value(value: &serde_json::Value) -> CreatedMessage {
    CreatedMessage {
        id: value["_id"]
            .as_str()
            .expect("message id should be a string")
            .to_string(),
        author: value["author"]
            .as_str()
            .expect("message author should be a string")
            .to_string(),
        body: value["body"]
            .as_str()
            .expect("message body should be a string")
            .to_string(),
    }
}

fn find_message_value(
    messages: &serde_json::Value,
    author: &str,
    body: &str,
) -> Option<serde_json::Value> {
    messages.as_array().and_then(|items| {
        items
            .iter()
            .find(|message| message["author"] == json!(author) && message["body"] == json!(body))
            .cloned()
    })
}

async fn wait_for_message_record(
    api: &HttpApiFixture<'_>,
    author: &str,
    body: &str,
) -> CreatedMessage {
    let messages = wait_for_message(api, author, body).await;
    let message = find_message_value(&messages, author, body)
        .expect("waited-for message should be present in the query response");
    message_from_value(&message)
}

fn seeded_convex_demo_context(
    seed: u64,
    operation_count: usize,
    case: Option<GeneratedTaskHistorySeedCase>,
    test_name: &str,
    invariant: &str,
    step_index: Option<usize>,
) -> String {
    match case {
        Some(case) => {
            let step_suffix = step_index
                .map(|step| format!(" at convex demo step {step}"))
                .unwrap_or_default();
            format!(
                "{invariant}{step_suffix}; convex demo seed {}, operations {}. {}",
                seed,
                operation_count,
                case.failure_context("neovex-server", test_name, invariant)
            )
        }
        None => history_context(seed, operation_count, invariant, step_index),
    }
}

fn history_context(
    seed: u64,
    operation_count: usize,
    invariant: &str,
    step_index: Option<usize>,
) -> String {
    match step_index {
        Some(step_index) => format!(
            "{invariant}; convex demo seed {seed}, operations {operation_count}, step {step_index}"
        ),
        None => format!("{invariant}; convex demo seed {seed}, operations {operation_count}"),
    }
}

fn choose_seeded_convex_demo_operation(
    seed: u64,
    step_index: usize,
    created: &[CreatedMessage],
    state: &mut u64,
) -> SeededDemoOperation {
    if created.is_empty() {
        return SeededDemoOperation::SendViaAction {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        };
    }

    let max_messages = scenario_message_budget();
    let can_write = created.len() < max_messages;
    let can_runtime_write = created.len() + 2 <= max_messages;
    let draw = seeded_convex_demo_draw(state) % 10;

    match draw {
        0 if can_runtime_write => SeededDemoOperation::RuntimeSendAndSchedule {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        },
        1 | 2 if can_write => SeededDemoOperation::SendViaAction {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        },
        3 if can_write => SeededDemoOperation::SendViaHttpAction {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        },
        4 if can_write => SeededDemoOperation::ScheduleSend {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        },
        5 => {
            let author = if seeded_convex_demo_draw(state).is_multiple_of(4) {
                None
            } else {
                Some(
                    created[(seeded_convex_demo_draw(state) as usize) % created.len()]
                        .author
                        .clone(),
                )
            };
            SeededDemoOperation::QueryByAuthor { author }
        }
        6 => SeededDemoOperation::LoadViaHttpAction {
            author: created[(seeded_convex_demo_draw(state) as usize) % created.len()]
                .author
                .clone(),
        },
        7 => SeededDemoOperation::LoadById {
            message_index: (seeded_convex_demo_draw(state) as usize) % created.len(),
        },
        8 => {
            let author = if seeded_convex_demo_draw(state).is_multiple_of(5) {
                format!("missing-author-{}", step_index)
            } else {
                created[(seeded_convex_demo_draw(state) as usize) % created.len()]
                    .author
                    .clone()
            };
            SeededDemoOperation::CheckUnique { author }
        }
        _ => {
            if seeded_convex_demo_draw(state).is_multiple_of(2) {
                let message = &created[(seeded_convex_demo_draw(state) as usize) % created.len()];
                SeededDemoOperation::CheckExact {
                    author: message.author.clone(),
                    body: message.body.clone(),
                    expect_match: true,
                }
            } else {
                SeededDemoOperation::CheckExact {
                    author: seeded_convex_demo_author(state),
                    body: format!("missing-body-{}", step_index),
                    expect_match: false,
                }
            }
        }
    }
}

fn assert_messages_match_expected(
    actual: &serde_json::Value,
    expected: &[CreatedMessage],
    author: Option<&str>,
    context: &str,
) {
    let actual_messages = normalize_message_snapshots(
        actual
            .as_array()
            .expect("messages response should contain an array"),
    );
    assert_eq!(
        actual_messages,
        expected_message_snapshots(expected, author),
        "{context}"
    );
}

async fn execute_faulted_seeded_convex_demo_overlap<F>(
    api: &HttpApiFixture<'_>,
    server: &ServerFixture,
    faults: &std::sync::Arc<ArmedBlockingFaultInjector>,
    created: &mut Vec<CreatedMessage>,
    seed: u64,
    step_index: usize,
    context: &F,
) where
    F: Fn(&str, Option<usize>) -> String,
{
    let author = format!("faulted-seed-{seed}");
    let action_body = format!("faulted-action-{step_index}");
    let http_body = format!("faulted-http-{step_index}");
    let second_action_body = format!("faulted-follow-up-{step_index}");

    faults.arm();

    let mut action = tokio::spawn({
        let client = server.client().clone();
        let url = api.convex_url("demo", "/action");
        let author = author.clone();
        let action_body = action_body.clone();
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

    timeout(
        seeded_convex_demo_request_timeout(),
        faults.wait_until_entered(),
    )
    .await
    .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut action)
            .await
            .is_err(),
        "{}",
        context(
            "faulted seeded action should remain pending while apply is blocked",
            Some(step_index),
        )
    );

    let mut blocked_query = tokio::spawn({
        let client = server.client().clone();
        let url = api.convex_url("demo", "/query");
        let author = author.clone();
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
        "{}",
        context(
            "faulted seeded query should remain pending until apply resumes",
            Some(step_index),
        )
    );

    let mut http_post = tokio::spawn({
        let client = server.client().clone();
        let url = api.convex_http_url("demo", "/messages");
        let author = author.clone();
        let http_body = http_body.clone();
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
        "{}",
        context(
            "faulted seeded httpAction post should remain pending while apply is blocked",
            Some(step_index),
        )
    );

    faults.release();

    let action = timeout(seeded_convex_demo_request_timeout(), action)
        .await
        .expect("runtime-backed action should resolve after apply resumes")
        .expect("action task should join");
    assert_eq!(
        action.status(),
        StatusCode::OK,
        "{}",
        context(
            "faulted seeded action should succeed after apply resumes",
            Some(step_index),
        )
    );
    let action_id = action
        .json::<serde_json::Value>()
        .await
        .expect("faulted seeded action response should parse");
    let action_message = wait_for_message_record(api, &author, &action_body).await;
    assert_eq!(
        action_id,
        json!(action_message.id),
        "{}",
        context(
            "faulted seeded action should return the inserted message id",
            Some(step_index),
        )
    );
    created.push(action_message);

    let blocked_query = timeout(seeded_convex_demo_request_timeout(), blocked_query)
        .await
        .expect("blocked query should resolve after apply resumes")
        .expect("blocked query task should join");
    assert_eq!(
        blocked_query.status(),
        StatusCode::OK,
        "{}",
        context(
            "faulted seeded query should succeed after apply resumes",
            Some(step_index),
        )
    );
    let blocked_query_body = blocked_query
        .json::<serde_json::Value>()
        .await
        .expect("faulted seeded query response should parse");
    assert!(blocked_query_body.as_array().is_some_and(|items| {
        items.iter().any(|message| {
            message["author"] == json!(author) && message["body"] == json!(action_body)
        })
    }));

    let http_post = timeout(seeded_convex_demo_request_timeout(), &mut http_post)
        .await
        .expect("follow-up httpAction post should resolve after apply resumes")
        .expect("httpAction post task should join");
    assert_eq!(
        http_post.status(),
        StatusCode::CREATED,
        "{}",
        context(
            "faulted seeded httpAction post should succeed after apply resumes",
            Some(step_index),
        )
    );
    let http_post_body = http_post
        .json::<serde_json::Value>()
        .await
        .expect("faulted seeded httpAction post response should parse");
    let http_message = wait_for_message_record(api, &author, &http_body).await;
    assert_eq!(
        http_post_body["id"],
        json!(http_message.id),
        "{}",
        context(
            "faulted seeded httpAction post should return the inserted message id",
            Some(step_index),
        )
    );
    created.push(http_message);

    let second_action = timeout(
        seeded_convex_demo_request_timeout(),
        api.convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": author, "body": second_action_body }),
        ),
    )
    .await
    .expect("follow-up runtime-backed action should resolve after the faulted overlap");
    assert_eq!(
        second_action.status(),
        StatusCode::OK,
        "{}",
        context(
            "faulted seeded follow-up action should succeed after overlap recovery",
            Some(step_index),
        )
    );
    let second_action_id = second_action
        .json::<serde_json::Value>()
        .await
        .expect("faulted seeded follow-up action response should parse");
    let second_action_message = wait_for_message_record(api, &author, &second_action_body).await;
    assert_eq!(
        second_action_id,
        json!(second_action_message.id),
        "{}",
        context(
            "faulted seeded follow-up action should return the inserted message id",
            Some(step_index),
        )
    );
    created.push(second_action_message);
}

async fn assert_seeded_convex_demo_usage_scenario_matches_model(
    seed: u64,
    operation_count: usize,
    case: Option<GeneratedTaskHistorySeedCase>,
    test_name: &str,
    faulted_overlap_step: Option<usize>,
) {
    let registry = http_demo_registry(0);
    let (fixture, faults) = if faulted_overlap_step.is_some() {
        let faults = ArmedBlockingFaultInjector::new(
            neovex_storage::FaultPoint::JournalDurableAppendBeforeApply,
        );
        let harness = DeterministicHarness::with_fault_injector(
            ScenarioMetadata::new(
                format!("{test_name}-faulted-overlap"),
                seed.saturating_add(10_000),
            ),
            Arc::new(neovex_storage::ManualClock::new(neovex_core::Timestamp(
                seed.saturating_add(10_000),
            ))),
            faults.clone(),
        );
        let fixture = ServiceFixture::new_with_harness(harness, |path, harness| {
            Service::new_with_simulation(path, harness.clock(), harness.fault_injector())
        });
        (fixture, Some(faults))
    } else {
        (ServiceFixture::new(|path| Service::new(path)), None)
    };
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service, shutdown_rx));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let context = |invariant: &str, step_index: Option<usize>| {
        seeded_convex_demo_context(
            seed,
            operation_count,
            case,
            test_name,
            invariant,
            step_index,
        )
    };

    let mut state = seed;
    let mut created = Vec::new();

    for step_index in 0..operation_count {
        if faulted_overlap_step == Some(step_index) {
            execute_faulted_seeded_convex_demo_overlap(
                &api,
                &server,
                faults
                    .as_ref()
                    .expect("faulted overlap steps require a blocking fault injector"),
                &mut created,
                seed,
                step_index,
                &context,
            )
            .await;
            continue;
        }

        match choose_seeded_convex_demo_operation(seed, step_index, &created, &mut state) {
            SeededDemoOperation::SendViaAction { author, body } => {
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_named_action(
                        "demo",
                        "messages:sendViaAction",
                        json!({ "author": author, "body": body }),
                    ),
                )
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "{}",
                        context(
                            &format!(
                                "seeded action should resolve for author {author} body {body}"
                            ),
                            Some(step_index),
                        )
                    )
                });
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{}",
                    context("seeded action write should succeed", Some(step_index))
                );
                let returned_id = response
                    .json::<serde_json::Value>()
                    .await
                    .expect("action response should parse");
                let message = wait_for_message_record(&api, &author, &body).await;
                assert_eq!(
                    returned_id,
                    json!(message.id),
                    "{}",
                    context(
                        "action responses should return the inserted message id",
                        Some(step_index),
                    )
                );
                created.push(message);
            }
            SeededDemoOperation::SendViaHttpAction { author, body } => {
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_http_json(
                        "demo",
                        reqwest::Method::POST,
                        "/messages",
                        json!({ "author": author, "body": body }),
                    ),
                )
                .await
                .expect("httpAction post should resolve");
                assert_eq!(
                    response.status(),
                    StatusCode::CREATED,
                    "{}",
                    context("seeded httpAction post should succeed", Some(step_index))
                );
                let returned_body = response
                    .json::<serde_json::Value>()
                    .await
                    .expect("httpAction post response should parse");
                let message = wait_for_message_record(&api, &author, &body).await;
                assert_eq!(
                    returned_body["id"],
                    json!(message.id),
                    "{}",
                    context(
                        "httpAction post responses should return the inserted message id",
                        Some(step_index),
                    )
                );
                created.push(message);
            }
            SeededDemoOperation::ScheduleSend { author, body } => {
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_named_mutation(
                        "demo",
                        "messages:scheduleSend",
                        json!({ "author": author, "body": body, "delayMs": 0 }),
                    ),
                )
                .await
                .expect("scheduled mutation should resolve");
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{}",
                    context("seeded scheduled mutation should succeed", Some(step_index))
                );
                let message = wait_for_message_record(&api, &author, &body).await;
                created.push(message);
            }
            SeededDemoOperation::RuntimeSendAndSchedule { author, body } => {
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_named_mutation(
                        "demo",
                        "messages:sendAndSchedule",
                        json!({ "author": author, "body": body }),
                    ),
                )
                .await
                .expect("runtime mutation should resolve");
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{}",
                    context("seeded runtime mutation should succeed", Some(step_index))
                );
                let returned_id = response
                    .json::<serde_json::Value>()
                    .await
                    .expect("runtime mutation response should parse");
                let immediate = wait_for_message_record(&api, &author, &body).await;
                assert_eq!(
                    returned_id,
                    json!(immediate.id),
                    "{}",
                    context(
                        "runtime mutation responses should return the immediate message id",
                        Some(step_index),
                    )
                );
                created.push(immediate);
                let scheduled =
                    wait_for_message_record(&api, &author, &format!("{body} (scheduled)")).await;
                created.push(scheduled);
            }
            SeededDemoOperation::QueryByAuthor { author } => {
                let messages = query_messages_by_author(&api, author.as_deref()).await;
                assert_messages_match_expected(
                    &messages,
                    &created,
                    author.as_deref(),
                    &context(
                        "seeded query should match the expected message set",
                        Some(step_index),
                    ),
                );
            }
            SeededDemoOperation::LoadViaHttpAction { author } => {
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_http(
                        "demo",
                        reqwest::Method::GET,
                        &format!("/messages/by-author?author={author}"),
                    ),
                )
                .await
                .expect("httpAction get should resolve");
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{}",
                    context("seeded httpAction get should succeed", Some(step_index))
                );
                let messages = response
                    .json::<serde_json::Value>()
                    .await
                    .expect("httpAction get response should parse");
                assert_messages_match_expected(
                    &messages,
                    &created,
                    Some(&author),
                    &context(
                        "seeded httpAction get should match the expected message set",
                        Some(step_index),
                    ),
                );
            }
            SeededDemoOperation::LoadById { message_index } => {
                let message = &created[message_index];
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_named_query("demo", "messages:byId", json!({ "id": message.id })),
                )
                .await
                .expect("byId query should resolve");
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{}",
                    context("seeded byId query should succeed", Some(step_index))
                );
                let body = response
                    .json::<serde_json::Value>()
                    .await
                    .expect("byId query response should parse");
                assert_eq!(body["author"], json!(message.author));
                assert_eq!(body["body"], json!(message.body));
            }
            SeededDemoOperation::CheckUnique { author } => {
                let expected_matches = created
                    .iter()
                    .filter(|message| message.author == author)
                    .collect::<Vec<_>>();
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_named_query(
                        "demo",
                        "messages:uniqueByAuthor",
                        json!({ "author": author }),
                    ),
                )
                .await
                .expect("unique query should resolve");
                match expected_matches.as_slice() {
                    [] => {
                        assert_eq!(
                            response.status(),
                            StatusCode::OK,
                            "{}",
                            context(
                                "unique query with no matching author should succeed",
                                Some(step_index)
                            )
                        );
                        let body = response
                            .json::<serde_json::Value>()
                            .await
                            .expect("unique query response should parse");
                        assert_eq!(
                            body,
                            serde_json::Value::Null,
                            "{}",
                            context(
                                "unique query without a match should return null",
                                Some(step_index)
                            )
                        );
                    }
                    [message] => {
                        assert_eq!(
                            response.status(),
                            StatusCode::OK,
                            "{}",
                            context(
                                "unique query with one matching author should succeed",
                                Some(step_index)
                            )
                        );
                        let body = response
                            .json::<serde_json::Value>()
                            .await
                            .expect("unique query response should parse");
                        assert_eq!(body["author"], json!(message.author));
                        assert_eq!(body["body"], json!(message.body));
                    }
                    _ => {
                        assert_eq!(
                            response.status(),
                            StatusCode::BAD_REQUEST,
                            "{}",
                            context(
                                "unique query with duplicate matches should fail",
                                Some(step_index)
                            )
                        );
                        let body = response
                            .json::<serde_json::Value>()
                            .await
                            .expect("unique query error should parse");
                        assert!(
                            body["error"]
                                .as_str()
                                .is_some_and(|message| message.contains("multiple documents")),
                            "{}",
                            context(
                                "duplicate unique query errors should explain the multiple-document conflict",
                                Some(step_index),
                            )
                        );
                    }
                }
            }
            SeededDemoOperation::CheckExact {
                author,
                body,
                expect_match,
            } => {
                let expected = created
                    .iter()
                    .find(|message| message.author == author && message.body == body);
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_named_query(
                        "demo",
                        "messages:exactByAuthorAndBody",
                        json!({ "author": author, "body": body }),
                    ),
                )
                .await
                .expect("exact query should resolve");
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{}",
                    context("exact query should succeed", Some(step_index))
                );
                let response_body = response
                    .json::<serde_json::Value>()
                    .await
                    .expect("exact query response should parse");
                match expected {
                    Some(message) => {
                        assert!(
                            expect_match,
                            "{}",
                            context(
                                "exact-match scenarios should only be generated when the oracle expects a message",
                                Some(step_index),
                            )
                        );
                        assert_eq!(response_body["author"], json!(message.author));
                        assert_eq!(response_body["body"], json!(message.body));
                    }
                    None => {
                        assert!(
                            !expect_match,
                            "{}",
                            context(
                                "missing exact-match scenarios should only be generated when the oracle expects null",
                                Some(step_index),
                            )
                        );
                        assert_eq!(
                            response_body,
                            serde_json::Value::Null,
                            "{}",
                            context("missing exact queries should return null", Some(step_index))
                        );
                    }
                }
            }
        }
    }

    let all_messages = query_messages_by_author(&api, None).await;
    assert_messages_match_expected(
        &all_messages,
        &created,
        None,
        &context(
            "final seeded Convex demo query should match the accumulated message model",
            None,
        ),
    );

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn convex_http_demo_seeded_usage_scenario_matches_model() {
    assert_seeded_convex_demo_usage_scenario_matches_model(
        17,
        seeded_convex_demo_operation_count(24),
        None,
        "convex_http_demo_seeded_usage_scenario_matches_model",
        None,
    )
    .await;
}

#[tokio::test]
async fn convex_http_demo_faulted_seeded_usage_scenario_matches_model() {
    assert_seeded_convex_demo_usage_scenario_matches_model(
        23,
        seeded_convex_demo_operation_count(24),
        None,
        "convex_http_demo_faulted_seeded_usage_scenario_matches_model",
        Some(seeded_convex_demo_faulted_overlap_step(
            seeded_convex_demo_operation_count(24),
        )),
    )
    .await;
}

#[tokio::test]
#[ignore = "run through verification harness pr mode"]
async fn verification_harness_pr_generated_history_seed_corpus_matches_model_on_convex_demo_surface()
 {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::PullRequest)
        .expect("pull-request corpus should resolve")
    {
        assert_seeded_convex_demo_usage_scenario_matches_model(
            case.seed,
            seeded_convex_demo_operation_count(case.step_count),
            Some(case),
            "verification_harness_pr_generated_history_seed_corpus_matches_model",
            None,
        )
        .await;
    }
}

#[tokio::test]
#[ignore = "run through verification harness nightly mode"]
async fn verification_harness_nightly_generated_history_seed_corpus_matches_model_on_convex_demo_surface()
 {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::Nightly)
        .expect("nightly corpus should resolve")
    {
        assert_seeded_convex_demo_usage_scenario_matches_model(
            case.seed,
            seeded_convex_demo_operation_count(case.step_count),
            Some(case),
            "verification_harness_nightly_generated_history_seed_corpus_matches_model",
            None,
        )
        .await;
    }
}

#[tokio::test]
#[ignore = "run through verification harness pr mode"]
async fn verification_harness_pr_generated_history_seed_corpus_matches_model_on_faulted_convex_demo_surface()
 {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::PullRequest)
        .expect("pull-request corpus should resolve")
    {
        let operation_count = seeded_convex_demo_operation_count(case.step_count);
        assert_seeded_convex_demo_usage_scenario_matches_model(
            case.seed,
            operation_count,
            Some(case),
            "verification_harness_pr_generated_history_seed_corpus_matches_model",
            Some(seeded_convex_demo_faulted_overlap_step(operation_count)),
        )
        .await;
    }
}

#[tokio::test]
#[ignore = "run through verification harness nightly mode"]
async fn verification_harness_nightly_generated_history_seed_corpus_matches_model_on_faulted_convex_demo_surface()
 {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::Nightly)
        .expect("nightly corpus should resolve")
    {
        let operation_count = seeded_convex_demo_operation_count(case.step_count);
        assert_seeded_convex_demo_usage_scenario_matches_model(
            case.seed,
            operation_count,
            Some(case),
            "verification_harness_nightly_generated_history_seed_corpus_matches_model",
            Some(seeded_convex_demo_faulted_overlap_step(operation_count)),
        )
        .await;
    }
}
