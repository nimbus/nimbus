use nimbus_core::Result;
use nimbus_storage::{ObjectManifest, ObjectMetaRead, ObjectMultipartUpload};

use super::TenantPersistence;

/// Read-plane dispatch only, and storage's `ObjectMetaRead` has no write half
/// to reach: object-metadata writes are journal commits sequenced inside the
/// tenant committer actor (`engine::objects`). A store-level write would
/// assign commit sequences outside the fenced committer path.
impl TenantPersistence {
    pub(crate) fn get_object_manifest(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Option<ObjectManifest>> {
        match_tenant_persistence!(self, |store| store.get_object_manifest(bucket, key))
    }

    pub(crate) fn list_object_manifests(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectManifest>> {
        match_tenant_persistence!(self, |store| {
            store.list_object_manifests(bucket, prefix, limit)
        })
    }

    pub(crate) fn get_multipart_upload(
        &self,
        upload_id: &str,
    ) -> Result<Option<ObjectMultipartUpload>> {
        match_tenant_persistence!(self, |store| store.get_multipart_upload(upload_id))
    }

    pub(crate) fn list_multipart_uploads(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectMultipartUpload>> {
        match_tenant_persistence!(self, |store| {
            store.list_multipart_uploads(bucket, prefix, limit)
        })
    }
}
