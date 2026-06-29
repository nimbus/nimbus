use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use nimbus_storage::ObjectChecksums;
use s3s::crypto::{Checksum, Crc64Nvme, Md5};
use s3s::{S3Error, S3ErrorCode, S3Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComputedChecksums {
    pub md5_hex: String,
    pub md5_base64: String,
    pub md5_raw: [u8; 16],
    pub crc64nvme_base64: String,
}

impl ComputedChecksums {
    pub fn for_bytes(bytes: &Bytes) -> Self {
        let md5_raw = Md5::checksum(bytes);
        let crc64 = Crc64Nvme::checksum(bytes);
        Self {
            md5_hex: hex::encode(md5_raw),
            md5_base64: STANDARD.encode(md5_raw),
            md5_raw,
            crc64nvme_base64: STANDARD.encode(crc64),
        }
    }

    pub fn object_checksums(&self) -> ObjectChecksums {
        ObjectChecksums {
            content_md5: Some(self.md5_base64.clone()),
            crc64nvme: Some(self.crc64nvme_base64.clone()),
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
