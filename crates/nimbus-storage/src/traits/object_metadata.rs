//! Object metadata-plane storage capability and DTOs.

use nimbus_core::{Document, DocumentId, Result, TableName};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use nimbus_core::CommitEntry;

use super::{TenantPointRead, TenantRangeScan};
// Point writes appear only on the test-only `*_direct` seeding helpers: the
// metadata plane's production writer is the engine committer, not this crate.
#[cfg(test)]
use super::TenantPointWrite;

/// Reserved table where object manifests are stored.
///
/// The byte plane (`nimbus-blob`) never depends on storage; this table is the
/// named metadata plane consumed by the S3 surface and object filesystem binder.
pub const OBJECT_MANIFEST_TABLE: &str = "_nimbus_objects";
pub const OBJECT_MULTIPART_TABLE: &str = "_nimbus_object_uploads";

const OBJECT_FIELD_BUCKET: &str = "bucket";
const OBJECT_FIELD_KEY: &str = "key";
const OBJECT_FIELD_SIZE: &str = "size";
const OBJECT_FIELD_CONTENT_TYPE: &str = "content_type";
const OBJECT_FIELD_USER_METADATA: &str = "user_metadata";
const OBJECT_FIELD_SYSTEM_METADATA: &str = "system_metadata";
const OBJECT_FIELD_ETAG: &str = "etag";
const OBJECT_FIELD_BLOB_LAYOUT: &str = "blob_layout";
const OBJECT_FIELD_CHECKSUMS: &str = "checksums";
const OBJECT_FIELD_LAST_MODIFIED_MILLIS: &str = "last_modified_millis";
const OBJECT_FIELD_UPLOAD_ID: &str = "upload_id";
const OBJECT_FIELD_INITIATED_AT_MILLIS: &str = "initiated_at_millis";
const OBJECT_FIELD_PARTS: &str = "parts";
const OBJECT_FIELD_REVISION: &str = "revision";

/// Revision a `CreateMultipartUpload` publishes. Revision 0 never exists, so
/// "absent" and "present at the first revision" stay distinguishable.
const FIRST_UPLOAD_REVISION: u64 = 1;

/// A blob reference inside an object manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectChunkRef {
    pub blob_hash: String,
    pub offset: u64,
    pub len: u64,
}

/// Object byte layout recorded in the metadata plane.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjectBlobLayout {
    Whole { blob_hash: String },
    Chunked { chunks: Vec<ObjectChunkRef> },
}

/// Checksums recorded in the protocol-neutral metadata plane.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectChecksums {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_md5: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crc64nvme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl ObjectChecksums {
    fn validate(&self, kind: &str) -> Result<()> {
        if self.content_md5.as_deref().is_some_and(str::is_empty) {
            return Err(nimbus_core::Error::InvalidInput(format!(
                "{kind} content_md5 checksum cannot be empty"
            )));
        }
        if self.crc64nvme.as_deref().is_some_and(str::is_empty) {
            return Err(nimbus_core::Error::InvalidInput(format!(
                "{kind} crc64nvme checksum cannot be empty"
            )));
        }
        if self.sha256.as_deref().is_some_and(str::is_empty) {
            return Err(nimbus_core::Error::InvalidInput(format!(
                "{kind} sha256 checksum cannot be empty"
            )));
        }
        Ok(())
    }
}

/// Protocol-neutral object metadata beside its blob layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectManifestAttributes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default)]
    pub user_metadata: Map<String, Value>,
    #[serde(default)]
    pub system_metadata: Map<String, Value>,
    pub etag: String,
    #[serde(default)]
    pub checksums: ObjectChecksums,
    pub last_modified_millis: u64,
}

impl ObjectManifestAttributes {
    pub fn new(etag: impl Into<String>, last_modified_millis: u64) -> Self {
        Self {
            content_type: None,
            user_metadata: Map::new(),
            system_metadata: Map::new(),
            etag: etag.into(),
            checksums: ObjectChecksums::default(),
            last_modified_millis,
        }
    }

    fn validate(&self, kind: &str) -> Result<()> {
        if self.etag.is_empty() {
            return Err(nimbus_core::Error::InvalidInput(format!(
                "{kind} etag cannot be empty"
            )));
        }
        self.checksums.validate(kind)
    }
}

/// Protocol-neutral object manifest.
///
/// `bucket` and `key` are the S3/developer-visible identity. They are not used
/// directly as a `DocumentId`, because object keys may contain `/`; storage uses
/// a stable derived document id and stores the original identity as data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectManifest {
    pub bucket: String,
    pub key: String,
    pub size: u64,
    pub blob_layout: ObjectBlobLayout,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default)]
    pub user_metadata: Map<String, Value>,
    #[serde(default)]
    pub system_metadata: Map<String, Value>,
    pub etag: String,
    #[serde(default)]
    pub checksums: ObjectChecksums,
    pub last_modified_millis: u64,
}

impl ObjectManifest {
    pub fn whole(
        bucket: impl Into<String>,
        key: impl Into<String>,
        size: u64,
        blob_hash: impl Into<String>,
        attributes: ObjectManifestAttributes,
    ) -> Result<Self> {
        Self::with_layout(
            bucket,
            key,
            size,
            ObjectBlobLayout::Whole {
                blob_hash: blob_hash.into(),
            },
            attributes,
        )
    }

    pub fn chunked(
        bucket: impl Into<String>,
        key: impl Into<String>,
        size: u64,
        chunks: Vec<ObjectChunkRef>,
        attributes: ObjectManifestAttributes,
    ) -> Result<Self> {
        Self::with_layout(
            bucket,
            key,
            size,
            ObjectBlobLayout::Chunked { chunks },
            attributes,
        )
    }

    fn with_layout(
        bucket: impl Into<String>,
        key: impl Into<String>,
        size: u64,
        blob_layout: ObjectBlobLayout,
        attributes: ObjectManifestAttributes,
    ) -> Result<Self> {
        attributes.validate("object manifest")?;
        let manifest = Self {
            bucket: bucket.into(),
            key: key.into(),
            size,
            blob_layout,
            content_type: attributes.content_type,
            user_metadata: attributes.user_metadata,
            system_metadata: attributes.system_metadata,
            etag: attributes.etag,
            checksums: attributes.checksums,
            last_modified_millis: attributes.last_modified_millis,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        validate_object_bucket(&self.bucket)?;
        validate_object_key(&self.key)?;
        if self.etag.is_empty() {
            return Err(nimbus_core::Error::InvalidInput(
                "object manifest etag cannot be empty".to_string(),
            ));
        }
        self.checksums.validate("object manifest")?;
        match &self.blob_layout {
            ObjectBlobLayout::Whole { blob_hash } if blob_hash.is_empty() => {
                Err(nimbus_core::Error::InvalidInput(
                    "object manifest whole blob hash cannot be empty".to_string(),
                ))
            }
            ObjectBlobLayout::Chunked { chunks } if chunks.is_empty() => {
                Err(nimbus_core::Error::InvalidInput(
                    "object manifest chunked layout cannot be empty".to_string(),
                ))
            }
            ObjectBlobLayout::Chunked { chunks } => {
                let mut expected_offset = 0_u64;
                for chunk in chunks {
                    if chunk.blob_hash.is_empty() {
                        return Err(nimbus_core::Error::InvalidInput(
                            "object manifest chunk blob hash cannot be empty".to_string(),
                        ));
                    }
                    if chunk.len == 0 {
                        return Err(nimbus_core::Error::InvalidInput(
                            "object manifest chunk length cannot be zero".to_string(),
                        ));
                    }
                    if chunk.offset != expected_offset {
                        return Err(nimbus_core::Error::InvalidInput(format!(
                            "object manifest chunk offset {} does not match expected offset {expected_offset}",
                            chunk.offset
                        )));
                    }
                    expected_offset = expected_offset.checked_add(chunk.len).ok_or_else(|| {
                        nimbus_core::Error::InvalidInput(
                            "object manifest chunk offsets overflow u64".to_string(),
                        )
                    })?;
                }
                if expected_offset != self.size {
                    return Err(nimbus_core::Error::InvalidInput(format!(
                        "object manifest chunked size {expected_offset} does not match object size {}",
                        self.size
                    )));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn document_id(&self) -> Result<DocumentId> {
        object_document_id(&self.bucket, &self.key)
    }

    pub fn to_document(&self) -> Result<Document> {
        self.validate()?;
        let mut fields = Map::new();
        fields.insert(
            OBJECT_FIELD_BUCKET.to_string(),
            Value::String(self.bucket.clone()),
        );
        fields.insert(
            OBJECT_FIELD_KEY.to_string(),
            Value::String(self.key.clone()),
        );
        fields.insert(OBJECT_FIELD_SIZE.to_string(), json!(self.size));
        fields.insert(
            OBJECT_FIELD_CONTENT_TYPE.to_string(),
            self.content_type
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        fields.insert(
            OBJECT_FIELD_USER_METADATA.to_string(),
            Value::Object(self.user_metadata.clone()),
        );
        fields.insert(
            OBJECT_FIELD_SYSTEM_METADATA.to_string(),
            Value::Object(self.system_metadata.clone()),
        );
        fields.insert(
            OBJECT_FIELD_ETAG.to_string(),
            Value::String(self.etag.clone()),
        );
        fields.insert(
            OBJECT_FIELD_CHECKSUMS.to_string(),
            serde_json::to_value(&self.checksums).map_err(|err| {
                nimbus_core::Error::Serialization(format!("encode object checksums: {err}"))
            })?,
        );
        fields.insert(
            OBJECT_FIELD_LAST_MODIFIED_MILLIS.to_string(),
            json!(self.last_modified_millis),
        );
        fields.insert(
            OBJECT_FIELD_BLOB_LAYOUT.to_string(),
            serde_json::to_value(&self.blob_layout).map_err(|err| {
                nimbus_core::Error::Serialization(format!("encode object blob layout: {err}"))
            })?,
        );
        Ok(Document::with_id(
            self.document_id()?,
            object_manifest_table()?,
            fields,
        ))
    }

    pub fn from_document(document: &Document) -> Result<Self> {
        let bucket = required_string(document, OBJECT_FIELD_BUCKET)?;
        let key = required_string(document, OBJECT_FIELD_KEY)?;
        let size = required_u64(document, OBJECT_FIELD_SIZE)?;
        let content_type = optional_string(document, OBJECT_FIELD_CONTENT_TYPE)?;
        let user_metadata = match document.fields.get(OBJECT_FIELD_USER_METADATA) {
            Some(Value::Object(map)) => map.clone(),
            Some(_) => {
                return Err(nimbus_core::Error::Serialization(
                    "object manifest user_metadata must be an object".to_string(),
                ));
            }
            None => Map::new(),
        };
        let system_metadata = match document.fields.get(OBJECT_FIELD_SYSTEM_METADATA) {
            Some(Value::Object(map)) => map.clone(),
            Some(_) => {
                return Err(nimbus_core::Error::Serialization(
                    "object manifest system_metadata must be an object".to_string(),
                ));
            }
            None => Map::new(),
        };
        let etag = required_string(document, OBJECT_FIELD_ETAG)?;
        let checksums =
            optional_json::<ObjectChecksums>(document, OBJECT_FIELD_CHECKSUMS)?.unwrap_or_default();
        let last_modified_millis = required_u64(document, OBJECT_FIELD_LAST_MODIFIED_MILLIS)?;
        let layout_value = document
            .fields
            .get(OBJECT_FIELD_BLOB_LAYOUT)
            .ok_or_else(|| {
                nimbus_core::Error::Serialization("object manifest missing blob_layout".to_string())
            })?
            .clone();
        let blob_layout: ObjectBlobLayout =
            serde_json::from_value(layout_value).map_err(|err| {
                nimbus_core::Error::Serialization(format!("decode object blob layout: {err}"))
            })?;
        let manifest = Self {
            bucket,
            key,
            size,
            blob_layout,
            content_type,
            user_metadata,
            system_metadata,
            etag,
            checksums,
            last_modified_millis,
        };
        manifest.validate()?;
        Ok(manifest)
    }
}

/// One uploaded multipart part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectMultipartPart {
    pub part_number: u32,
    pub blob_hash: String,
    pub size: u64,
    pub etag: String,
    #[serde(default)]
    pub checksums: ObjectChecksums,
    pub last_modified_millis: u64,
}

impl ObjectMultipartPart {
    pub fn validate(&self) -> Result<()> {
        if !(1..=10_000).contains(&self.part_number) {
            return Err(nimbus_core::Error::InvalidInput(
                "multipart part number must be between 1 and 10000".to_string(),
            ));
        }
        if self.blob_hash.is_empty() {
            return Err(nimbus_core::Error::InvalidInput(
                "multipart part blob hash cannot be empty".to_string(),
            ));
        }
        if self.etag.is_empty() {
            return Err(nimbus_core::Error::InvalidInput(
                "multipart part etag cannot be empty".to_string(),
            ));
        }
        self.checksums.validate("multipart part")
    }
}

/// Durable state for an in-progress multipart upload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectMultipartUpload {
    pub upload_id: String,
    pub bucket: String,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default)]
    pub user_metadata: Map<String, Value>,
    pub initiated_at_millis: u64,
    #[serde(default)]
    pub parts: Vec<ObjectMultipartPart>,
    /// Monotonic version of this upload row, starting at 1 for the row a
    /// `CreateMultipartUpload` publishes. Every later write must name the
    /// revision it observed, so the commit authority can refuse a write that
    /// merged onto a stale image. See [`ObjectUploadExpectedState`].
    pub revision: u64,
}

impl ObjectMultipartUpload {
    pub fn new(
        upload_id: impl Into<String>,
        bucket: impl Into<String>,
        key: impl Into<String>,
        content_type: Option<String>,
        user_metadata: Map<String, Value>,
        initiated_at_millis: u64,
    ) -> Result<Self> {
        let upload = Self {
            upload_id: upload_id.into(),
            bucket: bucket.into(),
            key: key.into(),
            content_type,
            user_metadata,
            initiated_at_millis,
            parts: Vec::new(),
            revision: FIRST_UPLOAD_REVISION,
        };
        upload.validate()?;
        Ok(upload)
    }

    pub fn validate(&self) -> Result<()> {
        validate_upload_id(&self.upload_id)?;
        validate_object_bucket(&self.bucket)?;
        validate_object_key(&self.key)?;
        if self.revision < FIRST_UPLOAD_REVISION {
            return Err(nimbus_core::Error::InvalidInput(
                "multipart upload revision must start at 1".to_string(),
            ));
        }
        let mut previous = None;
        for part in &self.parts {
            part.validate()?;
            if previous.is_some_and(|number| number >= part.part_number) {
                return Err(nimbus_core::Error::InvalidInput(
                    "multipart parts must be strictly ordered by part number".to_string(),
                ));
            }
            previous = Some(part.part_number);
        }
        Ok(())
    }

    pub fn replace_part(
        &mut self,
        part: ObjectMultipartPart,
    ) -> Result<Option<ObjectMultipartPart>> {
        part.validate()?;
        match self
            .parts
            .binary_search_by_key(&part.part_number, |entry| entry.part_number)
        {
            Ok(index) => Ok(Some(std::mem::replace(&mut self.parts[index], part))),
            Err(index) => {
                self.parts.insert(index, part);
                Ok(None)
            }
        }
    }

    /// The clause a write derived from this image must carry, so the
    /// authority can confirm nothing changed between the read and the write.
    #[must_use]
    pub fn observed_state(&self) -> ObjectUploadExpectedState {
        ObjectUploadExpectedState::AtRevision(self.revision)
    }

    /// Advances this image to the revision that succeeds the one it was read
    /// at. Call it once, on the merged image, immediately before the write.
    pub fn advance_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    pub fn document_id(&self) -> Result<DocumentId> {
        upload_document_id(&self.upload_id)
    }

    pub fn to_document(&self) -> Result<Document> {
        self.validate()?;
        let mut fields = Map::new();
        fields.insert(
            OBJECT_FIELD_UPLOAD_ID.to_string(),
            Value::String(self.upload_id.clone()),
        );
        fields.insert(
            OBJECT_FIELD_BUCKET.to_string(),
            Value::String(self.bucket.clone()),
        );
        fields.insert(
            OBJECT_FIELD_KEY.to_string(),
            Value::String(self.key.clone()),
        );
        fields.insert(
            OBJECT_FIELD_CONTENT_TYPE.to_string(),
            self.content_type
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        fields.insert(
            OBJECT_FIELD_USER_METADATA.to_string(),
            Value::Object(self.user_metadata.clone()),
        );
        fields.insert(
            OBJECT_FIELD_INITIATED_AT_MILLIS.to_string(),
            json!(self.initiated_at_millis),
        );
        fields.insert(
            OBJECT_FIELD_PARTS.to_string(),
            serde_json::to_value(&self.parts).map_err(|err| {
                nimbus_core::Error::Serialization(format!("encode multipart parts: {err}"))
            })?,
        );
        fields.insert(OBJECT_FIELD_REVISION.to_string(), json!(self.revision));
        Ok(Document::with_id(
            self.document_id()?,
            object_multipart_table()?,
            fields,
        ))
    }

    pub fn from_document(document: &Document) -> Result<Self> {
        let upload_id = required_string(document, OBJECT_FIELD_UPLOAD_ID)?;
        let bucket = required_string(document, OBJECT_FIELD_BUCKET)?;
        let key = required_string(document, OBJECT_FIELD_KEY)?;
        let content_type = optional_string(document, OBJECT_FIELD_CONTENT_TYPE)?;
        let user_metadata = match document.fields.get(OBJECT_FIELD_USER_METADATA) {
            Some(Value::Object(map)) => map.clone(),
            Some(_) => {
                return Err(nimbus_core::Error::Serialization(
                    "multipart upload user_metadata must be an object".to_string(),
                ));
            }
            None => Map::new(),
        };
        let initiated_at_millis = required_u64(document, OBJECT_FIELD_INITIATED_AT_MILLIS)?;
        let parts = optional_json::<Vec<ObjectMultipartPart>>(document, OBJECT_FIELD_PARTS)?
            .unwrap_or_default();
        let revision = required_u64(document, OBJECT_FIELD_REVISION)?;
        let upload = Self {
            upload_id,
            bucket,
            key,
            content_type,
            user_metadata,
            initiated_at_millis,
            parts,
            revision,
        };
        upload.validate()?;
        Ok(upload)
    }
}

/// Metadata-plane read capability for named object manifests and multipart
/// uploads.
///
/// Reads only, by design. Object metadata is published exclusively by the
/// engine's committer-sequenced object commit path (`nimbus-engine`'s
/// `TenantObjectMeta`), which writes the manifest/upload rows as ordinary
/// documents through the tenant write log: the journal sequence is assigned
/// inside the committer actor under the committer lease, persistence goes
/// through the fenced provider batch, and the commit advances the engine's
/// durable/applied watermarks and fans out to subscriptions.
///
/// A store-level write entry point would bypass all of that — assigning a
/// commit sequence outside the committer, leaving watermarks stale, skipping
/// the provider fence, and letting two writers on the same key interleave — so
/// this trait deliberately has no write half and no publicly reachable
/// substitute exists in this crate (SUC2.2, closed by FU6a).
pub trait ObjectMetaRead {
    fn get_object_manifest(&self, bucket: &str, key: &str) -> Result<Option<ObjectManifest>>;
    fn list_object_manifests(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectManifest>>;
    fn get_multipart_upload(&self, upload_id: &str) -> Result<Option<ObjectMultipartUpload>>;
    fn list_multipart_uploads(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectMultipartUpload>>;
}

/// One state clause the authoritative committer evaluates against its own
/// read of the current object row, before it assigns a sequence number.
///
/// A clause carries opaque `ETag` values only. Wire policy — header parsing,
/// strong against weak comparison, and response mapping — belongs to the
/// protocol surface that builds the clause, not to the commit authority that
/// decides it.
///
/// There is deliberately no `Default`. A writer that omits a condition must
/// say so by passing no clauses, so "unconditional" is always written down
/// rather than inherited from a silent default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectExpectedState {
    /// The row must be absent.
    Absent,
    /// The row must be present, whatever `ETag` it carries.
    Present,
    /// The row must be present and carry exactly this opaque `ETag`.
    PresentWithEtag(String),
    /// The row must be absent, or present with a different opaque `ETag`.
    AbsentOrEtagDiffers(String),
}

impl ObjectExpectedState {
    /// Evaluates this clause against the current row's opaque `ETag`, or
    /// `None` when the row is absent.
    #[must_use]
    pub fn holds_for(&self, current_etag: Option<&str>) -> bool {
        match (self, current_etag) {
            (Self::Absent, None) | (Self::Present, Some(_)) => true,
            (Self::Absent, Some(_)) | (Self::Present | Self::PresentWithEtag(_), None) => false,
            (Self::PresentWithEtag(expected), Some(current)) => expected == current,
            (Self::AbsentOrEtagDiffers(_), None) => true,
            (Self::AbsentOrEtagDiffers(rejected), Some(current)) => rejected != current,
        }
    }

    /// Evaluates every clause in order and returns the first one that does
    /// not hold. An empty clause list is an unconditional write.
    #[must_use]
    pub fn first_unmet<'a>(clauses: &'a [Self], current_etag: Option<&str>) -> Option<&'a Self> {
        clauses
            .iter()
            .find(|clause| !clause.holds_for(current_etag))
    }
}

/// What the commit authority decided for a conditional object-metadata write.
///
/// A rejected condition consumes no sequence number, appends no journal
/// record, publishes no fan-out, and retains nothing in the byte plane: the
/// authority returns [`ObjectConditionOutcome::Rejected`] before it reaches
/// sequence assignment.
#[derive(Clone, Debug)]
pub enum ObjectConditionOutcome {
    /// Every clause held, and the write committed.
    Committed {
        /// The commit the write produced.
        commit: CommitEntry,
        /// The manifest this write replaced, decoded from the authority's own
        /// read. Callers use this — never a pre-read taken outside the
        /// authority — to decide which blobs the superseded manifest released.
        previous: Option<ObjectManifest>,
    },
    /// A clause did not hold. Nothing was written.
    Rejected {
        /// The first clause that did not hold.
        unmet: ObjectExpectedState,
        /// The manifest the authority observed when it decided. Callers use
        /// this to decide whether a just-written blob is still retained by
        /// the manifest that won.
        current: Option<ObjectManifest>,
    },
}

/// One state clause the authoritative committer evaluates against its own
/// read of the current multipart-upload row, before it assigns a sequence
/// number.
///
/// `UploadPart` carries no conditional headers on the wire, so no client
/// policy can protect a multipart merge. The clause is therefore internal: a
/// writer states the revision it merged onto, and the authority refuses the
/// write when the row has moved on. The writer then reloads and re-merges,
/// which is a pure operation on the reloaded image.
///
/// There is deliberately no `Default`, for the reason
/// [`ObjectExpectedState`] documents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectUploadExpectedState {
    /// The upload row must be absent.
    Absent,
    /// The upload row must be present at exactly this revision.
    AtRevision(u64),
}

impl ObjectUploadExpectedState {
    /// Evaluates this clause against the current row's revision, or `None`
    /// when the row is absent.
    #[must_use]
    pub fn holds_for(&self, current_revision: Option<u64>) -> bool {
        match (self, current_revision) {
            (Self::Absent, None) => true,
            (Self::Absent, Some(_)) | (Self::AtRevision(_), None) => false,
            (Self::AtRevision(expected), Some(current)) => *expected == current,
        }
    }

    /// The revision a write guarded by this clause must publish. An absent
    /// row becomes the first revision; a present row becomes its successor.
    #[must_use]
    pub fn successor_revision(&self) -> u64 {
        match self {
            Self::Absent => FIRST_UPLOAD_REVISION,
            Self::AtRevision(current) => current.saturating_add(1),
        }
    }

    /// Evaluates every clause in order and returns the first one that does
    /// not hold. An empty clause list is an unconditional write.
    #[must_use]
    pub fn first_unmet(clauses: &[Self], current_revision: Option<u64>) -> Option<&Self> {
        clauses
            .iter()
            .find(|clause| !clause.holds_for(current_revision))
    }
}

/// What the commit authority decided for a conditional multipart-upload
/// write. A rejected clause has the same no-effect guarantee that
/// [`ObjectConditionOutcome`] documents.
#[derive(Clone, Debug)]
pub enum ObjectUploadConditionOutcome {
    /// Every clause held, and the write committed.
    Committed {
        /// The commit the write produced.
        commit: CommitEntry,
        /// The upload row this write replaced or removed, decoded from the
        /// authority's own read. `None` when the write created the row.
        previous: Option<ObjectMultipartUpload>,
    },
    /// A clause did not hold. Nothing was written.
    Rejected {
        /// The first clause that did not hold.
        unmet: ObjectUploadExpectedState,
        /// The upload row the authority observed when it decided. The caller
        /// re-merges onto this image rather than onto its own stale read.
        current: Option<ObjectMultipartUpload>,
    },
}

fn object_identity_matches_prefix(document: &Document, bucket: &str, prefix: &str) -> bool {
    let bucket_matches = match document.fields.get(OBJECT_FIELD_BUCKET) {
        Some(Value::String(document_bucket)) => document_bucket == bucket,
        _ => false,
    };
    let key_matches = match document.fields.get(OBJECT_FIELD_KEY) {
        Some(Value::String(key)) => key.starts_with(prefix),
        _ => false,
    };
    bucket_matches && key_matches
}

fn validate_object_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(nimbus_core::Error::InvalidInput(
            "object key cannot be empty".to_string(),
        ));
    }
    if key.len() > 1_024 {
        return Err(nimbus_core::Error::InvalidInput(
            "object key cannot exceed 1024 bytes".to_string(),
        ));
    }
    if key.bytes().any(|byte| byte == 0) {
        return Err(nimbus_core::Error::InvalidInput(
            "object key cannot contain NUL bytes".to_string(),
        ));
    }
    Ok(())
}

/// Document id of the manifest row for `bucket`/`key`, for callers that
/// address a manifest by name without holding a decoded [`ObjectManifest`].
pub fn object_manifest_document_id(bucket: &str, key: &str) -> Result<DocumentId> {
    object_document_id(bucket, key)
}

/// Document id of the multipart-upload row for `upload_id`, for callers that
/// address an upload by id without holding a decoded [`ObjectMultipartUpload`].
pub fn multipart_upload_document_id(upload_id: &str) -> Result<DocumentId> {
    upload_document_id(upload_id)
}

fn object_manifest_table() -> Result<TableName> {
    TableName::new(OBJECT_MANIFEST_TABLE)
}

fn object_multipart_table() -> Result<TableName> {
    TableName::new(OBJECT_MULTIPART_TABLE)
}

fn object_document_id(bucket: &str, key: &str) -> Result<DocumentId> {
    validate_object_bucket(bucket)?;
    validate_object_key(key)?;
    let mut hasher = Sha256::new();
    hasher.update(bucket.as_bytes());
    hasher.update([0]);
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    DocumentId::from_key(format!("object_{}", hex::encode(digest)))
}

fn upload_document_id(upload_id: &str) -> Result<DocumentId> {
    validate_upload_id(upload_id)?;
    let digest = Sha256::digest(upload_id.as_bytes());
    DocumentId::from_key(format!("upload_{}", hex::encode(digest)))
}

fn validate_object_bucket(bucket: &str) -> Result<()> {
    if bucket.is_empty() {
        return Err(nimbus_core::Error::InvalidInput(
            "object bucket cannot be empty".to_string(),
        ));
    }
    if bucket.len() > 63 {
        return Err(nimbus_core::Error::InvalidInput(
            "object bucket cannot exceed 63 bytes".to_string(),
        ));
    }
    if bucket.bytes().any(|byte| byte == 0 || byte == b'/') {
        return Err(nimbus_core::Error::InvalidInput(
            "object bucket cannot contain NUL bytes or slashes".to_string(),
        ));
    }
    Ok(())
}

fn validate_upload_id(upload_id: &str) -> Result<()> {
    if upload_id.is_empty() {
        return Err(nimbus_core::Error::InvalidInput(
            "multipart upload id cannot be empty".to_string(),
        ));
    }
    if upload_id.len() > 256 {
        return Err(nimbus_core::Error::InvalidInput(
            "multipart upload id cannot exceed 256 bytes".to_string(),
        ));
    }
    if upload_id.bytes().any(|byte| byte == 0) {
        return Err(nimbus_core::Error::InvalidInput(
            "multipart upload id cannot contain NUL bytes".to_string(),
        ));
    }
    Ok(())
}

fn required_string(document: &Document, field: &str) -> Result<String> {
    match document.fields.get(field) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(nimbus_core::Error::Serialization(format!(
            "object manifest field {field} must be a string"
        ))),
        None => Err(nimbus_core::Error::Serialization(format!(
            "object manifest missing field {field}"
        ))),
    }
}

fn optional_string(document: &Document, field: &str) -> Result<Option<String>> {
    match document.fields.get(field) {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(nimbus_core::Error::Serialization(format!(
            "object manifest field {field} must be a string or null"
        ))),
    }
}

fn optional_json<T>(document: &Document, field: &str) -> Result<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    match document.fields.get(field) {
        Some(Value::Null) | None => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|err| {
                nimbus_core::Error::Serialization(format!("decode object field {field}: {err}"))
            }),
    }
}

fn required_u64(document: &Document, field: &str) -> Result<u64> {
    match document.fields.get(field).and_then(Value::as_u64) {
        Some(value) => Ok(value),
        None => Err(nimbus_core::Error::Serialization(format!(
            "object manifest field {field} must be an unsigned integer"
        ))),
    }
}

/// Writes a manifest row straight to `store`, outside the engine committer.
///
/// Test-only, and the four `*_direct` helpers below are the crate's only
/// object-metadata writers. Production publishes object metadata through the
/// engine's committer-sequenced commit path — see [`ObjectMetaRead`] for why a
/// store-level write entry point is unsafe. A test owns its whole tenant with
/// a single writer, so it may seed the metadata plane directly to exercise the
/// read half against each provider.
#[cfg(test)]
pub(crate) fn put_object_manifest_direct<S>(
    store: &S,
    manifest: &ObjectManifest,
) -> Result<CommitEntry>
where
    S: TenantPointRead + TenantPointWrite,
{
    let document = manifest.to_document()?;
    if store.get(&document.table, &document.id)?.is_some() {
        store.update_document_validated(&document.table, &document.id, &document.fields, |_, _| {
            Ok(())
        })
    } else {
        store.insert_document(&document)
    }
}

pub(super) fn get_object_manifest_for_store<S>(
    store: &S,
    bucket: &str,
    key: &str,
) -> Result<Option<ObjectManifest>>
where
    S: TenantPointRead,
{
    let table = object_manifest_table()?;
    let id = object_document_id(bucket, key)?;
    store
        .get(&table, &id)?
        .as_ref()
        .map(ObjectManifest::from_document)
        .transpose()
}

/// Test-only direct delete; see [`put_object_manifest_direct`].
#[cfg(test)]
pub(crate) fn delete_object_manifest_direct<S>(
    store: &S,
    bucket: &str,
    key: &str,
) -> Result<Option<(CommitEntry, ObjectManifest)>>
where
    S: TenantPointRead + TenantPointWrite,
{
    let table = object_manifest_table()?;
    let id = object_document_id(bucket, key)?;
    let Some(existing) = store.get(&table, &id)? else {
        return Ok(None);
    };
    let manifest = ObjectManifest::from_document(&existing)?;
    let (commit, _) = store.delete_document_validated(&table, &id, |_| Ok(()))?;
    Ok(Some((commit, manifest)))
}

pub(super) fn list_object_manifests_for_store<S>(
    store: &S,
    bucket: &str,
    prefix: &str,
    limit: usize,
) -> Result<Vec<ObjectManifest>>
where
    S: TenantRangeScan,
{
    validate_object_bucket(bucket)?;
    let table = object_manifest_table()?;
    let mut check_cancel = || Ok(());
    let mut manifests = store
        .scan_table_matching_with_filters_cancellable(&table, &[], &mut check_cancel, |document| {
            Ok(object_identity_matches_prefix(document, bucket, prefix))
        })?
        .iter()
        .map(ObjectManifest::from_document)
        .collect::<Result<Vec<_>>>()?;
    manifests.sort_by(|left, right| left.key.cmp(&right.key));
    if manifests.len() > limit {
        manifests.truncate(limit);
    }
    Ok(manifests)
}

/// Test-only direct write; see [`put_object_manifest_direct`].
#[cfg(test)]
pub(crate) fn put_multipart_upload_direct<S>(
    store: &S,
    upload: &ObjectMultipartUpload,
) -> Result<CommitEntry>
where
    S: TenantPointRead + TenantPointWrite,
{
    let document = upload.to_document()?;
    if store.get(&document.table, &document.id)?.is_some() {
        store.update_document_validated(&document.table, &document.id, &document.fields, |_, _| {
            Ok(())
        })
    } else {
        store.insert_document(&document)
    }
}

pub(super) fn get_multipart_upload_for_store<S>(
    store: &S,
    upload_id: &str,
) -> Result<Option<ObjectMultipartUpload>>
where
    S: TenantPointRead,
{
    let table = object_multipart_table()?;
    let id = upload_document_id(upload_id)?;
    store
        .get(&table, &id)?
        .as_ref()
        .map(ObjectMultipartUpload::from_document)
        .transpose()
}

/// Test-only direct delete; see [`put_object_manifest_direct`].
#[cfg(test)]
pub(crate) fn delete_multipart_upload_direct<S>(
    store: &S,
    upload_id: &str,
) -> Result<Option<(CommitEntry, ObjectMultipartUpload)>>
where
    S: TenantPointRead + TenantPointWrite,
{
    let table = object_multipart_table()?;
    let id = upload_document_id(upload_id)?;
    let Some(existing) = store.get(&table, &id)? else {
        return Ok(None);
    };
    let upload = ObjectMultipartUpload::from_document(&existing)?;
    let (commit, _) = store.delete_document_validated(&table, &id, |_| Ok(()))?;
    Ok(Some((commit, upload)))
}

pub(super) fn list_multipart_uploads_for_store<S>(
    store: &S,
    bucket: &str,
    prefix: &str,
    limit: usize,
) -> Result<Vec<ObjectMultipartUpload>>
where
    S: TenantRangeScan,
{
    validate_object_bucket(bucket)?;
    let table = object_multipart_table()?;
    let mut check_cancel = || Ok(());
    let mut uploads = store
        .scan_table_matching_with_filters_cancellable(&table, &[], &mut check_cancel, |document| {
            Ok(object_identity_matches_prefix(document, bucket, prefix))
        })?
        .iter()
        .map(ObjectMultipartUpload::from_document)
        .collect::<Result<Vec<_>>>()?;
    uploads.sort_by(|left, right| {
        left.key
            .cmp(&right.key)
            .then_with(|| left.initiated_at_millis.cmp(&right.initiated_at_millis))
            .then_with(|| left.upload_id.cmp(&right.upload_id))
    });
    if uploads.len() > limit {
        uploads.truncate(limit);
    }
    Ok(uploads)
}
