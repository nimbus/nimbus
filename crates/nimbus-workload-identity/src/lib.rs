//! Workload-identity issuance seam.
//!
//! This crate consumes the admission-owned workload identity projection from
//! `nimbus-tenant`. Mint requests are intentionally constructible only from a
//! `TenantIsolationDecision`. Mint authorization also requires the admitted
//! decision to carry an explicit runtime `identity` grant; provider policies
//! still own subject, audience, and TTL scoping.
//!
//! SI2 adds key-derived node/machine identity records and trust-domain
//! configuration, but credential issuance remains deliberately unimplemented:
//! `DenyAllIssuer` stays the only issuer until SI3.
//!
//! Cluster-membership-sourced identity is not constructible in SI2 — the
//! variant demands a [`MembershipAttestation`], which has no public
//! constructor until HS1 delivers membership-bound identity:
//!
//! ```compile_fail
//! use nimbus_workload_identity::{IdentitySourceKind, MembershipAttestation};
//!
//! let _forged = IdentitySourceKind::ClusterMembership(MembershipAttestation {
//!     _reserved: (),
//! });
//! ```
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
mod source;
mod trust;

pub use audit::{IdentityAuditEvent, IdentityAuditOutcome};
pub use claims::CredentialClaims;
pub use issuer::{
    CredentialKind, DenyAllIssuer, IdentityIssueError, IdentityIssuer, MintedCredential,
};
pub use mint::{
    IdentityMintError, IdentityMintRequest, MintAuthorization, MintParams, authorize_mint,
};
pub use policy::{PolicyValidationError, ProviderAuthPolicy, ProviderAuthRule, SubjectMatch};
pub use source::{
    IdentitySourceKind, MachineIdentityRecord, MembershipAttestation, NodeIdentityRecord,
};
pub use trust::{IdentityTrustConfig, TrustConfigError, TrustMode};
