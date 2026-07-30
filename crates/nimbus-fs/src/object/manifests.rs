//! The manifest-plane capability the object backend depends on.
//!
//! Declared here rather than imported from `nimbus-storage`: this crate names
//! the four operations [`ObjectRwBackend`](super::ObjectRwBackend) actually
//! performs, and whoever mounts the backend supplies an implementation. The
//! manifest DTOs stay shared with storage; only the capability is inverted.

use nimbus_core::Result;
use nimbus_storage::ObjectManifest;

/// Named-object manifest plane for the bucket an
/// [`ObjectRwBackend`](super::ObjectRwBackend) is mounted on.
///
/// # Fencing contract
///
/// [`put_manifest`](Self::put_manifest) and
/// [`delete_manifest`](Self::delete_manifest) publish object metadata. A
/// production implementation **must** route them through the engine's
/// committer-sequenced object commit path (`nimbus-engine`'s
/// `TenantObjectMeta`), which assigns the journal sequence inside the tenant
/// committer actor under the committer lease, persists through the fenced
/// provider batch, advances the durable/applied watermarks, and fans out to
/// subscriptions. `nimbus-s3`'s `S3ObjectMeta` is the same inversion for the
/// S3 surface and is implemented that way by `nimbus-server`; this trait is
/// synchronous because `deno_fs::FileSystem` is, so an implementation over the
/// async engine bridges through the byte-plane blocking adapter.
///
/// `nimbus-storage` deliberately exposes no object-write API on its tenant
/// stores. Writing a manifest row straight to a store would assign a commit
/// sequence outside the committer, leave the engine's watermarks stale, skip
/// the provider fence, and let two writers on the same key interleave — the
/// defect SUC2.2 removed. Implementing this trait directly over a raw store is
/// therefore valid only in tests, which own their whole tenant.
pub trait ObjectManifestStore: Send + Sync {
    /// Manifest for `bucket`/`key`, or `None` when no object is published
    /// there.
    fn get_manifest(&self, bucket: &str, key: &str) -> Result<Option<ObjectManifest>>;

    /// Manifests in `bucket` whose key starts with `prefix`, ordered by key and
    /// truncated to `limit` entries.
    fn list_manifests(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectManifest>>;

    /// Publishes `manifest`, replacing any manifest already at its
    /// bucket/key.
    fn put_manifest(&self, manifest: &ObjectManifest) -> Result<()>;

    /// Unpublishes the manifest at `bucket`/`key`. Deleting an absent manifest
    /// succeeds and publishes nothing.
    fn delete_manifest(&self, bucket: &str, key: &str) -> Result<()>;
}
