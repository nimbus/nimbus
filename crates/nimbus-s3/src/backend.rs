use async_trait::async_trait;
use bytes::Bytes;
use nimbus_blob::BlobHash;
use nimbus_core::{CommitEntry, Result, TenantId};
use nimbus_storage::{ObjectManifest, ObjectMultipartUpload};

#[async_trait]
pub trait S3ObjectBackend: Send + Sync + 'static {
    async fn ensure_tenant(&self, tenant: &TenantId) -> Result<()>;

    async fn put_blob(&self, tenant: &TenantId, bytes: Bytes) -> Result<BlobHash>;
    async fn get_blob(&self, tenant: &TenantId, hash: &BlobHash) -> Result<Bytes>;
    async fn release_blob(&self, tenant: &TenantId, hash: &BlobHash) -> Result<()>;

    async fn put_manifest(
        &self,
        tenant: &TenantId,
        manifest: ObjectManifest,
    ) -> Result<CommitEntry>;
    async fn get_manifest(
        &self,
        tenant: &TenantId,
        bucket: &str,
        key: &str,
    ) -> Result<Option<ObjectManifest>>;
    async fn delete_manifest(
        &self,
        tenant: &TenantId,
        bucket: &str,
        key: &str,
    ) -> Result<Option<(CommitEntry, ObjectManifest)>>;
    async fn list_manifests(
        &self,
        tenant: &TenantId,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectManifest>>;

    async fn put_multipart_upload(
        &self,
        tenant: &TenantId,
        upload: ObjectMultipartUpload,
    ) -> Result<CommitEntry>;
    async fn get_multipart_upload(
        &self,
        tenant: &TenantId,
        upload_id: &str,
    ) -> Result<Option<ObjectMultipartUpload>>;
    async fn delete_multipart_upload(
        &self,
        tenant: &TenantId,
        upload_id: &str,
    ) -> Result<Option<(CommitEntry, ObjectMultipartUpload)>>;
}
