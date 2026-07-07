# SI0 Spec — `nimbus-workload-identity` crate

Design authority: `docs/private/plans/service-identity-provider-auth-plan.md`
(SI0 row + Identity Contract + Goal) and
`docs/private/plans/architecture-review-2026-07-plan.md` AD2 (refinement
paragraph). This file is the implementation contract for the SI0 slice.

## Hard constraints (violating any of these fails review)

1. New crate `crates/nimbus-workload-identity`. Workspace dependencies:
   `nimbus-core`, `nimbus-tenant` ONLY (plus external: `serde` with derive,
   `thiserror`, `zeroize` — all already workspace deps). No provider SDKs,
   no `jsonwebtoken`/`openidconnect` yet (that is SI3), no tokio, no I/O of
   any kind. The crate is pure types + validation, fully deterministic.
2. The `WorkloadIdentity` projection does NOT move. It stays in
   `nimbus-tenant` (`from_decision`-only construction is a security
   property). This crate consumes it.
3. Admission anchoring is type-level: the mint request type must be
   constructible ONLY from a `&TenantIsolationDecision`. No public
   constructor from raw strings/parts. Prove it with a `compile_fail`
   doctest.
4. Deny-by-default everywhere: empty policy denies; unmatched subject
   denies; unmatched audience denies; zero/negative TTL denies.
5. Provider policy subjects are STABLE subjects only — a policy rule whose
   subject contains placement segments (`/node/`, `/machine/`,
   `/sandbox/`, `/invocation/`) or the audit-projection prefix
   (`nimbus-workload-audit:`) must be rejected at policy construction.
   (SI plan: "provider policy subjects do not include invocation IDs".)
6. No secret material in Debug/Serialize of anything except the credential
   secret itself, which is `zeroize::Zeroizing` and has a redacting Debug.
   Audit events carry no secret field by construction.
7. Every authorization produces an audit event — allowed AND denied. The
   API shape makes skipping the audit event impossible.
8. Follow repo conventions: `[lints] workspace = true` in Cargo.toml
   (copy the pattern from `crates/nimbus-egress/Cargo.toml` — it is the
   closest sibling: a small pure policy crate on nimbus-core). Register
   the crate in the root `Cargo.toml` workspace members list
   (alphabetical position). Concept-owned module names, no
   `helpers.rs`/`util.rs`.

## Crate layout

```
crates/nimbus-workload-identity/
  Cargo.toml
  src/
    lib.rs        # crate docs + re-exports only
    policy.rs     # ProviderAuthPolicy, ProviderAuthRule, SubjectMatch, PolicyValidationError
    mint.rs       # IdentityMintRequest, MintParams, authorize_mint, MintAuthorization, IdentityMintError
    claims.rs     # CredentialClaims (serde field names below are wire contract)
    issuer.rs     # IdentityIssuer trait, MintedCredential, DenyAllIssuer, IdentityIssueError
    audit.rs      # IdentityAuditEvent, IdentityAuditOutcome
```

## API contract (signatures are normative; bodies are yours)

### policy.rs

```rust
pub struct ProviderAuthPolicy { /* private: Vec<ProviderAuthRule> */ }

#[derive(Debug, Clone)]
pub struct ProviderAuthRule {
    // private fields + builder or ctor:
    // subject: SubjectMatch,
    // audiences: Vec<String>,     // exact-match audiences; empty vec = invalid rule (reject at construction)
    // max_ttl: std::time::Duration, // zero = invalid rule
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectMatch {
    Exact(String),
    // Prefix must end on a segment boundary: either ends with '/' or the
    // match is applied segment-wise. "nimbus-workload:v1/tenant/acme" must
    // NOT match subject ".../tenant/acme-corp/...". Enforce and test.
    SegmentPrefix(String),
}

impl ProviderAuthPolicy {
    /// Fail-closed constructor. Rejects (PolicyValidationError):
    /// - any rule subject not starting with "nimbus-workload:v1/" (for Exact)
    ///   or "nimbus-workload:v1" (for SegmentPrefix)
    /// - any rule subject containing a placement segment: "/node/",
    ///   "/machine/", "/sandbox/", "/invocation/"
    /// - any rule subject starting with "nimbus-workload-audit:"
    /// - empty audiences, an empty audience string, zero max_ttl
    pub fn try_new(rules: Vec<ProviderAuthRule>) -> Result<Self, PolicyValidationError>;

    /// Empty policy — denies every mint. Useful as the fail-closed default.
    pub fn deny_all() -> Self;
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PolicyValidationError { /* variants per the rejects above */ }
```

### mint.rs

```rust
/// Admission-anchored mint request. NO public field access, NO public
/// constructor other than `for_decision`.
pub struct IdentityMintRequest<'a> { /* identity: WorkloadIdentity (owned, via decision.workload_identity()),
                                        decision_id: &'a TenantIsolationDecisionId,
                                        audience: String,
                                        requested_ttl: std::time::Duration */ }

impl<'a> IdentityMintRequest<'a> {
    pub fn for_decision(
        decision: &'a nimbus_tenant::TenantIsolationDecision,
        audience: impl Into<String>,
        requested_ttl: std::time::Duration,
    ) -> Self;
}

/// Deterministic inputs the caller (future issuer plumbing) supplies —
/// keeps this crate I/O-free and tests deterministic.
pub struct MintParams {
    pub issued_at_epoch_ms: u64,
    /// jti — credential instance id, generated by the caller (SI2+ owns
    /// generation); must be non-empty.
    pub credential_instance_id: String,
}

/// The ONLY authorization entry point. Returns the outcome AND the audit
/// event together so a caller cannot obtain a decision without also
/// holding the event to record.
pub fn authorize_mint(
    policy: &ProviderAuthPolicy,
    request: &IdentityMintRequest<'_>,
    params: &MintParams,
) -> MintAuthorization;

pub struct MintAuthorization {
    pub outcome: Result<CredentialClaims, IdentityMintError>,
    pub audit: IdentityAuditEvent,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdentityMintError {
    // NoMatchingSubjectRule, AudienceNotAllowed, TtlInvalid (zero requested),
    // InvalidParams (empty jti) — reason strings must be tenant-safe (no
    // secret material; subject/audience strings are fine, they are not secret)
}
```

TTL semantics: effective_ttl = min(requested_ttl, rule.max_ttl); zero
requested_ttl denies. `exp_epoch_ms = issued_at_epoch_ms + effective_ttl`
(saturating).

### claims.rs

```rust
/// Wire contract — serde rename to EXACTLY these claim names (SI plan
/// Identity Contract): sub, aud, exp, jti, nimbus_decision_id,
/// nimbus_workload_subject, nimbus_workload_audit_projection,
/// nimbus_node_id, nimbus_machine_id, nimbus_sandbox_id,
/// nimbus_invocation_id. exp serializes as epoch MILLISECONDS (u64).
/// Placement claims are Option and serialize as null when absent
/// (do NOT skip_serializing_if — the SI audit correlation wants explicit
/// nulls).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CredentialClaims { /* fields per above; sub == nimbus_workload_subject == identity.subject() */ }
```

Accessors for every field (no pub fields).

### issuer.rs

```rust
/// SI3+ implements real minting. SI0 defines the seam.
pub trait IdentityIssuer: Send + Sync {
    fn mint(&self, claims: &CredentialClaims) -> Result<MintedCredential, IdentityIssueError>;
}

/// Fail-closed default (mirrors DenyAllEgressGateway).
pub struct DenyAllIssuer;
impl IdentityIssuer for DenyAllIssuer { /* always Err(IdentityIssueError::IssuanceNotConfigured) */ }

pub struct MintedCredential { /* kind: CredentialKind, secret: zeroize::Zeroizing<String> */ }
// Debug prints kind + "[REDACTED]"; NO Serialize impl; provide
// `pub fn secret(&self) -> &str` and `pub fn kind(&self) -> CredentialKind`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind { OidcJwt, SpiffeSvid, MtlsClientCert, ServiceAccountToken }

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdentityIssueError { /* IssuanceNotConfigured + a provider-opaque Failed(String) with tenant-safe message */ }
```

### audit.rs

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct IdentityAuditEvent {
    // tenant_id: String, decision_id: String, workload_subject: String,
    // workload_audit_projection: String, audience: String,
    // outcome: IdentityAuditOutcome,
    // exp_epoch_ms: Option<u64>, credential_instance_id: Option<String>
    // — NO field can carry secret material; there is no place to put a token.
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum IdentityAuditOutcome { Minted, Denied { reason: String } }
```

## Required tests (all must exist and assert real behavior)

Build real decisions via the same path production uses. Look at
`crates/nimbus-tenant/src/context.rs` (`admit_decision`) and existing
nimbus-tenant tests for how to construct a `TenantIsolationPolicyInput` /
get a `TenantIsolationDecision` in tests; reuse that pattern via a small
test helper in the new crate's tests (dev-dep on nimbus-tenant is already
a normal dep). If constructing a full decision requires substantial
fixture plumbing, mirror the minimal pattern used by nimbus-tenant's own
unit tests — do NOT add new public constructors to production types to
make testing easier.

1. `compile_fail` doctest: `IdentityMintRequest { .. }` literal and/or a
   `new(...)` call does not compile (no such public surface).
2. Forged/foreign identity denied: decision admitted for tenant `alpha`,
   policy rule Exact-subject for tenant `beta` → `NoMatchingSubjectRule`,
   audit event `Denied` carrying tenant `alpha`'s subject.
3. Policy construction rejects: subject containing `/invocation/`;
   subject containing `/node/`; audit-projection-prefixed subject;
   subject not starting with `nimbus-workload:v1`; empty audiences;
   zero max_ttl. One assert per case on the specific
   `PolicyValidationError` variant.
4. SegmentPrefix boundary: prefix for `/tenant/acme` matches
   `/tenant/acme/...` but NOT `/tenant/acme-corp/...`.
5. Audience: allowed audience mints; unlisted audience denies.
6. TTL: requested > max clamps to max (check exp math); requested zero
   denies; exp saturates on overflow.
7. Claims contract: serialize a minted `CredentialClaims` to JSON and
   assert the EXACT key set and key names, `sub == nimbus_workload_subject`,
   exp value, explicit nulls for absent placement claims.
8. Deny-by-default: `ProviderAuthPolicy::deny_all()` and empty-rules
   policy deny a valid admitted identity.
9. Audit completeness: both the mint path and every deny path return an
   audit event whose serialized JSON contains no key named anything like
   `secret`/`token`/`credential` value — and `MintedCredential`'s Debug
   output contains `[REDACTED]` and not the secret bytes.
10. `DenyAllIssuer` returns `IssuanceNotConfigured`.

## Workspace integration

- Add `"crates/nimbus-workload-identity"` to root `Cargo.toml` members
  (alphabetical: after `nimbus-tenant`, before `nimbus-runtime`? The list
  is not strictly sorted — match the existing grouping style; place it
  next to `nimbus-tenant`).
- Add one row to `ARCHITECTURE.md`'s crate table (alphabetical position):
  `nimbus-workload-identity` — "Workload-identity issuance seam:
  provider-auth policy, admission-anchored mint authorization,
  credential claim set, and mint/deny audit schema (SI0). Projection
  stays in `nimbus-tenant`."
- Do NOT touch nimbus-tenant, nimbus-core, or any other crate's source.
  If something in nimbus-tenant seems missing (e.g. a getter), STOP and
  report instead of adding it.

## Verification gates (run from the worktree root, in this order)

```
cargo fmt --all --check
cargo clippy -p nimbus-workload-identity --all-targets -- -D warnings
cargo test -p nimbus-workload-identity
cargo check -p nimbus-server   # proves workspace membership didn't break the big consumer
```

Record actual test counts in your report.

## As built (PR #126, squash-merged `c3a92d486`, 2026-07-06)

Landed exactly to this contract; no deviations.

- `ProviderAuthPolicy` / `ProviderAuthRule` fail-closed construction:
  audit-projection subjects, placement segments (`/node/`, `/machine/`,
  `/sandbox/`, `/invocation/`), non-`nimbus-workload:v1` prefixes, empty
  audiences, and zero max-TTL are all rejected at build time.
  `SegmentPrefix` matches only on path boundaries (`/tenant/acme` does
  not match `/tenant/acme-corp`).
- `IdentityMintRequest` is admission-anchored: constructible only from a
  `TenantIsolationDecision`; a `compile_fail` doctest proves there is no
  forged-construction path. `WorkloadIdentity` stayed in `nimbus-tenant`
  (`from_decision`-only).
- `authorize_mint -> MintAuthorization { outcome, audit }` with the
  audit event unskippable on both mint and deny paths; audit events have
  no field that can carry secret material.
- `CredentialClaims` carries the exact Identity Contract claim names;
  `sub == nimbus_workload_subject`; placement claims serialize as
  explicit nulls.
- `IdentityIssuer` seam with `DenyAllIssuer` fail-closed default;
  `MintedCredential` wraps its secret in `Zeroizing<String>` with a
  redacting `Debug`. Zero I/O; deps `nimbus-core` + `nimbus-tenant` only.

Evidence: 9 integration tests + 1 `compile_fail` doctest; fmt/clippy
clean; `cargo check -p nimbus-server`; autoreview (Codex) clean first
pass ("patch is correct (0.89)").
