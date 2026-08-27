use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use nimbus_core::{Error, SystemWallClock, TenantId, WallClock};
use nimbus_storage::KvPut;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state::AppState;

use super::CloudflareConfig;

const STORAGE_PREFIX: &[u8] = b"cloudflare-kv\0";
const METADATA_JSON_KEY: &str = "__cloudflare_metadata_json";
const MAX_KEY_BYTES: usize = 512;
const MAX_VALUE_BYTES: usize = 25 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 1024;
const MIN_EXPIRATION_TTL_SECONDS: i64 = 60;
pub(super) const DEFAULT_LIST_LIMIT: usize = 1000;
pub(super) const MAX_LIST_LIMIT: usize = 1000;

pub(crate) fn router(config: Arc<CloudflareConfig>) -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/client/v4/accounts/{account_id}/storage/kv/namespaces/{namespace_id}/values/{*key}",
            get(get_value).put(put_value).delete(delete_value),
        )
        .route(
            "/client/v4/accounts/{account_id}/storage/kv/namespaces/{namespace_id}/metadata/{*key}",
            get(get_metadata),
        )
        .route(
            "/client/v4/accounts/{account_id}/storage/kv/namespaces/{namespace_id}/keys",
            get(list_keys),
        )
        .layer(Extension(config))
}

#[derive(Debug, Deserialize)]
struct KvValuePath {
    account_id: String,
    namespace_id: String,
    key: String,
}

#[derive(Debug, Deserialize)]
struct KvListPath {
    account_id: String,
    namespace_id: String,
}

#[derive(Debug, Deserialize)]
struct PutQuery {
    expiration: Option<i64>,
    #[serde(alias = "expirationTtl")]
    expiration_ttl: Option<i64>,
    metadata: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    prefix: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
}

async fn get_value(
    State(state): State<Arc<AppState>>,
    Extension(config): Extension<Arc<CloudflareConfig>>,
    headers: HeaderMap,
    Path(params): Path<KvValuePath>,
) -> Result<Response, KvRestError> {
    let tenant_id = authenticate(&headers, &config)?;
    ensure_tenant(&state, &tenant_id).await?;
    let namespace = resolve_namespace(&config, &params.account_id, &params.namespace_id)?;
    let storage_key = storage_key(&namespace, &params.key)?;
    let entry = state
        .engine
        .tenant_kv_get(&tenant_id, &storage_key, now_ms())
        .map_err(KvRestError::from_core)?
        .ok_or_else(|| KvRestError::not_found("KV key not found"))?;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/octet-stream")],
        entry.value,
    )
        .into_response())
}

async fn get_metadata(
    State(state): State<Arc<AppState>>,
    Extension(config): Extension<Arc<CloudflareConfig>>,
    headers: HeaderMap,
    Path(params): Path<KvValuePath>,
) -> Result<Response, KvRestError> {
    let tenant_id = authenticate(&headers, &config)?;
    ensure_tenant(&state, &tenant_id).await?;
    let namespace = resolve_namespace(&config, &params.account_id, &params.namespace_id)?;
    let storage_key = storage_key(&namespace, &params.key)?;
    let entry = state
        .engine
        .tenant_kv_get(&tenant_id, &storage_key, now_ms())
        .map_err(KvRestError::from_core)?
        .ok_or_else(|| KvRestError::not_found("KV key not found"))?;
    Ok(Json(CloudflareEnvelope::ok(decode_metadata(&entry.metadata))).into_response())
}

async fn put_value(
    State(state): State<Arc<AppState>>,
    Extension(config): Extension<Arc<CloudflareConfig>>,
    headers: HeaderMap,
    Path(params): Path<KvValuePath>,
    Query(query): Query<PutQuery>,
    body: Bytes,
) -> Result<Response, KvRestError> {
    let tenant_id = authenticate(&headers, &config)?;
    ensure_tenant(&state, &tenant_id).await?;
    let namespace = resolve_namespace(&config, &params.account_id, &params.namespace_id)?;
    if body.len() > MAX_VALUE_BYTES {
        return Err(KvRestError::bad_request(format!(
            "Workers KV values must be at most {MAX_VALUE_BYTES} bytes"
        )));
    }
    let storage_key = storage_key(&namespace, &params.key)?;
    let expire_at_ms = resolve_expire_at_ms(&query)?;
    let metadata = encode_metadata(query.metadata.as_deref())?;
    let mut put = KvPut::new(storage_key, body.to_vec());
    put.metadata = metadata;
    put.expire_at_ms = expire_at_ms;
    state
        .engine
        .tenant_kv_put(&tenant_id, put)
        .map_err(KvRestError::from_core)?;
    Ok(Json(CloudflareEnvelope::ok(json!(null))).into_response())
}

async fn delete_value(
    State(state): State<Arc<AppState>>,
    Extension(config): Extension<Arc<CloudflareConfig>>,
    headers: HeaderMap,
    Path(params): Path<KvValuePath>,
) -> Result<Response, KvRestError> {
    let tenant_id = authenticate(&headers, &config)?;
    ensure_tenant(&state, &tenant_id).await?;
    let namespace = resolve_namespace(&config, &params.account_id, &params.namespace_id)?;
    let storage_key = storage_key(&namespace, &params.key)?;
    let _ = state
        .engine
        .tenant_kv_delete(&tenant_id, &storage_key)
        .map_err(KvRestError::from_core)?;
    Ok(Json(CloudflareEnvelope::ok(json!(null))).into_response())
}

async fn list_keys(
    State(state): State<Arc<AppState>>,
    Extension(config): Extension<Arc<CloudflareConfig>>,
    headers: HeaderMap,
    Path(params): Path<KvListPath>,
    Query(query): Query<ListQuery>,
) -> Result<Response, KvRestError> {
    let tenant_id = authenticate(&headers, &config)?;
    ensure_tenant(&state, &tenant_id).await?;
    let namespace = resolve_namespace(&config, &params.account_id, &params.namespace_id)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIST_LIMIT);
    if limit > MAX_LIST_LIMIT {
        return Err(KvRestError::bad_request(format!(
            "Workers KV list limit must be at most {MAX_LIST_LIMIT}"
        )));
    }
    let prefix = storage_prefix(&namespace, query.prefix.as_deref().unwrap_or_default())?;
    let cursor = query.cursor.as_deref().map(decode_cursor).transpose()?;
    let page = state
        .engine
        .tenant_kv_scan(&tenant_id, &prefix, cursor.as_deref(), limit, now_ms())
        .map_err(KvRestError::from_core)?;
    let mut keys = Vec::with_capacity(page.entries.len());
    for entry in page.entries {
        let name = display_key(&namespace, &entry.key)?;
        let metadata = decode_metadata(&entry.metadata);
        keys.push(KvListedKey {
            name,
            expiration: entry.expire_at_ms.map(|value| value / 1000),
            metadata: (!metadata.is_null()).then_some(metadata),
        });
    }
    let cursor = page
        .next_cursor
        .map(|cursor| URL_SAFE_NO_PAD.encode(cursor));
    Ok(Json(KvListEnvelope {
        success: true,
        errors: Vec::new(),
        messages: Vec::new(),
        result: keys,
        result_info: KvListInfo {
            cursor: cursor.clone().unwrap_or_default(),
            list_complete: cursor.is_none(),
        },
    })
    .into_response())
}

fn authenticate(headers: &HeaderMap, config: &CloudflareConfig) -> Result<TenantId, KvRestError> {
    let header = headers
        .get(header::AUTHORIZATION)
        .ok_or_else(|| KvRestError::unauthorized("Cloudflare KV REST requires Authorization"))?
        .to_str()
        .map_err(|_| KvRestError::unauthorized("Authorization must be valid ASCII"))?;
    let token = header
        .strip_prefix("Bearer ")
        .ok_or_else(|| KvRestError::unauthorized("Authorization must use the Bearer scheme"))?;
    let (access_key_id, secret) = token
        .split_once(':')
        .ok_or_else(|| KvRestError::unauthorized("Bearer token must be ACCESS_KEY_ID:SECRET"))?;
    let binding = config
        .access_keys()
        .binding(access_key_id)
        .map_err(|_| KvRestError::unauthorized("Cloudflare KV credential is not recognized"))?;
    if binding.secret.as_deref() != Some(secret) {
        return Err(KvRestError::unauthorized(
            "Cloudflare KV credential secret is invalid",
        ));
    }
    Ok(binding.tenant.clone())
}

async fn ensure_tenant(state: &Arc<AppState>, tenant_id: &TenantId) -> Result<(), KvRestError> {
    state
        .engine
        .ensure_tenant_ready_async(tenant_id.clone())
        .await
        .map(|_| ())
        .map_err(KvRestError::from_core)
}

fn resolve_namespace(
    config: &CloudflareConfig,
    account_id: &str,
    namespace_id: &str,
) -> Result<String, KvRestError> {
    if account_id.trim().is_empty() {
        return Err(KvRestError::bad_request(
            "Cloudflare account id is required",
        ));
    }
    if namespace_id.trim().is_empty() {
        return Err(KvRestError::bad_request("KV namespace id is required"));
    }
    resolve_worker_namespace(config, namespace_id)
}

pub(super) fn resolve_worker_namespace(
    config: &CloudflareConfig,
    namespace_id: &str,
) -> Result<String, KvRestError> {
    if namespace_id.trim().is_empty() {
        return Err(KvRestError::bad_request("KV namespace id is required"));
    }
    if let Some(binding) = config.bindings().kv_namespaces().iter().find(|binding| {
        binding.binding == namespace_id
            || binding.id.as_deref() == Some(namespace_id)
            || binding.preview_id.as_deref() == Some(namespace_id)
    }) {
        return Ok(binding.binding.clone());
    }
    if config.bindings().kv_namespaces().is_empty() {
        return Ok(namespace_id.to_string());
    }
    Err(KvRestError::not_found(format!(
        "KV namespace `{namespace_id}` is not configured"
    )))
}

pub(super) fn storage_key(namespace: &str, key: &str) -> Result<Vec<u8>, KvRestError> {
    if key.is_empty() {
        return Err(KvRestError::bad_request(
            "Workers KV keys must not be empty",
        ));
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(KvRestError::bad_request(format!(
            "Workers KV keys must be at most {MAX_KEY_BYTES} bytes"
        )));
    }
    storage_prefix(namespace, key)
}

pub(super) fn storage_prefix(namespace: &str, key_prefix: &str) -> Result<Vec<u8>, KvRestError> {
    if namespace.as_bytes().contains(&0) || key_prefix.as_bytes().contains(&0) {
        return Err(KvRestError::bad_request(
            "Workers KV names must not contain NUL",
        ));
    }
    let mut key = Vec::with_capacity(STORAGE_PREFIX.len() + namespace.len() + 1 + key_prefix.len());
    key.extend_from_slice(STORAGE_PREFIX);
    key.extend_from_slice(namespace.as_bytes());
    key.push(0);
    key.extend_from_slice(key_prefix.as_bytes());
    Ok(key)
}

pub(super) fn display_key(namespace: &str, storage_key: &[u8]) -> Result<String, KvRestError> {
    let prefix = storage_prefix(namespace, "")?;
    let Some(key) = storage_key.strip_prefix(prefix.as_slice()) else {
        return Err(KvRestError::internal(
            "KV scan returned an out-of-namespace key",
        ));
    };
    String::from_utf8(key.to_vec()).map_err(|_| KvRestError::internal("KV key is not valid UTF-8"))
}

fn resolve_expire_at_ms(query: &PutQuery) -> Result<Option<i64>, KvRestError> {
    resolve_expire_at_ms_values(query.expiration, query.expiration_ttl)
}

pub(super) fn resolve_expire_at_ms_values(
    expiration: Option<i64>,
    expiration_ttl: Option<i64>,
) -> Result<Option<i64>, KvRestError> {
    match (expiration, expiration_ttl) {
        (Some(_), Some(_)) => Err(KvRestError::bad_request(
            "expiration and expiration_ttl are mutually exclusive",
        )),
        (Some(expiration), None) => Ok(Some(expiration.saturating_mul(1000))),
        (None, Some(ttl)) => {
            if ttl < MIN_EXPIRATION_TTL_SECONDS {
                return Err(KvRestError::bad_request(format!(
                    "expiration_ttl must be at least {MIN_EXPIRATION_TTL_SECONDS} seconds"
                )));
            }
            Ok(Some(now_ms().saturating_add(ttl.saturating_mul(1000))))
        }
        (None, None) => Ok(None),
    }
}

fn encode_metadata(raw: Option<&str>) -> Result<BTreeMap<String, Vec<u8>>, KvRestError> {
    let metadata = BTreeMap::new();
    let Some(raw) = raw else {
        return Ok(metadata);
    };
    let value: Value = serde_json::from_str(raw)
        .map_err(|error| KvRestError::bad_request(format!("metadata must be JSON: {error}")))?;
    encode_metadata_value(Some(&value))
}

pub(super) fn encode_metadata_value(
    value: Option<&Value>,
) -> Result<BTreeMap<String, Vec<u8>>, KvRestError> {
    let mut metadata = BTreeMap::new();
    let Some(value) = value else {
        return Ok(metadata);
    };
    if value.is_null() {
        return Ok(metadata);
    }
    let encoded = serde_json::to_vec(&value)
        .map_err(|error| KvRestError::bad_request(format!("metadata must serialize: {error}")))?;
    if encoded.len() > MAX_METADATA_BYTES {
        return Err(KvRestError::bad_request(format!(
            "Workers KV metadata must be at most {MAX_METADATA_BYTES} bytes"
        )));
    }
    metadata.insert(METADATA_JSON_KEY.to_string(), encoded);
    Ok(metadata)
}

pub(super) fn decode_metadata(metadata: &BTreeMap<String, Vec<u8>>) -> Value {
    metadata
        .get(METADATA_JSON_KEY)
        .and_then(|bytes| serde_json::from_slice(bytes).ok())
        .unwrap_or(Value::Null)
}

pub(super) fn decode_cursor(raw: &str) -> Result<Vec<u8>, KvRestError> {
    URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| KvRestError::bad_request("KV list cursor is invalid"))
}

pub(super) fn now_ms() -> i64 {
    let millis = SystemWallClock.now_millis();
    millis.min(i64::MAX as u64) as i64
}

#[derive(Debug, Serialize)]
struct CloudflareEnvelope<T> {
    success: bool,
    errors: Vec<CloudflareApiError>,
    messages: Vec<String>,
    result: T,
}

impl<T> CloudflareEnvelope<T> {
    fn ok(result: T) -> Self {
        Self {
            success: true,
            errors: Vec::new(),
            messages: Vec::new(),
            result,
        }
    }
}

#[derive(Debug, Serialize)]
struct KvListEnvelope {
    success: bool,
    errors: Vec<CloudflareApiError>,
    messages: Vec<String>,
    result: Vec<KvListedKey>,
    result_info: KvListInfo,
}

#[derive(Debug, Serialize)]
struct KvListedKey {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    expiration: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Value>,
}

#[derive(Debug, Serialize)]
struct KvListInfo {
    cursor: String,
    list_complete: bool,
}

#[derive(Debug, Serialize)]
struct CloudflareApiError {
    code: u16,
    message: String,
}

#[derive(Debug)]
pub(super) struct KvRestError {
    status: StatusCode,
    code: u16,
    message: String,
}

impl KvRestError {
    pub(super) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: 10000,
            message: message.into(),
        }
    }

    pub(super) fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: 10001,
            message: message.into(),
        }
    }

    pub(super) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: 10009,
            message: message.into(),
        }
    }

    pub(super) fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: 10099,
            message: message.into(),
        }
    }

    pub(super) fn from_core(error: Error) -> Self {
        match error {
            Error::InvalidInput(message) => Self::bad_request(message),
            Error::TenantNotFound(_) | Error::NotFound(_) => Self::not_found(error.to_string()),
            Error::PermissionDenied(message) => Self::unauthorized(message),
            _ => Self::internal(error.to_string()),
        }
    }
}

impl std::fmt::Display for KvRestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl IntoResponse for KvRestError {
    fn into_response(self) -> Response {
        let status = self.status;
        let body = CloudflareEnvelope {
            success: false,
            errors: vec![CloudflareApiError {
                code: self.code,
                message: self.message,
            }],
            messages: Vec::new(),
            result: Value::Null,
        };
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::{Body, to_bytes};
    use nimbus_engine::{EmbeddedProviderKind, Engine};
    use nimbus_testing::EngineFixture;
    use tower::ServiceExt;

    use crate::{RouterOptions, build_router};

    use super::super::{CloudflareBindingRegistry, KvNamespaceBinding};

    const ACCESS_KEY: &str = "CFAKEY";
    const SECRET: &str = "local-secret";
    const AUTH: &str = "Bearer CFAKEY:local-secret";

    struct KvTestApp {
        _fixture: EngineFixture<Engine>,
        router: Router,
    }

    impl KvTestApp {
        fn new() -> Self {
            let fixture = EngineFixture::new(Engine::new);
            Self::from_fixture(fixture)
        }

        fn with_redb_provider() -> Self {
            Self::from_fixture(EngineFixture::new(|path| {
                Engine::new_with_embedded_provider(path, EmbeddedProviderKind::Redb)
            }))
        }

        fn with_memory_provider() -> Self {
            Self::from_fixture(EngineFixture::new(|path| {
                Engine::new_with_memory_persistence(path)
            }))
        }

        fn from_fixture(fixture: EngineFixture<Engine>) -> Self {
            let tenant = TenantId::new("tenant-a").expect("tenant id should build");
            let config = CloudflareConfig::new(CloudflareBindingRegistry::new(
                vec![KvNamespaceBinding {
                    binding: "CACHE".to_string(),
                    id: Some("namespace-prod".to_string()),
                    preview_id: None,
                }],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ))
            .with_signed_access_key(ACCESS_KEY, tenant, SECRET);
            let router = build_router(
                RouterOptions::protocol_only(fixture.engine()).with_cloudflare_config(config),
            );
            Self {
                _fixture: fixture,
                router,
            }
        }
    }

    #[tokio::test]
    async fn cloudflare_kv_tenant_admission_uses_provider_lifecycle() {
        let app = KvTestApp::with_memory_provider();
        let base = "/client/v4/accounts/acct/storage/kv/namespaces/namespace-prod";
        let overlong_key = "k".repeat(MAX_KEY_BYTES + 1);
        let (status, _, body) = request(
            test_router(&app),
            axum::http::Method::PUT,
            &format!("{base}/values/{overlong_key}"),
            Some(AUTH),
            "value".to_string(),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "validation body: {}",
            json_body(&body)
        );
        assert_eq!(
            json_body(&body)["errors"][0]["message"],
            format!("Workers KV keys must be at most {MAX_KEY_BYTES} bytes"),
            "the request must pass tenant admission before stopping at the provider-independent key boundary"
        );
        app._fixture
            .engine()
            .ensure_tenant_exists_async(TenantId::new("tenant-a").expect("tenant"))
            .await
            .expect("async admission must register the provider tenant");
    }

    fn test_app() -> KvTestApp {
        KvTestApp::new()
    }

    fn test_router(app: &KvTestApp) -> &Router {
        &app.router
    }

    async fn request(
        router: &Router,
        method: axum::http::Method,
        uri: &str,
        auth: Option<&str>,
        body: impl Into<Body>,
    ) -> (StatusCode, HeaderMap, Bytes) {
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if let Some(auth) = auth {
            builder = builder.header(header::AUTHORIZATION, auth);
        }
        let response = router
            .clone()
            .oneshot(builder.body(body.into()).expect("request should build"))
            .await
            .expect("route should respond");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should collect");
        (status, headers, bytes)
    }

    fn json_body(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).expect("response body should be json")
    }

    #[tokio::test]
    async fn kv_rest_contract_round_trips_value_metadata_delete_and_list() {
        let app = test_app();
        let router = test_router(&app);
        let base = "/client/v4/accounts/acct/storage/kv/namespaces/namespace-prod";
        let put_uri = format!("{base}/values/greeting?metadata=%7B%22lang%22%3A%22en%22%7D");
        let (status, _, body) = request(
            router,
            axum::http::Method::PUT,
            &put_uri,
            Some(AUTH),
            "hello".to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "put body: {}", json_body(&body));

        let (status, headers, body) = request(
            router,
            axum::http::Method::GET,
            &format!("{base}/values/greeting"),
            Some(AUTH),
            Body::empty(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/octet-stream")
        );
        assert_eq!(&body[..], b"hello");

        let (status, _, body) = request(
            router,
            axum::http::Method::GET,
            &format!("{base}/metadata/greeting"),
            Some(AUTH),
            Body::empty(),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "metadata body: {}",
            json_body(&body)
        );
        assert_eq!(json_body(&body)["result"], json!({"lang": "en"}));

        let (status, _, body) = request(
            router,
            axum::http::Method::GET,
            &format!("{base}/keys?prefix=g&limit=1"),
            Some(AUTH),
            Body::empty(),
        )
        .await;
        let json = json_body(&body);
        assert_eq!(status, StatusCode::OK, "list body: {json}");
        assert_eq!(json["result"][0]["name"], json!("greeting"));
        assert_eq!(json["result"][0]["metadata"], json!({"lang": "en"}));
        assert_eq!(json["result_info"]["list_complete"], json!(false));
        assert!(
            json["result_info"]["cursor"]
                .as_str()
                .is_some_and(|cursor| !cursor.is_empty()),
            "a full page must return a cursor: {json}"
        );

        let (status, _, body) = request(
            router,
            axum::http::Method::DELETE,
            &format!("{base}/values/missing"),
            Some(AUTH),
            Body::empty(),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "missing-key delete should succeed: {}",
            json_body(&body)
        );
    }

    #[tokio::test]
    async fn kv_rest_contract_remains_available_with_redb_tenants() {
        let app = KvTestApp::with_redb_provider();
        let base = "/client/v4/accounts/acct/storage/kv/namespaces/namespace-prod";
        let (status, _, body) = request(
            test_router(&app),
            axum::http::Method::PUT,
            &format!("{base}/values/redb"),
            Some(AUTH),
            "value".to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "put body: {}", json_body(&body));
    }

    #[tokio::test]
    async fn kv_rest_contract_rejects_invalid_limits_and_missing_auth() {
        let app = test_app();
        let router = test_router(&app);
        let base = "/client/v4/accounts/acct/storage/kv/namespaces/namespace-prod";

        let (status, _, body) = request(
            router,
            axum::http::Method::PUT,
            &format!("{base}/values/too-soon?expiration_ttl=59"),
            Some(AUTH),
            "short ttl".to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            json_body(&body)["errors"][0]["message"]
                .as_str()
                .unwrap()
                .contains("at least 60 seconds"),
            "got {}",
            json_body(&body)
        );

        let oversized_metadata = format!(
            "%7B%22m%22%3A%22{}%22%7D",
            "x".repeat(MAX_METADATA_BYTES + 1)
        );
        let (status, _, body) = request(
            router,
            axum::http::Method::PUT,
            &format!("{base}/values/meta?metadata={oversized_metadata}"),
            Some(AUTH),
            "value".to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            json_body(&body)["errors"][0]["message"]
                .as_str()
                .unwrap()
                .contains("metadata"),
            "got {}",
            json_body(&body)
        );

        let long_key = "x".repeat(MAX_KEY_BYTES + 1);
        let (status, _, body) = request(
            router,
            axum::http::Method::PUT,
            &format!("{base}/values/{long_key}"),
            Some(AUTH),
            "value".to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            json_body(&body)["errors"][0]["message"]
                .as_str()
                .unwrap()
                .contains("keys"),
            "got {}",
            json_body(&body)
        );

        let (status, _, body) = request(
            router,
            axum::http::Method::GET,
            &format!("{base}/values/greeting"),
            None,
            Body::empty(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(
            json_body(&body)["errors"][0]["message"]
                .as_str()
                .unwrap()
                .contains("Authorization"),
            "got {}",
            json_body(&body)
        );
    }
}
