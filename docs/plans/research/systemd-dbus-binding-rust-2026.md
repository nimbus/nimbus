# Systemd D-Bus Binding for Rust — May 2026 Decision

Decision record for the concrete dependency that fills the
`SystemdDbusClient` trait at
`crates/nimbus-node/src/systemd_transient.rs:15-32`. Companion
execution plan: `docs/plans/node-dbus-client-binding-plan.md`.

## Context

The TSB7 wave of
`docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
recorded the unresolved dependency choice on lines 582-586:

> "The product implementation should use D-Bus directly, with
> `systemd-run` kept in docs, diagnostics, and manual reproduction
> snippets. The D-Bus adapter should use a small, typed Rust seam.
> `zbus` is the current preferred candidate because it is stable,
> async-friendly, and avoids shell parsing; dependency selection must
> still be recorded during implementation."

This note records the selection.

## Decision

Use **`lucab/zbus_systemd`** with `features = ["systemd1",
"zbus-async-tokio"]`, default-features off. Pin exact at
`=0.26000.0`. Pair with `zbus` as a direct workspace dependency so
that any hand-rolled `#[proxy]` traits (e.g., for surfaces not yet
covered by `zbus_systemd`) live in the same dep graph.

Reject `unitbus`, `dbus`+`dbus-tokio`, rust-`systemd` (FFI),
`systemd-zbus`, and `systemd-run`. Each rejection is documented
below.

## Options considered

| Crate | Last release | DL/mo | License | Async | Pure Rust | Manager1 coverage | Verdict |
|---|---|---|---|---|---|---|---|
| `zbus_systemd` (lucab) | 2026-03-28 (`0.26000.0`) | ~85k | MIT/Apache | tokio + smol (feature) | yes | full auto-gen, 14 interfaces | **selected** |
| `zbus` direct + `#[proxy]` | 2026-04-26 (`5.15.0`) | 4.7M | MIT | tokio + smol | yes | only what you write | fallback for niche cases |
| `unitbus` (lvillis) | 2026-01-23 (`0.1.8`) | low | MIT | yes | yes (on zbus) | partial, opinionated SDK | reject — 0 stars, unknown author, hobby-grade |
| `dbus` + `dbus-tokio` | 2026/2023 | 2.5M/182k | Apache/MIT | retrofitted (tokio bridge stale 3y) | no — `libdbus-1` FFI | n/a (hand-write) | reject — wrong shape for 2026 async |
| `systemd` (rust-systemd, Cody Schafer) | 2025-07-19 (`0.10.1`) | 318k | **LGPL-2.1+ WITH GCC-exception-2.0** | no | no — `libsystemd.so` FFI | n/a | reject — license-toxic, FFI |
| `systemd-zbus` (flukejones) | 2025-05-20 (`5.3.2`) | ~12k | Apache | yes | yes | partial, hand-curated | reject — single maintainer, lower adoption |
| `systemd-run` (Xi Ruoyao) | 2024-11-06 (`0.9.0`) | low | MIT/Apache | yes | yes | transient-only | reject — self-described "highly unstable" |

Companion crates (orthogonal — not D-Bus client surface):

- **`sd-notify`** (Laurențiu Nicola, `0.5.0`, MIT/Apache, ~602k
  DL/mo) — `READY=1`/watchdog from the node daemon itself toward
  systemd. Pull in when the node daemon plan lands. Not part of NDB.
- **`libsystemd`** (lucab, `0.7.2`, MIT/Apache) — pure-Rust
  reimplementation of `sd_notify`/activation/machine-id protocols.
  Companion to `sd-notify` for activation-socket inheritance.

## Trust signals for the selection

`zbus_systemd` is maintained by **Luca Bruno (lucab)** — ex-CoreOS,
now Red Hat, long-time Debian systemd-adjacent contributor and
owner of `libsystemd-rs` — co-owned by **Zeeshan Ali Khan**, the
zbus author. Auto-generated from current systemd D-Bus XML; covers
14 systemd interfaces (`systemd1`, `login1`, `hostname1`,
`machine1`, `home1`, `import1`, `locale1`, `network1`, `oom1`,
`portable1`, `resolve1`, `sysupdate1`, `timedate1`, `timesync1`) as
Cargo features. Dual MIT/Apache-2.0. 20 reverse-deps on crates.io.

`zbus` is the foundation of the modern Rust D-Bus ecosystem (4.7M
DL/mo, 1,379 reverse-deps). Its `#[proxy]` macro is what
`zbus_systemd` uses under the hood, so the dep graph is shallow.

## Trade-off: generated bindings vs. hand-rolled `#[proxy]`

The viable alternative to `zbus_systemd` is to use `zbus` directly
and write our own `#[proxy]` trait for just the Manager1 methods we
need (`StartTransientUnit`, `StopUnit`, `GetUnit`, `Subscribe`,
plus the `JobRemoved` signal).

**Hand-rolled wins when** the surface is 2-3 methods and one
signal. A `#[proxy]` trait for that is ~60 lines, gives total
control over the typed surface, and removes a transitive dep with
its own version-bump cadence.

**Generated wins when** we want to read unit properties broadly
(`ActiveState`, `SubState`, `LoadState`, `MainPID`, `ControlGroup`,
`Result`, etc.), subscribe to multiple signals (`JobNew`,
`JobRemoved`, `UnitNew`, `UnitRemoved`, `PropertiesChanged`), or
touch `login1`/`machine1` later. Generation handles signal-glue and
property bulk decoding cleanly.

`SystemdTransientUnitBackend` already designs for property breadth
(`SystemdUnitStatus` carries `active_state`, `sub_state`,
`main_pid`, `cgroup_path`, `job_path`) and signal-based completion
is a NDB3 deliverable. Generation repays itself immediately.

**Escape hatch:** because we also direct-depend on `zbus`, falling
back to a hand-rolled `#[proxy]` for any single endpoint where the
generated binding hits an ergonomic wall is one-line cost. We are
not locked in.

## Pinning strategy

`zbus_systemd` uses an unusual version scheme: the version tracks
the systemd interface revision the bindings were generated against.
`0.26000.0` corresponds to systemd `260`. Minor bumps (`0.26001`)
track systemd `261`, etc.

Pin **exact** with `=0.26000.0` in
`[workspace.dependencies]`. Bump deliberately when:

- A systemd LTS release we care about ships
- An upstream zbus_systemd bug fix lands in a patch version
- A security advisory requires it

Document the cadence in `deny.toml` as a one-line comment so
`make deny` auditors understand why the version sits still for
months at a time.

## Bus selection

`zbus::Connection::system()` for production (privileged daemon
talking to PID 1's systemd; requires uid 0 or polkit rules
permitting StartTransientUnit on the Manager1 interface).

`zbus::Connection::session()` for tests (per-user systemd via
`systemctl --user`; no privilege required; perfect for CI runners
with `loginctl enable-linger`).

`ZbusSystemdClient::new(bus_kind: BusKind)` is the constructor; the
caller chooses. NDB7's default activation will use
`BusKind::System` on Linux production builds.

## Signal-based completion vs. polling

The naïve flow — *call `StartTransientUnit`, log the returned job
path, return success* — masks unit-start failures. If the
`ExecStart` binary is missing, the unit fails asynchronously; the
Manager returns the job path long before the unit actually starts.
Polling `ActiveState` afterward works but adds latency and complicates
cancellation.

NDB3 implements the correct flow:

```text
1. manager.subscribe().await?;
2. let mut stream = manager.receive_job_removed().await?;
3. let job_path = manager.start_transient_unit(name, mode, props, aux).await?;
4. while let Some(sig) = stream.next().await {
       let (id, removed_path, unit, result) = sig.args()?;
       if removed_path == &job_path {
           return classify(result);
       }
   }
```

Order matters: systemd's `Manager.Subscribe` is called and the stream is
established *before* the method call to eliminate the race where
JobRemoved fires before our subscription exists. zbus's stream is
buffered, so signals arriving between stream creation and the first
`next()` are not lost.

`result` enum: `"done"` → success; `"failed"`/`"canceled"`/
`"timeout"`/`"dependency"` → error; `"skipped"` → unit was already
in target state.

## Authorization model

Production:

- Process runs as uid 0 OR
- A polkit `.policy` file grants the process's identity
  `org.freedesktop.systemd1.manage-units` on the Manager1
  interface

The polkit policy is owned by the operator-install plan; NDB
documents the requirement in `docs/operating/node-dbus-binding.md`
and tests the permission-denied path in NDB5.

Session bus tests run as the invoking user against their own user
systemd instance; no privilege required, no polkit interaction.

## Error taxonomy preview

(Full mapping landed in NDB4.)

| `zbus::fdo::Error` variant | `nimbus_core::Error` |
|---|---|
| `Disconnected`, `IOError`, `InputOutput` | `Transport` |
| `AuthFailed`, `AccessDenied` | `Permission` |
| `UnknownObject`, `UnknownInterface`, `UnknownMethod` | `NotFound` or capability degradation depending on probe context |
| `InvalidArgs`, `Failed` | `Invariant` |
| `NoMemory`, `LimitsExceeded` | `ResourceExhausted` |
| (any other) | `Internal` |

Systemd-specific names such as
`org.freedesktop.systemd1.NoSuchUnit` arrive through
`zbus::Error::MethodError`, not as `zbus::fdo::Error` enum variants.
NDB4 must match those method-error names explicitly.

Capability-degradation per NDB2:

- `Disconnected` during construction → `capabilities.dbus_available = false`
- `UnknownMethod`/`UnknownInterface` during probe →
  `capabilities.transient_units = false`

## What this decision does not commit us to

- Not committed to keeping `zbus_systemd` long-term if upstream
  becomes unmaintained — the trait seam makes substitution cheap.
- Not committed to using only generated bindings — `zbus` direct
  remains available for any endpoint that needs hand-rolling.
- Not committed to a specific systemd version on the host — the
  bindings target the Manager1 interface protocol, which is
  backwards-stable across systemd 220+.
