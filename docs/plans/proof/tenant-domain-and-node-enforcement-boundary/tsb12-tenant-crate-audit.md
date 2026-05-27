# TSB12 Tenant Crate Audit

## Phase

- Phase ID: TSB12
- Status: done
- Git base: `05e74790` on `main`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb12-tenant-crate-audit.md`
- `crates/nimbus-server/src/tenant/decision.rs`
- `crates/nimbus-server/src/tenant/tests.rs`
- `crates/nimbus-server/src/service_manager/launch.rs`

## Requirement IDs

- REQ-ADMIT
- REQ-CRATE
- REQ-DOCS

## Behavior Changed

- The audit found and removed one narrow server-local dependency: tenant
  service-access validation now accepts `nimbus_sandbox::SandboxSpec` directly
  instead of the server-local `SandboxServiceLaunch` wrapper.
- `SandboxServiceManager::start_service_for_decision` still owns the concrete
  `SandboxServiceLaunch`; it now passes the launch's narrow `SandboxSpec`
  projection into `TenantServiceAccessDecision`.
- `nimbus-tenant` extraction is intentionally deferred. The production tenant
  module no longer depends on server-local sandbox launch wrappers, but it still
  contains the concrete artifact verifier command runner that uses
  `std::process::Command`. Pulling that into a tenant-domain crate would violate
  REQ-CRATE's no process-launch boundary.

## Tests Added Or Updated

- Updated the tenant service-access tests to build `SandboxSpec` directly and
  assert the same tenant, service, and backend mismatch failures before sandbox
  launch.
- No new crate was created because the dependency proof is not clean.

## Verification Commands

- `rg --files crates/nimbus-server/src/tenant crates/nimbus-server/src/tenant.rs | sort`
  - Result: listed `tenant.rs` plus 24 tenant-domain files under
    `crates/nimbus-server/src/tenant/`.
- `rg -n "SandboxServiceLaunch|ensure_sandbox_launch_matches|ensure_sandbox_spec_matches" crates/nimbus-server/src`
  - Result: `ensure_sandbox_launch_matches` has no matches; tenant production
    code only exposes `ensure_sandbox_spec_matches`; `SandboxServiceLaunch`
    remains in the server-local sandbox/service-manager surfaces and tests.
- `rg -n "std::process|Command::new|ProcessArtifactVerifierCommandRunner|crate::(adapters|http|ws|system_tenant|local_enforcement|service_manager|sandbox)|axum|tokio|nimbus_engine|nimbus_storage|nimbus_machine" crates/nimbus-server/src/tenant.rs crates/nimbus-server/src/tenant`
  - Result: no server transport, adapter, system-tenant, local-enforcement,
    service-manager, sandbox, axum, tokio, engine, storage, or machine matches
    in production tenant-domain files; remaining matches are
    `ProcessArtifactVerifierCommandRunner` and `std::process::Command` in
    artifact-provenance code.
- `rg -n "use (crate|super|nimbus_|serde|std|sha2|anyhow)|crate::|super::" crates/nimbus-server/src/tenant.rs crates/nimbus-server/src/tenant`
  - Result: production tenant-domain dependencies are limited to intra-tenant
    module references, `nimbus_core`, `nimbus_runtime`, `nimbus_sandbox`,
    `serde`, `serde_json`, `sha2`, and standard collections/net/path/sync/time
    primitives, except for the concrete process-launch verifier blocker.
- `cargo test -p nimbus-server tenant:: -- --nocapture`
  - Result: pass; 123 passed, 0 failed, 0 ignored, 757 filtered out in
    `src/lib.rs`; integration test binaries ran 0 selected tests.
- `cargo test -p nimbus-server service_manager -- --nocapture`
  - Result: pass; 14 passed, 0 failed, 0 ignored, 866 filtered out in
    `src/lib.rs`; integration test binaries ran 0 selected tests.
- `cargo check -p nimbus-server`
  - Result: pass; finished dev profile in 4.39s.
- `cargo clippy -p nimbus-server --all-targets --no-deps`
  - Result: pass; finished dev profile in 10.25s.
- `cargo fmt --all --check`
  - Result: pass.
- `git diff --check`
  - Result: pass.
- `npm run docs:validate-refs:strict`
  - Result: pass; docs reference validation covered 211 working-tree Markdown
    files.

## Final Evidence

- Inspected `crates/nimbus-server/src/tenant.rs` and all files under
  `crates/nimbus-server/src/tenant/`.
- Ran dependency/caller inventory searches for server/adapters/axum/storage
  provider/process-launch/host-lifecycle/system-tenant dependencies.
- Found no `axum`, `tokio`, `nimbus_engine`, `nimbus_storage`,
  `nimbus_machine`, `system_tenant`, `local_enforcement`, server transport, or
  adapter dependencies in production tenant-domain files.
- Found a production `crate::sandbox::SandboxServiceLaunch` dependency in
  `tenant/decision.rs`; it has been removed and replaced by a direct
  `SandboxSpec` projection check.
- Found a process-launch blocker in `tenant/artifact_provenance.rs`:
  `ProcessArtifactVerifierCommandRunner` uses `std::process::Command` and
  belongs outside a pure `nimbus-tenant` crate boundary.
- REQ-ADMIT remains satisfied for the touched path: service-manager code still
  receives a `TenantIsolationDecision`, creates a local-enforcement binding,
  asks for a narrow service-access projection, and tenant-domain code validates
  only the supplied `SandboxSpec` and backend kind.
- REQ-CRATE is satisfied by the no-extraction decision: the audit proves a
  server-local dependency was removed but process-launch code still blocks a
  clean tenant crate boundary.
- REQ-DOCS is satisfied by this proof note and the passing docs reference
  validation.

## Remaining Risks

- The artifact verifier default backends still instantiate
  `ProcessArtifactVerifierCommandRunner` inside the tenant module. A future
  extraction should first split concrete command execution into a server,
  operator, or verifier-adapter layer and leave the tenant crate with injected
  verifier traits and pure policy/result types.

## Next Resumable Action

- Start TSB13 by recording that `crates/nimbus-tenant` must not be created from
  the current boundary. Close TSB13 as a conditional no-extraction decision
  unless the concrete artifact verifier process-runner is split first.
