# Plan: Artifact Provenance Verification

Canonical design, execution plan, and control plane for production artifact
verification in Nimbus. This plan owns concrete verification backends for OCI
images, runtime/function bundles, machine images, SBOMs, and attestations.

The tenant-isolation baseline intentionally landed only the policy seam:
`TenantImageVerificationProvider`. That seam is correct, but it is not a
production cryptographic verifier. Nimbus must not hand-roll signature,
Fulcio/Rekor, in-toto, SLSA, SBOM, OCI reference, or OCI referrer
verification logic.

---

## Status

- **Status:** `AP0` through `AP7` are complete.
- **Primary owner:** this plan
- **Activation gate:** `AP0` was promoted as a prerequisite for enterprise
  policy admission on 2026-05-23. `AP1+` is now the next implementation lane
  after the enterprise policy and sandbox egress baseline.
- **Current policy seam:** `TenantImageVerificationProvider` in
  `crates/nimbus-server/src/tenant_isolation/image_admission.rs`
- **Current posture reference:** `docs/tenant-isolation.md`
- **Progress state:** this file's phase ledger plus local git commits on the
  active worktree/branch are the handoff state. Update the ledger before any
  context loss, stop, or commit that changes AP scope.

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

## Execution Control Plane

This plan is intended to run as one autonomous `/goal` from `AP1` through
`AP7`. Do not split the work into separate plans unless a phase proves
impossible without an external service, account, or trust root that is not
available locally.

The implementation order is:

1. **Verifier contract first (`AP1`)**
   - Add a Nimbus-owned verifier adapter boundary that can wrap command-line
     tools or future library backends.
   - Evidence entering tenant admission must be typed and normalized. Raw
     verifier stdout/stderr, registry credentials, bearer tokens, private keys,
     and cloud credentials must not enter audit output, policy explain output,
     or errors.
   - Command/library failures, malformed outputs, timeouts, missing tools, and
     unsupported artifact classes fail closed.

2. **OCI image verification (`AP2` and `AP3`)**
   - Wire Cosign and SLSA verifier adapters behind the existing image
     verification provider seam.
   - Prefer command adapters first because Cosign/SLSA CLIs are the canonical
     production tools. Keep parsing narrow and fixture-driven; do not implement
     signature, Fulcio/Rekor, DSSE, in-toto, or SLSA crypto in Nimbus.
   - Accept verified issuer/subject, builder ID, predicate type, SBOM presence,
     and digest-claim evidence only from verifier-normalized output.

3. **Non-image executable artifacts (`AP4`)**
   - Extend the provenance seam from OCI images to runtime/function bundles and
     machine/guest artifacts that feed execution.
   - Runtime invocation must not execute a bundle that policy says requires
     provenance until the bundle has verified evidence.
   - Maintain single-binary simplicity: provenance logic lives in server/core
     admission seams and optional verifier adapters, not in runtime hot paths.

4. **SBOM and offline enterprise modes (`AP5` and `AP6`)**
   - Add SBOM presence policy without pretending that "SBOM exists" proves
     vulnerability posture or license compliance.
   - Add offline/private-root verification fixtures so enterprise and air-gapped
     deployments can verify without relying on public network calls at
     admission time.

5. **Operator proof and conformance (`AP7`)**
   - Publish an operator runbook and a reusable verification script, exposed via
     Makefile, that proves image, bundle, SLSA, SBOM, offline, and failure-path
     behavior in one command.
   - The final gate must run alongside tenant isolation and enterprise policy
     gates without weakening their assertions.

Autonomous execution rules:

- Keep phase status accurate: exactly one AP phase may be `in_progress`.
- Commit after meaningful phase checkpoints with messages prefixed by the
  phase, for example `AP1: add verifier adapter contract`.
- If a real external binary such as `cosign` or `slsa-verifier` is unavailable,
  implement the adapter boundary and deterministic fixture/fake command
  harness first, record the missing binary as a residual risk, and continue
  with local verifiable behavior. Do not block the whole goal on downloading a
  live verifier unless the user has approved network/tool installation.
- Do not add hand-written cryptographic verification code to make tests pass.
- Prefer narrow fixtures checked into the repo over live network verification.
- Before completion, update this plan, `docs/plans/README.md`,
  `docs/tenant-isolation.md`, and relevant runbooks so tracked docs match the
  implemented product shape.

## Required Verification

Every completed phase must have a focused test command in the ledger. Final
completion requires all of:

```sh
cargo fmt --all --check
cargo clippy -p nimbus-server -p nimbus-bin -- -D warnings
cargo test -p nimbus-server image_admission -- --nocapture
cargo test -p nimbus-bin production_compose_admission -- --nocapture
bash scripts/verify-artifact-provenance.sh
bash scripts/verify-enterprise-policy-egress.sh
git diff --check
```

If `AP4` adds runtime/bundle admission tests in another crate, add the focused
command to this list and to `scripts/verify-artifact-provenance.sh`.

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

`AP1` is complete:

- Artifact verification now has a typed Nimbus-owned adapter boundary for
  executable artifact requests, policy requirements, normalized evidence,
  backend identity, command invocation, and fail-closed verifier errors.
- Image admission now passes a typed `TenantImageVerificationRequest` carrying
  canonical image reference plus signature, provenance, and SBOM requirements,
  so real Cosign/SLSA adapters do not have to recover policy context from
  global state.
- `ArtifactImageVerificationProvider` bridges artifact verifier backends into
  the existing tenant image admission seam and keeps policy evaluation in
  Nimbus.
- The command adapter accepts normalized verifier output only. Non-zero exits,
  backend/library errors, missing executables, malformed output, timeouts, and
  unsupported artifact classes fail closed.
- Verifier stdout/stderr and backend errors are redacted for tokens,
  credentials, secret handles, registry auth, cookies, bearer values, and
  private key material before they enter errors or policy-facing output.

`AP2` is complete:

- `CosignVerifierBackend` wraps the Cosign CLI through the AP1 command-runner
  seam. Nimbus does not implement signature, Fulcio/Rekor, or certificate-chain
  verification.
- The backend requires an OCI image subject, an immutable `sha256:` digest
  reference, and explicit certificate issuer plus identity policy before
  invoking Cosign.
- The command uses `cosign verify --output json --check-claims=true` with
  `--certificate-identity` and `--certificate-oidc-issuer`, preserving Cosign as
  the canonical verification authority.
- Successful Cosign output is treated as verified evidence only after Nimbus
  confirms that every returned payload carries the expected manifest digest
  claim. The normalized evidence records the policy issuer/subject that Cosign
  verified.
- Unsigned images, wrong issuer/subject, mutable/tag-only references, wrong
  digest claims, malformed JSON, missing Cosign binary, unsupported artifact
  classes, and missing signature policy fail closed with redacted diagnostics.

`AP3` is complete:

- `SlsaVerifierBackend` wraps the `slsa-verifier` CLI through the AP1
  command-runner seam. Nimbus does not implement in-toto, DSSE, SLSA, or
  provenance cryptography.
- Provenance policy now carries optional `source_uri` context alongside builder
  ID and predicate types so SLSA verification can pass source expectations to
  the verifier without hard-coding them in backend config.
- OCI image verification requires an immutable `sha256:` image reference before
  invoking `slsa-verifier verify-image`.
- File-like artifacts (`file`, `runtime_bundle`, and `guest_executable`
  subjects) require an immutable SHA-256 subject and configured local
  provenance path before invoking `slsa-verifier verify-artifact`.
- Successful SLSA output is treated as verified evidence only after Nimbus
  confirms the printed statement builder ID, predicate type, and subject digest
  match policy and immutable artifact inputs.
- Wrong builder ID, wrong predicate type, wrong subject digest, mutable image
  references, missing file digest, missing provenance path, malformed output,
  timeout, and missing verifier binary fail closed with redacted diagnostics.

`AP4` is complete:

- `ArtifactAdmission` records the executable artifact subject plus normalized
  verifier evidence without carrying raw verifier logs.
- Runtime bundle admission now requires an immutable bundle SHA-256 identity,
  re-hashes the bundle before verification, and rejects missing or mismatched
  digest identities before calling verifier backends.
- Runtime invocation options can carry a provenance gate; blocking and worker
  runtime invocation paths call it before entering the runtime executor.
- Guest executable admission reuses the same policy and verifier evidence shape
  as runtime bundles, giving sandbox helper binaries the same pre-launch
  provenance seam.
- Admission checks returned evidence against signature, provenance, and SBOM
  policy requirements so fixture or future library backends cannot bypass
  Nimbus-owned policy evaluation with incomplete evidence.

`AP5` is complete:

- `SbomVerifierBackend` provides SBOM-presence evidence through the AP1 command
  runner seam. The default command shape uses Cosign SBOM download for
  digest-pinned OCI images.
- SBOM admission requires `sbom_required` policy context and immutable
  `sha256:` image references before invoking the verifier.
- Successful SBOM retrieval only records `sbom_present=true`; Nimbus does not
  treat SBOM existence as vulnerability clearance, license clearance, or full
  format validation.
- Missing SBOMs, empty/malformed output, missing verifier binary, tag-only
  images, and missing SBOM policy fail closed with redacted diagnostics.
- Operator image policy now has a focused regression proving `sbom_required`
  compiles into tenant image admission and rejects missing SBOM evidence.

`AP6` is complete:

- `OfflineVerificationConfig` validates local trusted-root material before any
  verifier command runs.
- `CosignVerifierBackend` can run in offline/private-root mode with
  `--offline=true --trusted-root <path>`.
- Missing or invalid trusted-root files fail closed before command invocation,
  so local/offline fixtures do not accidentally fall back to public-network
  verification.
- The current SLSA file-artifact path already supports local provenance files
  through `SlsaVerifierBackend::with_provenance_path`; stronger SLSA/private
  root customization remains tool-specific follow-on scope if the selected
  verifier exposes it.

`AP7` is complete:

- `scripts/verify-artifact-provenance.sh` runs the artifact provenance
  conformance gate without live registry network access.
- `make verify-artifact-provenance` exposes the same gate through the repo's
  Makefile, and `proof-helpers` syntax-checks the script.
- `docs/tenant-isolation.md` now names the concrete Cosign, SLSA, SBOM,
  offline/private-root, runtime bundle, and guest executable provenance
  controls plus the evidence command.
- `docs/plans/README.md` points to this plan as the current provenance
  verification baseline.

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
| AP1 | `done` | Add verifier command/library adapter contract. | `cargo test -p nimbus-server artifact_provenance -- --nocapture` passed 8 focused adapter tests covering success, non-zero exit, library/backend error, missing executable, malformed output, timeout, unsupported artifact class, and redaction. `cargo test -p nimbus-server image_admission -- --nocapture` passed 12 image admission tests after the request seam widened. |
| AP2 | `done` | Implement Cosign verifier backend for OCI images. | `cargo test -p nimbus-server cosign -- --nocapture` passed 9 focused Cosign adapter tests covering signed, unsigned, wrong issuer/subject, mutable/tag-only, wrong digest claim, malformed verifier output, missing tool, unsupported artifact class, and missing signature policy behavior. |
| AP3 | `done` | Implement SLSA provenance verifier backend for OCI images and file artifacts. | `cargo test -p nimbus-server slsa -- --nocapture` passed 10 focused SLSA adapter tests covering digest-pinned image, file artifact with provenance path, builder ID, predicate type, immutable subject, malformed output, timeout, missing tool, mutable image, and missing file/provenance inputs. Regression checks `cargo test -p nimbus-server image_admission -- --nocapture` and `cargo test -p nimbus-server operator_policy -- --nocapture` passed after adding optional provenance `source_uri` policy context. |
| AP4 | `done` | Extend provenance policy from images to runtime/function bundles and executable guest artifacts. | `cargo test -p nimbus-server artifact_admission -- --nocapture` passed 4 admission tests covering verified runtime bundles, missing digest, wrong digest, wrong builder, wrong predicate, and guest executable admission. `cargo test -p nimbus-server runtime_invocation_options_admit_bundle_provenance -- --nocapture` passed and proves the runtime invocation option admits provenance before executor entry. |
| AP5 | `done` | Add SBOM evidence backend and operator policy hooks. | `cargo test -p nimbus-server sbom -- --nocapture` passed 8 SBOM backend/image-admission tests covering SBOM-present, SBOM-missing, empty/malformed output, missing tool, mutable image, missing SBOM policy, redaction, and tenant image admission. `cargo test -p nimbus-server operator_image_policy_sbom_required -- --nocapture` passed and proves the operator policy hook. |
| AP6 | `done` | Add offline/private-root verification mode. | `cargo test -p nimbus-server offline -- --nocapture` passed local offline/private-root fixtures covering trusted-root command args and missing trusted-root fail-closed behavior. |
| AP7 | `done` | Publish operator runbook and conformance gate. | `bash scripts/verify-artifact-provenance.sh` passed: 40 artifact provenance fixtures, 1 runtime invocation gate fixture, 13 image admission fixtures, 1 operator SBOM policy hook fixture, and 6 production Compose admission fixtures. Makefile target `verify-artifact-provenance` and docs are updated. |

## Acceptance Criteria

- No Nimbus code implements raw signature, certificate-chain, transparency-log,
  DSSE, or SLSA cryptography.
- OCI references are parsed through a maintained parser, not string slicing.
- Tag-only or mutable references fail closed for production verification.
- Cosign and SLSA verifier outputs are normalized into Nimbus evidence without
  trusting caller-supplied JSON.
- Verification failures are audit-visible and redact sensitive registry tokens.
- Tenant isolation conformance still passes after verifier integration.
- `scripts/verify-artifact-provenance.sh` exists, is referenced from the
  Makefile, and passes on a clean local checkout without live registry network
  access.
- The AP1-AP7 ledger is updated to `done`, or any incomplete phase is marked
  with a concrete blocker and failing verification evidence.

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
