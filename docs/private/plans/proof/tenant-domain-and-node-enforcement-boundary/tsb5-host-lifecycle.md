# TSB5 Host Lifecycle Seam

Date: 2026-05-27

## Status

Status: `done`

## Git Base

- Branch: `main`
- Base revision: `dad4830b`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb5-host-lifecycle.md`
- `crates/nimbus-server/src/local_enforcement.rs`
- `crates/nimbus-server/src/local_enforcement/host_lifecycle.rs`

## Requirement IDs Touched

- `REQ-ADMIT`: `HostLifecyclePlan::from_binding` and
  `HostLifecycleBackend::validate` require a `LocalEnforcementBinding`, which
  was materialized from an admitted `TenantWorkloadSpec` in TSB4. The fake
  backend test validates and starts only from this binding-derived plan.
- `REQ-RAW`: `TenantWorkloadId` derives from the admitted spec and decision;
  `SystemdUnitName::for_workload` sanitizes the derived ID; raw unit names
  reject path separators, whitespace, semicolons, `..`, and unsupported unit
  extensions. `HostLifecyclePropertySet::from_raw_systemd_properties` accepts
  only the typed allowlist and denies pass-through fields such as `ExecStart`,
  `EnvironmentFile`, `PodmanArgs`, and `Network`.
- `REQ-LIFECYCLE`: `HostLifecycleStatus` normalizes backend states into
  `TenantWorkloadPhase`, stable condition types, and evidence correlation IDs.
  The plan remains construction-only; DirectProcess and systemd D-Bus runtime
  implementations are later phases.
- `REQ-TRUST`: `RuntimePoolTrustState` is monotonic. Tests prove broader
  exposure raises the trust class and stricter reuse requires teardown.
- `REQ-DOCS`: plan state and this proof note record exact files, commands,
  result counts, risks, and the next phase.

## Behavior Changed

Behavior changed intentionally but narrowly:

- Added `local_enforcement::host_lifecycle` as a child module.
- Added `HostLifecycleBackend`, `HostLifecyclePlan`, `HostLifecycleRequest`,
  `TenantWorkloadId`, `SystemdUnitName`, `HostLifecyclePropertySet`,
  `HostLifecycleStatus`, and runtime-pool trust classification types.
- Re-exported the host lifecycle seam from `local_enforcement`.

No product host mutation is introduced in this phase. The fake backend exists
only in tests; real DirectProcess and systemd D-Bus backends remain TSB6 and
TSB7.

## Tests Added Or Updated

Added 6 host lifecycle tests under
`crates/nimbus-server/src/local_enforcement/host_lifecycle.rs`:

- host lifecycle plan derives identity, unit name, executable, and properties
  from an admitted binding
- systemd unit names are sanitized and raw escape shapes are rejected
- host lifecycle property allowlist rejects pass-through escape hatches
- host lifecycle status normalizes backend states to workload status
- runtime pool trust class is monotonic and requires teardown for downgrade
- fake backend validates a plan from a binding and tracks start/inspect/stop
  status

## Verification Commands

Commands run:

```sh
cargo test -p nimbus-server host_lifecycle -- --nocapture
cargo test -p nimbus-server local_enforcement -- --nocapture
cargo check -p nimbus-server
cargo clippy -p nimbus-server --all-targets --no-deps
cargo test -p nimbus-server tenant_isolation -- --nocapture
cargo fmt --all --check
git diff --check -- crates/nimbus-server/src/local_enforcement.rs crates/nimbus-server/src/local_enforcement/host_lifecycle.rs docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb5-host-lifecycle.md
npm run docs:validate-refs:strict
```

Results:

- `cargo test -p nimbus-server host_lifecycle -- --nocapture`: 6 passed, 0
  failed, 864 filtered out; integration test binaries had 0 matching tests.
- `cargo test -p nimbus-server local_enforcement -- --nocapture`: 13 passed,
  0 failed, 857 filtered out; integration test binaries had 0 matching tests.
- `cargo check -p nimbus-server`: passed, `Finished dev profile`.
- `cargo clippy -p nimbus-server --all-targets --no-deps`: passed,
  `Finished dev profile`.
- `cargo test -p nimbus-server tenant_isolation -- --nocapture`: 20 passed, 0
  failed, 850 filtered out. The conformance harness reported 21 scenarios: 12
  allowed and 9 denied.
- `cargo fmt --all --check`: passed with no output.
- `git diff --check -- ...`: passed with no output.
- `npm run docs:validate-refs:strict`: `docs reference validation: pass (212
  working-tree Markdown files)`.

## Remaining Risks

- Full clippy remains subject to the pre-existing `nimbus-core` lint failures
  recorded in TSB4; the server no-deps clippy lane passed.
- This phase defines the seam and fake backend only. TSB6 must add the
  `DirectProcessBackend`, and TSB7 must add the Linux systemd D-Bus transient
  backend.

## Next Resumable Action

Commit the TSB5 host lifecycle seam checkpoint, then start TSB6 by adding the
`DirectProcessBackend` for tests, local smoke harnesses, and non-systemd
developer environments.
