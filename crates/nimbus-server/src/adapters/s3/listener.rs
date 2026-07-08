//! S3 HTTP listener and Engine-backed object backend.
//!
//! The listener delegates HTTP/S3 parsing, SigV4 verification, and XML/REST
//! response shaping to `s3s` through `nimbus-s3`. The server-owned work here is
//! binding that protocol surface to the Engine's object metadata seam and the
//! native object-storage byte-plane resolver.

use std::sync::Arc;
#[cfg(test)]
use std::time::SystemTime;

use async_trait::async_trait;
use axum::Router;
use axum::error_handling::HandleError;
use axum::extract::{Path, State};
use axum::http::{Response, StatusCode, header};
use axum::routing::get;
use nimbus_blob::BlobStore;
use nimbus_core::{CommitEntry, Error, Result, TenantId};
use nimbus_engine::{Engine, TenantObjectMeta};
use nimbus_object_storage::{ObjectStorageConfig, ObjectStorageResolver};
use nimbus_s3::convex::{
    CONVEX_DOWNLOAD_PATH_PREFIX, ConvexObjectStorage, ConvexStorageError, DownloadTokenSigner,
};
use nimbus_s3::{NimbusS3, S3ObjectMeta, S3TenantBlobs, S3TenantObjects, S3TenantResolver};
use nimbus_storage::{ObjectManifest, ObjectMultipartUpload};
use s3s::service::S3ServiceBuilder;
use s3s::{Body, HttpError};
use tokio::net::TcpListener;
use tokio::sync::OnceCell;
use tracing::{error, info};

use super::S3Config;

/// Resolves a tenant into its byte/metadata planes once per S3 or Convex
/// storage request, backed by the Engine's object metadata seam and the
/// native object-storage byte-plane resolver.
#[derive(Clone)]
struct EngineS3Resolver {
    engine: Arc<Engine>,
    objects: ObjectStorageResolver,
}

impl EngineS3Resolver {
    fn new(engine: Arc<Engine>, object_storage: ObjectStorageConfig) -> Self {
        Self {
            objects: ObjectStorageResolver::with_config(engine.clone(), object_storage),
            engine,
        }
    }
}

#[async_trait]
impl S3TenantResolver for EngineS3Resolver {
    async fn resolve(&self, tenant: &TenantId) -> Result<S3TenantObjects> {
        let meta = self.engine.tenant_object_meta(tenant.clone()).await?;
        let blobs = Arc::new(EngineBlobResolver {
            engine: self.engine.clone(),
            objects: self.objects.clone(),
            tenant: tenant.clone(),
            resolved: OnceCell::new(),
        });
        Ok(S3TenantObjects::new(
            blobs,
            Arc::new(EngineObjectMeta(meta)),
        ))
    }

    async fn ensure_tenant(&self, tenant: &TenantId) -> Result<()> {
        self.engine.ensure_object_tenant_async(tenant.clone()).await
    }
}

/// Lazily resolves and memoizes one tenant's byte-plane [`BlobStore`].
///
/// The tenant's object-metadata handle (`tenant_object_meta`) already exists
/// by the time [`EngineS3Resolver::resolve`] builds one of these, so
/// `resolve` here only fires for requests that actually perform a byte
/// operation (put/get/release) — metadata-only requests (HeadObject,
/// ListObjectsV2, CreateMultipartUpload, the Convex metadata path) never
/// construct a [`BlobStore`] or touch the underlying object-store
/// credentials/local pack files.
///
/// [`Engine::enter_object_blob_operation`] is entered around the resolution
/// so a tenant mid-deletion is rejected with the same
/// [`Error::TenantNotFound`] the metadata plane produces, instead of racing
/// [`ObjectStorageResolver::blob_store`] (which has no guard of its own)
/// into opening byte-plane state for a tenant that is being torn down.
struct EngineBlobResolver {
    engine: Arc<Engine>,
    objects: ObjectStorageResolver,
    tenant: TenantId,
    resolved: OnceCell<Arc<dyn BlobStore>>,
}

#[async_trait]
impl S3TenantBlobs for EngineBlobResolver {
    async fn resolve(&self) -> Result<Arc<dyn BlobStore>> {
        self.resolved
            .get_or_try_init(|| async {
                let _guard = self
                    .engine
                    .enter_object_blob_operation(&self.tenant)
                    .await?;
                self.objects.blob_store(&self.tenant)
            })
            .await
            .map(Arc::clone)
    }
}

/// Newtype satisfying Rust's orphan rule: `nimbus-engine`'s [`TenantObjectMeta`]
/// and `nimbus-s3`'s [`S3ObjectMeta`] are each owned by their own crate, so
/// only a locally-owned wrapper type in this crate (which depends on both)
/// can implement the trait for the struct.
struct EngineObjectMeta(TenantObjectMeta);

#[async_trait]
impl S3ObjectMeta for EngineObjectMeta {
    async fn put_manifest(&self, manifest: ObjectManifest) -> Result<CommitEntry> {
        self.0.put_manifest(manifest).await
    }

    async fn get_manifest(&self, bucket: &str, key: &str) -> Result<Option<ObjectManifest>> {
        self.0
            .get_manifest(bucket.to_string(), key.to_string())
            .await
    }

    async fn delete_manifest(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Option<(CommitEntry, ObjectManifest)>> {
        self.0
            .delete_manifest(bucket.to_string(), key.to_string())
            .await
    }

    async fn list_manifests(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectManifest>> {
        self.0
            .list_manifests(bucket.to_string(), prefix.to_string(), limit)
            .await
    }

    async fn put_multipart_upload(&self, upload: ObjectMultipartUpload) -> Result<CommitEntry> {
        self.0.put_multipart_upload(upload).await
    }

    async fn get_multipart_upload(&self, upload_id: &str) -> Result<Option<ObjectMultipartUpload>> {
        self.0.get_multipart_upload(upload_id.to_string()).await
    }

    async fn delete_multipart_upload(
        &self,
        upload_id: &str,
    ) -> Result<Option<(CommitEntry, ObjectMultipartUpload)>> {
        self.0.delete_multipart_upload(upload_id.to_string()).await
    }

    async fn list_multipart_uploads(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectMultipartUpload>> {
        self.0
            .list_multipart_uploads(bucket.to_string(), prefix.to_string(), limit)
            .await
    }
}

#[derive(Clone)]
struct ConvexDownloadState {
    storage: ConvexObjectStorage,
    signer: DownloadTokenSigner,
}

pub(crate) fn guard_config(config: &S3Config) -> std::io::Result<()> {
    if config.access_keys.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "S3 listener requires at least one signed access key binding",
        ));
    }
    if config
        .convex_download_secret
        .as_deref()
        .is_some_and(<[u8]>::is_empty)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "S3 Convex download proxy secret cannot be empty",
        ));
    }
    Ok(())
}

pub fn router(engine: Arc<Engine>, config: S3Config) -> Router {
    let S3Config {
        access_keys,
        convex_download_secret,
        object_storage,
        ..
    } = config;
    let resolver = Arc::new(EngineS3Resolver::new(engine, object_storage));
    let s3 = NimbusS3::new(resolver.clone(), access_keys.clone());
    let mut builder = S3ServiceBuilder::new(s3);
    builder.set_auth(access_keys);
    let s3_service = HandleError::new(builder.build(), handle_s3_error);
    let router = Router::new().fallback_service(s3_service);
    match convex_download_secret {
        Some(secret) => {
            let state = ConvexDownloadState {
                storage: ConvexObjectStorage::new(resolver),
                signer: DownloadTokenSigner::new(secret)
                    .expect("S3Config guard rejects empty Convex download secrets"),
            };
            router.route(
                &format!("{}{{token}}", CONVEX_DOWNLOAD_PATH_PREFIX),
                get(convex_download).with_state(state),
            )
        }
        None => router,
    }
}

pub async fn run_listener(listener: TcpListener, engine: Arc<Engine>, config: S3Config) {
    info!("S3 listener started on {:?}", listener.local_addr().ok());
    if let Err(error) = axum::serve(listener, router(engine, config)).await {
        error!("S3 listener error: {error}");
    }
}

async fn convex_download(
    State(state): State<ConvexDownloadState>,
    Path(token): Path<String>,
) -> Response<Body> {
    match state
        .storage
        .download_with_token(&state.signer, &token, current_millis())
        .await
    {
        Ok(object) => {
            let mut builder = Response::builder().status(StatusCode::OK);
            if let Some(content_type) = object.metadata.content_type {
                builder = builder.header(header::CONTENT_TYPE, content_type);
            }
            builder
                .header("x-nimbus-storage-id", object.metadata.id.to_string())
                .header("x-nimbus-storage-sha256", object.metadata.sha256)
                .body(Body::from(object.bytes))
                .expect("static Convex download response builds")
        }
        Err(error) => convex_download_error(error),
    }
}

fn convex_download_error(error: ConvexStorageError) -> Response<Body> {
    let status = match error {
        ConvexStorageError::MissingObject => StatusCode::NOT_FOUND,
        ConvexStorageError::InvalidToken(_)
        | ConvexStorageError::ExpiredToken
        | ConvexStorageError::Forbidden(_) => StatusCode::FORBIDDEN,
        ConvexStorageError::Core(Error::NotFound(_)) => StatusCode::NOT_FOUND,
        ConvexStorageError::Core(_) | ConvexStorageError::Archive(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    Response::builder()
        .status(status)
        .body(Body::empty())
        .expect("static Convex download error response builds")
}

fn current_millis() -> u64 {
    nimbus_core::clock::system_now_millis()
}

async fn handle_s3_error(err: HttpError) -> Response<Body> {
    error!(?err, "S3 service error");
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::from("Internal Server Error".to_string()))
        .expect("static S3 error response builds")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_credential_types::Credentials as AwsCredentials;
    use aws_sigv4::http_request::{
        SignableBody, SignableRequest, SignatureLocation, SigningSettings,
        UriPathNormalizationMode, sign,
    };
    use aws_sigv4::sign::v4;
    use axum::body::to_bytes;
    use bytes::Bytes;
    use nimbus_core::TenantId;
    use nimbus_s3::AccessKeyRegistry;
    use nimbus_storage::{
        ObjectManifest, ObjectManifestAttributes, ObjectStorePlacementTarget,
        ObjectStoreProviderCredentials, ObjectStoreProviderKind, PlacementPolicy,
    };
    use std::time::Duration;
    use tower::ServiceExt;

    const ACCESS_KEY: &str = "AKIATESTS3";
    const SECRET_KEY: &str = "secret";

    fn access_keys() -> AccessKeyRegistry {
        AccessKeyRegistry::new().bind_signed(
            ACCESS_KEY,
            TenantId::new("tenant-s3").expect("tenant id"),
            SECRET_KEY,
        )
    }

    #[test]
    fn backend_uses_configured_default_placement() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let tenant = TenantId::new("tenant-s3").expect("tenant id");
        let target = ObjectStorePlacementTarget::new(
            ObjectStoreProviderKind::Memory,
            "tenant-mirror",
            ObjectStoreProviderCredentials::Anonymous,
        )
        .expect("target should build");
        let policy = PlacementPolicy::Mirror {
            target,
            require_ack: true,
        };
        let resolver = EngineS3Resolver::new(engine, ObjectStorageConfig::new(policy.clone()));

        assert_eq!(
            resolver
                .objects
                .effective_policy(&tenant)
                .expect("policy should resolve"),
            policy
        );
    }

    fn presigned_get_uri(path: &str) -> String {
        let identity =
            AwsCredentials::new(ACCESS_KEY, SECRET_KEY, None, None, "nimbus-s3-presign-test")
                .into();
        let mut settings = SigningSettings::default();
        settings.signature_location = SignatureLocation::QueryParams;
        settings.expires_in = Some(Duration::from_secs(60));
        settings.uri_path_normalization_mode = UriPathNormalizationMode::Disabled;
        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .region("us-east-1")
            .name("s3")
            .time(SystemTime::now())
            .settings(settings)
            .build()
            .expect("signing params should build")
            .into();
        let signing_uri = format!("http://localhost{path}");
        let headers = [("host", "localhost")];
        let signable = SignableRequest::new(
            "GET",
            signing_uri.as_str(),
            headers.iter().copied(),
            SignableBody::UnsignedPayload,
        )
        .expect("signable request should build");
        let (instructions, _) = sign(signable, &signing_params)
            .expect("request should sign")
            .into_parts();
        let mut request = axum::http::Request::builder()
            .method("GET")
            .uri(signing_uri)
            .header(header::HOST, "localhost")
            .body(())
            .expect("signed request shell should build");
        instructions.apply_to_request_http1x(&mut request);
        request
            .uri()
            .path_and_query()
            .expect("presigned uri should have path and query")
            .as_str()
            .to_string()
    }

    #[test]
    fn guard_refuses_empty_access_key_registry() {
        let error = guard_config(&S3Config::default())
            .expect_err("S3 must not boot without explicit signed credentials");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn guard_accepts_signed_access_key_registry() {
        assert!(guard_config(&S3Config::default().with_access_keys(access_keys())).is_ok());
    }

    #[test]
    fn guard_refuses_empty_convex_download_secret() {
        let error = guard_config(
            &S3Config::default()
                .with_access_keys(access_keys())
                .with_convex_download_secret(Vec::new()),
        )
        .expect_err("empty Convex download secret must fail closed");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn unsigned_request_is_rejected_by_s3_service() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let router = router(engine, S3Config::default().with_access_keys(access_keys()));
        let request = axum::http::Request::builder()
            .method("GET")
            .uri("/bucket/key")
            .body(axum::body::Body::empty())
            .expect("request builds");
        let response = router.oneshot(request).await.expect("route responds");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn convex_download_route_serves_valid_hmac_token() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let tenant = TenantId::new("tenant-s3").expect("tenant id");
        let storage = ConvexObjectStorage::new(Arc::new(EngineS3Resolver::new(
            engine.clone(),
            ObjectStorageConfig::default(),
        )));
        let metadata = storage
            .store(
                &tenant,
                Bytes::from_static(b"proxied bytes"),
                Some("text/plain".to_string()),
                1_776_960_000_000,
            )
            .await
            .expect("object should store");
        let secret = b"convex-download-secret".to_vec();
        let signer = DownloadTokenSigner::new(secret.clone()).expect("signer should build");
        let token = signer
            .sign(&tenant, &metadata.id, current_millis() + 60_000)
            .expect("token should sign");
        let router = router(
            engine,
            S3Config::default()
                .with_access_keys(access_keys())
                .with_convex_download_secret(secret),
        );
        let request = axum::http::Request::builder()
            .method("GET")
            .uri(format!("{CONVEX_DOWNLOAD_PATH_PREFIX}{token}"))
            .body(axum::body::Body::empty())
            .expect("request builds");

        let response = router.oneshot(request).await.expect("route responds");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain"
        );
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        assert_eq!(bytes, Bytes::from_static(b"proxied bytes"));
    }

    #[tokio::test]
    async fn s3_route_accepts_v4_presigned_get_urls() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let tenant = TenantId::new("tenant-s3").expect("tenant id");
        let resolver = EngineS3Resolver::new(engine.clone(), ObjectStorageConfig::default());
        resolver
            .ensure_tenant(&tenant)
            .await
            .expect("tenant exists");
        let ctx = resolver.resolve(&tenant).await.expect("tenant resolves");
        let hash = ctx
            .blobs()
            .await
            .expect("blob accessor should resolve")
            .put(Bytes::from_static(b"presigned bytes"))
            .await
            .expect("blob should store");
        let mut attributes = ObjectManifestAttributes::new("presigned-etag", current_millis());
        attributes.content_type = Some("text/plain".to_string());
        ctx.meta
            .put_manifest(
                ObjectManifest::whole("bucket", "presigned.txt", 15, hash.to_hex(), attributes)
                    .expect("manifest should build"),
            )
            .await
            .expect("manifest should store");

        let router = router(engine, S3Config::default().with_access_keys(access_keys()));
        let request = axum::http::Request::builder()
            .method("GET")
            .uri(presigned_get_uri("/bucket/presigned.txt"))
            .header(header::HOST, "localhost")
            .body(axum::body::Body::empty())
            .expect("request builds");

        let response = router.oneshot(request).await.expect("route responds");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        assert_eq!(bytes, Bytes::from_static(b"presigned bytes"));
    }

    /// A tenant that was never created (no `ensure_tenant` call, e.g. no S3
    /// `PutObject`/`CreateMultipartUpload` or Convex store ever ran for it)
    /// must fail `resolve` with the same [`Error::TenantNotFound`] the old
    /// per-call `Engine::get_existing_tenant_async` check produced on the
    /// first manifest/multipart call. Moving tenant resolution to once per
    /// request must not change which requests succeed.
    #[tokio::test]
    async fn resolve_fails_for_a_tenant_the_engine_never_created() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let tenant = TenantId::new("tenant-never-created").expect("tenant id");
        let resolver = EngineS3Resolver::new(engine, ObjectStorageConfig::default());

        let error = match resolver.resolve(&tenant).await {
            Ok(_) => panic!("resolve must not implicitly create an unensured tenant"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::TenantNotFound(missing) if missing == tenant));
    }
}
