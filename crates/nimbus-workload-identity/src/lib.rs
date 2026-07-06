//! Workload-identity issuance seam.
//!
//! This crate consumes the admission-owned workload identity projection from
//! `nimbus-tenant`. Mint requests are intentionally constructible only from a
//! `TenantIsolationDecision`. Mint authorization also requires the admitted
//! decision to carry an explicit runtime `identity` grant; provider policies
//! still own subject, audience, and TTL scoping.
//!
//! ```compile_fail
//! use nimbus_workload_identity::IdentityMintRequest;
//!
//! fn forge() {
//!     let _request = IdentityMintRequest {
//!         identity: todo!(),
//!         decision_id: todo!(),
//!         identity_grants: todo!(),
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
