use std::time::Instant;

use serde_json::Value;

use super::super::jwt::{
    JsonWebKeySet, OidcDiscoveryDocument, decode_data_url_json, normalize_issuer,
};
use super::{ConvexAuthVerifier, MetadataCacheEntry};
use nimbus_auth::ApplicationAuthError;

impl ConvexAuthVerifier {
    pub async fn fetch_oidc_discovery(
        &self,
        domain: &str,
    ) -> Result<OidcDiscoveryDocument, ApplicationAuthError> {
        let issuer = normalize_issuer(domain);
        let url = format!("{issuer}/.well-known/openid-configuration");
        let value = self.fetch_json_value(&url).await?;
        serde_json::from_value(value).map_err(|error| {
            ApplicationAuthError::unauthorized(format!("invalid OIDC discovery document: {error}"))
        })
    }

    pub(super) async fn refresh_oidc_discovery(
        &self,
        domain: &str,
    ) -> Result<OidcDiscoveryDocument, ApplicationAuthError> {
        let issuer = normalize_issuer(domain);
        let url = format!("{issuer}/.well-known/openid-configuration");
        let value = self.fetch_json_value_uncached(&url).await?;
        self.cache_metadata_value(&url, value.clone());
        serde_json::from_value(value).map_err(|error| {
            ApplicationAuthError::unauthorized(format!("invalid OIDC discovery document: {error}"))
        })
    }

    pub async fn fetch_jwks(&self, source: &str) -> Result<JsonWebKeySet, ApplicationAuthError> {
        let value = self.fetch_json_value(source).await?;
        serde_json::from_value(value).map_err(|error| {
            ApplicationAuthError::unauthorized(format!("invalid JWKS document: {error}"))
        })
    }

    pub(super) async fn refresh_jwks(
        &self,
        source: &str,
    ) -> Result<JsonWebKeySet, ApplicationAuthError> {
        let value = self.fetch_json_value_uncached(source).await?;
        self.cache_metadata_value(source, value.clone());
        serde_json::from_value(value).map_err(|error| {
            ApplicationAuthError::unauthorized(format!("invalid JWKS document: {error}"))
        })
    }

    async fn fetch_json_value(&self, source: &str) -> Result<Value, ApplicationAuthError> {
        if let Some(value) = self.cached_metadata_value(source) {
            return Ok(value);
        }
        let value = self.fetch_json_value_uncached(source).await?;
        self.cache_metadata_value(source, value.clone());
        Ok(value)
    }

    async fn fetch_json_value_uncached(&self, source: &str) -> Result<Value, ApplicationAuthError> {
        Ok(if source.starts_with("data:") {
            decode_data_url_json(source)?
        } else {
            let response = self.client.get(source).send().await.map_err(|error| {
                ApplicationAuthError::unauthorized(format!(
                    "failed to fetch auth metadata: {error}"
                ))
            })?;
            let status = response.status();
            if !status.is_success() {
                return Err(ApplicationAuthError::unauthorized(format!(
                    "failed to fetch auth metadata: received HTTP {status}"
                )));
            }
            response.json::<Value>().await.map_err(|error| {
                ApplicationAuthError::unauthorized(format!(
                    "failed to parse auth metadata: {error}"
                ))
            })?
        })
    }

    fn cached_metadata_value(&self, source: &str) -> Option<Value> {
        let now = Instant::now();
        let guard = self
            .metadata_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .get(source)
            .filter(|entry| entry.expires_at > now)
            .map(|entry| entry.value.clone())
    }

    fn cache_metadata_value(&self, source: &str, value: Value) {
        let expires_at = Instant::now() + self.metadata_cache_ttl;
        self.metadata_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(source.to_string(), MetadataCacheEntry { expires_at, value });
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn auth_metadata_is_cached_and_refresh_can_replace_stale_jwks() {
        let verifier = ConvexAuthVerifier::empty();
        let source = data_url(json!({
            "keys": [
                {
                    "kid": "fresh",
                    "kty": "OKP",
                    "crv": "Ed25519",
                    "x": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                }
            ]
        }));
        verifier.cache_metadata_value(&source, json!({ "keys": [] }));

        let cached = verifier
            .fetch_jwks(&source)
            .await
            .expect("cached empty JWKS should parse");
        assert!(cached.keys.is_empty());

        let refreshed = verifier
            .refresh_jwks(&source)
            .await
            .expect("refresh should bypass cached empty JWKS");
        assert_eq!(refreshed.keys.len(), 1);
        assert_eq!(refreshed.keys[0].kid.as_deref(), Some("fresh"));

        let cached_after_refresh = verifier
            .fetch_jwks(&source)
            .await
            .expect("refreshed JWKS should be cached");
        assert_eq!(cached_after_refresh.keys.len(), 1);
    }

    fn data_url(value: serde_json::Value) -> String {
        format!(
            "data:application/json;base64,{}",
            BASE64.encode(serde_json::to_vec(&value).expect("metadata should serialize"))
        )
    }
}
