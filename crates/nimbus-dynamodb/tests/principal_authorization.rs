//! The DynamoDB surface must execute engine calls as the *authenticated
//! caller*, not as the system principal (SUC5.1).
//!
//! Every request reaching this adapter has already been authenticated by its
//! SigV4 access-key id, and that access key is the only identity the surface
//! has. If the adapter then calls the engine as `system` (or as nobody at all),
//! a table access policy cannot express who is asking: two access keys bound to
//! the same tenant become indistinguishable, and a policy written against the
//! caller admits neither of them.
//!
//! These tests pin the contract through the public `dispatch` entrypoint, using
//! two access keys bound to the **same** tenant so nothing here is provable by
//! tenant scoping alone — only a real per-caller principal can satisfy them.

use std::sync::Arc;

use http::{HeaderMap, HeaderValue};
use nimbus_core::{
    AccessOperator, AccessPredicate, AccessRule, AccessValue, PrincipalClaimSource,
    TableAccessPolicy, TableName, TableSchema, TenantId,
};
use nimbus_dynamodb::{AccessKeyRegistry, AuthMode, DispatchContext, dispatch};
use nimbus_engine::Engine;
use serde_json::{Value, json};

/// The access key a policy is written for.
const OWNER_KEY: &str = "AKIAOWNER";
/// A second access key bound to the *same* tenant. Tenant scoping cannot tell
/// it apart from `OWNER_KEY`; only the caller principal can.
const OTHER_KEY: &str = "AKIAOTHER";
const TENANT: &str = "acme";

fn fixture() -> (Arc<Engine>, AccessKeyRegistry, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("tempdir");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
    let tenant = TenantId::new(TENANT).expect("tenant");
    engine
        .create_tenant(tenant.clone())
        .expect("embedded fixture should pre-admit the tenant");
    // Synthetic signatures: this lane proves *authorization* of an already
    // authenticated caller, so it uses the lookup escape hatch rather than
    // reimplementing SigV4 signing. Strict-mode authentication has its own
    // coverage in `failure_injection.rs`.
    let registry = AccessKeyRegistry::new()
        .bind(OWNER_KEY, tenant.clone())
        .bind(OTHER_KEY, tenant)
        .with_mode(AuthMode::LookupOnly);
    (engine, registry, temp)
}

fn signed_as(key: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256 Credential={key}/20260101/us-east-1/dynamodb/aws4_request, \
         SignedHeaders=host;x-amz-target, Signature=deadbeef"
    )
}

fn headers(key: Option<&str>, target: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-amz-target",
        HeaderValue::from_str(&format!("DynamoDB_20120810.{target}")).expect("target"),
    );
    if let Some(key) = key {
        headers.insert(
            "authorization",
            HeaderValue::from_str(&signed_as(key)).expect("auth"),
        );
    }
    headers
}

fn call(
    engine: &Arc<Engine>,
    registry: &AccessKeyRegistry,
    key: &str,
    operation: &str,
    body: &Value,
) -> (u16, Value) {
    let ctx = DispatchContext {
        engine,
        access_keys: registry,
    };
    dispatch(
        &ctx,
        &headers(Some(key), operation),
        body.to_string().as_bytes(),
    )
}

fn error_type(body: &Value) -> String {
    body["__type"].as_str().unwrap_or_default().to_owned()
}

fn hash_only_table(name: &str) -> Value {
    json!({
        "TableName": name,
        "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
        "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        "BillingMode": "PAY_PER_REQUEST",
    })
}

fn hash_range_table(name: &str) -> Value {
    json!({
        "TableName": name,
        "KeySchema": [
            { "AttributeName": "pk", "KeyType": "HASH" },
            { "AttributeName": "sk", "KeyType": "RANGE" },
        ],
        "AttributeDefinitions": [
            { "AttributeName": "pk", "AttributeType": "S" },
            { "AttributeName": "sk", "AttributeType": "S" },
        ],
        "BillingMode": "PAY_PER_REQUEST",
    })
}

/// An access rule satisfied only by the caller whose access-key id is `key`.
///
/// `aws_access_key_id` is the claim the adapter puts on the principal it builds
/// from the authenticated SigV4 credential.
fn only_access_key(key: &str) -> AccessRule {
    AccessRule {
        require_authenticated: true,
        predicates: vec![AccessPredicate {
            left: AccessValue::PrincipalClaim {
                principal: PrincipalClaimSource::Identity,
                claim: "aws_access_key_id".to_owned(),
            },
            op: AccessOperator::Eq,
            right: AccessValue::Literal { value: json!(key) },
        }],
    }
}

/// Attach `policy` to the Nimbus table backing DynamoDB table `name`.
///
/// The adapter keeps its DynamoDB table metadata in its own `_ddb_catalog` and
/// never writes a Nimbus `TableSchema`, so this is the operator-side way to put
/// an access policy on a DynamoDB-surfaced table.
fn set_policy(engine: &Arc<Engine>, name: &str, policy: TableAccessPolicy) {
    let tenant = TenantId::new(TENANT).expect("tenant");
    engine
        .set_table_schema(
            &tenant,
            TableSchema {
                table: TableName::new(name).expect("table name"),
                fields: Vec::new(),
                indexes: Vec::new(),
                access_policy: Some(policy),
            },
        )
        .expect("policy should be storable");
}

fn read_only_policy(key: &str) -> TableAccessPolicy {
    TableAccessPolicy {
        read: only_access_key(key),
        ..TableAccessPolicy::default()
    }
}

#[test]
fn get_item_runs_as_the_calling_access_key() {
    let (engine, registry, _temp) = fixture();
    assert_eq!(
        call(
            &engine,
            &registry,
            OWNER_KEY,
            "CreateTable",
            &hash_only_table("Guarded")
        )
        .0,
        200
    );
    call(
        &engine,
        &registry,
        OWNER_KEY,
        "PutItem",
        &json!({ "TableName": "Guarded", "Item": { "pk": { "S": "a" }, "secret": { "S": "s" } } }),
    );
    set_policy(&engine, "Guarded", read_only_policy(OWNER_KEY));

    let get = json!({ "TableName": "Guarded", "Key": { "pk": { "S": "a" } } });

    let (_status, owner) = call(&engine, &registry, OWNER_KEY, "GetItem", &get);
    assert_eq!(
        owner["Item"]["secret"]["S"], "s",
        "the access key the read policy names must be admitted, which requires the adapter to \
         call the engine as that caller: {owner}"
    );

    let (_status, other) = call(&engine, &registry, OTHER_KEY, "GetItem", &get);
    assert!(
        other.get("Item").is_none(),
        "a different access key on the same tenant must not satisfy a policy naming another \
         caller: {other}"
    );
}

#[test]
fn query_runs_as_the_calling_access_key() {
    let (engine, registry, _temp) = fixture();
    assert_eq!(
        call(
            &engine,
            &registry,
            OWNER_KEY,
            "CreateTable",
            &hash_range_table("GuardedRange")
        )
        .0,
        200
    );
    for sk in ["1", "2"] {
        call(
            &engine,
            &registry,
            OWNER_KEY,
            "PutItem",
            &json!({
                "TableName": "GuardedRange",
                "Item": { "pk": { "S": "a" }, "sk": { "S": sk }, "secret": { "S": "s" } },
            }),
        );
    }
    set_policy(&engine, "GuardedRange", read_only_policy(OWNER_KEY));

    let query = json!({
        "TableName": "GuardedRange",
        "KeyConditionExpression": "pk = :p",
        "ExpressionAttributeValues": { ":p": { "S": "a" } },
    });

    let (_status, owner) = call(&engine, &registry, OWNER_KEY, "Query", &query);
    assert_eq!(
        owner["Count"], 2,
        "the named caller must read its own partition: {owner}"
    );

    let (_status, other) = call(&engine, &registry, OTHER_KEY, "Query", &query);
    assert_eq!(
        other["Count"], 0,
        "the partition read must enforce the table's read policy against the caller, not scan \
         storage unauthorized: {other}"
    );
}

#[test]
fn scan_runs_as_the_calling_access_key() {
    let (engine, registry, _temp) = fixture();
    assert_eq!(
        call(
            &engine,
            &registry,
            OWNER_KEY,
            "CreateTable",
            &hash_only_table("GuardedScan")
        )
        .0,
        200
    );
    call(
        &engine,
        &registry,
        OWNER_KEY,
        "PutItem",
        &json!({ "TableName": "GuardedScan", "Item": { "pk": { "S": "a" } } }),
    );
    set_policy(&engine, "GuardedScan", read_only_policy(OWNER_KEY));

    let scan = json!({ "TableName": "GuardedScan" });

    let (_status, owner) = call(&engine, &registry, OWNER_KEY, "Scan", &scan);
    assert_eq!(
        owner["Count"], 1,
        "the named caller must see its own rows in a Scan: {owner}"
    );

    let (_status, other) = call(&engine, &registry, OTHER_KEY, "Scan", &scan);
    assert_eq!(
        other["Count"], 0,
        "a different access key must see nothing under a policy naming another caller: {other}"
    );
}

#[test]
fn put_item_runs_as_the_calling_access_key() {
    let (engine, registry, _temp) = fixture();
    assert_eq!(
        call(
            &engine,
            &registry,
            OWNER_KEY,
            "CreateTable",
            &hash_only_table("GuardedWrite")
        )
        .0,
        200
    );
    set_policy(
        &engine,
        "GuardedWrite",
        TableAccessPolicy {
            create: only_access_key(OWNER_KEY),
            update: only_access_key(OWNER_KEY),
            ..TableAccessPolicy::default()
        },
    );

    let put = |pk: &str| json!({ "TableName": "GuardedWrite", "Item": { "pk": { "S": pk } } });

    let (status, owner) = call(&engine, &registry, OWNER_KEY, "PutItem", &put("mine"));
    assert_eq!(
        status, 200,
        "the access key the create policy names must be able to write: {owner}"
    );

    let (_status, other) = call(&engine, &registry, OTHER_KEY, "PutItem", &put("theirs"));
    assert!(
        error_type(&other).ends_with("AccessDeniedException"),
        "a different access key must be refused by the create policy: {other}"
    );
}

#[test]
fn an_unauthenticated_request_never_reaches_the_engine() {
    let (engine, registry, _temp) = fixture();
    let ctx = DispatchContext {
        engine: &engine,
        access_keys: &registry,
    };
    let (_status, body) = dispatch(
        &ctx,
        &headers(None, "ListTables"),
        json!({}).to_string().as_bytes(),
    );
    assert!(
        error_type(&body).ends_with("MissingAuthenticationToken"),
        "a request with no credential must be rejected before any engine call: {body}"
    );
}
