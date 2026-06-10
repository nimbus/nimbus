# TSB14 Node Extraction Decision

## Phase

- Phase ID: TSB14
- Status: done
- Git base: `a0d543f8` on `main`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb14-node-extraction-decision.md`

## Requirement IDs

- REQ-ADMIT
- REQ-STATUS
- REQ-CREDS
- REQ-TRUST
- REQ-CRATE
- REQ-DOCS

## Behavior Changed

- No crate was created. TSB14's extraction condition is false in the current
  tree.
- `local_enforcement` has real production consumers for admitted bindings and
  narrow projections, but the host-lifecycle backends are still exercised only
  by local-enforcement tests. There is not yet a production node/control-plane
  caller that starts, stops, or inspects tenant workloads through
  `HostLifecycleBackend`.
- `nimbus-node` extraction is intentionally deferred until the host-lifecycle
  seam has real callers and until the tenant-domain crate boundary is clean.
  Extracting a node crate before `nimbus-tenant` is clean would either pull
  tenant code through `nimbus-server` or force a premature split around the
  artifact verifier process-runner blocker recorded in TSB12/TSB13.

## Tests Added Or Updated

- No tests were added or updated because no production code moved. Existing
  local-enforcement and system-tenant tests were rerun to prove status,
  credentials, trust monotonicity, host lifecycle, and `_nimbus` evidence paths
  remain intact.

## Verification Commands

- `rg --files crates/nimbus-server/src/local_enforcement crates/nimbus-server/src/local_enforcement.rs | sort`
  - Result: listed the local-enforcement root plus `direct_process.rs`,
    `host_lifecycle.rs`, `systemd_transient.rs`, and `tests.rs`.
- `find crates -maxdepth 1 -type d -name 'nimbus-node' -print`
  - Result: pass; no output, so no `crates/nimbus-node` directory exists.
- `rg -n "\"crates/nimbus-node\"|name = \"nimbus-node\"|nimbus_node" Cargo.toml Cargo.lock crates/nimbus-server crates/nimbus-bin crates/nimbus`
  - Result: pass; no machine-node crate or package references. Separate
    Node.js runtime extension names live under `nimbus_node22` and are not the
    machine-node crate discussed by this plan.
- `rg -n "^use |^pub use |mod |pub mod" crates/nimbus-server/src/local_enforcement.rs crates/nimbus-server/src/local_enforcement/*.rs`
  - Result: production local-enforcement imports are `std` collections/future/
    pin/sync, `nimbus_core`, `serde`, `sha2`, and `crate::tenant`; test modules
    import `nimbus_runtime`, `nimbus_sandbox`, and `tokio`.
- `rg -n "crate::local_enforcement|local_enforcement::|LocalEnforcementBinding|TenantWorkloadStatus|HostLifecycleBackend|HostLifecyclePlan|DirectProcessBackend|SystemdTransientUnitBackend" crates/nimbus-server/src --glob '!local_enforcement/**' --glob '!local_enforcement.rs'`
  - Result: production binding/status consumers are
    `runtime_host/mod.rs`, Convex `host_bridge/bridge.rs`,
    `service_manager/activation.rs`, `service_manager/launch.rs`, and
    `system_tenant/records.rs`; host-lifecycle backend start/stop/inspect
    callers were not found outside local-enforcement tests.
- `rg -n "DirectProcessBackend::|SystemdTransientUnitBackend::|\\.start\\(plan\\)|\\.stop\\(workload_id|\\.inspect\\(workload_id|HostLifecycleBackend for" crates/nimbus-server/src --glob '!**/tests.rs' --glob '!tests/**'`
  - Result: backend implementations are production code, but all concrete
    backend construction and start/stop/inspect calls appear after local module
    `mod tests` lines: `direct_process.rs:291`, `systemd_transient.rs:781`,
    and `host_lifecycle.rs:800`.
- `rg -n "nimbus_storage|nimbus_engine|nimbus_machine|axum|crate::(adapters|http|ws|system_tenant|service_manager|runtime_host|execution)" crates/nimbus-server/src/local_enforcement.rs crates/nimbus-server/src/local_enforcement`
  - Result: pass; no server transport, adapter, concrete storage provider,
    runtime-host, service-manager, or control-plane internal dependencies in
    local-enforcement production code.
- `rg -n "tokio|std::process|Command::new" crates/nimbus-server/src/local_enforcement.rs crates/nimbus-server/src/local_enforcement`
  - Result: `tokio` appears only in local test attributes; no process-launch
    command runner appears in local-enforcement code.
- `cargo test -p nimbus-server local_enforcement -- --nocapture`
  - Result: pass; 22 passed, 0 failed, 0 ignored, 858 filtered out in
    `src/lib.rs`; integration test binaries ran 0 selected tests.
- `cargo test -p nimbus-server system_tenant -- --nocapture`
  - Result: pass; 14 passed, 0 failed, 0 ignored, 866 filtered out in
    `src/lib.rs`; integration test binaries ran 0 selected tests.
- `cargo check -p nimbus-server`
  - Result: pass; finished dev profile in 1.05s.
- `cargo fmt --all --check`
  - Result: pass.
- `git diff --check`
  - Result: pass.
- `npm run docs:validate-refs:strict`
  - Result: pass; docs reference validation covered 212 working-tree Markdown
    files.

## Current Evidence

- TSB14 must prove both conditions before extraction: real local-enforcement
  callers including host-lifecycle callers, and a clean dependency graph that
  keeps server transport, adapters, concrete storage providers, and
  control-plane replication internals outside `nimbus-node`.
- The first condition is only partially met. Runtime host, Convex HostBridge,
  and sandbox service-manager paths consume `LocalEnforcementBinding` or its
  projections in production, and system-tenant record projection consumes
  `TenantWorkloadStatus`. However, no production caller currently drives
  `HostLifecycleBackend::validate`, `start`, `stop`, or `inspect`; those calls
  are test-only.
- The second condition is blocked by sequencing. `local_enforcement` production
  code depends on `crate::tenant`; TSB13 intentionally deferred `nimbus-tenant`
  extraction because tenant artifact provenance still contains concrete process
  launch. A clean `nimbus-node` crate cannot depend back on `nimbus-server` for
  tenant-domain types.
- REQ-ADMIT remains satisfied because all production consumers still derive
  local enforcement from `TenantIsolationDecision` and narrow projections.
- REQ-STATUS remains satisfied because node-local status remains observed-only;
  the rerun local-enforcement and system-tenant tests cover assigned node,
  workload UID, observed generation, denied desired-state mutation targets, and
  operator/system-owned `_nimbus` projection.
- REQ-CREDS remains satisfied because credential projection still requires
  admitted workload UID, generation, decision ID, audience, provider, node
  match when node-mediated, invocation match, and redaction metadata.
- REQ-TRUST remains satisfied because runtime-pool trust monotonicity tests
  continue to require teardown for downgrade reuse.
- REQ-CRATE is satisfied by withholding extraction until the real caller and
  dependency prerequisites exist.
- REQ-DOCS is satisfied by this proof note and passing strict docs reference
  validation.

## Remaining Risks

- `nimbus-node` remains a future extraction candidate once a real node or
  control-plane reconciler drives `HostLifecycleBackend` and once
  `nimbus-tenant` can be extracted without process-launch code.
- TSB11's `_nimbus.workload_status` production schema/status DTOs exist, but
  the async write helper remains test-scoped until a real distributed
  node/control-plane writer exists.

## Next Resumable Action

- Run final `git diff --check` and strict docs reference validation, then
  perform the plan completion audit across TSB0-TSB14.
