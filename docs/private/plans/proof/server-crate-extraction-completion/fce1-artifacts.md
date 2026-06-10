# FCE1: Extract `nimbus-artifacts`

Status: completed
Started: 2026-05-28
Completed: 2026-05-28
Requirements: FCE-REQ-001, FCE-REQ-002, FCE-REQ-003, FCE-REQ-004, FCE-REQ-006, FCE-REQ-007, FCE-REQ-008, FCE-REQ-010

## Scope

- Files/modules moved:
  - `crates/nimbus-tenant/src/artifact_provenance.rs` -> `crates/nimbus-artifacts/src/lib.rs`
  - `crates/nimbus-tenant/src/artifact_provenance/admission.rs` -> `crates/nimbus-artifacts/src/admission.rs`
- Files/modules intentionally left in `nimbus-server`:
  - process-backed artifact verifier command execution
  - default `cosign`, `slsa`, and SBOM process-runner wiring
  - operator/server composition that chooses verifier backends
- Crates created or updated:
  - created: `crates/nimbus-artifacts`
  - updated: `crates/nimbus-tenant`, `crates/nimbus-server`

## Ownership Decisions

- Authority owner: `nimbus-tenant` keeps tenant artifact admission decisions.
- Effect owner: `nimbus-server` keeps process-backed verifier execution and default tool wiring.
- Server composition shell: server chooses concrete verifier backends and wires them into tenant admission.
- Explicit keep decisions: `nimbus-artifacts` must contain pure artifact contracts only; it must not depend on `nimbus-tenant`, `nimbus-system`, storage, Axum, or process execution.

## Seam Fix Attempts

- Messy seam found: pure artifact request/evidence contracts shared ownership pressure with tenant image admission and server verifier effects.
- Right-sized ownership-correct repair attempted: created `nimbus-artifacts`, moved pure artifact contracts and pure artifact admission there, made `nimbus-tenant` consume those contracts through a small `ArtifactImageVerificationProvider` adapter, and made server verifier/provenance code import artifact contracts from `nimbus-artifacts`.
- Files changed or spike/proof performed:
  - `Cargo.toml`
  - `crates/nimbus-artifacts/Cargo.toml`
  - `crates/nimbus-artifacts/src/lib.rs`
  - `crates/nimbus-artifacts/src/admission.rs`
  - `crates/nimbus-tenant/Cargo.toml`
  - `crates/nimbus-tenant/src/artifact_provenance.rs`
  - `crates/nimbus-tenant/src/image_admission.rs`
  - `crates/nimbus-server/Cargo.toml`
  - `crates/nimbus-server/src/artifact_verifier_effects.rs`
  - `crates/nimbus-server/src/artifact_verifier_effects/cosign.rs`
  - `crates/nimbus-server/src/artifact_verifier_effects/sbom.rs`
  - `crates/nimbus-server/src/artifact_verifier_effects/slsa.rs`
  - `crates/nimbus-server/src/execution/invocations/provenance.rs`
  - `crates/nimbus-server/src/adapters/cloud_functions/registry.rs`
  - `crates/nimbus-server/src/adapters/convex/registry/loading.rs`
  - `crates/nimbus-server/src/lib.rs`
- Result: completed extraction. `nimbus-artifacts` exists as a workspace crate and owns artifact contracts without server, tenant, system, storage, Axum, or process execution.
- If blocked, exact architectural reason: n/a.
- Next implementation move: advance to FCE2 provenance extraction.

## Dependency Evidence

Command:

```text
cargo tree -p nimbus-artifacts --edges normal --depth 1
```

Output:

```text
nimbus-artifacts v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-artifacts)
├── nimbus-core v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-core)
├── oci-client v0.16.1
└── serde v1.0.228
```

Focused compile checks:

```text
cargo check -p nimbus-artifacts
Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.94s

cargo check -p nimbus-tenant
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.72s

cargo check -p nimbus-server
Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.33s
```

## Denied-Import Evidence

Command:

```text
rg -n "nimbus-server|nimbus_tenant|nimbus-tenant|nimbus_system|nimbus-system|std::process|Command::new|Stdio|axum|nimbus_storage|nimbus-storage" crates/nimbus-artifacts -g '*.rs' -g 'Cargo.toml'
```

Output:

```text
<no output; rg exited 1>
```

Server call sites now import artifact contracts from `nimbus_artifacts`:

```text
crates/nimbus-server/src/artifact_verifier_effects.rs
crates/nimbus-server/src/artifact_verifier_effects/cosign.rs
crates/nimbus-server/src/artifact_verifier_effects/sbom.rs
crates/nimbus-server/src/artifact_verifier_effects/slsa.rs
crates/nimbus-server/src/execution/invocations/provenance.rs
crates/nimbus-server/src/adapters/cloud_functions/registry.rs
crates/nimbus-server/src/adapters/convex/registry/loading.rs
crates/nimbus-server/src/lib.rs
```

## Tests

```text
cargo test -p nimbus-artifacts -- --nocapture

running 6 tests
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```text
cargo test -p nimbus-tenant artifact -- --nocapture

running 2 tests
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 72 filtered out; finished in 0.20s
```

```text
cargo test -p nimbus-server artifact_verifier_effects -- --nocapture

running 37 tests
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 733 filtered out; finished in 0.22s

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 23 filtered out; finished in 0.00s

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 32 filtered out; finished in 0.00s
```

Ignored tests:

- none recorded yet.

## Verifier Update

- Conditions added or updated:
  - FCE1 completed proof must exist.
  - `nimbus-artifacts` must exist in cargo metadata.
  - `cargo tree -p nimbus-artifacts` must have no `nimbus-server` dependency.
  - denied import scan must reject server, tenant, system, storage, Axum, and process execution imports in `nimbus-artifacts`.
  - FCE1 proof must record artifact, tenant adapter, and server verifier focused test counts.
  - server artifact/provenance call sites must import from `nimbus_artifacts`.
  - tenant-owned `ArtifactImageVerificationProvider` adapter must remain in `nimbus-tenant`.
- Current verifier result:

```text
bash scripts/verify-server-crate-extraction-completion.sh

[9] FCE1 nimbus-artifacts extraction is enforced when complete
  PASS  nimbus-artifacts is extracted, server-free, process-free, and covered by focused tests

Summary: 9 passed, 0 failed
```

## Residual Risk And Resume Notes

- Remaining risk: `nimbus-artifacts` intentionally uses `oci-client::Reference` for pure OCI reference parsing; no network calls or process execution are present in the crate.
- Next action: continue FCE2 provenance extraction.
