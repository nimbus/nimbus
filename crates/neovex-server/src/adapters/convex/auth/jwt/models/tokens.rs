use serde::Deserialize;
use serde_json::{Map, Value};

use crate::state::AppError;

use super::super::super::config::ConfiguredJwtAlgorithm;
use super::super::parsing::{decode_base64_url, decode_json_segment};
use super::parsed_claims::ParsedClaims;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(in crate::adapters::convex::auth) enum ParsedJwtAlgorithm {
    #[serde(rename = "RS256")]
    RS256,
    #[serde(rename = "ES256")]
    ES256,
    #[serde(rename = "EdDSA")]
    EdDsa,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex::auth) struct JwtHeader {
    #[serde(rename = "alg")]
    pub(in crate::adapters::convex::auth) algorithm: ParsedJwtAlgorithm,
    pub(in crate::adapters::convex::auth) kid: Option<String>,
}

#[derive(Debug, Clone)]
pub(in crate::adapters::convex::auth) struct ParsedJwt {
    pub(in crate::adapters::convex::auth::jwt) signing_input: String,
    pub(in crate::adapters::convex::auth::jwt) signature: Vec<u8>,
    pub(in crate::adapters::convex::auth) header: JwtHeader,
    pub(in crate::adapters::convex::auth) raw_claims: Map<String, Value>,
    pub(in crate::adapters::convex::auth) claims: ParsedClaims,
}

impl ParsedJwt {
    pub(in crate::adapters::convex::auth) fn parse(token: &str) -> Result<Self, AppError> {
        let parts: Vec<_> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(AppError::unauthorized(
                "auth token must be a JWT with three dot-separated segments",
            ));
        }
        let header: JwtHeader = decode_json_segment(parts[0])?;
        let raw_claims: Map<String, Value> = decode_json_segment(parts[1])?;
        let claims: ParsedClaims = serde_json::from_value(Value::Object(raw_claims.clone()))
            .map_err(|error| {
                AppError::unauthorized(format!("invalid JWT JSON payload: {error}"))
            })?;
        let signature = decode_base64_url(parts[2])
            .map_err(|error| AppError::unauthorized(format!("invalid JWT signature: {error}")))?;
        Ok(Self {
            signing_input: format!("{}.{}", parts[0], parts[1]),
            signature,
            header,
            raw_claims,
            claims,
        })
    }
}

impl ConfiguredJwtAlgorithm {
    pub(in crate::adapters::convex::auth) fn to_parsed(self) -> ParsedJwtAlgorithm {
        match self {
            Self::RS256 => ParsedJwtAlgorithm::RS256,
            Self::ES256 => ParsedJwtAlgorithm::ES256,
        }
    }
}

impl ParsedJwtAlgorithm {
    pub(in crate::adapters::convex::auth) fn as_str(self) -> &'static str {
        match self {
            Self::RS256 => "RS256",
            Self::ES256 => "ES256",
            Self::EdDsa => "EdDSA",
        }
    }
}
