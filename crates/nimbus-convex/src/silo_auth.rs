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
        let verifier = self.verifier_for_silo(silo).ok_or_else(|| {
            ApplicationAuthError::unauthorized(format!(
                "no Convex auth providers are configured for silo `{silo}`"
            ))
        })?;
        verifier.verify_bearer_token(token).await
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
}
