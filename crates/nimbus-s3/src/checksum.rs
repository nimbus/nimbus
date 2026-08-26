use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use nimbus_blob::BlobStore;
use nimbus_core::{Error, Result, StorageErrorKind};
use nimbus_storage::ObjectChecksums;
use nimbus_storage::ObjectChunkRef;
use s3s::crypto::{Checksum, Crc64Nvme, Md5};
use s3s::{S3Error, S3ErrorCode, S3Result};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt as _;

use crate::object_io::parse_blob_hash;

const CHECKSUM_READ_BUFFER_LEN: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComputedChecksums {
    pub md5_hex: String,
    pub md5_base64: String,
    pub md5_raw: [u8; 16],
    pub crc64nvme_base64: String,
    pub sha256_hex: String,
}

impl ComputedChecksums {
    pub fn for_bytes(bytes: &Bytes) -> Self {
        let md5_raw = Md5::checksum(bytes);
        let crc64 = Crc64Nvme::checksum(bytes);
        let sha256 = Sha256::digest(bytes);
        Self {
            md5_hex: hex::encode(md5_raw),
            md5_base64: STANDARD.encode(md5_raw),
            md5_raw,
            crc64nvme_base64: STANDARD.encode(crc64),
            sha256_hex: hex::encode(sha256),
        }
    }

    pub fn object_checksums(&self) -> ObjectChecksums {
        ObjectChecksums {
            content_md5: Some(self.md5_base64.clone()),
            crc64nvme: Some(self.crc64nvme_base64.clone()),
            sha256: Some(self.sha256_hex.clone()),
        }
    }

    pub fn verify_content_md5(&self, expected: Option<&str>) -> S3Result<()> {
        if let Some(expected) = expected
            && expected != self.md5_base64
        {
            return Err(S3Error::with_message(
                S3ErrorCode::BadDigest,
                "Content-MD5 does not match the uploaded bytes",
            ));
        }
        Ok(())
    }

    pub fn verify_crc64nvme(&self, expected: Option<&str>) -> S3Result<()> {
        if let Some(expected) = expected
            && expected != self.crc64nvme_base64
        {
            return Err(S3Error::with_message(
                S3ErrorCode::BadDigest,
                "CRC64NVME checksum does not match the uploaded bytes",
            ));
        }
        Ok(())
    }
}

/// Computes one full-object CRC64NVME over the selected multipart chunks.
///
/// The read stays bounded to one fixed buffer even when a blob backend later
/// makes `get_stream` lazy. Each part's recorded length is verified while the
/// checksum is built.
pub(crate) async fn crc64nvme_for_chunks(
    blobs: &dyn BlobStore,
    chunks: &[ObjectChunkRef],
) -> Result<String> {
    let mut checksum = Crc64Nvme::new();
    let mut buffer = vec![0_u8; CHECKSUM_READ_BUFFER_LEN];
    for chunk in chunks {
        let hash = parse_blob_hash(&chunk.blob_hash)?;
        let mut reader = blobs.get_stream(&hash).await?;
        let mut actual_len = 0_u64;
        loop {
            let read = reader.read(&mut buffer).await.map_err(|error| {
                Error::storage(
                    StorageErrorKind::Io,
                    format!("read multipart chunk {}: {error}", chunk.blob_hash),
                )
            })?;
            if read == 0 {
                break;
            }
            actual_len = actual_len.checked_add(read as u64).ok_or_else(|| {
                Error::storage(
                    StorageErrorKind::Corruption,
                    "multipart chunk length overflow",
                )
            })?;
            if actual_len > chunk.len {
                return Err(chunk_length_error(chunk, actual_len));
            }
            checksum.update(&buffer[..read]);
        }
        if actual_len != chunk.len {
            return Err(chunk_length_error(chunk, actual_len));
        }
    }
    Ok(STANDARD.encode(checksum.finalize()))
}

fn chunk_length_error(chunk: &ObjectChunkRef, actual_len: u64) -> Error {
    Error::storage(
        StorageErrorKind::Corruption,
        format!(
            "multipart chunk {} expected {} bytes but blob returned {actual_len}",
            chunk.blob_hash, chunk.len
        ),
    )
}

pub(crate) fn decode_md5_base64(value: &str) -> S3Result<[u8; 16]> {
    let bytes = STANDARD.decode(value).map_err(|_| {
        S3Error::with_message(
            S3ErrorCode::InvalidDigest,
            "Content-MD5 is not valid base64",
        )
    })?;
    bytes.try_into().map_err(|_| {
        S3Error::with_message(
            S3ErrorCode::InvalidDigest,
            "Content-MD5 must decode to 16 bytes",
        )
    })
}

pub(crate) fn multipart_etag(parts: &[[u8; 16]]) -> String {
    let mut concatenated = Vec::with_capacity(parts.len() * 16);
    for part in parts {
        concatenated.extend_from_slice(part);
    }
    let digest = Md5::checksum(&Bytes::from(concatenated));
    format!("{}-{}", hex::encode(digest), parts.len())
}
