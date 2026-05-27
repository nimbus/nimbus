# TSB6 Direct Process Backend

Date: 2026-05-27

## Status

Status: `done`

## Git Base

- Branch: `main`
- Base revision: `a29d2a26`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb6-direct-process.md`
- `crates/nimbus-server/src/local_enforcement.rs`
- `crates/nimbus-server/src/local_enforcement/direct_process.rs`

## Requirement IDs Touched

- `REQ-ADMIT`: `DirectProcessBackend::validate` accepts only
  `HostLifecycleRequest` values lowered through `HostLifecyclePlan::from_binding`
  from a `LocalEnforcementBinding`. Tests prove systemd plans are rejected by
  the DirectProcess backend.
- `REQ-RAW`: `DirectProcessEvidence` records only decision-derived workload ID,
  sanitized unit name, trusted executable path, args, deterministic process ID,
  and explicit platform-dependency booleans. Tests prove deterministic logs and
  no PID 1, D-Bus, Podman, conmon, or KVM dependency.
- `REQ-LIFECYCLE`: `DirectProcessBackend` implements validate/start/stop/inspect
  semantics through the `HostLifecycleBackend` seam and maps running/stopped
  backend states into normalized host lifecycle status and workload status.
- `REQ-DOCS`: plan state and this proof note record exact files, commands,
  result counts, risks, and the next phase.

## Behavior Changed

Behavior changed intentionally but narrowly:

- Added `local_enforcement::direct_process` as a child module.
- Added `DirectProcessBackend`, `DirectProcessEvidence`,
  `HostPlatformDependencies`, and `DirectProcessStatusSnapshot`.
- Re-exported the DirectProcess seam from `local_enforcement`.

The backend is deterministic and in-memory for local smoke/test use; it does
not start OS processes, mutate host service managers, or require PID 1, D-Bus,
Podman, conmon, or KVM.

## Tests Added Or Updated

Added 4 DirectProcess tests under
`crates/nimbus-server/src/local_enforcement/direct_process.rs`:

- backend starts, inspects, and stops workloads
- backend emits deterministic logs and evidence
- backend rejects non-DirectProcess host lifecycle plans
- backend fails closed for unknown workloads

## Verification Commands

Commands run:

```sh
cargo test -p nimbus-server direct_process -- --nocapture
cargo test -p nimbus-server local_enforcement -- --nocapture
cargo check -p nimbus-server
cargo clippy -p nimbus-server --all-targets --no-deps
cargo test -p nimbus-server tenant_isolation -- --nocapture
cargo fmt --all --check
git diff --check -- crates/nimbus-server/src/local_enforcement.rs crates/nimbus-server/src/local_enforcement/direct_process.rs docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb6-direct-process.md
npm run docs:validate-refs:strict
```

Results:

- `cargo test -p nimbus-server direct_process -- --nocapture`: 4 passed, 0
  failed, 870 filtered out; integration test binaries had 0 matching tests.
- `cargo test -p nimbus-server local_enforcement -- --nocapture`: 17 passed,
  0 failed, 857 filtered out; integration test binaries had 0 matching tests.
- `cargo check -p nimbus-server`: passed, `Finished dev profile`.
- `cargo clippy -p nimbus-server --all-targets --no-deps`: passed,
  `Finished dev profile`.
- `cargo test -p nimbus-server tenant_isolation -- --nocapture`: 20 passed, 0
  failed, 854 filtered out. The conformance harness reported 21 scenarios: 12
  allowed and 9 denied.
- `cargo fmt --all --check`: passed with no output.
- `git diff --check -- ...`: passed with no output.
- `npm run docs:validate-refs:strict`: `docs reference validation: pass (212
  working-tree Markdown files)`.

## Remaining Risks

- Full clippy remains subject to the pre-existing `nimbus-core` lint failures
  recorded in TSB4; the server no-deps clippy lane passed.
- `DirectProcessBackend` is deterministic and non-mutating. A real process
  runner can be added later if product requirements need it, but TSB7 is the
  next required runtime host-lifecycle path.

## Next Resumable Action

Commit the TSB6 DirectProcess checkpoint, then start TSB7 by adding the Linux
`SystemdTransientUnitBackend` D-Bus request construction and fail-closed
feature checks.
