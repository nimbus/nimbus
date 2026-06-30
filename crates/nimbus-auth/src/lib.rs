use std::error::Error as StdError;
use std::fmt;

use futures::future::BoxFuture;
use nimbus_core::{InvocationAuth, PrincipalContext};
use serde::Serialize;
use serde_json::{Map, Value};

pub trait ApplicationAuthVerifier: Send + Sync {
    fn verify_bearer_token<'a>(
        &'a self,
        token: &'a str,
    ) -> BoxFuture<'a, Result<InvocationAuth, ApplicationAuthError>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplicationAuthError {
    Unauthorized(String),
    Forbidden(String),
    Internal(String),
}

impl ApplicationAuthError {
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Unauthorized(message) | Self::Forbidden(message) | Self::Internal(message) => {
                message
            }
        }
    }
}

impl fmt::Display for ApplicationAuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl StdError for ApplicationAuthError {}

#[derive(Debug, Clone)]
pub struct ResolvedApplicationAuth {
    pub auth: Option<InvocationAuth>,
    pub principal: PrincipalContext,
}

impl ResolvedApplicationAuth {
    pub fn anonymous() -> Self {
        Self {
            auth: None,
            principal: PrincipalContext::anonymous(),
        }
    }
}

/// DEV-MODE TOKEN-VERIFICATION BYPASS — fabricate a *verified* principal from an
/// unsigned, unverified Firebase Emulator token.
///
/// The Firebase Local Emulator Suite issues unsigned tokens by design for local
/// development. This function performs **no** cryptographic verification: it
/// accepts any bearer that parses as a JSON object and treats its contents as a
/// fully verified identity — including any `iss` (issuer), from which the
/// Firestore adapter derives the *verified Firebase project* (#24). It therefore
/// lets the caller **forge** a verified project from attacker-controlled claims.
///
/// It is gated behind `FirebaseConfig::allows_emulator_token_verification_bypass`,
/// and the `nimbus-bin` boot guard refuses that flag on any non-loopback bind, so
/// this fabricator is structurally unreachable on a network-reachable listener.
/// Signed production bearer paths must use an `ApplicationAuthVerifier` instead;
/// this must never run on a public bind.
pub fn firebase_emulator_verification_bypass_principal_from_bearer(
    token: &str,
) -> Option<PrincipalContext> {
    let Value::Object(mut claims) = serde_json::from_str::<Value>(token).ok()? else {
        return None;
    };
    normalize_subject_aliases(&mut claims);
    Some(PrincipalContext {
        authenticated: true,
        // Dev-mode bypass: the opted-in emulator token *is* the trusted identity,
        // so its (unverified) claims populate `verified_claims` too. This is what
        // fabricates the verified project the #24 binding reads — sound only
        // because the boot guard forbids enabling the bypass on a non-loopback
        // bind.
        verified_claims: claims.clone(),
        claims,
    })
}

pub fn normalize_subject_aliases(claims: &mut Map<String, Value>) {
    let canonical = claims
        .get("subject")
        .cloned()
        .or_else(|| claims.get("sub").cloned())
        .or_else(|| claims.get("user_id").cloned())
        .or_else(|| claims.get("uid").cloned());
    let Some(subject) = canonical else {
        return;
    };
    claims
        .entry("subject".to_string())
        .or_insert_with(|| subject.clone());
    claims.entry("sub".to_string()).or_insert(subject);
}

pub fn normalize_principal_context(auth: Option<&InvocationAuth>) -> PrincipalContext {
    let Some(auth) = auth else {
        return PrincipalContext::anonymous();
    };

    PrincipalContext {
        authenticated: auth.identity.is_some() || auth.verified_identity.is_some(),
        claims: claims_map(auth.identity.as_ref()),
        verified_claims: claims_map(auth.verified_identity.as_ref()),
    }
}

fn claims_map<T>(value: Option<&T>) -> Map<String, Value>
where
    T: Serialize,
{
    value
        .and_then(|value| serde_json::to_value(value).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

pub fn parse_bearer_value(value: &str) -> Result<&str, ApplicationAuthError> {
    let (scheme, token) = value.split_once(' ').ok_or_else(|| {
        ApplicationAuthError::unauthorized("authorization header must use the Bearer scheme")
    })?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(ApplicationAuthError::unauthorized(
            "authorization header must use the Bearer scheme",
        ));
    }
    let token = token.trim();
    if token.is_empty() {
        return Err(ApplicationAuthError::unauthorized(
            "authorization header is missing a token",
        ));
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;
    use nimbus_core::{RuntimeUserIdentity, VerifiedUserIdentity, VerifiedUserIdentityKind};
    use serde_json::json;

    use super::*;

    struct StaticVerifier;

    impl ApplicationAuthVerifier for StaticVerifier {
        fn verify_bearer_token<'a>(
            &'a self,
            token: &'a str,
        ) -> BoxFuture<'a, Result<InvocationAuth, ApplicationAuthError>> {
            async move {
                if token != "ok" {
                    return Err(ApplicationAuthError::unauthorized("bad token"));
                }
                Ok(test_auth())
            }
            .boxed()
        }
    }

    #[test]
    fn verifier_success_returns_invocation_auth() {
        let auth = futures::executor::block_on(StaticVerifier.verify_bearer_token("ok"))
            .expect("static verifier should accept ok token");

        assert_eq!(auth.token_identifier(), Some("issuer|subject"));
    }

    #[test]
    fn malformed_bearer_values_are_classified_as_unauthorized() {
        let error =
            parse_bearer_value("Basic abc").expect_err("wrong scheme should be unauthorized");

        assert!(matches!(error, ApplicationAuthError::Unauthorized(_)));
        assert!(error.to_string().contains("Bearer"));
    }

    #[test]
    fn missing_verifier_can_be_classified_without_server_state() {
        let error = ApplicationAuthError::unauthorized(
            "no application auth providers are configured for the active deployment",
        );

        assert!(matches!(error, ApplicationAuthError::Unauthorized(_)));
        assert!(error.message().contains("no application auth providers"));
    }

    #[test]
    fn principal_normalization_uses_runtime_and_verified_claims() {
        let auth = test_auth();
        let principal = normalize_principal_context(Some(&auth));

        assert!(principal.authenticated);
        assert_eq!(principal.claims["subject"], json!("subject"));
        assert_eq!(principal.verified_claims["kind"], json!("oidc"));
    }

    #[test]
    fn emulator_verification_bypass_normalizes_aliases_and_fabricates_verified_claims() {
        let principal = firebase_emulator_verification_bypass_principal_from_bearer(
            r#"{"uid":"user-123","iss":"https://securetoken.google.com/demo"}"#,
        )
        .expect("emulator bearer should parse");

        assert!(principal.authenticated);
        assert_eq!(principal.claims["subject"], json!("user-123"));
        assert_eq!(principal.claims["sub"], json!("user-123"));
        // The dev-mode bypass fabricates verified_claims from the unverified
        // token, so the issuer is treated as verified (this is the forge).
        assert_eq!(
            principal.verified_claims["iss"],
            json!("https://securetoken.google.com/demo")
        );
        assert_eq!(principal.verified_claims["subject"], json!("user-123"));
    }

    fn test_auth() -> InvocationAuth {
        InvocationAuth::with_identities(
            RuntimeUserIdentity {
                token_identifier: "issuer|subject".to_string(),
                subject: "subject".to_string(),
                issuer: "issuer".to_string(),
                name: Some("Test User".to_string()),
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
                custom_claims: Map::new(),
            },
            VerifiedUserIdentity {
                kind: VerifiedUserIdentityKind::Oidc,
                token_identifier: "issuer|subject".to_string(),
                subject: "subject".to_string(),
                issuer: "issuer".to_string(),
                name: Some("Test User".to_string()),
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
                custom_claims: Map::new(),
            },
            false,
        )
    }
}
