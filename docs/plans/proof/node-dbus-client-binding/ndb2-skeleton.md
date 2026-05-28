# NDB2 Proof — ZbusSystemdClient skeleton + capability probe

Lands the live-client module tree, bus selection, and the capability probe.
Trait methods are stubbed (they return `Error::Internal("… lands in NDB3")`);
signal-correlated completion and property encoding arrive in NDB3.

## Module tree

```
crates/nimbus-node/src/systemd_transient.rs
  #[cfg(feature = "systemd-dbus")] pub mod zbus_client;
crates/nimbus-node/src/systemd_transient/zbus_client/mod.rs
  pub enum BusKind { System, Session }
  pub struct ZbusSystemdClient { connection, manager: ManagerProxy<'static>, capabilities }
crates/nimbus-node/src/lib.rs
  #[cfg(feature = "systemd-dbus")]
  pub use systemd_transient::zbus_client::{BusKind, ZbusSystemdClient};
```

## Constructor surface

- `ZbusSystemdClient::new(bus: BusKind) -> Result<Self>` — async, fallible.
  Opens `Connection::system()` / `Connection::session()`, builds
  `ManagerProxy::new(&conn)`, runs the capability probe once, caches the
  result. Fails only when the bus connection cannot be opened; a reachable
  but degraded daemon yields degraded capabilities (fail-closed via
  `SystemdTransientUnitBackend::ensure_capable`), not a constructor error.
- `from_connection_for_test(Connection)` — `#[cfg(feature = "systemd-dbus-test-bus")]`
  injection seam for tests / NDB5.

**Why the probe runs in the constructor:** the trait method
`fn capabilities(&self) -> SystemdTransientCapabilities` is *synchronous* and
returns an owned struct, so it cannot perform async D-Bus I/O. The probe runs
once in the async constructor and `capabilities()` returns the cached copy.

## Capability probe + classifier

A cheap `GetUnit("init.scope")` round-trip. The decision matrix (in
`classify_probe_error`):

| zbus error | capability result | rationale |
|---|---|---|
| `InputOutput` / `Address` / `Handshake` | `unavailable()` | transport down |
| `MethodError("…Disconnected/NoServer/NoNetwork")` | `unavailable()` | transport down |
| `MethodError("…UnknownMethod/UnknownInterface/ServiceUnknown")` | `available().without_transient_units()` | interface present, method absent |
| `MethodError(other, …)` e.g. `NoSuchUnit` | `available()` | the Manager **replied** → reachable |
| `FDO(Disconnected/NoServer/NoNetwork)` | `unavailable()` | transport down (internal) |
| `FDO(UnknownMethod/UnknownInterface/ServiceUnknown)` | `available().without_transient_units()` | method absent |
| `InterfaceNotFound` | `available().without_transient_units()` | interface absent |
| `Ok(_)` / anything else | `available()` | reachable |

Key correctness point verified against zbus 5.15 source: D-Bus **error
replies** surface as `zbus::Error::MethodError(name, …)` keyed by the error
name string — `zbus::Error::FDO(_)` is only for zbus-internal errors. Both
shapes are handled; the `MethodError` arm is the one real systemd exercises.

## Tests

`cargo test -p nimbus-node --features systemd-dbus,systemd-dbus-test-bus`:

- `transport_errors_mark_dbus_unavailable` — `InputOutput` → `!dbus_available`
- `internal_disconnected_marks_dbus_unavailable` — `FDO(Disconnected)` → `!dbus_available`
- `interface_not_found_disables_transient_units_but_stays_reachable`
- `internal_unknown_method_disables_transient_units`

These cover the classifier decision matrix with synthetic `zbus::Error`
values. The full constructor → probe → `GetUnit` round-trip (and the
`MethodError`/`NoSuchUnit` reply path, which cannot be synthesized without a
live `Message`) is covered by NDB5's Linux session-bus integration test via
the `from_connection_for_test` / `new(BusKind::Session)` seam. An in-process
p2p mock was prototyped but the p2p proxy↔server round-trip hung; the live
test is the higher-value, non-brittle coverage.

Result: `test result: ok. 4 passed; 0 failed` (the 4 `zbus_client::tests`).

## Verifier

Condition 5 (`ZbusSystemdClient` + `BusKind` defined and re-exported) flips to
PASS. Verifier state after NDB2: `5 passed, 5 failed`.
