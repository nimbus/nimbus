//! Native object routes: buckets, listings, and whole-object bytes for one
//! tenant over the session-authenticated main listener.
//!
//! The S3 listener is the object surface for SigV4 clients. The operator
//! console must never hold S3 credentials, so it reaches the same per-tenant
//! object planes through these routes instead. Metadata reads go to the
//! engine's object-metadata handle; byte operations and the blob lifecycle
//! go through the shared resolver in [`AppState`] and the lifecycle contract
//! in `nimbus_s3::objects`, so an object written here reads back over S3
//! with the same ETag, and an object deleted here releases its blobs the
//! same way an S3 delete does.

use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query as QueryParams, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use nimbus_s3::S3TenantResolver;
use nimbus_s3::objects::{
    delete_object as delete_stored_object, put_whole_object, read_object_bytes,
};
use nimbus_storage::{ObjectBucketSummary, ObjectManifest};
use serde::{Deserialize, Serialize};

use super::{AppError, AppState, parse_operator_tenant_context};

/// Largest body the whole-object upload route accepts. Same ceiling as the
/// DynamoDB listener; a larger object belongs to the S3 multipart path.
pub(crate) const MAX_OBJECT_UPLOAD_BYTES: usize = 16 * 1024 * 1024;

const DEFAULT_LIST_LIMIT: usize = 1_000;
const MAX_LIST_LIMIT: usize = 5_000;
const OCTET_STREAM: &str = "application/octet-stream";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObjectBucketListResponse {
    buckets: Vec<ObjectBucketResponse>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObjectBucketResponse {
    bucket: String,
    object_count: u64,
    total_bytes: u64,
}

impl From<ObjectBucketSummary> for ObjectBucketResponse {
    fn from(summary: ObjectBucketSummary) -> Self {
        Self {
            bucket: summary.bucket,
            object_count: summary.object_count,
            total_bytes: summary.total_bytes,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObjectListResponse {
    bucket: String,
    prefix: String,
    objects: Vec<ObjectSummaryResponse>,
    /// True when more objects match than `limit` allowed; narrow the prefix
    /// or raise the limit to see them.
    truncated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObjectSummaryResponse {
    bucket: String,
    key: String,
    size: u64,
    content_type: Option<String>,
    etag: String,
    last_modified_millis: u64,
}

impl From<ObjectManifest> for ObjectSummaryResponse {
    fn from(manifest: ObjectManifest) -> Self {
        Self {
            bucket: manifest.bucket,
            key: manifest.key,
            size: manifest.size,
            content_type: manifest.content_type,
            etag: manifest.etag,
            last_modified_millis: manifest.last_modified_millis,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ObjectListQuery {
    prefix: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct ObjectGetQuery {
    /// Present (any value) to answer with `Content-Disposition: attachment`,
    /// so a browser saves the bytes instead of rendering them.
    download: Option<String>,
}

/// Lists every bucket at least one object names, with counts and byte totals.
pub(crate) async fn list_object_buckets(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<Json<ObjectBucketListResponse>, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.objects.buckets")?;
    let meta = state
        .engine
        .tenant_object_meta(tenant.tenant_id().clone())
        .await?;
    let buckets = meta
        .list_buckets()
        .await?
        .into_iter()
        .map(ObjectBucketResponse::from)
        .collect();
    Ok(Json(ObjectBucketListResponse { buckets }))
}

/// Lists the objects in one bucket under an optional key prefix, sorted by key.
pub(crate) async fn list_objects(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, bucket)): Path<(String, String)>,
    QueryParams(query): QueryParams<ObjectListQuery>,
) -> Result<Json<ObjectListResponse>, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.objects.list")?;
    let prefix = query.prefix.unwrap_or_default();
    let limit = query
        .limit
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT);
    let meta = state
        .engine
        .tenant_object_meta(tenant.tenant_id().clone())
        .await?;
    // One past the limit tells the caller whether the page is complete
    // without a second scan.
    let mut manifests = meta
        .list_manifests(bucket.clone(), prefix.clone(), limit + 1)
        .await?;
    let truncated = manifests.len() > limit;
    manifests.truncate(limit);
    Ok(Json(ObjectListResponse {
        bucket,
        prefix,
        objects: manifests
            .into_iter()
            .map(ObjectSummaryResponse::from)
            .collect(),
        truncated,
    }))
}

/// Answers with the object bytes and the manifest headers a client needs to
/// render or save them.
pub(crate) async fn get_object(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, bucket, key)): Path<(String, String, String)>,
    QueryParams(query): QueryParams<ObjectGetQuery>,
) -> Result<Response, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.objects.get")?;
    let ctx = state.objects().resolve(tenant.tenant_id()).await?;
    let Some(manifest) = ctx.meta.get_manifest(&bucket, &key).await? else {
        return Err(object_not_found(&bucket, &key));
    };
    let bytes = read_object_bytes(&ctx, &manifest).await?;
    let disposition = if query.download.is_some() {
        "attachment"
    } else {
        "inline"
    };
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type_header(&manifest));
    headers.insert(header::ETAG, etag_header(&manifest));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "{disposition}; filename=\"{}\"",
            ascii_filename(&manifest.key)
        ))
        .unwrap_or_else(|_| HeaderValue::from_static("inline")),
    );
    // Object bytes are user content served from the console origin. The
    // sandbox directive keeps a stored HTML or SVG document from running
    // script with that origin when a browser opens the link directly.
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    Ok((StatusCode::OK, headers, bytes).into_response())
}

/// Stores the request body as one whole object, replacing any object at
/// the same key. The `Content-Type` header, when present, is recorded on
/// the manifest and answered back on every read.
pub(crate) async fn put_object(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, bucket, key)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<ObjectSummaryResponse>), AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.objects.put")?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    // `resolve` rejects an unknown tenant. The S3 surface creates a tenant
    // on first write because its access key binds one; an operator route
    // must not create tenants as a side effect of an upload.
    let ctx = state.objects().resolve(tenant.tenant_id()).await?;
    let manifest = put_whole_object(&ctx, &bucket, &key, body, content_type).await?;
    Ok((
        StatusCode::CREATED,
        Json(ObjectSummaryResponse::from(manifest)),
    ))
}

/// Deletes one object and releases its blobs.
pub(crate) async fn delete_object(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, bucket, key)): Path<(String, String, String)>,
) -> Result<StatusCode, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.objects.delete")?;
    let ctx = state.objects().resolve(tenant.tenant_id()).await?;
    match delete_stored_object(&ctx, &bucket, &key).await? {
        Some(_) => Ok(StatusCode::NO_CONTENT),
        None => Err(object_not_found(&bucket, &key)),
    }
}

fn object_not_found(bucket: &str, key: &str) -> AppError {
    AppError::not_found(format!("object `{bucket}/{key}` was not found"))
}

fn content_type_header(manifest: &ObjectManifest) -> HeaderValue {
    manifest
        .content_type
        .as_deref()
        .and_then(|value| HeaderValue::from_str(value).ok())
        .unwrap_or_else(|| HeaderValue::from_static(OCTET_STREAM))
}

fn etag_header(manifest: &ObjectManifest) -> HeaderValue {
    HeaderValue::from_str(&format!("\"{}\"", manifest.etag))
        .unwrap_or_else(|_| HeaderValue::from_static("\"\""))
}

/// The last key segment reduced to the printable ASCII a quoted
/// `filename=` parameter can carry. Non-ASCII, quotes, and backslashes
/// become underscores; an empty result falls back to `object`.
fn ascii_filename(key: &str) -> String {
    let segment = key
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or("");
    let name: String = segment
        .chars()
        .map(|ch| {
            if ch.is_ascii_graphic() && ch != '"' && ch != '\\' {
                ch
            } else if ch == ' ' {
                ' '
            } else {
                '_'
            }
        })
        .collect();
    if name.trim().is_empty() {
        "object".to_string()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::ascii_filename;

    #[test]
    fn filename_uses_the_last_key_segment_and_stays_ascii() {
        assert_eq!(ascii_filename("notes/hello.txt"), "hello.txt");
        assert_eq!(ascii_filename("dir/"), "dir");
        assert_eq!(ascii_filename("a/b/\"quoted\".txt"), "_quoted_.txt");
        assert_eq!(ascii_filename("caf\u{e9}.png"), "caf_.png");
        assert_eq!(ascii_filename("/"), "object");
    }
}
