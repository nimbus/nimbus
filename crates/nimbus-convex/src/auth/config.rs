use std::path::Path;

use nimbus_core::Error;
use serde::Deserialize;

use super::jwt::{OidcDiscoveryDocument, ParsedJwtAlgorithm, normalize_issuer};
use nimbus_auth::ApplicationAuthError;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConvexAuthConfig {
    #[serde(default)]
    pub providers: Vec<ConvexAuthProvider>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ConvexAuthProvider {
    Oidc(ConvexOidcAuthProvider),
    CustomJwt(ConvexCustomJwtAuthProvider),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConvexOidcAuthProvider {
    pub domain: String,
    #[serde(rename = "applicationID")]
    pub application_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConvexCustomJwtAuthProvider {
    #[serde(rename = "type")]
    _provider_type: String,
    pub issuer: String,
    pub jwks: String,
    pub algorithm: ConfiguredJwtAlgorithm,
    #[serde(rename = "applicationID")]
    pub application_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum ConfiguredJwtAlgorithm {
    #[serde(rename = "RS256")]
    RS256,
    #[serde(rename = "ES256")]
    ES256,
}

impl ConvexAuthProvider {
    pub fn matches_token(&self, issuer: &str, audiences: &[String]) -> bool {
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

    pub fn allows_token_algorithm(&self, algorithm: ParsedJwtAlgorithm) -> bool {
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

    pub fn jwks_source(
        &self,
        metadata: Option<&OidcDiscoveryDocument>,
    ) -> Result<String, ApplicationAuthError> {
        match self {
            Self::Oidc(_) => metadata
                .map(|metadata| metadata.jwks_uri.clone())
                .ok_or_else(|| {
                    ApplicationAuthError::unauthorized("OIDC discovery metadata is missing JWKS")
                }),
            Self::CustomJwt(provider) => Ok(provider.jwks.clone()),
        }
    }
}

pub fn read_auth_config(path: impl AsRef<Path>) -> Result<ConvexAuthConfig, Error> {
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
