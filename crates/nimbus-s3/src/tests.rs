use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use nimbus_blob::{BlobHash, BlobStore, MemoryBlobStore};
use nimbus_core::{CommitEntry, Result, SequenceNumber, TenantId, Timestamp};
use nimbus_storage::{
    ObjectChunkRef, ObjectManifest, ObjectManifestAttributes, ObjectMultipartUpload,
};
use s3s::auth::{Credentials, SecretKey};
use s3s::dto::{
    ChecksumAlgorithm, CompletedMultipartUpload, CompletedPart, CreateMultipartUploadInput,
    GetObjectInput, ListObjectsV2Input, PutObjectInput, Range, StreamingBlob, UploadPartInput,
};
use s3s::{Body, S3, S3ErrorCode, S3Request};

use crate::{AccessKeyRegistry, NimbusS3, S3ObjectBackend};

const ACCESS_KEY_A: &str = "AKIATESTANTA";
const ACCESS_KEY_B: &str = "AKIATESTANTB";
const BUCKET: &str = "bucket";

#[derive(Default)]
struct InMemoryBackend {
    blobs: Mutex<HashMap<TenantId, Arc<MemoryBlobStore>>>,
    manifests: Mutex<BTreeMap<(TenantId, String, String), ObjectManifest>>,
    uploads: Mutex<BTreeMap<(TenantId, String), ObjectMultipartUpload>>,
}

#[async_trait]
impl S3ObjectBackend for InMemoryBackend {
    async fn ensure_tenant(&self, tenant: &TenantId) -> Result<()> {
        self.store(tenant);
        Ok(())
    }

    async fn put_blob(&self, tenant: &TenantId, bytes: Bytes) -> Result<BlobHash> {
        self.store(tenant).put(bytes).await
    }

    async fn get_blob(&self, tenant: &TenantId, hash: &BlobHash) -> Result<Bytes> {
        self.store(tenant).get(hash).await
    }

    async fn release_blob(&self, tenant: &TenantId, hash: &BlobHash) -> Result<()> {
        self.store(tenant).release(hash).await
    }

    async fn put_manifest(
        &self,
        tenant: &TenantId,
        manifest: ObjectManifest,
    ) -> Result<CommitEntry> {
        self.manifests.lock().unwrap().insert(
            (
                tenant.clone(),
                manifest.bucket.clone(),
                manifest.key.clone(),
            ),
            manifest,
        );
        Ok(commit())
    }

    async fn get_manifest(
        &self,
        tenant: &TenantId,
        bucket: &str,
        key: &str,
    ) -> Result<Option<ObjectManifest>> {
        Ok(self
            .manifests
            .lock()
            .unwrap()
            .get(&(tenant.clone(), bucket.to_string(), key.to_string()))
            .cloned())
    }

    async fn delete_manifest(
        &self,
        tenant: &TenantId,
        bucket: &str,
        key: &str,
    ) -> Result<Option<(CommitEntry, ObjectManifest)>> {
        Ok(self
            .manifests
            .lock()
            .unwrap()
            .remove(&(tenant.clone(), bucket.to_string(), key.to_string()))
            .map(|manifest| (commit(), manifest)))
    }

    async fn list_manifests(
        &self,
        tenant: &TenantId,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectManifest>> {
        let manifests_guard = self.manifests.lock().unwrap();
        let mut manifests = manifests_guard
            .iter()
            .filter_map(|((entry_tenant, entry_bucket, _), manifest)| {
                (entry_tenant == tenant
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

    async fn put_multipart_upload(
        &self,
        tenant: &TenantId,
        upload: ObjectMultipartUpload,
    ) -> Result<CommitEntry> {
        self.uploads
            .lock()
            .unwrap()
            .insert((tenant.clone(), upload.upload_id.clone()), upload);
        Ok(commit())
    }

    async fn get_multipart_upload(
        &self,
        tenant: &TenantId,
        upload_id: &str,
    ) -> Result<Option<ObjectMultipartUpload>> {
        Ok(self
            .uploads
            .lock()
            .unwrap()
            .get(&(tenant.clone(), upload_id.to_string()))
            .cloned())
    }

    async fn delete_multipart_upload(
        &self,
        tenant: &TenantId,
        upload_id: &str,
    ) -> Result<Option<(CommitEntry, ObjectMultipartUpload)>> {
        Ok(self
            .uploads
            .lock()
            .unwrap()
            .remove(&(tenant.clone(), upload_id.to_string()))
            .map(|upload| (commit(), upload)))
    }
}

impl InMemoryBackend {
    fn store(&self, tenant: &TenantId) -> Arc<MemoryBlobStore> {
        self.blobs
            .lock()
            .unwrap()
            .entry(tenant.clone())
            .or_insert_with(|| Arc::new(MemoryBlobStore::new()))
            .clone()
    }
}

fn commit() -> CommitEntry {
    CommitEntry {
        sequence: SequenceNumber(1),
        timestamp: Timestamp::now(),
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
async fn access_keys_isolate_tenants() {
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
