//! Failure-injection + fail-closed proofs (D9.3) driven through the public
//! `dispatch` entrypoint.
//!
//! Every adversarial input — malformed JSON, an unknown operation, a missing or
//! unbound credential, an oversize key, a condition-failed transaction, and a
//! bad SigV4 signature in strict mode — must map to a *modeled* DynamoDB error
//! (a typed 4xx with an `__type`), never an unhandled 5xx, a panic, or a
//! partial-success envelope. A panic would abort the test process, so each test
//! completing is itself the "0 panics" assertion.

use std::sync::Arc;

use http::{HeaderMap, HeaderValue};
use nimbus_core::TenantId;
use nimbus_dynamodb::{AccessKeyRegistry, AuthMode, DispatchContext, dispatch};
use nimbus_engine::Service;
use serde_json::{Value, json};

const KEY: &str = "AKIATEST";

fn service() -> (Arc<Service>, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("tempdir");
    (Arc::new(Service::new(temp.path()).expect("service")), temp)
}

fn registry() -> AccessKeyRegistry {
    AccessKeyRegistry::new().bind(KEY, TenantId::new("acme").expect("tenant"))
}

fn signed_as(key: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256 Credential={key}/20260101/us-east-1/dynamodb/aws4_request, \
         SignedHeaders=host;x-amz-target, Signature=deadbeef"
    )
}

fn headers(target: &str, authorization: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-amz-target",
        HeaderValue::from_str(&format!("DynamoDB_20120810.{target}")).expect("target"),
    );
    if let Some(value) = authorization {
        headers.insert("authorization", HeaderValue::from_str(value).expect("auth"));
    }
    headers
}

fn error_type(body: &Value) -> String {
    body["__type"].as_str().unwrap_or_default().to_owned()
}

/// A modeled failure is a 4xx carrying an `__type` — never an unhandled 5xx or
/// the "not yet implemented" placeholder.
fn assert_modeled_failure(status: u16, body: &Value, expected_suffix: &str) {
    assert!(
        (400..500).contains(&status),
        "failure must be a modeled 4xx, got {status}: {body}"
    );
    let ty = error_type(body);
    assert!(
        ty.ends_with(expected_suffix),
        "expected {expected_suffix}, got __type={ty}: {body}"
    );
    let message = body["message"].as_str().unwrap_or_default();
    assert!(
        !message.contains("not yet implemented"),
        "must not hit the unimplemented placeholder: {body}"
    );
}

#[test]
fn malformed_json_body_is_serialization_exception() {
    let (svc, _t) = service();
    let reg = registry();
    let ctx = DispatchContext {
        service: &svc,
        access_keys: &reg,
    };
    let (status, body) = dispatch(
        &ctx,
        &headers("PutItem", Some(&signed_as(KEY))),
        b"{ not json",
    );
    assert_modeled_failure(status, &body, "SerializationException");
}

#[test]
fn unknown_operation_is_unknown_operation_exception() {
    let (svc, _t) = service();
    let reg = registry();
    let ctx = DispatchContext {
        service: &svc,
        access_keys: &reg,
    };
    let (status, body) = dispatch(&ctx, &headers("Frobnicate", Some(&signed_as(KEY))), b"{}");
    assert_modeled_failure(status, &body, "UnknownOperationException");
}

#[test]
fn missing_authentication_token_is_rejected() {
    let (svc, _t) = service();
    let reg = registry();
    let ctx = DispatchContext {
        service: &svc,
        access_keys: &reg,
    };
    let (status, body) = dispatch(&ctx, &headers("PutItem", None), b"{}");
    assert_modeled_failure(status, &body, "MissingAuthenticationToken");
}

#[test]
fn unbound_access_key_is_unrecognized_client() {
    let (svc, _t) = service();
    let reg = registry();
    let ctx = DispatchContext {
        service: &svc,
        access_keys: &reg,
    };
    let (status, body) = dispatch(
        &ctx,
        &headers("CreateTable", Some(&signed_as("AKIAUNBOUND"))),
        b"{}",
    );
    assert_modeled_failure(status, &body, "UnrecognizedClientException");
}

#[test]
fn oversize_partition_key_is_validation_exception() {
    let (svc, _t) = service();
    let reg = registry();
    let ctx = DispatchContext {
        service: &svc,
        access_keys: &reg,
    };
    let create = json!({
        "TableName": "Sizes",
        "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
        "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        "BillingMode": "PAY_PER_REQUEST",
    });
    let (status, _) = dispatch(
        &ctx,
        &headers("CreateTable", Some(&signed_as(KEY))),
        create.to_string().as_bytes(),
    );
    assert_eq!(status, 200);

    // A 3000-byte partition key exceeds the 1500-byte DocumentId cap (DDB-DIV-001).
    let huge = "x".repeat(3000);
    let put = json!({ "TableName": "Sizes", "Item": { "pk": { "S": huge } } });
    let (status, body) = dispatch(
        &ctx,
        &headers("PutItem", Some(&signed_as(KEY))),
        put.to_string().as_bytes(),
    );
    assert_modeled_failure(status, &body, "ValidationException");
}

#[test]
fn condition_failed_transaction_cancels_without_partial_writes() {
    let (svc, _t) = service();
    let reg = registry();
    let ctx = DispatchContext {
        service: &svc,
        access_keys: &reg,
    };
    let create = json!({
        "TableName": "Txns",
        "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
        "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        "BillingMode": "PAY_PER_REQUEST",
    });
    let _ = dispatch(
        &ctx,
        &headers("CreateTable", Some(&signed_as(KEY))),
        create.to_string().as_bytes(),
    );

    // One write would succeed, but the second's condition fails → the whole
    // transaction is canceled and neither item is written.
    let txn = json!({
        "TransactItems": [
            { "Put": { "TableName": "Txns", "Item": { "pk": { "S": "good" }, "v": { "N": "1" } } } },
            { "Put": {
                "TableName": "Txns",
                "Item": { "pk": { "S": "blocked" }, "v": { "N": "2" } },
                "ConditionExpression": "attribute_exists(pk)"
            } }
        ]
    });
    let (status, body) = dispatch(
        &ctx,
        &headers("TransactWriteItems", Some(&signed_as(KEY))),
        txn.to_string().as_bytes(),
    );
    assert_modeled_failure(status, &body, "TransactionCanceledException");

    // The first write must NOT have landed (no partial success).
    let get = json!({ "TableName": "Txns", "Key": { "pk": { "S": "good" } } });
    let (status, body) = dispatch(
        &ctx,
        &headers("GetItem", Some(&signed_as(KEY))),
        get.to_string().as_bytes(),
    );
    assert_eq!(status, 200, "GetItem: {body}");
    assert!(
        body["Item"].is_null(),
        "the aborted transaction must leave no partial write: {body}"
    );
}

#[test]
fn strict_mode_missing_amz_date_fails_closed_with_incomplete_signature() {
    // In strict SigV4 mode the request must carry a valid X-Amz-Date; its
    // absence fails closed before any handler runs. (A wrong-secret signature
    // surfacing InvalidSignatureException is proven end-to-end through the real
    // SDK in the parity runner; here we prove the fail-closed timestamp gate.)
    let (svc, _t) = service();
    let reg = AccessKeyRegistry::new()
        .bind_signed(KEY, TenantId::new("acme").expect("tenant"), "real-secret")
        .with_mode(AuthMode::Strict);
    let ctx = DispatchContext {
        service: &svc,
        access_keys: &reg,
    };
    let (status, body) = dispatch(&ctx, &headers("CreateTable", Some(&signed_as(KEY))), b"{}");
    assert_modeled_failure(status, &body, "IncompleteSignature");
}
