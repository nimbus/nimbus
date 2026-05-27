# TNE1 Artifact Verifier Effects

- **Phase ID:** TNE1
- **Status:** `done`
- **Git base:** `5e5e34bd` on `main`
- **Files touched:**
  `crates/nimbus-server/src/artifact_verifier_effects.rs`,
  `crates/nimbus-server/src/artifact_verifier_effects/cosign.rs`,
  `crates/nimbus-server/src/artifact_verifier_effects/slsa.rs`,
  `crates/nimbus-server/src/artifact_verifier_effects/sbom.rs`,
  `crates/nimbus-server/src/lib.rs`,
  `crates/nimbus-server/src/tenant.rs`,
  `crates/nimbus-server/src/tenant/artifact_provenance.rs`,
  removed
  `crates/nimbus-server/src/tenant/artifact_provenance/cosign.rs`,
  removed
  `crates/nimbus-server/src/tenant/artifact_provenance/slsa.rs`,
  removed
  `crates/nimbus-server/src/tenant/artifact_provenance/sbom.rs`,
  `docs/plans/tenant-and-node-crate-extraction-readiness-plan.md`, and
  this proof note.
- **Requirement IDs touched:** REQ-EFFECTS, REQ-VERIFIER, REQ-TENANT-CRATE,
  REQ-ADMIT, REQ-RAW, REQ-DOCS

## Baseline Findings

- `crates/nimbus-server/src/tenant/artifact_provenance.rs` currently imports
  `std::process::{Command, Stdio}` and owns
  `ProcessArtifactVerifierCommandRunner`.
- `OfflineVerificationConfig::validate` currently calls `std::fs::metadata`
  inside the tenant-domain module.
- `CosignVerifierBackend`, `SlsaVerifierBackend`, `SbomVerifierBackend`, and
  `ArtifactVerifierCommandBackend` currently default to process-backed command
  execution from tenant-domain constructors.
- Tenant-domain tests cover command normalization, redaction, timeout,
  missing-tool behavior, cosign digest and offline-root failures, SLSA builder
  and predicate validation, and SBOM-required behavior. These tests need to
  move with server-owned verifier effects or be rewritten against pure tenant
  contracts.

## Behavior Changed

- The tenant candidate now keeps pure artifact authority contracts only:
  request, subject, policy, evidence, backend identity, backend trait, errors,
  redaction, composite verifier, image verification provider, and artifact
  admission helpers.
- Concrete verifier effects moved to server-owned
  `artifact_verifier_effects`: generic command backend, command invocation and
  output shapes, command runner trait, process-backed command runner, default
  cosign/SLSA/SBOM CLI constructors, default verifier timeout, and offline
  trusted-root filesystem validation.
- `nimbus-server` still re-exports the moved effect types intentionally from
  the server crate root, while `crate::tenant` no longer re-exports them.
- `SLSA_PROVENANCE_V1_PREDICATE_TYPE` remains in the tenant candidate as a
  pure provenance predicate constant used by tenant admission policies.

## Tests Added Or Updated

- Moved command backend, process-runner, cosign, SLSA, and SBOM tests with the
  server-owned effect modules.
- Kept tenant artifact-provenance tests focused on pure admission, composite
  evidence, provider fail-closed behavior, and redaction.
- No compatibility shim was added; the internal module ownership changed
  directly.

## Verification Log

- `cargo test -p nimbus-server artifact_verifier_effects -- --nocapture`
  passed: 34 unit tests passed, 0 failed, 846 filtered out; integration test
  binaries selected 0 tests in `mongodb_spec` and 0 tests in `reactive_loop`.
- `cargo test -p nimbus-server tenant::artifact_provenance -- --nocapture`
  passed: 7 unit tests passed, 0 failed, 873 filtered out; integration test
  binaries selected 0 tests in `mongodb_spec` and 0 tests in `reactive_loop`.
- `cargo test -p nimbus-server tenant_isolation -- --nocapture` passed: 20
  unit tests passed, 0 failed, 860 filtered out. The conformance harness
  reported 21 scenarios: 12 allowed and 9 denied.
- `cargo fmt --all --check` passed with no output.
- `cargo check -p nimbus-server` passed: finished `dev` profile for
  `nimbus-server`.
- `cargo clippy -p nimbus-server --all-targets --no-deps` passed: finished
  `dev` profile with no warnings reported.
- Tenant candidate forbidden-effect audit:
  `rg -n "std::process|Command::new|Stdio|std::fs::metadata|fs::metadata|ProcessArtifactVerifierCommandRunner|ArtifactVerifierCommandBackend|ArtifactVerifierCommandRunner|ArtifactVerifierCommandInvocation|ArtifactVerifierCommandOutput|OfflineVerificationConfig|CosignVerifierBackend|SlsaVerifierBackend|SbomVerifierBackend|DEFAULT_ARTIFACT_VERIFIER_TIMEOUT|Arc::new\\(ProcessArtifactVerifierCommandRunner\\)" crates/nimbus-server/src/tenant.rs crates/nimbus-server/src/tenant --glob '!**/tests.rs'`
  returned no matches, exit code 1.
- Server-owned effect audit:
  `rg -n "std::process|Command::new|Stdio|std::fs::metadata|fs::metadata|ProcessArtifactVerifierCommandRunner|Arc::new\\(ProcessArtifactVerifierCommandRunner\\)" crates/nimbus-server/src/artifact_verifier_effects.rs crates/nimbus-server/src/artifact_verifier_effects`
  matched only the new server-owned effect module and moved cosign/SLSA/SBOM
  constructors.
- `git diff --check -- crates/nimbus-server/src/lib.rs crates/nimbus-server/src/tenant.rs crates/nimbus-server/src/tenant/artifact_provenance.rs crates/nimbus-server/src/artifact_verifier_effects.rs crates/nimbus-server/src/artifact_verifier_effects/cosign.rs crates/nimbus-server/src/artifact_verifier_effects/slsa.rs crates/nimbus-server/src/artifact_verifier_effects/sbom.rs docs/plans/tenant-and-node-crate-extraction-readiness-plan.md docs/plans/proof/tenant-node-extraction-readiness/tne1-artifact-verifier-effects.md`
  passed with no output.
- `npm run docs:validate-refs:strict` passed: docs reference validation passed
  for 213 working-tree Markdown files.

## Requirement Evidence

- REQ-EFFECTS: tenant-domain production code no longer contains process
  launch, command runner, default CLI verifier constructors, or trusted-root
  filesystem metadata probing.
- REQ-VERIFIER: effect-owned tests cover generic command backend success,
  non-zero failure, missing executable, malformed output, timeout, unsupported
  artifact, cosign success, unsigned failure, wrong issuer/subject, wrong
  digest, malformed output, missing tool, offline trusted-root success and
  missing-root fail-closed behavior, SLSA image/file success, mutable image,
  wrong builder, wrong predicate, wrong subject digest, malformed output,
  timeout, missing tool, and SBOM success/failure/malformed/mutable/missing
  tool/required-policy behavior.
- REQ-TENANT-CRATE: this phase removes the known process and filesystem
  blockers from the tenant candidate, but the crate extraction proof remains
  TNE2 scope.
- REQ-ADMIT: tenant-isolation conformance still passes with 21 scenarios across
  runtime services, storage, system control, cleanup, and sandbox projections.
- REQ-RAW: verifier failure and provider failure tests still redact tokens,
  credentials, secret handles, registry auth, and runner timeout secrets.
- REQ-DOCS: plan state, this proof note, `git diff --check`, and strict docs
  reference validation are current.

## Remaining Risks

- TNE2 still needs to extract `crates/nimbus-tenant` and prove the new crate
  dependency graph is clean. TNE1 only made the tenant candidate extractable
  with respect to verifier host effects.

## Next Resumable Action

- Start TNE2 by creating `crates/nimbus-tenant`, moving the pure tenant
  candidate into it, and proving the crate has no server, adapter, storage
  provider, process-launch, host-lifecycle, runtime-executor, or system-tenant
  persistence dependencies.
