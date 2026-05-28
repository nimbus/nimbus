# NDB7 Proof — activation + closeout

Final band: `systemd-dbus` becomes a default feature, a Linux live-client
factory lands, the operator doc ships, and the plan is archived.

## Activation

- `crates/nimbus-node/Cargo.toml`: `default = ["systemd-dbus"]` — the live
  binding compiles in production builds by default.
- `SystemdTransientUnitBackend::linux_systemd_default()` (cfg
  `target_os = "linux"` + `systemd-dbus`): async/fallible factory that builds a
  live `ZbusSystemdClient` on the system bus. The generic default type
  parameter cannot construct an async/fallible client, so this is explicit.
  Non-Linux keeps `SystemdTransientUnitBackend::unavailable(...)`.

## Docs

- `docs/operating/node-dbus-binding.md` — bus selection, capability probe,
  `Manager.Subscribe` + `JobRemoved` completion, property encoding, error
  taxonomy, capability-degradation matrix, polkit privilege model, CI lane.
- `docs/operating/node-lifecycle.md` + `docs/architecture/runtime/adapter-boundary.md`
  point at it.

## Trust posture achieved

- A real `ZbusSystemdClient` speaks the systemd Manager1 protocol with
  `Manager.Subscribe` + signal-correlated `JobRemoved` completion (no polling).
- The live integration tests run on every PR against real `systemctl --user`;
  the CI run on `node-dbus-binding` proved the bootstrap recipe and that
  start/inspect/stop round-trip + failure observation work against real
  systemd (see `ndb6-ci-lane.md` for the green run).
- The error surface maps cleanly to `nimbus_core::Error` (with new
  `Transport`/`NotFound` variants).

## What this plan did NOT do (deferred)

No production daemon calls `NodeWorkloadReconciler` yet — TSB14's deferral of a
node/control-plane caller stands. The live binding is *default-constructed and
CI-guarded*, not yet on a live request path. Wiring a workload source /
reconcile loop is the next plan.

## CI iteration log

- Run 1 (`64a492e0`): bootstrap recipe ✅, `start`/`inspect` ✅ live; surfaced a
  non-idempotent `Manager.Subscribe` bug (stop's second subscribe →
  `AlreadySubscribed`). Also caught a `cargo fmt` miss.
- Fixes: `855a7005` idempotent subscribe; `5152bc27` rustfmt.
- Run 2 (`855a7005`): `node-dbus-integration` **green** — both live tests pass
  (`2 passed; 0 failed`); `Rust Format` green too. `Rust Runtime Tests` +
  `Bun/JSC Runtime Contract` are red, but **only** on `crates/nimbus-runtime/`
  tests NDB never touches (the concurrent node-faas-runtime plan's in-flight
  failures on the base commit) — not NDB regressions. The full-suite-green
  goal condition is therefore blocked by the base branch, not by NDB.

## Closeout

Every ledger row `done`; Execution Log carries real SHAs; plan moved to
`docs/plans/archive/`; routing in `AGENTS.md` + `docs/plans/README.md`
updated; verifier `10 passed, 0 failed`. The closeout PR
`node-dbus-binding → main` is opened as the last step (its diff also carries
the not-yet-on-`origin/main` node-extraction base — the documented
base-dependency caveat).
