# SI2 Spec — canonical node/machine identity, trust-domain config, signing keys

Design authority: `docs/private/plans/service-identity-provider-auth-plan.md`
SI2 row + Dependency Posture, AD2 (architecture-review plan), and the
2026-07-06 SI2 inventory. SI2's verification contract: "Key rotation,
stale-key denial, canonical `node_id`/`machine_id`, and local-dev
fallback tests pass without exposing private key material."

## Scope boundaries (violating any fails review)

- SI2 does NOT mint tokens. OIDC/JWT minting is SI3 — do not add
  `jsonwebtoken`/`openidconnect`, do not implement a real
  `IdentityIssuer`. `DenyAllIssuer` remains the only issuer.
- SI2 does NOT own cluster membership or the iroh endpoint keypair (HS1).
  The signing seam must be swappable so HS1/FIPS (`NodeSigner`, FIE2) can
  back it later; nothing here may assume single-node-forever, and nothing
  here may claim production trust: local-dev identity is explicitly
  non-production by construction.
- Do not touch `nimbus-proxy` (`tls_authority` is not workload identity),
  `nimbus-tenant` (projection stays put), `nimbus-node`/`nimbus-workloads`
  (their `NodeIdentity` string newtype is the enforcement-binding rung of
  the ladder — different concept; avoid name collision).
- No secret-value storage (secret plan).

## Deliverable 1 — Ed25519 identity-signing seam in `nimbus-crypto`

New module `crates/nimbus-crypto/src/signing.rs` (+ re-exports in lib.rs).
Uses `ring::signature::Ed25519KeyPair` — `ring` is already a workspace
dependency; add `ring.workspace = true` to nimbus-crypto. No new external
deps.

```rust
/// Signing seam for workload/node identity. Deliberately a SIBLING of
/// LocalKeyProvider (which is symmetric-DEK envelope-shaped) — see
/// provider.rs. The FIPS retrofit plan's NodeSigner (FIE2) and HS1's
/// membership-bound key are future implementors.
pub trait IdentitySigner: Send + Sync {
    fn sign(&self, message: &[u8]) -> SigningResult<IdentitySignature>;
    fn verify(&self, message: &[u8], signature: &IdentitySignature) -> SigningResult<()>;
    fn public_key(&self) -> IdentityPublicKey;
    fn kind(&self) -> IdentitySignerKind;   // diagnostics-safe descriptor, no key material
}
```

- `IdentityPublicKey`: 32-byte Ed25519 public key + `fingerprint()` →
  lowercase hex of SHA-256 of the public key, prefixed `nk_` (node key),
  truncated to 32 hex chars. The fingerprint IS the canonical identity id
  (self-certifying, mirrors the iroh EndpointId philosophy so HS1 can
  swap keys behind the same derivation).
- `IdentitySignature` carries the signature bytes + the signing key's
  fingerprint (key id), so verification can reject stale keys by id
  before checking bytes.
- `FileBackedIdentitySigner` — the local impl:
  - `open(path, OpenMode) -> SigningResult<Self>`. `OpenMode::Existing`
    fails closed on missing file; `OpenMode::GenerateIfAbsent` creates
    one (LOCAL-DEV ONLY semantics — the caller in deliverable 2 enforces
    mode). ALWAYS fail closed on: unreadable file, wrong permissions
    (must be 0600 on unix — reject group/other bits; mirror HS1's stated
    posture), malformed contents.
  - Key file format: version-tagged (`nimbus-identity-key-v1`), stores
    the Ed25519 PKCS#8 seed. Written 0600 via a create-new + fsync +
    atomic rename staging pattern.
  - **Rotation, mirroring `rotation.rs`'s staged pattern** (read it
    first): `rotate()` generates a new keypair, stages to
    `<path>.rotating`, fsyncs, atomically renames over live, and the
    in-memory signer swaps to the new key. After rotation, `verify`
    REJECTS signatures made by the previous key (stale-key denial —
    signature key-id no longer matches; test proves it). Crash-safety:
    an interrupted rotation (staged file present, live intact) is
    recovered or discarded deterministically on next `open` — document
    which and test it.
  - Private key material: held zeroizing (`Zeroize`/`ZeroizeOnDrop`
    pattern from `key.rs`); `Debug` prints kind + fingerprint +
    `[REDACTED]`; NO Serialize impl; error messages never include key
    bytes or file contents (path is fine).

## Deliverable 2 — identity source + trust config in `nimbus-workload-identity`

The crate's production deps grow by `nimbus-crypto` ONLY (core + tenant +
crypto + serde + thiserror + zeroize). The new types are zero-I/O — they
consume an `IdentityPublicKey`/`&dyn IdentitySigner`, never touch files
themselves.

New module `src/source.rs`:

```rust
/// Canonical machine identity record. NOT the ladder's enforcement
/// string (nimbus_workloads::NodeIdentity) — this is the key-derived
/// canonical id SI2 introduces.
pub struct MachineIdentityRecord { /* id: String (pubkey fingerprint, prefix mk_),
                                      public_key: IdentityPublicKey,
                                      source: IdentitySourceKind */ }
pub struct NodeIdentityRecord    { /* id: String (fingerprint, prefix nk_),
                                      public_key, source */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentitySourceKind {
    /// Explicitly non-production. Provider credential minting from a
    /// LocalDev identity must never be presented as production trust.
    LocalDev,
    /// Reserved for HS1 membership-bound identity. Constructing a
    /// record with this kind requires a future membership proof type —
    /// SI2 provides NO constructor for it (unimplementable by design).
    ClusterMembership,
}
```

- Constructors: `NodeIdentityRecord::local_dev(&IdentityPublicKey)` /
  `MachineIdentityRecord::local_dev(...)` only. Id derivation is pure and
  deterministic from the public key; test determinism.
- Records serialize (id + source + public key hex) — no private material
  exists in these types at all.

New module `src/trust.rs`:

```rust
pub struct IdentityTrustConfig { /* trust_domain: String (validated),
                                    mode: TrustMode */ }
pub enum TrustMode { LocalDev, Production }
```

- `IdentityTrustConfig::local_dev(trust_domain)` and
  `::production(trust_domain)` constructors; validation of trust_domain
  mirrors `nimbus-tenant`'s SPIFFE rules (non-empty, no scheme, no `/`,
  no whitespace — reimplement locally with a comment pointing at
  `identity.rs::validate_spiffe_trust_domain`; do NOT modify
  nimbus-tenant).
- Fail-closed coupling rule, enforced by type + test:
  `TrustMode::Production` + `IdentitySourceKind::LocalDev` must be
  rejected wherever they meet. Provide
  `IdentityTrustConfig::admit_source(&self, source: IdentitySourceKind)
  -> Result<(), TrustConfigError>` — Production admits only
  ClusterMembership (which cannot exist yet ⇒ production minting is
  structurally impossible pre-HS1, exactly the plan's hard gate);
  LocalDev admits LocalDev.
- Crate docs updated: SI2 state — keys + identity + trust config exist;
  issuance still `DenyAllIssuer` until SI3.

## Required tests

nimbus-crypto (signing.rs):
1. Sign/verify round-trip; verify rejects tampered message.
2. `Existing` mode fails closed on missing file; open fails closed on
   0644 permissions (unix) and on malformed/truncated file.
3. `GenerateIfAbsent` creates 0600 file; reopening yields the same
   fingerprint (durability).
4. Rotation: fingerprint changes; NEW signatures verify; a signature
   made pre-rotation is REJECTED (stale-key denial) — assert the error,
   not just is_err.
5. Interrupted-rotation recovery: simulate staged-file-present crash
   state; next open resolves deterministically (per the documented
   choice) and the surviving key signs/verifies.
6. Redaction: Debug output of signer + errors contain fingerprint but
   not key bytes (assert absence of the seed hex); key file bytes never
   appear in any error string.

nimbus-workload-identity:
7. Id derivation deterministic + prefixes correct (`nk_`/`mk_`); record
   serialization carries no private material.
8. Trust config validation rejects bad trust domains (scheme, slash,
   whitespace, empty).
9. `Production.admit_source(LocalDev)` → error; `LocalDev.admit_source
   (LocalDev)` → ok. There is no way to construct a
   ClusterMembership-sourced record (compile-level: no public
   constructor; assert via compile_fail doctest if practical, else
   document).

## Verification gates (worktree root, in order)

```
cargo fmt --all --check
cargo clippy -p nimbus-crypto -p nimbus-workload-identity --all-targets -- -D warnings
cargo test -p nimbus-crypto
cargo test -p nimbus-workload-identity
cargo check -p nimbus-server
```

Report real per-suite counts. nimbus-crypto currently has substantial
tests (materials/rotation/framed) — they must all stay green.
