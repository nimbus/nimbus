use nimbus_core::{CommitEntry, Result};
use nimbus_storage::{ObjectManifest, ObjectMetaStore, ObjectMultipartUpload};

use super::TenantPersistence;

impl TenantPersistence {
    pub(crate) fn put_object_manifest(&self, manifest: &ObjectManifest) -> Result<CommitEntry> {
        match_tenant_persistence!(self, |store| store.put_object_manifest(manifest))
    }

    pub(crate) fn get_object_manifest(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Option<ObjectManifest>> {
        match_tenant_persistence!(self, |store| store.get_object_manifest(bucket, key))
    }

    pub(crate) fn delete_object_manifest(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Option<(CommitEntry, ObjectManifest)>> {
        match_tenant_persistence!(self, |store| store.delete_object_manifest(bucket, key))
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

    pub(crate) fn put_multipart_upload(
        &self,
        upload: &ObjectMultipartUpload,
    ) -> Result<CommitEntry> {
        match_tenant_persistence!(self, |store| store.put_multipart_upload(upload))
    }

    pub(crate) fn get_multipart_upload(
        &self,
        upload_id: &str,
    ) -> Result<Option<ObjectMultipartUpload>> {
        match_tenant_persistence!(self, |store| store.get_multipart_upload(upload_id))
    }

    pub(crate) fn delete_multipart_upload(
        &self,
        upload_id: &str,
    ) -> Result<Option<(CommitEntry, ObjectMultipartUpload)>> {
        match_tenant_persistence!(self, |store| store.delete_multipart_upload(upload_id))
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
