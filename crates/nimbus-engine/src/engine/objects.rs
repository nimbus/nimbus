use std::sync::Arc;

use nimbus_core::{CommitEntry, Result, TenantId};
use nimbus_storage::{ObjectManifest, ObjectMultipartUpload};

use super::Engine;
use crate::tenant::{TenantOperationGuard, TenantRuntime};

impl Engine {
    pub async fn ensure_object_tenant_async(self: &Arc<Self>, tenant_id: TenantId) -> Result<()> {
        self.ensure_tenant_ready_async(tenant_id).await.map(|_| ())
    }

    /// Resolves `tenant_id` to its object-metadata handle once, so callers
    /// that issue several manifest/multipart operations against the same
    /// tenant (an S3 or Convex-storage request) no longer re-resolve the
    /// tenant on every call.
    pub async fn tenant_object_meta(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<TenantObjectMeta> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        Ok(TenantObjectMeta { runtime, tenant_id })
    }

    /// Enters a tenant operation guard for the object byte-plane without
    /// resolving any storage. The byte-plane resolver (`nimbus-object-storage`)
    /// depends on this crate, so it cannot be called from here; instead,
    /// callers that need guarded, lazy blob-plane resolution enter this guard
    /// first and resolve the blob store themselves while it is held. This
    /// mirrors [`TenantObjectMeta`]'s methods, which enter the same guard
    /// around each metadata-plane call: a tenant mid-deletion rejects with
    /// the same [`nimbus_core::Error::TenantNotFound`] either way.
    pub async fn enter_object_blob_operation(
        self: &Arc<Self>,
        tenant_id: &TenantId,
    ) -> Result<TenantOperationGuard> {
        let runtime = self.get_existing_tenant_async(tenant_id).await?;
        runtime.enter_operation(tenant_id)
    }
}

/// One tenant's object-metadata plane, resolved once via
/// [`Engine::tenant_object_meta`]. Each method still enters a fresh
/// [`TenantRuntime::enter_operation`] guard per call: resolution is hoisted,
/// but the deletion-blocking guard remains scoped to the individual
/// operation, matching every other tenant-scoped call in the engine.
pub struct TenantObjectMeta {
    runtime: Arc<TenantRuntime>,
    tenant_id: TenantId,
}

impl TenantObjectMeta {
    pub async fn put_manifest(&self, manifest: ObjectManifest) -> Result<CommitEntry> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.runtime
            .read_storage()
            .execute(move |store| store.put_object_manifest(&manifest))
            .await
    }

    pub async fn get_manifest(
        &self,
        bucket: String,
        key: String,
    ) -> Result<Option<ObjectManifest>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.runtime
            .read_storage()
            .execute(move |store| store.get_object_manifest(&bucket, &key))
            .await
    }

    pub async fn delete_manifest(
        &self,
        bucket: String,
        key: String,
    ) -> Result<Option<(CommitEntry, ObjectManifest)>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.runtime
            .read_storage()
            .execute(move |store| store.delete_object_manifest(&bucket, &key))
            .await
    }

    pub async fn list_manifests(
        &self,
        bucket: String,
        prefix: String,
        limit: usize,
    ) -> Result<Vec<ObjectManifest>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.runtime
            .read_storage()
            .execute(move |store| store.list_object_manifests(&bucket, &prefix, limit))
            .await
    }

    pub async fn put_multipart_upload(&self, upload: ObjectMultipartUpload) -> Result<CommitEntry> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.runtime
            .read_storage()
            .execute(move |store| store.put_multipart_upload(&upload))
            .await
    }

    pub async fn get_multipart_upload(
        &self,
        upload_id: String,
    ) -> Result<Option<ObjectMultipartUpload>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.runtime
            .read_storage()
            .execute(move |store| store.get_multipart_upload(&upload_id))
            .await
    }

    pub async fn delete_multipart_upload(
        &self,
        upload_id: String,
    ) -> Result<Option<(CommitEntry, ObjectMultipartUpload)>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.runtime
            .read_storage()
            .execute(move |store| store.delete_multipart_upload(&upload_id))
            .await
    }

    pub async fn list_multipart_uploads(
        &self,
        bucket: String,
        prefix: String,
        limit: usize,
    ) -> Result<Vec<ObjectMultipartUpload>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.runtime
            .read_storage()
            .execute(move |store| store.list_multipart_uploads(&bucket, &prefix, limit))
            .await
    }
}
