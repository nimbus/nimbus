# TSB4 Local Enforcement Module

Date: 2026-05-27

## Status

Status: `done`

## Git Base

- Branch: `main`
- Base revision: `36a4062e`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb4-local-enforcement.md`
- `crates/nimbus-server/src/lib.rs`
- `crates/nimbus-server/src/local_enforcement.rs`
- `crates/nimbus-server/src/local_enforcement/tests.rs`
- `crates/nimbus-server/src/tenant/identity.rs`
- `crates/nimbus-server/src/runtime_host/mod.rs`
- `crates/nimbus-server/src/adapters/convex/host_bridge/bridge.rs`
- `crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/runtime_calls.rs`
- `crates/nimbus-server/src/service_manager/activation.rs`
- `crates/nimbus-server/src/service_manager/launch.rs`

## Requirement IDs Touched

- `REQ-ADMIT`: `LocalEnforcementBinding` can be built only from an admitted
  `TenantIsolationDecision` or already materialized `TenantWorkloadSpec`.
  Runtime host storage projections, Convex HostBridge service lookup,
  sandbox service activation/launch, and sandbox egress reload now consume the
  binding or its narrow service/storage projections. Existing tenant gates
  prove direct runtime, HostBridge, sandbox, and storage/API widening still
  fails closed.
- `REQ-SYSTEM`: `TenantSystemEvidenceProjection` derives system evidence
  correlation from the binding, preserving decision ID, workload UID,
  generation, tenant ID, stable workload ID, and redaction metadata. Existing
  tenant-isolation conformance still proves application runtime cannot read
  `_nimbus` and `_nimbus` reads require operator authority.
- `REQ-STORAGE`: runtime host and Convex HostBridge storage access now clone
  the decision-derived storage projection from `LocalEnforcementBinding`.
  Existing authorization and tenant-isolation tests still prove payload tenant
  spoofing and wrong-table Convex document IDs fail.
- `REQ-STATUS`: `NodeStatusAuthorizer` accepts only observed status, lease,
  heartbeat, logs, diagnostics, evidence, and cleanup-progress patches for the
  assigned node, workload UID, observed generation, and decision ID. Tests deny
  wrong node, missing node, stale generation, wrong decision, and every desired
  state target.
- `REQ-CREDS`: credential projection requests require matching workload UID,
  generation, decision ID, node when node-mediated, runtime invocation, admitted
  provider/audience scope, redaction metadata, and no echo-back subject.
- `REQ-LIFECYCLE`: `TenantPolicyArea` to `TenantPolicyLifecycle` records the
  static/dynamic/server-owned classification. Tests prove filesystem and
  placement are recreate-required, HostBridge grants are dynamic reload, and
  deletion/finalizer state is server-owned.
- `REQ-DELETE`: `TenantWorkloadDeletionState` and finalizer records live on the
  desired spec. Tests prove node status can report cleanup progress but cannot
  mutate deletion authority.
- `REQ-QUOTA`: `TenantWorkloadResourcePolicy` carries admitted quota decisions,
  while `TenantObservedResourceUsage` is status evidence. Tests prove observed
  usage does not mutate admitted hard-limit policy and quota hard-limit patches
  are denied as desired-state mutations.
- `REQ-DOCS`: plan state and this proof note record exact files, tests, counts,
  and the next resumable phase.

## Behavior Changed

Behavior changed intentionally but narrowly:

- Added the public in-server `local_enforcement` module as the future
  `nimbus-node` precursor.
- Added decision-derived `TenantWorkloadSpec`, `TenantWorkloadStatus`,
  `TenantWorkloadCondition`, `TenantStorageProjection`,
  `TenantServiceProjection`, `TenantCredentialProjectionPolicy`,
  `TenantWorkloadResourcePolicy`, deletion/finalizer state,
  `NodeStatusAuthorizer`, `TenantEgressReloadRequest`, and
  `LocalEnforcementBinding`.
- Added `TenantWorkloadStableIdentity::sandbox_id()` and
  `TenantWorkloadStableIdentity::invocation_id()` accessors so credential
  projection can bind runtime/sandbox facts to the admitted identity.
- Runtime host storage projections, Convex HostBridge storage/service access,
  sandbox service activation/launch, and egress reload now consume
  `LocalEnforcementBinding` or narrow projections derived from it.

No runtime, sandbox, or storage semantics are intended to change; the existing
decision-derived checks remain the authority source.

## Tests Added Or Updated

Added `crates/nimbus-server/src/local_enforcement/tests.rs` with 7 focused
tests:

- binding materializes decision-derived specs and projections
- node status authorizer accepts observed status for assigned nodes
- node status authorizer rejects wrong node, stale generation, wrong decision,
  missing node, and desired-state mutations
- credential projection requires admitted scope, node, generation, invocation,
  redaction metadata, and no echo-back subject
- deletion and quota state stay server-owned while cleanup progress is observed
- egress reload and policy lifecycle require admitted binding identity
- malformed local enforcement identifiers fail closed

## Verification Commands

Commands run:

```sh
cargo fmt --all
cargo test -p nimbus-server local_enforcement -- --nocapture
cargo test -p nimbus-server authorization -- --nocapture
cargo test -p nimbus-server service_manager -- --nocapture
cargo test -p nimbus-server 'tenant::' -- --nocapture
cargo test -p nimbus-server tenant_isolation -- --nocapture
cargo test -p nimbus-server tenant_isolation_drift -- --nocapture
cargo test -p nimbus-server audit_events -- --nocapture
cargo check -p nimbus-server
cargo clippy -p nimbus-server --all-targets
cargo clippy -p nimbus-server --all-targets --no-deps
cargo fmt --all --check
git diff --check -- crates/nimbus-server/src/lib.rs crates/nimbus-server/src/local_enforcement.rs crates/nimbus-server/src/local_enforcement/tests.rs crates/nimbus-server/src/tenant/identity.rs crates/nimbus-server/src/runtime_host/mod.rs crates/nimbus-server/src/adapters/convex/host_bridge/bridge.rs crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/runtime_calls.rs crates/nimbus-server/src/service_manager/activation.rs crates/nimbus-server/src/service_manager/launch.rs docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb4-local-enforcement.md
npm run docs:validate-refs:strict
```

Results:

- `cargo test -p nimbus-server local_enforcement -- --nocapture`: 7 passed, 0
  failed, 857 filtered out; integration test binaries had 0 matching tests.
- `cargo test -p nimbus-server authorization -- --nocapture`: 12 passed, 0
  failed, 852 filtered out; integration test binaries had 0 matching tests.
- `cargo test -p nimbus-server service_manager -- --nocapture`: 14 passed, 0
  failed, 850 filtered out; integration test binaries had 0 matching tests.
- `cargo test -p nimbus-server 'tenant::' -- --nocapture`: 122 passed, 0
  failed, 742 filtered out; integration test binaries had 0 matching tests.
- `cargo test -p nimbus-server tenant_isolation -- --nocapture`: 20 passed, 0
  failed, 844 filtered out. The conformance harness reported 21 scenarios: 12
  allowed and 9 denied.
- `cargo test -p nimbus-server tenant_isolation_drift -- --nocapture`: 2
  passed, 0 failed, 862 filtered out; integration test binaries had 0 matching
  tests.
- `cargo test -p nimbus-server audit_events -- --nocapture`: 6 passed, 0
  failed, 858 filtered out; integration test binaries had 0 matching tests.
- `cargo check -p nimbus-server`: passed, `Finished dev profile`.
- `cargo clippy -p nimbus-server --all-targets`: failed before the server
  crate due pre-existing `nimbus-core` lint debt:
  `TenantEventKind` large enum variant, `IndexState` derivable default, and
  `TableState` derivable default.
- `cargo clippy -p nimbus-server --all-targets --no-deps`: passed,
  `Finished dev profile`.
- `cargo fmt --all --check`: passed with no output.
- `git diff --check -- ...`: passed with no output.
- `npm run docs:validate-refs:strict`: `docs reference validation: pass (213
  working-tree Markdown files)`.

## Remaining Risks

- Full `cargo clippy -p nimbus-server --all-targets` is currently blocked by
  pre-existing `nimbus-core` clippy failures unrelated to this phase. The
  server-local clippy lane passed with `--no-deps`.
- TSB5 still needs the host lifecycle seam, sanitized systemd unit names,
  host lifecycle status normalization, and runtime-pool trust-class tests.

## Next Resumable Action

Commit the TSB4 local enforcement checkpoint, then start TSB5 by adding the
host lifecycle backend seam under `local_enforcement`.
