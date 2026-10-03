//! Workers KV policy over Nimbus tenant KV storage.
//!
//! This module owns the namespace resolution, key encoding, value and metadata
//! limits, expiration rules, and list paging that the KV REST surface and the
//! Worker host bridge share. It has no transport. `nimbus-server` maps
//! [`KvError`] to the Cloudflare REST envelope.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use nimbus_core::{Error, SystemWallClock, WallClock};
use nimbus_storage::{KvEntry, KvScanPage};
use serde_json::Value;

use crate::CloudflareConfig;

const STORAGE_PREFIX: &[u8] = b"cloudflare-kv\0";
const METADATA_JSON_KEY: &str = "__cloudflare_metadata_json";
pub const MAX_KEY_BYTES: usize = 512;
pub const MAX_VALUE_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_METADATA_BYTES: usize = 1024;
const MIN_EXPIRATION_TTL_SECONDS: i64 = 60;
pub const DEFAULT_LIST_LIMIT: usize = 1000;
const MIN_LIST_LIMIT: usize = 10;
const MAX_LIST_LIMIT: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KvErrorKind {
    BadRequest,
    Unauthorized,
    NotFound,
    Internal,
}

#[derive(Debug)]
pub struct KvError {
    kind: KvErrorKind,
    message: String,
}

impl KvError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(KvErrorKind::BadRequest, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(KvErrorKind::Unauthorized, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(KvErrorKind::NotFound, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(KvErrorKind::Internal, message)
    }

    pub fn from_core(error: Error) -> Self {
        match error {
            Error::InvalidInput(message) => Self::bad_request(message),
            Error::TenantNotFound(_) | Error::NotFound(_) => Self::not_found(error.to_string()),
            Error::PermissionDenied(message) => Self::unauthorized(message),
            _ => Self::internal(error.to_string()),
        }
    }

    pub fn kind(&self) -> KvErrorKind {
        self.kind
    }

    pub fn into_message(self) -> String {
        self.message
    }

    fn new(kind: KvErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for KvError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

pub fn validate_list_limit(limit: usize) -> Result<(), KvError> {
    if !(MIN_LIST_LIMIT..=MAX_LIST_LIMIT).contains(&limit) {
        return Err(KvError::bad_request(format!(
            "Workers KV list limit must be between {MIN_LIST_LIMIT} and {MAX_LIST_LIMIT}"
        )));
    }
    Ok(())
}

pub fn finalize_list_page(
    page: KvScanPage,
    requested_limit: usize,
) -> (Vec<KvEntry>, Option<Vec<u8>>) {
    let mut entries = page.entries;
    let has_more = entries.len() > requested_limit;
    entries.truncate(requested_limit);
    let cursor = has_more.then(|| {
        entries
            .last()
            .expect("validated nonzero list limit must retain a cursor entry")
            .key
            .clone()
    });
    (entries, cursor)
}

pub fn resolve_worker_namespace(
    config: &CloudflareConfig,
    namespace_id: &str,
) -> Result<String, KvError> {
    if namespace_id.trim().is_empty() {
        return Err(KvError::bad_request("KV namespace id is required"));
    }
    if let Some(binding) = config.bindings().kv_namespaces().iter().find(|binding| {
        binding.binding == namespace_id
            || binding.id.as_deref() == Some(namespace_id)
            || binding.preview_id.as_deref() == Some(namespace_id)
    }) {
        return Ok(binding.binding.clone());
    }
    if config.bindings().kv_namespaces().is_empty() {
        return Ok(namespace_id.to_string());
    }
    Err(KvError::not_found(format!(
        "KV namespace `{namespace_id}` is not configured"
    )))
}

pub fn storage_key(namespace: &str, key: &str) -> Result<Vec<u8>, KvError> {
    if key.is_empty() {
        return Err(KvError::bad_request("Workers KV keys must not be empty"));
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(KvError::bad_request(format!(
            "Workers KV keys must be at most {MAX_KEY_BYTES} bytes"
        )));
    }
    storage_prefix(namespace, key)
}

pub fn storage_prefix(namespace: &str, key_prefix: &str) -> Result<Vec<u8>, KvError> {
    if namespace.as_bytes().contains(&0) || key_prefix.as_bytes().contains(&0) {
        return Err(KvError::bad_request(
            "Workers KV names must not contain NUL",
        ));
    }
    let mut key = Vec::with_capacity(STORAGE_PREFIX.len() + namespace.len() + 1 + key_prefix.len());
    key.extend_from_slice(STORAGE_PREFIX);
    key.extend_from_slice(namespace.as_bytes());
    key.push(0);
    key.extend_from_slice(key_prefix.as_bytes());
    Ok(key)
}

pub fn display_key(namespace: &str, storage_key: &[u8]) -> Result<String, KvError> {
    let prefix = storage_prefix(namespace, "")?;
    let Some(key) = storage_key.strip_prefix(prefix.as_slice()) else {
        return Err(KvError::internal(
            "KV scan returned an out-of-namespace key",
        ));
    };
    String::from_utf8(key.to_vec()).map_err(|_| KvError::internal("KV key is not valid UTF-8"))
}

pub fn resolve_expire_at_ms_values(
    expiration: Option<i64>,
    expiration_ttl: Option<i64>,
) -> Result<Option<i64>, KvError> {
    match (expiration, expiration_ttl) {
        (Some(_), Some(_)) => Err(KvError::bad_request(
            "expiration and expiration_ttl are mutually exclusive",
        )),
        (Some(expiration), None) => Ok(Some(expiration.saturating_mul(1000))),
        (None, Some(ttl)) => {
            if ttl < MIN_EXPIRATION_TTL_SECONDS {
                return Err(KvError::bad_request(format!(
                    "expiration_ttl must be at least {MIN_EXPIRATION_TTL_SECONDS} seconds"
                )));
            }
            Ok(Some(now_ms().saturating_add(ttl.saturating_mul(1000))))
        }
        (None, None) => Ok(None),
    }
}

pub fn encode_metadata_value(value: Option<&Value>) -> Result<BTreeMap<String, Vec<u8>>, KvError> {
    let mut metadata = BTreeMap::new();
    let Some(value) = value else {
        return Ok(metadata);
    };
    if value.is_null() {
        return Ok(metadata);
    }
    let encoded = serde_json::to_vec(&value)
        .map_err(|error| KvError::bad_request(format!("metadata must serialize: {error}")))?;
    if encoded.len() > MAX_METADATA_BYTES {
        return Err(KvError::bad_request(format!(
            "Workers KV metadata must be at most {MAX_METADATA_BYTES} bytes"
        )));
    }
    metadata.insert(METADATA_JSON_KEY.to_string(), encoded);
    Ok(metadata)
}

pub fn decode_metadata(metadata: &BTreeMap<String, Vec<u8>>) -> Value {
    metadata
        .get(METADATA_JSON_KEY)
        .and_then(|bytes| serde_json::from_slice(bytes).ok())
        .unwrap_or(Value::Null)
}

pub fn decode_cursor(raw: &str) -> Result<Vec<u8>, KvError> {
    URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| KvError::bad_request("KV list cursor is invalid"))
}

pub fn now_ms() -> i64 {
    let millis = SystemWallClock.now_millis();
    millis.min(i64::MAX as u64) as i64
}
