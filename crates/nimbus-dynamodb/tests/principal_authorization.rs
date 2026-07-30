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

/// A read rule `key` satisfies only for items whose `owner` attribute names it.
///
/// The literal is written in AttributeValue wire form (`{"S": ...}`) because
/// that is how the adapter persists every attribute — `Document.fields` holds
/// the wire JSON, not a bare scalar — so a policy on a DynamoDB-surfaced table
/// compares against the wire shape.
fn read_only_owned_by(key: &str) -> TableAccessPolicy {
    let mut read = only_access_key(key);
    read.predicates.push(AccessPredicate {
        left: AccessValue::DocumentField {
            field: "owner".to_owned(),
        },
        op: AccessOperator::Eq,
        right: AccessValue::Literal {
            value: json!({ "S": key }),
        },
    });
    TableAccessPolicy {
        read,
        ..TableAccessPolicy::default()
    }
}

/// `policy` with a term naming a document lifecycle timestamp added to its read
/// rule.
///
/// `_creationTime` is document metadata, not a stored attribute, so this term
/// can only be answered by a document that carries its real lifecycle times.
/// Every persisted document satisfies `> 0`, which is what makes it a clean
/// probe: it changes no outcome unless the evaluator cannot evaluate it at all.
fn with_creation_time_term(mut policy: TableAccessPolicy) -> TableAccessPolicy {
    policy.read.predicates.push(AccessPredicate {
        left: AccessValue::DocumentField {
            field: "_creationTime".to_owned(),
        },
        op: AccessOperator::Gt,
        right: AccessValue::Literal { value: json!(0) },
    });
    policy
}

/// A read rule admitting only documents never modified since they were created.
///
/// Both sides are document metadata, so this is answerable only against real
/// lifecycle times — and unlike a `> 0` probe it *discriminates*, which is what
/// distinguishes reconstructed times from placeholder zeros: under placeholders
/// every image would compare equal and pass.
fn read_only_unmodified() -> TableAccessPolicy {
    TableAccessPolicy {
        read: AccessRule {
            require_authenticated: true,
            predicates: vec![AccessPredicate {
                left: AccessValue::DocumentField {
                    field: "_creationTime".to_owned(),
                },
                op: AccessOperator::Eq,
                right: AccessValue::DocumentField {
                    field: "_updateTime".to_owned(),
                },
            }],
        },
        ..TableAccessPolicy::default()
    }
}

/// Create `name` with a single `pk` and a NEW_AND_OLD_IMAGES stream, returning
/// the stream ARN.
fn create_streamed_table(engine: &Arc<Engine>, registry: &AccessKeyRegistry, name: &str) -> String {
    let mut table = hash_only_table(name);
    table["StreamSpecification"] = json!({
        "StreamEnabled": true,
        "StreamViewType": "NEW_AND_OLD_IMAGES",
    });
    let (status, created) = call(engine, registry, OWNER_KEY, "CreateTable", &table);
    assert_eq!(status, 200, "create table: {created}");
    created["TableDescription"]["LatestStreamArn"]
        .as_str()
        .expect("a stream-enabled table has a stream ARN")
        .to_owned()
}

/// A TRIM_HORIZON shard iterator for `stream_arn`, obtained as `key`.
fn trim_horizon_iterator(
    engine: &Arc<Engine>,
    registry: &AccessKeyRegistry,
    key: &str,
    stream_arn: &str,
) -> String {
    let (status, described) = call(
        engine,
        registry,
        key,
        "DescribeStream",
        &json!({ "StreamArn": stream_arn }),
    );
    assert_eq!(status, 200, "describe stream as {key}: {described}");
    let shard_id = described["StreamDescription"]["Shards"][0]["ShardId"]
        .as_str()
        .expect("the single shard must be described")
        .to_owned();
    let (status, iterator) = call(
        engine,
        registry,
        key,
        "GetShardIterator",
        &json!({
            "StreamArn": stream_arn,
            "ShardId": shard_id,
            "ShardIteratorType": "TRIM_HORIZON",
        }),
    );
    assert_eq!(status, 200, "get shard iterator as {key}: {iterator}");
    iterator["ShardIterator"]
        .as_str()
        .expect("an open shard always yields an iterator")
        .to_owned()
}

/// GetRecords as `key`, returning the `pk` of each returned record and the
/// iterator to resume from.
fn get_records(
    engine: &Arc<Engine>,
    registry: &AccessKeyRegistry,
    key: &str,
    iterator: &str,
    limit: Option<i64>,
) -> (Vec<String>, String) {
    let mut request = json!({ "ShardIterator": iterator });
    if let Some(limit) = limit {
        request["Limit"] = json!(limit);
    }
    let (status, body) = call(engine, registry, key, "GetRecords", &request);
    assert_eq!(status, 200, "get records as {key}: {body}");
    let keys = body["Records"]
        .as_array()
        .expect("Records is an array")
        .iter()
        .map(|record| {
            record["dynamodb"]["Keys"]["pk"]["S"]
                .as_str()
                .expect("every record carries its key")
                .to_owned()
        })
        .collect();
    let next = body["NextShardIterator"]
        .as_str()
        .expect("the single shard never closes, so an iterator is always returned")
        .to_owned();
    (keys, next)
}

/// Put `pk` into `table` with an `owner` attribute, as `key`.
fn put_owned(
    engine: &Arc<Engine>,
    registry: &AccessKeyRegistry,
    key: &str,
    table: &str,
    pk: &str,
    owner: &str,
) {
    let (status, body) = call(
        engine,
        registry,
        key,
        "PutItem",
        &json!({
            "TableName": table,
            "Item": { "pk": { "S": pk }, "owner": { "S": owner } },
        }),
    );
    assert_eq!(status, 200, "put {pk}: {body}");
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

/// A stream record carries the item contents that changed, so GetRecords has to
/// answer the same question a read of the source table would. DynamoDB grants
/// stream access as its own IAM resource; Nimbus has no stream-permission
/// surface, so an access key's rights come from its tenant binding and the
/// table's policies — and a stream that ignored them would be a side door around
/// the table's read policy for any authenticated key on the tenant.
///
/// The adapter still reaches its own `_ddb_stream_*` event store as the adapter
/// principal. What is authorized here is the *returned records*.
#[test]
fn get_records_authorizes_returned_records_against_the_source_table() {
    let (engine, registry, _temp) = fixture();
    let stream_arn = create_streamed_table(&engine, &registry, "Streamed");
    for pk in ["a", "b", "c"] {
        let (status, body) = call(
            &engine,
            &registry,
            OWNER_KEY,
            "PutItem",
            &json!({ "TableName": "Streamed", "Item": { "pk": { "S": pk } } }),
        );
        assert_eq!(status, 200, "seed put {pk}: {body}");
    }
    // A read policy naming OWNER_KEY: OTHER_KEY cannot read the table's items.
    set_policy(&engine, "Streamed", read_only_policy(OWNER_KEY));

    let owner_iterator = trim_horizon_iterator(&engine, &registry, OWNER_KEY, &stream_arn);
    let (owner_view, _next) = get_records(&engine, &registry, OWNER_KEY, &owner_iterator, None);
    assert_eq!(
        owner_view,
        vec!["a", "b", "c"],
        "the caller the policy names reads every captured change"
    );

    // DescribeStream and GetShardIterator stay open to any authenticated key on
    // the tenant: they return stream metadata and a position, never item data.
    // The records themselves are where the policy has to bite.
    let other_iterator = trim_horizon_iterator(&engine, &registry, OTHER_KEY, &stream_arn);
    let (other_view, other_next) =
        get_records(&engine, &registry, OTHER_KEY, &other_iterator, None);
    assert!(
        other_view.is_empty(),
        "a caller the table's read policy withholds items from must not receive those items \
         back as stream records: {other_view:?}"
    );
    assert_ne!(
        other_next, other_iterator,
        "withheld records must still advance the iterator, or the caller re-reads them forever"
    );

    // The policy is real: it gates the table's items by the same rule.
    let (_status, other_scan) = call(
        &engine,
        &registry,
        OTHER_KEY,
        "Scan",
        &json!({ "TableName": "Streamed" }),
    );
    assert_eq!(
        other_scan["Count"], 0,
        "the same policy withholds the table's items from that caller: {other_scan}"
    );
}

/// The interleaved-owners paging scenario, run under `policy`.
fn run_paging_scenario(table: &str, policy: TableAccessPolicy) {
    let (engine, registry, _temp) = fixture();
    let stream_arn = create_streamed_table(&engine, &registry, table);
    // Alternating owners, so no raw page of two consecutive events ever holds
    // two the caller may read.
    for (index, owner) in [OWNER_KEY, OTHER_KEY].iter().cycle().take(6).enumerate() {
        put_owned(
            &engine,
            &registry,
            OWNER_KEY,
            table,
            &format!("i{index}"),
            owner,
        );
    }
    set_policy(&engine, table, policy);

    let iterator = trim_horizon_iterator(&engine, &registry, OWNER_KEY, &stream_arn);
    let (first, iterator) = get_records(&engine, &registry, OWNER_KEY, &iterator, Some(2));
    assert_eq!(
        first,
        vec!["i0", "i2"],
        "the page must be filled with readable records, skipping the interleaved ones a \
         policy-blind read would have returned (i0, i1)"
    );

    let (second, iterator) = get_records(&engine, &registry, OWNER_KEY, &iterator, Some(2));
    assert_eq!(
        second,
        vec!["i4"],
        "the returned iterator must resume after the last record handed back, not after the \
         last event examined: a page short only because the stream ran out"
    );

    let (third, _iterator) = get_records(&engine, &registry, OWNER_KEY, &iterator, Some(2));
    assert!(
        third.is_empty(),
        "a drained stream returns nothing further: {third:?}"
    );
}

/// Withheld records must not consume page slots. A caller asking for `Limit`
/// records gets `Limit` records it may read, not a short page it would mistake
/// for the end of the stream.
#[test]
fn get_records_fills_pages_from_the_records_the_caller_may_read() {
    run_paging_scenario("Interleaved", read_only_owned_by(OWNER_KEY));
}

/// The same paging contract under a rule that also names `_creationTime`.
///
/// A record's images are item attributes; the lifecycle times a rule like this
/// asks about are document metadata that has to be captured with them. Records
/// must flow exactly as before — a policy the evaluator cannot answer is a
/// policy whose records are silently dropped, and the iterator advances past
/// them for good.
#[test]
fn get_records_fills_pages_under_a_timestamp_referencing_policy() {
    run_paging_scenario(
        "InterleavedTimed",
        with_creation_time_term(read_only_owned_by(OWNER_KEY)),
    );
}

/// The ownership-handover straddle scenario, run under `policy`.
fn run_straddle_scenario(table: &str, policy: TableAccessPolicy) {
    let (engine, registry, _temp) = fixture();
    let stream_arn = create_streamed_table(&engine, &registry, table);
    // x is created owned by OTHER_KEY, then handed to OWNER_KEY: the MODIFY's
    // new image is readable by OWNER_KEY but its old image is not.
    put_owned(&engine, &registry, OWNER_KEY, table, "x", OTHER_KEY);
    put_owned(&engine, &registry, OWNER_KEY, table, "x", OWNER_KEY);
    put_owned(&engine, &registry, OWNER_KEY, table, "y", OWNER_KEY);
    set_policy(&engine, table, policy);

    let iterator = trim_horizon_iterator(&engine, &registry, OWNER_KEY, &stream_arn);
    let (records, _next) = get_records(&engine, &registry, OWNER_KEY, &iterator, None);
    assert_eq!(
        records,
        vec!["y"],
        "only the record whose every image is readable may be returned; authorizing on the \
         new image alone would have handed back x's handover record and with it the old \
         image of an item owned by someone else"
    );

    // The caller can read x's current state — the withheld record is about where
    // that state came from, not about x being invisible. This is also what keeps
    // the two sides comparable: the stream must be no more permissive than the
    // table read under the very same policy.
    let (_status, scan) = call(
        &engine,
        &registry,
        OWNER_KEY,
        "Scan",
        &json!({ "TableName": table }),
    );
    assert_eq!(
        scan["Count"], 2,
        "both items are currently owned by the caller and readable: {scan}"
    );
}

/// A record pairs the image it replaced with the image it wrote, so either half
/// discloses the other. When the two straddle the policy, the conservative
/// answer — withhold unless *both* are readable — is the only one that does not
/// leak.
#[test]
fn get_records_withholds_a_record_whose_images_straddle_the_policy() {
    run_straddle_scenario("Handover", read_only_owned_by(OWNER_KEY));
}

/// The conservative both-images rule survives a rule that also names a
/// lifecycle timestamp: evaluating timestamps faithfully must not turn into
/// evaluating only the new image.
#[test]
fn get_records_withholds_a_straddling_record_under_a_timestamp_policy() {
    run_straddle_scenario(
        "HandoverTimed",
        with_creation_time_term(read_only_owned_by(OWNER_KEY)),
    );
}

/// The reconstructed images must carry the *real* lifecycle history, not a
/// placeholder — so a rule that can tell a created-and-untouched document from a
/// modified one must reach the same verdict on a stream record as on the table.
///
/// `_creationTime == _updateTime` is that rule. Under placeholder times both
/// sides would be equal for every image and every record would flow; under the
/// real history the MODIFY's new image has an update time later than its
/// creation time and its record is withheld.
#[test]
fn get_records_evaluates_lifecycle_times_against_the_real_document_history() {
    let (engine, registry, _temp) = fixture();
    let stream_arn = create_streamed_table(&engine, &registry, "History");
    let put = |pk: &str, revision: &str| {
        let (status, body) = call(
            &engine,
            &registry,
            OWNER_KEY,
            "PutItem",
            &json!({
                "TableName": "History",
                "Item": { "pk": { "S": pk }, "revision": { "S": revision } },
            }),
        );
        assert_eq!(status, 200, "put {pk}: {body}");
    };
    // Commit timestamps have millisecond resolution and are assigned as
    // `max(now, previous)`, so two writes inside one millisecond would share a
    // timestamp. Separating them is what makes "modified after creation"
    // observable at all; the scan assertion below fails loudly if it did not
    // take effect, rather than passing vacuously. The second write to `a` must
    // also change the item: rewriting identical content leaves the document
    // unmodified, update time and all.
    put("a", "1");
    std::thread::sleep(std::time::Duration::from_millis(5));
    put("a", "2");
    std::thread::sleep(std::time::Duration::from_millis(5));
    put("b", "1");

    // Before any policy exists, the stream carries all three changes. This is
    // what makes the filtered read below a statement about authorization: were
    // the MODIFY simply absent, the expected result would be the same.
    let unfiltered = trim_horizon_iterator(&engine, &registry, OWNER_KEY, &stream_arn);
    let (all_records, _) = get_records(&engine, &registry, OWNER_KEY, &unfiltered, None);
    assert_eq!(
        all_records,
        vec!["a", "a", "b"],
        "the stream must carry a's INSERT, a's MODIFY, and b's INSERT"
    );

    set_policy(&engine, "History", read_only_unmodified());
    let (_status, scan) = call(
        &engine,
        &registry,
        OWNER_KEY,
        "Scan",
        &json!({ "TableName": "History" }),
    );
    assert_eq!(
        scan["Count"], 1,
        "the table read must admit only b, the item never modified since creation — if it \
         admits both, the two writes to a shared a commit timestamp and the rest of this test \
         proves nothing: {scan}"
    );

    let iterator = trim_horizon_iterator(&engine, &registry, OWNER_KEY, &stream_arn);
    let (records, _next) = get_records(&engine, &registry, OWNER_KEY, &iterator, None);
    assert_eq!(
        records,
        vec!["a", "b"],
        "the INSERT of a and the INSERT of b describe documents whose creation and update \
         times are equal, but a's MODIFY carries a new image updated after it was created — \
         reconstructing that image with placeholder times would have returned all three"
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
