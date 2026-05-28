# NDB0 Baseline Proof

Records the starting state at the point this plan begins (commit
base: `main` as of 2026-05-28). NDB0's commit lands the plan +
verifier + research note + this baseline + routing entry.

## State of the seam

`crates/nimbus-node/src/systemd_transient.rs` defines the
`SystemdDbusClient` trait at lines 15-32:

```rust
pub trait SystemdDbusClient: Send + Sync + 'static {
    fn capabilities(&self) -> SystemdTransientCapabilities;
    fn start_transient_unit<'a>(&'a self, request: SystemdStartTransientUnitRequest)
        -> HostLifecycleFuture<'a, SystemdStartTransientUnitResponse>;
    fn stop_unit<'a>(&'a self, request: SystemdStopUnitRequest)
        -> HostLifecycleFuture<'a, SystemdStopUnitResponse>;
    fn inspect_unit<'a>(&'a self, request: SystemdInspectUnitRequest)
        -> HostLifecycleFuture<'a, SystemdUnitStatus>;
}
```

Backend at lines 35-143:

```rust
pub struct SystemdTransientUnitBackend<C = UnavailableSystemdDbusClient> {
    client: Arc<C>,
}
```

Default type parameter `UnavailableSystemdDbusClient` (lines
243-295) returns `nimbus_core::Error::ResourceExhausted` from every
method.

Mock for tests: `FakeSystemdDbusClient` at lines 795-910
(`#[cfg(test)]` only).

## Why the live binding was deferred

The original plan
`docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
recorded the deferral on lines 582-586:

> "The product implementation should use D-Bus directly... `zbus`
> is the current preferred candidate... dependency selection must
> still be recorded during implementation."

TSB7 completion gate (plan line 957) required only typed request
construction, property allowlist enforcement, status mapping, and
fail-closed behavior. The `FakeSystemdDbusClient` +
`UnavailableSystemdDbusClient` pair satisfied that gate.

The TSB7 proof note at
`docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb7-systemd-transient.md`
records the deferral explicitly:

- Lines 57-59: *"No live zbus/systemd connection is introduced in
  this phase."*
- Lines 111-113: *"A live zbus adapter can be added behind
  `SystemdDbusClient` when product packaging chooses the concrete
  dependency."*

TSB14 (`tsb14-node-extraction-decision.md:28-37`) further recorded
that "host-lifecycle backends are still exercised only by
local-enforcement tests. There is not yet a production
node/control-plane caller."

## Production wiring state

Zero production callers of `SystemdTransientUnitBackend`,
`SystemdDbusClient`, or `NodeWorkloadReconciler`. Confirmed via:

```text
rg -n "SystemdTransientUnitBackend|SystemdDbusClient|NodeWorkloadReconciler" \
   --glob '!**/tests.rs' --glob '!**/tests/**'
```

Results outside `crates/nimbus-node/`:

- `crates/nimbus-server/src/local_enforcement.rs:1` — `pub use
  nimbus_node::*;` (re-export only)
- `crates/nimbus-bin/src/node_service.rs` — operator-install CLI
  (`nimbus node install/status/logs`), shells out to `systemctl`
  via `std::process::Command`; does NOT call the reconciler

## Dependency state

`cargo metadata --format-version=1 | jq '.packages[].name'`:

- No `zbus*` packages
- No `dbus*` packages
- No `systemd*` packages (except internal `nimbus-*` crate names)
- `libsystemd`: not present
- `sd-notify`: not present

Root `Cargo.toml` `[workspace.dependencies]` has no D-Bus or systemd
crate.

`crates/nimbus-node/Cargo.toml` dependencies:

```toml
[dependencies]
nimbus-core = { path = "../nimbus-core" }
nimbus-tenant = { path = "../nimbus-tenant" }
serde.workspace = true
sha2.workspace = true
```

No features defined.

## Test state

All systemd transient tests pass against mocks:

- `crates/nimbus-node/src/systemd_transient.rs` unit tests (lines
  973-1130): 4 tests covering request construction, property
  allowlist, start/stop/inspect round-trip, fail-closed behavior.
- `crates/nimbus-node/src/reconciler.rs` integration tests (lines
  225-783): 4 tests using `ReconcilerSystemdClient` mock.

No tests exercise a live D-Bus daemon.

## Decision recorded

Dependency: `lucab/zbus_systemd =0.26000.0` with features
`["systemd1", "zbus-async-tokio"]`, paired with direct `zbus`
workspace dep for hand-rolled fallback paths.

Full rationale and option comparison:
`docs/plans/research/systemd-dbus-binding-rust-2026.md`.

## Verifier state at NDB0

`bash scripts/verify-node-dbus-binding.sh` expected output after
NDB0 lands:

- Condition 1 (plan exists): PASS
- Condition 2 (routing in CLAUDE.md): PASS
- Condition 3 (NDB0 deliverables): PASS
- Conditions 4-10: FAIL (none of NDB1-NDB7 has landed)

Summary line: `3 passed, 7 failed`.

Each subsequent band flips its condition from FAIL to PASS. NDB7
closes with `10 passed, 0 failed`.
