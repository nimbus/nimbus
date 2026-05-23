# Plan: Artifact Provenance Verification

Canonical deferred design and execution plan for production artifact
verification in Nimbus. This plan owns concrete verification backends for OCI
images, runtime/function bundles, machine images, SBOMs, and attestations.

The tenant-isolation baseline intentionally landed only the policy seam:
`TenantImageVerificationProvider`. That seam is correct, but it is not a
production cryptographic verifier. Nimbus must not hand-roll signature,
Fulcio/Rekor, in-toto, SLSA, SBOM, OCI reference, or OCI referrer
verification logic.

---

## Status

- **Status:** `deferred` for verifier backends; `AP0` parser hardening is
  complete.
- **Primary owner:** this plan
- **Activation gate:** `AP0` was promoted as a prerequisite for enterprise
  policy admission on 2026-05-23. Promote `AP1+` when production deployment
  needs signed images, signed runtime/function bundles, signed machine images,
  SBOM enforcement, or SLSA provenance beyond digest pinning.
- **Current policy seam:** `TenantImageVerificationProvider` in
  `crates/nimbus-server/src/tenant_isolation/image_admission.rs`
- **Current posture reference:** `docs/tenant-isolation.md`

## Goal

Replace proof-only/static verification evidence with battle-tested
verification tooling behind Nimbus-owned policy evaluation.

Nimbus owns:

- policy inputs: allowed registries, digest-required floor, signature issuer
  and subject, builder identity, attestation predicate types, SBOM required,
  and local-build allowance
- evidence normalization into `TenantImageVerificationEvidence`
- admission decision and audit event shape

Nimbus does **not** own:

- cryptographic signature verification
- Fulcio certificate-chain verification
- Rekor transparency-log inclusion or bundle verification
- in-toto DSSE envelope verification
- SLSA provenance verification
- SBOM format validation
- OCI reference or referrer parsing

## Planned Backends

| Backend | Purpose | Product rule |
| --- | --- | --- |
| `OciReferenceParser` | Parse registry/repository/tag/digest correctly. | Use `oci-client`'s `Reference` parser or an equivalent maintained OCI parser; remove hand-rolled `@sha256:` and registry parsing before relying on policy in production. |
| `CosignVerifierBackend` | Verify Sigstore/Cosign image signatures and attestations. | Prefer the Cosign CLI or a narrowly wrapped, proven library path first. `cosign verify` validates image digest claims by default; do not disable claim checking in production. |
| `SlsaVerifierBackend` | Verify SLSA provenance for images or files. | Require immutable digest references for images to avoid TOCTOU; accept verified builder ID and predicate evidence only from the verifier output. |
| `SbomVerifierBackend` | Confirm SBOM presence and optional policy shape. | Treat "SBOM exists" separately from "SBOM is trusted and acceptable"; wire stronger policy only after a concrete format is selected. |
| `OfflineBundleVerifierBackend` | Enterprise/offline verification. | Support Sigstore bundles/private roots without forcing live network calls in every environment. |

`sigstore-rs` can become a later implementation option, but it must not be the
only enterprise verifier while it remains experimental/pre-1.0 and lacks
attestation verification coverage.

## Dependency Posture

Use the dependency audit at
`docs/plans/research/service-identity-provenance-dependency-audit.md` before
promoting implementation.

- Iroh-blobs may distribute artifacts by BLAKE3 hash, but BLAKE3 transfer
  integrity is not publisher identity, image signature, SLSA provenance, or
  SBOM trust. Runtime bundles and machine images may need both a BLAKE3
  distribution hash and a SHA-256/Sigstore provenance anchor.
- Use a maintained OCI reference parser before enforcing production image
  policy. Nimbus already depends on `oci-client` in some crates; AP0 should
  choose a single parser path instead of adding more string slicing.
- Prefer Cosign and SLSA verifier command adapters first, with fixture parity
  and output normalization, before replacing them with a Rust library path.
- Keep verifier credentials and registry tokens out of gossip, audit payloads,
  and process output captured in failure events.

## Current Implementation

`AP0` is complete:

- Tenant image admission parses registry references with `oci-client::Reference`
  instead of hand-splitting `@sha256:` or registry prefixes.
- The parser path accepts Docker Hub short names, explicit registries,
  localhost registries with ports, and tag-plus-digest references according to
  the maintained OCI parser.
- Digest-required policy specifically requires a `sha256:` digest on the parsed
  reference. Tag-only references, malformed references, unsupported digest
  algorithms, and wrong registries fail closed before verifier evidence can
  authorize them.
- `TenantImageVerificationProvider` receives the canonical OCI reference string
  without transport prefixes such as `docker://`. Nimbus still only normalizes
  policy and evidence; Cosign, SLSA, SBOM, Fulcio/Rekor, in-toto, and OCI
  referrer verification remain future backend responsibilities.
- Compose production image admission uses the same maintained parser class for
  its digest-pinned provenance floor.

## Scope

This plan covers artifact classes that can carry executable or trusted code:

- OCI service images
- runtime/function bundles (`bundle.mjs`, `bundle.sha256`, future bundle
  envelopes)
- machine OS images
- guest helper binaries such as `nimbus-guest-user-switch`
- installer/release artifacts when they feed sandbox or runtime execution

This plan does not own runtime permission policy, tenant admission, or service
identity. It feeds verified artifact evidence into those seams.

## Phase Ledger

| Phase | Status | Goal | Verification |
| --- | --- | --- | --- |
| AP0 | `done` | Replace hand-rolled OCI reference parsing at image admission call sites. | `cargo test -p nimbus-server image_admission -- --nocapture` covers Docker Hub defaults, explicit registries, localhost with ports, digest references, tag+digest references, malformed references, allowed-registry matching, and provider canonical-reference input. Compose production admission has focused digest and invalid-reference coverage in `cargo test -p nimbus-bin production_compose_admission -- --nocapture`. |
| AP1 | `todo` | Add verifier command/library adapter contract. | Tests prove verifier process/library failures fail closed and redact command output in audit events. |
| AP2 | `todo` | Implement Cosign verifier backend. | Signed/unsigned/wrong-identity fixtures prove issuer/subject and digest-claim enforcement. |
| AP3 | `todo` | Implement SLSA provenance verifier backend. | Digest-pinned image and file/bundle fixtures prove builder ID and predicate type enforcement. |
| AP4 | `todo` | Extend provenance policy from images to runtime/function bundles. | Bundle admission rejects unsigned or mutable artifacts before runtime invocation. |
| AP5 | `todo` | Add SBOM evidence backend and operator policy hooks. | SBOM-present and SBOM-missing fixtures prove policy behavior without parsing secrets into logs. |
| AP6 | `todo` | Add offline/private-root verification mode. | Fixture with local trust material verifies without public network dependencies. |
| AP7 | `todo` | Publish operator runbook and conformance gate. | One command verifies image, bundle, SLSA, SBOM, and failure-path fixtures. |

## Acceptance Criteria

- No Nimbus code implements raw signature, certificate-chain, transparency-log,
  DSSE, or SLSA cryptography.
- OCI references are parsed through a maintained parser, not string slicing.
- Tag-only or mutable references fail closed for production verification.
- Cosign and SLSA verifier outputs are normalized into Nimbus evidence without
  trusting caller-supplied JSON.
- Verification failures are audit-visible and redact sensitive registry tokens.
- Tenant isolation conformance still passes after verifier integration.

## References

- `docs/tenant-isolation.md`
- `docs/architecture/sandbox/microvm-service-baseline.md`
- `docs/plans/archive/tenant-isolation-enterprise-hardening-plan.md`
- `docs/plans/research/tenant-isolation-enterprise-hardening-prior-art.md`
- `https://docs.sigstore.dev/cosign/verifying/verify/`
- `https://docs.sigstore.dev/language_clients/rust/`
- `https://github.com/sigstore/sigstore-rs`
- `https://github.com/slsa-framework/slsa-verifier`
- `https://docs.rs/oci-client/latest/oci_client/struct.Reference.html`
