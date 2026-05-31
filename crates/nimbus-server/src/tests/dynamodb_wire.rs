//! DynamoDB verification-harness scenarios (D8.7).
//!
//! Five deterministic transport-level cases registered in the server
//! verification harness (PR + nightly lanes). Each drives the real `POST /`
//! DynamoDB route through the axum router (the same path the listener serves),
//! exercising one operation family end-to-end with concrete assertions.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use nimbus_core::TenantId;
use nimbus_dynamodb::{AccessKeyRegistry, AuthMode};
use nimbus_engine::Service;
use nimbus_testing::DeterministicTestCase;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::adapters::dynamodb::listener::router;

const ACCESS_KEY: &str = "AKIATEST";
const PROFILE: &str = "run-to-completion-snapshot";

pub(crate) const DYNAMODB_WIRE_HANDSHAKE_AND_CONTROL_PLANE_CASE: DeterministicTestCase =
    DeterministicTestCase::new(
        "dynamodb-wire-handshake-and-control-plane",
        PROFILE,
        "DynamoDB X-Amz-Target dispatch + Create/Describe/List/Delete table over POST /",
    );

pub(crate) const DYNAMODB_ITEM_CRUD_ROUNDTRIP_CASE: DeterministicTestCase =
    DeterministicTestCase::new(
        "dynamodb-item-crud-roundtrip",
        PROFILE,
        "DynamoDB PutItem/GetItem/UpdateItem/DeleteItem roundtrip over the wire",
    );

pub(crate) const DYNAMODB_QUERY_SCAN_WITH_PAGINATION_CASE: DeterministicTestCase =
    DeterministicTestCase::new(
        "dynamodb-query-scan-with-pagination",
        PROFILE,
        "DynamoDB Query with Limit/ExclusiveStartKey pagination and Scan",
    );

pub(crate) const DYNAMODB_TRANSACT_WRITE_COMMIT_ABORT_CASE: DeterministicTestCase =
    DeterministicTestCase::new(
        "dynamodb-transact-write-commit-abort",
        PROFILE,
        "DynamoDB TransactWriteItems commit then a condition-failed abort leaves no partial writes",
    );

pub(crate) const DYNAMODB_STREAMS_EVENT_DELIVERY_CASE: DeterministicTestCase =
    DeterministicTestCase::new(
        "dynamodb-streams-event-delivery",
        PROFILE,
        "DynamoDB stream delivers INSERT then REMOVE records via GetRecords",
    );

fn harness_router() -> (Router, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("tempdir");
    let service = Arc::new(Service::new(temp.path()).expect("service"));
    let registry = AccessKeyRegistry::new()
        .bind(ACCESS_KEY, TenantId::new("acme").expect("tenant"))
        .with_mode(AuthMode::LookupOnly);
    (router(service, registry), temp)
}

fn signed_authorization() -> String {
    format!(
        "AWS4-HMAC-SHA256 Credential={ACCESS_KEY}/20260101/us-east-1/dynamodb/aws4_request, \
         SignedHeaders=host;x-amz-target, Signature=deadbeef"
    )
}

/// Issue one `POST /` against the shared router (state persists across calls
/// because the cloned router shares the same `Arc<Service>`).
async fn call(router: &Router, operation: &str, body: &Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri("/")
        .header("x-amz-target", format!("DynamoDB_20120810.{operation}"))
        .header("authorization", signed_authorization())
        .body(Body::from(body.to_string()))
        .expect("request builds");
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("route responds");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body collects");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn error_type(body: &Value) -> &str {
    body["__type"].as_str().unwrap_or_default()
}

pub(crate) async fn handshake_and_control_plane_inner() {
    let (router, _temp) = harness_router();

    let (status, body) = call(
        &router,
        "CreateTable",
        &json!({
            "TableName": "Orders",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "BillingMode": "PAY_PER_REQUEST",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "CreateTable: {body}");
    assert_eq!(body["TableDescription"]["TableName"], "Orders");
    assert_eq!(body["TableDescription"]["TableStatus"], "ACTIVE");

    let (status, body) = call(&router, "DescribeTable", &json!({ "TableName": "Orders" })).await;
    assert_eq!(status, StatusCode::OK, "DescribeTable: {body}");
    assert_eq!(body["Table"]["TableName"], "Orders");

    let (status, body) = call(&router, "ListTables", &json!({})).await;
    assert_eq!(status, StatusCode::OK, "ListTables: {body}");
    assert!(
        body["TableNames"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n == "Orders"),
        "ListTables should include Orders: {body}"
    );

    let (status, body) = call(&router, "DeleteTable", &json!({ "TableName": "Orders" })).await;
    assert_eq!(status, StatusCode::OK, "DeleteTable: {body}");
    assert_eq!(body["TableDescription"]["TableName"], "Orders");
}

pub(crate) async fn item_crud_roundtrip_inner() {
    let (router, _temp) = harness_router();
    call(
        &router,
        "CreateTable",
        &json!({
            "TableName": "Items",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "BillingMode": "PAY_PER_REQUEST",
        }),
    )
    .await;

    let (status, _) = call(
        &router,
        "PutItem",
        &json!({ "TableName": "Items", "Item": { "pk": { "S": "a" }, "v": { "N": "1" } } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_status, body) = call(
        &router,
        "GetItem",
        &json!({ "TableName": "Items", "Key": { "pk": { "S": "a" } } }),
    )
    .await;
    assert_eq!(body["Item"]["v"]["N"], "1", "GetItem after put: {body}");

    let (status, _) = call(
        &router,
        "UpdateItem",
        &json!({
            "TableName": "Items",
            "Key": { "pk": { "S": "a" } },
            "UpdateExpression": "SET v = :two",
            "ExpressionAttributeValues": { ":two": { "N": "2" } },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_status, body) = call(
        &router,
        "GetItem",
        &json!({ "TableName": "Items", "Key": { "pk": { "S": "a" } } }),
    )
    .await;
    assert_eq!(body["Item"]["v"]["N"], "2", "GetItem after update: {body}");

    let (status, _) = call(
        &router,
        "DeleteItem",
        &json!({ "TableName": "Items", "Key": { "pk": { "S": "a" } } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_status, body) = call(
        &router,
        "GetItem",
        &json!({ "TableName": "Items", "Key": { "pk": { "S": "a" } } }),
    )
    .await;
    assert!(
        body["Item"].is_null(),
        "deleted item should be absent: {body}"
    );
}

pub(crate) async fn query_scan_with_pagination_inner() {
    let (router, _temp) = harness_router();
    call(
        &router,
        "CreateTable",
        &json!({
            "TableName": "Events",
            "KeySchema": [
                { "AttributeName": "pk", "KeyType": "HASH" },
                { "AttributeName": "sk", "KeyType": "RANGE" }
            ],
            "AttributeDefinitions": [
                { "AttributeName": "pk", "AttributeType": "S" },
                { "AttributeName": "sk", "AttributeType": "N" }
            ],
            "BillingMode": "PAY_PER_REQUEST",
        }),
    )
    .await;
    for sk in ["1", "2", "3"] {
        call(
            &router,
            "PutItem",
            &json!({ "TableName": "Events", "Item": { "pk": { "S": "p" }, "sk": { "N": sk } } }),
        )
        .await;
    }

    let (status, page1) = call(
        &router,
        "Query",
        &json!({
            "TableName": "Events",
            "KeyConditionExpression": "pk = :p",
            "ExpressionAttributeValues": { ":p": { "S": "p" } },
            "Limit": 2,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "Query page1: {page1}");
    assert_eq!(page1["Items"].as_array().unwrap().len(), 2, "{page1}");
    let cursor = page1["LastEvaluatedKey"].clone();
    assert!(
        cursor.is_object(),
        "page1 should report LastEvaluatedKey: {page1}"
    );

    let (_status, page2) = call(
        &router,
        "Query",
        &json!({
            "TableName": "Events",
            "KeyConditionExpression": "pk = :p",
            "ExpressionAttributeValues": { ":p": { "S": "p" } },
            "Limit": 2,
            "ExclusiveStartKey": cursor,
        }),
    )
    .await;
    assert_eq!(
        page2["Items"].as_array().unwrap().len(),
        1,
        "page2 should hold the final item: {page2}"
    );

    let (_status, scan) = call(&router, "Scan", &json!({ "TableName": "Events" })).await;
    assert_eq!(scan["Count"], 3, "Scan should see all three items: {scan}");
}

pub(crate) async fn transact_write_commit_abort_inner() {
    let (router, _temp) = harness_router();
    call(
        &router,
        "CreateTable",
        &json!({
            "TableName": "Accounts",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "BillingMode": "PAY_PER_REQUEST",
        }),
    )
    .await;

    // Commit: two puts land atomically.
    let (status, body) = call(
        &router,
        "TransactWriteItems",
        &json!({
            "TransactItems": [
                { "Put": { "TableName": "Accounts", "Item": { "pk": { "S": "x" }, "v": { "N": "1" } } } },
                { "Put": { "TableName": "Accounts", "Item": { "pk": { "S": "y" }, "v": { "N": "2" } } } }
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "commit txn: {body}");
    let (_s, x) = call(
        &router,
        "GetItem",
        &json!({ "TableName": "Accounts", "Key": { "pk": { "S": "x" } } }),
    )
    .await;
    assert_eq!(x["Item"]["v"]["N"], "1", "x committed: {x}");

    // Abort: a failed condition cancels the whole transaction; `z` must not land.
    let (status, body) = call(
        &router,
        "TransactWriteItems",
        &json!({
            "TransactItems": [
                { "Put": { "TableName": "Accounts", "Item": { "pk": { "S": "z" }, "v": { "N": "9" } } } },
                { "Put": {
                    "TableName": "Accounts",
                    "Item": { "pk": { "S": "x" }, "v": { "N": "5" } },
                    "ConditionExpression": "attribute_not_exists(pk)"
                } }
            ]
        }),
    )
    .await;
    assert_ne!(status, StatusCode::OK, "abort txn should fail: {body}");
    assert!(
        error_type(&body).ends_with("TransactionCanceledException"),
        "abort should be TransactionCanceledException: {body}"
    );
    let (_s, z) = call(
        &router,
        "GetItem",
        &json!({ "TableName": "Accounts", "Key": { "pk": { "S": "z" } } }),
    )
    .await;
    assert!(
        z["Item"].is_null(),
        "z must not be written by the aborted txn: {z}"
    );
}

pub(crate) async fn streams_event_delivery_inner() {
    let (router, _temp) = harness_router();
    let (status, created) = call(
        &router,
        "CreateTable",
        &json!({
            "TableName": "Streamed",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES" },
            "BillingMode": "PAY_PER_REQUEST",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create streamed: {created}");
    let arn = created["TableDescription"]["LatestStreamArn"]
        .as_str()
        .expect("stream arn")
        .to_owned();

    call(
        &router,
        "PutItem",
        &json!({ "TableName": "Streamed", "Item": { "pk": { "S": "a" }, "v": { "N": "1" } } }),
    )
    .await;
    call(
        &router,
        "DeleteItem",
        &json!({ "TableName": "Streamed", "Key": { "pk": { "S": "a" } } }),
    )
    .await;

    let (_s, desc) = call(&router, "DescribeStream", &json!({ "StreamArn": arn })).await;
    let shard = desc["StreamDescription"]["Shards"][0]["ShardId"]
        .as_str()
        .expect("shard id")
        .to_owned();

    let (_s, iter) = call(
        &router,
        "GetShardIterator",
        &json!({ "StreamArn": arn, "ShardId": shard, "ShardIteratorType": "TRIM_HORIZON" }),
    )
    .await;
    let iterator = iter["ShardIterator"].as_str().expect("iterator").to_owned();

    let (status, records) =
        call(&router, "GetRecords", &json!({ "ShardIterator": iterator })).await;
    assert_eq!(status, StatusCode::OK, "GetRecords: {records}");
    let recs = records["Records"].as_array().expect("records array");
    assert_eq!(recs.len(), 2, "INSERT + REMOVE delivered: {records}");
    assert_eq!(recs[0]["eventName"], "INSERT", "{records}");
    assert_eq!(recs[1]["eventName"], "REMOVE", "{records}");
}

#[tokio::test]
async fn dynamodb_wire_handshake_and_control_plane() {
    handshake_and_control_plane_inner().await;
}

#[tokio::test]
async fn dynamodb_wire_item_crud_roundtrip() {
    item_crud_roundtrip_inner().await;
}

#[tokio::test]
async fn dynamodb_wire_query_scan_with_pagination() {
    query_scan_with_pagination_inner().await;
}

#[tokio::test]
async fn dynamodb_wire_transact_write_commit_abort() {
    transact_write_commit_abort_inner().await;
}

#[tokio::test]
async fn dynamodb_wire_streams_event_delivery() {
    streams_event_delivery_inner().await;
}
