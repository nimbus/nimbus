use std::borrow::Cow;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppError;

pub(in crate::adapters::convex::auth::jwt) fn decode_json_segment<T: for<'de> Deserialize<'de>>(
    segment: &str,
) -> Result<T, AppError> {
    let bytes = decode_base64_url(segment)
        .map_err(|error| AppError::unauthorized(format!("invalid JWT segment: {error}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| AppError::unauthorized(format!("invalid JWT JSON payload: {error}")))
}

pub(in crate::adapters::convex::auth::jwt) fn decode_base64_url(
    input: &str,
) -> Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD.decode(input)
}

pub(in crate::adapters::convex::auth) fn decode_data_url_json(
    source: &str,
) -> Result<Value, AppError> {
    let (metadata, payload) = source
        .split_once(',')
        .ok_or_else(|| AppError::unauthorized("invalid data URL in auth configuration"))?;
    let bytes = if metadata.ends_with(";base64") {
        STANDARD
            .decode(payload)
            .map_err(|error| AppError::unauthorized(format!("invalid base64 data URL: {error}")))?
    } else {
        payload.as_bytes().to_vec()
    };
    serde_json::from_slice(&bytes)
        .map_err(|error| AppError::unauthorized(format!("invalid JSON data URL: {error}")))
}

pub(in crate::adapters::convex::auth) fn normalize_issuer(value: &str) -> Cow<'_, str> {
    let value = value.trim_end_matches('/');
    if value.starts_with("https://") || value.starts_with("http://") {
        Cow::Borrowed(value)
    } else {
        Cow::Owned(format!("https://{value}"))
    }
}

pub(in crate::adapters::convex::auth::jwt) fn deserialize_audiences<'de, D>(
    deserializer: D,
) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(value)) => vec![value],
        Some(Value::Array(values)) => values
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect(),
        Some(other) => {
            return Err(serde::de::Error::custom(format!(
                "invalid aud claim: expected string or array, got {other}"
            )));
        }
    })
}
