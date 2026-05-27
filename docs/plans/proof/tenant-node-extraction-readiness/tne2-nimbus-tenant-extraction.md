# TNE2 Nimbus Tenant Extraction

- **Phase ID:** TNE2
- **Status:** `done`
- **Git base:** `727a8952` on `main`
- **Files touched:** `Cargo.toml`, `crates/nimbus-tenant/Cargo.toml`, all
  Rust files and policy fixtures moved under `crates/nimbus-tenant/`,
  `crates/nimbus-server/Cargo.toml`, `crates/nimbus-server/src/tenant.rs`,
  `crates/nimbus-server/src/lib.rs`,
  `crates/nimbus-server/src/artifact_verifier_effects.rs`,
  `crates/nimbus-server/src/execution/invocations/mod.rs`,
  `crates/nimbus-bin/src/policy.rs`,
  `docs/plans/tenant-and-node-crate-extraction-readiness-plan.md`, and this
  proof note.
- **Requirement IDs touched:** REQ-EFFECTS, REQ-VERIFIER, REQ-TENANT-CRATE,
  REQ-ADMIT, REQ-RAW, REQ-DOCS

## Baseline Findings

- TNE1 moved command/process verifier effects out of the tenant candidate.
- Before extraction, `crates/nimbus-server/src/tenant/artifact_provenance/admission.rs`
  still computes file SHA-256 values through
  `RuntimeBundle::compute_sha256_for_path` for runtime bundles and guest
  executables. That is host filesystem authority and must move to server-owned
  wiring before the crate can honestly be pure.
- The extracted crate will need only pure dependencies:
  `nimbus-core`, `nimbus-runtime`, `nimbus-sandbox`, `serde`, `serde_json`,
  `sha2`, and OCI reference parsing support unless that parsing is replaced.

## Intended Change

Implemented:

- Split executable artifact host hashing into server-owned verifier wiring.
  `nimbus-tenant` now admits an already-identified artifact subject purely;
  `nimbus-server` hashes runtime bundle and guest executable paths before
  calling the pure tenant admission helper.
- Moved pure tenant authority modules and policy fixtures into
  `crates/nimbus-tenant`.
- Kept `crates/nimbus-server/src/tenant.rs` as a thin re-export of
  `nimbus_tenant` so existing server internals consume the extracted crate
  through one intentional compatibility point.
- Grouped `nimbus-server` public re-exports so pure tenant contracts come from
  `tenant`, while command verifier and executable-hash effects come from
  `artifact_verifier_effects`.
- Moved policy fixtures from `crates/nimbus-server/tests/fixtures/policy` to
  `crates/nimbus-tenant/tests/fixtures/policy` and updated the CLI policy test
  fixture include.

## Verification Log

- `cargo check -p nimbus-tenant` passed after extraction: finished `dev`
  profile for `nimbus-tenant`.
- `cargo check -p nimbus-server` passed after server re-export wiring:
  finished `dev` profile for `nimbus-server`.
- `cargo test -p nimbus-tenant -- --nocapture` passed: 79 unit tests passed,
  0 failed, 0 ignored, 0 filtered out.
- `cargo test -p nimbus-server artifact_verifier_effects -- --nocapture`
  passed: 37 unit tests passed, 0 failed, 767 filtered out; integration
  binaries selected 0 tests in `mongodb_spec` and 0 tests in `reactive_loop`.
- `cargo test -p nimbus-server tenant_isolation -- --nocapture` passed: 3
  server tests passed, 0 failed, 801 filtered out. The conformance harness
  still reported 21 scenarios: 12 allowed and 9 denied.
- `cargo test -p nimbus-server local_enforcement -- --nocapture` passed: 22
  unit tests passed, 0 failed, 782 filtered out; integration binaries selected
  0 tests in `mongodb_spec` and 0 tests in `reactive_loop`.
- `cargo test -p nimbus-bin policy -- --nocapture` passed: 10 unit tests
  passed, 0 failed, 544 filtered out; `server_discovery_serde` selected 0
  tests.
- `cargo check --workspace` passed: finished `dev` profile for the workspace,
  including `nimbus`, `nimbus-bin`, `nimbus-server`, and `nimbus-tenant`.
- `cargo clippy -p nimbus-tenant --all-targets --no-deps` passed: finished
  `dev` profile with no warnings reported.
- `cargo clippy -p nimbus-server --all-targets --no-deps` passed: finished
  `dev` profile with no warnings reported.
- `cargo fmt --all --check` passed with no output.
- `cargo tree -p nimbus-tenant -e normal --depth 1` showed only direct normal
  dependencies on `nimbus-core`, `nimbus-runtime`, `nimbus-sandbox`,
  `oci-client`, `serde`, `serde_json`, and `sha2`.
- Forbidden dependency/effect audit:
  `rg -n "use .*axum|use .*tokio|use .*tonic|use .*tower|use .*mongodb|use .*firebase|use .*convex|nimbus_server|nimbus_storage|nimbus_machine|nimbus_engine|system_tenant|crate::system|std::process|Command::new|Stdio|std::fs::|compute_sha256_for_path|HostLifecycle|HostLifecycleBackend|RuntimeExecutor|_nimbus" crates/nimbus-tenant/src crates/nimbus-tenant/Cargo.toml --glob '!**/tests.rs'`
  returned no matches, exit code 1.
- Broad host-effect audit:
  `rg -n "compute_sha256_for_path|std::fs|std::process|Command::new|Stdio|metadata\\(|read_to|File::open|OpenOptions|std::io" crates/nimbus-tenant/src --glob '*.rs'`
  returned only pure metadata method names in operator-policy request types,
  not filesystem/process effects.
- `git diff --check -- Cargo.toml crates/nimbus-tenant crates/nimbus-server/Cargo.toml crates/nimbus-server/src crates/nimbus-bin/src/policy.rs docs/plans/tenant-and-node-crate-extraction-readiness-plan.md docs/plans/proof/tenant-node-extraction-readiness/tne2-nimbus-tenant-extraction.md`
  passed with no output.
- `npm run docs:validate-refs:strict` passed: docs reference validation passed
  for 213 working-tree Markdown files.

## Requirement Evidence

- REQ-EFFECTS: `nimbus-tenant` contains no process launch, command execution,
  trusted-root filesystem probing, executable file hashing, host lifecycle,
  server transport, adapter, or storage-provider effects.
- REQ-VERIFIER: command verifier effects and executable-path hashing are owned
  by `nimbus-server::artifact_verifier_effects`; server verifier tests cover
  CLI verifier behavior and host hash fail-closed behavior.
- REQ-TENANT-CRATE: `nimbus-tenant` now owns tenant identity, context,
  decisions, policy input, quotas, projections, audit/evidence shapes, image
  admission, operator policy, and pure artifact contracts. The direct
  dependency audit is limited to approved primitive/pure dependencies.
- REQ-ADMIT: tenant tests, server conformance, runtime artifact verifier tests,
  and local enforcement tests prove lower layers still consume
  `TenantIsolationDecision` or narrow projections after extraction.
- REQ-RAW: tenant tests and server verifier/local-enforcement tests continue to
  cover redaction, digest normalization, OCI reference canonicalization,
  property allowlists, sanitized unit names, and metric-label cardinality.
- REQ-DOCS: plan state, proof note, docs reference validation, and diff checks
  are current.

## Remaining Risks

- `nimbus-tenant` intentionally depends on `nimbus-runtime` and
  `nimbus-sandbox` because tenant decisions describe runtime and sandbox
  primitives. This is plan-approved but broad; future shrinking could split
  pure data types out of those crates if the dependency graph becomes too
  expensive.
- `oci-client` is used for OCI reference parsing only. The crate is not a
  server adapter dependency, but it is not a tiny parser crate; this remains an
  explicit dependency decision in TNE2 evidence.

## Next Resumable Action

- Start TNE3 by adding a production `NodeWorkloadReconciler` that drives
  `HostLifecycleBackend::validate`, `start`, `stop`, and `inspect` and writes
  observed status through a narrow server-owned writer trait.
