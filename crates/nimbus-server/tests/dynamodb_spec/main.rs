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
use nimbus_engine::Engine;
use nimbus_server::{DynamoDbConfig, ServeOptions, serve};
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::time::{Duration, Instant, sleep};

const ACCESS_KEY: &str = "AKIATEST";
const TENANT: &str = "acme";

/// A running adapter bound to a loopback port. Tests build official SDK clients
/// against [`Fixture::client`]. The tempdir and listener task are held for the
/// fixture's lifetime; the listener is aborted on drop.
struct Fixture {
    addr: SocketAddr,
    _temp: tempfile::TempDir,
    listener: tokio::task::JoinHandle<std::io::Result<()>>,
    /// Retained engine handle so tests can configure persisted state (e.g. the
    /// D7.3 access-key store) on the same `Engine` the listener serves.
    engine: Arc<Engine>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.listener.abort();
    }
}

impl Fixture {
    /// An `aws-sdk-dynamodb` client signing as `access_key` with [`CLIENT_SECRET`],
    /// pointed at this fixture's listener. The default fixtures bind that same
    /// secret in [`AuthMode::Strict`], so the SDK's real SigV4 signature verifies.
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
                CLIENT_SECRET,
                None,
                None,
                "dynamodb_spec",
            ))
            .build();
        Client::from_conf(config)
    }

    /// An `aws-sdk-dynamodbstreams` client pointed at the same listener. The
    /// streams data plane shares the endpoint and access-key auth; only the
    /// `X-Amz-Target` prefix differs (`DynamoDBStreams_20120810.`).
    fn streams_client(&self, access_key: &str) -> aws_sdk_dynamodbstreams::Client {
        let config = aws_sdk_dynamodbstreams::Config::builder()
            .behavior_version(aws_sdk_dynamodbstreams::config::BehaviorVersion::latest())
            .region(aws_sdk_dynamodbstreams::config::Region::new("us-east-1"))
            .endpoint_url(format!("http://{}", self.addr))
            .retry_config(aws_sdk_dynamodbstreams::config::retry::RetryConfig::disabled())
            .credentials_provider(aws_sdk_dynamodbstreams::config::Credentials::new(
                access_key.to_owned(),
                CLIENT_SECRET,
                None,
                None,
                "dynamodb_spec",
            ))
            .build();
        aws_sdk_dynamodbstreams::Client::from_conf(config)
    }
}

/// Boot a fresh adapter on `127.0.0.1:0` with the given access-key → tenant
/// bindings, in the secure-by-default [`AuthMode::Strict`] mode. Every key is
/// bound with [`CLIENT_SECRET`] — the same secret [`Fixture::client`] signs
/// with — so every parity scenario exercises real SigV4 verification end to end
/// rather than the signature-skipping lookup escape hatch.
async fn fixture_with_keys(bindings: &[(&str, &str)]) -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
    let port = reserve_loopback_port().await;
    let mut config = DynamoDbConfig::new(port).with_ttl_sweep_interval(None);
    for (key, tenant) in bindings {
        config = config.with_signed_access_key(
            *key,
            TenantId::new(*tenant).expect("valid tenant"),
            CLIENT_SECRET,
        );
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let handle = tokio::spawn(serve(
        listener,
        ServeOptions::new(Arc::clone(&engine)).with_dynamodb(config),
    ));
    wait_for_tcp_port(addr, &handle).await;
    Fixture {
        addr,
        _temp: temp,
        listener: handle,
        engine,
    }
}

async fn fixture() -> Fixture {
    fixture_with_keys(&[(ACCESS_KEY, TENANT)]).await
}

/// The secret every [`Fixture::client`] signs with (see `client`). A strict
/// fixture bound with this same secret accepts those signatures.
const CLIENT_SECRET: &str = "test-secret";

/// Boot a strict-mode adapter binding `ACCESS_KEY` to `TENANT` with `secret`.
/// In strict mode the adapter verifies the full SigV4 signature, so a client
/// signing with a different secret is rejected.
async fn fixture_strict(secret: &str) -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
    let port = reserve_loopback_port().await;
    let config = DynamoDbConfig::new(port)
        .with_signed_access_key(ACCESS_KEY, TenantId::new(TENANT).expect("tenant"), secret)
        .with_ttl_sweep_interval(None);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let handle = tokio::spawn(serve(
        listener,
        ServeOptions::new(Arc::clone(&engine)).with_dynamodb(config),
    ));
    wait_for_tcp_port(addr, &handle).await;
    Fixture {
        addr,
        _temp: temp,
        listener: handle,
        engine,
    }
}

/// Boot a strict-mode adapter with an **empty** static registry, so access keys
/// must come from the persisted store (D7.3). Returns the fixture; configure
/// keys via `fx.engine` before signing requests.
async fn fixture_strict_store_only() -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
    let port = reserve_loopback_port().await;
    let config = DynamoDbConfig::new(port).with_ttl_sweep_interval(None);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let handle = tokio::spawn(serve(
        listener,
        ServeOptions::new(Arc::clone(&engine)).with_dynamodb(config),
    ));
    wait_for_tcp_port(addr, &handle).await;
    Fixture {
        addr,
        _temp: temp,
        listener: handle,
        engine,
    }
}

async fn reserve_loopback_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral listener should bind");
    listener
        .local_addr()
        .expect("ephemeral listener address should resolve")
        .port()
}

async fn wait_for_tcp_port(
    addr: SocketAddr,
    server: &tokio::task::JoinHandle<std::io::Result<()>>,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            !server.is_finished(),
            "Nimbus server exited before DynamoDB listener accepted connections"
        );
        match TcpStream::connect(addr).await {
            Ok(_) => return,
            Err(error) if Instant::now() < deadline => {
                sleep(Duration::from_millis(25)).await;
                drop(error);
            }
            Err(error) => panic!("DynamoDB listener at {addr} did not become ready: {error}"),
        }
    }
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
async fn batch_write_item_through_official_sdk() {
    use aws_sdk_dynamodb::types::{DeleteRequest, PutRequest, WriteRequest};

    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_orders(&client).await;
    client
        .put_item()
        .table_name("Orders")
        .item("pk", AttributeValue::S("old".into()))
        .send()
        .await
        .expect("seed");

    let writes = vec![
        WriteRequest::builder()
            .put_request(
                PutRequest::builder()
                    .item("pk", AttributeValue::S("a".into()))
                    .build()
                    .expect("put a"),
            )
            .build(),
        WriteRequest::builder()
            .delete_request(
                DeleteRequest::builder()
                    .key("pk", AttributeValue::S("old".into()))
                    .build()
                    .expect("delete old"),
            )
            .build(),
    ];
    let out = client
        .batch_write_item()
        .request_items("Orders", writes)
        .send()
        .await
        .expect("batch_write_item");
    assert!(
        out.unprocessed_items()
            .is_none_or(std::collections::HashMap::is_empty),
        "store applies every op"
    );

    // The put landed and the delete removed `old`.
    let got = client
        .get_item()
        .table_name("Orders")
        .key("pk", AttributeValue::S("a".into()))
        .send()
        .await
        .expect("get a");
    assert!(got.item().is_some());
    let gone = client
        .get_item()
        .table_name("Orders")
        .key("pk", AttributeValue::S("old".into()))
        .send()
        .await
        .expect("get old");
    assert!(gone.item().is_none(), "old was deleted");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transact_get_items_through_official_sdk() {
    use aws_sdk_dynamodb::types::{Get, TransactGetItem};
    use std::collections::HashMap;

    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_orders(&client).await;
    client
        .put_item()
        .table_name("Orders")
        .item("pk", AttributeValue::S("a".into()))
        .item("v", AttributeValue::N("1".into()))
        .send()
        .await
        .expect("put");

    let get = |pk: &str| -> TransactGetItem {
        TransactGetItem::builder()
            .get(
                Get::builder()
                    .table_name("Orders")
                    .key("pk", AttributeValue::S(pk.into()))
                    .build()
                    .expect("get"),
            )
            .build()
    };
    let out = client
        .transact_get_items()
        .transact_items(get("a"))
        .transact_items(get("missing"))
        .send()
        .await
        .expect("transact_get_items");
    let responses = out.responses();
    assert_eq!(responses.len(), 2, "one response per request, in order");
    assert_eq!(
        responses[0].item().and_then(|i| i.get("v")),
        Some(&AttributeValue::N("1".into()))
    );
    assert!(
        responses[1].item().is_none_or(HashMap::is_empty),
        "missing item absent"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transact_write_items_through_official_sdk() {
    use aws_sdk_dynamodb::types::{ConditionCheck, Put, TransactWriteItem};

    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_orders(&client).await;
    client
        .put_item()
        .table_name("Orders")
        .item("pk", AttributeValue::S("guard".into()))
        .send()
        .await
        .expect("seed guard");

    // ConditionCheck (guard exists) gates a Put — both apply atomically.
    client
        .transact_write_items()
        .transact_items(
            TransactWriteItem::builder()
                .condition_check(
                    ConditionCheck::builder()
                        .table_name("Orders")
                        .key("pk", AttributeValue::S("guard".into()))
                        .condition_expression("attribute_exists(pk)")
                        .build()
                        .expect("condition check"),
                )
                .build(),
        )
        .transact_items(
            TransactWriteItem::builder()
                .put(
                    Put::builder()
                        .table_name("Orders")
                        .item("pk", AttributeValue::S("new".into()))
                        .build()
                        .expect("put"),
                )
                .build(),
        )
        .send()
        .await
        .expect("transact write should commit");
    let got = client
        .get_item()
        .table_name("Orders")
        .key("pk", AttributeValue::S("new".into()))
        .send()
        .await
        .expect("get new");
    assert!(got.item().is_some(), "gated put applied");

    // A failing condition cancels the whole transaction — the sibling Put of
    // `doomed` must not land.
    let err = client
        .transact_write_items()
        .transact_items(
            TransactWriteItem::builder()
                .put(
                    Put::builder()
                        .table_name("Orders")
                        .item("pk", AttributeValue::S("doomed".into()))
                        .build()
                        .expect("put"),
                )
                .build(),
        )
        .transact_items(
            TransactWriteItem::builder()
                .put(
                    Put::builder()
                        .table_name("Orders")
                        .item("pk", AttributeValue::S("guard".into()))
                        .condition_expression("attribute_not_exists(pk)")
                        .build()
                        .expect("put"),
                )
                .build(),
        )
        .send()
        .await
        .expect_err("transaction should cancel");
    assert!(
        err.into_service_error().is_transaction_canceled_exception(),
        "a failing condition must return TransactionCanceledException"
    );
    let doomed = client
        .get_item()
        .table_name("Orders")
        .key("pk", AttributeValue::S("doomed".into()))
        .send()
        .await
        .expect("get doomed");
    assert!(doomed.item().is_none(), "no partial write on cancellation");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_secondary_index_query_through_official_sdk() {
    use aws_sdk_dynamodb::types::{LocalSecondaryIndex, Projection, ProjectionType};

    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    let ks = |name: &str, kt: KeyType| {
        KeySchemaElement::builder()
            .attribute_name(name)
            .key_type(kt)
            .build()
            .expect("key schema element")
    };
    let attr = |name: &str, t: ScalarAttributeType| {
        AttributeDefinition::builder()
            .attribute_name(name)
            .attribute_type(t)
            .build()
            .expect("attribute definition")
    };
    client
        .create_table()
        .table_name("Tasks")
        .key_schema(ks("pk", KeyType::Hash))
        .key_schema(ks("sk", KeyType::Range))
        .attribute_definitions(attr("pk", ScalarAttributeType::S))
        .attribute_definitions(attr("sk", ScalarAttributeType::N))
        .attribute_definitions(attr("prio", ScalarAttributeType::N))
        .local_secondary_indexes(
            LocalSecondaryIndex::builder()
                .index_name("by_priority")
                .key_schema(ks("pk", KeyType::Hash))
                .key_schema(ks("prio", KeyType::Range))
                .projection(
                    Projection::builder()
                        .projection_type(ProjectionType::All)
                        .build(),
                )
                .build()
                .expect("lsi"),
        )
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await
        .expect("create Tasks with LSI");

    for (sk, prio) in [("1", "30"), ("2", "10"), ("3", "20")] {
        client
            .put_item()
            .table_name("Tasks")
            .item("pk", AttributeValue::S("p1".into()))
            .item("sk", AttributeValue::N(sk.into()))
            .item("prio", AttributeValue::N(prio.into()))
            .send()
            .await
            .expect("put");
    }

    let out = client
        .query()
        .table_name("Tasks")
        .index_name("by_priority")
        .key_condition_expression("pk = :p AND prio > :min")
        .expression_attribute_values(":p", AttributeValue::S("p1".into()))
        .expression_attribute_values(":min", AttributeValue::N("15".into()))
        .send()
        .await
        .expect("LSI query");
    let prios: Vec<String> = out
        .items()
        .iter()
        .map(|item| item.get("prio").unwrap().as_n().unwrap().clone())
        .collect();
    assert_eq!(
        prios,
        vec!["20", "30"],
        "ordered by the LSI sort key (prio)"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn global_secondary_index_crud_through_official_sdk() {
    use aws_sdk_dynamodb::types::{
        CreateGlobalSecondaryIndexAction, DeleteGlobalSecondaryIndexAction,
        GlobalSecondaryIndexUpdate, Projection, ProjectionType,
    };

    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_orders(&client).await;

    // Create a GSI on a new attribute `gsk` via UpdateTable.
    client
        .update_table()
        .table_name("Orders")
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("gsk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .expect("attr"),
        )
        .global_secondary_index_updates(
            GlobalSecondaryIndexUpdate::builder()
                .create(
                    CreateGlobalSecondaryIndexAction::builder()
                        .index_name("by_gsk")
                        .key_schema(
                            KeySchemaElement::builder()
                                .attribute_name("gsk")
                                .key_type(KeyType::Hash)
                                .build()
                                .expect("ks"),
                        )
                        .projection(
                            Projection::builder()
                                .projection_type(ProjectionType::All)
                                .build(),
                        )
                        .build()
                        .expect("create gsi action"),
                )
                .build(),
        )
        .send()
        .await
        .expect("create GSI");

    // DescribeTable shows the ACTIVE GSI.
    let described = client
        .describe_table()
        .table_name("Orders")
        .send()
        .await
        .expect("describe");
    let gsis = described.table().unwrap().global_secondary_indexes();
    assert_eq!(gsis.len(), 1);
    assert_eq!(gsis[0].index_name(), Some("by_gsk"));
    assert_eq!(gsis[0].index_status().map(|s| s.as_str()), Some("ACTIVE"));

    // Delete the GSI.
    client
        .update_table()
        .table_name("Orders")
        .global_secondary_index_updates(
            GlobalSecondaryIndexUpdate::builder()
                .delete(
                    DeleteGlobalSecondaryIndexAction::builder()
                        .index_name("by_gsk")
                        .build()
                        .expect("delete gsi action"),
                )
                .build(),
        )
        .send()
        .await
        .expect("delete GSI");
    let after = client
        .describe_table()
        .table_name("Orders")
        .send()
        .await
        .expect("describe after delete");
    assert!(
        after.table().unwrap().global_secondary_indexes().is_empty(),
        "GSI removed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gsi_query_projection_through_official_sdk() {
    use aws_sdk_dynamodb::types::{GlobalSecondaryIndex, Projection, ProjectionType};

    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    let ks = |name: &str, kt: KeyType| {
        KeySchemaElement::builder()
            .attribute_name(name)
            .key_type(kt)
            .build()
            .expect("ks")
    };
    let attr = |name: &str, t: ScalarAttributeType| {
        AttributeDefinition::builder()
            .attribute_name(name)
            .attribute_type(t)
            .build()
            .expect("attr")
    };
    client
        .create_table()
        .table_name("Catalog")
        .key_schema(ks("pk", KeyType::Hash))
        .attribute_definitions(attr("pk", ScalarAttributeType::S))
        .attribute_definitions(attr("g", ScalarAttributeType::S))
        .global_secondary_indexes(
            GlobalSecondaryIndex::builder()
                .index_name("by_g")
                .key_schema(ks("g", KeyType::Hash))
                .projection(
                    Projection::builder()
                        .projection_type(ProjectionType::KeysOnly)
                        .build(),
                )
                .build()
                .expect("gsi"),
        )
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await
        .expect("create Catalog with GSI");

    client
        .put_item()
        .table_name("Catalog")
        .item("pk", AttributeValue::S("a".into()))
        .item("g", AttributeValue::S("x".into()))
        .item("extra", AttributeValue::N("9".into()))
        .send()
        .await
        .expect("put");

    let out = client
        .query()
        .table_name("Catalog")
        .index_name("by_g")
        .key_condition_expression("g = :g")
        .expression_attribute_values(":g", AttributeValue::S("x".into()))
        .send()
        .await
        .expect("GSI query");
    let items = out.items();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert!(item.contains_key("pk") && item.contains_key("g"));
    assert!(
        !item.contains_key("extra"),
        "KEYS_ONLY GSI must not project non-key attributes"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stream_specification_through_official_sdk() {
    use aws_sdk_dynamodb::types::{StreamSpecification, StreamViewType};

    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    client
        .create_table()
        .table_name("Streamed")
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("pk")
                .key_type(KeyType::Hash)
                .build()
                .expect("ks"),
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("pk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .expect("attr"),
        )
        .stream_specification(
            StreamSpecification::builder()
                .stream_enabled(true)
                .stream_view_type(StreamViewType::NewAndOldImages)
                .build()
                .expect("stream spec"),
        )
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await
        .expect("create streamed table");

    let described = client
        .describe_table()
        .table_name("Streamed")
        .send()
        .await
        .expect("describe");
    let table = described.table().unwrap();
    let spec = table.stream_specification().expect("stream spec");
    assert!(spec.stream_enabled());
    assert_eq!(
        spec.stream_view_type(),
        Some(&StreamViewType::NewAndOldImages)
    );
    assert!(table.latest_stream_arn().is_some(), "stream ARN reported");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streams_data_plane_through_official_streams_sdk() {
    use aws_sdk_dynamodb::types::{StreamSpecification, StreamViewType};
    use aws_sdk_dynamodbstreams::types::AttributeValue as StreamAv;
    use aws_sdk_dynamodbstreams::types::{OperationType, ShardIteratorType};

    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);

    // A stream-enabled table carrying both before/after images.
    client
        .create_table()
        .table_name("Streamed")
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("pk")
                .key_type(KeyType::Hash)
                .build()
                .expect("ks"),
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("pk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .expect("attr"),
        )
        .stream_specification(
            StreamSpecification::builder()
                .stream_enabled(true)
                .stream_view_type(StreamViewType::NewAndOldImages)
                .build()
                .expect("stream spec"),
        )
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await
        .expect("create streamed table");

    // Drive INSERT → MODIFY → REMOVE on the same key through the data plane.
    client
        .put_item()
        .table_name("Streamed")
        .item("pk", AttributeValue::S("a".into()))
        .item("v", AttributeValue::N("1".into()))
        .send()
        .await
        .expect("insert");
    client
        .put_item()
        .table_name("Streamed")
        .item("pk", AttributeValue::S("a".into()))
        .item("v", AttributeValue::N("2".into()))
        .send()
        .await
        .expect("modify");
    client
        .delete_item()
        .table_name("Streamed")
        .key("pk", AttributeValue::S("a".into()))
        .send()
        .await
        .expect("remove");

    let arn = client
        .describe_table()
        .table_name("Streamed")
        .send()
        .await
        .expect("describe")
        .table()
        .and_then(|t| t.latest_stream_arn())
        .expect("stream arn")
        .to_owned();

    let streams = fx.streams_client(ACCESS_KEY);

    // ListStreams enumerates the stream, and the TableName filter narrows it.
    let listed = streams.list_streams().send().await.expect("list streams");
    assert!(
        listed
            .streams()
            .iter()
            .any(|s| s.stream_arn() == Some(arn.as_str())),
        "the stream is enumerated by ListStreams"
    );
    let filtered = streams
        .list_streams()
        .table_name("Streamed")
        .send()
        .await
        .expect("filtered list");
    assert_eq!(filtered.streams().len(), 1, "filtered to the one table");
    assert_eq!(filtered.streams()[0].table_name(), Some("Streamed"));

    // DescribeStream → single open shard (DDB-DIV-006).
    let described = streams
        .describe_stream()
        .stream_arn(&arn)
        .send()
        .await
        .expect("describe stream");
    let description = described.stream_description().expect("stream description");
    assert_eq!(description.shards().len(), 1, "single shard");
    let shard_id = description.shards()[0]
        .shard_id()
        .expect("shard id")
        .to_owned();

    // GetShardIterator(TRIM_HORIZON) + GetRecords → the three change events.
    let iterator = streams
        .get_shard_iterator()
        .stream_arn(&arn)
        .shard_id(shard_id)
        .shard_iterator_type(ShardIteratorType::TrimHorizon)
        .send()
        .await
        .expect("shard iterator")
        .shard_iterator()
        .expect("iterator")
        .to_owned();
    let got = streams
        .get_records()
        .shard_iterator(iterator)
        .send()
        .await
        .expect("get records");
    let records = got.records();
    assert_eq!(records.len(), 3, "INSERT + MODIFY + REMOVE captured");

    assert_eq!(records[0].event_name(), Some(&OperationType::Insert));
    let insert = records[0].dynamodb().expect("insert image");
    assert!(insert.old_image().is_none(), "INSERT has no old image");
    assert_eq!(
        insert.new_image().expect("new image").get("v"),
        Some(&StreamAv::N("1".into()))
    );

    assert_eq!(records[1].event_name(), Some(&OperationType::Modify));
    let modify = records[1].dynamodb().expect("modify image");
    assert_eq!(
        modify.old_image().expect("old image").get("v"),
        Some(&StreamAv::N("1".into()))
    );
    assert_eq!(
        modify.new_image().expect("new image").get("v"),
        Some(&StreamAv::N("2".into()))
    );

    assert_eq!(records[2].event_name(), Some(&OperationType::Remove));
    let remove = records[2].dynamodb().expect("remove image");
    assert!(remove.new_image().is_none(), "REMOVE has no new image");
    assert_eq!(
        remove.old_image().expect("old image").get("v"),
        Some(&StreamAv::N("2".into()))
    );

    assert!(
        got.next_shard_iterator().is_some(),
        "the open shard always returns a next iterator"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn time_to_live_through_official_sdk() {
    use aws_sdk_dynamodb::types::{TimeToLiveSpecification, TimeToLiveStatus};

    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    create_orders(&client).await;

    // Default: TTL is DISABLED with no attribute.
    let initial = client
        .describe_time_to_live()
        .table_name("Orders")
        .send()
        .await
        .expect("describe ttl")
        .time_to_live_description()
        .cloned()
        .expect("ttl description");
    assert_eq!(
        initial.time_to_live_status(),
        Some(&TimeToLiveStatus::Disabled)
    );
    assert!(initial.attribute_name().is_none());

    // Enable TTL on `expiresAt`; the response echoes the spec.
    let enabled = client
        .update_time_to_live()
        .table_name("Orders")
        .time_to_live_specification(
            TimeToLiveSpecification::builder()
                .enabled(true)
                .attribute_name("expiresAt")
                .build()
                .expect("ttl spec"),
        )
        .send()
        .await
        .expect("enable ttl")
        .time_to_live_specification()
        .cloned()
        .expect("echoed spec");
    assert!(enabled.enabled());
    assert_eq!(enabled.attribute_name(), "expiresAt");

    // DescribeTimeToLive now reports ENABLED + the attribute.
    let after = client
        .describe_time_to_live()
        .table_name("Orders")
        .send()
        .await
        .expect("describe ttl")
        .time_to_live_description()
        .cloned()
        .expect("ttl description");
    assert_eq!(
        after.time_to_live_status(),
        Some(&TimeToLiveStatus::Enabled)
    );
    assert_eq!(after.attribute_name(), Some("expiresAt"));

    // Disabling removes the reported attribute.
    client
        .update_time_to_live()
        .table_name("Orders")
        .time_to_live_specification(
            TimeToLiveSpecification::builder()
                .enabled(false)
                .attribute_name("expiresAt")
                .build()
                .expect("ttl spec"),
        )
        .send()
        .await
        .expect("disable ttl");
    let disabled = client
        .describe_time_to_live()
        .table_name("Orders")
        .send()
        .await
        .expect("describe ttl")
        .time_to_live_description()
        .cloned()
        .expect("ttl description");
    assert_eq!(
        disabled.time_to_live_status(),
        Some(&TimeToLiveStatus::Disabled)
    );
    assert!(disabled.attribute_name().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tagging_through_official_sdk() {
    use aws_sdk_dynamodb::types::Tag;

    let fx = fixture().await;
    let client = fx.client(ACCESS_KEY);
    let arn = create_orders(&client)
        .await
        .table_description()
        .and_then(|t| t.table_arn())
        .expect("table arn")
        .to_owned();

    // No tags initially.
    let initial = client
        .list_tags_of_resource()
        .resource_arn(&arn)
        .send()
        .await
        .expect("list tags");
    assert!(initial.tags().is_empty());

    // Tag, then read them back.
    client
        .tag_resource()
        .resource_arn(&arn)
        .tags(
            Tag::builder()
                .key("env")
                .value("prod")
                .build()
                .expect("tag"),
        )
        .tags(
            Tag::builder()
                .key("team")
                .value("payments")
                .build()
                .expect("tag"),
        )
        .send()
        .await
        .expect("tag resource");
    let mut tagged: Vec<(String, String)> = client
        .list_tags_of_resource()
        .resource_arn(&arn)
        .send()
        .await
        .expect("list tags")
        .tags()
        .iter()
        .map(|tag| (tag.key().to_owned(), tag.value().to_owned()))
        .collect();
    tagged.sort();
    assert_eq!(
        tagged,
        vec![
            ("env".to_owned(), "prod".to_owned()),
            ("team".to_owned(), "payments".to_owned())
        ]
    );

    // Untag one key.
    client
        .untag_resource()
        .resource_arn(&arn)
        .tag_keys("env")
        .send()
        .await
        .expect("untag resource");
    let remaining = client
        .list_tags_of_resource()
        .resource_arn(&arn)
        .send()
        .await
        .expect("list tags");
    assert_eq!(remaining.tags().len(), 1);
    assert_eq!(remaining.tags()[0].key(), "team");
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn strict_mode_accepts_a_correctly_signed_request() {
    // D7.1: under strict SigV4, a request signed by the real aws-sdk-rust with
    // the matching secret must verify end-to-end — proving the adapter's
    // canonical request, derived-key chain, and signature comparison all agree
    // with the official SDK signer.
    let fx = fixture_strict(CLIENT_SECRET).await;
    let created = create_orders(&fx.client(ACCESS_KEY)).await;
    assert_eq!(
        created.table_description().and_then(|t| t.table_name()),
        Some("Orders"),
        "a correctly-signed request is accepted in strict mode"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn strict_mode_rejects_a_wrong_secret() {
    // D7.2: strict mode bound with a different secret than the client signs with
    // → the signatures cannot match → InvalidSignatureException.
    let fx = fixture_strict("a-different-secret").await;
    let err = fx
        .client(ACCESS_KEY)
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
        .expect_err("a mis-signed request must be rejected in strict mode");
    assert_eq!(
        err.into_service_error().code(),
        Some("InvalidSignatureException"),
        "a signature computed with the wrong secret is InvalidSignatureException"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn strict_mode_still_isolates_tenants() {
    // Strict verification does not weaken tenant isolation: the verified key is
    // still scoped to exactly its bound tenant.
    let fx = fixture_strict(CLIENT_SECRET).await;
    let client = fx.client(ACCESS_KEY);
    create_orders(&client).await;
    let listed = client.list_tables().send().await.expect("list");
    assert!(listed.table_names().contains(&"Orders".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persisted_signed_key_authenticates_and_rotates_in_strict_mode() {
    // D7.3: a key configured only in the persisted store (empty static registry)
    // authenticates under strict SigV4, and rotating its secret immediately
    // invalidates signatures made with the old secret — no restart.
    let fx = fixture_strict_store_only().await;
    nimbus_dynamodb::put_access_key(
        &fx.engine,
        ACCESS_KEY,
        &TenantId::new(TENANT).expect("tenant"),
        Some(CLIENT_SECRET.to_owned()),
        Some("us-east-1".to_owned()),
    )
    .expect("configure persisted key");

    // The client signs with CLIENT_SECRET, matching the stored secret → verifies.
    let created = create_orders(&fx.client(ACCESS_KEY)).await;
    assert_eq!(
        created.table_description().and_then(|t| t.table_name()),
        Some("Orders")
    );

    // Rotate the stored secret; the client still signs with the old one → reject.
    nimbus_dynamodb::rotate_secret(&fx.engine, ACCESS_KEY, "rotated-secret").expect("rotate");
    let err = fx
        .client(ACCESS_KEY)
        .list_tables()
        .send()
        .await
        .expect_err("a signature made with the rotated-away secret must be rejected");
    assert_eq!(
        err.into_service_error().code(),
        Some("InvalidSignatureException"),
        "after rotation the old secret no longer verifies"
    );
}
