# Node systemd D-Bus binding

`crates/nimbus-node` talks to systemd over D-Bus to manage tenant workloads as
**transient units**. The live binding (`ZbusSystemdClient`) sits behind the
`SystemdDbusClient` trait and is built on
[`lucab/zbus_systemd`](https://github.com/lucab/zbus_systemd) (pinned
`=0.26000.0`) over [`zbus`](https://github.com/dbus2/zbus) (`5.x`). The
dependency rationale and alternatives are recorded in
`docs/plans/research/systemd-dbus-binding-rust-2026.md`; the execution history
is `docs/plans/archive/node-dbus-client-binding-plan.md`.

> The binding is compiled by default and exercised by CI, but no production
> daemon calls `NodeWorkloadReconciler` yet (TSB14 deferral). Wiring a workload
> source / reconcile loop is a separate plan.

## Bus selection

`ZbusSystemdClient::new(BusKind)` chooses the bus:

- **`BusKind::System`** — PID 1's systemd. Production. Requires `uid 0` or a
  polkit rule granting `org.freedesktop.systemd1.manage-units` on the
  `Manager` interface. `SystemdTransientUnitBackend::linux_systemd_default()`
  uses this on Linux.
- **`BusKind::Session`** — the per-user `systemctl --user` manager. No
  privilege required; used by the Linux integration tests.

The constructor is async and fallible: it opens the connection, caches a
`Manager` proxy, and probes capabilities once. On non-Linux platforms (or when
the system bus cannot be opened) callers fall back to
`SystemdTransientUnitBackend::unavailable(...)`, which fails closed.

## Capability probe

Because the trait's `capabilities()` is synchronous, the probe runs **once in
the constructor** and the result is cached. A cheap `GetUnit("init.scope")`
round-trip distinguishes:

| outcome | capability |
|---|---|
| transport down (I/O, `Disconnected`, `NoServer`, `NoNetwork`) | `dbus_available = false` |
| `UnknownMethod`/`UnknownInterface`/`ServiceUnknown`/interface absent | `transient_units = false` |
| any reply (success, or e.g. `NoSuchUnit`) | reachable — fully capable |

`SystemdTransientUnitBackend` calls `ensure_capable()` before every operation,
so a degraded daemon yields `ResourceExhausted` rather than a panic.

## Signal-correlated job completion (no polling)

`StartTransientUnit`/`StopUnit` return a *job* object path long before the unit
reaches its target state. The binding does **not** treat the returned path as
success. Instead, for every start/stop:

1. `Manager.Subscribe()` — enable signal emission on the connection.
2. Establish the `JobRemoved` stream **before** issuing the method call (zbus
   buffers from subscription, closing the race where a fast job fires the
   signal before we are listening).
3. Call `StartTransientUnit`/`StopUnit`; capture the returned job path.
4. Complete only when the `JobRemoved` whose `job` path matches arrives.

`JobRemoved` `result` classification:

| result | meaning |
|---|---|
| `done` | success |
| `skipped` | unit was already in the requested state |
| `failed` / `canceled` / `timeout` / `dependency` | error |
| anything else (`once`/`merged`/`assert`/…) | error (never silently success) |

## Property encoding

Transient-unit properties are encoded to `Vec<(String, OwnedValue)>` in one
place (`zbus_client/properties.rs`):

| typed property | D-Bus name | signature |
|---|---|---|
| `Description` / `Slice` | same | `s` |
| `Restart` | `Restart` | `s` (`no`/`on-failure`/`always`) |
| `RestartSec` (seconds) | `RestartUSec` | `t` (microseconds) |
| `MemoryMax` / `CpuWeight` / `TasksMax` | `MemoryMax` / `CPUWeight` / `TasksMax` | `t` |
| `ExecStart` | `ExecStart` | `a(sasb)` (argv[0] = program path) |

## Error taxonomy

D-Bus error *replies* arrive as `zbus::Error::MethodError(name, …)` keyed by
the error-name string; only zbus-internal failures use `zbus::Error::FDO`.
`zbus_client/error.rs` maps both to `nimbus_core::Error`:

| D-Bus error | `nimbus_core::Error` |
|---|---|
| I/O / `Disconnected` / `NoServer` / `NoNetwork` / `NoReply` / `Timeout` | `Transport` |
| `AccessDenied` / `AuthFailed` / `InteractiveAuthorizationRequired` | `PermissionDenied` |
| `NoSuchUnit` / `UnknownObject` / `UnknownInterface` / `UnknownMethod` / `ServiceUnknown` / `FileNotFound` | `NotFound` |
| `InvalidArgs` / `InvalidSignature` / `NotSupported` / `Failed` | `InvalidInput` |
| `NoMemory` / `LimitsExceeded` | `ResourceExhausted` |
| (any other) | `Internal` |

`Transport` and `NotFound` were added to `nimbus_core::Error` for this binding.

## Privilege model

Production (`BusKind::System`) requires one of:

- the process runs as `uid 0`, or
- a polkit `.policy` grants the process identity
  `org.freedesktop.systemd1.manage-units` on the `Manager` interface.

Shipping the polkit policy file is owned by the operator-install plan, not this
binding. The session bus (`BusKind::Session`, tests) needs no privilege.

## CI

The `node-dbus-integration` lane (`.github/workflows/ci.yml`, `ubuntu-24.04`)
bootstraps a user-mode systemd (`dbus-user-session`, `loginctl enable-linger`,
`systemctl --user`) and runs the live tests in
`crates/nimbus-node/tests/zbus_systemd_live.rs` on every PR. An unreachable
session bus is a hard failure, never a skip.
