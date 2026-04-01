use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderMap, header};
use neovex_core::Error;
use neovex_runtime::{
    InvocationAuth, RuntimeUserIdentity, VerifiedUserIdentity, VerifiedUserIdentityKind,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppError;

mod jwt;
#[cfg(test)]
mod tests;

use self::jwt::{
    JsonWebKeySet, OidcDiscoveryDocument, ParsedJwt, ParsedJwtAlgorithm, decode_data_url_json,
    normalize_issuer, select_jwk, validate_temporal_claims, verify_signature,
};

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
