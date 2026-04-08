use super::*;

pub(super) use neovex_testing::ArmedBlockingFaultInjector;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MessageSnapshot {
    author: String,
    body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CreatedMessage {
    pub(super) id: String,
    pub(super) author: String,
    pub(super) body: String,
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

pub(super) async fn wait_for_message_record(
    api: &HttpApiFixture<'_>,
    author: &str,
    body: &str,
) -> CreatedMessage {
    let messages = wait_for_message(api, author, body).await;
    let message = find_message_value(&messages, author, body)
        .expect("waited-for message should be present in the query response");
    message_from_value(&message)
}

pub(super) fn assert_messages_match_expected(
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
