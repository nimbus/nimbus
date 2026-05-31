//! Tenant + auth isolation proof (D9.4).
//!
//! Two access keys bound to two tenants must not cross-read, cross-write, list,
//! TTL-configure, tag, or infer each other's tables — even when both tenants use
//! the **same** table name. Driven through the public `dispatch` entrypoint.

use std::sync::Arc;

use http::{HeaderMap, HeaderValue};
use nimbus_core::TenantId;
use nimbus_dynamodb::{AccessKeyRegistry, AuthMode, DispatchContext, dispatch};
use nimbus_engine::Service;
use serde_json::{Value, json};

const ACME_KEY: &str = "AKIAACME";
const GLOBEX_KEY: &str = "AKIAGLOBEX";

fn fixture() -> (Arc<Service>, AccessKeyRegistry, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("tempdir");
    let service = Arc::new(Service::new(temp.path()).expect("service"));
    // Synthetic-signature requests exercise tenant scoping through the lookup
    // escape hatch; cross-tenant isolation holds independently of auth mode.
    let registry = AccessKeyRegistry::new()
        .bind(ACME_KEY, TenantId::new("acme").expect("tenant"))
        .bind(GLOBEX_KEY, TenantId::new("globex").expect("tenant"))
        .with_mode(AuthMode::LookupOnly);
    (service, registry, temp)
}

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

/// Dispatch `operation` for `key` with `body`, returning `(status, json)`.
fn call(
    service: &Arc<Service>,
    registry: &AccessKeyRegistry,
    key: &str,
    operation: &str,
    body: &Value,
) -> (u16, Value) {
    let ctx = DispatchContext {
        service,
        access_keys: registry,
    };
    dispatch(&ctx, &headers(key, operation), body.to_string().as_bytes())
}

fn table(name: &str) -> Value {
    json!({
        "TableName": name,
        "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
        "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        "BillingMode": "PAY_PER_REQUEST",
    })
}

fn error_type(body: &Value) -> String {
    body["__type"].as_str().unwrap_or_default().to_owned()
}

#[test]
fn tables_and_items_are_isolated_across_tenants() {
    let (svc, reg, _t) = fixture();
    // Both tenants create a table with the SAME name and put a DIFFERENT item.
    assert_eq!(
        call(&svc, &reg, ACME_KEY, "CreateTable", &table("Shared")).0,
        200
    );
    assert_eq!(
        call(&svc, &reg, GLOBEX_KEY, "CreateTable", &table("Shared")).0,
        200
    );
    call(
        &svc,
        &reg,
        ACME_KEY,
        "PutItem",
        &json!({ "TableName": "Shared", "Item": { "pk": { "S": "k" }, "owner": { "S": "acme" } } }),
    );
    call(
        &svc,
        &reg,
        GLOBEX_KEY,
        "PutItem",
        &json!({ "TableName": "Shared", "Item": { "pk": { "S": "k" }, "owner": { "S": "globex" } } }),
    );

    // Each tenant reads back ONLY its own value — no cross-read.
    let (_s, acme_item) = call(
        &svc,
        &reg,
        ACME_KEY,
        "GetItem",
        &json!({ "TableName": "Shared", "Key": { "pk": { "S": "k" } } }),
    );
    assert_eq!(acme_item["Item"]["owner"]["S"], "acme", "{acme_item}");
    let (_s, globex_item) = call(
        &svc,
        &reg,
        GLOBEX_KEY,
        "GetItem",
        &json!({ "TableName": "Shared", "Key": { "pk": { "S": "k" } } }),
    );
    assert_eq!(globex_item["Item"]["owner"]["S"], "globex", "{globex_item}");
}

#[test]
fn one_tenants_table_is_invisible_to_another() {
    let (svc, reg, _t) = fixture();
    assert_eq!(
        call(&svc, &reg, ACME_KEY, "CreateTable", &table("Private")).0,
        200
    );

    // globex cannot describe, read, or list acme's table.
    let (status, body) = call(
        &svc,
        &reg,
        GLOBEX_KEY,
        "DescribeTable",
        &json!({ "TableName": "Private" }),
    );
    assert!(
        error_type(&body).ends_with("ResourceNotFoundException"),
        "cross-tenant describe must 404: {status} {body}"
    );

    let (_s, listed) = call(&svc, &reg, GLOBEX_KEY, "ListTables", &json!({}));
    assert!(
        !listed["TableNames"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n == "Private"),
        "another tenant's table must not appear in ListTables: {listed}"
    );

    // acme still sees its own table.
    let (status, _) = call(
        &svc,
        &reg,
        ACME_KEY,
        "DescribeTable",
        &json!({ "TableName": "Private" }),
    );
    assert_eq!(status, 200);
}

#[test]
fn ttl_and_tag_metadata_are_isolated() {
    let (svc, reg, _t) = fixture();
    assert_eq!(
        call(&svc, &reg, ACME_KEY, "CreateTable", &table("Meta")).0,
        200
    );
    assert_eq!(
        call(&svc, &reg, GLOBEX_KEY, "CreateTable", &table("Meta")).0,
        200
    );
    let arn = "arn:aws:dynamodb:ddblocal:000000000000:table/Meta";

    // acme enables TTL and tags its table.
    call(
        &svc,
        &reg,
        ACME_KEY,
        "UpdateTimeToLive",
        &json!({ "TableName": "Meta", "TimeToLiveSpecification": { "Enabled": true, "AttributeName": "ttl" } }),
    );
    call(
        &svc,
        &reg,
        ACME_KEY,
        "TagResource",
        &json!({ "ResourceArn": arn, "Tags": [{ "Key": "env", "Value": "prod" }] }),
    );

    // globex's identically-named table sees neither the TTL config nor the tags.
    let (_s, ttl) = call(
        &svc,
        &reg,
        GLOBEX_KEY,
        "DescribeTimeToLive",
        &json!({ "TableName": "Meta" }),
    );
    assert_eq!(
        ttl["TimeToLiveDescription"]["TimeToLiveStatus"], "DISABLED",
        "another tenant's TTL config must not leak: {ttl}"
    );
    let (_s, tags) = call(
        &svc,
        &reg,
        GLOBEX_KEY,
        "ListTagsOfResource",
        &json!({ "ResourceArn": arn }),
    );
    assert!(
        tags["Tags"].as_array().unwrap().is_empty(),
        "another tenant's tags must not leak: {tags}"
    );
}

#[test]
fn wrong_access_key_cannot_act_as_another_tenant() {
    let (svc, reg, _t) = fixture();
    // An access key absent from the registry is rejected outright — it cannot
    // borrow any tenant's identity.
    let (_status, body) = call(&svc, &reg, "AKIAINTRUDER", "ListTables", &json!({}));
    assert!(
        error_type(&body).ends_with("UnrecognizedClientException"),
        "an unbound key must be UnrecognizedClientException: {body}"
    );
}
