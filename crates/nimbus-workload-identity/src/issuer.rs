use std::fmt;

use zeroize::Zeroizing;

use crate::CredentialClaims;

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
