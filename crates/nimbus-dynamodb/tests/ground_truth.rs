//! Ground-truth corpus replay (H7 / F14).
//!
//! The parity suite (`crates/nimbus-server/tests/dynamodb_spec`) proves Nimbus
//! works through the official AWS SDK. This complementary lane pins the
//! *response contract* itself: a committed golden corpus of canonical DynamoDB
//! operations with the exact response fields DynamoDB Local / the AWS API
//! reference return, replayed through the adapter's `dispatch` entrypoint and
//! diffed field-by-field. It runs in the ordinary `cargo test` run (no Docker),
//! so a drift from the documented contract fails CI. The Dockerized DynamoDB
//! Local capture lane that the corpus is distilled from is the optional refresh
//! path documented in
//! `docs/private/plans/proof/dynamodb-adapter-hardening/ground-truth-corpus.md`.

use std::sync::Arc;

use http::{HeaderMap, HeaderValue};
use nimbus_core::TenantId;
use nimbus_dynamodb::{AccessKeyRegistry, AuthMode, DispatchContext, dispatch};
use nimbus_engine::Engine;
use serde_json::{Value, json};

const KEY: &str = "AKIATEST";

fn headers(target: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-amz-target",
        HeaderValue::from_str(&format!("DynamoDB_20120810.{target}")).expect("target"),
    );
    headers.insert(
        "authorization",
        HeaderValue::from_str(&format!(
            "AWS4-HMAC-SHA256 Credential={KEY}/20260101/us-east-1/dynamodb/aws4_request, \
             SignedHeaders=host;x-amz-target, Signature=deadbeef"
        ))
        .expect("auth"),
    );
    headers
}

/// One golden corpus entry: an operation, its request, the expected HTTP status,
/// and the response fields (by JSON pointer) that must match the DynamoDB
/// contract exactly. `Value::Null` in an expectation asserts the pointer is
/// absent (DynamoDB omits the field).
struct Golden {
    operation: &'static str,
    request: Value,
    status: u16,
    expect: Vec<(&'static str, Value)>,
}

fn fixture() -> (Arc<Engine>, AccessKeyRegistry, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("tempdir");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
    let registry = AccessKeyRegistry::new()
        .bind(KEY, TenantId::new("acme").expect("tenant"))
        .with_mode(AuthMode::LookupOnly);
    (engine, registry, temp)
}

/// Replay one golden entry and diff it against the captured contract.
fn replay(ctx: &DispatchContext<'_>, golden: &Golden) {
    let (status, body) = dispatch(
        ctx,
        &headers(golden.operation),
        golden.request.to_string().as_bytes(),
    );
    assert_eq!(
        status, golden.status,
        "{} status: got {status}, body {body}",
        golden.operation
    );
    for (pointer, expected) in &golden.expect {
        let actual = body.pointer(pointer);
        if expected.is_null() {
            assert!(
                actual.is_none() || actual == Some(&Value::Null),
                "{} {pointer}: expected absent, got {actual:?}",
                golden.operation
            );
        } else {
            assert_eq!(
                actual,
                Some(expected),
                "{} {pointer}: contract mismatch (body {body})",
                golden.operation
            );
        }
    }
}

#[test]
fn ground_truth_corpus_matches_the_dynamodb_contract() {
    let (engine, registry, _temp) = fixture();
    let ctx = DispatchContext {
        engine: &engine,
        access_keys: &registry,
    };

    // Setup ops are part of the corpus too (their responses are pinned).
    let corpus = vec![
        Golden {
            operation: "CreateTable",
            request: json!({
                "TableName": "Music",
                "KeySchema": [
                    { "AttributeName": "Artist", "KeyType": "HASH" },
                    { "AttributeName": "SongTitle", "KeyType": "RANGE" }
                ],
                "AttributeDefinitions": [
                    { "AttributeName": "Artist", "AttributeType": "S" },
                    { "AttributeName": "SongTitle", "AttributeType": "S" }
                ],
                "BillingMode": "PAY_PER_REQUEST"
            }),
            status: 200,
            expect: vec![
                ("/TableDescription/TableName", json!("Music")),
                ("/TableDescription/TableStatus", json!("ACTIVE")),
                ("/TableDescription/ItemCount", json!(0)),
                (
                    "/TableDescription/KeySchema/0/AttributeName",
                    json!("Artist"),
                ),
                ("/TableDescription/KeySchema/0/KeyType", json!("HASH")),
            ],
        },
        Golden {
            operation: "DescribeLimits",
            request: json!({}),
            status: 200,
            expect: vec![
                ("/AccountMaxReadCapacityUnits", json!(80000)),
                ("/AccountMaxWriteCapacityUnits", json!(80000)),
                ("/TableMaxReadCapacityUnits", json!(40000)),
                ("/TableMaxWriteCapacityUnits", json!(40000)),
            ],
        },
        Golden {
            operation: "PutItem",
            request: json!({
                "TableName": "Music",
                "Item": {
                    "Artist": { "S": "No One You Know" },
                    "SongTitle": { "S": "Call Me Today" },
                    "Awards": { "N": "1" }
                }
            }),
            // PutItem with no ReturnValues echoes an empty body.
            status: 200,
            expect: vec![("/Attributes", Value::Null)],
        },
        Golden {
            operation: "GetItem",
            request: json!({
                "TableName": "Music",
                "Key": {
                    "Artist": { "S": "No One You Know" },
                    "SongTitle": { "S": "Call Me Today" }
                }
            }),
            status: 200,
            expect: vec![
                ("/Item/Awards/N", json!("1")),
                ("/Item/Artist/S", json!("No One You Know")),
            ],
        },
        Golden {
            operation: "UpdateItem",
            request: json!({
                "TableName": "Music",
                "Key": {
                    "Artist": { "S": "No One You Know" },
                    "SongTitle": { "S": "Call Me Today" }
                },
                "UpdateExpression": "SET Awards = :a",
                "ExpressionAttributeValues": { ":a": { "N": "10" } },
                "ReturnValues": "UPDATED_NEW"
            }),
            status: 200,
            expect: vec![("/Attributes/Awards/N", json!("10"))],
        },
        Golden {
            operation: "Query",
            request: json!({
                "TableName": "Music",
                "KeyConditionExpression": "Artist = :a",
                "ExpressionAttributeValues": { ":a": { "S": "No One You Know" } }
            }),
            status: 200,
            expect: vec![
                ("/Count", json!(1)),
                ("/ScannedCount", json!(1)),
                ("/Items/0/Awards/N", json!("10")),
            ],
        },
        Golden {
            operation: "DeleteItem",
            request: json!({
                "TableName": "Music",
                "Key": {
                    "Artist": { "S": "No One You Know" },
                    "SongTitle": { "S": "Call Me Today" }
                }
            }),
            status: 200,
            expect: vec![("/Attributes", Value::Null)],
        },
        // Error-contract entries: the modeled exception envelope must match too.
        Golden {
            operation: "GetItem",
            request: json!({ "TableName": "DoesNotExist", "Key": { "Artist": { "S": "x" } } }),
            status: 400,
            expect: vec![(
                "/__type",
                json!("com.amazonaws.dynamodb.v20120810#ResourceNotFoundException"),
            )],
        },
    ];

    for golden in &corpus {
        replay(&ctx, golden);
    }
}
