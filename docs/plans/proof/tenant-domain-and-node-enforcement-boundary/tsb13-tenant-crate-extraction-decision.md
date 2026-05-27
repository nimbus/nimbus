# TSB13 Tenant Crate Extraction Decision

## Phase

- Phase ID: TSB13
- Status: done
- Git base: `2ef30cd1` on `main`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb13-tenant-crate-extraction-decision.md`

## Requirement IDs

- REQ-ADMIT
- REQ-SYSTEM
- REQ-STORAGE
- REQ-QUOTA
- REQ-CRATE
- REQ-DOCS

## Behavior Changed

- No crate was created. TSB13's extraction condition is false in the current
  tree because TSB12 proved the tenant module still owns concrete process-launch
  artifact verifier code.
- Tenant-domain code remains in `nimbus-server` so process launching does not
  cross into a nominally pure `nimbus-tenant` domain crate.
- No runtime, storage, system-tenant, quota, credential, or admission behavior
  changed in this phase.

## Tests Added Or Updated

- No tests were added or updated because no production code moved. Existing
  tenant-domain tests were rerun to prove the admission and artifact-verifier
  behavior remains intact after the TSB12 boundary cleanup.

## Verification Commands

- `find crates -maxdepth 1 -type d -name 'nimbus-tenant' -print`
  - Result: pass; no output, so no `crates/nimbus-tenant` directory exists.
- `rg -n "\"crates/nimbus-tenant\"|name = \"nimbus-tenant\"|nimbus_tenant" Cargo.toml Cargo.lock crates`
  - Result: no workspace crate or package manifest references; the only match
    was the existing runtime claim key `nimbus_tenant_id` in
    `crates/nimbus-server/src/tenant/context.rs`, which is not a crate member.
- `rg -n "ProcessArtifactVerifierCommandRunner|std::process|Command::new" crates/nimbus-server/src/tenant crates/nimbus-server/src/tenant.rs`
  - Result: process-launch blocker remains in production tenant-domain code:
    `ProcessArtifactVerifierCommandRunner`, `std::process::{Command, Stdio}`,
    and `Command::new(...)` are still present in artifact-provenance surfaces.
- `rg -n "crate::(adapters|http|ws|system_tenant|local_enforcement|service_manager|sandbox)|axum|tokio|nimbus_engine|nimbus_storage|nimbus_machine" crates/nimbus-server/src/tenant crates/nimbus-server/src/tenant.rs`
  - Result: pass; no matches.
- `cargo test -p nimbus-server tenant:: -- --nocapture`
  - Result: pass; 123 passed, 0 failed, 0 ignored, 757 filtered out in
    `src/lib.rs`; integration test binaries ran 0 selected tests.
- `cargo check --workspace`
  - Result: pass; finished dev profile in 13.28s.
- `cargo fmt --all --check`
  - Result: pass.
- `git diff --check`
  - Result: pass.
- `npm run docs:validate-refs:strict`
  - Result: pass; docs reference validation covered 212 working-tree Markdown
    files.

## Current Evidence

- TSB12 proved the tenant-domain code no longer imports server-local
  `SandboxServiceLaunch`, but it still contains
  `ProcessArtifactVerifierCommandRunner` and `std::process::Command` in
  artifact-provenance code.
- REQ-ADMIT remains satisfied because no admitted decision or lower-layer
  projection code moved in this phase.
- REQ-SYSTEM, REQ-STORAGE, and REQ-QUOTA remain satisfied by non-movement: no
  system-tenant, storage namespace, or quota authority was pulled into a new
  crate or re-exported through a wider API.
- REQ-CRATE is satisfied by withholding extraction until the concrete command
  runner is split from tenant-domain policy types.
- REQ-DOCS is satisfied by this proof note and passing strict docs reference
  validation.

## Remaining Risks

- `nimbus-tenant` remains a future extraction candidate only after concrete
  artifact verifier process execution moves behind an injected adapter owned by
  server/operator code.

## Next Resumable Action

- Start TSB14 by auditing `local_enforcement` and host lifecycle dependencies.
  Extract `nimbus-node` only if real host-lifecycle callers and a clean
  dependency graph both exist.
