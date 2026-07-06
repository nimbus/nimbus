//! Workload-identity issuance seam.
//!
//! This crate consumes the admission-owned workload identity projection from
//! `nimbus-tenant`. Mint requests are intentionally constructible only from a
//! `TenantIsolationDecision`:
//!
//! ```compile_fail
//! use nimbus_workload_identity::IdentityMintRequest;
//!
//! fn forge() {
//!     let _request = IdentityMintRequest {
//!         identity: todo!(),
//!         decision_id: todo!(),
//!         audience: "provider".to_string(),
//!         requested_ttl: std::time::Duration::from_secs(60),
//!     };
//! }
//! ```

mod audit;
mod claims;
mod issuer;
mod mint;
mod policy;

pub use audit::{IdentityAuditEvent, IdentityAuditOutcome};
pub use claims::CredentialClaims;
pub use issuer::{
    CredentialKind, DenyAllIssuer, IdentityIssueError, IdentityIssuer, MintedCredential,
};
pub use mint::{
    IdentityMintError, IdentityMintRequest, MintAuthorization, MintParams, authorize_mint,
};
pub use policy::{PolicyValidationError, ProviderAuthPolicy, ProviderAuthRule, SubjectMatch};
