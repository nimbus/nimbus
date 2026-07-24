//! Convex-faithful per-silo authentication.
//!
//! A Convex application token is meaningful only inside the deployment whose
//! auth configuration verifies it. Nimbus hosts multiple data silos on one
//! process, so this registry makes that deployment boundary explicit: the URL
//! selects a silo, the silo selects its verifier, and only then can the bearer
//! become an authenticated principal. There is no global verification step
//! whose result can later be paired with a caller-selected silo.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use nimbus_auth::{ApplicationAuthError, ApplicationAuthVerifier};
use nimbus_core::{InvocationAuth, TenantId};

/// Snapshot-bound authority for one Convex silo.
///
/// Long-lived transports retain this value so every authentication attempt
/// uses the verifier selected from the same deployment snapshot that admitted
/// the transport. Replacing the active deployment cannot silently switch the
/// trust domain of an established connection.
#[derive(Clone)]
pub struct ConvexSiloAuthAuthority {
    silo: TenantId,
    verifier: Option<Arc<dyn ApplicationAuthVerifier>>,
}

impl ConvexSiloAuthAuthority {
    /// Verify with the verifier captured for this authority's silo. Missing
    /// provisioning fails closed instead of falling back to another verifier.
    pub async fn verify_bearer_token(
        &self,
        token: &str,
    ) -> Result<InvocationAuth, ApplicationAuthError> {
        let verifier = self.verifier.as_ref().ok_or_else(|| {
            ApplicationAuthError::unauthorized(format!(
                "no Convex auth providers are configured for silo `{}`",
                self.silo
            ))
        })?;
        verifier.verify_bearer_token(token).await
    }
}

impl fmt::Debug for ConvexSiloAuthAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConvexSiloAuthAuthority")
            .field("silo", &self.silo)
            .field("provisioned", &self.verifier.is_some())
            .finish()
    }
}

/// Trusted, deployment-owned mapping from a Convex silo to that silo's
/// application-auth verifier.
#[derive(Clone, Default)]
pub struct ConvexSiloAuthRegistry {
    verifiers: BTreeMap<String, Arc<dyn ApplicationAuthVerifier>>,
}

impl ConvexSiloAuthRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind or replace a silo's verifier. This is an operator/deployment action,
    /// never request input.
    #[must_use]
    pub fn bind(mut self, silo: &TenantId, verifier: Arc<dyn ApplicationAuthVerifier>) -> Self {
        self.verifiers.insert(silo.as_str().to_owned(), verifier);
        self
    }

    #[must_use]
    pub fn verifier_for_silo(&self, silo: &TenantId) -> Option<Arc<dyn ApplicationAuthVerifier>> {
        self.verifiers.get(silo.as_str()).cloned()
    }

    /// Select a snapshot-bound authentication authority for a silo.
    #[must_use]
    pub fn authority_for_silo(&self, silo: &TenantId) -> ConvexSiloAuthAuthority {
        ConvexSiloAuthAuthority {
            silo: silo.clone(),
            verifier: self.verifier_for_silo(silo),
        }
    }

    #[must_use]
    pub fn contains_silo(&self, silo: &TenantId) -> bool {
        self.verifiers.contains_key(silo.as_str())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.verifiers.is_empty()
    }

    /// Verify with the verifier selected by `silo`. An unprovisioned silo is an
    /// authentication failure, not a fallback to any deployment-wide verifier.
    pub async fn verify_bearer_token(
        &self,
        silo: &TenantId,
        token: &str,
    ) -> Result<InvocationAuth, ApplicationAuthError> {
        self.authority_for_silo(silo)
            .verify_bearer_token(token)
            .await
    }
}

impl fmt::Debug for ConvexSiloAuthRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConvexSiloAuthRegistry")
            .field("silos", &self.verifiers.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use futures::future::BoxFuture;

    use super::*;

    struct NamedVerifier(&'static str);

    impl ApplicationAuthVerifier for NamedVerifier {
        fn verify_bearer_token<'a>(
            &'a self,
            token: &'a str,
        ) -> BoxFuture<'a, Result<InvocationAuth, ApplicationAuthError>> {
            Box::pin(async move {
                Err(ApplicationAuthError::unauthorized(format!(
                    "{}:{token}",
                    self.0
                )))
            })
        }
    }

    fn silo(value: &str) -> TenantId {
        TenantId::new(value).expect("silo id should parse")
    }

    #[tokio::test]
    async fn verifier_selection_is_scoped_to_the_requested_silo() {
        let registry = ConvexSiloAuthRegistry::new()
            .bind(&silo("alpha"), Arc::new(NamedVerifier("alpha-verifier")))
            .bind(&silo("beta"), Arc::new(NamedVerifier("beta-verifier")));

        let alpha_error = registry
            .verify_bearer_token(&silo("alpha"), "same-token")
            .await
            .expect_err("fixture verifier should refuse");
        let beta_error = registry
            .verify_bearer_token(&silo("beta"), "same-token")
            .await
            .expect_err("fixture verifier should refuse");

        assert_eq!(alpha_error.message(), "alpha-verifier:same-token");
        assert_eq!(beta_error.message(), "beta-verifier:same-token");
    }

    #[tokio::test]
    async fn unprovisioned_silo_never_falls_back_to_another_verifier() {
        let registry = ConvexSiloAuthRegistry::new()
            .bind(&silo("alpha"), Arc::new(NamedVerifier("alpha-verifier")));

        let error = registry
            .verify_bearer_token(&silo("unprovisioned"), "token")
            .await
            .expect_err("unprovisioned silo must fail closed");

        assert!(error.message().contains("unprovisioned"));
        assert!(!error.message().contains("alpha-verifier"));
    }

    #[tokio::test]
    async fn selected_authority_keeps_its_deployment_snapshot() {
        let alpha = silo("alpha");
        let original_registry = ConvexSiloAuthRegistry::new()
            .bind(&alpha, Arc::new(NamedVerifier("original-verifier")));
        let authority = original_registry.authority_for_silo(&alpha);
        let replacement_registry =
            original_registry.bind(&alpha, Arc::new(NamedVerifier("replacement-verifier")));

        let authority_error = authority
            .verify_bearer_token("same-token")
            .await
            .expect_err("fixture authority should refuse");
        let replacement_error = replacement_registry
            .verify_bearer_token(&alpha, "same-token")
            .await
            .expect_err("fixture verifier should refuse");

        assert_eq!(authority_error.message(), "original-verifier:same-token");
        assert_eq!(
            replacement_error.message(),
            "replacement-verifier:same-token"
        );
    }

    #[tokio::test]
    async fn unprovisioned_authority_remains_fail_closed() {
        let authority = ConvexSiloAuthRegistry::new().authority_for_silo(&silo("unprovisioned"));

        let error = authority
            .verify_bearer_token("token")
            .await
            .expect_err("unprovisioned authority must fail closed");

        assert_eq!(
            error.message(),
            "no Convex auth providers are configured for silo `unprovisioned`"
        );
    }
}
