use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::{HeaderMap, header};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use neovex_core::Error;
use neovex_runtime::{
    InvocationAuth, RuntimeUserIdentity, VerifiedUserIdentity, VerifiedUserIdentityKind,
};
use reqwest::Client;
use ring::signature;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::state::AppError;

const CLOCK_SKEW: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct ConvexAuthConfig {
    #[serde(default)]
    providers: Vec<ConvexAuthProvider>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum ConvexAuthProvider {
    Oidc(ConvexOidcAuthProvider),
    CustomJwt(ConvexCustomJwtAuthProvider),
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexOidcAuthProvider {
    pub(super) domain: String,
    #[serde(rename = "applicationID")]
    pub(super) application_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexCustomJwtAuthProvider {
    #[serde(rename = "type")]
    _provider_type: String,
    pub(super) issuer: String,
    pub(super) jwks: String,
    pub(super) algorithm: ConfiguredJwtAlgorithm,
    #[serde(rename = "applicationID")]
    pub(super) application_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(super) enum ConfiguredJwtAlgorithm {
    #[serde(rename = "RS256")]
    RS256,
    #[serde(rename = "ES256")]
    ES256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
enum ParsedJwtAlgorithm {
    #[serde(rename = "RS256")]
    RS256,
    #[serde(rename = "ES256")]
    ES256,
    #[serde(rename = "EdDSA")]
    EdDsa,
}

#[derive(Debug, Clone)]
struct ResolvedAuthIdentity {
    convex_identity: RuntimeUserIdentity,
    verified_identity: VerifiedUserIdentity,
}

#[derive(Clone)]
pub(super) struct ConvexAuthVerifier {
    client: Client,
    providers: Arc<Vec<ConvexAuthProvider>>,
}

impl std::fmt::Debug for ConvexAuthVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConvexAuthVerifier")
            .field("providers", &self.providers.len())
            .finish_non_exhaustive()
    }
}

impl ConvexAuthProvider {
    fn matches_token(&self, issuer: &str, audiences: &[String]) -> bool {
        match self {
            Self::Oidc(provider) => {
                normalize_issuer(&provider.domain) == normalize_issuer(issuer)
                    && audiences
                        .iter()
                        .any(|audience| audience == &provider.application_id)
            }
            Self::CustomJwt(provider) => {
                normalize_issuer(&provider.issuer) == normalize_issuer(issuer)
                    && provider
                        .application_id
                        .as_ref()
                        .is_none_or(|application_id| {
                            audiences.iter().any(|audience| audience == application_id)
                        })
            }
        }
    }

    fn allows_token_algorithm(&self, algorithm: ParsedJwtAlgorithm) -> bool {
        match self {
            Self::Oidc(_) => {
                matches!(
                    algorithm,
                    ParsedJwtAlgorithm::RS256 | ParsedJwtAlgorithm::EdDsa
                )
            }
            Self::CustomJwt(provider) => provider.algorithm.to_parsed() == algorithm,
        }
    }

    fn jwks_source(&self, metadata: Option<&OidcDiscoveryDocument>) -> Result<String, AppError> {
        match self {
            Self::Oidc(_) => metadata
                .map(|metadata| metadata.jwks_uri.clone())
                .ok_or_else(|| AppError::unauthorized("OIDC discovery metadata is missing JWKS")),
            Self::CustomJwt(provider) => Ok(provider.jwks.clone()),
        }
    }
}

impl ConvexAuthVerifier {
    pub(super) fn empty() -> Self {
        Self::new(ConvexAuthConfig::default())
    }

    pub(super) fn new(config: ConvexAuthConfig) -> Self {
        Self {
            client: Client::new(),
            providers: Arc::new(config.providers),
        }
    }

    pub(super) fn from_config(config: ConvexAuthConfig) -> Self {
        Self::new(config)
    }

    pub(super) async fn verify_authorization_header(
        &self,
        headers: &HeaderMap,
    ) -> Result<Option<InvocationAuth>, AppError> {
        let Some(token) = extract_bearer_token(headers)? else {
            return Ok(None);
        };
        let identity = self.verify_token(&token).await?;
        Ok(Some(InvocationAuth::with_identities(
            identity.convex_identity,
            identity.verified_identity,
            false,
        )))
    }

    pub(super) async fn verify_socket_token(
        &self,
        token: &str,
    ) -> Result<InvocationAuth, AppError> {
        let identity = self.verify_token(token).await?;
        Ok(InvocationAuth::with_identities(
            identity.convex_identity,
            identity.verified_identity,
            false,
        ))
    }

    async fn verify_token(&self, token: &str) -> Result<ResolvedAuthIdentity, AppError> {
        if self.providers.is_empty() {
            return Err(AppError::unauthorized(
                "no auth providers are configured; check convex/auth.config.ts",
            ));
        }

        let parsed = ParsedJwt::parse(token)?;
        let provider = self
            .providers
            .iter()
            .find(|provider| {
                provider.matches_token(&parsed.claims.issuer, &parsed.claims.audiences)
            })
            .cloned()
            .ok_or_else(|| {
                AppError::unauthorized(
                    "no auth provider matched this token; check convex/auth.config.ts",
                )
            })?;
        if !provider.allows_token_algorithm(parsed.header.algorithm) {
            return Err(AppError::unauthorized(format!(
                "token algorithm {} does not match configured provider",
                parsed.header.algorithm.as_str()
            )));
        }
        if matches!(&provider, ConvexAuthProvider::Oidc(_)) && parsed.claims.audiences.len() > 1 {
            return Err(AppError::unauthorized(
                "OIDC tokens with multiple audiences are not supported",
            ));
        }

        let discovery = match &provider {
            ConvexAuthProvider::Oidc(oidc) => Some(self.fetch_oidc_discovery(&oidc.domain).await?),
            ConvexAuthProvider::CustomJwt(_) => None,
        };
        if let Some(discovery) = discovery.as_ref()
            && normalize_issuer(&discovery.issuer) != normalize_issuer(&parsed.claims.issuer)
        {
            return Err(AppError::unauthorized(
                "token issuer does not match OIDC discovery metadata",
            ));
        }

        let jwks_source = provider.jwks_source(discovery.as_ref())?;
        let jwks = self.fetch_jwks(&jwks_source).await?;
        let key = select_jwk(&jwks, &parsed.header)?;
        verify_signature(&parsed, key)?;
        validate_temporal_claims(&parsed.claims)?;
        let verified_identity = parsed
            .claims
            .clone()
            .into_verified_identity(match &provider {
                ConvexAuthProvider::Oidc(_) => VerifiedUserIdentityKind::Oidc,
                ConvexAuthProvider::CustomJwt(_) => VerifiedUserIdentityKind::CustomJwt,
            });
        let convex_identity = match provider {
            ConvexAuthProvider::Oidc(_) => parsed.claims.into_convex_oidc_identity(),
            ConvexAuthProvider::CustomJwt(_) => parsed
                .claims
                .into_convex_custom_jwt_identity(&parsed.raw_claims),
        };
        Ok(ResolvedAuthIdentity {
            convex_identity,
            verified_identity,
        })
    }

    async fn fetch_oidc_discovery(&self, domain: &str) -> Result<OidcDiscoveryDocument, AppError> {
        let issuer = normalize_issuer(domain);
        let url = format!("{issuer}/.well-known/openid-configuration");
        let value = self.fetch_json_value(&url).await?;
        serde_json::from_value(value).map_err(|error| {
            AppError::unauthorized(format!("invalid OIDC discovery document: {error}"))
        })
    }

    async fn fetch_jwks(&self, source: &str) -> Result<JsonWebKeySet, AppError> {
        let value = self.fetch_json_value(source).await?;
        serde_json::from_value(value)
            .map_err(|error| AppError::unauthorized(format!("invalid JWKS document: {error}")))
    }

    async fn fetch_json_value(&self, source: &str) -> Result<Value, AppError> {
        Ok(if source.starts_with("data:") {
            decode_data_url_json(source)?
        } else {
            let response = self.client.get(source).send().await.map_err(|error| {
                AppError::unauthorized(format!("failed to fetch auth metadata: {error}"))
            })?;
            let status = response.status();
            if !status.is_success() {
                return Err(AppError::unauthorized(format!(
                    "failed to fetch auth metadata: received HTTP {status}"
                )));
            }
            response.json::<Value>().await.map_err(|error| {
                AppError::unauthorized(format!("failed to parse auth metadata: {error}"))
            })?
        })
    }
}

pub(super) fn read_auth_config(path: impl AsRef<Path>) -> Result<ConvexAuthConfig, Error> {
    let path = path.as_ref();
    if !path.is_file() {
        return Ok(ConvexAuthConfig::default());
    }

    let contents = std::fs::read_to_string(path).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to read Convex auth config {}: {error}",
            path.display()
        ))
    })?;
    serde_json::from_str(&contents).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse Convex auth config {}: {error}",
            path.display()
        ))
    })
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<Option<String>, AppError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|error| {
        AppError::unauthorized(format!("invalid authorization header: {error}"))
    })?;
    let (scheme, token) = value
        .split_once(' ')
        .ok_or_else(|| AppError::unauthorized("authorization header must use the Bearer scheme"))?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(AppError::unauthorized(
            "authorization header must use the Bearer scheme",
        ));
    }
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::unauthorized(
            "authorization header is missing a token",
        ));
    }
    Ok(Some(token.to_string()))
}

#[derive(Debug, Clone, Deserialize)]
struct OidcDiscoveryDocument {
    issuer: String,
    jwks_uri: String,
}

#[derive(Debug, Clone, Deserialize)]
struct JsonWebKeySet {
    keys: Vec<JsonWebKey>,
}

#[derive(Debug, Clone, Deserialize)]
struct JsonWebKey {
    kid: Option<String>,
    kty: String,
    alg: Option<String>,
    #[serde(rename = "use")]
    use_: Option<String>,
    n: Option<String>,
    e: Option<String>,
    crv: Option<String>,
    x: Option<String>,
    y: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct JwtHeader {
    #[serde(rename = "alg")]
    algorithm: ParsedJwtAlgorithm,
    kid: Option<String>,
}

#[derive(Debug, Clone)]
struct ParsedJwt {
    signing_input: String,
    signature: Vec<u8>,
    header: JwtHeader,
    raw_claims: Map<String, Value>,
    claims: ParsedClaims,
}

impl ParsedJwt {
    fn parse(token: &str) -> Result<Self, AppError> {
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
struct ParsedClaims {
    #[serde(rename = "iss")]
    issuer: String,
    #[serde(rename = "sub")]
    subject: String,
    #[serde(rename = "aud", default, deserialize_with = "deserialize_audiences")]
    audiences: Vec<String>,
    #[serde(rename = "exp")]
    expires_at: Option<u64>,
    #[serde(rename = "nbf")]
    not_before: Option<u64>,
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
    fn into_verified_identity(mut self, kind: VerifiedUserIdentityKind) -> VerifiedUserIdentity {
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

    fn into_convex_oidc_identity(mut self) -> RuntimeUserIdentity {
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

    fn into_convex_custom_jwt_identity(
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

fn select_jwk<'a>(jwks: &'a JsonWebKeySet, header: &JwtHeader) -> Result<&'a JsonWebKey, AppError> {
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

fn verify_signature(parsed: &ParsedJwt, jwk: &JsonWebKey) -> Result<(), AppError> {
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

fn validate_temporal_claims(claims: &ParsedClaims) -> Result<(), AppError> {
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

fn decode_data_url_json(source: &str) -> Result<Value, AppError> {
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

fn normalize_issuer(value: &str) -> Cow<'_, str> {
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
    fn to_parsed(self) -> ParsedJwtAlgorithm {
        match self {
            Self::RS256 => ParsedJwtAlgorithm::RS256,
            Self::ES256 => ParsedJwtAlgorithm::ES256,
        }
    }
}

impl ParsedJwtAlgorithm {
    fn as_str(self) -> &'static str {
        match self {
            Self::RS256 => "RS256",
            Self::ES256 => "ES256",
            Self::EdDsa => "EdDSA",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn custom_jwt_convex_projection_stays_narrower_than_verified_identity() {
        let raw_claims = serde_json::from_value::<Map<String, Value>>(json!({
            "iss": "https://issuer.example.com",
            "sub": "user-123",
            "aud": "neovex-test",
            "exp": 2_000_000_000u64,
            "email": "ada@example.com",
            "given_name": "Ada",
            "updated_at": 1710000000,
            "address": {
                "formatted": "123 Analytical Engine Way"
            },
            "profile": "https://example.com/ada",
            "role": "admin",
            "nested": {
                "team": "platform"
            }
        }))
        .expect("raw custom jwt claims should parse");
        let claims: ParsedClaims = serde_json::from_value(Value::Object(raw_claims.clone()))
            .expect("parsed claims should deserialize");

        let verified = claims
            .clone()
            .into_verified_identity(VerifiedUserIdentityKind::CustomJwt);
        let convex = claims.into_convex_custom_jwt_identity(&raw_claims);

        assert_eq!(verified.email.as_deref(), Some("ada@example.com"));
        assert_eq!(verified.given_name.as_deref(), Some("Ada"));
        assert_eq!(
            verified.address.as_deref(),
            Some("123 Analytical Engine Way")
        );
        assert_eq!(verified.updated_at.as_deref(), Some("1710000000"));
        assert_eq!(
            verified.profile_url.as_deref(),
            Some("https://example.com/ada")
        );

        assert_eq!(convex.email, None);
        assert_eq!(convex.given_name, None);
        assert_eq!(convex.address, None);
        assert_eq!(convex.updated_at, None);
        assert_eq!(convex.profile_url, None);
        assert_eq!(
            convex.custom_claims.get("email"),
            Some(&json!("ada@example.com"))
        );
        assert_eq!(convex.custom_claims.get("given_name"), Some(&json!("Ada")));
        assert_eq!(
            convex.custom_claims.get("updated_at"),
            Some(&json!(1710000000))
        );
        assert_eq!(
            convex.custom_claims.get("address.formatted"),
            Some(&json!("123 Analytical Engine Way"))
        );
        assert_eq!(convex.custom_claims.get("address"), None);
        assert_eq!(
            convex.custom_claims.get("profile"),
            Some(&json!("https://example.com/ada"))
        );
        assert_eq!(
            convex.custom_claims.get("nested.team"),
            Some(&json!("platform"))
        );
    }
}
