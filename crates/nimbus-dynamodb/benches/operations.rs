//! Operation-family latency baseline (D9.6).
//!
//! Custom-harness benchmark (`cargo bench -p nimbus-dynamodb --bench operations`):
//! drives each operation family through the public `dispatch` against an
//! in-process, tempdir-backed `Service` and reports p50/p95/p99 latency. This is
//! an in-process protocol+engine baseline (no network), so the numbers isolate
//! adapter + engine cost. Initial non-regression thresholds are documented in
//! `docs/plans/proof/dynamodb-adapter/performance-baseline.md`.

use std::sync::Arc;
use std::time::Instant;

use http::{HeaderMap, HeaderValue};
use nimbus_core::TenantId;
use nimbus_dynamodb::{AccessKeyRegistry, AuthMode, DispatchContext, dispatch};
use nimbus_engine::Service;
use serde_json::{Value, json};

const KEY: &str = "AKIATEST";
const ITERS: usize = 1000;

fn signed() -> String {
    format!(
        "AWS4-HMAC-SHA256 Credential={KEY}/20260101/us-east-1/dynamodb/aws4_request, \
         SignedHeaders=host;x-amz-target, Signature=deadbeef"
    )
}

fn headers(target: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-amz-target",
        HeaderValue::from_str(&format!("DynamoDB_20120810.{target}")).expect("target"),
    );
    headers.insert(
        "authorization",
        HeaderValue::from_str(&signed()).expect("auth"),
    );
    headers
}

fn percentiles(mut samples: Vec<u128>) -> (u128, u128, u128) {
    samples.sort_unstable();
    let pick = |p: f64| {
        let idx = ((samples.len() as f64 * p) as usize).min(samples.len() - 1);
        samples[idx]
    };
    (pick(0.50), pick(0.95), pick(0.99))
}

fn main() {
    let temp = tempfile::tempdir().expect("tempdir");
    let service = Arc::new(Service::new(temp.path()).expect("service"));
    // Synthetic headers (no real SigV4 signature), so the bench drives the
    // lookup escape hatch rather than strict verification.
    let registry = AccessKeyRegistry::new()
        .bind(KEY, TenantId::new("acme").expect("tenant"))
        .with_mode(AuthMode::LookupOnly);
    let ctx = DispatchContext {
        service: &service,
        access_keys: &registry,
    };
    let call = |op: &str, body: &Value| -> (u16, Value) {
        dispatch(&ctx, &headers(op), body.to_string().as_bytes())
    };

    // ---- setup ----
    call(
        "CreateTable",
        &json!({
            "TableName": "Bench",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "BillingMode": "PAY_PER_REQUEST",
        }),
    );
    call(
        "CreateTable",
        &json!({
            "TableName": "BenchRange",
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
    let (_s, stream_created) = call(
        "CreateTable",
        &json!({
            "TableName": "BenchStream",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES" },
            "BillingMode": "PAY_PER_REQUEST",
        }),
    );
    let stream_arn = stream_created["TableDescription"]["LatestStreamArn"]
        .as_str()
        .expect("stream arn")
        .to_owned();

    // Seed data.
    call(
        "PutItem",
        &json!({ "TableName": "Bench", "Item": { "pk": { "S": "seed" }, "v": { "N": "1" } } }),
    );
    for i in 0..20 {
        call(
            "PutItem",
            &json!({ "TableName": "BenchRange", "Item": { "pk": { "S": "p" }, "sk": { "N": i.to_string() } } }),
        );
    }
    call(
        "PutItem",
        &json!({ "TableName": "BenchStream", "Item": { "pk": { "S": "a" }, "v": { "N": "1" } } }),
    );
    call(
        "DeleteItem",
        &json!({ "TableName": "BenchStream", "Key": { "pk": { "S": "a" } } }),
    );
    let (_s, desc) = call("DescribeStream", &json!({ "StreamArn": stream_arn }));
    let shard = desc["StreamDescription"]["Shards"][0]["ShardId"]
        .as_str()
        .expect("shard")
        .to_owned();
    let (_s, iter) = call(
        "GetShardIterator",
        &json!({ "StreamArn": stream_arn, "ShardId": shard, "ShardIteratorType": "TRIM_HORIZON" }),
    );
    let iterator = iter["ShardIterator"].as_str().expect("iterator").to_owned();

    // ---- benches: (label, operation, body) ----
    let cases: Vec<(&str, String, Value)> = vec![
        (
            "PutItem",
            "PutItem".into(),
            json!({ "TableName": "Bench", "Item": { "pk": { "S": "seed" }, "v": { "N": "2" } } }),
        ),
        (
            "GetItem",
            "GetItem".into(),
            json!({ "TableName": "Bench", "Key": { "pk": { "S": "seed" } } }),
        ),
        (
            "UpdateItem",
            "UpdateItem".into(),
            json!({
                "TableName": "Bench",
                "Key": { "pk": { "S": "seed" } },
                "UpdateExpression": "SET v = :v",
                "ExpressionAttributeValues": { ":v": { "N": "3" } },
            }),
        ),
        (
            "Query",
            "Query".into(),
            json!({
                "TableName": "BenchRange",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": { "S": "p" } },
            }),
        ),
        ("Scan", "Scan".into(), json!({ "TableName": "BenchRange" })),
        (
            "BatchGetItem",
            "BatchGetItem".into(),
            json!({ "RequestItems": { "Bench": { "Keys": [{ "pk": { "S": "seed" } }] } } }),
        ),
        (
            "BatchWriteItem",
            "BatchWriteItem".into(),
            json!({ "RequestItems": { "Bench": [{ "PutRequest": { "Item": { "pk": { "S": "bw" }, "v": { "N": "1" } } } }] } }),
        ),
        (
            "TransactWriteItems",
            "TransactWriteItems".into(),
            json!({ "TransactItems": [{ "Put": { "TableName": "Bench", "Item": { "pk": { "S": "tw" }, "v": { "N": "1" } } } }] }),
        ),
        (
            "GetRecords",
            "GetRecords".into(),
            json!({ "ShardIterator": iterator }),
        ),
    ];

    println!("operation,p50_us,p95_us,p99_us,iters");
    for (label, op, body) in &cases {
        let mut samples = Vec::with_capacity(ITERS);
        for _ in 0..ITERS {
            let start = Instant::now();
            let (status, _) = call(op, body);
            let elapsed = start.elapsed().as_nanos();
            // Every benched case is a valid operation: assert the *expected*
            // success status, not merely `< 500` (which a modeled 4xx would
            // silently pass), so a regression that turns a 200 into a 400 fails
            // the bench instead of skewing the latency numbers (F15).
            assert_eq!(status, 200, "{label} returned {status}");
            samples.push(elapsed);
        }
        let (p50, p95, p99) = percentiles(samples);
        println!(
            "{label},{:.1},{:.1},{:.1},{ITERS}",
            p50 as f64 / 1000.0,
            p95 as f64 / 1000.0,
            p99 as f64 / 1000.0
        );
    }
}
