//! Mixed-workload soak test (D9.5).
//!
//! Runs a sustained, varied stream of operations — reads, writes, conditional
//! writes, queries, TTL + tag metadata, and auth failures — through the public
//! `dispatch` entrypoint, tallying outcomes by class. The invariant: no
//! operation ever produces an unhandled 5xx or a panic; every failure is a
//! modeled DynamoDB error. A panic would abort the test, so completion is the
//! "0 panics / 0 task leaks" proof (dispatch is synchronous — it spawns no
//! background tasks to leak).

use std::sync::Arc;

use http::{HeaderMap, HeaderValue};
use nimbus_core::TenantId;
use nimbus_dynamodb::{AccessKeyRegistry, AuthMode, DispatchContext, dispatch};
use nimbus_engine::Service;
use serde_json::{Value, json};

const KEY: &str = "AKIATEST";
/// Mixed-workload iterations; each iteration issues several operations.
const ITERATIONS: usize = 400;

fn signed_as(key: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256 Credential={key}/20260101/us-east-1/dynamodb/aws4_request, \
         SignedHeaders=host;x-amz-target, Signature=deadbeef"
    )
}

fn headers(key: &str, target: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-amz-target",
        HeaderValue::from_str(&format!("DynamoDB_20120810.{target}")).expect("target"),
    );
    headers.insert(
        "authorization",
        HeaderValue::from_str(&signed_as(key)).expect("auth"),
    );
    headers
}

#[derive(Default)]
struct Tally {
    total: usize,
    ok: usize,
    modeled_errors: usize,
}

#[test]
fn mixed_workload_soak_fails_closed_without_panics() {
    let temp = tempfile::tempdir().expect("tempdir");
    let service = Arc::new(Service::new(temp.path()).expect("service"));
    let registry = AccessKeyRegistry::new()
        .bind(KEY, TenantId::new("acme").expect("tenant"))
        .with_mode(AuthMode::LookupOnly);
    let ctx = DispatchContext {
        service: &service,
        access_keys: &registry,
    };

    let arn = "arn:aws:dynamodb:ddblocal:000000000000:table/Soak";
    let mut tally = Tally::default();

    let mut run = |operation: &str, key: &str, body: Value| -> Value {
        let (status, json) = dispatch(&ctx, &headers(key, operation), body.to_string().as_bytes());
        tally.total += 1;
        if (200..300).contains(&status) {
            tally.ok += 1;
        } else {
            // Every non-2xx must be a modeled 4xx error, never an unhandled 5xx
            // or the not-yet-implemented placeholder.
            assert!(
                (400..500).contains(&status),
                "{operation} produced a non-modeled status {status}: {json}"
            );
            assert!(
                json["__type"].as_str().is_some(),
                "{operation} error lacks a typed __type: {json}"
            );
            assert!(
                !json["message"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("not yet implemented"),
                "{operation} hit the unimplemented placeholder: {json}"
            );
            tally.modeled_errors += 1;
        }
        json
    };

    // Two tables: a simple HASH table and a HASH+RANGE table for queries.
    run(
        "CreateTable",
        KEY,
        json!({
            "TableName": "Soak",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "BillingMode": "PAY_PER_REQUEST",
        }),
    );
    run(
        "CreateTable",
        KEY,
        json!({
            "TableName": "SoakRange",
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
    );

    for i in 0..ITERATIONS {
        let pk = format!("k{}", i % 16);

        run(
            "PutItem",
            KEY,
            json!({ "TableName": "Soak", "Item": { "pk": { "S": pk }, "v": { "N": i.to_string() } } }),
        );
        run(
            "GetItem",
            KEY,
            json!({ "TableName": "Soak", "Key": { "pk": { "S": pk } } }),
        );
        run(
            "UpdateItem",
            KEY,
            json!({
                "TableName": "Soak",
                "Key": { "pk": { "S": pk } },
                "UpdateExpression": "SET v = :v",
                "ExpressionAttributeValues": { ":v": { "N": (i + 1).to_string() } },
            }),
        );

        // Read-back correctness invariant (F15): the value just written must be
        // the value read back. This proves the workload actually mutates and
        // persists state correctly, not merely that each call returns 2xx.
        let read_back = run(
            "GetItem",
            KEY,
            json!({ "TableName": "Soak", "Key": { "pk": { "S": pk } } }),
        );
        assert_eq!(
            read_back["Item"]["v"]["N"].as_str(),
            Some((i + 1).to_string().as_str()),
            "GetItem must read back the value written by the preceding UpdateItem"
        );

        // A range write + query.
        run(
            "PutItem",
            KEY,
            json!({ "TableName": "SoakRange", "Item": { "pk": { "S": "p" }, "sk": { "N": i.to_string() } } }),
        );
        run(
            "Query",
            KEY,
            json!({
                "TableName": "SoakRange",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": { "S": "p" } },
                "Limit": 5,
            }),
        );

        // A conditional write that fails on every other iteration (the item
        // already exists) — exercises the ConditionalCheckFailed path.
        run(
            "PutItem",
            KEY,
            json!({
                "TableName": "Soak",
                "Item": { "pk": { "S": pk }, "v": { "N": "0" } },
                "ConditionExpression": "attribute_not_exists(pk)",
            }),
        );

        // Metadata churn every few iterations.
        if i % 5 == 0 {
            run(
                "TagResource",
                KEY,
                json!({ "ResourceArn": arn, "Tags": [{ "Key": "iter", "Value": i.to_string() }] }),
            );
            run(
                "UpdateTimeToLive",
                KEY,
                json!({ "TableName": "Soak", "TimeToLiveSpecification": { "Enabled": i % 10 == 0, "AttributeName": "ttl" } }),
            );
        }

        // An auth failure every few iterations (unbound key).
        if i % 7 == 0 {
            run(
                "GetItem",
                "AKIAINTRUDER",
                json!({ "TableName": "Soak", "Key": { "pk": { "S": pk } } }),
            );
        }
    }

    // The workload produced a healthy mix of successes and modeled failures and
    // never a single unhandled error.
    assert!(
        tally.total > 2000,
        "soak should issue a sustained load: {}",
        tally.total
    );
    assert!(
        tally.ok > 1000,
        "most operations should succeed: {}",
        tally.ok
    );
    assert!(
        tally.modeled_errors > 0,
        "the conditional + auth-failure paths should produce modeled errors"
    );
    assert_eq!(
        tally.ok + tally.modeled_errors,
        tally.total,
        "every operation is either a 2xx or a modeled 4xx — no unhandled 5xx"
    );

    eprintln!(
        "soak: total={} ok={} modeled_errors={} unhandled_5xx=0 panics=0",
        tally.total, tally.ok, tally.modeled_errors
    );
}
