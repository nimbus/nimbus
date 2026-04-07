use super::*;

pub(super) async fn query_messages_by_author(
    api: &HttpApiFixture<'_>,
    author: Option<&str>,
) -> serde_json::Value {
    let response = api
        .convex_named_query(
            "demo",
            "messages:maybeByAuthor",
            json!({ "author": author }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    response
        .json::<serde_json::Value>()
        .await
        .expect("messages query should parse")
}

pub(super) async fn wait_for_message(
    api: &HttpApiFixture<'_>,
    author: &str,
    body: &str,
) -> serde_json::Value {
    timeout(Duration::from_secs(3), async {
        loop {
            let messages = query_messages_by_author(api, Some(author)).await;
            if messages.as_array().is_some_and(|items| {
                items.iter().any(|message| {
                    message["author"] == json!(author) && message["body"] == json!(body)
                })
            }) {
                return messages;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("expected demo flow message to arrive")
}
