use std::sync::Arc;

use nimbus_core::{CommitEntry, Result, TenantId};
use nimbus_storage::{ObjectManifest, ObjectMultipartUpload};

use super::Engine;

impl Engine {
    pub async fn ensure_object_tenant_async(self: &Arc<Self>, tenant_id: TenantId) -> Result<()> {
        match self.create_tenant_async(tenant_id).await {
            Ok(()) | Err(nimbus_core::Error::AlreadyExists(_)) => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub async fn put_object_manifest_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        manifest: ObjectManifest,
    ) -> Result<CommitEntry> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        runtime
            .read_storage()
            .execute(move |store| store.put_object_manifest(&manifest))
            .await
    }

    pub async fn get_object_manifest_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        bucket: String,
        key: String,
    ) -> Result<Option<ObjectManifest>> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        runtime
            .read_storage()
            .execute(move |store| store.get_object_manifest(&bucket, &key))
            .await
    }

    pub async fn delete_object_manifest_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        bucket: String,
        key: String,
    ) -> Result<Option<(CommitEntry, ObjectManifest)>> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        runtime
            .read_storage()
            .execute(move |store| store.delete_object_manifest(&bucket, &key))
            .await
    }

    pub async fn list_object_manifests_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        bucket: String,
        prefix: String,
        limit: usize,
    ) -> Result<Vec<ObjectManifest>> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        runtime
            .read_storage()
            .execute(move |store| store.list_object_manifests(&bucket, &prefix, limit))
            .await
    }

    pub async fn put_multipart_upload_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        upload: ObjectMultipartUpload,
    ) -> Result<CommitEntry> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        runtime
            .read_storage()
            .execute(move |store| store.put_multipart_upload(&upload))
            .await
    }

    pub async fn get_multipart_upload_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        upload_id: String,
    ) -> Result<Option<ObjectMultipartUpload>> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        runtime
            .read_storage()
            .execute(move |store| store.get_multipart_upload(&upload_id))
            .await
    }

    pub async fn delete_multipart_upload_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        upload_id: String,
    ) -> Result<Option<(CommitEntry, ObjectMultipartUpload)>> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        runtime
            .read_storage()
            .execute(move |store| store.delete_multipart_upload(&upload_id))
            .await
    }

    pub async fn list_multipart_uploads_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        bucket: String,
        prefix: String,
        limit: usize,
    ) -> Result<Vec<ObjectMultipartUpload>> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        runtime
            .read_storage()
            .execute(move |store| store.list_multipart_uploads(&bucket, &prefix, limit))
            .await
    }
}
