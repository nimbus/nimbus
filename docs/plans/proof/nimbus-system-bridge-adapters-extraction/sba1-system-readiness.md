# SBA1 System Readiness Proof

Date: 2026-05-27
Status: completed

## Scope

Prepare `nimbus-system` by removing adapter-private deployment evidence inputs
from `system_tenant` before any crate extraction.

## Current Blockers

- `crates/nimbus-server/src/system_tenant/records.rs` accepts
  `crate::adapters::convex::ConvexRegistryDeploySummary` in
  `record_convex_deployment_state_async`.
- `deployment_bundle_sha256` also accepts
  `crate::adapters::convex::ConvexRegistryDeploySummary`.
- Startup and deploy activation call the system writer from `router.rs` and
  `http/deploy.rs`.

## Intended Moves

- Introduce neutral system deployment record input structs owned by
  `system_tenant`.
- Rename the system writer to an adapter-neutral deployment state writer.
- Move Convex-specific summary conversion out of system code and keep it near
  the adapter/composition call sites.
- Add a focused system test proving deployment records persist through the
  neutral input shape.

## Forbidden Imports

SBA1 is not complete while any production file under
`crates/nimbus-server/src/system_tenant` imports or names:

- `ConvexRegistryDeploySummary`
- `ConvexFunctionDeploySummary`
- `ConvexHttpRouteDeploySummary`
- `crate::adapters`

## Task Checklist

- [x] SBA1.1 Introduce neutral system deployment record inputs.
- [x] SBA1.2 Move adapter-specific conversion out of system code.
- [x] SBA1.3 Define system record inputs for required evidence classes or prove
      existing typed inputs are sufficient.
- [x] SBA1.4 Keep `_nimbus` writes centralized.
- [x] SBA1.5 Prove observed-only node status.

## Changes Made

- Added neutral system deployment evidence inputs in
  `crates/nimbus-server/src/system_tenant/records.rs`:
  `SystemDeploymentRecordInput`, `SystemDeploymentFunctionRecordInput`, and
  `SystemDeploymentHttpRouteRecordInput`.
- Replaced `record_convex_deployment_state_async` with adapter-neutral
  `record_deployment_state_async`.
- Changed fallback bundle hashing from the Convex-specific domain separator to
  `nimbus-system-deployment-record-v1`.
- Added Convex-specific conversion in
  `crates/nimbus-server/src/adapters/convex/registry/deploy_summary.rs` via
  `ConvexRegistryDeploySummary::system_deployment_record_input`.
- Updated startup and deploy activation call sites to convert Convex deployment
  summaries before entering `system_tenant`.
- Added
  `record_deployment_state_projects_neutral_bundle_and_functions`, which proves
  neutral deployment inputs write bundle/function records and remove stale
  bundle/function documents.

## Record Input Decision

SBA1 introduced a new neutral deployment record input because deployment was the
only system evidence path accepting an adapter-private shape.

The other evidence paths already use owner-appropriate typed inputs:

- listener evidence: `record_listener_state_async` accepts scalar listener
  fields;
- scheduler evidence: scheduled and cron writers accept `ScheduledJob`,
  `ScheduledJobResult`, `CronJob`, and `TenantId`;
- table evidence: `record_table_state_async` accepts `TenantId` and
  `TableName`, then reads the authoritative table state through `Service`;
- machine evidence: machine writers accept `MachineConfigRecord` and
  `MachineStateRecord`;
- sandbox/service evidence: `record_service_handle_async` accepts
  `SandboxHandle`;
- node evidence: workload status writes accept
  `TenantSystemEvidenceProjection` plus `TenantWorkloadStatus`.

No new adapter-specific input class remained necessary for these paths in SBA1.

## `_nimbus` Write Audit

Command:

```bash
rg -n "record_.*_state|upsert_system_document|system_tenant" crates/nimbus-server/src/adapters -g '*.rs'
```

Result summary:

- Adapter code calls `crate::system_tenant` interfaces for scheduler,
  subscription, run, deployment-conversion, and system-tenant checks.
- No adapter file calls `upsert_system_document_async` directly.
- The only deployment-specific adapter conversion is
  `ConvexRegistryDeploySummary::system_deployment_record_input`, which produces
  neutral system input values before the system writer is called by server
  composition.

## Observed-Only Node Status Proof

Node status persistence remains observed-only:

- `record_tenant_workload_status_async` requires
  `ensure_system_or_operator_authority("_nimbus workload status projection")`.
- The writer calls `projection.ensure_status_matches(status)` before
  persistence.
- The persisted document includes observed generation, node id, phase, target,
  lifecycle evidence, node observation ids, cleanup progress, correlation ids,
  redaction metadata, diagnostics, and timestamp.
- The writer does not accept or persist workload spec, policy, grants, quota,
  placement, or credentials.
- The focused system-tenant test
  `workload_status_projection_requires_system_or_operator_authority` passed in
  the SBA1 verification run.

## Verification Log

1. Forbidden system import audit:

   ```bash
   rg -n "ConvexRegistryDeploySummary|ConvexFunctionDeploySummary|ConvexHttpRouteDeploySummary|crate::adapters|nimbus-convex-deploy-summary" crates/nimbus-server/src/system_tenant -g '*.rs'
   ```

   Result: no matches, exit code 1.

2. Initial focused system-tenant test attempt:

   ```bash
   cargo test -p nimbus-server system_tenant -- --nocapture
   ```

   Result: failed before tests due to environment disk pressure:
   `rustc-LLVM ERROR: IO failure on output stream: No space left on device`.

3. Disk recovery:

   ```bash
   df -h .
   du -sh target
   cargo clean
   df -h .
   ```

   Result: filesystem had 365 MiB available; `target` was 116 GiB.
   `cargo clean` removed 135239 generated files / 167.8 GiB total.
   After cleanup, filesystem had 114 GiB available.

4. Focused system-tenant tests after cleanup:

   ```bash
   cargo test -p nimbus-server system_tenant -- --nocapture
   ```

   Result: passed. Unit tests reported 15 passed, 0 failed, 768 filtered out;
   integration filters reported 0 passed, 0 failed, 23 and 32 filtered out.

5. Deploy call-site tests:

   ```bash
   cargo test -p nimbus-server deploy -- --nocapture
   ```

   Result: passed. Unit tests reported 10 passed, 0 failed, 773 filtered out;
   integration filters reported 0 passed, 0 failed, 23 and 32 filtered out.

6. Server check:

   ```bash
   cargo check -p nimbus-server
   ```

   Result: passed, finished dev profile in 30.93s.

7. Formatting:

   ```bash
   cargo fmt --all --check
   ```

   Result: passed.

## Closeout

SBA1 is complete. `system_tenant` production code no longer depends on
adapter-private deployment summaries, deployment evidence now enters through a
neutral system record input, `_nimbus` writes remain centralized behind
system interfaces, and observed-only node status enforcement remains covered by
focused tests.
