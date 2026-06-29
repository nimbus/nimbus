//! S3 HTTP listener and Engine-backed object backend.
//!
//! The listener delegates HTTP/S3 parsing, SigV4 verification, and XML/REST
//! response shaping to `s3s` through `nimbus-s3`. The server-owned work here is
//! binding that protocol surface to the Engine's object metadata seam and the
//! local byte plane rooted under the Engine data directory.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::Router;
use axum::error_handling::HandleError;
use axum::http::{Response, StatusCode};
use bytes::Bytes;
use nimbus_blob::{BlobHash, BlobStore, LocalPackStore};
use nimbus_core::{CommitEntry, Error, Result, StorageErrorKind, TenantId};
use nimbus_engine::Engine;
use nimbus_s3::{AccessKeyRegistry, NimbusS3, S3ObjectBackend};
use nimbus_storage::{ObjectManifest, ObjectMultipartUpload};
use s3s::service::S3ServiceBuilder;
use s3s::{Body, HttpError};
use tokio::net::TcpListener;
use tracing::{error, info};

#[derive(Clone)]
struct EngineS3Backend {
    engine: Arc<Engine>,
    stores: Arc<Mutex<HashMap<TenantId, Arc<LocalPackStore>>>>,
}

impl EngineS3Backend {
    fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            stores: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn store(&self, tenant: &TenantId) -> Result<Arc<LocalPackStore>> {
        let mut stores = self.stores.lock().map_err(|_| {
            Error::storage(
                StorageErrorKind::Other,
                "S3 local pack store cache lock poisoned",
            )
        })?;
        if let Some(store) = stores.get(tenant) {
            return Ok(store.clone());
        }
        let root = self
            .engine
            .data_dir()
            .join("object-blobs")
            .join(tenant.as_str());
        let store = Arc::new(LocalPackStore::open(root)?);
        stores.insert(tenant.clone(), store.clone());
        Ok(store)
    }
}

#[async_trait]
impl S3ObjectBackend for EngineS3Backend {
    async fn ensure_tenant(&self, tenant: &TenantId) -> Result<()> {
        self.engine.ensure_object_tenant_async(tenant.clone()).await
    }

    async fn put_blob(&self, tenant: &TenantId, bytes: Bytes) -> Result<BlobHash> {
        self.store(tenant)?.put(bytes).await
    }

    async fn get_blob(&self, tenant: &TenantId, hash: &BlobHash) -> Result<Bytes> {
        self.store(tenant)?.get(hash).await
    }

    async fn release_blob(&self, tenant: &TenantId, hash: &BlobHash) -> Result<()> {
        self.store(tenant)?.release(hash).await
    }

    async fn put_manifest(
        &self,
        tenant: &TenantId,
        manifest: ObjectManifest,
    ) -> Result<CommitEntry> {
        self.engine
            .put_object_manifest_async(tenant.clone(), manifest)
            .await
    }

    async fn get_manifest(
        &self,
        tenant: &TenantId,
        bucket: &str,
        key: &str,
    ) -> Result<Option<ObjectManifest>> {
        self.engine
            .get_object_manifest_async(tenant.clone(), bucket.to_string(), key.to_string())
            .await
    }

    async fn delete_manifest(
        &self,
        tenant: &TenantId,
        bucket: &str,
        key: &str,
    ) -> Result<Option<(CommitEntry, ObjectManifest)>> {
        self.engine
            .delete_object_manifest_async(tenant.clone(), bucket.to_string(), key.to_string())
            .await
    }

    async fn list_manifests(
        &self,
        tenant: &TenantId,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectManifest>> {
        self.engine
            .list_object_manifests_async(
                tenant.clone(),
                bucket.to_string(),
                prefix.to_string(),
                limit,
            )
            .await
    }

    async fn put_multipart_upload(
        &self,
        tenant: &TenantId,
        upload: ObjectMultipartUpload,
    ) -> Result<CommitEntry> {
        self.engine
            .put_multipart_upload_async(tenant.clone(), upload)
            .await
    }

    async fn get_multipart_upload(
        &self,
        tenant: &TenantId,
        upload_id: &str,
    ) -> Result<Option<ObjectMultipartUpload>> {
        self.engine
            .get_multipart_upload_async(tenant.clone(), upload_id.to_string())
            .await
    }

    async fn delete_multipart_upload(
        &self,
        tenant: &TenantId,
        upload_id: &str,
    ) -> Result<Option<(CommitEntry, ObjectMultipartUpload)>> {
        self.engine
            .delete_multipart_upload_async(tenant.clone(), upload_id.to_string())
            .await
    }
}

pub(crate) fn guard_has_access_keys(access_keys: &AccessKeyRegistry) -> std::io::Result<()> {
    if access_keys.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "S3 listener requires at least one signed access key binding",
        ));
    }
    Ok(())
}

pub fn router(engine: Arc<Engine>, access_keys: AccessKeyRegistry) -> Router {
    let s3 = NimbusS3::new(Arc::new(EngineS3Backend::new(engine)), access_keys.clone());
    let mut builder = S3ServiceBuilder::new(s3);
    builder.set_auth(access_keys);
    let s3_service = HandleError::new(builder.build(), handle_s3_error);
    Router::new().fallback_service(s3_service)
}

pub async fn run_listener(
    listener: TcpListener,
    engine: Arc<Engine>,
    access_keys: AccessKeyRegistry,
) {
    info!("S3 listener started on {:?}", listener.local_addr().ok());
    if let Err(error) = axum::serve(listener, router(engine, access_keys)).await {
        error!("S3 listener error: {error}");
    }
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
    use nimbus_core::TenantId;
    use tower::ServiceExt;

    const ACCESS_KEY: &str = "AKIATESTS3";

    fn access_keys() -> AccessKeyRegistry {
        AccessKeyRegistry::new().bind_signed(
            ACCESS_KEY,
            TenantId::new("tenant-s3").expect("tenant id"),
            "secret",
        )
    }

    #[test]
    fn guard_refuses_empty_access_key_registry() {
        let error = guard_has_access_keys(&AccessKeyRegistry::new())
            .expect_err("S3 must not boot without explicit signed credentials");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn guard_accepts_signed_access_key_registry() {
        assert!(guard_has_access_keys(&access_keys()).is_ok());
    }

    #[tokio::test]
    async fn unsigned_request_is_rejected_by_s3_service() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        let router = router(engine, access_keys());
        let request = axum::http::Request::builder()
            .method("GET")
            .uri("/bucket/key")
            .body(axum::body::Body::empty())
            .expect("request builds");
        let response = router.oneshot(request).await.expect("route responds");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
