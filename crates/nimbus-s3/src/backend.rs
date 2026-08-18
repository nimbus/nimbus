use std::sync::Arc;

use async_trait::async_trait;
use nimbus_blob::BlobStore;
use nimbus_core::{CommitEntry, Error, Result, TenantId};
use nimbus_storage::{
    ObjectConditionOutcome, ObjectExpectedState, ObjectManifest, ObjectMultipartUpload,
};

/// One tenant's byte-plane accessor plus named-metadata handle, resolved once
/// per request via [`S3TenantResolver::resolve`] instead of re-resolving the
/// tenant on every backend call.
///
/// The byte-plane side is lazy: [`S3TenantObjects::blobs`] resolves (and
/// guards) the underlying [`BlobStore`] only when a byte operation actually
/// needs it. Metadata-only operations (HeadObject, ListObjectsV2,
/// CreateMultipartUpload, ...) must never call it, so they never create
/// per-tenant byte-plane state or require blob-plane credentials.
#[derive(Clone)]
pub struct S3TenantObjects {
    blobs: Arc<dyn S3TenantBlobs>,
    pub meta: Arc<dyn S3ObjectMeta>,
}

impl S3TenantObjects {
    #[must_use]
    pub fn new(blobs: Arc<dyn S3TenantBlobs>, meta: Arc<dyn S3ObjectMeta>) -> Self {
        Self { blobs, meta }
    }

    /// Resolves the guarded byte-plane store for this request. Call only
    /// from byte operations; metadata-only operations must never call this.
    pub async fn blobs(&self) -> Result<Arc<dyn BlobStore>> {
        self.blobs.resolve().await
    }
}

/// Resolves one tenant's byte-plane [`BlobStore`], guarded and memoized.
///
/// Implementations must reject a tenant whose runtime is mid-deletion with
/// the same error metadata operations reject with (paralleling
/// [`S3TenantResolver::resolve`]'s existence check), and must memoize the
/// resolved store so repeated byte operations within one request neither
/// re-resolve nor re-enter the guard.
#[async_trait]
pub trait S3TenantBlobs: Send + Sync + 'static {
    async fn resolve(&self) -> Result<Arc<dyn BlobStore>>;
}

/// The named object-metadata plane for one already-resolved tenant: object
/// manifests and multipart uploads. No method takes a tenant argument — the
/// handle itself is tenant-scoped, matching [`BlobStore`]'s per-tenant shape.
#[async_trait]
pub trait S3ObjectMeta: Send + Sync + 'static {
    /// Writes a manifest, but only if every clause in `expected` holds
    /// against the commit authority's own read of the current row.
    ///
    /// There is no unconditional sibling on this trait. The S3 surface must
    /// not read a manifest, decide a precondition against that read, and then
    /// write unconditionally: between the read and the write another request
    /// can commit at the same key, and both writers then believe they won.
    /// The clauses travel to the authority instead, which decides them
    /// against its own serialized read before it assigns a sequence.
    ///
    /// Pass an empty `expected` for a genuinely unconditional write, through
    /// [`put_manifest_unconditional`], which names that choice at the call
    /// site and rejects the outcome that cannot happen.
    async fn put_manifest_conditional(
        &self,
        manifest: ObjectManifest,
        expected: Vec<ObjectExpectedState>,
    ) -> Result<ObjectConditionOutcome>;
    async fn get_manifest(&self, bucket: &str, key: &str) -> Result<Option<ObjectManifest>>;
    async fn delete_manifest(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Option<(CommitEntry, ObjectManifest)>>;
    async fn list_manifests(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectManifest>>;

    async fn put_multipart_upload(&self, upload: ObjectMultipartUpload) -> Result<CommitEntry>;
    async fn get_multipart_upload(&self, upload_id: &str) -> Result<Option<ObjectMultipartUpload>>;
    async fn delete_multipart_upload(
        &self,
        upload_id: &str,
    ) -> Result<Option<(CommitEntry, ObjectMultipartUpload)>>;
    async fn list_multipart_uploads(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectMultipartUpload>>;
}

/// Writes `manifest` with no expected-state clauses and returns the manifest
/// it superseded, decoded from the commit authority's own read.
///
/// Callers use the returned manifest — never a pre-read taken outside the
/// authority — to decide which of the superseded manifest's blobs the new
/// manifest still retains.
///
/// # Errors
/// Fails if the commit fails. A clause-free write cannot be rejected, so a
/// rejection here is an internal contract violation, not a client error.
pub async fn put_manifest_unconditional(
    meta: &dyn S3ObjectMeta,
    manifest: ObjectManifest,
) -> Result<Option<ObjectManifest>> {
    match meta.put_manifest_conditional(manifest, Vec::new()).await? {
        ObjectConditionOutcome::Committed { previous, .. } => Ok(previous),
        ObjectConditionOutcome::Rejected { .. } => Err(Error::Internal(
            "a manifest write with no expected state cannot be rejected".to_string(),
        )),
    }
}

/// Resolves a tenant into its byte/metadata planes for the S3 and Convex
/// storage surfaces.
///
/// [`resolve`](Self::resolve) is a pure existence check: called against a
/// tenant that was never created, it must fail with the same error a
/// per-call lookup would have produced today — it must not implicitly
/// create the tenant. Auto-create-on-first-write (today's behavior on the S3
/// `PutObject`/`CreateMultipartUpload` and Convex store/import paths) is a
/// separate, explicit action via [`ensure_tenant`](Self::ensure_tenant),
/// which those call sites invoke before `resolve`.
#[async_trait]
pub trait S3TenantResolver: Send + Sync + 'static {
    /// Resolves an existing tenant's byte/metadata planes. Fails if the
    /// tenant does not exist; never creates one.
    async fn resolve(&self, tenant: &TenantId) -> Result<S3TenantObjects>;

    /// Creates the tenant if it does not already exist. Idempotent.
    async fn ensure_tenant(&self, tenant: &TenantId) -> Result<()>;
}
