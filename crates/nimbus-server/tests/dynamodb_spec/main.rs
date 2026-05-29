//! DynamoDB adapter parity runner.
//!
//! Proves the adapter through the **official AWS Rust SDK** (`aws-sdk-dynamodb`)
//! pointed at an in-process listener via an endpoint override — the same way a
//! real AWS DynamoDB customer's application talks to the service. Each test
//! boots a fresh `Service` + listener on an ephemeral loopback port, binds an
//! access key to a tenant, and drives real signed SDK calls end-to-end.
//!
//! This is the home of the end-to-end SDK proofs the completion gate requires;
//! later tiers (item CRUD, Query/Scan with `ExclusiveStartKey`/`LastEvaluatedKey`
//! pagination, batch/transact with `CancellationReasons`/`UnprocessedItems`,
//! failure injection) add scenarios here.

use std::net::SocketAddr;
use std::sync::Arc;

use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::config::retry::RetryConfig;
use aws_sdk_dynamodb::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_dynamodb::error::ProvideErrorMetadata;
use aws_sdk_dynamodb::operation::create_table::CreateTableOutput;
use aws_sdk_dynamodb::types::{
    AttributeDefinition, AttributeValue, BillingMode, KeySchemaElement, KeyType, ReturnValue,
    ScalarAttributeType, TableStatus,
};
use nimbus_core::TenantId;
use nimbus_dynamodb::AccessKeyRegistry;
use nimbus_engine::Service;
use nimbus_server::adapters_dynamodb::listener::run_listener;
use tokio::net::TcpListener;

const ACCESS_KEY: &str = "AKIATEST";
const TENANT: &str = "acme";

/// A running adapter bound to a loopback port. Tests build official SDK clients
/// against [`Fixture::client`]. The tempdir and listener task are held for the
/// fixture's lifetime; the listener is aborted on drop.
struct Fixture {
    addr: SocketAddr,
    _temp: tempfile::TempDir,
    listener: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.listener.abort();
    }
}

impl Fixture {
    /// An `aws-sdk-dynamodb` client signing as `access_key`, pointed at this
    /// fixture's listener. The secret is arbitrary — the adapter is lookup-only
    /// (D0.8); strict signature verification is D7.
    fn client(&self, access_key: &str) -> Client {
        let config = aws_sdk_dynamodb::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .endpoint_url(format!("http://{}", self.addr))
            // Deterministic + fast: a modeled error should surface on the first
            // attempt, not be masked or delayed by retries.
            .retry_config(RetryConfig::disabled())
            .credentials_provider(Credentials::new(
                access_key.to_owned(),
                "test-secret",
                None,
                None,
                "dynamodb_spec",
            ))
            .build();
        Client::from_conf(config)
    }
}

/// Boot a fresh adapter on `127.0.0.1:0` with the given access-key → tenant
/// bindings.
async fn fixture_with_keys(bindings: &[(&str, &str)]) -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let service = Arc::new(Service::new(temp.path()).expect("service"));
    let mut registry = AccessKeyRegistry::new();
    for (key, tenant) in bindings {
        registry = registry.bind(*key, TenantId::new(*tenant).expect("valid tenant"));
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let handle = tokio::spawn(run_listener(listener, service, registry));
    Fixture {
        addr,
        _temp: temp,
        listener: handle,
    }
}

async fn fixture() -> Fixture {
    fixture_with_keys(&[(ACCESS_KEY, TENANT)]).await
}

/// CreateTable "Orders" with a single HASH key `pk` (String), on-demand billing.
async fn create_orders(client: &Client) -> CreateTableOutput {
    client
        .create_table()
        .table_name("Orders")
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("pk")
                .key_type(KeyType::Hash)
                .build()
                .expect("key schema"),
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("pk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .expect("attribute definition"),
        )
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await
        .expect("create_table should succeed")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn control_plane_roundtrip_through_official_sdk() {
    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);

    // CreateTable — the SDK round-trips the TableDescription it expects.
    let created = create_orders(&client).await;
    let desc = created.table_description().expect("table description");
    assert_eq!(desc.table_name(), Some("Orders"));
    assert_eq!(desc.table_status(), Some(&TableStatus::Active));

    // DescribeTable.
    let described = client
        .describe_table()
        .table_name("Orders")
        .send()
        .await
        .expect("describe_table");
    assert_eq!(
        described.table().and_then(|t| t.table_name()),
        Some("Orders")
    );

    // ListTables.
    let listed = client.list_tables().send().await.expect("list_tables");
    assert!(
        listed.table_names().contains(&"Orders".to_string()),
        "ListTables should include Orders, got {:?}",
        listed.table_names()
    );

    // UpdateTable — enable deletion protection, then read it back.
    client
        .update_table()
        .table_name("Orders")
        .deletion_protection_enabled(true)
        .send()
        .await
        .expect("update_table");
    let after = client
        .describe_table()
        .table_name("Orders")
        .send()
        .await
        .expect("describe after update");
    assert_eq!(
        after.table().and_then(|t| t.deletion_protection_enabled()),
        Some(true),
        "deletion protection should persist across describe"
    );

    // DeleteTable — returns the description, then the table is gone.
    let deleted = client
        .delete_table()
        .table_name("Orders")
        .send()
        .await
        .expect("delete_table");
    assert_eq!(
        deleted.table_description().and_then(|t| t.table_name()),
        Some("Orders")
    );
    let missing = client
        .describe_table()
        .table_name("Orders")
        .send()
        .await
        .expect_err("describe after delete should fail");
    assert!(
        missing
            .into_service_error()
            .is_resource_not_found_exception(),
        "describe of a deleted table must be ResourceNotFoundException"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn put_item_through_official_sdk() {
    // PutItem end-to-end through the official SDK: insert, replace with
    // ReturnValues=ALL_OLD, and a create-if-absent condition that fails.
    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_orders(&client).await;

    // Fresh insert; ALL_OLD has nothing to return.
    let first = client
        .put_item()
        .table_name("Orders")
        .item("pk", AttributeValue::S("o1".into()))
        .item("v", AttributeValue::N("1".into()))
        .return_values(ReturnValue::AllOld)
        .send()
        .await
        .expect("first put_item");
    assert!(first.attributes().is_none(), "no previous item to return");

    // Overwrite; ALL_OLD returns the previous item.
    let second = client
        .put_item()
        .table_name("Orders")
        .item("pk", AttributeValue::S("o1".into()))
        .item("v", AttributeValue::N("2".into()))
        .return_values(ReturnValue::AllOld)
        .send()
        .await
        .expect("second put_item");
    let old = second.attributes().expect("previous item");
    assert_eq!(old.get("v"), Some(&AttributeValue::N("1".into())));

    // create-if-absent must fail now that the item exists.
    let err = client
        .put_item()
        .table_name("Orders")
        .item("pk", AttributeValue::S("o1".into()))
        .item("v", AttributeValue::N("3".into()))
        .condition_expression("attribute_not_exists(pk)")
        .send()
        .await
        .expect_err("create-if-absent should fail");
    assert!(
        err.into_service_error()
            .is_conditional_check_failed_exception(),
        "overwriting with attribute_not_exists must be ConditionalCheckFailedException"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn put_get_roundtrip_through_official_sdk() {
    use aws_sdk_dynamodb::primitives::Blob;
    use std::collections::HashMap;

    // The lossless-storage proof: write every typed shape through the official
    // SDK and read it back byte-for-byte, including 38-digit N precision and
    // binary, which a naive clean-JSON projection would corrupt.
    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_orders(&client).await;

    let mut nested = HashMap::new();
    nested.insert("city".to_string(), AttributeValue::S("nyc".into()));
    let big = "99999999999999999999999999999999999999"; // 38 nines

    client
        .put_item()
        .table_name("Orders")
        .item("pk", AttributeValue::S("o1".into()))
        .item("n", AttributeValue::N("42".into()))
        .item("big", AttributeValue::N(big.into()))
        .item("bin", AttributeValue::B(Blob::new(vec![0u8, 250, 7])))
        .item(
            "tags",
            AttributeValue::Ss(vec!["a".into(), "b".into(), "c".into()]),
        )
        .item("m", AttributeValue::M(nested))
        .send()
        .await
        .expect("put_item");

    let got = client
        .get_item()
        .table_name("Orders")
        .key("pk", AttributeValue::S("o1".into()))
        .send()
        .await
        .expect("get_item");
    let item = got.item().expect("item present");

    assert_eq!(item.get("n"), Some(&AttributeValue::N("42".into())));
    assert_eq!(
        item.get("big"),
        Some(&AttributeValue::N(big.into())),
        "38-digit N precision must survive the round-trip"
    );
    assert_eq!(
        item.get("bin"),
        Some(&AttributeValue::B(Blob::new(vec![0u8, 250, 7]))),
        "binary bytes must survive exactly"
    );
    // String sets are unordered; compare as sorted multisets.
    let mut tags = item.get("tags").unwrap().as_ss().unwrap().clone();
    tags.sort();
    assert_eq!(tags, vec!["a", "b", "c"]);
    assert_eq!(
        item.get("m").unwrap().as_m().unwrap().get("city"),
        Some(&AttributeValue::S("nyc".into()))
    );

    // Projection returns only the requested attributes.
    let projected = client
        .get_item()
        .table_name("Orders")
        .key("pk", AttributeValue::S("o1".into()))
        .projection_expression("n")
        .send()
        .await
        .expect("projected get_item");
    let pitem = projected.item().expect("item present");
    assert_eq!(pitem.len(), 1);
    assert!(pitem.contains_key("n"));

    // A missing key yields no Item.
    let missing = client
        .get_item()
        .table_name("Orders")
        .key("pk", AttributeValue::S("absent".into()))
        .send()
        .await
        .expect("get_item missing");
    assert!(missing.item().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_item_through_official_sdk() {
    // DeleteItem end-to-end: ALL_OLD returns the deleted item, the item is then
    // gone, and deleting an absent key is a successful no-op.
    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_orders(&client).await;

    client
        .put_item()
        .table_name("Orders")
        .item("pk", AttributeValue::S("o1".into()))
        .item("v", AttributeValue::N("7".into()))
        .send()
        .await
        .expect("put_item");

    let deleted = client
        .delete_item()
        .table_name("Orders")
        .key("pk", AttributeValue::S("o1".into()))
        .return_values(ReturnValue::AllOld)
        .send()
        .await
        .expect("delete_item");
    assert_eq!(
        deleted.attributes().and_then(|a| a.get("v")),
        Some(&AttributeValue::N("7".into()))
    );

    // The item is gone.
    let got = client
        .get_item()
        .table_name("Orders")
        .key("pk", AttributeValue::S("o1".into()))
        .send()
        .await
        .expect("get_item");
    assert!(got.item().is_none());

    // Deleting an absent key succeeds with nothing returned.
    let noop = client
        .delete_item()
        .table_name("Orders")
        .key("pk", AttributeValue::S("o1".into()))
        .return_values(ReturnValue::AllOld)
        .send()
        .await
        .expect("delete of absent key succeeds");
    assert!(noop.attributes().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn update_item_through_official_sdk() {
    // UpdateItem end-to-end: SET + ADD (on an absent number → base 0), with
    // ReturnValues=UPDATED_NEW returning only the touched attributes; the
    // mutation then reads back through GetItem.
    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_orders(&client).await;
    client
        .put_item()
        .table_name("Orders")
        .item("pk", AttributeValue::S("o1".into()))
        .item("v", AttributeValue::N("1".into()))
        .send()
        .await
        .expect("put_item");

    let updated = client
        .update_item()
        .table_name("Orders")
        .key("pk", AttributeValue::S("o1".into()))
        .update_expression("SET v = :v ADD n :i")
        .expression_attribute_values(":v", AttributeValue::N("9".into()))
        .expression_attribute_values(":i", AttributeValue::N("5".into()))
        .return_values(ReturnValue::UpdatedNew)
        .send()
        .await
        .expect("update_item");
    let changed = updated.attributes().expect("UPDATED_NEW attributes");
    assert_eq!(changed.get("v"), Some(&AttributeValue::N("9".into())));
    assert_eq!(changed.get("n"), Some(&AttributeValue::N("5".into())));
    assert!(
        !changed.contains_key("pk"),
        "only touched attributes returned"
    );

    let got = client
        .get_item()
        .table_name("Orders")
        .key("pk", AttributeValue::S("o1".into()))
        .send()
        .await
        .expect("get_item");
    let item = got.item().expect("item present");
    assert_eq!(item.get("v"), Some(&AttributeValue::N("9".into())));
    assert_eq!(item.get("n"), Some(&AttributeValue::N("5".into())));

    // Updating a key attribute is rejected with ValidationException.
    let err = client
        .update_item()
        .table_name("Orders")
        .key("pk", AttributeValue::S("o1".into()))
        .update_expression("SET pk = :p")
        .expression_attribute_values(":p", AttributeValue::S("other".into()))
        .send()
        .await
        .expect_err("key-attribute update should be rejected");
    assert_eq!(
        err.into_service_error().code(),
        Some("ValidationException"),
        "updating a key attribute must be ValidationException"
    );
}

/// Create a composite-key table "Events" (pk String HASH, sk Number RANGE).
async fn create_events(client: &Client) {
    client
        .create_table()
        .table_name("Events")
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("pk")
                .key_type(KeyType::Hash)
                .build()
                .expect("hash key"),
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("sk")
                .key_type(KeyType::Range)
                .build()
                .expect("range key"),
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("pk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .expect("pk def"),
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("sk")
                .attribute_type(ScalarAttributeType::N)
                .build()
                .expect("sk def"),
        )
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await
        .expect("create Events");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn query_through_official_sdk() {
    // Query end-to-end: a sort-key range with type-correct numeric ordering and
    // Limit/ExclusiveStartKey pagination, all through the official SDK.
    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_events(&client).await;
    for sk in ["2", "10", "100"] {
        client
            .put_item()
            .table_name("Events")
            .item("pk", AttributeValue::S("p1".into()))
            .item("sk", AttributeValue::N(sk.into()))
            .send()
            .await
            .expect("put");
    }
    // A different partition that must not appear.
    client
        .put_item()
        .table_name("Events")
        .item("pk", AttributeValue::S("other".into()))
        .item("sk", AttributeValue::N("5".into()))
        .send()
        .await
        .expect("put other");

    let sks = |out: &aws_sdk_dynamodb::operation::query::QueryOutput| -> Vec<String> {
        out.items()
            .iter()
            .map(|item| item.get("sk").unwrap().as_n().unwrap().clone())
            .collect()
    };

    // sk > 9 selects 10 and 100 (numeric, not lexicographic), in order.
    let ranged = client
        .query()
        .table_name("Events")
        .key_condition_expression("pk = :p AND sk > :min")
        .expression_attribute_values(":p", AttributeValue::S("p1".into()))
        .expression_attribute_values(":min", AttributeValue::N("9".into()))
        .send()
        .await
        .expect("query range");
    assert_eq!(sks(&ranged), vec!["10", "100"]);

    // Paginate the full partition with Limit=2.
    let page1 = client
        .query()
        .table_name("Events")
        .key_condition_expression("pk = :p")
        .expression_attribute_values(":p", AttributeValue::S("p1".into()))
        .limit(2)
        .send()
        .await
        .expect("page1");
    assert_eq!(sks(&page1), vec!["2", "10"]);
    let cursor = page1.last_evaluated_key().expect("page truncated").clone();
    let page2 = client
        .query()
        .table_name("Events")
        .key_condition_expression("pk = :p")
        .expression_attribute_values(":p", AttributeValue::S("p1".into()))
        .set_exclusive_start_key(Some(cursor))
        .send()
        .await
        .expect("page2");
    assert_eq!(sks(&page2), vec!["100"]);
    assert!(page2.last_evaluated_key().is_none(), "last page");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn query_filter_and_projection_through_official_sdk() {
    // Query with FilterExpression + ProjectionExpression: Count is post-filter,
    // ScannedCount counts all key-matched items, and items are projected.
    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_events(&client).await;
    for (sk, kind) in [("1", "a"), ("2", "b"), ("3", "a")] {
        client
            .put_item()
            .table_name("Events")
            .item("pk", AttributeValue::S("p1".into()))
            .item("sk", AttributeValue::N(sk.into()))
            .item("kind", AttributeValue::S(kind.into()))
            .send()
            .await
            .expect("put");
    }
    let out = client
        .query()
        .table_name("Events")
        .key_condition_expression("pk = :p")
        .filter_expression("kind = :k")
        .projection_expression("sk")
        .expression_attribute_values(":p", AttributeValue::S("p1".into()))
        .expression_attribute_values(":k", AttributeValue::S("a".into()))
        .send()
        .await
        .expect("query filter");
    assert_eq!(out.count(), 2, "post-filter Count");
    assert_eq!(
        out.scanned_count(),
        3,
        "ScannedCount counts key-matched items"
    );
    let items = out.items();
    assert_eq!(items.len(), 2);
    for item in items {
        assert!(item.contains_key("sk"), "projected to sk");
        assert!(!item.contains_key("kind"), "kind projected out");
    }

    // Select=COUNT omits Items.
    let counted = client
        .query()
        .table_name("Events")
        .key_condition_expression("pk = :p")
        .select(aws_sdk_dynamodb::types::Select::Count)
        .expression_attribute_values(":p", AttributeValue::S("p1".into()))
        .send()
        .await
        .expect("query count");
    assert_eq!(counted.count(), 3);
    assert!(counted.items().is_empty(), "COUNT returns no items");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scan_through_official_sdk() {
    // Scan end-to-end: full-table read, a FilterExpression (Count post-filter,
    // ScannedCount over all items), and Limit/ExclusiveStartKey pagination that
    // covers the table exactly once.
    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_events(&client).await;
    for (pk, kind) in [("p1", "a"), ("p2", "b"), ("p3", "a"), ("p4", "a")] {
        client
            .put_item()
            .table_name("Events")
            .item("pk", AttributeValue::S(pk.into()))
            .item("sk", AttributeValue::N("1".into()))
            .item("kind", AttributeValue::S(kind.into()))
            .send()
            .await
            .expect("put");
    }

    let filtered = client
        .scan()
        .table_name("Events")
        .filter_expression("kind = :k")
        .expression_attribute_values(":k", AttributeValue::S("a".into()))
        .send()
        .await
        .expect("scan filter");
    assert_eq!(filtered.count(), 3, "three kind=a items");
    assert_eq!(filtered.scanned_count(), 4, "all four scanned");

    // Paginate the whole table with Limit=2; collect every pk exactly once.
    let mut seen: Vec<String> = Vec::new();
    let mut start = None;
    loop {
        let mut req = client.scan().table_name("Events").limit(2);
        if let Some(cursor) = start.take() {
            req = req.set_exclusive_start_key(Some(cursor));
        }
        let page = req.send().await.expect("scan page");
        for item in page.items() {
            seen.push(item.get("pk").unwrap().as_s().unwrap().clone());
        }
        match page.last_evaluated_key() {
            Some(key) => start = Some(key.clone()),
            None => break,
        }
    }
    seen.sort();
    assert_eq!(
        seen,
        vec!["p1", "p2", "p3", "p4"],
        "scan covers the table once"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn batch_get_item_through_official_sdk() {
    use aws_sdk_dynamodb::types::KeysAndAttributes;
    use std::collections::HashMap;

    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_orders(&client).await;
    for pk in ["a", "b", "c"] {
        client
            .put_item()
            .table_name("Orders")
            .item("pk", AttributeValue::S(pk.into()))
            .send()
            .await
            .expect("put");
    }

    let key = |pk: &str| -> HashMap<String, AttributeValue> {
        HashMap::from([("pk".to_string(), AttributeValue::S(pk.into()))])
    };
    let requested = KeysAndAttributes::builder()
        .keys(key("a"))
        .keys(key("c"))
        .keys(key("absent"))
        .build()
        .expect("keys and attributes");
    let out = client
        .batch_get_item()
        .request_items("Orders", requested)
        .send()
        .await
        .expect("batch_get_item");
    let items = &out.responses().expect("responses")["Orders"];
    assert_eq!(items.len(), 2, "present keys only");
    let pks: std::collections::BTreeSet<String> = items
        .iter()
        .map(|item| item.get("pk").unwrap().as_s().unwrap().clone())
        .collect();
    assert_eq!(pks, ["a", "c"].iter().map(|s| (*s).to_string()).collect());
    assert!(
        out.unprocessed_keys().is_none_or(HashMap::is_empty),
        "store processes every key"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn describe_limits_through_official_sdk() {
    // DescribeLimits round-trips through the official SDK with the documented
    // default limit shape.
    let fx = fixture().await;
    let limits = fx
        .client(ACCESS_KEY)
        .describe_limits()
        .send()
        .await
        .expect("describe_limits");
    assert_eq!(limits.account_max_read_capacity_units(), Some(80_000));
    assert_eq!(limits.account_max_write_capacity_units(), Some(80_000));
    assert_eq!(limits.table_max_read_capacity_units(), Some(40_000));
    assert_eq!(limits.table_max_write_capacity_units(), Some(40_000));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_create_is_resource_in_use_through_official_sdk() {
    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_orders(&client).await;
    let err = client
        .create_table()
        .table_name("Orders")
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("pk")
                .key_type(KeyType::Hash)
                .build()
                .expect("key schema"),
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("pk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .expect("attribute definition"),
        )
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await
        .expect_err("duplicate create should fail");
    assert!(
        err.into_service_error().is_resource_in_use_exception(),
        "second CreateTable for the same name must be ResourceInUseException"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_access_keys_are_isolated_through_official_sdk() {
    // The trust-critical isolation check through the real SDK: a table created
    // under one AWS account's access key is invisible to a different account's
    // client (a different tenant), and visible to its own.
    let fx = fixture_with_keys(&[("AKIAACME", "acme"), ("AKIAGLOBEX", "globex")]).await;
    let acme = fx.client("AKIAACME");
    let globex = fx.client("AKIAGLOBEX");

    create_orders(&acme).await;

    // globex cannot see acme's table.
    let cross = globex
        .describe_table()
        .table_name("Orders")
        .send()
        .await
        .expect_err("cross-tenant describe must fail");
    assert!(
        cross.into_service_error().is_resource_not_found_exception(),
        "another tenant's table must be ResourceNotFoundException"
    );

    // acme still sees its own table.
    let own = acme
        .describe_table()
        .table_name("Orders")
        .send()
        .await
        .expect("own describe");
    assert_eq!(own.table().and_then(|t| t.table_name()), Some("Orders"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_access_key_is_unrecognized_client_through_official_sdk() {
    // A client signing with an unbound access key is rejected — proves the
    // adapter authenticates by access key, not just routes by target.
    let fx = fixture().await;
    let stranger = fx.client("AKIASTRANGER");
    let err = stranger
        .list_tables()
        .send()
        .await
        .expect_err("unbound access key must be rejected");
    let service_err = err.into_service_error();
    // UnrecognizedClientException is not a modeled ListTables error, so it
    // arrives as an unhandled error carrying the wire `__type` code in its
    // metadata. Assert that code directly.
    assert_eq!(
        service_err.code(),
        Some("UnrecognizedClientException"),
        "unbound key must map to UnrecognizedClientException, got {service_err:?}"
    );
}
