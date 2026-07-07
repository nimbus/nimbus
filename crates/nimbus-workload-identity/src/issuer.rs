use std::fmt;
use std::sync::Arc;

use nimbus_crypto::IdentitySigner;
use zeroize::Zeroizing;

use crate::jwt;
use crate::{
    CredentialClaims, IdentitySourceKind, IdentityTrustConfig, NodeIdentityRecord, TrustConfigError,
};

/// Workload credential issuer seam.
pub trait IdentityIssuer: Send + Sync {
    fn mint(&self, claims: &CredentialClaims) -> Result<MintedCredential, IdentityIssueError>;
}

pub struct DenyAllIssuer;

impl IdentityIssuer for DenyAllIssuer {
    fn mint(&self, _claims: &CredentialClaims) -> Result<MintedCredential, IdentityIssueError> {
        Err(IdentityIssueError::IssuanceNotConfigured)
    }
}

pub struct LocalDevIssuer {
    trust: IdentityTrustConfig,
    source: IdentitySourceKind,
    signer: Arc<dyn IdentitySigner>,
}

impl LocalDevIssuer {
    /// Fail-closed constructor: consults trust.admit_source(source).
    ///
    /// A Production config can never admit a LocalDev source, so this issuer is
    /// unconstructible under Production — the HS1 gate holds.
    pub fn new(
        trust: IdentityTrustConfig,
        record: &NodeIdentityRecord,
        signer: Arc<dyn IdentitySigner>,
    ) -> Result<Self, TrustConfigError> {
        trust.admit_source(record.source())?;
        Ok(Self {
            trust,
            source: record.source().clone(),
            signer,
        })
    }
}

impl IdentityIssuer for LocalDevIssuer {
    fn mint(&self, claims: &CredentialClaims) -> Result<MintedCredential, IdentityIssueError> {
        debug_assert!(self.trust.admit_source(&self.source).is_ok());
        let token = jwt::mint_oidc_jwt(self.trust.trust_domain(), claims, self.signer.as_ref())
            .map_err(|error| IdentityIssueError::Failed(error.tenant_safe_message()))?;
        Ok(MintedCredential::new(CredentialKind::OidcJwt, token))
    }
}

pub struct MintedCredential {
    kind: CredentialKind,
    secret: Zeroizing<String>,
}

impl MintedCredential {
    pub fn new(kind: CredentialKind, secret: impl Into<String>) -> Self {
        Self {
            kind,
            secret: Zeroizing::new(secret.into()),
        }
    }

    pub fn secret(&self) -> &str {
        &self.secret
    }

    pub fn kind(&self) -> CredentialKind {
        self.kind
    }
}

impl fmt::Debug for MintedCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MintedCredential")
            .field("kind", &self.kind)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    OidcJwt,
    SpiffeSvid,
    MtlsClientCert,
    ServiceAccountToken,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdentityIssueError {
    #[error("identity issuance is not configured")]
    IssuanceNotConfigured,
    #[error("identity issuance failed: {0}")]
    Failed(String),
}
