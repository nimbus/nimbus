use serde::Deserialize;
use serde_json::{Map, Value};

use nimbus_auth::ApplicationAuthError;

use super::super::super::config::ConfiguredJwtAlgorithm;
use super::super::parsing::{decode_base64_url, decode_json_segment};
use super::parsed_claims::ParsedClaims;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum ParsedJwtAlgorithm {
    #[serde(rename = "RS256")]
    RS256,
    #[serde(rename = "ES256")]
    ES256,
    #[serde(rename = "EdDSA")]
    EdDsa,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JwtHeader {
    #[serde(rename = "alg")]
    pub algorithm: ParsedJwtAlgorithm,
    pub kid: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedJwt {
    pub signing_input: String,
    pub signature: Vec<u8>,
    pub header: JwtHeader,
    pub raw_claims: Map<String, Value>,
    pub claims: ParsedClaims,
}

impl ParsedJwt {
    pub fn parse(token: &str) -> Result<Self, ApplicationAuthError> {
        let parts: Vec<_> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(ApplicationAuthError::unauthorized(
                "auth token must be a JWT with three dot-separated segments",
            ));
        }
        let header: JwtHeader = decode_json_segment(parts[0])?;
        let raw_claims: Map<String, Value> = decode_json_segment(parts[1])?;
        let claims: ParsedClaims = serde_json::from_value(Value::Object(raw_claims.clone()))
            .map_err(|error| {
                ApplicationAuthError::unauthorized(format!("invalid JWT JSON payload: {error}"))
            })?;
        let signature = decode_base64_url(parts[2]).map_err(|error| {
            ApplicationAuthError::unauthorized(format!("invalid JWT signature: {error}"))
        })?;
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
    pub fn to_parsed(self) -> ParsedJwtAlgorithm {
        match self {
            Self::RS256 => ParsedJwtAlgorithm::RS256,
            Self::ES256 => ParsedJwtAlgorithm::ES256,
        }
    }
}

impl ParsedJwtAlgorithm {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RS256 => "RS256",
            Self::ES256 => "ES256",
            Self::EdDsa => "EdDSA",
        }
    }
}
