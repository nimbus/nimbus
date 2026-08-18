use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use futures::StreamExt;
use http::HeaderMap;
use nimbus_blob::BlobHash;
use nimbus_core::TenantId;
use nimbus_storage::{
    ObjectChunkRef, ObjectConditionOutcome, ObjectExpectedState, ObjectManifest,
    ObjectManifestAttributes, ObjectMultipartPart, ObjectMultipartUpload,
    ObjectUploadConditionOutcome, ObjectUploadExpectedState,
};
use s3s::dto::ChecksumAlgorithm;
use s3s::dto::{
    AbortMultipartUploadInput, AbortMultipartUploadOutput, CommonPrefix,
    CompleteMultipartUploadInput, CompleteMultipartUploadOutput, CreateMultipartUploadInput,
    CreateMultipartUploadOutput, DeleteObjectInput, DeleteObjectOutput, ETag, ETagCondition,
    GetObjectInput, GetObjectOutput, HeadObjectInput, HeadObjectOutput, ListObjectsV2Input,
    ListObjectsV2Output, Object, PutObjectInput, PutObjectOutput, StreamingBlob, Timestamp,
    UploadPartInput, UploadPartOutput,
};
use s3s::{Body, S3Error, S3ErrorCode, S3Request, S3Response, S3Result, TrailingHeaders, s3_error};
use serde_json::{Map, Value};

use crate::auth::AccessKeyRegistry;
use crate::backend::{S3TenantObjects, S3TenantResolver, put_manifest_unconditional};
use crate::checksum::{ComputedChecksums, decode_md5_base64, multipart_etag};
use crate::object_io;

const DEFAULT_MAX_KEYS: i32 = 1000;
const MAX_MAX_KEYS: i32 = 1000;
const CRC64NVME_HEADER: &str = "x-amz-checksum-crc64nvme";

/// Bounded re-merge budget for `UploadPart`. Each attempt reloads the
/// published upload image and re-applies this request's part, so the retry
/// covers only a pure merge that lost a conditional decision — never an
/// ambiguous durable outcome.
const UPLOAD_PART_MERGE_ATTEMPTS: u32 = 8;

#[derive(Clone)]
pub struct NimbusS3 {
    resolver: Arc<dyn S3TenantResolver>,
    access_keys: Arc<AccessKeyRegistry>,
}

impl NimbusS3 {
    #[must_use]
    pub fn new(resolver: Arc<dyn S3TenantResolver>, access_keys: AccessKeyRegistry) -> Self {
        Self {
            resolver,
            access_keys: Arc::new(access_keys),
        }
    }

    #[must_use]
    pub fn access_keys(&self) -> Arc<AccessKeyRegistry> {
        self.access_keys.clone()
    }

    fn tenant<T>(&self, req: &S3Request<T>) -> S3Result<TenantId> {
        let credentials = req
            .credentials
            .as_ref()
            .ok_or_else(|| s3_error!(AccessDenied, "S3 requests must be signed"))?;
        self.access_keys.tenant(&credentials.access_key)
    }

    async fn ensure_tenant(&self, tenant: &TenantId) -> S3Result<()> {
        self.resolver
            .ensure_tenant(tenant)
            .await
            .map_err(map_core_error)
    }

    async fn resolve(&self, tenant: &TenantId) -> S3Result<S3TenantObjects> {
        self.resolver.resolve(tenant).await.map_err(map_core_error)
    }
}

#[async_trait::async_trait]
impl s3s::S3 for NimbusS3 {
    async fn put_object(
        &self,
        req: S3Request<PutObjectInput>,
    ) -> S3Result<S3Response<PutObjectOutput>> {
        let tenant = self.tenant(&req)?;
        self.ensure_tenant(&tenant).await?;
        let ctx = self.resolve(&tenant).await?;
        let trailing_headers = req.trailing_headers.clone();
        let input = req.input;
        reject_unsupported_checksum_headers(
            input.checksum_algorithm.as_ref(),
            [
                ("CRC32", input.checksum_crc32.is_some()),
                ("CRC32C", input.checksum_crc32c.is_some()),
                ("SHA1", input.checksum_sha1.is_some()),
                ("SHA256", input.checksum_sha256.is_some()),
            ],
        )?;
        let bytes = collect_body(input.body).await?;
        let checksum_crc64nvme = checksum_crc64nvme(
            input.checksum_crc64nvme.as_deref(),
            trailing_headers.as_ref(),
        )?;
        verify_required_checksum_headers(
            input.checksum_algorithm.as_ref(),
            checksum_crc64nvme.as_deref(),
        )?;
        verify_content_length(input.content_length, bytes.len())?;
        let byte_len = bytes.len() as u64;
        let computed = ComputedChecksums::for_bytes(&bytes);
        computed.verify_content_md5(input.content_md5.as_deref())?;
        computed.verify_crc64nvme(checksum_crc64nvme.as_deref())?;

        // The preconditions travel to the commit authority as expected-state
        // clauses. This surface never reads the manifest and then decides
        // against that read: two concurrent requests would both observe the
        // pre-write state and both believe they won.
        let expected =
            write_condition_clauses(input.if_match.as_ref(), input.if_none_match.as_ref())?;
        let blobs = ctx.blobs().await.map_err(map_core_error)?;
        let hash = blobs.put(bytes).await.map_err(map_core_error)?;
        let manifest = match ObjectManifest::whole(
            input.bucket.clone(),
            input.key.clone(),
            byte_len,
            hash.to_hex(),
            manifest_attributes(
                input.content_type,
                input.metadata,
                computed.md5_hex.clone(),
                computed.object_checksums(),
            ),
        ) {
            Ok(manifest) => manifest,
            Err(error) => {
                self.release_uncommitted_blob(&ctx, &input.bucket, &input.key, &hash)
                    .await?;
                return Err(map_core_error(error));
            }
        };
        let size = manifest.size;
        let retained = manifest.clone();
        let previous = match ctx.meta.put_manifest_conditional(manifest, expected).await {
            Ok(ObjectConditionOutcome::Committed { previous, .. }) => previous,
            Ok(ObjectConditionOutcome::Rejected { unmet, current }) => {
                // Nothing was sequenced, journalled, or fanned out. The only
                // residue is the blob this request just wrote, and it stays
                // only if the manifest that won still names it.
                self.release_blob_unless_manifest_retains(&ctx, &hash, current.as_ref())
                    .await?;
                return Err(condition_rejected(&unmet));
            }
            Err(error) => {
                self.release_uncommitted_blob(&ctx, &input.bucket, &input.key, &hash)
                    .await?;
                return Err(map_core_error(error));
            }
        };
        if let Some(previous) = previous {
            self.release_manifest_blobs_except(&ctx, &previous, Some(&retained))
                .await?;
        }

        Ok(S3Response::new(PutObjectOutput {
            e_tag: Some(ETag::Strong(computed.md5_hex)),
            checksum_crc64nvme: Some(computed.crc64nvme_base64),
            size: Some(size_to_i64(size)?),
            ..Default::default()
        }))
    }

    async fn get_object(
        &self,
        req: S3Request<GetObjectInput>,
    ) -> S3Result<S3Response<GetObjectOutput>> {
        let tenant = self.tenant(&req)?;
        let ctx = self.resolve(&tenant).await?;
        let input = req.input;
        let manifest = ctx
            .meta
            .get_manifest(&input.bucket, &input.key)
            .await
            .map_err(map_core_error)?
            .ok_or_else(|| s3_error!(NoSuchKey))?;
        verify_read_preconditions(
            &manifest,
            input.if_match.as_ref(),
            input.if_none_match.as_ref(),
            input.if_modified_since.as_ref(),
            input.if_unmodified_since.as_ref(),
        )?;
        let mut bytes = self.read_manifest_bytes(&ctx, &manifest).await?;
        let mut status = None;
        let mut content_range = None;
        if let Some(range) = input.range {
            let selected = range.check(manifest.size)?;
            let start = selected.start;
            let end_exclusive = selected.end;
            bytes = bytes.slice(start as usize..end_exclusive as usize);
            content_range = Some(format!(
                "bytes {}-{}/{}",
                start,
                end_exclusive.saturating_sub(1),
                manifest.size
            ));
            status = Some(http::StatusCode::PARTIAL_CONTENT);
        }
        let output = GetObjectOutput {
            accept_ranges: Some("bytes".to_string()),
            body: Some(StreamingBlob::from(Body::from(bytes.clone()))),
            checksum_crc64nvme: manifest.checksums.crc64nvme.clone(),
            content_length: Some(size_to_i64(bytes.len() as u64)?),
            content_range,
            content_type: manifest.content_type.clone(),
            e_tag: Some(ETag::Strong(manifest.etag.clone())),
            last_modified: Some(timestamp_from_millis(manifest.last_modified_millis)),
            metadata: Some(metadata_to_s3(&manifest.user_metadata)),
            ..Default::default()
        };
        Ok(match status {
            Some(status) => S3Response::with_status(output, status),
            None => S3Response::new(output),
        })
    }

    async fn head_object(
        &self,
        req: S3Request<HeadObjectInput>,
    ) -> S3Result<S3Response<HeadObjectOutput>> {
        let tenant = self.tenant(&req)?;
        let ctx = self.resolve(&tenant).await?;
        let input = req.input;
        let manifest = ctx
            .meta
            .get_manifest(&input.bucket, &input.key)
            .await
            .map_err(map_core_error)?
            .ok_or_else(|| s3_error!(NoSuchKey))?;
        verify_read_preconditions(
            &manifest,
            input.if_match.as_ref(),
            input.if_none_match.as_ref(),
            input.if_modified_since.as_ref(),
            input.if_unmodified_since.as_ref(),
        )?;
        let mut content_length = manifest.size;
        let mut content_range = None;
        if let Some(range) = input.range {
            let selected = range.check(manifest.size)?;
            content_length = selected.end - selected.start;
            content_range = Some(format!(
                "bytes {}-{}/{}",
                selected.start,
                selected.end.saturating_sub(1),
                manifest.size
            ));
        }
        Ok(S3Response::new(HeadObjectOutput {
            accept_ranges: Some("bytes".to_string()),
            checksum_crc64nvme: manifest.checksums.crc64nvme.clone(),
            content_length: Some(size_to_i64(content_length)?),
            content_range,
            content_type: manifest.content_type.clone(),
            e_tag: Some(ETag::Strong(manifest.etag)),
            last_modified: Some(timestamp_from_millis(manifest.last_modified_millis)),
            metadata: Some(metadata_to_s3(&manifest.user_metadata)),
            ..Default::default()
        }))
    }

    async fn delete_object(
        &self,
        req: S3Request<DeleteObjectInput>,
    ) -> S3Result<S3Response<DeleteObjectOutput>> {
        let tenant = self.tenant(&req)?;
        let ctx = self.resolve(&tenant).await?;
        let input = req.input;
        if let Some((_, manifest)) = ctx
            .meta
            .delete_manifest(&input.bucket, &input.key)
            .await
            .map_err(map_core_error)?
        {
            self.release_manifest_blobs(&ctx, &manifest).await?;
        }
        Ok(S3Response::new(DeleteObjectOutput::default()))
    }

    async fn list_objects_v2(
        &self,
        req: S3Request<ListObjectsV2Input>,
    ) -> S3Result<S3Response<ListObjectsV2Output>> {
        let tenant = self.tenant(&req)?;
        let ctx = self.resolve(&tenant).await?;
        let input = req.input;
        let prefix = input.prefix.clone().unwrap_or_default();
        let max_keys = input
            .max_keys
            .unwrap_or(DEFAULT_MAX_KEYS)
            .clamp(0, MAX_MAX_KEYS);
        let start_after = input
            .continuation_token
            .clone()
            .or(input.start_after.clone());
        let delimiter = input.delimiter.clone();
        let manifests = ctx
            .meta
            .list_manifests(&input.bucket, &prefix, usize::MAX)
            .await
            .map_err(map_core_error)?;

        let mut contents = Vec::new();
        let mut common_prefixes = Vec::new();
        let mut seen_prefixes = BTreeSet::new();
        let mut emitted = 0;
        let mut next_token = None;

        for manifest in manifests {
            if start_after
                .as_deref()
                .is_some_and(|cursor| manifest.key.as_str() <= cursor)
            {
                continue;
            }
            if emitted >= max_keys {
                next_token = Some(manifest.key.clone());
                break;
            }
            if let Some(delimiter) = delimiter.as_deref()
                && let Some(common_prefix) = common_prefix(&prefix, &manifest.key, delimiter)
            {
                if seen_prefixes.insert(common_prefix.clone()) {
                    common_prefixes.push(CommonPrefix {
                        prefix: Some(common_prefix),
                    });
                    emitted += 1;
                }
                continue;
            }
            contents.push(object_summary(&manifest)?);
            emitted += 1;
        }

        Ok(S3Response::new(ListObjectsV2Output {
            name: Some(input.bucket),
            prefix: Some(prefix),
            delimiter,
            max_keys: Some(max_keys),
            key_count: Some(emitted),
            continuation_token: input.continuation_token,
            is_truncated: Some(next_token.is_some()),
            next_continuation_token: next_token,
            contents: (!contents.is_empty()).then_some(contents),
            common_prefixes: (!common_prefixes.is_empty()).then_some(common_prefixes),
            start_after: input.start_after,
            ..Default::default()
        }))
    }

    async fn create_multipart_upload(
        &self,
        req: S3Request<CreateMultipartUploadInput>,
    ) -> S3Result<S3Response<CreateMultipartUploadOutput>> {
        let tenant = self.tenant(&req)?;
        self.ensure_tenant(&tenant).await?;
        let ctx = self.resolve(&tenant).await?;
        let input = req.input;
        let upload_id = ulid::Ulid::new().to_string();
        let upload = ObjectMultipartUpload::new(
            upload_id.clone(),
            input.bucket.clone(),
            input.key.clone(),
            input.content_type.clone(),
            metadata_from_s3(input.metadata),
            current_millis(),
        )
        .map_err(map_core_error)?;
        // `Absent` makes the commit authority prove the freshly minted id is
        // unused instead of trusting the generator and overwriting a row.
        match ctx
            .meta
            .put_multipart_upload_conditional(upload, vec![ObjectUploadExpectedState::Absent])
            .await
            .map_err(map_core_error)?
        {
            ObjectUploadConditionOutcome::Committed { .. } => {}
            ObjectUploadConditionOutcome::Rejected { .. } => {
                return Err(S3Error::with_message(
                    S3ErrorCode::InternalError,
                    "a new multipart upload id collided with an existing upload",
                ));
            }
        }
        Ok(S3Response::new(CreateMultipartUploadOutput {
            bucket: Some(input.bucket),
            key: Some(input.key),
            upload_id: Some(upload_id),
            ..Default::default()
        }))
    }

    async fn upload_part(
        &self,
        req: S3Request<UploadPartInput>,
    ) -> S3Result<S3Response<UploadPartOutput>> {
        let tenant = self.tenant(&req)?;
        let ctx = self.resolve(&tenant).await?;
        let trailing_headers = req.trailing_headers.clone();
        let input = req.input;
        let upload = ctx
            .meta
            .get_multipart_upload(&input.upload_id)
            .await
            .map_err(map_core_error)?
            .ok_or_else(|| s3_error!(NoSuchUpload))?;
        if upload.bucket != input.bucket || upload.key != input.key {
            return Err(s3_error!(NoSuchUpload));
        }
        if !(1..=10_000).contains(&input.part_number) {
            return Err(S3Error::with_message(
                S3ErrorCode::InvalidArgument,
                "part number must be between 1 and 10000",
            ));
        }
        reject_unsupported_checksum_headers(
            input.checksum_algorithm.as_ref(),
            [
                ("CRC32", input.checksum_crc32.is_some()),
                ("CRC32C", input.checksum_crc32c.is_some()),
                ("SHA1", input.checksum_sha1.is_some()),
                ("SHA256", input.checksum_sha256.is_some()),
            ],
        )?;
        let bytes = collect_body(input.body).await?;
        let checksum_crc64nvme = checksum_crc64nvme(
            input.checksum_crc64nvme.as_deref(),
            trailing_headers.as_ref(),
        )?;
        verify_required_checksum_headers(
            input.checksum_algorithm.as_ref(),
            checksum_crc64nvme.as_deref(),
        )?;
        verify_content_length(input.content_length, bytes.len())?;
        let byte_len = bytes.len() as u64;
        let computed = ComputedChecksums::for_bytes(&bytes);
        computed.verify_content_md5(input.content_md5.as_deref())?;
        computed.verify_crc64nvme(checksum_crc64nvme.as_deref())?;
        let blobs = ctx.blobs().await.map_err(map_core_error)?;
        let hash = blobs.put(bytes).await.map_err(map_core_error)?;
        let hash_hex = hash.to_hex();
        // `UploadPart` merges one part into the whole upload record, so an
        // unconditional write does not overwrite one field — it discards every
        // part another request committed since this one read the row. The
        // observed revision travels to the commit authority, which decides it
        // against its own serialized read. The merge itself is pure, so a
        // rejection reloads the published image and re-applies this part to
        // it. An ambiguous durable outcome is never retried this way.
        let mut observed = upload;
        let mut attempt = 0_u32;
        let replaced_release_hash = loop {
            attempt += 1;
            let observed_retains_hash = object_io::multipart_upload_contains_blob(&observed, &hash)
                .map_err(map_core_error)?;
            let expected = vec![observed.observed_state()];
            let mut merged = observed.clone();
            let replaced = match merged.replace_part(ObjectMultipartPart {
                part_number: input.part_number as u32,
                blob_hash: hash_hex.clone(),
                size: byte_len,
                etag: computed.md5_hex.clone(),
                checksums: computed.object_checksums(),
                last_modified_millis: current_millis(),
            }) {
                Ok(replaced) => replaced,
                Err(error) => {
                    self.release_blob_unless_upload_retains(&ctx, &hash, observed_retains_hash)
                        .await?;
                    return Err(map_core_error(error));
                }
            };
            let replaced_release_hash = if let Some(replaced) = replaced
                && replaced.blob_hash != hash_hex
            {
                let replaced_hash =
                    object_io::parse_blob_hash(&replaced.blob_hash).map_err(map_core_error)?;
                (!object_io::multipart_upload_contains_blob(&merged, &replaced_hash)
                    .map_err(map_core_error)?)
                .then_some(replaced_hash)
            } else {
                None
            };
            merged.advance_revision();
            match ctx
                .meta
                .put_multipart_upload_conditional(merged, expected)
                .await
            {
                Ok(ObjectUploadConditionOutcome::Committed { .. }) => break replaced_release_hash,
                Ok(ObjectUploadConditionOutcome::Rejected { current, .. }) => {
                    let Some(current) = current else {
                        // Completion or abort consumed the upload while this
                        // part was in flight. Nothing merged, so the only
                        // residue is the blob this request wrote.
                        self.release_blob_unless_upload_retains(&ctx, &hash, false)
                            .await?;
                        return Err(s3_error!(NoSuchUpload));
                    };
                    if attempt >= UPLOAD_PART_MERGE_ATTEMPTS {
                        let current_retains_hash =
                            object_io::multipart_upload_contains_blob(&current, &hash)
                                .map_err(map_core_error)?;
                        self.release_blob_unless_upload_retains(&ctx, &hash, current_retains_hash)
                            .await?;
                        return Err(S3Error::with_message(
                            S3ErrorCode::OperationAborted,
                            "concurrent multipart writes did not converge; retry this part",
                        ));
                    }
                    observed = current;
                }
                Err(error) => {
                    // The outcome is ambiguous, not rejected: the write can
                    // still have become durable. Re-read the authority before
                    // deciding whether the blob has a holder.
                    let current = ctx
                        .meta
                        .get_multipart_upload(&input.upload_id)
                        .await
                        .map_err(map_core_error)?;
                    let current_retains_hash = match current.as_ref() {
                        Some(current) => object_io::multipart_upload_contains_blob(current, &hash)
                            .map_err(map_core_error)?,
                        None => false,
                    };
                    self.release_blob_unless_upload_retains(&ctx, &hash, current_retains_hash)
                        .await?;
                    return Err(map_core_error(error));
                }
            }
        };
        if let Some(replaced_hash) = replaced_release_hash {
            blobs
                .release(&replaced_hash)
                .await
                .map_err(map_core_error)?;
        }
        Ok(S3Response::new(UploadPartOutput {
            e_tag: Some(ETag::Strong(computed.md5_hex)),
            checksum_crc64nvme: Some(computed.crc64nvme_base64),
            ..Default::default()
        }))
    }

    async fn complete_multipart_upload(
        &self,
        req: S3Request<CompleteMultipartUploadInput>,
    ) -> S3Result<S3Response<CompleteMultipartUploadOutput>> {
        let tenant = self.tenant(&req)?;
        let ctx = self.resolve(&tenant).await?;
        let input = req.input;
        let upload = ctx
            .meta
            .get_multipart_upload(&input.upload_id)
            .await
            .map_err(map_core_error)?
            .ok_or_else(|| s3_error!(NoSuchUpload))?;
        if upload.bucket != input.bucket || upload.key != input.key {
            return Err(s3_error!(NoSuchUpload));
        }
        let requested_parts = input
            .multipart_upload
            .as_ref()
            .and_then(|upload| upload.parts.as_ref())
            .ok_or_else(|| s3_error!(InvalidPart, "multipart completion requires parts"))?;
        if requested_parts.is_empty() {
            return Err(s3_error!(
                InvalidPart,
                "multipart completion requires parts"
            ));
        }
        let mut chunks = Vec::with_capacity(requested_parts.len());
        let mut md5_parts = Vec::with_capacity(requested_parts.len());
        let mut offset = 0_u64;
        let mut previous_part_number = 0_i32;
        for requested in requested_parts {
            let part_number = requested.part_number.ok_or_else(|| {
                s3_error!(
                    InvalidPart,
                    "completed multipart part is missing part number"
                )
            })?;
            if part_number <= previous_part_number {
                return Err(s3_error!(InvalidPartOrder));
            }
            previous_part_number = part_number;
            let part = upload
                .parts
                .iter()
                .find(|part| part.part_number == part_number as u32)
                .ok_or_else(|| s3_error!(InvalidPart))?;
            if let Some(expected) = &requested.e_tag
                && expected.value() != part.etag
            {
                return Err(s3_error!(InvalidPart));
            }
            let md5 = part
                .checksums
                .content_md5
                .as_deref()
                .ok_or_else(|| s3_error!(InvalidPart, "uploaded part is missing MD5"))?;
            md5_parts.push(decode_md5_base64(md5)?);
            chunks.push(ObjectChunkRef {
                blob_hash: part.blob_hash.clone(),
                offset,
                len: part.size,
            });
            offset += part.size;
        }
        let etag = multipart_etag(&md5_parts);
        let mut attributes = ObjectManifestAttributes::new(etag.clone(), current_millis());
        attributes.content_type = upload.content_type.clone();
        attributes.user_metadata = upload.user_metadata.clone();
        attributes.checksums.crc64nvme = input.checksum_crc64nvme.clone();
        let manifest = ObjectManifest::chunked(
            input.bucket.clone(),
            input.key.clone(),
            offset,
            chunks,
            attributes,
        )
        .map_err(map_core_error)?;
        let retained = manifest.clone();
        // The upload row is consumed first, fenced on the revision this
        // request read. A completion that lost the race to another completion
        // or to an abort is rejected here, before anything is sequenced: it
        // publishes no manifest and releases no blob.
        let consumed = match ctx
            .meta
            .delete_multipart_upload_conditional(&input.upload_id, vec![upload.observed_state()])
            .await
            .map_err(map_core_error)?
        {
            ObjectUploadConditionOutcome::Committed { previous, .. } => previous,
            ObjectUploadConditionOutcome::Rejected { current, .. } => {
                return Err(stale_upload_error(current.is_some()));
            }
        };
        // Unconditional by contract: CompleteMultipartUpload takes no
        // preconditions. The superseded manifest comes back from the commit
        // authority's own read, so blob cleanup never acts on a stale image.
        let previous = put_manifest_unconditional(ctx.meta.as_ref(), manifest)
            .await
            .map_err(map_core_error)?;
        if let Some(previous) = previous {
            self.release_manifest_blobs_except(&ctx, &previous, Some(&retained))
                .await?;
        }
        if let Some(consumed) = consumed {
            self.release_upload_parts_except(&ctx, &consumed, Some(&retained))
                .await?;
        }
        Ok(S3Response::new(CompleteMultipartUploadOutput {
            bucket: Some(input.bucket.clone()),
            key: Some(input.key.clone()),
            e_tag: Some(ETag::Strong(etag)),
            checksum_crc64nvme: input.checksum_crc64nvme,
            location: Some(format!("/{}/{}", input.bucket, input.key)),
            ..Default::default()
        }))
    }

    async fn abort_multipart_upload(
        &self,
        req: S3Request<AbortMultipartUploadInput>,
    ) -> S3Result<S3Response<AbortMultipartUploadOutput>> {
        let tenant = self.tenant(&req)?;
        let ctx = self.resolve(&tenant).await?;
        let input = req.input;
        let Some(upload) = ctx
            .meta
            .get_multipart_upload(&input.upload_id)
            .await
            .map_err(map_core_error)?
        else {
            return Ok(S3Response::new(AbortMultipartUploadOutput::default()));
        };
        // Fenced on the revision this request read: an abort that raced a
        // completion or a concurrent part must not release bytes the winner
        // still owns.
        let aborted = match ctx
            .meta
            .delete_multipart_upload_conditional(&input.upload_id, vec![upload.observed_state()])
            .await
            .map_err(map_core_error)?
        {
            ObjectUploadConditionOutcome::Committed { previous, .. } => previous,
            ObjectUploadConditionOutcome::Rejected { current, .. } => {
                if current.is_none() {
                    // Another abort or completion already consumed the upload.
                    // Abort stays idempotent against an absent row.
                    return Ok(S3Response::new(AbortMultipartUploadOutput::default()));
                }
                return Err(stale_upload_error(true));
            }
        };
        if let Some(aborted) = aborted
            && !aborted.parts.is_empty()
        {
            let blobs = ctx.blobs().await.map_err(map_core_error)?;
            for part in aborted.parts {
                blobs
                    .release(&object_io::parse_blob_hash(&part.blob_hash).map_err(map_core_error)?)
                    .await
                    .map_err(map_core_error)?;
            }
        }
        Ok(S3Response::new(AbortMultipartUploadOutput::default()))
    }
}

impl NimbusS3 {
    async fn read_manifest_bytes(
        &self,
        ctx: &S3TenantObjects,
        manifest: &ObjectManifest,
    ) -> S3Result<Bytes> {
        let blobs = ctx.blobs().await.map_err(map_core_error)?;
        object_io::read_manifest_bytes(blobs.as_ref(), manifest)
            .await
            .map_err(map_core_error)
    }

    async fn release_manifest_blobs(
        &self,
        ctx: &S3TenantObjects,
        manifest: &ObjectManifest,
    ) -> S3Result<()> {
        self.release_manifest_blobs_except(ctx, manifest, None)
            .await
    }

    async fn release_manifest_blobs_except(
        &self,
        ctx: &S3TenantObjects,
        manifest: &ObjectManifest,
        retained: Option<&ObjectManifest>,
    ) -> S3Result<()> {
        let blobs = ctx.blobs().await.map_err(map_core_error)?;
        object_io::release_manifest_blobs_except(blobs.as_ref(), manifest, retained)
            .await
            .map_err(map_core_error)
    }

    async fn release_blob_unless_manifest_retains(
        &self,
        ctx: &S3TenantObjects,
        hash: &BlobHash,
        retained: Option<&ObjectManifest>,
    ) -> S3Result<()> {
        if let Some(retained) = retained
            && object_io::manifest_contains_blob(retained, hash).map_err(map_core_error)?
        {
            return Ok(());
        }
        ctx.blobs()
            .await
            .map_err(map_core_error)?
            .release(hash)
            .await
            .map_err(map_core_error)
    }

    /// Releases a blob written for a manifest that never committed.
    ///
    /// Re-reads the authoritative manifest rather than trusting a pre-read:
    /// a failed commit can still have become durable, and a concurrent writer
    /// can have stored the identical bytes under the same content hash. The
    /// blob goes only when the committed manifest does not name it.
    async fn release_uncommitted_blob(
        &self,
        ctx: &S3TenantObjects,
        bucket: &str,
        key: &str,
        hash: &BlobHash,
    ) -> S3Result<()> {
        let current = ctx
            .meta
            .get_manifest(bucket, key)
            .await
            .map_err(map_core_error)?;
        self.release_blob_unless_manifest_retains(ctx, hash, current.as_ref())
            .await
    }

    /// Releases the part blobs of a consumed upload that the manifest built
    /// from it does not retain. A part the client did not name in the
    /// completion is discarded by the S3 contract, and nothing else holds its
    /// bytes.
    async fn release_upload_parts_except(
        &self,
        ctx: &S3TenantObjects,
        upload: &ObjectMultipartUpload,
        retained: Option<&ObjectManifest>,
    ) -> S3Result<()> {
        for part in &upload.parts {
            let hash = object_io::parse_blob_hash(&part.blob_hash).map_err(map_core_error)?;
            if let Some(retained) = retained
                && object_io::manifest_contains_blob(retained, &hash).map_err(map_core_error)?
            {
                continue;
            }
            ctx.blobs()
                .await
                .map_err(map_core_error)?
                .release(&hash)
                .await
                .map_err(map_core_error)?;
        }
        Ok(())
    }

    async fn release_blob_unless_upload_retains(
        &self,
        ctx: &S3TenantObjects,
        hash: &BlobHash,
        retained_by_upload: bool,
    ) -> S3Result<()> {
        if retained_by_upload {
            return Ok(());
        }
        ctx.blobs()
            .await
            .map_err(map_core_error)?
            .release(hash)
            .await
            .map_err(map_core_error)
    }
}

async fn collect_body(body: Option<StreamingBlob>) -> S3Result<Bytes> {
    let Some(mut body) = body else {
        return Ok(Bytes::new());
    };
    let mut out = Vec::new();
    while let Some(chunk) = body.next().await {
        out.extend_from_slice(
            &chunk.map_err(|error| S3Error::with_source(S3ErrorCode::InternalError, error))?,
        );
    }
    Ok(Bytes::from(out))
}

fn verify_content_length(expected: Option<i64>, actual: usize) -> S3Result<()> {
    if let Some(expected) = expected
        && (expected < 0 || expected as usize != actual)
    {
        return Err(S3Error::with_message(
            S3ErrorCode::InvalidRequest,
            "Content-Length does not match the uploaded bytes",
        ));
    }
    Ok(())
}

fn checksum_crc64nvme(
    header_value: Option<&str>,
    trailing_headers: Option<&TrailingHeaders>,
) -> S3Result<Option<String>> {
    if let Some(value) = header_value {
        return Ok(Some(value.to_string()));
    }
    let Some(trailing_headers) = trailing_headers else {
        return Ok(None);
    };
    trailing_headers
        .read(trailing_crc64nvme_from_headers)
        .unwrap_or(Ok(None))
}

pub(crate) fn trailing_crc64nvme_from_headers(headers: &HeaderMap) -> S3Result<Option<String>> {
    let Some(value) = headers.get(CRC64NVME_HEADER) else {
        return Ok(None);
    };
    value
        .to_str()
        .map(|value| Some(value.to_string()))
        .map_err(|_| {
            S3Error::with_message(
                S3ErrorCode::InvalidRequest,
                "trailing x-amz-checksum-crc64nvme is not valid ASCII",
            )
        })
}

/// Translates the write preconditions on a request into expected-state
/// clauses for the commit authority.
///
/// This is wire policy only — `ETag` syntax and RFC 9110 strong against weak
/// comparison — and it reads no object state. Stored `ETag`s are always
/// strong ([`manifest_etag`]), which is what lets both comparisons reduce to
/// an opaque-value clause:
///
/// + `If-Match: *` requires the row to be present.
/// + `If-Match: "v"` requires the row to carry exactly `v`. Strong comparison
///   against a strong stored `ETag` is opaque-value equality.
/// + `If-Match: W/"v"` can never match a strong stored `ETag` under strong
///   comparison, so it fails here. The answer does not depend on any object
///   state, so deciding it on this side is not a read-then-write race.
/// + `If-None-Match: *` requires the row to be absent.
/// + `If-None-Match: "v"` or `W/"v"` requires the row to be absent or to
///   carry something other than `v`; weak comparison ignores the strength
///   marker and compares the opaque value.
fn write_condition_clauses(
    if_match: Option<&ETagCondition>,
    if_none_match: Option<&ETagCondition>,
) -> S3Result<Vec<ObjectExpectedState>> {
    let mut clauses = Vec::new();
    if let Some(condition) = if_match {
        if condition.is_any() {
            clauses.push(ObjectExpectedState::Present);
        } else {
            let expected = condition
                .as_etag()
                .and_then(ETag::as_strong)
                .ok_or_else(|| precondition_failed("If-Match did not match the current ETag"))?;
            clauses.push(ObjectExpectedState::PresentWithEtag(expected.to_string()));
        }
    }
    if let Some(condition) = if_none_match {
        if condition.is_any() {
            clauses.push(ObjectExpectedState::Absent);
        } else if let Some(etag) = condition.as_etag() {
            clauses.push(ObjectExpectedState::AbsentOrEtagDiffers(
                etag.value().to_string(),
            ));
        }
    }
    Ok(clauses)
}

/// Maps the clause the commit authority found unmet onto its S3 response.
fn condition_rejected(unmet: &ObjectExpectedState) -> S3Error {
    match unmet {
        ObjectExpectedState::Present | ObjectExpectedState::PresentWithEtag(_) => {
            precondition_failed("If-Match did not match the current ETag")
        }
        ObjectExpectedState::Absent | ObjectExpectedState::AbsentOrEtagDiffers(_) => {
            precondition_failed("If-None-Match matched the current ETag")
        }
    }
}

/// Maps a rejected multipart fence onto its S3 response. Nothing was
/// sequenced, journalled, or fanned out, so the client can re-read and retry.
fn stale_upload_error(exists: bool) -> S3Error {
    if exists {
        S3Error::with_message(
            S3ErrorCode::OperationAborted,
            "a conflicting multipart operation changed this upload; retry it",
        )
    } else {
        s3_error!(NoSuchUpload)
    }
}

fn verify_read_preconditions(
    manifest: &ObjectManifest,
    if_match: Option<&ETagCondition>,
    if_none_match: Option<&ETagCondition>,
    if_modified_since: Option<&Timestamp>,
    if_unmodified_since: Option<&Timestamp>,
) -> S3Result<()> {
    let current_etag = manifest_etag(manifest);
    let last_modified = timestamp_from_millis(manifest.last_modified_millis);
    if let Some(condition) = if_match {
        if !etag_condition_matches(condition, &current_etag, true) {
            return Err(precondition_failed(
                "If-Match did not match the current ETag",
            ));
        }
    } else if let Some(unmodified_since) = if_unmodified_since
        && &last_modified > unmodified_since
    {
        return Err(precondition_failed(
            "object has been modified since If-Unmodified-Since",
        ));
    }

    if let Some(condition) = if_none_match {
        if etag_condition_matches(condition, &current_etag, false) {
            return Err(S3Error::with_message(
                S3ErrorCode::NotModified,
                "If-None-Match matched the current ETag",
            ));
        }
    } else if let Some(modified_since) = if_modified_since
        && &last_modified <= modified_since
    {
        return Err(S3Error::with_message(
            S3ErrorCode::NotModified,
            "object has not been modified since If-Modified-Since",
        ));
    }
    Ok(())
}

fn etag_condition_matches(condition: &ETagCondition, current: &ETag, strong: bool) -> bool {
    if condition.is_any() {
        return true;
    }
    condition.as_etag().is_some_and(|expected| {
        if strong {
            expected.strong_cmp(current)
        } else {
            expected.weak_cmp(current)
        }
    })
}

fn manifest_etag(manifest: &ObjectManifest) -> ETag {
    ETag::Strong(manifest.etag.clone())
}

fn precondition_failed(message: &'static str) -> S3Error {
    S3Error::with_message(S3ErrorCode::PreconditionFailed, message)
}

fn reject_unsupported_checksum_headers<const N: usize>(
    algorithm: Option<&ChecksumAlgorithm>,
    unsupported_headers: [(&str, bool); N],
) -> S3Result<()> {
    for (name, present) in unsupported_headers {
        if present {
            return Err(S3Error::with_message(
                S3ErrorCode::InvalidRequest,
                format!("checksum algorithm {name} is not supported; use CRC64NVME or Content-MD5"),
            ));
        }
    }

    let Some(algorithm) = algorithm else {
        return Ok(());
    };
    if algorithm.as_str() != ChecksumAlgorithm::CRC64NVME {
        return Err(S3Error::with_message(
            S3ErrorCode::InvalidRequest,
            format!(
                "checksum algorithm {} is not supported; use CRC64NVME or Content-MD5",
                algorithm.as_str()
            ),
        ));
    }
    Ok(())
}

fn verify_required_checksum_headers(
    algorithm: Option<&ChecksumAlgorithm>,
    checksum_crc64nvme: Option<&str>,
) -> S3Result<()> {
    if algorithm.is_some_and(|algorithm| algorithm.as_str() == ChecksumAlgorithm::CRC64NVME)
        && checksum_crc64nvme.is_none()
    {
        return Err(S3Error::with_message(
            S3ErrorCode::InvalidRequest,
            "checksum algorithm CRC64NVME requires x-amz-checksum-crc64nvme",
        ));
    }
    Ok(())
}

fn manifest_attributes(
    content_type: Option<String>,
    metadata: Option<s3s::dto::Metadata>,
    etag: String,
    checksums: nimbus_storage::ObjectChecksums,
) -> ObjectManifestAttributes {
    let mut attributes = ObjectManifestAttributes::new(etag, current_millis());
    attributes.content_type = content_type;
    attributes.user_metadata = metadata_from_s3(metadata);
    attributes.checksums = checksums;
    attributes
}

fn metadata_from_s3(metadata: Option<s3s::dto::Metadata>) -> Map<String, Value> {
    metadata
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| (key, Value::String(value)))
        .collect()
}

fn metadata_to_s3(metadata: &Map<String, Value>) -> s3s::dto::Metadata {
    metadata
        .iter()
        .map(|(key, value)| {
            let value = value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string());
            (key.clone(), value)
        })
        .collect()
}

fn object_summary(manifest: &ObjectManifest) -> S3Result<Object> {
    Ok(Object {
        e_tag: Some(ETag::Strong(manifest.etag.clone())),
        key: Some(manifest.key.clone()),
        last_modified: Some(timestamp_from_millis(manifest.last_modified_millis)),
        size: Some(size_to_i64(manifest.size)?),
        ..Default::default()
    })
}

fn common_prefix(prefix: &str, key: &str, delimiter: &str) -> Option<String> {
    if delimiter.is_empty() || !key.starts_with(prefix) {
        return None;
    }
    let rest = &key[prefix.len()..];
    rest.find(delimiter)
        .map(|index| format!("{}{}", prefix, &rest[..index + delimiter.len()]))
}

fn timestamp_from_millis(millis: u64) -> s3s::dto::Timestamp {
    s3s::dto::Timestamp::from(SystemTime::UNIX_EPOCH + Duration::from_millis(millis))
}

fn current_millis() -> u64 {
    nimbus_core::clock::system_now_millis()
}

fn size_to_i64(size: u64) -> S3Result<i64> {
    i64::try_from(size).map_err(|_| {
        S3Error::with_message(
            S3ErrorCode::InvalidRequest,
            "object size exceeds S3 i64 range",
        )
    })
}

fn map_core_error(error: nimbus_core::Error) -> S3Error {
    match error {
        nimbus_core::Error::InvalidInput(message) => {
            S3Error::with_message(S3ErrorCode::InvalidRequest, message)
        }
        nimbus_core::Error::Storage {
            kind: nimbus_core::StorageErrorKind::Corruption,
            message,
        } => S3Error::with_message(S3ErrorCode::InternalError, message),
        other => S3Error::internal_error(other),
    }
}
