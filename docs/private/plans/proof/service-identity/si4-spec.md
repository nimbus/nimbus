# SI4 Spec — SPIFFE/SVID integration path (JWT-SVID, registration shape, rotation)

Design authority: `service-identity-provider-auth-plan.md` SI4 row + the
2026-07-07 SPIFFE inventory. Crate scope: `crates/nimbus-workload-identity`
plus ONE surgical hardening in `crates/nimbus-tenant` (trust-domain
charset). SI4 is path definition + rendering + rotation proof. It does
NOT build: SPIRE/Workload-API adapters or any serving socket (SI5),
runtime/sandbox propagation (SI6), X.509-SVID issuance (deferred with the
provider adapters; `rcgen` stays untouched in nimbus-proxy — the MITM CA
fence holds).

## Changes

### 1. Trust-domain hardening (both validators, breaking)

SPIFFE trust domains are lowercase DNS-authority-like: only
`a-z`, `0-9`, `.`, `-`, `_`. Both validators currently accept uppercase
and arbitrary non-slash charsets:
- `nimbus-tenant/src/identity.rs:286-302 validate_spiffe_trust_domain`
- `nimbus-workload-identity/src/trust.rs:67-83 validate_trust_domain`

Tighten BOTH to the SPIFFE charset (reject, don't normalize — a config
with `Example.COM` should fail loudly, not be silently lowercased). Keep
the two copies mirror-identical including comments referencing each
other. Add rejection tests for uppercase, `@`, `:`, non-ASCII, and keep
all existing accept/reject cases green.

### 2. JWT-SVID rendering (new mode in `jwt.rs`)

- `CredentialFormat { OidcJwt, JwtSvid }` — a construction-time choice on
  `LocalDevIssuer` (`LocalDevIssuer::new` keeps current behavior =
  OidcJwt; add `with_format` or a second constructor — your call, one
  obvious way only). `mint()` returns
  `CredentialKind::{OidcJwt|SpiffeSvid}` accordingly.
- JWT-SVID payload differences vs the SI3 payload (all else identical,
  same JOSE/EdDSA/kid path through `IdentitySigner`):
  - `sub` = the SPIFFE ID URI. Derive it from the stable subject by the
    documented transform: `nimbus-workload:v1/<suffix>` →
    `spiffe://<trust_domain>/nimbus/workload/v1/<suffix>`. Add a
    cross-crate test (nimbus-tenant is already a dependency) asserting
    the derived URI EQUALS `WorkloadIdentity::spiffe_id(trust_domain)`
    for a real decision — the two renderings must never drift.
  - `aud` = a JSON ARRAY of audiences (single-element from the mint
    request's audience today; type it as `Vec<&str>` so SI5 can widen).
  - Keep `nimbus_*` claims, `exp`/`iat` seconds, jti, placement
    omission — unchanged.

### 3. Registration-entry shape (new `src/registration.rs`, zero-I/O)

```rust
pub struct SpiffeRegistrationEntry {
    /* spiffe_id: String            — from the workload's SPIFFE URI (same transform as §2)
       parent_id: String            — the node's SPIFFE URI: spiffe://<td>/nimbus/node/<nk_fingerprint>
       selectors: Vec<SpiffeSelector> */
}
pub struct SpiffeSelector { /* key: &'static str, value: String  e.g. ("nimbus:tenant", ...) */ }
```

- Built by `SpiffeRegistrationEntry::for_workload(trust: &IdentityTrustConfig,
  identity: &nimbus_tenant::WorkloadIdentity, node: &NodeIdentityRecord)
  -> Result<Self, TrustConfigError>`:
  - consults `trust.admit_source(node.source())` (fail-closed — a
    Production config cannot build an entry from a LocalDev node);
  - selectors = the 8 subject dimensions
    (`nimbus:tenant`, `nimbus:deployment`, `nimbus:surface`,
    `nimbus:kind`, `nimbus:name`, `nimbus:runtime-tier`,
    `nimbus:runtime-backend`, `nimbus:sandbox-backend`) plus, when
    present, placement selectors (`nimbus:node`, `nimbus:machine`,
    `nimbus:sandbox`) — invocation_id is per-invocation cardinality and
    is EXCLUDED from selectors (document why).
  - node attestation input = the node record's key-derived id; the
    `IdentitySourceKind` gates it exactly like issuance.
- Serialize (serde) for evidence/registration export; no I/O, no SPIRE
  client.

### 4. SVID rotation proof (tests, no new prod machinery)

Credential-level rotation rides signer rotation + re-mint:
1. Mint a JWT-SVID; capture kid/jti.
2. `FileBackedIdentitySigner::rotate()`.
3. Old token's signature no longer verifies via the signer
   (`StaleKey` by key-id) while ring-verification against the OLD public
   key still succeeds (documents that rotation is issuer-side denial,
   not retroactive cryptographic revocation — providers rely on exp).
4. Re-mint: new token verifies, new kid == new fingerprint, same
   `sub`/spiffe URI, fresh jti/exp.
Assert each step's specific outcome.

## Required tests (beyond §4)

1. Trust-domain hardening rejections (both crates) + existing cases green.
2. Derived SPIFFE URI == `WorkloadIdentity::spiffe_id` for a real
   admitted decision (cross-crate equality oracle).
3. JWT-SVID payload: `sub` is the spiffe URI, `aud` is an ARRAY,
   independent ring verification passes, tamper fails.
4. OidcJwt format behavior unchanged (existing tests green untouched).
5. Registration entry: full selector set for a placement-bearing
   decision; no invocation selector; Production trust + LocalDev node
   record ⇒ TrustConfigError; serialized JSON carries no key material.
6. `CredentialKind::SpiffeSvid` is what a JwtSvid-format issuer returns.

## Verification gates (worktree root, in order)

```
cargo fmt --all --check
cargo clippy -p nimbus-workload-identity -p nimbus-tenant --all-targets -- -D warnings
cargo test -p nimbus-workload-identity
cargo test -p nimbus-tenant
cargo test -p nimbus-crypto
cargo check -p nimbus-server
```

Report real per-suite counts. nimbus-tenant's suite is substantial —
all of it must stay green (the validator tightening is the only tenant
change allowed).

## As built (PR #133, squash-merged `352094a4d`, 2026-07-07)

Landed to contract; no SPIRE/Workload-API dependencies, no serving
endpoints, no X.509 (SI5+); the proxy MITM-CA fence holds.

- Trust-domain hardening in BOTH mirror validators
  (`nimbus-workload-identity` + `nimbus-tenant`): SPIFFE charset only,
  loud rejection, copies kept identical.
- JWT-SVID format on `LocalDevIssuer` via construction-time
  `CredentialFormat`: `sub` = the `spiffe://` URI through a documented
  transform, pinned byte-for-byte against
  `WorkloadIdentity::spiffe_id` by a cross-crate equality test; `aud`
  as a JSON array; `CredentialKind::SpiffeSvid`.
- `SpiffeRegistrationEntry`: `admit_source`-gated (Production cannot
  build from LocalDev nodes), 8 subject selectors + placement,
  invocation excluded, node-fingerprint parent id, zero I/O.
- SVID rotation proof: stale-key denial by key-id post-rotation while
  old-pubkey ring verification still passes — documented as issuer-side
  denial (providers rely on `exp`) — then re-mint with the new `kid`.

Evidence: 205 tests (workload-identity 29 / tenant 93 / crypto 83);
fmt/clippy clean; `cargo check -p nimbus-server`; branch rebased onto
post-GR2 main with first-party `cargo clean -p` first
(contamination-proofed); autoreview (Codex, adversarial: transform
drift, validator divergence, selector injection, parent-id collisions)
clean first pass. CI green; single flaky engine-subscription lane
attributed 5/5 green locally before merge.
