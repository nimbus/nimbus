use std::borrow::Cow;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use neovex_runtime::{RuntimeUserIdentity, VerifiedUserIdentity, VerifiedUserIdentityKind};
use ring::signature;
use serde::Deserialize;
use serde_json::{Map, Value};

use super::{CLOCK_SKEW, ConfiguredJwtAlgorithm};
use crate::state::AppError;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct OidcDiscoveryDocument {
    pub(super) issuer: String,
    pub(super) jwks_uri: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct JsonWebKeySet {
    pub(super) keys: Vec<JsonWebKey>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct JsonWebKey {
    pub(super) kid: Option<String>,
    pub(super) kty: String,
    pub(super) alg: Option<String>,
    #[serde(rename = "use")]
    pub(super) use_: Option<String>,
    pub(super) n: Option<String>,
    pub(super) e: Option<String>,
    pub(super) crv: Option<String>,
    pub(super) x: Option<String>,
    pub(super) y: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(super) enum ParsedJwtAlgorithm {
    #[serde(rename = "RS256")]
    RS256,
    #[serde(rename = "ES256")]
    ES256,
    #[serde(rename = "EdDSA")]
    EdDsa,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct JwtHeader {
    #[serde(rename = "alg")]
    pub(super) algorithm: ParsedJwtAlgorithm,
    pub(super) kid: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedJwt {
    pub(super) signing_input: String,
    pub(super) signature: Vec<u8>,
    pub(super) header: JwtHeader,
    pub(super) raw_claims: Map<String, Value>,
    pub(super) claims: ParsedClaims,
}

impl ParsedJwt {
    pub(super) fn parse(token: &str) -> Result<Self, AppError> {
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

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ParsedClaims {
    #[serde(rename = "iss")]
    pub(super) issuer: String,
    #[serde(rename = "sub")]
    pub(super) subject: String,
    #[serde(rename = "aud", default, deserialize_with = "deserialize_audiences")]
    pub(super) audiences: Vec<String>,
    #[serde(rename = "exp")]
    pub(super) expires_at: Option<u64>,
    #[serde(rename = "nbf")]
    pub(super) not_before: Option<u64>,
    name: Option<String>,
    #[serde(rename = "given_name")]
    given_name: Option<String>,
    #[serde(rename = "family_name")]
    family_name: Option<String>,
    nickname: Option<String>,
    #[serde(rename = "preferred_username")]
    preferred_username: Option<String>,
    #[serde(rename = "profile")]
    profile_url: Option<String>,
    #[serde(rename = "picture")]
    picture_url: Option<String>,
    email: Option<String>,
    #[serde(rename = "email_verified")]
    email_verified: Option<bool>,
    gender: Option<String>,
    #[serde(rename = "birthdate")]
    birthday: Option<String>,
    #[serde(rename = "zoneinfo")]
    timezone: Option<String>,
    #[serde(rename = "locale")]
    language: Option<String>,
    #[serde(rename = "phone_number")]
    phone_number: Option<String>,
    #[serde(rename = "phone_number_verified")]
    phone_number_verified: Option<bool>,
    address: Option<Value>,
    #[serde(rename = "updated_at")]
    updated_at: Option<Value>,
    #[serde(flatten)]
    other_claims: Map<String, Value>,
}

impl ParsedClaims {
    pub(super) fn into_verified_identity(
        mut self,
        kind: VerifiedUserIdentityKind,
    ) -> VerifiedUserIdentity {
        strip_known_identity_claims(&mut self.other_claims);
        VerifiedUserIdentity {
            kind,
            token_identifier: format!("{}|{}", self.issuer, self.subject),
            subject: self.subject,
            issuer: self.issuer,
            name: self.name,
            given_name: self.given_name,
            family_name: self.family_name,
            nickname: self.nickname,
            preferred_username: self.preferred_username,
            profile_url: self.profile_url,
            picture_url: self.picture_url,
            email: self.email,
            email_verified: self.email_verified,
            gender: self.gender,
            birthday: self.birthday,
            timezone: self.timezone,
            language: self.language,
            phone_number: self.phone_number,
            phone_number_verified: self.phone_number_verified,
            address: self.address.and_then(extract_address_claim),
            updated_at: self.updated_at.map(|value| match value {
                Value::String(value) => value,
                Value::Number(value) => value.to_string(),
                other => other.to_string(),
            }),
            custom_claims: self.other_claims,
        }
    }

    pub(super) fn into_convex_oidc_identity(mut self) -> RuntimeUserIdentity {
        strip_known_identity_claims(&mut self.other_claims);
        RuntimeUserIdentity {
            token_identifier: format!("{}|{}", self.issuer, self.subject),
            subject: self.subject,
            issuer: self.issuer,
            name: self.name,
            given_name: self.given_name,
            family_name: self.family_name,
            nickname: self.nickname,
            preferred_username: self.preferred_username,
            profile_url: self.profile_url,
            picture_url: self.picture_url,
            email: self.email,
            email_verified: self.email_verified,
            gender: self.gender,
            birthday: self.birthday,
            timezone: self.timezone,
            language: self.language,
            phone_number: self.phone_number,
            phone_number_verified: self.phone_number_verified,
            address: self.address.and_then(extract_address_claim),
            updated_at: self.updated_at.map(|value| match value {
                Value::String(value) => value,
                Value::Number(value) => value.to_string(),
                other => other.to_string(),
            }),
            custom_claims: self.other_claims,
        }
    }

    pub(super) fn into_convex_custom_jwt_identity(
        self,
        raw_claims: &Map<String, Value>,
    ) -> RuntimeUserIdentity {
        RuntimeUserIdentity {
            token_identifier: format!("{}|{}", self.issuer, self.subject),
            subject: self.subject,
            issuer: self.issuer,
            name: None,
            given_name: None,
            family_name: None,
            nickname: None,
            preferred_username: None,
            profile_url: None,
            picture_url: None,
            email: None,
            email_verified: None,
            gender: None,
            birthday: None,
            timezone: None,
            language: None,
            phone_number: None,
            phone_number_verified: None,
            address: None,
            updated_at: None,
            custom_claims: extract_custom_jwt_claims(raw_claims),
        }
    }
}

pub(super) fn select_jwk<'a>(
    jwks: &'a JsonWebKeySet,
    header: &JwtHeader,
) -> Result<&'a JsonWebKey, AppError> {
    let kid = header
        .kid
        .as_deref()
        .ok_or_else(|| AppError::unauthorized("auth token is missing a JWT kid header"))?;
    jwks.keys
        .iter()
        .find(|key| {
            key.kid.as_deref() == Some(kid)
                && key.use_.as_deref().is_none_or(|use_| use_ == "sig")
                && key
                    .alg
                    .as_deref()
                    .is_none_or(|alg| alg == header.algorithm.as_str())
        })
        .ok_or_else(|| {
            AppError::unauthorized("auth token kid did not match any configured JWKS signing key")
        })
}

pub(super) fn verify_signature(parsed: &ParsedJwt, jwk: &JsonWebKey) -> Result<(), AppError> {
    match parsed.header.algorithm {
        ParsedJwtAlgorithm::RS256 => verify_rsa_signature(parsed, jwk),
        ParsedJwtAlgorithm::ES256 => verify_es256_signature(parsed, jwk),
        ParsedJwtAlgorithm::EdDsa => verify_ed25519_signature(parsed, jwk),
    }
}

fn verify_rsa_signature(parsed: &ParsedJwt, jwk: &JsonWebKey) -> Result<(), AppError> {
    if jwk.kty != "RSA" {
        return Err(AppError::unauthorized("JWKS key type did not match RS256"));
    }
    let modulus = decode_base64_url(
        jwk.n
            .as_deref()
            .ok_or_else(|| AppError::unauthorized("RSA JWKS key is missing modulus"))?,
    )
    .map_err(|error| AppError::unauthorized(format!("invalid RSA modulus: {error}")))?;
    let exponent = decode_base64_url(
        jwk.e
            .as_deref()
            .ok_or_else(|| AppError::unauthorized("RSA JWKS key is missing exponent"))?,
    )
    .map_err(|error| AppError::unauthorized(format!("invalid RSA exponent: {error}")))?;
    let key = signature::RsaPublicKeyComponents {
        n: &modulus,
        e: &exponent,
    };
    key.verify(
        &signature::RSA_PKCS1_2048_8192_SHA256,
        parsed.signing_input.as_bytes(),
        &parsed.signature,
    )
    .map_err(|_| AppError::unauthorized("invalid JWT signature"))
}

fn verify_es256_signature(parsed: &ParsedJwt, jwk: &JsonWebKey) -> Result<(), AppError> {
    if jwk.kty != "EC" {
        return Err(AppError::unauthorized("JWKS key type did not match ES256"));
    }
    if jwk.crv.as_deref() != Some("P-256") {
        return Err(AppError::unauthorized("ES256 requires a P-256 JWKS key"));
    }
    let x = decode_base64_url(
        jwk.x
            .as_deref()
            .ok_or_else(|| AppError::unauthorized("EC JWKS key is missing x coordinate"))?,
    )
    .map_err(|error| AppError::unauthorized(format!("invalid EC x coordinate: {error}")))?;
    let y = decode_base64_url(
        jwk.y
            .as_deref()
            .ok_or_else(|| AppError::unauthorized("EC JWKS key is missing y coordinate"))?,
    )
    .map_err(|error| AppError::unauthorized(format!("invalid EC y coordinate: {error}")))?;
    let mut public_key = Vec::with_capacity(1 + x.len() + y.len());
    public_key.push(0x04);
    public_key.extend_from_slice(&x);
    public_key.extend_from_slice(&y);
    signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, public_key)
        .verify(parsed.signing_input.as_bytes(), &parsed.signature)
        .map_err(|_| AppError::unauthorized("invalid JWT signature"))
}

fn verify_ed25519_signature(parsed: &ParsedJwt, jwk: &JsonWebKey) -> Result<(), AppError> {
    if jwk.kty != "OKP" {
        return Err(AppError::unauthorized("JWKS key type did not match EdDSA"));
    }
    if jwk.crv.as_deref() != Some("Ed25519") {
        return Err(AppError::unauthorized("EdDSA requires an Ed25519 JWKS key"));
    }
    let public_key = decode_base64_url(
        jwk.x
            .as_deref()
            .ok_or_else(|| AppError::unauthorized("Ed25519 JWKS key is missing x coordinate"))?,
    )
    .map_err(|error| AppError::unauthorized(format!("invalid Ed25519 public key: {error}")))?;
    signature::UnparsedPublicKey::new(&signature::ED25519, public_key)
        .verify(parsed.signing_input.as_bytes(), &parsed.signature)
        .map_err(|_| AppError::unauthorized("invalid JWT signature"))
}

pub(super) fn validate_temporal_claims(claims: &ParsedClaims) -> Result<(), AppError> {
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

fn decode_json_segment<T: for<'de> Deserialize<'de>>(segment: &str) -> Result<T, AppError> {
    let bytes = decode_base64_url(segment)
        .map_err(|error| AppError::unauthorized(format!("invalid JWT segment: {error}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| AppError::unauthorized(format!("invalid JWT JSON payload: {error}")))
}

fn decode_base64_url(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD.decode(input)
}

pub(super) fn decode_data_url_json(source: &str) -> Result<Value, AppError> {
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

pub(super) fn normalize_issuer(value: &str) -> Cow<'_, str> {
    let value = value.trim_end_matches('/');
    if value.starts_with("https://") || value.starts_with("http://") {
        Cow::Borrowed(value)
    } else {
        Cow::Owned(format!("https://{value}"))
    }
}

fn deserialize_audiences<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
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

fn strip_known_identity_claims(claims: &mut Map<String, Value>) {
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

fn extract_custom_jwt_claims(raw_claims: &Map<String, Value>) -> Map<String, Value> {
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

fn extract_address_claim(value: Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value),
        Value::Object(object) => object
            .get("formatted")
            .and_then(Value::as_str)
            .map(str::to_owned),
        _ => None,
    }
}

impl ConfiguredJwtAlgorithm {
    pub(super) fn to_parsed(self) -> ParsedJwtAlgorithm {
        match self {
            Self::RS256 => ParsedJwtAlgorithm::RS256,
            Self::ES256 => ParsedJwtAlgorithm::ES256,
        }
    }
}

impl ParsedJwtAlgorithm {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::RS256 => "RS256",
            Self::ES256 => "ES256",
            Self::EdDsa => "EdDSA",
        }
    }
}
