# TSB11 Lifecycle Evidence

## Phase

- Phase ID: TSB11
- Status: done
- Git base: `32db5948` on `main`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb11-lifecycle-evidence.md`
- `docs/architecture/server/auth-runtime-trust.md`
- `docs/architecture/server/local-enforcement-boundary.md`
- `crates/nimbus-server/src/local_enforcement.rs`
- `crates/nimbus-server/src/local_enforcement/direct_process.rs`
- `crates/nimbus-server/src/local_enforcement/host_lifecycle.rs`
- `crates/nimbus-server/src/local_enforcement/systemd_transient.rs`
- `crates/nimbus-server/src/local_enforcement/tests.rs`
- `crates/nimbus-server/src/system_tenant.rs`
- `crates/nimbus-server/src/system_tenant/keys.rs`
- `crates/nimbus-server/src/system_tenant/records.rs`
- `crates/nimbus-server/src/system_tenant/schema.rs`
- `crates/nimbus-server/src/system_tenant/tests.rs`
- `crates/nimbus-server/src/tenant/audit_events.rs`
- `crates/nimbus-server/src/tenant/authority.rs`
- `crates/nimbus-server/src/tenant/context.rs`
- `crates/nimbus-server/src/tenant/decision.rs`

## Requirement IDs

- REQ-ADMIT
- REQ-RAW
- REQ-SYSTEM
- REQ-STORAGE
- REQ-STATUS
- REQ-CREDS
- REQ-LIFECYCLE
- REQ-TRUST
- REQ-HOST
- REQ-DELETE
- REQ-QUOTA
- REQ-DOCS

## Behavior Changed

- `TenantWorkloadStatus` now carries typed observed lifecycle evidence, node
  lease/heartbeat IDs, cleanup progress, backend diagnostics, and system
  evidence correlation IDs without changing desired-state authority.
- Host lifecycle normalization now propagates direct-process process IDs and
  systemd transient unit evidence: unit name, job path, process/main PID,
  cgroup path, and journal selectors.
- `TenantWorkloadStatus::metric_labels()` exposes only low-cardinality labels:
  backend kind, phase, and patch target.
- `TenantIsolationEventKind::LifecycleStatus` records lifecycle status as an
  observed audit event with admitted decision ID and high-cardinality evidence
  in event attributes/correlation IDs.
- `_nimbus.workload_status` schema was added for workload-status evidence. The
  current write helper is test-scoped because there is not yet a live node or
  control-plane caller; tests prove it requires system/operator authority and
  rejects application authority before any `_nimbus` write.
- Systemd transient capabilities now produce operator diagnostics with feature
  booleans and actionable failure reasons.
- Architecture docs now state that lifecycle high-cardinality IDs belong in
  evidence/events/system records, not metrics labels.

## Tests Added Or Updated

- Added `lifecycle_evidence_audit_events_keep_high_cardinality_ids_out_of_metric_labels`
  in `crates/nimbus-server/src/local_enforcement/tests.rs`.
- Extended local-enforcement cleanup tests to carry observed cleanup progress
  and reject cleanup progress on the wrong patch target.
- Added `workload_status_projection_requires_system_or_operator_authority` in
  `crates/nimbus-server/src/system_tenant/tests.rs`.
- Updated system-tenant schema coverage for `_nimbus.workload_status`.
- Updated audit-event taxonomy tests for `lifecycle_status`.

## Verification Commands

- `cargo fmt --all`
  - Result: pass.
- `cargo check -p nimbus-server`
  - Result: pass; finished `dev` profile in 11.14s.
- `cargo test -p nimbus-server local_enforcement -- --nocapture`
  - Result: pass; 22 passed, 0 failed, 858 filtered out in the server unit
    test target; integration targets had 0 matching tests.
- `cargo test -p nimbus-server system_tenant -- --nocapture`
  - Result: pass; 14 passed, 0 failed, 866 filtered out in the server unit
    test target; integration targets had 0 matching tests.
- `cargo test -p nimbus-server audit_events -- --nocapture`
  - Result: pass; 7 passed, 0 failed, 873 filtered out in the server unit
    test target; integration targets had 0 matching tests.
- `cargo test -p nimbus-server tenant_isolation -- --nocapture`
  - Result: pass; 20 passed, 0 failed, 860 filtered out in the server unit
    test target; tenant-isolation conformance reported 21 scenarios, 12
    allowed, 9 denied; integration targets had 0 matching tests.
- `cargo clippy -p nimbus-server --all-targets --no-deps`
  - Result: pass; finished `dev` profile in 19.32s.
- `cargo fmt --all --check`
  - Result: pass.
- `git diff --check`
  - Result: pass.
- `npm run docs:validate-refs:strict`
  - Result: pass; docs reference validation passed for 211 working-tree
    Markdown files.

## Current Evidence

- Re-read the execution contract and active TSB11 row in
  `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`.
- Re-read current tenant-separation references:
  `docs/tenant-isolation.md`, `docs/operating/tenant-isolation.md`,
  `docs/architecture/server/auth-runtime-trust.md`,
  `docs/architecture/storage/table-identity.md`,
  `docs/operating/container-image.md`, and
  `docs/architecture/server/local-enforcement-boundary.md`.
- Inspected current implementation surfaces:
  `crates/nimbus-server/src/local_enforcement.rs`,
  `crates/nimbus-server/src/local_enforcement/host_lifecycle.rs`,
  `crates/nimbus-server/src/local_enforcement/direct_process.rs`,
  `crates/nimbus-server/src/local_enforcement/systemd_transient.rs`,
  `crates/nimbus-server/src/tenant/audit_events.rs`,
  `crates/nimbus-server/src/system_tenant/records.rs`,
  `crates/nimbus-server/src/system_tenant/schema.rs`, and
  `crates/nimbus-server/src/system_tenant/tests.rs`.
- REQ-ADMIT evidence: status projection and `_nimbus` write tests require
  `TenantWorkloadSpec`/`TenantSystemEvidenceProjection` derived from an
  admitted decision; `ensure_status_matches` rejects mismatched workload UID,
  decision ID, or observed generation.
- REQ-RAW evidence: lifecycle unit/job/process/cgroup/journal/lease/heartbeat
  IDs are asserted present in audit evidence and absent from metric labels.
- REQ-SYSTEM evidence: application authority is denied before writing
  `_nimbus.workload_status`; operator authority succeeds in the test-scoped
  write helper.
- REQ-STORAGE evidence: no storage transaction semantics changed; system
  evidence remains under explicit `_nimbus` schema/table projection, and
  tenant-isolation conformance still denies application runtime `_nimbus`
  access while allowing operator `_nimbus` route inventory reads.
- REQ-STATUS evidence: node status authorization still rejects wrong node,
  stale generation, wrong decision, missing writer node, and desired-state
  patch targets; new status payloads remain observed-only.
- REQ-CREDS evidence: existing focused credential projection test still covers
  missing grant, wrong audience, wrong node, stale generation, wrong invocation,
  echo-back subject spoofing, and missing redaction metadata.
- REQ-LIFECYCLE evidence: lifecycle status now carries backend evidence and
  existing policy-lifecycle tests still distinguish recreate-required,
  dynamic-reload, and server-owned deletion/finalizer transitions.
- REQ-TRUST evidence: local-enforcement test lane still covers runtime-pool
  monotonic trust reuse and teardown on downgrade.
- REQ-HOST evidence: systemd transient tests still prove request construction,
  trusted `ExecStart`, property allowlisting, stop/inspect mapping, and
  fail-closed capability detection; diagnostics now expose backend feature
  posture and failure reasons.
- REQ-DELETE evidence: cleanup progress is observed-only, cannot target
  deletion authority, and cannot be attached to general status patches.
- REQ-QUOTA evidence: observed usage and retained bytes remain evidence only
  and do not mutate admitted quota hard limits.
- REQ-DOCS evidence: strict docs reference validation passed after updating
  the lifecycle-status event and metric-label/evidence rule.

## Remaining Risks

- There is still no live distributed node/control-plane caller for
  `_nimbus.workload_status`. The schema and status DTOs are production code;
  the async write helper remains test-scoped to avoid adding unused production
  surface before TSB12-TSB14 decide crate and caller boundaries.
- Existing unrelated working-tree changes remain outside this phase, including
  archived sandbox/storage plan moves and the unstaged TSB0 proof-note update.

## Next Resumable Action

- Start TSB12: evaluate whether `nimbus-tenant` can be extracted from the
  stabilized `tenant` module without pulling server transport, adapters,
  concrete storage providers, process launch, host lifecycle implementation,
  or system-tenant persistence across the crate boundary.
