use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use nimbus_blob::{BlobHash, BlobStore, MemoryBlobStore};
use nimbus_core::{
    CommitEntry, Error, Result, SequenceNumber, SystemWallClock, TenantId, WallClock,
};
use nimbus_storage::{
    ObjectBlobLayout, ObjectChunkRef, ObjectConditionOutcome, ObjectExpectedState, ObjectManifest,
    ObjectManifestAttributes, ObjectMultipartUpload, ObjectUploadConditionOutcome,
    ObjectUploadExpectedState,
};
use s3s::auth::{Credentials, SecretKey};
use s3s::dto::{
    ChecksumAlgorithm, CompleteMultipartUploadInput, CompletedMultipartUpload, CompletedPart,
    CreateMultipartUploadInput, ETag, ETagCondition, GetObjectInput, HeadObjectInput,
    ListObjectsV2Input, PutObjectInput, Range, StreamingBlob, UploadPartInput,
};
use s3s::{Body, S3, S3ErrorCode, S3Request, S3Result};

use crate::checksum::ComputedChecksums;
use crate::convex::{
    CONVEX_STORAGE_BUCKET, ConvexObjectStorage, ConvexStorageError, ConvexStorageId,
    DownloadTokenSigner,
};
use crate::{
    AccessKeyRegistry, NimbusS3, S3ObjectMeta, S3TenantBlobs, S3TenantObjects, S3TenantResolver,
    put_manifest_unconditional,
};

const ACCESS_KEY_A: &str = "AKIATESTANTA";
const ACCESS_KEY_B: &str = "AKIATESTANTB";
const BUCKET: &str = "bucket";

/// Shared mutable state behind [`InMemoryBackend`], held via `Arc` so a
/// resolved [`InMemoryTenantMeta`] handle can outlive the `&self` call that
/// created it without borrowing back from `InMemoryBackend` itself.
#[derive(Default)]
struct Inner {
    blobs: Mutex<HashMap<TenantId, Arc<MemoryBlobStore>>>,
    manifests: Mutex<BTreeMap<(TenantId, String, String), ObjectManifest>>,
    uploads: Mutex<BTreeMap<(TenantId, String), ObjectMultipartUpload>>,
    known_tenants: Mutex<HashSet<TenantId>>,
    fail_put_manifest: AtomicBool,
    fail_put_multipart_upload: AtomicBool,
    /// Counts calls to [`InMemoryBlobs::resolve`], so tests can assert that
    /// metadata-only operations never resolve (or create) blob-plane state.
    blob_resolutions: AtomicUsize,
    /// Counts manifest writes that actually committed, so tests can assert
    /// that a rejected condition consumed no commit at all.
    manifest_commits: AtomicUsize,
}

#[derive(Default)]
struct InMemoryBackend {
    inner: Arc<Inner>,
}

#[async_trait]
impl S3TenantResolver for InMemoryBackend {
    async fn resolve(&self, tenant: &TenantId) -> Result<S3TenantObjects> {
        if !self.inner.known_tenants.lock().unwrap().contains(tenant) {
            return Err(Error::TenantNotFound(tenant.clone()));
        }
        let blobs = Arc::new(InMemoryBlobs {
            tenant: tenant.clone(),
            inner: self.inner.clone(),
            resolved: tokio::sync::OnceCell::new(),
        });
        Ok(S3TenantObjects::new(blobs, self.meta(tenant)))
    }

    async fn ensure_tenant(&self, tenant: &TenantId) -> Result<()> {
        self.inner
            .known_tenants
            .lock()
            .unwrap()
            .insert(tenant.clone());
        self.store(tenant);
        Ok(())
    }
}

impl InMemoryBackend {
    fn store(&self, tenant: &TenantId) -> Arc<MemoryBlobStore> {
        self.inner
            .blobs
            .lock()
            .unwrap()
            .entry(tenant.clone())
            .or_insert_with(|| Arc::new(MemoryBlobStore::new()))
            .clone()
    }

    fn meta(&self, tenant: &TenantId) -> Arc<InMemoryTenantMeta> {
        Arc::new(InMemoryTenantMeta {
            tenant: tenant.clone(),
            inner: self.inner.clone(),
        })
    }

    /// Number of times a resolved [`S3TenantObjects`] for this backend has
    /// had its lazy blob accessor actually invoked. Used to assert
    /// metadata-only S3/Convex operations never touch the byte plane.
    fn blob_resolutions(&self) -> usize {
        self.inner.blob_resolutions.load(Ordering::SeqCst)
    }

    /// Number of manifest writes that committed against this backend. A
    /// rejected condition must not move this.
    fn manifest_commits(&self) -> usize {
        self.inner.manifest_commits.load(Ordering::SeqCst)
    }

    /// Part numbers the durable upload record holds, in order. Reads the
    /// backing map directly so the assertion does not depend on the surface
    /// under test.
    fn durable_part_numbers(&self, tenant: &TenantId, upload_id: &str) -> Vec<u32> {
        self.inner
            .uploads
            .lock()
            .unwrap()
            .get(&(tenant.clone(), upload_id.to_string()))
            .map(|upload| {
                upload
                    .parts
                    .iter()
                    .map(|part| part.part_number)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    fn convex_manifest_keys(&self, tenant: &TenantId) -> Vec<String> {
        self.inner
            .manifests
            .lock()
            .unwrap()
            .iter()
            .filter_map(|((entry_tenant, entry_bucket, _), manifest)| {
                (entry_tenant == tenant && entry_bucket == CONVEX_STORAGE_BUCKET)
                    .then_some(manifest.key.clone())
            })
            .collect()
    }

    // Test-setup conveniences mirroring what a resolved `S3TenantObjects`
    // would offer, used by tests that poke tenant state directly (bypassing
    // the `NimbusS3`/`ConvexObjectStorage` surfaces under test).
    async fn put_blob(&self, tenant: &TenantId, bytes: Bytes) -> Result<BlobHash> {
        self.store(tenant).put(bytes).await
    }

    async fn put_manifest(
        &self,
        tenant: &TenantId,
        manifest: ObjectManifest,
    ) -> Result<Option<ObjectManifest>> {
        put_manifest_unconditional(self.meta(tenant).as_ref(), manifest).await
    }

    async fn list_manifests(
        &self,
        tenant: &TenantId,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectManifest>> {
        self.meta(tenant)
            .list_manifests(bucket, prefix, limit)
            .await
    }

    async fn release_blob(&self, tenant: &TenantId, hash: &BlobHash) -> Result<()> {
        self.store(tenant).release(hash).await
    }
}

/// Lazy [`S3TenantBlobs`] handle for a resolved [`InMemoryBackend`] tenant.
/// Resolution is deferred to [`resolve`](S3TenantBlobs::resolve) and
/// memoized in `resolved`, mirroring how the real engine-backed resolver
/// only opens per-tenant byte-plane state once a byte operation actually
/// needs it, and only opens it once per request.
struct InMemoryBlobs {
    tenant: TenantId,
    inner: Arc<Inner>,
    resolved: tokio::sync::OnceCell<Arc<dyn BlobStore>>,
}

#[async_trait]
impl S3TenantBlobs for InMemoryBlobs {
    async fn resolve(&self) -> Result<Arc<dyn BlobStore>> {
        self.resolved
            .get_or_try_init(|| async {
                self.inner.blob_resolutions.fetch_add(1, Ordering::SeqCst);
                let store = self
                    .inner
                    .blobs
                    .lock()
                    .unwrap()
                    .entry(self.tenant.clone())
                    .or_insert_with(|| Arc::new(MemoryBlobStore::new()))
                    .clone();
                Ok::<Arc<dyn BlobStore>, Error>(store)
            })
            .await
            .map(Arc::clone)
    }
}

struct InMemoryTenantMeta {
    tenant: TenantId,
    inner: Arc<Inner>,
}

#[async_trait]
impl S3ObjectMeta for InMemoryTenantMeta {
    async fn put_manifest_conditional(
        &self,
        manifest: ObjectManifest,
        expected: Vec<ObjectExpectedState>,
    ) -> Result<ObjectConditionOutcome> {
        // Every real metadata call crosses to the tenant committer and awaits
        // it. Yielding here keeps that property in the double: a caller that
        // reads, decides, and then writes really can be overtaken between the
        // two calls, which is the whole defect the conditional seam closes.
        tokio::task::yield_now().await;
        if self.inner.fail_put_manifest.load(Ordering::SeqCst) {
            return Err(nimbus_core::Error::storage(
                nimbus_core::StorageErrorKind::Unavailable,
                "injected put_manifest failure",
            ));
        }
        // One lock hold covers the read, the decision, and the write. This is
        // the same exclusion the tenant committer actor gives the real path:
        // no other writer at this key can land between the decision and the
        // commit, so a condition decided here is decided against the state
        // the write actually replaces.
        let mut manifests = self.inner.manifests.lock().unwrap();
        let slot = (
            self.tenant.clone(),
            manifest.bucket.clone(),
            manifest.key.clone(),
        );
        let current = manifests.get(&slot).cloned();
        if let Some(unmet) =
            ObjectExpectedState::first_unmet(&expected, current.as_ref().map(|m| m.etag.as_str()))
        {
            return Ok(ObjectConditionOutcome::Rejected {
                unmet: unmet.clone(),
                current,
            });
        }
        let previous = manifests.insert(slot, manifest);
        drop(manifests);
        self.inner.manifest_commits.fetch_add(1, Ordering::SeqCst);
        Ok(ObjectConditionOutcome::Committed {
            commit: commit(),
            previous,
        })
    }

    async fn get_manifest(&self, bucket: &str, key: &str) -> Result<Option<ObjectManifest>> {
        tokio::task::yield_now().await;
        Ok(self
            .inner
            .manifests
            .lock()
            .unwrap()
            .get(&(self.tenant.clone(), bucket.to_string(), key.to_string()))
            .cloned())
    }

    async fn delete_manifest(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Option<(CommitEntry, ObjectManifest)>> {
        Ok(self
            .inner
            .manifests
            .lock()
            .unwrap()
            .remove(&(self.tenant.clone(), bucket.to_string(), key.to_string()))
            .map(|manifest| (commit(), manifest)))
    }

    async fn list_manifests(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectManifest>> {
        let manifests_guard = self.inner.manifests.lock().unwrap();
        let mut manifests = manifests_guard
            .iter()
            .filter_map(|((entry_tenant, entry_bucket, _), manifest)| {
                (entry_tenant == &self.tenant
                    && entry_bucket == bucket
                    && manifest.key.starts_with(prefix))
                .then_some(manifest.clone())
            })
            .collect::<Vec<_>>();
        drop(manifests_guard);
        manifests.sort_by(|left, right| left.key.cmp(&right.key));
        manifests.truncate(limit);
        Ok(manifests)
    }

    async fn put_multipart_upload_conditional(
        &self,
        upload: ObjectMultipartUpload,
        expected: Vec<ObjectUploadExpectedState>,
    ) -> Result<ObjectUploadConditionOutcome> {
        // Yields before the decision so a concurrent writer at the same
        // upload id interleaves here, where a read-then-write surface would
        // already have lost a part.
        tokio::task::yield_now().await;
        if self.inner.fail_put_multipart_upload.load(Ordering::SeqCst) {
            return Err(nimbus_core::Error::storage(
                nimbus_core::StorageErrorKind::Unavailable,
                "injected put_multipart_upload failure",
            ));
        }
        // One lock hold covers the read, the decision, and the write, which is
        // the exclusion the tenant committer actor gives the real path.
        let mut uploads = self.inner.uploads.lock().unwrap();
        let slot = (self.tenant.clone(), upload.upload_id.clone());
        let current = uploads.get(&slot).cloned();
        if let Some(unmet) = ObjectUploadExpectedState::first_unmet(
            &expected,
            current.as_ref().map(|upload| upload.revision),
        ) {
            return Ok(ObjectUploadConditionOutcome::Rejected {
                unmet: unmet.clone(),
                current,
            });
        }
        let previous = uploads.insert(slot, upload);
        drop(uploads);
        Ok(ObjectUploadConditionOutcome::Committed {
            commit: commit(),
            previous,
        })
    }

    async fn get_multipart_upload(&self, upload_id: &str) -> Result<Option<ObjectMultipartUpload>> {
        // Yields so a concurrent caller can observe the same upload image that
        // this one just read. A read-modify-write that decides outside the
        // authority loses a part here; one that carries its expected revision
        // into the write does not.
        tokio::task::yield_now().await;
        Ok(self
            .inner
            .uploads
            .lock()
            .unwrap()
            .get(&(self.tenant.clone(), upload_id.to_string()))
            .cloned())
    }

    async fn delete_multipart_upload_conditional(
        &self,
        upload_id: &str,
        expected: Vec<ObjectUploadExpectedState>,
    ) -> Result<ObjectUploadConditionOutcome> {
        tokio::task::yield_now().await;
        let mut uploads = self.inner.uploads.lock().unwrap();
        let slot = (self.tenant.clone(), upload_id.to_string());
        let current = uploads.get(&slot).cloned();
        if let Some(unmet) = ObjectUploadExpectedState::first_unmet(
            &expected,
            current.as_ref().map(|upload| upload.revision),
        ) {
            return Ok(ObjectUploadConditionOutcome::Rejected {
                unmet: unmet.clone(),
                current,
            });
        }
        let previous = uploads.remove(&slot);
        drop(uploads);
        Ok(ObjectUploadConditionOutcome::Committed {
            commit: commit(),
            previous,
        })
    }

    async fn list_multipart_uploads(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectMultipartUpload>> {
        let uploads_guard = self.inner.uploads.lock().unwrap();
        let mut uploads = uploads_guard
            .iter()
            .filter_map(|((entry_tenant, _), upload)| {
                (entry_tenant == &self.tenant
                    && upload.bucket == bucket
                    && upload.key.starts_with(prefix))
                .then_some(upload.clone())
            })
            .collect::<Vec<_>>();
        drop(uploads_guard);
        uploads.sort_by(|left, right| left.upload_id.cmp(&right.upload_id));
        uploads.truncate(limit);
        Ok(uploads)
    }
}

fn commit() -> CommitEntry {
    CommitEntry {
        sequence: SequenceNumber(1),
        timestamp: SystemWallClock.now(),
        writes: Vec::new(),
    }
}

fn service() -> NimbusS3 {
    service_with_backend(Arc::new(InMemoryBackend::default()))
}

fn service_with_backend(backend: Arc<InMemoryBackend>) -> NimbusS3 {
    let registry = AccessKeyRegistry::new()
        .bind_signed(ACCESS_KEY_A, tenant("tenant-a"), "secret-a")
        .bind_signed(ACCESS_KEY_B, tenant("tenant-b"), "secret-b");
    NimbusS3::new(backend, registry)
}

fn tenant(value: &str) -> TenantId {
    TenantId::new(value).expect("tenant id")
}

fn req<T>(input: T, access_key: &str) -> S3Request<T> {
    S3Request {
        input,
        method: http::Method::GET,
        uri: "/".parse().unwrap(),
        headers: http::HeaderMap::new(),
        extensions: http::Extensions::new(),
        credentials: Some(Credentials {
            access_key: access_key.to_string(),
            secret_key: SecretKey::from("secret"),
        }),
        region: None,
        service: None,
        trailing_headers: None,
    }
}

fn blob(bytes: &'static [u8]) -> StreamingBlob {
    StreamingBlob::from(Body::from(Bytes::from_static(bytes)))
}

/// Same as [`blob`], for bodies a test computes at runtime.
fn blob_owned(bytes: Vec<u8>) -> StreamingBlob {
    StreamingBlob::from(Body::from(Bytes::from(bytes)))
}

async fn collect(body: StreamingBlob) -> Bytes {
    let mut out = Vec::new();
    futures::pin_mut!(body);
    while let Some(chunk) = body.next().await {
        out.extend_from_slice(&chunk.expect("stream chunk"));
    }
    Bytes::from(out)
}

async fn put(service: &NimbusS3, access_key: &str, key: &str, bytes: &'static [u8]) -> String {
    let response = service
        .put_object(req(
            PutObjectInput {
                bucket: BUCKET.to_string(),
                key: key.to_string(),
                body: Some(blob(bytes)),
                content_length: Some(bytes.len() as i64),
                content_type: Some("text/plain".to_string()),
                ..Default::default()
            },
            access_key,
        ))
        .await
        .expect("put should succeed");
    response.output.e_tag.expect("etag").into_value()
}

#[tokio::test]
async fn put_get_range_and_list_are_s3_shaped() {
    let service = service();
    let etag = put(&service, ACCESS_KEY_A, "docs/readme.txt", b"hello world").await;
    put(&service, ACCESS_KEY_A, "docs/archive/log.txt", b"archived").await;

    let full = service
        .get_object(req(
            GetObjectInput {
                bucket: BUCKET.to_string(),
                key: "docs/readme.txt".to_string(),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("get should succeed");
    assert_eq!(
        collect(full.output.body.expect("body")).await,
        Bytes::from_static(b"hello world")
    );
    assert_eq!(full.output.e_tag.unwrap().into_value(), etag);

    let ranged = service
        .get_object(req(
            GetObjectInput {
                bucket: BUCKET.to_string(),
                key: "docs/readme.txt".to_string(),
                range: Some(Range::Int {
                    first: 6,
                    last: Some(10),
                }),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("range get should succeed");
    assert_eq!(ranged.status, Some(http::StatusCode::PARTIAL_CONTENT));
    assert_eq!(
        collect(ranged.output.body.expect("range body")).await,
        Bytes::from_static(b"world")
    );

    let listing = service
        .list_objects_v2(req(
            ListObjectsV2Input {
                bucket: BUCKET.to_string(),
                prefix: Some("docs/".to_string()),
                delimiter: Some("/".to_string()),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("list should succeed");
    assert_eq!(
        listing
            .output
            .contents
            .unwrap()
            .iter()
            .map(|object| object.key.as_deref().unwrap())
            .collect::<Vec<_>>(),
        vec!["docs/readme.txt"]
    );
    assert_eq!(
        listing.output.common_prefixes.unwrap()[0].prefix.as_deref(),
        Some("docs/archive/")
    );
}

#[tokio::test]
async fn access_keys_scope_objects_to_their_own_tenant_and_reject_cross_tenant_fetch_of_the_same_key_path()
 {
    let service = service();
    put(&service, ACCESS_KEY_A, "same/key.txt", b"tenant-a").await;
    put(&service, ACCESS_KEY_B, "same/key.txt", b"tenant-b").await;

    for (access_key, expected) in [
        (ACCESS_KEY_A, Bytes::from_static(b"tenant-a")),
        (ACCESS_KEY_B, Bytes::from_static(b"tenant-b")),
    ] {
        let response = service
            .get_object(req(
                GetObjectInput {
                    bucket: BUCKET.to_string(),
                    key: "same/key.txt".to_string(),
                    ..Default::default()
                },
                access_key,
            ))
            .await
            .expect("tenant get should succeed");
        assert_eq!(collect(response.output.body.unwrap()).await, expected);
    }

    // The round trip above proves each access key's own store resolves the shared
    // key path correctly, but it never attempts a cross-tenant read: a bug that
    // resolved objects by key alone, ignoring which tenant's store owns the
    // access key, would not be caught by same-key round trips alone. Put an
    // object that exists ONLY under access key A, then fetch that exact key
    // using access key B's credentials and require the same not-found refusal
    // as a key that was never written at all.
    put(&service, ACCESS_KEY_A, "alpha-only/key.txt", b"alpha-only").await;
    let cross_tenant_fetch = service
        .get_object(req(
            GetObjectInput {
                bucket: BUCKET.to_string(),
                key: "alpha-only/key.txt".to_string(),
                ..Default::default()
            },
            ACCESS_KEY_B,
        ))
        .await
        .expect_err("access key B must not resolve an object owned by access key A's tenant");
    assert_eq!(cross_tenant_fetch.code(), &S3ErrorCode::NoSuchKey);
}

#[tokio::test]
async fn overwriting_object_with_same_bytes_keeps_blob_readable() {
    let service = service();
    put(&service, ACCESS_KEY_A, "same-bytes.txt", b"stable").await;
    put(&service, ACCESS_KEY_A, "same-bytes.txt", b"stable").await;

    let response = service
        .get_object(req(
            GetObjectInput {
                bucket: BUCKET.to_string(),
                key: "same-bytes.txt".to_string(),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("overwritten object should remain readable");
    assert_eq!(
        collect(response.output.body.unwrap()).await,
        Bytes::from_static(b"stable")
    );
}

#[tokio::test]
async fn put_object_releases_new_blob_when_manifest_commit_fails() {
    let backend = Arc::new(InMemoryBackend::default());
    let service = service_with_backend(backend.clone());
    backend
        .inner
        .fail_put_manifest
        .store(true, Ordering::SeqCst);

    service
        .put_object(req(
            PutObjectInput {
                bucket: BUCKET.to_string(),
                key: "commit-fails.txt".to_string(),
                body: Some(blob(b"uncommitted bytes")),
                content_length: Some(17),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect_err("manifest commit failure should fail the S3 write");

    let hash = BlobHash::of(b"uncommitted bytes");
    assert!(
        !backend
            .store(&tenant("tenant-a"))
            .has(&hash)
            .await
            .expect("blob store should answer has"),
        "failed metadata commit must release the newly written blob"
    );
}

#[tokio::test]
async fn conditional_requests_enforce_s3_etag_preconditions() {
    let service = service();
    let original_etag = put(&service, ACCESS_KEY_A, "conditional.txt", b"original").await;

    let created = service
        .put_object(req(
            PutObjectInput {
                bucket: BUCKET.to_string(),
                key: "create-only.txt".to_string(),
                body: Some(blob(b"new")),
                content_length: Some(3),
                if_none_match: Some(ETagCondition::Any),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("If-None-Match:* should create absent objects");
    assert!(created.output.e_tag.is_some());

    let create_conflict = service
        .put_object(req(
            PutObjectInput {
                bucket: BUCKET.to_string(),
                key: "conditional.txt".to_string(),
                body: Some(blob(b"blocked")),
                content_length: Some(7),
                if_none_match: Some(ETagCondition::Any),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect_err("If-None-Match:* must reject existing objects");
    assert_eq!(create_conflict.code(), &S3ErrorCode::PreconditionFailed);

    let weak_update = service
        .put_object(req(
            PutObjectInput {
                bucket: BUCKET.to_string(),
                key: "conditional.txt".to_string(),
                body: Some(blob(b"blocked")),
                content_length: Some(7),
                if_match: Some(ETagCondition::ETag(ETag::Weak(original_etag.clone()))),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect_err("If-Match must use strong ETag comparison");
    assert_eq!(weak_update.code(), &S3ErrorCode::PreconditionFailed);

    let updated = service
        .put_object(req(
            PutObjectInput {
                bucket: BUCKET.to_string(),
                key: "conditional.txt".to_string(),
                body: Some(blob(b"updated")),
                content_length: Some(7),
                if_match: Some(ETagCondition::ETag(ETag::Strong(original_etag.clone()))),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("matching strong If-Match should update");
    let updated_etag = updated.output.e_tag.expect("updated etag").into_value();
    assert_ne!(updated_etag, original_etag);

    let not_modified = service
        .get_object(req(
            GetObjectInput {
                bucket: BUCKET.to_string(),
                key: "conditional.txt".to_string(),
                if_none_match: Some(ETagCondition::ETag(ETag::Weak(updated_etag.clone()))),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect_err("matching If-None-Match should return NotModified");
    assert_eq!(not_modified.code(), &S3ErrorCode::NotModified);

    let head_precondition = service
        .head_object(req(
            HeadObjectInput {
                bucket: BUCKET.to_string(),
                key: "conditional.txt".to_string(),
                if_match: Some(ETagCondition::ETag(ETag::Strong(original_etag))),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect_err("stale If-Match should reject HEAD");
    assert_eq!(head_precondition.code(), &S3ErrorCode::PreconditionFailed);

    let head = service
        .head_object(req(
            HeadObjectInput {
                bucket: BUCKET.to_string(),
                key: "conditional.txt".to_string(),
                if_match: Some(ETagCondition::ETag(ETag::Strong(updated_etag))),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("current If-Match should allow HEAD");
    assert_eq!(head.output.content_length, Some(7));
}

/// Issues one `PutObject` carrying write preconditions and returns the new
/// `ETag`, or the S3 error the request was refused with.
async fn conditional_put(
    service: &NimbusS3,
    key: &str,
    bytes: &'static [u8],
    if_match: Option<ETagCondition>,
    if_none_match: Option<ETagCondition>,
) -> S3Result<String> {
    service
        .put_object(req(
            PutObjectInput {
                bucket: BUCKET.to_string(),
                key: key.to_string(),
                body: Some(blob(bytes)),
                content_length: Some(bytes.len() as i64),
                if_match,
                if_none_match,
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .map(|response| response.output.e_tag.expect("etag").into_value())
}

async fn read_object(service: &NimbusS3, key: &str) -> Bytes {
    let response = service
        .get_object(req(
            GetObjectInput {
                bucket: BUCKET.to_string(),
                key: key.to_string(),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("object should be readable");
    collect(response.output.body.expect("object body")).await
}

async fn head_etag(service: &NimbusS3, key: &str) -> String {
    service
        .head_object(req(
            HeadObjectInput {
                bucket: BUCKET.to_string(),
                key: key.to_string(),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("object should be head-able")
        .output
        .e_tag
        .expect("etag")
        .into_value()
}

/// The four-write conditional shape, sequentially: create under
/// `If-None-Match: *`, have a second create refused, update under the current
/// `If-Match`, and have the now-stale `If-Match` refused.
///
/// Sequential coverage pins the response mapping and the surviving state. It
/// cannot see the race the concurrent probe below covers — before the
/// condition moved to the commit authority, this test passed while two
/// concurrent creates at one key both succeeded.
#[tokio::test]
async fn conditional_put_probe_create_reject_update_reject_stale() {
    let backend = Arc::new(InMemoryBackend::default());
    let service = service_with_backend(backend.clone());
    let key = "probe.txt";

    let created = conditional_put(&service, key, b"created", None, Some(ETagCondition::Any))
        .await
        .expect("If-None-Match: * must create an absent object");
    assert_eq!(
        read_object(&service, key).await,
        Bytes::from_static(b"created")
    );

    let duplicate = conditional_put(&service, key, b"duplicate", None, Some(ETagCondition::Any))
        .await
        .expect_err("If-None-Match: * must refuse an existing object");
    assert_eq!(duplicate.code(), &S3ErrorCode::PreconditionFailed);
    assert_eq!(
        read_object(&service, key).await,
        Bytes::from_static(b"created")
    );

    let updated = conditional_put(
        &service,
        key,
        b"updated",
        Some(ETagCondition::ETag(ETag::Strong(created.clone()))),
        None,
    )
    .await
    .expect("the current If-Match must update the object");
    assert_ne!(updated, created);
    assert_eq!(
        read_object(&service, key).await,
        Bytes::from_static(b"updated")
    );

    let stale = conditional_put(
        &service,
        key,
        b"stale writer",
        Some(ETagCondition::ETag(ETag::Strong(created))),
        None,
    )
    .await
    .expect_err("a stale If-Match must be refused");
    assert_eq!(stale.code(), &S3ErrorCode::PreconditionFailed);
    assert_eq!(
        read_object(&service, key).await,
        Bytes::from_static(b"updated")
    );
    assert_eq!(
        backend.manifest_commits(),
        2,
        "only the create and the update may consume a commit"
    );
}

/// Many concurrent `If-None-Match: *` creates at one key must admit exactly
/// one claimant.
///
/// This is the probe the sequential test cannot be: while the S3 surface read
/// the manifest and then decided the precondition against that read, every
/// claimant observed the key as absent and every claimant was admitted. The
/// clauses now travel to the commit authority, which decides them against its
/// own serialized read.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn conditional_put_if_none_match_is_linearizable() {
    const CLAIMANTS: usize = 100;
    let backend = Arc::new(InMemoryBackend::default());
    let service = service_with_backend(backend.clone());
    let key = "linearizable.txt";

    let mut claimants = Vec::with_capacity(CLAIMANTS);
    for index in 0..CLAIMANTS {
        let service = service.clone();
        claimants.push(tokio::spawn(async move {
            let body = Bytes::from(format!("claimant-{index:03}"));
            let content_length = body.len() as i64;
            service
                .put_object(req(
                    PutObjectInput {
                        bucket: BUCKET.to_string(),
                        key: key.to_string(),
                        body: Some(StreamingBlob::from(Body::from(body.clone()))),
                        content_length: Some(content_length),
                        if_none_match: Some(ETagCondition::Any),
                        ..Default::default()
                    },
                    ACCESS_KEY_A,
                ))
                .await
                .map(|_| body)
        }));
    }

    let mut admitted = Vec::new();
    let mut refused = 0usize;
    for claimant in claimants {
        match claimant.await.expect("claimant task should not panic") {
            Ok(body) => admitted.push(body),
            Err(error) => {
                assert_eq!(
                    error.code(),
                    &S3ErrorCode::PreconditionFailed,
                    "a losing claimant must be refused with PreconditionFailed"
                );
                refused += 1;
            }
        }
    }

    assert_eq!(
        admitted.len(),
        1,
        "exactly one of {CLAIMANTS} concurrent If-None-Match: * creates may be admitted"
    );
    assert_eq!(refused, CLAIMANTS - 1);
    assert_eq!(
        backend.manifest_commits(),
        1,
        "the refused claimants must consume no commit"
    );
    assert_eq!(
        read_object(&service, key).await,
        admitted[0],
        "the stored object must be the admitted claimant's bytes"
    );
}

/// A refused condition must leave no commit and no byte-plane damage.
///
/// The identical-bytes case is the dangerous one: both writers resolve to the
/// same content hash, so the loser's cleanup is exactly what can delete the
/// winner's object. The loser releases its hash only when the manifest the
/// authority reports does not name it.
#[tokio::test]
async fn rejected_object_condition_has_no_commit_or_blob_effect() {
    let backend = Arc::new(InMemoryBackend::default());
    let service = service_with_backend(backend.clone());
    let store = backend.store(&tenant("tenant-a"));
    let key = "rejected.txt";

    let winner = conditional_put(&service, key, b"identical", None, Some(ETagCondition::Any))
        .await
        .expect("the first claimant must be admitted");
    let commits_after_winner = backend.manifest_commits();

    // Same bytes, same content hash: the loser must not release what the
    // winner's manifest still names.
    let loser = conditional_put(&service, key, b"identical", None, Some(ETagCondition::Any))
        .await
        .expect_err("the second claimant must be refused");
    assert_eq!(loser.code(), &S3ErrorCode::PreconditionFailed);
    assert_eq!(
        backend.manifest_commits(),
        commits_after_winner,
        "a refused condition must consume no commit"
    );
    assert_eq!(
        head_etag(&service, key).await,
        winner,
        "the head must not move"
    );
    assert!(
        store
            .has(&BlobHash::of(b"identical"))
            .await
            .expect("blob store should answer has"),
        "the loser's cleanup must keep bytes the winning manifest retains"
    );
    assert_eq!(
        read_object(&service, key).await,
        Bytes::from_static(b"identical")
    );

    // Different bytes on a stale If-Match: nothing retains the new blob, so
    // the refused write must not leave it behind either.
    let stale = conditional_put(
        &service,
        key,
        b"orphan bytes",
        Some(ETagCondition::ETag(ETag::Strong("stale-etag".to_string()))),
        None,
    )
    .await
    .expect_err("a stale If-Match must be refused");
    assert_eq!(stale.code(), &S3ErrorCode::PreconditionFailed);
    assert_eq!(
        backend.manifest_commits(),
        commits_after_winner,
        "a refused condition must consume no commit"
    );
    assert_eq!(
        head_etag(&service, key).await,
        winner,
        "the head must not move"
    );
    assert!(
        !store
            .has(&BlobHash::of(b"orphan bytes"))
            .await
            .expect("blob store should answer has"),
        "a refused write must release the blob no manifest retains"
    );
    assert_eq!(
        read_object(&service, key).await,
        Bytes::from_static(b"identical")
    );
}

#[tokio::test]
async fn put_object_rejects_unsupported_checksum_headers() {
    let service = service();
    let sha256 = service
        .put_object(req(
            PutObjectInput {
                bucket: BUCKET.to_string(),
                key: "checksums/sha256.txt".to_string(),
                body: Some(blob(b"payload")),
                checksum_sha256: Some("unsupported".to_string()),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect_err("unsupported checksum must fail closed");
    assert_eq!(sha256.code(), &S3ErrorCode::InvalidRequest);
    assert!(sha256.message().unwrap_or_default().contains("SHA256"));

    let missing_crc64 = service
        .put_object(req(
            PutObjectInput {
                bucket: BUCKET.to_string(),
                key: "checksums/missing-crc64.txt".to_string(),
                body: Some(blob(b"payload")),
                checksum_algorithm: Some(ChecksumAlgorithm::from_static(
                    ChecksumAlgorithm::CRC64NVME,
                )),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect_err("CRC64NVME algorithm must include the checksum header");
    assert_eq!(missing_crc64.code(), &S3ErrorCode::InvalidRequest);
    assert!(
        missing_crc64
            .message()
            .unwrap_or_default()
            .contains("requires x-amz-checksum-crc64nvme")
    );
}

#[test]
fn crc64nvme_trailer_extraction_uses_verified_trailing_headers() {
    let bytes = Bytes::from_static(b"payload");
    let checksum = ComputedChecksums::for_bytes(&bytes).crc64nvme_base64;
    let mut headers = http::HeaderMap::new();
    headers.insert(
        "x-amz-checksum-crc64nvme",
        checksum.parse().expect("checksum header value"),
    );

    assert_eq!(
        crate::service::trailing_crc64nvme_from_headers(&headers).expect("trailer should parse"),
        Some(checksum)
    );
}

#[tokio::test]
async fn multipart_upload_assembles_chunks_and_etag() {
    let service = service();
    let created = service
        .create_multipart_upload(req(
            CreateMultipartUploadInput {
                bucket: BUCKET.to_string(),
                key: "large/object.txt".to_string(),
                content_type: Some("text/plain".to_string()),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("create multipart should succeed");
    let upload_id = created.output.upload_id.expect("upload id");

    let first = service
        .upload_part(req(
            UploadPartInput {
                bucket: BUCKET.to_string(),
                key: "large/object.txt".to_string(),
                upload_id: upload_id.clone(),
                part_number: 1,
                body: Some(blob(b"hello ")),
                content_length: Some(6),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("first part should upload")
        .output
        .e_tag
        .expect("first part etag");
    let second = service
        .upload_part(req(
            UploadPartInput {
                bucket: BUCKET.to_string(),
                key: "large/object.txt".to_string(),
                upload_id: upload_id.clone(),
                part_number: 2,
                body: Some(blob(b"world")),
                content_length: Some(5),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("second part should upload")
        .output
        .e_tag
        .expect("second part etag");

    let completed = service
        .complete_multipart_upload(req(
            s3s::dto::CompleteMultipartUploadInput {
                bucket: BUCKET.to_string(),
                key: "large/object.txt".to_string(),
                upload_id,
                multipart_upload: Some(CompletedMultipartUpload {
                    parts: Some(vec![
                        CompletedPart {
                            part_number: Some(1),
                            e_tag: Some(first),
                            ..Default::default()
                        },
                        CompletedPart {
                            part_number: Some(2),
                            e_tag: Some(second),
                            ..Default::default()
                        },
                    ]),
                }),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("multipart complete should succeed");
    assert!(
        completed
            .output
            .e_tag
            .expect("complete etag")
            .into_value()
            .ends_with("-2")
    );

    let fetched = service
        .get_object(req(
            GetObjectInput {
                bucket: BUCKET.to_string(),
                key: "large/object.txt".to_string(),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("multipart object should read");
    assert_eq!(
        collect(fetched.output.body.unwrap()).await,
        Bytes::from_static(b"hello world")
    );
}

#[tokio::test]
async fn replacing_duplicate_multipart_part_keeps_shared_blob_readable() {
    let service = service();
    let created = service
        .create_multipart_upload(req(
            CreateMultipartUploadInput {
                bucket: BUCKET.to_string(),
                key: "large/duplicates.txt".to_string(),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("create multipart should succeed");
    let upload_id = created.output.upload_id.expect("upload id");

    let first_original = service
        .upload_part(req(
            UploadPartInput {
                bucket: BUCKET.to_string(),
                key: "large/duplicates.txt".to_string(),
                upload_id: upload_id.clone(),
                part_number: 1,
                body: Some(blob(b"same")),
                content_length: Some(4),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("first duplicate part should upload")
        .output
        .e_tag
        .expect("first duplicate etag");
    let second = service
        .upload_part(req(
            UploadPartInput {
                bucket: BUCKET.to_string(),
                key: "large/duplicates.txt".to_string(),
                upload_id: upload_id.clone(),
                part_number: 2,
                body: Some(blob(b"same")),
                content_length: Some(4),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("second duplicate part should upload")
        .output
        .e_tag
        .expect("second duplicate etag");
    assert_eq!(
        first_original, second,
        "identical part bytes share one content address and ETag"
    );

    let replacement = service
        .upload_part(req(
            UploadPartInput {
                bucket: BUCKET.to_string(),
                key: "large/duplicates.txt".to_string(),
                upload_id: upload_id.clone(),
                part_number: 1,
                body: Some(blob(b"new")),
                content_length: Some(3),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("replacement part should upload")
        .output
        .e_tag
        .expect("replacement etag");

    service
        .complete_multipart_upload(req(
            s3s::dto::CompleteMultipartUploadInput {
                bucket: BUCKET.to_string(),
                key: "large/duplicates.txt".to_string(),
                upload_id,
                multipart_upload: Some(CompletedMultipartUpload {
                    parts: Some(vec![
                        CompletedPart {
                            part_number: Some(1),
                            e_tag: Some(replacement),
                            ..Default::default()
                        },
                        CompletedPart {
                            part_number: Some(2),
                            e_tag: Some(second),
                            ..Default::default()
                        },
                    ]),
                }),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("multipart complete should preserve the still-referenced duplicate blob");

    let fetched = service
        .get_object(req(
            GetObjectInput {
                bucket: BUCKET.to_string(),
                key: "large/duplicates.txt".to_string(),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("multipart object should read");
    assert_eq!(
        collect(fetched.output.body.unwrap()).await,
        Bytes::from_static(b"newsame")
    );
}

#[tokio::test]
async fn upload_part_releases_new_blob_when_upload_commit_fails() {
    let backend = Arc::new(InMemoryBackend::default());
    let service = service_with_backend(backend.clone());
    let created = service
        .create_multipart_upload(req(
            CreateMultipartUploadInput {
                bucket: BUCKET.to_string(),
                key: "large/upload-fails.txt".to_string(),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("create multipart should succeed");
    let upload_id = created.output.upload_id.expect("upload id");
    backend
        .inner
        .fail_put_multipart_upload
        .store(true, Ordering::SeqCst);

    service
        .upload_part(req(
            UploadPartInput {
                bucket: BUCKET.to_string(),
                key: "large/upload-fails.txt".to_string(),
                upload_id,
                part_number: 1,
                body: Some(blob(b"uncommitted part")),
                content_length: Some(16),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect_err("upload metadata commit failure should fail the part write");

    let hash = BlobHash::of(b"uncommitted part");
    assert!(
        !backend
            .store(&tenant("tenant-a"))
            .has(&hash)
            .await
            .expect("blob store should answer has"),
        "failed upload metadata commit must release the newly written part blob"
    );
}

#[tokio::test]
async fn chunked_manifest_read_rejects_blob_length_mismatch() {
    let backend = Arc::new(InMemoryBackend::default());
    let service = service_with_backend(backend.clone());
    let tenant = tenant("tenant-a");
    backend.ensure_tenant(&tenant).await.unwrap();
    let hash = backend
        .put_blob(&tenant, Bytes::from_static(b"short"))
        .await
        .unwrap();
    let manifest = ObjectManifest::chunked(
        BUCKET,
        "bad/chunked.bin",
        9,
        vec![ObjectChunkRef {
            blob_hash: hash.to_hex(),
            offset: 0,
            len: 9,
        }],
        ObjectManifestAttributes::new("multipart-etag-1", 1),
    )
    .expect("manifest shape is valid; blob backend length is corrupt");
    backend.put_manifest(&tenant, manifest).await.unwrap();

    let error = service
        .get_object(req(
            GetObjectInput {
                bucket: BUCKET.to_string(),
                key: "bad/chunked.bin".to_string(),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect_err("read must reject chunk length mismatch");

    assert_eq!(error.code(), &S3ErrorCode::InternalError);
    assert!(
        error
            .message()
            .unwrap_or_default()
            .contains("object manifest corruption")
    );
}

#[tokio::test]
async fn convex_storage_projects_virtual_metadata_and_hides_internal_key() {
    let backend = Arc::new(InMemoryBackend::default());
    let storage = ConvexObjectStorage::new(backend.clone());
    let tenant = tenant("tenant-a");

    let metadata = storage
        .store(
            &tenant,
            Bytes::from_static(b"hello"),
            Some("text/plain".to_string()),
            1_776_960_000_000,
        )
        .await
        .expect("Convex storage put should succeed");

    assert!(metadata.id.as_str().starts_with("_storage:storage_"));
    assert_eq!(
        metadata.sha256,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    let document = metadata.to_virtual_document();
    assert_eq!(document["_id"], metadata.id.as_str());
    assert_eq!(document["contentType"], "text/plain");
    assert!(document.get("storage_key").is_none());
    assert!(document.get("convex.storage_id").is_none());

    let internal_keys = backend.convex_manifest_keys(&tenant);
    assert_eq!(internal_keys.len(), 1);
    assert!(internal_keys[0].starts_with("convex/objects/"));
    assert_ne!(internal_keys[0], metadata.id.as_str());
}

#[tokio::test]
async fn convex_hmac_download_serves_bytes_and_fails_closed_after_blob_loss() {
    let backend = Arc::new(InMemoryBackend::default());
    let storage = ConvexObjectStorage::new(backend.clone());
    let tenant = tenant("tenant-a");
    let metadata = storage
        .store(
            &tenant,
            Bytes::from_static(b"download me"),
            Some("text/plain".to_string()),
            1_000,
        )
        .await
        .expect("Convex storage put should succeed");
    let signer = DownloadTokenSigner::new(b"test-download-secret".to_vec()).unwrap();
    let token = signer
        .sign(&tenant, &metadata.id, 2_000)
        .expect("token signs");

    let downloaded = storage
        .download_with_token(&signer, &token, 1_500)
        .await
        .expect("valid token should serve bytes");
    assert_eq!(downloaded.bytes, Bytes::from_static(b"download me"));

    let expired = storage
        .download_with_token(&signer, &token, 2_001)
        .await
        .expect_err("expired token must fail");
    assert!(matches!(expired, ConvexStorageError::ExpiredToken));

    let manifest = backend
        .list_manifests(&tenant, CONVEX_STORAGE_BUCKET, "", 1)
        .await
        .unwrap()
        .pop()
        .expect("manifest remains");
    match manifest.blob_layout {
        ObjectBlobLayout::Whole { blob_hash } => {
            backend
                .release_blob(&tenant, &BlobHash::from_hex(&blob_hash).unwrap())
                .await
                .unwrap();
        }
        ObjectBlobLayout::Chunked { .. } => panic!("Convex storage stores whole objects"),
    }
    let forbidden = storage
        .download_with_token(&signer, &token, 1_500)
        .await
        .expect_err("missing bytes must fail closed");
    assert!(matches!(forbidden, ConvexStorageError::Forbidden(_)));
}

#[tokio::test]
async fn convex_export_import_zip_preserves_storage_ids_and_rotates_internal_keys() {
    let source_backend = Arc::new(InMemoryBackend::default());
    let source = ConvexObjectStorage::new(source_backend.clone());
    let tenant = tenant("tenant-a");
    let metadata = source
        .store(
            &tenant,
            Bytes::from_static(br#"{"ok":true}"#),
            Some("application/json".to_string()),
            1_776_960_000_000,
        )
        .await
        .expect("source object should store");
    let source_key = source_backend.convex_manifest_keys(&tenant)[0].clone();

    let archive = source
        .export_zip(&tenant)
        .await
        .expect("export should produce zip bytes");

    let target_backend = Arc::new(InMemoryBackend::default());
    let target = ConvexObjectStorage::new(target_backend.clone());
    let imported = target
        .import_zip(&tenant, archive.clone(), 1_776_960_001_000)
        .await
        .expect("import should succeed");
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].id, metadata.id);
    assert_eq!(imported[0].sha256, metadata.sha256);

    let read_back = target
        .read(&tenant, &metadata.id)
        .await
        .expect("read should succeed")
        .expect("imported object should exist");
    assert_eq!(read_back.bytes, Bytes::from_static(br#"{"ok":true}"#));
    let target_key = target_backend.convex_manifest_keys(&tenant)[0].clone();
    assert_ne!(target_key, source_key);

    let imported_again = target
        .import_zip(&tenant, archive, 1_776_960_002_000)
        .await
        .expect("re-import with the same storage id should replace cleanly");
    assert_eq!(imported_again[0].id, metadata.id);
    let target_keys = target_backend.convex_manifest_keys(&tenant);
    assert_eq!(target_keys.len(), 1);
    assert_ne!(target_keys[0], target_key);
    let read_again = target
        .read(&tenant, &metadata.id)
        .await
        .expect("read should succeed after re-import")
        .expect("re-imported object should exist");
    assert_eq!(read_again.bytes, Bytes::from_static(br#"{"ok":true}"#));
}

#[tokio::test]
async fn convex_import_zip_requires_content_type_extension_to_match_manifest() {
    let backend = Arc::new(InMemoryBackend::default());
    let storage = ConvexObjectStorage::new(backend);
    let tenant = tenant("tenant-a");
    let id = ConvexStorageId::generate().expect("storage id should generate");
    let bytes = Bytes::from_static(br#"{"ok":true}"#);
    let checksums = ComputedChecksums::for_bytes(&bytes);
    let document = serde_json::json!({
        "_id": id.as_str(),
        "_creationTime": 1_776_960_000_000_u64,
        "_updateTime": 1_776_960_000_000_u64,
        "contentType": "application/json",
        "sha256": checksums.sha256_hex,
        "size": bytes.len() as u64,
    });

    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer
        .start_file("_storage/documents.jsonl", options)
        .expect("manifest entry should start");
    writer
        .write_all(
            format!(
                "{}\n",
                serde_json::to_string(&document).expect("document should encode")
            )
            .as_bytes(),
        )
        .expect("manifest entry should write");
    writer
        .start_file(format!("_storage/{}.txt", id.raw_id().as_str()), options)
        .expect("mismatched object entry should start");
    writer
        .write_all(&bytes)
        .expect("mismatched object entry should write");
    let archive = Bytes::from(writer.finish().expect("archive should finish").into_inner());

    let error = storage
        .import_zip(&tenant, archive, 1_776_960_001_000)
        .await
        .expect_err("mismatched object extension must not import");
    assert!(matches!(
        error,
        ConvexStorageError::Archive(message)
            if message.contains("archive missing object bytes")
    ));
}

/// A read against a tenant that was never `ensure_tenant`'d must fail at
/// [`S3TenantResolver::resolve`] time with the same error the old per-call
/// facade produced on its first tenant-scoped storage call — it must not
/// succeed on a resolve that silently creates the tenant, and it must not
/// surface as an object-not-found error instead of a tenant-not-found error.
#[tokio::test]
async fn resolve_fails_closed_for_a_tenant_that_was_never_ensured() {
    let backend = Arc::new(InMemoryBackend::default());
    let never_created = tenant("tenant-a");

    let error = match backend.resolve(&never_created).await {
        Ok(_) => panic!("resolve must not implicitly create an unensured tenant"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        Error::TenantNotFound(missing) if missing == never_created
    ));

    // The same failure must reach callers through the S3 surface: a GET
    // against a tenant nobody has ever written through (so `ensure_tenant`
    // was never called) fails at `resolve` inside `get_object`, before any
    // manifest lookup runs.
    let service = service_with_backend(backend.clone());
    let get_error = service
        .get_object(req(
            GetObjectInput {
                bucket: BUCKET.to_string(),
                key: "anything.txt".to_string(),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect_err("a request against a never-created tenant must fail at resolve");
    assert_eq!(get_error.code(), &S3ErrorCode::InternalError);
}

/// HeadObject and ListObjectsV2 are metadata-only: they must be answerable
/// from `ctx.meta` alone. Resolving (and thereby creating) per-tenant
/// byte-plane state for these requests would be both a behavior regression
/// (pre-existing deployments never required blob-plane credentials for
/// these calls) and a tenant-safety hole (byte-plane state could be opened
/// for a tenant outside of any `enter_operation` guard). Assert the lazy
/// blob accessor is never invoked for either operation.
#[tokio::test]
async fn head_object_and_list_objects_v2_never_resolve_blob_plane_state() {
    let backend = Arc::new(InMemoryBackend::default());
    let owner = tenant("tenant-a");
    backend.ensure_tenant(&owner).await.expect("ensure tenant");
    // `ensure_tenant` itself touches `store` directly (not through the lazy
    // `S3TenantBlobs` accessor), so the resolution counter starts at zero.
    assert_eq!(backend.blob_resolutions(), 0);

    backend
        .put_manifest(
            &owner,
            ObjectManifest::whole(
                BUCKET.to_string(),
                "docs/readme.txt".to_string(),
                11,
                BlobHash::of(b"hello world").to_hex(),
                ObjectManifestAttributes::new("\"etag\"", 0),
            )
            .expect("manifest should build"),
        )
        .await
        .expect("manifest write should succeed");
    assert_eq!(
        backend.blob_resolutions(),
        0,
        "writing manifest metadata directly must not touch the blob accessor"
    );

    let service = service_with_backend(backend.clone());

    service
        .head_object(req(
            HeadObjectInput {
                bucket: BUCKET.to_string(),
                key: "docs/readme.txt".to_string(),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("head should succeed from metadata alone");
    assert_eq!(
        backend.blob_resolutions(),
        0,
        "HeadObject must never resolve blob-plane state"
    );

    service
        .list_objects_v2(req(
            ListObjectsV2Input {
                bucket: BUCKET.to_string(),
                prefix: Some("docs/".to_string()),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("list should succeed from metadata alone");
    assert_eq!(
        backend.blob_resolutions(),
        0,
        "ListObjectsV2 must never resolve blob-plane state"
    );
}

/// Every `UploadPart` that the service accepts must survive in the durable
/// upload record. Distinct part numbers do not conflict with each other, so a
/// concurrent batch must retain all of them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_upload_parts_preserve_all_accepted_parts() {
    const PARTS: u32 = 8;

    let backend = Arc::new(InMemoryBackend::default());
    let service = Arc::new(service_with_backend(backend.clone()));
    let created = service
        .create_multipart_upload(req(
            CreateMultipartUploadInput {
                bucket: BUCKET.to_string(),
                key: "large/concurrent.txt".to_string(),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("create multipart should succeed");
    let upload_id = created.output.upload_id.expect("upload id");

    let mut tasks = Vec::with_capacity(PARTS as usize);
    for part_number in 1..=PARTS {
        let service = Arc::clone(&service);
        let upload_id = upload_id.clone();
        tasks.push(tokio::spawn(async move {
            let bytes = format!("part-{part_number:04}").into_bytes();
            let len = bytes.len() as i64;
            service
                .upload_part(req(
                    UploadPartInput {
                        bucket: BUCKET.to_string(),
                        key: "large/concurrent.txt".to_string(),
                        upload_id,
                        part_number: part_number as i32,
                        body: Some(blob_owned(bytes)),
                        content_length: Some(len),
                        ..Default::default()
                    },
                    ACCESS_KEY_A,
                ))
                .await
                .map(|_| part_number)
        }));
    }

    let mut accepted = Vec::new();
    for task in tasks {
        if let Ok(part_number) = task.await.expect("upload part task should not panic") {
            accepted.push(part_number);
        }
    }
    accepted.sort_unstable();
    assert_eq!(
        accepted,
        (1..=PARTS).collect::<Vec<_>>(),
        "every distinct part number must be accepted"
    );

    assert_eq!(
        backend.durable_part_numbers(&tenant("tenant-a"), &upload_id),
        accepted,
        "every accepted part must survive in the durable upload record"
    );
}

/// A completion or an abort that fenced on an upload image another request
/// has already advanced must be rejected, and the rejection must leave the
/// upload row and every accepted part exactly as the winner left them.
#[tokio::test]
async fn stale_multipart_fence_is_rejected_without_losing_parts() {
    let backend = Arc::new(InMemoryBackend::default());
    let service = service_with_backend(backend.clone());
    let created = service
        .create_multipart_upload(req(
            CreateMultipartUploadInput {
                bucket: BUCKET.to_string(),
                key: "large/stale.txt".to_string(),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("create multipart should succeed");
    let upload_id = created.output.upload_id.expect("upload id");

    for part_number in 1..=2_i32 {
        service
            .upload_part(req(
                UploadPartInput {
                    bucket: BUCKET.to_string(),
                    key: "large/stale.txt".to_string(),
                    upload_id: upload_id.clone(),
                    part_number,
                    body: Some(blob_owned(format!("part-{part_number}").into_bytes())),
                    content_length: Some(6),
                    ..Default::default()
                },
                ACCESS_KEY_A,
            ))
            .await
            .expect("upload part should succeed");
    }

    let meta = backend.meta(&tenant("tenant-a"));
    let current = meta
        .get_multipart_upload(&upload_id)
        .await
        .expect("upload read should succeed")
        .expect("upload should exist");
    assert_eq!(current.parts.len(), 2);
    let stale = ObjectUploadExpectedState::AtRevision(current.revision - 1);

    // A stale abort or completion consumes nothing.
    match meta
        .delete_multipart_upload_conditional(&upload_id, vec![stale.clone()])
        .await
        .expect("a stale delete must decide, not fail")
    {
        ObjectUploadConditionOutcome::Rejected { unmet, current } => {
            assert_eq!(unmet, stale);
            assert_eq!(
                current
                    .expect("the row must survive a rejection")
                    .parts
                    .len(),
                2
            );
        }
        ObjectUploadConditionOutcome::Committed { .. } => {
            panic!("a delete fenced on a superseded revision must not commit")
        }
    }

    // A stale merge does not overwrite the parts the winner published.
    let mut stale_image = current.clone();
    stale_image.parts.truncate(1);
    stale_image.revision = current.revision;
    match meta
        .put_multipart_upload_conditional(stale_image, vec![stale.clone()])
        .await
        .expect("a stale put must decide, not fail")
    {
        ObjectUploadConditionOutcome::Rejected { unmet, .. } => assert_eq!(unmet, stale),
        ObjectUploadConditionOutcome::Committed { .. } => {
            panic!("a merge fenced on a superseded revision must not commit")
        }
    }
    assert_eq!(
        backend.durable_part_numbers(&tenant("tenant-a"), &upload_id),
        vec![1, 2],
        "a rejected fence must leave every accepted part in place"
    );

    // The current image still completes, so the rejection cost nothing.
    let completed = service
        .complete_multipart_upload(req(
            CompleteMultipartUploadInput {
                bucket: BUCKET.to_string(),
                key: "large/stale.txt".to_string(),
                upload_id: upload_id.clone(),
                multipart_upload: Some(CompletedMultipartUpload {
                    parts: Some(
                        current
                            .parts
                            .iter()
                            .map(|part| CompletedPart {
                                part_number: Some(part.part_number as i32),
                                e_tag: Some(ETag::Strong(part.etag.clone())),
                                ..Default::default()
                            })
                            .collect(),
                    ),
                }),
                ..Default::default()
            },
            ACCESS_KEY_A,
        ))
        .await
        .expect("completion on the current image should succeed");
    assert!(completed.output.e_tag.is_some());
    assert!(
        meta.get_multipart_upload(&upload_id)
            .await
            .expect("upload read should succeed")
            .is_none(),
        "completion must consume the upload row"
    );
}
