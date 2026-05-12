use nimbus_runtime::{
    InvocationAuth, RuntimeUserIdentity, VerifiedUserIdentity, VerifiedUserIdentityKind,
};

use super::super::config::ConvexAuthProvider;
use super::super::jwt::{
    ParsedJwt, normalize_issuer, select_jwk, validate_temporal_claims, verify_signature,
};
use super::ConvexAuthVerifier;
use crate::state::AppError;

struct ResolvedAuthIdentity {
    convex_identity: RuntimeUserIdentity,
    verified_identity: VerifiedUserIdentity,
}

impl ConvexAuthVerifier {
    pub(crate) async fn verify_bearer_token(
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
}
