use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

use crate::state::AppError;

use super::super::CLOCK_SKEW;
use super::models::ParsedClaims;

pub(in crate::adapters::convex::auth) fn validate_temporal_claims(
    claims: &ParsedClaims,
) -> Result<(), AppError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs();
    let now_with_skew = now.saturating_add(CLOCK_SKEW.as_secs());
    let now_without_skew = now.saturating_sub(CLOCK_SKEW.as_secs());
    if let Some(not_before) = claims.not_before
        && not_before > now_with_skew
    {
        return Err(AppError::unauthorized("auth token is not valid yet"));
    }
    let expires_at = claims
        .expires_at
        .ok_or_else(|| AppError::unauthorized("auth token is missing an exp claim"))?;
    if expires_at <= now_without_skew {
        return Err(AppError::unauthorized("auth token has expired"));
    }
    Ok(())
}

pub(in crate::adapters::convex::auth::jwt) fn strip_known_identity_claims(
    claims: &mut Map<String, Value>,
) {
    for key in [
        "iss",
        "sub",
        "aud",
        "exp",
        "nbf",
        "iat",
        "jti",
        "name",
        "given_name",
        "family_name",
        "nickname",
        "preferred_username",
        "profile",
        "picture",
        "email",
        "email_verified",
        "gender",
        "birthdate",
        "zoneinfo",
        "locale",
        "phone_number",
        "phone_number_verified",
        "address",
        "updated_at",
        "tokenIdentifier",
    ] {
        claims.remove(key);
    }
}

pub(in crate::adapters::convex::auth::jwt) fn extract_custom_jwt_claims(
    raw_claims: &Map<String, Value>,
) -> Map<String, Value> {
    let mut claims = Map::new();
    for (key, value) in raw_claims {
        if matches!(
            key.as_str(),
            "iss" | "sub" | "aud" | "exp" | "nbf" | "iat" | "jti"
        ) {
            continue;
        }
        flatten_custom_jwt_claim(&mut claims, key, value);
    }
    claims
}

fn flatten_custom_jwt_claim(claims: &mut Map<String, Value>, key: &str, value: &Value) {
    if let Value::Object(object) = value {
        for (nested_key, nested_value) in object {
            flatten_custom_jwt_claim(claims, &format!("{key}.{nested_key}"), nested_value);
        }
    } else {
        claims.insert(key.to_string(), value.clone());
    }
}

pub(in crate::adapters::convex::auth::jwt) fn extract_address_claim(
    value: Value,
) -> Option<String> {
    match value {
        Value::String(value) => Some(value),
        Value::Object(object) => object
            .get("formatted")
            .and_then(Value::as_str)
            .map(str::to_owned),
        _ => None,
    }
}
