# Tenant Domain And Node Enforcement Boundary Completion Audit

## Phase

- Phase ID: closeout
- Status: done
- Git base: `03c3749c` on `main`

## Files Touched

- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/completion-audit.md`

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
- REQ-ARTIFACT
- REQ-DELETE
- REQ-QUOTA
- REQ-CRATE
- REQ-DOCS

## Behavior Changed

- None. This is a closeout evidence note only.

## Tests Added Or Updated

- None. This closeout reran the focused tenant-boundary and node-enforcement
  verification lanes from the completed phases.

## Completion Gate Audit

| Gate | Evidence |
| --- | --- |
| `tenant_isolation` paths renamed to `tenant`, except preserved security-concept names | Plan rows TSB1-TSB2 are `done`; `tsb1-module-rename.md` and `tsb2-docs.md` record the rename and docs proof. Current `crates/nimbus-server/src/lib.rs` has `mod tenant;` and `TenantIsolation*` type names remain explicit. |
| `TenantIsolationContext` and `TenantIsolationDecision` remain explicit | `crates/nimbus-server/src/tenant.rs`, `context.rs`, and `decision.rs` still expose the explicit security boundary names; final tenant tests pass. |
| `nimbus-tenant` extraction is clean or deferred with proof | `tsb12-tenant-crate-audit.md` and `tsb13-tenant-crate-extraction-decision.md` prove extraction is premature because tenant artifact provenance still owns `ProcessArtifactVerifierCommandRunner` and `std::process::Command`. |
| `_nimbus` is operator/system-owned evidence/control data | `tsb4-local-enforcement.md`, `tsb11-lifecycle-evidence.md`, and final `system_tenant`/tenant-isolation tests prove application/runtime access is denied and system/operator projection is required. |
| Storage/API enforcement remains tenant and stable-`TableId` scoped | TSB0 inventory records storage/table identity baseline; TSB4/TSB11 proof notes record admitted storage projections and system evidence projection; final tenant-isolation conformance covers runtime storage and `_nimbus` denial. |
| `_nimbus` all-tenant/cross-tenant targets are typed and read-only by default | TSB4 and TSB11 proof notes record system/operator authority requirements; final `system_tenant` tests include reserved system tenant route protections. |
| Local enforcement and workload-local supervisor responsibilities are separated | `docs/architecture/server/local-enforcement-boundary.md`, TSB3 proof, and TSB10 docs proof separate tenant admission, local enforcement, host lifecycle, sandbox/runtime primitives, and workload-local supervisor roles. |
| Desired workload state and observed status are explicit enough for future control-plane/node consumers | TSB4 and TSB11 define `TenantWorkloadSpec`, `TenantWorkloadStatus`, status patches, lifecycle evidence, diagnostics, deletion/cleanup, quota, and node lease/heartbeat fields; TSB14 records why node crate extraction waits for real callers. |
| Node-local status/lease/heartbeat/evidence writes are observed-only | TSB4 and TSB11 tests prove assigned-node, UID, generation, decision, and denied desired-state mutation targets; final `local_enforcement` and `system_tenant` tests pass. |
| Deletion/cleanup uses server-owned deletion state and finalizer-like records | TSB4 and TSB11 proof notes cover deletion/finalizer state and cleanup progress; final tenant-isolation conformance proves tenant-a cleanup does not remove tenant-b artifacts/storage. |
| Quotas separate admitted limits from observed usage and retained cleanup bytes | TSB4 and TSB11 proof notes record hard-limit/usage/retained-byte separation; TSB13 confirms no crate extraction widened quota authority. |
| Host lifecycle has typed direct-process, systemd-transient, and Quadlet-export roles | TSB5-TSB7 prove `HostLifecycleBackend`, `DirectProcessBackend`, and `SystemdTransientUnitBackend`; TSB9-TSB10 prove explicit Quadlet export and docs separation. |
| Nimbus node service installation is explicit and tested | TSB8 and TSB10 proof notes cover native systemd and containerized Quadlet node installs, install/status/logs/doctor/uninstall docs, dry-run, user/system mode, and provenance. |
| Default Nimbus OCI image remains a foreground non-systemd app image | TSB8 and TSB10 proof notes reference `docs/operating/container-image.md` and golden tests for foreground entrypoint, UID/GID, health, digest/provenance/SBOM posture, and no systemd-in-container default. |
| Linux dynamic tenant workloads use D-Bus transient units, not `systemd-run` | TSB7 proof records typed D-Bus request construction, allowlisted properties, trusted `ExecStart`, status mapping, and fail-closed unavailable backend behavior. |
| `nimbus compose export quadlet` is reviewed export, not runtime source of truth | TSB9 proof records static operator export behavior, strict mode, warnings, provenance, and no raw tenant systemd text. |
| CLI/operator docs cover node install/status/logs/doctor/uninstall | TSB8 and TSB10 proof notes record docs and golden coverage for those surfaces. |
| Tenant input cannot provide raw host authority or credentials without admission | TSB4-TSB9 and TSB11 proof notes cover raw-value sanitization, allowlists, no raw unit/ExecStart/Quadlet/PodmanArgs fields, no unsafe pass-through, no system-tenant writes, and credential projection checks. |
| Generated host artifacts carry deterministic provenance and fail on unsupported pass-through | TSB8-TSB9 proof notes record native systemd, Quadlet node install, and Quadlet export provenance and strict validation. |
| Static versus dynamic policy lifecycle is documented and tested | TSB4, TSB5, TSB10, and TSB11 proof notes cover recreate-required/static controls, dynamic reload, invalid-update rollback, and host lifecycle status. |
| Runtime/sandbox/pool reuse is monotonic in trust | TSB4-TSB5 and TSB11 proof notes record runtime-pool trust classification and no-downgrade reuse tests. |
| Runtime, sandbox, HostBridge, storage/API, egress, host lifecycle, credentials, and evidence require admitted decisions | TSB4, TSB5, TSB7, TSB11, and final security tests prove lower layers consume `TenantIsolationDecision` or narrow projections. |
| Docs describe the OpenShell-inspired split | TSB3 proof and `docs/architecture/server/local-enforcement-boundary.md` document control-plane intent, tenant admission, local enforcement, sandbox/runtime primitives, and workload-local supervisor roles. |

## Verification Commands

- `rg -n '^\\| TSB[0-9]+ \\|' docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
  - Result: all execution-plan rows TSB0 through TSB14 are `done`.
- `find docs/plans/proof/tenant-domain-and-node-enforcement-boundary -maxdepth 1 -type f -print | sort`
  - Result: proof bundle contains `README.md`, TSB0 through TSB14 proof notes,
    and this closeout audit note.
- `rg -n "Status:|Requirement IDs|Verification Commands|Remaining Risks|Next Resumable Action|Result:" docs/plans/proof/tenant-domain-and-node-enforcement-boundary`
  - Result: every TSB proof note has status, requirement IDs, verification
    commands, result summaries, remaining risks, and next resumable action
    sections.
- `rg -n "tenant_isolation|tenant_isolation::|src/tenant_isolation|mod tenant_isolation" crates docs ARCHITECTURE.md README.md --glob '!docs/plans/archive/**' --glob '!docs/plans/proof/**'`
  - Result: remaining `tenant_isolation` matches are security-concept names,
    drift scanner path, test filters, schema/event strings, policy fields, or
    historical research/plan text; the production module root is `mod tenant`.
- `rg -n "mod tenant;|pub mod tenant|tenant_isolation|TenantIsolationContext|TenantIsolationDecision" crates/nimbus-server/src/lib.rs crates/nimbus-server/src/tenant.rs crates/nimbus-server/src/tenant -g '*.rs'`
  - Result: `crates/nimbus-server/src/lib.rs` has `mod tenant;`, the drift
    scanner remains separate as `tenant_isolation_drift`, and the explicit
    `TenantIsolationContext`/`TenantIsolationDecision` names remain.
- `cargo test -p nimbus-server tenant_isolation -- --nocapture`
  - Result: pass; 20 passed, 0 failed, 0 ignored, 860 filtered out in
    `src/lib.rs`; conformance printed 21 scenarios, 12 allowed, 9 denied;
    integration test binaries ran 0 selected tests.
- `cargo test -p nimbus-server tenant_isolation_drift -- --nocapture`
  - Result: pass; 2 passed, 0 failed, 0 ignored, 878 filtered out in
    `src/lib.rs`; integration test binaries ran 0 selected tests.
- `cargo test -p nimbus-server audit_events -- --nocapture`
  - Result: pass; 7 passed, 0 failed, 0 ignored, 873 filtered out in
    `src/lib.rs`; integration test binaries ran 0 selected tests.
- `cargo test -p nimbus-server local_enforcement -- --nocapture`
  - Result: pass; 22 passed, 0 failed, 0 ignored, 858 filtered out in
    `src/lib.rs`; integration test binaries ran 0 selected tests.
- `cargo test -p nimbus-server system_tenant -- --nocapture`
  - Result: pass; 14 passed, 0 failed, 0 ignored, 866 filtered out in
    `src/lib.rs`; integration test binaries ran 0 selected tests.
- `cargo check --workspace`
  - Result: pass; finished dev profile in 0.82s.
- `cargo clippy -p nimbus-server --all-targets --no-deps`
  - Result: pass; finished dev profile in 13.94s.
- `cargo fmt --all --check`
  - Result: pass.
- `git diff --check`
  - Result: pass.
- `npm run docs:validate-refs:strict`
  - Result: pass; docs reference validation covered 212 working-tree Markdown
    files.

## Remaining Risks

- `nimbus-tenant` and `nimbus-node` are intentionally not extracted. The proof
  bundle records why: tenant artifact provenance still contains concrete
  process-launch code, and host-lifecycle backends do not yet have real
  production node/control-plane callers.
- TSB11's `_nimbus.workload_status` production schema/status DTOs exist, but
  the async write helper remains test-scoped until a real distributed
  node/control-plane writer exists.
- The working tree contains unrelated dirty plan/archive/research/build files
  outside this closeout; they were not staged for this goal.

## Next Resumable Action

- No tenant-domain-and-node-enforcement-boundary phase remains. A future plan
  should split artifact verifier process execution out of tenant-domain code
  before attempting `nimbus-tenant`, then add real node/control-plane
  host-lifecycle callers before attempting `nimbus-node`.
