# TSB1 Module Rename

Date: 2026-05-27

## Status

Status: `in_progress`

## Git Base

- Branch: `main`
- Base revision: `4412a41a`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb1-module-rename.md`
- `crates/nimbus-engine/src/tenant/materialized_reads/backend/loading.rs`
- `crates/nimbus-server/src/tests/tenant_isolation_harness.rs`

Expected implementation files, once the rename begins:

- `crates/nimbus-server/src/tenant_isolation.rs`
- `crates/nimbus-server/src/tenant_isolation/`
- `crates/nimbus-server/src/tenant.rs`
- `crates/nimbus-server/src/tenant/`
- Rust call sites importing `crate::tenant_isolation::*`

## Requirement IDs Touched

- `REQ-ADMIT`: the rename must preserve the explicit
  `TenantIsolationDecision` admission artifact and lower-layer decision-derived
  bindings.
- `REQ-STORAGE`: pre-rename validation found and fixed a materialized serving
  snapshot freshness bug that could make `_nimbus` system evidence reads fail
  during tenant cleanup.
- `REQ-DOCS`: plan and proof state must stay resumable and validate.

No other requirement ID is being closed in this phase unless the rename reveals
an actual behavioral change.

## Behavior Changed

Pre-rename validation found an existing root-cause bug before any module-path
rename: `_nimbus` system evidence reads could fail while deleting a tenant
because loading a new materialized table published against stale already-loaded
tables. `MaterializedServingBackend::load_serving_snapshot_cancellable` now
catches up already-loaded tables from the durable commit log before publishing
the newly loaded table snapshot, preserving the serving snapshot global covered
sequence invariant.

The upcoming module-path rename remains behavior-preserving:

- path changes from `tenant_isolation` to `tenant`
- `TenantIsolation*` type names remain explicit where they mark the security
  boundary
- public re-exports from `nimbus-server` remain behaviorally unchanged

## Tests Added Or Updated

- `crates/nimbus-server/src/tests/tenant_isolation_harness.rs` now preserves
  the structured delete response body in the failure message for the tenant
  cleanup assertion. This made the pre-rename failure actionable and remains a
  useful regression diagnostic.
- No new test case was added yet; the existing conformance harness now covers
  the fixed `_nimbus.ports` materialized-read path during tenant cleanup.

## Pre-Rename Verification

Commands to run before moving files:

```sh
cargo test -p nimbus-server tenant_isolation -- --nocapture
cargo test -p nimbus-server tenant_isolation_drift -- --nocapture
cargo test -p nimbus-server audit_events -- --nocapture
```

Results so far:

- First `cargo test -p nimbus-server tenant_isolation -- --nocapture` failed
  before any Rust rename: 115 passed, 1 failed, 741 filtered out. The failing
  assertion was tenant deletion returning HTTP 500 instead of 204 in
  `tenant_isolation_conformance_suite_covers_runtime_services_storage_and_system_control`.
- Exact failing test rerun reproduced the failure: 0 passed, 1 failed, 856
  filtered out.
- After improving the assertion body, the structured error was
  `service.internal`: `materialized serving snapshot for sequence 256 should be
  available after loading table ports`.
- Root-cause fix in `nimbus-engine` catches up already-loaded materialized
  tables before publishing the newly loaded table snapshot.
- Exact failing test after the fix passed: 1 passed, 0 failed, 856 filtered out,
  and the conformance harness reported 21 scenarios: 12 allowed, 9 denied.
- `cargo test -p nimbus-engine materialized_serving -- --nocapture`: 17 passed,
  0 failed, 253 filtered out.
- `cargo test -p nimbus-server tenant_isolation -- --nocapture`: 116 passed,
  0 failed, 741 filtered out; conformance harness reported 21 scenarios:
  12 allowed, 9 denied.
- `cargo test -p nimbus-server tenant_isolation_drift -- --nocapture`: 2 passed,
  0 failed, 855 filtered out.
- `cargo test -p nimbus-server audit_events -- --nocapture`: 6 passed, 0
  failed, 851 filtered out.

## Post-Rename Verification

Commands to run after the module-path rename:

```sh
cargo test -p nimbus-server tenant_isolation -- --nocapture
cargo test -p nimbus-server tenant_isolation_drift -- --nocapture
cargo test -p nimbus-server audit_events -- --nocapture
git diff --check
npm run docs:validate-refs:strict
```

If filters must change because Rust module paths changed, this proof note will
record both the old and new filters plus the exact output summaries.

## Remaining Risks

- The source tree currently has unrelated user work around Firecracker/libkrun,
  computer-use, and GPU plans. TSB1 must not stage or commit those files.
- Because many test names include the phrase `tenant_isolation`, type names
  should stay stable unless a later reviewed phase changes them.

## Next Resumable Action

Run formatting and diff/docs checks for the pre-rename fix, commit that
checkpoint, then move `crates/nimbus-server/src/tenant_isolation.rs` and
`crates/nimbus-server/src/tenant_isolation/` to the new `tenant` module path.
