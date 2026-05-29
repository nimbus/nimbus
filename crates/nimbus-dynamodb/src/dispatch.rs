//! X-Amz-Target dispatch entrypoint.
//!
//! Transport-agnostic: `nimbus-server` mounts [`dispatch`] on a `POST /` route
//! for the dedicated DynamoDB port, passing a [`DispatchContext`] that carries
//! the shared `Service` and the access-key registry. The flow mirrors real
//! DynamoDB / ExtendDB:
//!
//! 1. parse the `X-Amz-Target` operation,
//! 2. reject unknown operations *before* auth,
//! 3. reject malformed JSON bodies *before* auth,
//! 4. authenticate (lookup mode): extract the access key from the SigV4
//!    `Authorization` header and resolve it to a tenant,
//! 5. ensure the tenant exists (idempotent),
//! 6. route to the per-operation handler.
//!
//! Lookup-mode auth (D0.8) extracts and resolves the access key but does not yet
//! verify the SigV4 signature; strict verification lands in D7. Operations whose
//! handlers have not landed yet (item ops D1, Query/Scan D2, …) are recognized
//! (so the unknown-vs-known distinction stays correct) but route to a
//! `not-yet-implemented` placeholder *after* authenticating, matching AWS order.

use std::sync::Arc;

use extenddb_core::error::DynamoDbError;
use http::HeaderMap;
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;
use serde_json::Value;

use crate::auth::sigv4::parse::parse_authorization;
use crate::commands::{batch, control_plane, discovery, item, query};
use crate::tenant::{AccessKeyRegistry, ensure_tenant, tenant_context};
use crate::wire::{self, WireResponse};

/// Surface label recorded on every DynamoDB-originated tenant context.
const DISPATCH_SURFACE: &str = "DynamoDB";

/// Capabilities a dispatched request operates over: the shared engine `Service`
/// and the access-key → tenant registry. Borrowed (not owned) so the server can
/// build one per request from long-lived state without cloning.
pub struct DispatchContext<'a> {
    /// Shared engine handle every handler scopes its reads/writes through.
    pub service: &'a Arc<Service>,
    /// AWS access-key id → tenant bindings used to authenticate the request.
    pub access_keys: &'a AccessKeyRegistry,
}

/// Every DynamoDB operation the adapter targets across tiers T0–T7 (data plane,
/// Query/Scan, batch/transact, Streams, TTL, tagging). GSI/LSI changes ride on
/// `CreateTable`/`UpdateTable`; SigV4 (T7) is auth, not an operation. An operation
/// outside this set is rejected with `UnknownOperationException`.
pub const KNOWN_OPERATIONS: &[&str] = &[
    // T0 — control plane
    "CreateTable",
    "DescribeTable",
    "ListTables",
    "UpdateTable",
    "DeleteTable",
    "DescribeEndpoints",
    "DescribeLimits",
    // T1 — single-item
    "PutItem",
    "GetItem",
    "DeleteItem",
    "UpdateItem",
    // T2 — query / scan
    "Query",
    "Scan",
    // T3 — batch / transact
    "BatchGetItem",
    "BatchWriteItem",
    "TransactGetItems",
    "TransactWriteItems",
    // T5 — streams
    "DescribeStream",
    "GetShardIterator",
    "GetRecords",
    "ListStreams",
    // T6 — TTL / tagging
    "UpdateTimeToLive",
    "DescribeTimeToLive",
    "TagResource",
    "UntagResource",
    "ListTagsOfResource",
];

/// True if `operation` is a DynamoDB operation the adapter targets.
#[must_use]
pub fn is_known_operation(operation: &str) -> bool {
    KNOWN_OPERATIONS.contains(&operation)
}

/// Dispatch a DynamoDB request to its operation handler.
///
/// Returns a [`WireResponse`] `(status, body)`; `nimbus-server` turns it into an
/// HTTP response. See the module docs for the ordered flow (unknown-op and
/// malformed-body rejection precede auth; auth precedes routing).
#[must_use]
pub fn dispatch(ctx: &DispatchContext<'_>, headers: &HeaderMap, body: &[u8]) -> WireResponse {
    // 1. Parse X-Amz-Target.
    let operation = match wire::extract_operation(headers) {
        Ok(op) => op,
        Err(error) => return wire::render_error(&error),
    };

    // 2. Reject unknown operations before auth (real DynamoDB order).
    if !is_known_operation(&operation) {
        return wire::render_error(&DynamoDbError::UnknownOperationException(String::new()));
    }

    // 3. Reject malformed JSON bodies before auth.
    let request: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(error) => {
            return wire::render_error(&DynamoDbError::SerializationException(format!(
                "Start of structure or map found where not expected: {error}"
            )));
        }
    };

    // 4. Authenticate (lookup mode): access key → tenant.
    let context = match authenticate(ctx, headers) {
        Ok(context) => context,
        Err(error) => return wire::render_error(&error),
    };

    // 5. Ensure the resolved tenant exists (idempotent).
    if let Err(error) = ensure_tenant(ctx.service, &context) {
        return wire::render_error(&error);
    }

    // 6. Route to the per-operation handler. The `Host` lets DescribeEndpoints
    //    echo the address the client used.
    let host = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost");
    route(ctx, &context, &operation, request, host)
}

/// Resolve the request's tenant from the SigV4 `Authorization` header.
///
/// Lookup mode (D0.8): parse the header for its access-key id and map it to a
/// tenant. The signature is not verified here — strict verification is D7. A
/// missing header is `MissingAuthenticationToken`; a malformed header is
/// `IncompleteSignature` (from the parser); an unbound key is
/// `UnrecognizedClientException` (from the registry).
fn authenticate(
    ctx: &DispatchContext<'_>,
    headers: &HeaderMap,
) -> Result<TenantIsolationContext, DynamoDbError> {
    let header = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            DynamoDbError::MissingAuthenticationToken("Missing Authentication Token".to_owned())
        })?;
    let parsed = parse_authorization(header)?;
    let tenant = ctx.access_keys.resolve(&parsed.access_key_id)?.clone();
    Ok(tenant_context(tenant, DISPATCH_SURFACE))
}

/// Route an authenticated request to its handler. Operations without a handler
/// yet are recognized but return the not-yet-implemented placeholder.
fn route(
    ctx: &DispatchContext<'_>,
    context: &TenantIsolationContext,
    operation: &str,
    request: Value,
    host: &str,
) -> WireResponse {
    match operation {
        "CreateTable" => run(request, |input| {
            control_plane::create_table(ctx.service, context, input)
        }),
        "DescribeTable" => run(request, |input| {
            control_plane::describe_table(ctx.service, context, input)
        }),
        "DeleteTable" => run(request, |input| {
            control_plane::delete_table(ctx.service, context, input)
        }),
        "ListTables" => run(request, |input| {
            control_plane::list_tables(ctx.service, context, input)
        }),
        "UpdateTable" => run(request, |input| {
            control_plane::update_table(ctx.service, context, input)
        }),
        // Discovery ops take no meaningful input and touch no tenant data.
        "DescribeEndpoints" => render_output(&discovery::describe_endpoints(host)),
        "DescribeLimits" => render_output(&discovery::describe_limits()),
        // T1 — single-item data plane.
        "PutItem" => run(request, |input| item::put_item(ctx.service, context, input)),
        "GetItem" => run(request, |input| item::get_item(ctx.service, context, input)),
        "DeleteItem" => run(request, |input| {
            item::delete_item(ctx.service, context, input)
        }),
        "UpdateItem" => run(request, |input| {
            item::update_item(ctx.service, context, input)
        }),
        // T2 — Query / Scan.
        "Query" => run(request, |input| query::query(ctx.service, context, input)),
        "Scan" => run(request, |input| query::scan(ctx.service, context, input)),
        // T3 — batch.
        "BatchGetItem" => run(request, |input| {
            batch::batch_get_item(ctx.service, context, input)
        }),
        "BatchWriteItem" => run(request, |input| {
            batch::batch_write_item(ctx.service, context, input)
        }),
        other => wire::render_error(&DynamoDbError::InternalServerError(format!(
            "{other} is not yet implemented"
        ))),
    }
}

/// Render a serializable handler output into a success envelope, or a 500 if it
/// cannot be serialized.
fn render_output<O: serde::Serialize>(output: &O) -> WireResponse {
    match serde_json::to_value(output) {
        Ok(body) => wire::render_success(body),
        Err(error) => wire::render_error(&DynamoDbError::InternalServerError(error.to_string())),
    }
}

/// Deserialize `request` into the handler's input type, invoke the handler, and
/// serialize its output into a success envelope.
///
/// A body that is valid JSON but the wrong shape for the operation is a
/// `SerializationException` (the JSON-protocol code AWS returns for a
/// deserialize failure); semantic validation (key schema, names) is the
/// handler's job and surfaces as `ValidationException`.
fn run<I, O>(request: Value, handler: impl FnOnce(I) -> Result<O, DynamoDbError>) -> WireResponse
where
    I: serde::de::DeserializeOwned,
    O: serde::Serialize,
{
    let input = match serde_json::from_value::<I>(request) {
        Ok(input) => input,
        Err(error) => {
            return wire::render_error(&DynamoDbError::SerializationException(error.to_string()));
        }
    };
    match handler(input) {
        Ok(output) => render_output(&output),
        Err(error) => wire::render_error(&error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_core::TenantId;

    const ACCESS_KEY: &str = "AKIAACME";

    /// A `Service` + registry binding `ACCESS_KEY` → tenant `acme`. The tempdir
    /// is returned so the caller holds it for the test's lifetime.
    fn fixture() -> (tempfile::TempDir, Arc<Service>, AccessKeyRegistry) {
        let temp = tempfile::tempdir().expect("tempdir");
        let service = Arc::new(Service::new(temp.path()).expect("service"));
        let registry =
            AccessKeyRegistry::new().bind(ACCESS_KEY, TenantId::new("acme").expect("tenant"));
        (temp, service, registry)
    }

    /// A well-formed SigV4 `Authorization` header for `access_key`. The
    /// signature is arbitrary — lookup mode (D0.8) does not verify it.
    fn signed_authorization(access_key: &str) -> String {
        format!(
            "AWS4-HMAC-SHA256 Credential={access_key}/20260101/us-east-1/dynamodb/aws4_request, \
             SignedHeaders=host;x-amz-target, Signature=deadbeef"
        )
    }

    fn headers_for(operation: &str, authorization: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            "x-amz-target",
            http::HeaderValue::from_str(&format!("DynamoDB_20120810.{operation}")).unwrap(),
        );
        if let Some(auth) = authorization {
            h.insert("authorization", http::HeaderValue::from_str(auth).unwrap());
        }
        h
    }

    fn create_table_body(name: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "TableName": name,
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        }))
        .unwrap()
    }

    fn error_type(body: &Value) -> String {
        body["__type"].as_str().unwrap_or_default().to_owned()
    }

    #[test]
    fn known_operation_set_is_nonempty_and_deduped() {
        assert!(KNOWN_OPERATIONS.len() >= 26);
        let mut seen = std::collections::HashSet::new();
        for op in KNOWN_OPERATIONS {
            assert!(
                seen.insert(*op),
                "duplicate operation in KNOWN_OPERATIONS: {op}"
            );
        }
    }

    #[test]
    fn unknown_operation_rejected_before_auth() {
        let (_temp, service, registry) = fixture();
        let ctx = DispatchContext {
            service: &service,
            access_keys: &registry,
        };
        // No authorization header at all, yet the unknown op is still rejected
        // first (the missing-key auth path is never reached).
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-amz-target",
            http::HeaderValue::from_static("DynamoDB_20120810.Frobnicate"),
        );
        headers.insert(
            "authorization",
            http::HeaderValue::from_static("AWS4-HMAC-SHA256 x"),
        );
        let (status, body) = dispatch(&ctx, &headers, b"{}");
        assert_eq!(status, 400);
        assert!(
            error_type(&body).ends_with("UnknownOperationException"),
            "{body}"
        );
    }

    #[test]
    fn malformed_body_is_serialization_exception_before_auth() {
        let (_temp, service, registry) = fixture();
        let ctx = DispatchContext {
            service: &service,
            access_keys: &registry,
        };
        // Body parsing precedes auth, so even a garbage authorization header
        // does not turn this into an auth error.
        let headers = headers_for("PutItem", Some("AWS4-HMAC-SHA256 garbage"));
        let (status, body) = dispatch(&ctx, &headers, b"not json");
        assert_eq!(status, 400);
        assert!(
            error_type(&body).ends_with("SerializationException"),
            "{body}"
        );
    }

    #[test]
    fn missing_authorization_is_missing_token() {
        let (_temp, service, registry) = fixture();
        let ctx = DispatchContext {
            service: &service,
            access_keys: &registry,
        };
        let headers = headers_for("CreateTable", None);
        let (_status, body) = dispatch(&ctx, &headers, &create_table_body("orders"));
        assert!(
            error_type(&body).ends_with("MissingAuthenticationToken"),
            "{body}"
        );
    }

    #[test]
    fn malformed_authorization_is_incomplete_signature() {
        let (_temp, service, registry) = fixture();
        let ctx = DispatchContext {
            service: &service,
            access_keys: &registry,
        };
        let headers = headers_for("CreateTable", Some("AWS4-HMAC-SHA256 nonsense"));
        let (_status, body) = dispatch(&ctx, &headers, &create_table_body("orders"));
        assert!(error_type(&body).ends_with("IncompleteSignature"), "{body}");
    }

    #[test]
    fn unknown_access_key_is_unrecognized_client() {
        let (_temp, service, registry) = fixture();
        let ctx = DispatchContext {
            service: &service,
            access_keys: &registry,
        };
        let headers = headers_for("CreateTable", Some(&signed_authorization("AKIAUNBOUND")));
        let (_status, body) = dispatch(&ctx, &headers, &create_table_body("orders"));
        assert!(
            error_type(&body).ends_with("UnrecognizedClientException"),
            "{body}"
        );
    }

    #[test]
    fn unimplemented_known_operation_returns_placeholder_after_auth() {
        // TransactGetItems is recognized but has no handler yet. With valid auth
        // it passes authentication and tenant-ensure, then hits the placeholder.
        let (_temp, service, registry) = fixture();
        let ctx = DispatchContext {
            service: &service,
            access_keys: &registry,
        };
        let headers = headers_for("TransactGetItems", Some(&signed_authorization(ACCESS_KEY)));
        let (status, body) = dispatch(&ctx, &headers, b"{}");
        assert_eq!(status, 500);
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("not yet implemented"),
            "{body}"
        );
    }

    #[test]
    fn describe_limits_returns_stub_limits_through_dispatch() {
        let (_temp, service, registry) = fixture();
        let ctx = DispatchContext {
            service: &service,
            access_keys: &registry,
        };
        let headers = headers_for("DescribeLimits", Some(&signed_authorization(ACCESS_KEY)));
        let (status, body) = dispatch(&ctx, &headers, b"{}");
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["AccountMaxReadCapacityUnits"].as_i64(), Some(80_000));
        assert_eq!(body["TableMaxWriteCapacityUnits"].as_i64(), Some(40_000));
    }

    #[test]
    fn describe_endpoints_echoes_host_through_dispatch() {
        let (_temp, service, registry) = fixture();
        let ctx = DispatchContext {
            service: &service,
            access_keys: &registry,
        };
        let mut headers = headers_for("DescribeEndpoints", Some(&signed_authorization(ACCESS_KEY)));
        headers.insert(
            "host",
            http::HeaderValue::from_static("dynamodb.local:8000"),
        );
        let (status, body) = dispatch(&ctx, &headers, b"{}");
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["Endpoints"][0]["Address"].as_str(),
            Some("dynamodb.local:8000")
        );
        assert_eq!(
            body["Endpoints"][0]["CachePeriodInMinutes"].as_i64(),
            Some(1440)
        );
    }

    #[test]
    fn create_table_succeeds_through_dispatch() {
        let (_temp, service, registry) = fixture();
        let ctx = DispatchContext {
            service: &service,
            access_keys: &registry,
        };
        let headers = headers_for("CreateTable", Some(&signed_authorization(ACCESS_KEY)));
        let (status, body) = dispatch(&ctx, &headers, &create_table_body("Orders"));
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["TableDescription"]["TableName"].as_str().unwrap(),
            "Orders"
        );
        assert_eq!(
            body["TableDescription"]["TableStatus"].as_str().unwrap(),
            "ACTIVE"
        );
    }

    #[test]
    fn missing_target_rejected_before_body() {
        let (_temp, service, registry) = fixture();
        let ctx = DispatchContext {
            service: &service,
            access_keys: &registry,
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            http::HeaderValue::from_static("AWS4-HMAC-SHA256 x"),
        );
        // Missing target (with auth present) is decided before body parsing, so
        // the malformed body never surfaces as SerializationException.
        let (status, body) = dispatch(&ctx, &headers, b"not json");
        assert_eq!(status, 400);
        assert!(
            error_type(&body).ends_with("UnknownOperationException"),
            "{body}"
        );
    }

    #[test]
    fn two_access_keys_isolate_tenants_through_dispatch() {
        // The trust-critical end-to-end isolation check: a table created under
        // one AWS access key is invisible to a request authenticated with a
        // different access key (a different tenant), and visible to its own.
        let temp = tempfile::tempdir().expect("tempdir");
        let service = Arc::new(Service::new(temp.path()).expect("service"));
        let registry = AccessKeyRegistry::new()
            .bind("AKIAACME", TenantId::new("acme").expect("tenant"))
            .bind("AKIAGLOBEX", TenantId::new("globex").expect("tenant"));
        let ctx = DispatchContext {
            service: &service,
            access_keys: &registry,
        };

        // acme creates "Orders".
        let (status, _) = dispatch(
            &ctx,
            &headers_for("CreateTable", Some(&signed_authorization("AKIAACME"))),
            &create_table_body("Orders"),
        );
        assert_eq!(status, 200);

        let describe_body =
            serde_json::to_vec(&serde_json::json!({ "TableName": "Orders" })).unwrap();

        // globex cannot see acme's table.
        let (status, body) = dispatch(
            &ctx,
            &headers_for("DescribeTable", Some(&signed_authorization("AKIAGLOBEX"))),
            &describe_body,
        );
        assert_eq!(status, 400, "{body}");
        assert!(
            error_type(&body).ends_with("ResourceNotFoundException"),
            "{body}"
        );

        // acme still sees its own table.
        let (status, body) = dispatch(
            &ctx,
            &headers_for("DescribeTable", Some(&signed_authorization("AKIAACME"))),
            &describe_body,
        );
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["Table"]["TableName"].as_str().unwrap(), "Orders");
    }
}
