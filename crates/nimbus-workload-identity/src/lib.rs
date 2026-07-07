//! Workload-identity issuance seam.
//!
//! This crate consumes the admission-owned workload identity projection from
//! `nimbus-tenant`. Mint requests are intentionally constructible only from a
//! `TenantIsolationDecision`. Mint authorization also requires the admitted
//! decision to carry an explicit runtime `identity` grant; provider policies
//! still own subject, audience, and TTL scoping.
//!
//! SI2 adds key-derived node/machine identity records and trust-domain
//! configuration. SI3 adds local-development JWT minting while keeping
//! production issuance fail-closed until HS1 provides membership-bound identity.
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
mod jwt;
mod mint;
mod policy;
mod registration;
mod source;
mod trust;

pub use audit::{IdentityAuditEvent, IdentityAuditOutcome};
pub use claims::CredentialClaims;
pub use issuer::{
    CredentialFormat, CredentialKind, DenyAllIssuer, IdentityIssueError, IdentityIssuer,
    LocalDevIssuer, MintedCredential,
};
pub use mint::{
    CredentialMint, CredentialMintError, IdentityMintError, IdentityMintRequest, MintAuthorization,
    MintParams, authorize_mint, mint_credential,
};
pub use policy::{PolicyValidationError, ProviderAuthPolicy, ProviderAuthRule, SubjectMatch};
pub use registration::{SpiffeRegistrationEntry, SpiffeSelector};
pub use source::{
    IdentitySourceKind, MachineIdentityRecord, MembershipAttestation, NodeIdentityRecord,
};
pub use trust::{IdentityTrustConfig, TrustConfigError, TrustMode};
