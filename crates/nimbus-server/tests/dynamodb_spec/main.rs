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
