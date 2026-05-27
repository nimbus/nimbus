# TSB7 Systemd Transient Backend

Date: 2026-05-27

## Status

Status: `done`

## Git Base

- Branch: `main`
- Base revision: `801eec75`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb7-systemd-transient.md`
- `crates/nimbus-server/src/local_enforcement.rs`
- `crates/nimbus-server/src/local_enforcement/host_lifecycle.rs`
- `crates/nimbus-server/src/local_enforcement/systemd_transient.rs`

## Requirement IDs Touched

- `REQ-ADMIT`: `SystemdTransientUnitBackend::validate` requires a
  `LocalEnforcementBinding` and lowers only a binding-derived
  `HostLifecyclePlan`. Tests prove the backend rejects DirectProcess plans and
  builds `StartTransientUnit` requests from the admitted plan.
- `REQ-RAW`: `SystemdStartTransientUnitRequest` contains typed properties,
  generated `ExecStart` from the trusted executable and args on the plan,
  sanitized unit name, cgroup path, and journal selectors. Tests prove raw
  `ExecStart` is rejected by the allowlist before backend validation.
- `REQ-LIFECYCLE`: start maps D-Bus submission to observed `Bound` /
  `UnitSubmitted`; inspect and stop map fake systemd active/inactive states
  into normalized host lifecycle status. Feature gaps fail closed before host
  mutation.
- `REQ-HOST`: the backend calls a typed `SystemdDbusClient` with
  `StartTransientUnit`, stop, and inspect requests. Tests prove request
  construction, restart/memory/cgroup/journal correlation, stop/inspect
  mapping, and D-Bus/transient-unit/service-unit unavailable failures.
- `REQ-DOCS`: plan state and this proof note record exact files, commands,
  result counts, risks, and the next phase.

## Behavior Changed

Behavior changed intentionally but narrowly:

- Added `local_enforcement::systemd_transient` as a child module.
- Added `SystemdTransientUnitBackend`, `SystemdDbusClient`,
  `SystemdStartTransientUnitRequest`, `SystemdStopUnitRequest`,
  `SystemdInspectUnitRequest`, `SystemdUnitStatus`, typed D-Bus property
  projection, journal selectors, capabilities, and an unavailable fail-closed
  client.
- Added a `HostLifecycleStatus::new_for_backend` constructor for backend status
  normalization.
- Re-exported the systemd transient seam from `local_enforcement`.

No live zbus/systemd connection is introduced in this phase. The product seam is
typed for D-Bus request construction and fail-closed feature detection; TSB11
will wire richer evidence into system tenant diagnostics.

## Tests Added Or Updated

Added 4 systemd transient tests under
`crates/nimbus-server/src/local_enforcement/systemd_transient.rs`:

- `StartTransientUnit` request uses trusted `ExecStart`, allowlisted
  properties, restart policy, cgroup path, and journal selectors
- backend rejects disallowed raw properties and wrong backend plans
- backend calls the fake D-Bus client and maps start/inspect/stop status
- backend fails closed when D-Bus, transient units, or service units are
  unavailable

## Verification Commands

Commands run:

```sh
cargo test -p nimbus-server systemd_transient -- --nocapture
cargo test -p nimbus-server local_enforcement -- --nocapture
cargo check -p nimbus-server
rg -n "systemd-run" crates/nimbus-server/src/local_enforcement crates/nimbus-server/src/service_manager crates/nimbus-server/src/runtime_host
cargo clippy -p nimbus-server --all-targets --no-deps
cargo test -p nimbus-server tenant_isolation -- --nocapture
cargo fmt --all --check
git diff --check -- crates/nimbus-server/src/local_enforcement.rs crates/nimbus-server/src/local_enforcement/host_lifecycle.rs crates/nimbus-server/src/local_enforcement/systemd_transient.rs docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb7-systemd-transient.md
npm run docs:validate-refs:strict
```

Results:

- `cargo test -p nimbus-server systemd_transient -- --nocapture`: 4 passed, 0
  failed, 874 filtered out; integration test binaries had 0 matching tests.
- `cargo test -p nimbus-server local_enforcement -- --nocapture`: 21 passed,
  0 failed, 857 filtered out; integration test binaries had 0 matching tests.
- `cargo check -p nimbus-server`: passed, `Finished dev profile`.
- `rg -n "systemd-run" crates/nimbus-server/src/local_enforcement crates/nimbus-server/src/service_manager crates/nimbus-server/src/runtime_host`: no matches; `rg` exited 1 as expected for no matches.
- `cargo clippy -p nimbus-server --all-targets --no-deps`: passed,
  `Finished dev profile`.
- `cargo test -p nimbus-server tenant_isolation -- --nocapture`: 20 passed, 0
  failed, 858 filtered out. The conformance harness reported 21 scenarios: 12
  allowed and 9 denied.
- `cargo fmt --all --check`: passed with no output.
- `git diff --check -- ...`: passed with no output.
- `npm run docs:validate-refs:strict`: `docs reference validation: pass (212
  working-tree Markdown files)`.

## Remaining Risks

- Full clippy remains subject to the pre-existing `nimbus-core` lint failures
  recorded in TSB4; the server no-deps clippy lane passed.
- The backend is a typed D-Bus seam with fake/unavailable clients. A live zbus
  adapter can be added behind `SystemdDbusClient` when product packaging
  chooses the concrete dependency.
- TSB11 still needs richer unit/job/process/cgroup/journal IDs in
  `TenantWorkloadStatus`, audit, diagnostics, and `_nimbus` evidence.

## Next Resumable Action

Commit the TSB7 systemd transient checkpoint, then start TSB8 by adding native
systemd and containerized Quadlet node service installation support.
