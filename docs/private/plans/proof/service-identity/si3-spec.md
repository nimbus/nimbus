# SI3 Spec — short-lived EdDSA JWT minting (LocalDevIssuer)

Design authority: `service-identity-provider-auth-plan.md` SI3 row +
the 2026-07-07 minting inventory. Crate scope:
`crates/nimbus-workload-identity` only (nimbus-crypto is consumed, not
modified). SI3 mints; it does NOT serve JWKS/discovery (no endpoint —
SI4 owns SPIFFE rendering, SI5 owns provider adapters) and it MUST NOT
touch the convex external-IdP verifier (`nimbus-convex/src/auth/`).

## Deliberate dependency decision (record verbatim in code docs)

The SI plan prefers `jsonwebtoken` for minting; SI3 instead assembles
the JOSE compact serialization directly and signs THROUGH the
`IdentitySigner` seam, because handing private-key material to
jsonwebtoken's `EncodingKey` would bypass the seam SI2 built (file
locking, rotation, stale-key denial, and the future FIPS/HS1 signer
swap). This is ~30 lines of assembly, not a re-implementation of JWT
parsing/verification — the convex-verifier warning is about
verification infrastructure, which SI3 does not build. Verification-side
code shipped later (SI5) still prefers jsonwebtoken/openidconnect.
Independent verification in tests uses ring's `UnparsedPublicKey`
(already in-tree; the same primitive the JWS spec requires).

## Changes

### 1. `CredentialClaims` gains issued-at (additive)

- New private field `iat_epoch_ms: u64` + getter `iat_epoch_ms()`,
  populated in `authorize_claims` from `MintParams.issued_at_epoch_ms`.
- Serialized in the internal/audit JSON as `nimbus_issued_at_ms`
  (keep `exp` ms semantics unchanged there). Update the SI0 claim-key
  serialization test to include the new key — extend, don't weaken.

### 2. JWT payload/header assembly (new `src/jwt.rs`)

- `JoseHeader`: `{"alg":"EdDSA","typ":"JWT","kid":<fingerprint>}` —
  kid = `IdentityPublicKey::fingerprint()` (matches
  `IdentitySignature.key_id`).
- JWT payload built FROM `CredentialClaims` + issuer string:
  - `iss`: the `IdentityTrustConfig.trust_domain()` string verbatim
    (documented as provisional until discovery work assigns URL form).
  - `sub`, `aud`, `jti`: as-is.
  - `exp`, `iat`: **epoch SECONDS** (`ms / 1000`, floor) — RFC 7519
    NumericDate. The internal ms shapes stay internal.
  - `nbf`: omit (not required by the SI3 verification column).
  - All `nimbus_*` claims included; placement claims with value `None`
    are OMITTED from the JWT payload (providers reject nulls), while
    the audit serialization keeps explicit nulls — document the
    divergence at the builder.
- base64url no-pad via the workspace `base64` crate
  (`URL_SAFE_NO_PAD`). Signing input = `b64(header)"."b64(payload)`;
  token = signing input + `"."` + `b64(signature_bytes)`.

### 3. `LocalDevIssuer` (first real `IdentityIssuer`, in `issuer.rs` or a sibling)

```rust
pub struct LocalDevIssuer { /* trust: IdentityTrustConfig,
                               source: IdentitySourceKind (from the node record),
                               signer: Arc<dyn IdentitySigner> */ }

impl LocalDevIssuer {
    /// Fail-closed constructor: consults trust.admit_source(source).
    /// A Production config can never admit a LocalDev source, so this
    /// issuer is unconstructible under Production — the HS1 gate holds.
    pub fn new(trust: IdentityTrustConfig, record: &NodeIdentityRecord,
               signer: Arc<dyn IdentitySigner>) -> Result<Self, TrustConfigError>;
}

impl IdentityIssuer for LocalDevIssuer {
    fn mint(&self, claims: &CredentialClaims) -> Result<MintedCredential, IdentityIssueError>;
    // -> MintedCredential::new(CredentialKind::OidcJwt, token)
    // signer errors map to IdentityIssueError::Failed with a
    // tenant-safe message (no key material, no signing-input dump).
}
```

New deps for nimbus-workload-identity: `base64.workspace`,
`serde_json.workspace` (moves from dev-deps to deps). Nothing else.

### 4. First authorize→mint wiring

```rust
pub struct CredentialMint {
    pub outcome: Result<MintedCredential, CredentialMintError>,
    pub audit: IdentityAuditEvent,
}
pub fn mint_credential(
    policy: &ProviderAuthPolicy,
    request: &IdentityMintRequest<'_>,
    params: &MintParams,
    issuer: &dyn IdentityIssuer,
) -> CredentialMint;
```

- Composes `authorize_mint` then `issuer.mint`. Authorization denials
  keep their existing audit event untouched. An authorization SUCCESS
  followed by an issuance FAILURE produces an audit event with
  `Denied { reason: "issuance failed: ..." }` (replace the Minted
  outcome — the credential never existed; keep exp/jti fields from the
  authorized claims so the event stays correlatable). The audit event
  remains unskippable.
- `CredentialMintError`: `Authorization(IdentityMintError)` |
  `Issuance(IdentityIssueError)`.

## Required tests

1. End-to-end mint: admitted decision + grant + policy ⇒ 3-segment
   token; header decodes to `{alg: EdDSA, typ: JWT, kid: <fingerprint>}`;
   payload carries sub == stable subject, aud, exp/iat in SECONDS with
   exp - iat == effective TTL in seconds, jti, nimbus_decision_id, and
   omits None placement claims.
2. INDEPENDENT signature verification: ring
   `UnparsedPublicKey::new(&ED25519, signer.public_key().as_bytes())`
   verifies the signing input against the decoded third segment;
   a tampered payload fails.
3. Wrong audience ⇒ `CredentialMintError::Authorization(AudienceNotAllowed)`
   and NO token; audit says Denied. (Wrong-provider denial = audience
   mismatch at this layer; provider adapters are SI5.)
4. `LocalDevIssuer::new` with a Production trust config ⇒
   `TrustConfigError::SourceNotAdmitted` (the HS1 gate).
5. Issuance failure (inject a failing `IdentitySigner` stub) ⇒
   `CredentialMintError::Issuance`, audit Denied with the
   issuance-failed reason, no secret material in the audit JSON.
6. `MintedCredential` Debug still redacts; the token round-trips via
   `secret()`.
7. Existing 20 workload-identity tests + 82 crypto tests stay green
   (claim-key test extended for `nimbus_issued_at_ms`).

## Verification gates (worktree root, in order)

```
cargo fmt --all --check
cargo clippy -p nimbus-workload-identity --all-targets -- -D warnings
cargo test -p nimbus-workload-identity
cargo test -p nimbus-crypto
cargo check -p nimbus-server
```

Report real per-suite counts.
