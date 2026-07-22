//! X-Amz-Target dispatch entrypoint.
//!
//! Transport-agnostic: `nimbus-server` mounts [`dispatch`] on a `POST /` route
//! for the dedicated DynamoDB port, passing a [`DispatchContext`] that carries
//! the shared `Engine` and the access-key registry. The flow mirrors real
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
use nimbus_core::TenantId;
use nimbus_engine::Engine;
use nimbus_tenant::TenantIsolationContext;
use serde_json::Value;

use crate::auth::sigv4::parse::{ParsedAuthorization, parse_authorization};
use crate::auth::sigv4::verify;
use crate::commands::{batch, control_plane, discovery, item, query, stream, tag, transact, ttl};
use crate::error::map_core_error;
use crate::key_management;
use crate::tenant::{
    AccessKeyRegistry, AuthMode, ensure_tenant, ensure_tenant_async, tenant_context,
};
use crate::wire::{self, WireResponse};

/// Surface label recorded on every DynamoDB-originated tenant context.
const DISPATCH_SURFACE: &str = "DynamoDB";

/// Capabilities a dispatched request operates over: the shared `Engine`
/// and the access-key → tenant registry. Borrowed (not owned) so the server can
/// build one per request from long-lived state without cloning.
pub struct DispatchContext<'a> {
    /// Shared engine handle every handler scopes its reads/writes through.
    pub engine: &'a Arc<Engine>,
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

/// Dispatch a DynamoDB request after synchronous tenant admission.
///
/// This entrypoint is retained for embedded callers that already own the
/// blocking lifecycle. Provider-capable transports must call
/// [`dispatch_async`], which admits the authenticated tenant through the
/// persistence-provider lifecycle before entering the synchronous command core.
#[must_use]
pub fn dispatch(ctx: &DispatchContext<'_>, headers: &HeaderMap, body: &[u8]) -> WireResponse {
    let prepared = match prepare_dispatch(ctx, headers, body) {
        Ok(prepared) => prepared,
        Err(response) => return response,
    };
    if let Err(error) = ensure_tenant(ctx.engine, &prepared.context) {
        return wire::render_error(&error);
    }
    route_prepared(ctx, headers, prepared)
}

/// Dispatch a DynamoDB request through canonical async tenant admission.
///
/// Unknown-operation and malformed-body rejection still precede auth; auth
/// precedes tenant creation; only `AlreadyExists` is an idempotent admission
/// success.
pub async fn dispatch_async(
    ctx: &DispatchContext<'_>,
    headers: &HeaderMap,
    body: &[u8],
) -> WireResponse {
    let prepared = match prepare_dispatch_async(ctx, headers, body).await {
        Ok(prepared) => prepared,
        Err(response) => return response,
    };
    if let Err(error) = ensure_tenant_async(ctx.engine, &prepared.context).await {
        return wire::render_error(&error);
    }
    route_prepared(ctx, headers, prepared)
}

struct PreparedDispatch {
    operation: String,
    request: Value,
    context: TenantIsolationContext,
}

fn prepare_dispatch(
    ctx: &DispatchContext<'_>,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<PreparedDispatch, WireResponse> {
    let (operation, request) = parse_dispatch(headers, body)?;

    // Authenticate: access key → tenant (and, in strict mode, verify the
    // SigV4 signature against the per-key secret + timestamp window).
    let context = match authenticate(ctx, headers, body) {
        Ok(context) => context,
        Err(error) => return Err(wire::render_error(&error)),
    };

    Ok(PreparedDispatch {
        operation,
        request,
        context,
    })
}

async fn prepare_dispatch_async(
    ctx: &DispatchContext<'_>,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<PreparedDispatch, WireResponse> {
    let (operation, request) = parse_dispatch(headers, body)?;
    let context = match authenticate_async(ctx, headers, body).await {
        Ok(context) => context,
        Err(error) => return Err(wire::render_error(&error)),
    };

    Ok(PreparedDispatch {
        operation,
        request,
        context,
    })
}

fn parse_dispatch(headers: &HeaderMap, body: &[u8]) -> Result<(String, Value), WireResponse> {
    // 1. Parse X-Amz-Target.
    let operation = match wire::extract_operation(headers) {
        Ok(op) => op,
        Err(error) => return Err(wire::render_error(&error)),
    };

    // 2. Reject unknown operations before auth (real DynamoDB order).
    if !is_known_operation(&operation) {
        return Err(wire::render_error(
            &DynamoDbError::UnknownOperationException(String::new()),
        ));
    }

    // 3. Reject malformed JSON bodies before auth.
    let request: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(error) => {
            return Err(wire::render_error(&DynamoDbError::SerializationException(
                format!("Start of structure or map found where not expected: {error}"),
            )));
        }
    };

    Ok((operation, request))
}

fn route_prepared(
    ctx: &DispatchContext<'_>,
    headers: &HeaderMap,
    prepared: PreparedDispatch,
) -> WireResponse {
    // Route to the per-operation handler. The `Host` lets DescribeEndpoints
    //    echo the address the client used.
    let host = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost");
    route(
        ctx,
        &prepared.context,
        &prepared.operation,
        prepared.request,
        host,
    )
}

/// Resolve the request's tenant from the SigV4 `Authorization` header, and —
/// under [`AuthMode::Strict`] — verify the signature.
///
/// Both modes parse the header for its access-key id and resolve it to a tenant
/// binding. A missing header is `MissingAuthenticationToken`; a malformed header
/// is `IncompleteSignature` (from the parser); an unbound key is
/// `UnrecognizedClientException` (from the registry).
///
/// In `Strict` mode the request must also carry a valid `X-Amz-Date` within the
/// ±15-minute window and a signature that matches the per-key secret over the
/// canonical `POST /` request; otherwise the binding's secret being absent is a
/// configuration error surfaced as `UnrecognizedClientException`. `LookupOnly`
/// skips signature verification only when explicitly selected for local dev.
fn authenticate(
    ctx: &DispatchContext<'_>,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<TenantIsolationContext, DynamoDbError> {
    let parsed = parse_request_authorization(headers)?;
    let (tenant, secret) = resolve_binding(ctx, &parsed.access_key_id)?;
    finish_authentication(ctx, headers, body, &parsed, tenant, secret)
}

async fn authenticate_async(
    ctx: &DispatchContext<'_>,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<TenantIsolationContext, DynamoDbError> {
    let parsed = parse_request_authorization(headers)?;
    let (tenant, secret) = resolve_binding_async(ctx, &parsed.access_key_id).await?;
    finish_authentication(ctx, headers, body, &parsed, tenant, secret)
}

fn parse_request_authorization(headers: &HeaderMap) -> Result<ParsedAuthorization, DynamoDbError> {
    let header = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            DynamoDbError::MissingAuthenticationToken("Missing Authentication Token".to_owned())
        })?;
    parse_authorization(header)
}

fn finish_authentication(
    ctx: &DispatchContext<'_>,
    headers: &HeaderMap,
    body: &[u8],
    parsed: &ParsedAuthorization,
    tenant: TenantId,
    secret: Option<String>,
) -> Result<TenantIsolationContext, DynamoDbError> {
    if ctx.access_keys.mode() == AuthMode::Strict {
        let secret = secret.ok_or_else(|| {
            DynamoDbError::UnrecognizedClientException(
                "The security token included in the request is invalid.".to_owned(),
            )
        })?;
        verify::validate_timestamp(headers)?;
        // DynamoDB is always `POST /` with no query string.
        verify::verify_signature(parsed, &secret, "POST", "/", "", headers, body)?;
    }

    Ok(tenant_context(tenant, DISPATCH_SURFACE))
}

/// Resolve an access-key id to its `(tenant, secret)` binding. The static
/// in-memory registry is the fast path; on a miss the persisted key store
/// (D7.3) is consulted so runtime-configured keys authenticate without a
/// restart. An id in neither is `UnrecognizedClientException`.
/// The `UnrecognizedClientException` returned for an unknown or refused key.
fn unrecognized_client_token() -> DynamoDbError {
    DynamoDbError::UnrecognizedClientException(
        "The security token included in the request is invalid.".to_owned(),
    )
}

fn resolve_binding(
    ctx: &DispatchContext<'_>,
    access_key_id: &str,
) -> Result<(TenantId, Option<String>), DynamoDbError> {
    if let Ok(binding) = ctx.access_keys.binding(access_key_id) {
        return Ok((binding.tenant.clone(), binding.secret.clone()));
    }
    match key_management::lookup(ctx.engine, access_key_id)? {
        Some(stored) => resolve_stored_binding(stored),
        None => Err(unrecognized_client_token()),
    }
}

async fn resolve_binding_async(
    ctx: &DispatchContext<'_>,
    access_key_id: &str,
) -> Result<(TenantId, Option<String>), DynamoDbError> {
    if let Ok(binding) = ctx.access_keys.binding(access_key_id) {
        return Ok((binding.tenant.clone(), binding.secret.clone()));
    }
    match key_management::lookup_async(ctx.engine, access_key_id).await? {
        Some(stored) => resolve_stored_binding(stored),
        None => Err(unrecognized_client_token()),
    }
}

fn resolve_stored_binding(
    stored: key_management::StoredAccessKey,
) -> Result<(TenantId, Option<String>), DynamoDbError> {
    let tenant = TenantId::new(stored.tenant).map_err(map_core_error)?;
    // Defense in depth: a stored key whose tenant is reserved (e.g. a
    // pre-existing or corrupt record) must never resolve, or a request could
    // read internal stores like the access-key catalog (F6a).
    if crate::tenant::is_reserved_tenant(&tenant) {
        return Err(unrecognized_client_token());
    }
    Ok((tenant, stored.secret))
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
            control_plane::create_table(ctx.engine, context, input)
        }),
        "DescribeTable" => run(request, |input| {
            control_plane::describe_table(ctx.engine, context, input)
        }),
        "DeleteTable" => run(request, |input| {
            control_plane::delete_table(ctx.engine, context, input)
        }),
        "ListTables" => run(request, |input| {
            control_plane::list_tables(ctx.engine, context, input)
        }),
        "UpdateTable" => run(request, |input| {
            control_plane::update_table(ctx.engine, context, input)
        }),
        // Discovery ops take no meaningful input and touch no tenant data.
        "DescribeEndpoints" => render_output(&discovery::describe_endpoints(host)),
        "DescribeLimits" => render_output(&discovery::describe_limits()),
        // T1 — single-item data plane.
        "PutItem" => run(request, |input| item::put_item(ctx.engine, context, input)),
        "GetItem" => run(request, |input| item::get_item(ctx.engine, context, input)),
        "DeleteItem" => run(request, |input| {
            item::delete_item(ctx.engine, context, input)
        }),
        "UpdateItem" => run(request, |input| {
            item::update_item(ctx.engine, context, input)
        }),
        // T2 — Query / Scan.
        "Query" => run(request, |input| query::query(ctx.engine, context, input)),
        "Scan" => run(request, |input| query::scan(ctx.engine, context, input)),
        // T3 — batch.
        "BatchGetItem" => run(request, |input| {
            batch::batch_get_item(ctx.engine, context, input)
        }),
        "BatchWriteItem" => run(request, |input| {
            batch::batch_write_item(ctx.engine, context, input)
        }),
        "TransactGetItems" => run(request, |input| {
            transact::transact_get_items(ctx.engine, context, input)
        }),
        "TransactWriteItems" => run(request, |input| {
            transact::transact_write_items(ctx.engine, context, input)
        }),
        // T5 — streams.
        "DescribeStream" => run(request, |input| {
            stream::describe_stream(ctx.engine, context, input)
        }),
        "GetShardIterator" => run(request, |input| {
            stream::get_shard_iterator(ctx.engine, context, input)
        }),
        "GetRecords" => run(request, |input| {
            stream::get_records(ctx.engine, context, input)
        }),
        "ListStreams" => run(request, |input| {
            stream::list_streams(ctx.engine, context, input)
        }),
        // T6 — TTL.
        "UpdateTimeToLive" => run(request, |input| {
            ttl::update_time_to_live(ctx.engine, context, input)
        }),
        "DescribeTimeToLive" => run(request, |input| {
            ttl::describe_time_to_live(ctx.engine, context, input)
        }),
        "TagResource" => run(request, |input| {
            tag::tag_resource(ctx.engine, context, input)
        }),
        "UntagResource" => run(request, |input| {
            tag::untag_resource(ctx.engine, context, input)
        }),
        "ListTagsOfResource" => run(request, |input| {
            tag::list_tags_of_resource(ctx.engine, context, input)
        }),
        // Defensive guard: every entry in `KNOWN_OPERATIONS` is routed above, so
        // a known op never reaches here. This arm only fires if a future op is
        // added to the recognized set without a handler — surfaced loudly rather
        // than silently mis-routed. The `every_known_operation_is_routed` test
        // asserts this arm is unreachable for the current surface.
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
    /// is returned so the caller holds it for the test's lifetime. These tests
    /// drive routing with synthetic (`Signature=deadbeef`) headers, so the
    /// registry is explicit [`AuthMode::LookupOnly`] — strict-mode verification
    /// is covered by the `strict_*` tests and the parity suite.
    fn fixture() -> (tempfile::TempDir, Arc<Engine>, AccessKeyRegistry) {
        let temp = tempfile::tempdir().expect("tempdir");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
        let registry = AccessKeyRegistry::new()
            .bind(ACCESS_KEY, TenantId::new("acme").expect("tenant"))
            .with_mode(AuthMode::LookupOnly);
        (temp, engine, registry)
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
        let (_temp, engine, registry) = fixture();
        let ctx = DispatchContext {
            engine: &engine,
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
        let (_temp, engine, registry) = fixture();
        let ctx = DispatchContext {
            engine: &engine,
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
        let (_temp, engine, registry) = fixture();
        let ctx = DispatchContext {
            engine: &engine,
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
        let (_temp, engine, registry) = fixture();
        let ctx = DispatchContext {
            engine: &engine,
            access_keys: &registry,
        };
        let headers = headers_for("CreateTable", Some("AWS4-HMAC-SHA256 nonsense"));
        let (_status, body) = dispatch(&ctx, &headers, &create_table_body("orders"));
        assert!(error_type(&body).ends_with("IncompleteSignature"), "{body}");
    }

    #[test]
    fn unknown_access_key_is_unrecognized_client() {
        let (_temp, engine, registry) = fixture();
        let ctx = DispatchContext {
            engine: &engine,
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
    fn every_known_operation_is_routed() {
        // The whole T0–T6 surface now has a handler: no recognized operation
        // falls through to the "not yet implemented" guard. Each authenticated
        // op with an empty body may legitimately fail (Validation /
        // ResourceNotFound / Serialization), but must never hit the placeholder.
        let (_temp, engine, registry) = fixture();
        let ctx = DispatchContext {
            engine: &engine,
            access_keys: &registry,
        };
        for operation in KNOWN_OPERATIONS {
            let headers = headers_for(operation, Some(&signed_authorization(ACCESS_KEY)));
            let (_status, body) = dispatch(&ctx, &headers, b"{}");
            let message = body["message"].as_str().unwrap_or_default();
            assert!(
                !message.contains("not yet implemented"),
                "operation {operation} is recognized but unrouted: {body}"
            );
        }
    }

    #[test]
    fn describe_limits_returns_stub_limits_through_dispatch() {
        let (_temp, engine, registry) = fixture();
        let ctx = DispatchContext {
            engine: &engine,
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
        let (_temp, engine, registry) = fixture();
        let ctx = DispatchContext {
            engine: &engine,
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
        let (_temp, engine, registry) = fixture();
        let ctx = DispatchContext {
            engine: &engine,
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
        let (_temp, engine, registry) = fixture();
        let ctx = DispatchContext {
            engine: &engine,
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
        let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
        let registry = AccessKeyRegistry::new()
            .bind("AKIAACME", TenantId::new("acme").expect("tenant"))
            .bind("AKIAGLOBEX", TenantId::new("globex").expect("tenant"))
            .with_mode(AuthMode::LookupOnly);
        let ctx = DispatchContext {
            engine: &engine,
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

    // ---- D7.2: strict-mode rejection paths ----

    /// A strict-mode `Service` + registry binding `ACCESS_KEY` with a secret.
    fn strict_fixture() -> (tempfile::TempDir, Arc<Engine>, AccessKeyRegistry) {
        let temp = tempfile::tempdir().expect("tempdir");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
        let registry = AccessKeyRegistry::new()
            .bind_signed(ACCESS_KEY, TenantId::new("acme").expect("tenant"), "secret")
            .with_mode(AuthMode::Strict);
        (temp, engine, registry)
    }

    #[test]
    fn persisted_access_key_authenticates_via_the_store() {
        // A key absent from the static in-memory registry but configured at
        // runtime in the persisted store still authenticates and routes
        // (D7.3 — no restart needed). It scopes to its configured tenant.
        let (_temp, engine, registry) = fixture();
        key_management::put_access_key(
            &engine,
            "AKIAPERSIST",
            &TenantId::new("persisted").expect("tenant"),
            None,
            None,
        )
        .expect("configure persisted key");
        let ctx = DispatchContext {
            engine: &engine,
            access_keys: &registry,
        };
        let (status, body) = dispatch(
            &ctx,
            &headers_for("CreateTable", Some(&signed_authorization("AKIAPERSIST"))),
            &create_table_body("orders"),
        );
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["TableDescription"]["TableName"].as_str().unwrap(),
            "orders"
        );
    }

    #[test]
    fn strict_mode_is_the_default() {
        // Secure-by-default: a bare registry verifies signatures. The routing
        // fixture opts into lookup explicitly; the strict fixture stays strict.
        assert_eq!(AccessKeyRegistry::new().mode(), AuthMode::Strict);
        assert_eq!(fixture().2.mode(), AuthMode::LookupOnly);
        assert_eq!(strict_fixture().2.mode(), AuthMode::Strict);
    }

    #[test]
    fn strict_mode_missing_amz_date_is_incomplete_signature() {
        // validate_timestamp runs first in strict mode: no X-Amz-Date header is
        // an IncompleteSignature, even though the access key is bound.
        let (_temp, engine, registry) = strict_fixture();
        let ctx = DispatchContext {
            engine: &engine,
            access_keys: &registry,
        };
        let headers = headers_for("CreateTable", Some(&signed_authorization(ACCESS_KEY)));
        let (status, body) = dispatch(&ctx, &headers, &create_table_body("orders"));
        assert_eq!(status, 400, "{body}");
        assert!(error_type(&body).ends_with("IncompleteSignature"), "{body}");
    }

    #[test]
    fn strict_mode_expired_request_is_rejected() {
        // A well-formed but stale X-Amz-Date (far outside the ±15-minute window)
        // is rejected before signature comparison.
        let (_temp, engine, registry) = strict_fixture();
        let ctx = DispatchContext {
            engine: &engine,
            access_keys: &registry,
        };
        let mut headers = headers_for("CreateTable", Some(&signed_authorization(ACCESS_KEY)));
        headers.insert(
            "x-amz-date",
            http::HeaderValue::from_static("20200101T000000Z"),
        );
        headers.insert("host", http::HeaderValue::from_static("localhost:8000"));
        let (_status, body) = dispatch(&ctx, &headers, &create_table_body("orders"));
        assert!(
            error_type(&body).ends_with("UnrecognizedClientException"),
            "{body}"
        );
        assert!(
            body["message"]
                .as_str()
                .unwrap_or_default()
                .contains("expired"),
            "{body}"
        );
    }
}
