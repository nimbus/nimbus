# TNE3 Node Reconciler

- **Phase ID:** TNE3
- **Status:** `done`
- **Git base:** `dcbeaf0d` on `main`
- **Files touched:**
  - `crates/nimbus-server/src/lib.rs`
  - `crates/nimbus-server/src/local_enforcement.rs`
  - `crates/nimbus-server/src/local_enforcement/reconciler.rs`
  - `crates/nimbus-server/src/system_tenant.rs`
  - `crates/nimbus-server/src/system_tenant/keys.rs`
  - `crates/nimbus-server/src/system_tenant/records.rs`
  - `crates/nimbus-server/src/system_tenant/tests.rs`
  - `docs/plans/tenant-and-node-crate-extraction-readiness-plan.md`
  - `docs/plans/proof/tenant-node-extraction-readiness/tne3-node-reconciler.md`
- **Requirement IDs touched:** REQ-ADMIT, REQ-RAW, REQ-SYSTEM,
  REQ-STATUS, REQ-CREDS, REQ-HOST, REQ-TRUST, REQ-DOCS

## Baseline Findings

- `HostLifecycleBackend::validate`, `start`, `stop`, and `inspect` are
  implemented by local-enforcement backends and covered by tests, but no
  production reconciler owns the control loop yet.
- `TenantWorkloadSpec` and `TenantWorkloadStatus` already provide desired and
  observed shapes with generation, decision, node, evidence, and deletion
  safeguards.
- `_nimbus` status persistence remains server-owned; TNE3 must add only a
  narrow observed-status writer trait in local enforcement and keep concrete
  system-tenant persistence in server wiring.

## Intended Change

- Add `NodeWorkloadReconciler` production code that consumes desired
  `TenantWorkloadSpec`, calls host lifecycle `validate`, `inspect`, `start`,
  and `stop`, and emits normalized observed-only `TenantWorkloadStatus`.
- Add a `StatusEvidenceWriter` trait that accepts observed status/evidence
  only.
- Implement the writer in server wiring against system-tenant persistence
  without moving `_nimbus` storage into local enforcement.

## Implementation

- Added `local_enforcement::reconciler` with:
  - `NodeWorkloadReconciler<B, W>`
  - `NodeWorkloadDesiredState`
  - `NodeWorkloadReconcileAction`
  - `NodeWorkloadReconcileOutcome`
  - `StatusEvidenceWrite<'a>`
  - `StatusEvidenceWriter`
- Desired state is derived only from server-owned
  `TenantWorkloadSpec::deletion()`:
  - `Active` reconciles toward running.
  - `Deleting { .. }` reconciles toward stopped.
- Production reconcile flow now calls:
  - `self.backend.validate(binding, request)?`
  - `self.backend.inspect(workload_id.clone()).await`
  - `self.backend.start(plan.clone()).await?`
  - `self.backend.stop(workload_id.clone()).await?`
  - post-start/post-stop `inspect(...)` before writing evidence.
- Status writing is inverted:
  - local enforcement owns only `StatusEvidenceWriter`.
  - `StatusEvidenceWrite::new(...)` validates
    `TenantSystemEvidenceProjection::ensure_status_matches(status)`.
  - server-owned `SystemTenantStatusEvidenceWriter` implements the trait and
    calls `record_tenant_workload_status_async(...)`.
  - `_nimbus` persistence and `Service` remain in `system_tenant`, not
    `local_enforcement`.

## Requirement Evidence

- **REQ-HOST:** production code in
  `crates/nimbus-server/src/local_enforcement/reconciler.rs` calls
  `validate`, `inspect`, `start`, and `stop` through `HostLifecycleBackend`.
  Tests cover direct-process and systemd transient reconciliation.
- **REQ-SYSTEM:** `StatusEvidenceWriter` is a narrow local-enforcement trait;
  the concrete `_nimbus` writer is server-owned
  `SystemTenantStatusEvidenceWriter`. The system-tenant test proves
  application authority is denied and operator authority succeeds.
- **REQ-STATUS:** reconciler writes only authorized `TenantWorkloadStatus`.
  Stale generation is rejected before `StatusEvidenceWrite` can be created.
  Existing status-authorizer tests still reject spec, labels, policy, grants,
  quota hard limits, placement, credentials, admission, deletion authority, and
  user data targets.
- **REQ-ADMIT / REQ-RAW:** host lifecycle requests still derive workload ID and
  systemd unit names from admitted binding identity. The systemd reconciler
  test proves `ExecStart` is generated from trusted server input and raw
  systemd escape hatches such as `EnvironmentFile` do not pass through.
- **REQ-CREDS:** local-enforcement credential projection tests still require
  admitted scope, node, generation, invocation, no subject echo-back, and
  redaction metadata.
- **REQ-TRUST:** runtime-pool trust monotonicity test remains in the broader
  local-enforcement lane and still passes.

## Verification Log

- `cargo test -p nimbus-server local_enforcement::reconciler -- --nocapture`
  - Result: 5 passed, 0 failed, 804 filtered out; integration targets selected
    0 tests.
  - Covered direct-process start/stop/write, systemd transient
    start/stop/write, unavailable systemd fail-closed, stale generation
    rejection, and desired-state derivation from deletion state.
- `cargo test -p nimbus-server system_tenant::tests::workload_status_projection_requires_system_or_operator_authority -- --nocapture`
  - Result: 1 passed, 0 failed, 808 filtered out; integration targets selected
    0 tests.
  - Proves `SystemTenantStatusEvidenceWriter` denies application authority and
    writes `_nimbus.workload_status` only with operator authority.
- `cargo test -p nimbus-server local_enforcement -- --nocapture`
  - Result: 27 passed, 0 failed, 782 filtered out; integration targets selected
    0 tests.
  - Covers existing local-enforcement binding, status, credential, lifecycle,
    direct-process, systemd transient, and trust-class tests plus the new
    reconciler tests.
- `cargo test -p nimbus-server system_tenant -- --nocapture`
  - Result: 14 passed, 0 failed, 795 filtered out; integration targets selected
    0 tests.
  - Covers `_nimbus` tenant protection, schema/system projection, route
    security, and workload status projection.
- `cargo check -p nimbus-server`
  - Result: pass; finished dev profile in 9.81s.
- `cargo clippy -p nimbus-server --all-targets --no-deps`
  - Result: pass; finished dev profile in 18.47s.
- `git diff --check`
  - Result: pass with no whitespace errors.
- `cargo fmt --all --check`
  - Result: pass.
- `npm run docs:validate-refs:strict`
  - Result: pass; 213 working-tree Markdown files checked.
- Production caller audit:
  - `crates/nimbus-server/src/local_enforcement/reconciler.rs` contains the
    only non-test reconciler calls to `self.backend.validate(...)`,
    `self.backend.inspect(...)`, `self.backend.start(...)`, and
    `self.backend.stop(...)`.
  - `crates/nimbus-server/src/system_tenant/records.rs` contains
    `impl StatusEvidenceWriter for SystemTenantStatusEvidenceWriter`.

## Remaining Risks

- TNE3 intentionally does not extract `nimbus-node`; it only creates the real
  production caller and writer inversion needed to make TNE4 meaningful.
- No daemon loop is introduced yet. The reconciler is production code and
  public server API surface, but scheduler/control-plane polling remains a
  future integration concern outside this extraction-readiness phase.

## Next Resumable Action

- Begin TNE4 extraction of `crates/nimbus-node`, preserving the writer
  inversion proven in TNE3.
