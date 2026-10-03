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
use nimbus_cloudflare::CloudflareConfig;
use nimbus_cloudflare::kv::{
    DEFAULT_LIST_LIMIT, KvError, KvErrorKind, MAX_VALUE_BYTES, decode_cursor, decode_metadata,
    display_key, encode_metadata_value, finalize_list_page, now_ms, resolve_expire_at_ms_values,
    resolve_worker_namespace, storage_key, storage_prefix, validate_list_limit,
};
use nimbus_core::TenantId;
use nimbus_storage::KvPut;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state::AppState;

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
        .map_err(KvError::from_core)?
        .ok_or_else(|| KvError::not_found("KV key not found"))?;
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
        .map_err(KvError::from_core)?
        .ok_or_else(|| KvError::not_found("KV key not found"))?;
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
        return Err(KvError::bad_request(format!(
            "Workers KV values must be at most {MAX_VALUE_BYTES} bytes"
        ))
        .into());
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
        .map_err(KvError::from_core)?;
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
        .map_err(KvError::from_core)?;
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
    validate_list_limit(limit)?;
    let scan_limit = limit + 1;
    let prefix = storage_prefix(&namespace, query.prefix.as_deref().unwrap_or_default())?;
    let cursor = query.cursor.as_deref().map(decode_cursor).transpose()?;
    let page = state
        .engine
        .tenant_kv_scan(&tenant_id, &prefix, cursor.as_deref(), scan_limit, now_ms())
        .map_err(KvError::from_core)?;
    let (entries, cursor) = finalize_list_page(page, limit);
    let mut keys = Vec::with_capacity(entries.len());
    for entry in entries {
        let name = display_key(&namespace, &entry.key)?;
        let metadata = decode_metadata(&entry.metadata);
        keys.push(KvListedKey {
            name,
            expiration: entry.expire_at_ms.map(|value| value / 1000),
            metadata: (!metadata.is_null()).then_some(metadata),
        });
    }
    let cursor = cursor.map(|cursor| URL_SAFE_NO_PAD.encode(cursor));
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

fn authenticate(headers: &HeaderMap, config: &CloudflareConfig) -> Result<TenantId, KvError> {
    let header = headers
        .get(header::AUTHORIZATION)
        .ok_or_else(|| KvError::unauthorized("Cloudflare KV REST requires Authorization"))?
        .to_str()
        .map_err(|_| KvError::unauthorized("Authorization must be valid ASCII"))?;
    let token = header
        .strip_prefix("Bearer ")
        .ok_or_else(|| KvError::unauthorized("Authorization must use the Bearer scheme"))?;
    let (access_key_id, secret) = token
        .split_once(':')
        .ok_or_else(|| KvError::unauthorized("Bearer token must be ACCESS_KEY_ID:SECRET"))?;
    let binding = config
        .access_keys()
        .binding(access_key_id)
        .map_err(|_| KvError::unauthorized("Cloudflare KV credential is not recognized"))?;
    if binding.secret.as_deref() != Some(secret) {
        return Err(KvError::unauthorized(
            "Cloudflare KV credential secret is invalid",
        ));
    }
    Ok(binding.tenant.clone())
}

async fn ensure_tenant(state: &Arc<AppState>, tenant_id: &TenantId) -> Result<(), KvError> {
    state
        .engine
        .ensure_tenant_ready_async(tenant_id.clone())
        .await
        .map(|_| ())
        .map_err(KvError::from_core)
}

fn resolve_namespace(
    config: &CloudflareConfig,
    account_id: &str,
    namespace_id: &str,
) -> Result<String, KvError> {
    if account_id.trim().is_empty() {
        return Err(KvError::bad_request("Cloudflare account id is required"));
    }
    if namespace_id.trim().is_empty() {
        return Err(KvError::bad_request("KV namespace id is required"));
    }
    resolve_worker_namespace(config, namespace_id)
}

fn resolve_expire_at_ms(query: &PutQuery) -> Result<Option<i64>, KvError> {
    resolve_expire_at_ms_values(query.expiration, query.expiration_ttl)
}

fn encode_metadata(raw: Option<&str>) -> Result<BTreeMap<String, Vec<u8>>, KvError> {
    let metadata = BTreeMap::new();
    let Some(raw) = raw else {
        return Ok(metadata);
    };
    let value: Value = serde_json::from_str(raw)
        .map_err(|error| KvError::bad_request(format!("metadata must be JSON: {error}")))?;
    encode_metadata_value(Some(&value))
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
struct KvRestError {
    status: StatusCode,
    code: u16,
    message: String,
}

impl From<KvError> for KvRestError {
    fn from(error: KvError) -> Self {
        let (status, code) = match error.kind() {
            KvErrorKind::BadRequest => (StatusCode::BAD_REQUEST, 10000),
            KvErrorKind::Unauthorized => (StatusCode::UNAUTHORIZED, 10001),
            KvErrorKind::NotFound => (StatusCode::NOT_FOUND, 10009),
            KvErrorKind::Internal => (StatusCode::INTERNAL_SERVER_ERROR, 10099),
        };
        Self {
            status,
            code,
            message: error.into_message(),
        }
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

    use nimbus_cloudflare::kv::{MAX_KEY_BYTES, MAX_METADATA_BYTES};
    use nimbus_cloudflare::{CloudflareBindingRegistry, KvNamespaceBinding};

    use crate::{RouterOptions, build_router};

    const ACCESS_KEY: &str = "CFAKEY";
    const SECRET: &str = "local-secret";
    const AUTH: &str = "Bearer CFAKEY:local-secret";

    struct KvTestApp {
        _fixture: EngineFixture<Engine>,
        router: Router,
    }

    impl KvTestApp {
        fn new() -> Self {
            let fixture = EngineFixture::new(|path| Engine::new(path));
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
        assert_kv_rest_contract(test_app()).await;
    }

    async fn assert_kv_rest_contract(app: KvTestApp) {
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
            axum::http::Method::PUT,
            &format!("{base}/values/greeting-two"),
            Some(AUTH),
            "hello again".to_string(),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "second put body: {}",
            json_body(&body)
        );

        for index in 1..=9 {
            let (status, _, body) = request(
                router,
                axum::http::Method::PUT,
                &format!("{base}/values/greeting-{index:02}"),
                Some(AUTH),
                format!("hello {index}"),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::OK,
                "pagination fixture put body: {}",
                json_body(&body)
            );
        }

        let (status, _, body) = request(
            router,
            axum::http::Method::GET,
            &format!("{base}/keys?prefix=g&limit=10"),
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
        let cursor = json["result_info"]["cursor"]
            .as_str()
            .expect("full first page should return a cursor");
        let (status, _, body) = request(
            router,
            axum::http::Method::GET,
            &format!("{base}/keys?prefix=g&limit=10&cursor={cursor}"),
            Some(AUTH),
            Body::empty(),
        )
        .await;
        let json = json_body(&body);
        assert_eq!(status, StatusCode::OK, "second list body: {json}");
        assert_eq!(json["result"][0]["name"], json!("greeting-two"));
        assert_eq!(json["result_info"]["list_complete"], json!(true));
        assert_eq!(json["result_info"]["cursor"], json!(""));

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
        assert_kv_rest_contract(KvTestApp::with_redb_provider()).await;
    }

    #[tokio::test]
    async fn kv_rest_contract_rejects_invalid_limits_and_missing_auth() {
        let app = test_app();
        let router = test_router(&app);
        let base = "/client/v4/accounts/acct/storage/kv/namespaces/namespace-prod";

        for invalid_limit in [0, 1, 9, 1001] {
            let (status, _, body) = request(
                router,
                axum::http::Method::GET,
                &format!("{base}/keys?limit={invalid_limit}"),
                Some(AUTH),
                Body::empty(),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(
                json_body(&body)["errors"][0]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("between 10 and 1000")),
                "got {}",
                json_body(&body)
            );
        }

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
