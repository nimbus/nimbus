//! Convex-compatible file storage surface over the Nimbus object backend.
//!
//! Convex exposes opaque `_storage` document ids instead of S3 bucket/key names.
//! This module keeps that compatibility face as an adapter over the same
//! byte-plane and manifest-plane seam used by the S3 implementation.

use std::fmt::{Display, Formatter};
use std::io::{Cursor, Read, Write};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::Bytes;
use hmac::{Hmac, Mac};
use nimbus_core::{DocumentId, Error, ResolvedDocumentId, Result, TableName, TenantId};
use nimbus_storage::{ObjectManifest, ObjectManifestAttributes};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::Sha256;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::backend::{S3TenantObjects, S3TenantResolver, put_manifest_unconditional};
use crate::checksum::ComputedChecksums;
use crate::object_io::{
    read_manifest_bytes, release_manifest_blobs, release_manifest_blobs_except,
};

type HmacSha256 = Hmac<Sha256>;

pub const CONVEX_STORAGE_TABLE: &str = "_storage";
pub const CONVEX_STORAGE_BUCKET: &str = "_storage";
pub const CONVEX_DOWNLOAD_PATH_PREFIX: &str = "/_nimbus/convex/storage/";

const SYSTEM_STORAGE_ID: &str = "convex.storage_id";
const SYSTEM_STORAGE_RAW_ID: &str = "convex.storage_raw_id";
const SYSTEM_CREATED_AT_MILLIS: &str = "convex.creation_time_millis";
const SYSTEM_UPDATED_AT_MILLIS: &str = "convex.update_time_millis";
const SYSTEM_EXPORT_EXTENSION: &str = "convex.export_extension";
const EXPORT_MANIFEST: &str = "_storage/documents.jsonl";
const TOKEN_VERSION: &str = "v1";

#[derive(Debug)]
pub enum ConvexStorageError {
    InvalidToken(String),
    ExpiredToken,
    MissingObject,
    Forbidden(String),
    Core(Error),
    Archive(String),
}

impl Display for ConvexStorageError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidToken(message) => write!(f, "invalid Convex storage token: {message}"),
            Self::ExpiredToken => f.write_str("Convex storage token expired"),
            Self::MissingObject => f.write_str("Convex storage object not found"),
            Self::Forbidden(message) => write!(f, "Convex storage object forbidden: {message}"),
            Self::Core(error) => write!(f, "{error}"),
            Self::Archive(message) => write!(f, "Convex storage archive error: {message}"),
        }
    }
}

impl std::error::Error for ConvexStorageError {}

impl From<Error> for ConvexStorageError {
    fn from(error: Error) -> Self {
        Self::Core(error)
    }
}

pub type ConvexStorageResult<T> = std::result::Result<T, ConvexStorageError>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConvexStorageId {
    scoped: DocumentId,
    raw: DocumentId,
}

impl ConvexStorageId {
    pub fn generate() -> Result<Self> {
        let raw = DocumentId::from_key(format!("storage_{}", ulid::Ulid::new()))?;
        Self::from_raw(raw)
    }

    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let scoped = DocumentId::from_key(value.into())?;
        let resolved = ResolvedDocumentId::resolve_table_scoped(&storage_table()?, scoped.clone())?;
        Ok(Self {
            scoped,
            raw: resolved.into_document_id(),
        })
    }

    pub fn from_raw(raw: DocumentId) -> Result<Self> {
        let scoped = ResolvedDocumentId::encode_table_scoped(&storage_table()?, &raw)?;
        Ok(Self { scoped, raw })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.scoped.as_str()
    }

    #[must_use]
    pub fn raw_id(&self) -> &DocumentId {
        &self.raw
    }
}

impl Display for ConvexStorageId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConvexStorageMetadata {
    pub id: ConvexStorageId,
    pub creation_time_millis: u64,
    pub update_time_millis: u64,
    pub sha256: String,
    pub size: u64,
    pub content_type: Option<String>,
}

impl ConvexStorageMetadata {
    #[must_use]
    pub fn to_virtual_document(&self) -> Value {
        let mut fields = Map::from_iter([
            ("_id".to_string(), Value::String(self.id.to_string())),
            (
                "_creationTime".to_string(),
                json!(self.creation_time_millis),
            ),
            ("_updateTime".to_string(), json!(self.update_time_millis)),
            ("sha256".to_string(), Value::String(self.sha256.clone())),
            ("size".to_string(), json!(self.size)),
        ]);
        if let Some(content_type) = &self.content_type {
            fields.insert(
                "contentType".to_string(),
                Value::String(content_type.clone()),
            );
        }
        Value::Object(fields)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConvexStoredObject {
    pub metadata: ConvexStorageMetadata,
    pub bytes: Bytes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedDownloadToken {
    pub tenant: TenantId,
    pub storage_id: ConvexStorageId,
    pub expires_at_millis: u64,
}

#[derive(Clone)]
pub struct DownloadTokenSigner {
    secret: Arc<[u8]>,
}

impl DownloadTokenSigner {
    pub fn new(secret: impl Into<Vec<u8>>) -> ConvexStorageResult<Self> {
        let secret = secret.into();
        if secret.is_empty() {
            return Err(ConvexStorageError::InvalidToken(
                "HMAC secret cannot be empty".to_string(),
            ));
        }
        Ok(Self {
            secret: Arc::from(secret.into_boxed_slice()),
        })
    }

    pub fn sign(
        &self,
        tenant: &TenantId,
        storage_id: &ConvexStorageId,
        expires_at_millis: u64,
    ) -> ConvexStorageResult<String> {
        let payload = token_payload(tenant, storage_id, expires_at_millis);
        let signature = self.mac(payload.as_bytes())?;
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(payload),
            URL_SAFE_NO_PAD.encode(signature)
        ))
    }

    pub fn verify(
        &self,
        token: &str,
        now_millis: u64,
    ) -> ConvexStorageResult<VerifiedDownloadToken> {
        let (payload, signature) = token.split_once('.').ok_or_else(|| {
            ConvexStorageError::InvalidToken("token must contain payload and signature".to_string())
        })?;
        let payload = URL_SAFE_NO_PAD.decode(payload).map_err(|_| {
            ConvexStorageError::InvalidToken("payload is not base64url".to_string())
        })?;
        let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| {
            ConvexStorageError::InvalidToken("signature is not base64url".to_string())
        })?;
        let payload = String::from_utf8(payload).map_err(|_| {
            ConvexStorageError::InvalidToken("payload is not valid UTF-8".to_string())
        })?;
        self.verify_mac(payload.as_bytes(), &signature)?;

        let mut fields = payload.split('\n');
        let version = fields.next().unwrap_or_default();
        if version != TOKEN_VERSION {
            return Err(ConvexStorageError::InvalidToken(
                "unsupported token version".to_string(),
            ));
        }
        let tenant = fields
            .next()
            .ok_or_else(|| ConvexStorageError::InvalidToken("missing tenant".to_string()))
            .and_then(|value| {
                TenantId::new(value)
                    .map_err(|error| ConvexStorageError::InvalidToken(error.to_string()))
            })?;
        let storage_id = fields
            .next()
            .ok_or_else(|| ConvexStorageError::InvalidToken("missing storage id".to_string()))
            .and_then(|value| {
                ConvexStorageId::parse(value)
                    .map_err(|error| ConvexStorageError::InvalidToken(error.to_string()))
            })?;
        let expires_at_millis = fields
            .next()
            .ok_or_else(|| ConvexStorageError::InvalidToken("missing expiration".to_string()))?
            .parse::<u64>()
            .map_err(|_| {
                ConvexStorageError::InvalidToken("expiration is not an integer".to_string())
            })?;
        if fields.next().is_some() {
            return Err(ConvexStorageError::InvalidToken(
                "payload has trailing fields".to_string(),
            ));
        }
        if expires_at_millis < now_millis {
            return Err(ConvexStorageError::ExpiredToken);
        }
        Ok(VerifiedDownloadToken {
            tenant,
            storage_id,
            expires_at_millis,
        })
    }

    fn mac(&self, payload: &[u8]) -> ConvexStorageResult<Vec<u8>> {
        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .map_err(|_| ConvexStorageError::InvalidToken("invalid HMAC key".to_string()))?;
        mac.update(payload);
        Ok(mac.finalize().into_bytes().to_vec())
    }

    fn verify_mac(&self, payload: &[u8], signature: &[u8]) -> ConvexStorageResult<()> {
        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .map_err(|_| ConvexStorageError::InvalidToken("invalid HMAC key".to_string()))?;
        mac.update(payload);
        mac.verify_slice(signature)
            .map_err(|_| ConvexStorageError::InvalidToken("signature mismatch".to_string()))
    }
}

#[derive(Clone)]
pub struct ConvexObjectStorage {
    resolver: Arc<dyn S3TenantResolver>,
}

impl ConvexObjectStorage {
    #[must_use]
    pub fn new(resolver: Arc<dyn S3TenantResolver>) -> Self {
        Self { resolver }
    }

    pub async fn store(
        &self,
        tenant: &TenantId,
        bytes: Bytes,
        content_type: Option<String>,
        now_millis: u64,
    ) -> ConvexStorageResult<ConvexStorageMetadata> {
        let id = ConvexStorageId::generate()?;
        self.import_with_id(tenant, &id, bytes, content_type, now_millis, now_millis)
            .await
    }

    pub async fn import_with_id(
        &self,
        tenant: &TenantId,
        id: &ConvexStorageId,
        bytes: Bytes,
        content_type: Option<String>,
        creation_time_millis: u64,
        update_time_millis: u64,
    ) -> ConvexStorageResult<ConvexStorageMetadata> {
        self.resolver.ensure_tenant(tenant).await?;
        let ctx = self.resolver.resolve(tenant).await?;
        let previous = self.manifest_for_storage_id(&ctx, id).await?;
        let byte_len = bytes.len() as u64;
        let computed = ComputedChecksums::for_bytes(&bytes);
        let blobs = ctx.blobs().await?;
        let hash = blobs.put(bytes).await?;
        let mut attributes =
            ObjectManifestAttributes::new(computed.md5_hex.clone(), update_time_millis);
        attributes.content_type = content_type.clone();
        attributes.checksums = computed.object_checksums();
        attributes.system_metadata = convex_system_metadata(
            id,
            creation_time_millis,
            update_time_millis,
            content_type.as_deref(),
        );
        let manifest = ObjectManifest::whole(
            CONVEX_STORAGE_BUCKET,
            internal_object_key(),
            byte_len,
            hash.to_hex(),
            attributes,
        )?;
        // The Convex storage surface writes each object under a freshly
        // generated internal key and supersedes the old one by deleting it
        // below, so this write carries no expected state.
        put_manifest_unconditional(ctx.meta.as_ref(), manifest.clone()).await?;
        if let Some(previous) = previous
            && ctx
                .meta
                .delete_manifest(&previous.bucket, &previous.key)
                .await?
                .is_some()
        {
            release_manifest_blobs_except(blobs.as_ref(), &previous, Some(&manifest)).await?;
        }
        metadata_from_manifest(&manifest)?.ok_or_else(|| {
            ConvexStorageError::Core(Error::Internal(
                "stored Convex object did not carry Convex metadata".to_string(),
            ))
        })
    }

    pub async fn metadata(
        &self,
        tenant: &TenantId,
        id: &ConvexStorageId,
    ) -> ConvexStorageResult<Option<ConvexStorageMetadata>> {
        let ctx = self.resolver.resolve(tenant).await?;
        Ok(self
            .manifest_for_storage_id(&ctx, id)
            .await?
            .as_ref()
            .map(metadata_from_manifest)
            .transpose()?
            .flatten())
    }

    pub async fn read(
        &self,
        tenant: &TenantId,
        id: &ConvexStorageId,
    ) -> ConvexStorageResult<Option<ConvexStoredObject>> {
        let ctx = self.resolver.resolve(tenant).await?;
        let Some(manifest) = self.manifest_for_storage_id(&ctx, id).await? else {
            return Ok(None);
        };
        let metadata = metadata_from_manifest(&manifest)?.ok_or_else(|| {
            ConvexStorageError::Core(Error::Serialization(
                "Convex storage manifest missing virtual metadata".to_string(),
            ))
        })?;
        let blobs = ctx.blobs().await?;
        let bytes = read_manifest_bytes(blobs.as_ref(), &manifest).await?;
        Ok(Some(ConvexStoredObject { metadata, bytes }))
    }

    pub async fn delete(
        &self,
        tenant: &TenantId,
        id: &ConvexStorageId,
    ) -> ConvexStorageResult<bool> {
        let ctx = self.resolver.resolve(tenant).await?;
        let Some(manifest) = self.manifest_for_storage_id(&ctx, id).await? else {
            return Ok(false);
        };
        if ctx
            .meta
            .delete_manifest(&manifest.bucket, &manifest.key)
            .await?
            .is_some()
        {
            let blobs = ctx.blobs().await?;
            release_manifest_blobs(blobs.as_ref(), &manifest).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn download_with_token(
        &self,
        signer: &DownloadTokenSigner,
        token: &str,
        now_millis: u64,
    ) -> ConvexStorageResult<ConvexStoredObject> {
        let verified = signer.verify(token, now_millis)?;
        let ctx = self.resolver.resolve(&verified.tenant).await?;
        let Some(manifest) = self
            .manifest_for_storage_id(&ctx, &verified.storage_id)
            .await?
        else {
            return Err(ConvexStorageError::MissingObject);
        };
        let metadata = metadata_from_manifest(&manifest)?.ok_or_else(|| {
            ConvexStorageError::Core(Error::Serialization(
                "Convex storage manifest missing virtual metadata".to_string(),
            ))
        })?;
        let blobs = ctx.blobs().await?;
        let bytes = read_manifest_bytes(blobs.as_ref(), &manifest)
            .await
            .map_err(|error| match error {
                Error::NotFound(_) => ConvexStorageError::Forbidden(
                    "object bytes are no longer accessible".to_string(),
                ),
                other => ConvexStorageError::Core(other),
            })?;
        Ok(ConvexStoredObject { metadata, bytes })
    }

    pub async fn export_zip(&self, tenant: &TenantId) -> ConvexStorageResult<Bytes> {
        let ctx = self.resolver.resolve(tenant).await?;
        let raw_manifests = ctx
            .meta
            .list_manifests(CONVEX_STORAGE_BUCKET, "", usize::MAX)
            .await?;
        let mut manifests = Vec::new();
        for manifest in raw_manifests {
            if let Some(metadata) = metadata_from_manifest(&manifest)? {
                manifests.push((metadata, manifest));
            }
        }
        manifests.sort_by(|(left, _), (right, _)| left.id.cmp(&right.id));

        let mut documents = String::new();
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

        let blobs = ctx.blobs().await?;
        for (metadata, manifest) in manifests {
            let object = read_manifest_bytes(blobs.as_ref(), &manifest).await?;
            let document = metadata.to_virtual_document();
            documents.push_str(
                &serde_json::to_string(&document)
                    .map_err(|error| ConvexStorageError::Archive(error.to_string()))?,
            );
            documents.push('\n');
            writer
                .start_file(export_object_path(&metadata, &manifest), options)
                .map_err(|error| ConvexStorageError::Archive(error.to_string()))?;
            writer
                .write_all(&object)
                .map_err(|error| ConvexStorageError::Archive(error.to_string()))?;
        }

        writer
            .start_file(EXPORT_MANIFEST, options)
            .map_err(|error| ConvexStorageError::Archive(error.to_string()))?;
        writer
            .write_all(documents.as_bytes())
            .map_err(|error| ConvexStorageError::Archive(error.to_string()))?;
        let cursor = writer
            .finish()
            .map_err(|error| ConvexStorageError::Archive(error.to_string()))?;
        Ok(Bytes::from(cursor.into_inner()))
    }

    pub async fn import_zip(
        &self,
        tenant: &TenantId,
        archive: Bytes,
        now_millis: u64,
    ) -> ConvexStorageResult<Vec<ConvexStorageMetadata>> {
        let mut archive = zip::ZipArchive::new(Cursor::new(archive.to_vec()))
            .map_err(|error| ConvexStorageError::Archive(error.to_string()))?;
        let mut documents = String::new();
        archive
            .by_name(EXPORT_MANIFEST)
            .map_err(|error| ConvexStorageError::Archive(error.to_string()))?
            .read_to_string(&mut documents)
            .map_err(|error| ConvexStorageError::Archive(error.to_string()))?;

        let mut imported = Vec::new();
        for line in documents.lines().filter(|line| !line.trim().is_empty()) {
            let document: ExportStorageDocument = serde_json::from_str(line)
                .map_err(|error| ConvexStorageError::Archive(error.to_string()))?;
            let id = ConvexStorageId::parse(document.id.clone())?;
            let path =
                find_export_object_path(&mut archive, &id, document.content_type.as_deref())?;
            let mut object = Vec::new();
            archive
                .by_name(&path)
                .map_err(|error| ConvexStorageError::Archive(error.to_string()))?
                .read_to_end(&mut object)
                .map_err(|error| ConvexStorageError::Archive(error.to_string()))?;
            let update_time = document.update_time.unwrap_or(now_millis);
            let metadata = self
                .import_with_id(
                    tenant,
                    &id,
                    Bytes::from(object),
                    document.content_type.clone(),
                    document.creation_time,
                    update_time,
                )
                .await?;
            document.validate_against(&metadata)?;
            imported.push(metadata);
        }
        Ok(imported)
    }

    async fn manifest_for_storage_id(
        &self,
        ctx: &S3TenantObjects,
        id: &ConvexStorageId,
    ) -> ConvexStorageResult<Option<ObjectManifest>> {
        let manifests = ctx
            .meta
            .list_manifests(CONVEX_STORAGE_BUCKET, "", usize::MAX)
            .await?;
        Ok(manifests
            .into_iter()
            .find(|manifest| manifest_storage_id(manifest).as_deref() == Some(id.as_str())))
    }
}

pub fn metadata_from_manifest(
    manifest: &ObjectManifest,
) -> ConvexStorageResult<Option<ConvexStorageMetadata>> {
    let Some(storage_id) = manifest_storage_id(manifest) else {
        return Ok(None);
    };
    let id = ConvexStorageId::parse(storage_id)?;
    let creation_time_millis =
        system_u64(manifest, SYSTEM_CREATED_AT_MILLIS).unwrap_or(manifest.last_modified_millis);
    let update_time_millis =
        system_u64(manifest, SYSTEM_UPDATED_AT_MILLIS).unwrap_or(manifest.last_modified_millis);
    let sha256 = manifest.checksums.sha256.clone().ok_or_else(|| {
        ConvexStorageError::Core(Error::Serialization(
            "Convex storage manifest missing sha256 checksum".to_string(),
        ))
    })?;
    Ok(Some(ConvexStorageMetadata {
        id,
        creation_time_millis,
        update_time_millis,
        sha256,
        size: manifest.size,
        content_type: manifest.content_type.clone(),
    }))
}

pub fn download_url(base_url: &str, token: &str) -> String {
    format!(
        "{}/{}{}",
        base_url.trim_end_matches('/'),
        CONVEX_DOWNLOAD_PATH_PREFIX.trim_start_matches('/'),
        token
    )
}

fn token_payload(
    tenant: &TenantId,
    storage_id: &ConvexStorageId,
    expires_at_millis: u64,
) -> String {
    format!(
        "{TOKEN_VERSION}\n{}\n{}\n{expires_at_millis}",
        tenant.as_str(),
        storage_id.as_str()
    )
}

fn convex_system_metadata(
    id: &ConvexStorageId,
    creation_time_millis: u64,
    update_time_millis: u64,
    content_type: Option<&str>,
) -> Map<String, Value> {
    let mut metadata = Map::from_iter([
        (
            SYSTEM_STORAGE_ID.to_string(),
            Value::String(id.as_str().to_string()),
        ),
        (
            SYSTEM_STORAGE_RAW_ID.to_string(),
            Value::String(id.raw_id().as_str().to_string()),
        ),
        (
            SYSTEM_CREATED_AT_MILLIS.to_string(),
            json!(creation_time_millis),
        ),
        (
            SYSTEM_UPDATED_AT_MILLIS.to_string(),
            json!(update_time_millis),
        ),
    ]);
    let extension = export_extension(content_type);
    if !extension.is_empty() {
        metadata.insert(
            SYSTEM_EXPORT_EXTENSION.to_string(),
            Value::String(extension.to_string()),
        );
    }
    metadata
}

fn manifest_storage_id(manifest: &ObjectManifest) -> Option<String> {
    manifest
        .system_metadata
        .get(SYSTEM_STORAGE_ID)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn system_u64(manifest: &ObjectManifest, field: &str) -> Option<u64> {
    manifest.system_metadata.get(field).and_then(Value::as_u64)
}

fn system_string<'a>(manifest: &'a ObjectManifest, field: &str) -> Option<&'a str> {
    manifest.system_metadata.get(field).and_then(Value::as_str)
}

fn storage_table() -> Result<TableName> {
    TableName::new(CONVEX_STORAGE_TABLE)
}

fn internal_object_key() -> String {
    format!("convex/objects/{}", ulid::Ulid::new())
}

fn export_object_path(metadata: &ConvexStorageMetadata, manifest: &ObjectManifest) -> String {
    let extension = system_string(manifest, SYSTEM_EXPORT_EXTENSION)
        .unwrap_or_else(|| export_extension(metadata.content_type.as_deref()));
    format!("_storage/{}{}", metadata.id.raw_id().as_str(), extension)
}

fn find_export_object_path(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    id: &ConvexStorageId,
    content_type: Option<&str>,
) -> ConvexStorageResult<String> {
    let prefix = format!("_storage/{}", id.raw_id().as_str());
    let expected_path = format!("{}{}", prefix, export_extension(content_type));
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|error| ConvexStorageError::Archive(error.to_string()))?
            .name()
            .to_string();
        if name != EXPORT_MANIFEST && name == expected_path {
            return Ok(name);
        }
    }
    Err(ConvexStorageError::Archive(format!(
        "archive missing object bytes for {}",
        id
    )))
}

fn export_extension(content_type: Option<&str>) -> &'static str {
    match content_type {
        Some("application/json") => ".json",
        Some("application/pdf") => ".pdf",
        Some("image/jpeg") => ".jpg",
        Some("image/png") => ".png",
        Some("text/plain") => ".txt",
        _ => "",
    }
}

#[derive(Debug, Deserialize)]
struct ExportStorageDocument {
    #[serde(rename = "_id")]
    id: String,
    #[serde(rename = "_creationTime")]
    creation_time: u64,
    #[serde(rename = "_updateTime", default)]
    update_time: Option<u64>,
    #[serde(rename = "contentType", default)]
    content_type: Option<String>,
    sha256: String,
    size: u64,
}

impl ExportStorageDocument {
    fn validate_against(&self, metadata: &ConvexStorageMetadata) -> ConvexStorageResult<()> {
        if self.sha256 != metadata.sha256 || self.size != metadata.size {
            return Err(ConvexStorageError::Archive(format!(
                "archive metadata for {} does not match imported bytes",
                self.id
            )));
        }
        Ok(())
    }
}

#[must_use]
pub fn expires_at(now_millis: u64, ttl: Duration) -> u64 {
    let ttl_millis = ttl.as_millis().min(u128::from(u64::MAX)) as u64;
    now_millis.saturating_add(ttl_millis)
}
