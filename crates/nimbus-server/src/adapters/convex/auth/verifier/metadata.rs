use serde_json::Value;

use super::super::jwt::{
    JsonWebKeySet, OidcDiscoveryDocument, decode_data_url_json, normalize_issuer,
};
use super::ConvexAuthVerifier;
use crate::state::AppError;

impl ConvexAuthVerifier {
    pub(in crate::adapters::convex::auth::verifier) async fn fetch_oidc_discovery(
        &self,
        domain: &str,
    ) -> Result<OidcDiscoveryDocument, AppError> {
        let issuer = normalize_issuer(domain);
        let url = format!("{issuer}/.well-known/openid-configuration");
        let value = self.fetch_json_value(&url).await?;
        serde_json::from_value(value).map_err(|error| {
            AppError::unauthorized(format!("invalid OIDC discovery document: {error}"))
        })
    }

    pub(in crate::adapters::convex::auth::verifier) async fn fetch_jwks(
        &self,
        source: &str,
    ) -> Result<JsonWebKeySet, AppError> {
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
