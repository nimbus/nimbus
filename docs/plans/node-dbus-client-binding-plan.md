# Node D-Bus Client Binding Plan (NDB)

The TSB7 wave of `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
designed the `SystemdDbusClient` trait and built `FakeSystemdDbusClient` +
`UnavailableSystemdDbusClient` to satisfy its completion gate. The gate
deliberately required only typed request construction, property
allowlisting, status mapping, and fail-closed behavior — not a live
D-Bus connection.

The TSB7 proof note recorded the deferral explicitly: *"A live zbus
adapter can be added behind `SystemdDbusClient` when product packaging
chooses the concrete dependency."* That dependency choice has now been
made (see `docs/plans/research/systemd-dbus-binding-rust-2026.md`); this
plan executes the binding.

## Why this plan exists

The current state is enterprise-cautious but not enterprise-grade:

- `crates/nimbus-node/src/systemd_transient.rs:15-32` defines the
  trait. `SystemdTransientUnitBackend<C = UnavailableSystemdDbusClient>`
  defaults to a fail-closed stub.
- Tests run against `FakeSystemdDbusClient`
  (`crates/nimbus-node/src/systemd_transient.rs:795-910`).
- No code path in the workspace calls a real D-Bus daemon.
- A reviewer comparing Nimbus to a production VM/workload manager
  (e.g., crun, conmon, systemd-nspawn drivers) sees the typed seam
  but no proof the binding actually works against a real systemd
  user/system instance.

NDB closes that gap: a real `ZbusSystemdClient` on top of
`lucab/zbus_systemd`, signal-correlated job completion (not polling),
Linux-gated integration tests against `systemctl --user` running on
every PR, and a documented error taxonomy.

The plan does **not** wire a production caller for
`NodeWorkloadReconciler` — TSB14 (`docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb14-node-extraction-decision.md:28-37`)
recorded that "host-lifecycle backends are still exercised only by
local-enforcement tests. There is not yet a production node/control-plane
caller." That wiring is the natural next plan (a node daemon) and is
out of scope here.

## Scope

In scope:

- `crates/nimbus-node/Cargo.toml` (new `systemd-dbus` feature)
- Root `Cargo.toml` (new `zbus_systemd` + `zbus` workspace deps)
- `crates/nimbus-node/src/systemd_transient/zbus_client.rs` (new
  module — concrete `SystemdDbusClient` impl, bus selection,
  capability detection, signal-based job completion, property reads)
- `crates/nimbus-node/src/systemd_transient/zbus_client/error.rs`
  (new module — `zbus::Error` / `zbus::fdo::Error` → `nimbus_core::Error`)
- `crates/nimbus-node/src/systemd_transient/zbus_client/signals.rs`
  (new module — JobRemoved correlation)
- `crates/nimbus-node/src/lib.rs` (re-export `ZbusSystemdClient`)
- `crates/nimbus-node/tests/zbus_systemd_live.rs` (new integration
  test, Linux + feature gated)
- `.github/workflows/ci.yml` (new `node-dbus-integration` job on
  `ubuntu-24.04` with user-mode systemd)
- `docs/operating/node-dbus-binding.md` (new operator-facing
  contract document)
- `docs/operating/node-lifecycle.md` + `docs/architecture/runtime/adapter-boundary.md`
  (refresh to note the live binding)
- `deny.toml` (pin policy note for the 0.26000.x version pattern)
- `scripts/verify-node-dbus-binding.sh` (this plan's verifier)
- Routing entries in `docs/plans/README.md` + `AGENTS.md` (=`CLAUDE.md`)

Out of scope:

- A production caller of `NodeWorkloadReconciler` (TSB14 deferred —
  needs its own plan defining workload source, daemon supervision,
  reconcile cadence)
- `sd-notify` integration (separate concern — emitted by the node
  daemon itself, not by the client toward systemd; pulled in when
  the node daemon plan lands)
- `login1` / `machine1` / `oom1` bindings (NDB only enables `systemd1`
  in `zbus_systemd`; other interfaces enabled when a caller needs them)
- Polkit policy authoring (NDB documents the privilege model but
  doesn't ship `.policy` files — operator install plan owns that)
- Any change to the `SystemdDbusClient` trait surface itself (the
  whole point of the seam-first design is that this plan slots in
  beneath it without API churn)

## Ledger

| NDB | Description | Status |
|-----|-------------|--------|
| NDB0 | Scaffold this plan + verifier at `scripts/verify-node-dbus-binding.sh` (10 conditions, mostly FAIL until later bands flip them); baseline proof at `docs/plans/proof/node-dbus-client-binding/ndb0-baseline.md` recording the starting state (no `zbus*` deps, trait abstract, all tests use mocks); research note at `docs/plans/research/systemd-dbus-binding-rust-2026.md` recording the dependency decision (zbus_systemd vs alternatives, pin strategy, signal-vs-polling, bus selection, authorization model); routing entry in `AGENTS.md`. | pending |
| NDB1 | Workspace + crate Cargo wiring. Add `zbus_systemd` (pin `=0.26000.0`, default-features off, features `["systemd1", "zbus-async-tokio"]`) and `zbus` (pin to whatever zbus_systemd 0.26000 selects) to root `Cargo.toml` `[workspace.dependencies]`. Add `systemd-dbus` feature to `crates/nimbus-node/Cargo.toml` that gates the new deps. Default is OFF until NDB7. `deny.toml` gets a comment explaining the 0.26000.x scheme. `make deny` stays green. | pending |
| NDB2 | `ZbusSystemdClient` skeleton + capability detection. New module tree under `crates/nimbus-node/src/systemd_transient/zbus_client/`: `mod.rs` exposes `ZbusSystemdClient` and `BusKind::{System, Session}`. Constructor accepts `BusKind`, opens `zbus::Connection`, caches `ManagerProxy<'static>`. `capabilities()` probes the daemon via a cheap `GetUnit("init.scope")` call; maps `Disconnected` → `dbus_available=false`, `UnknownMethod`/`InterfaceNotFound` → `transient_units=false`. Implements `SystemdDbusClient` with `start/stop/inspect` returning `not yet implemented` errors. Unit tests use a mocked `zbus::Connection` (via `zbus::conn::Builder::p2p()` paired sockets) to prove constructor and probe paths. | pending |
| NDB3 | Signal-based completion. New `signals.rs` submodule. Implements: subscribe to `JobRemoved` *before* calling `StartTransientUnit`/`StopUnit`; correlate by the `job_path` returned from the method call; the future resolves only when the matching `JobRemoved` signal with `result` ∈ `{"done","failed","canceled","timeout","dependency","skipped"}` is observed. Drop semantics for the subscription on cancel are verified. The `inspect_unit` impl uses `GetUnit` → unit object path → `org.freedesktop.DBus.Properties.GetAll` on the `Unit` + `Service` interfaces, populating `SystemdUnitStatus` (active_state, sub_state, main_pid, cgroup_path). Race tests prove no signal loss when the unit transitions faster than the method response arrives. | pending |
| NDB4 | Error taxonomy. New `error.rs` submodule. Exhaustive match on `zbus::fdo::Error` variants and the general `zbus::Error` shape: `Disconnected`/`InputOutput` → `nimbus_core::Error::Transport`; `AuthFailed`/`AccessDenied` → `Permission`; `UnknownObject`/`NoSuchUnit` → `NotFound`; `InvalidArgs` → `Invariant`; capability-missing per NDB2 → `ResourceExhausted`. Unit tests instantiate each variant via mock and assert the mapped Nimbus error. Every D-Bus call in NDB3 flows through this mapper. | pending |
| NDB5 | Linux-gated integration tests. New `crates/nimbus-node/tests/zbus_systemd_live.rs` gated on `#[cfg(all(target_os = "linux", feature = "systemd-dbus-integration-tests"))]`. Each test: builds `ZbusSystemdClient` against the session bus (`systemctl --user`); starts a `sleep 30` transient unit with a unique UUID-suffixed name; observes JobRemoved with `"done"`; reads properties to verify active+running state; calls `stop_unit`; observes JobRemoved with `"done"`; verifies the unit reaches `inactive`/`dead`; teardown calls `ResetFailedUnit` to clean up. Additional cases: ExecStart-not-found path (verifies `"failed"` result mapping), permission-denied path (forces system bus when not root). | pending |
| NDB6 | CI lane. New `node-dbus-integration` job in `.github/workflows/ci.yml` on `ubuntu-24.04`. Pre-steps: `apt-get install -y dbus-user-session systemd-container`; `loginctl enable-linger $USER`; verify `systemctl --user is-system-running` returns. Uses the `setup-rust-cached` composite. Runs `cargo test -p nimbus-node --features systemd-dbus,systemd-dbus-integration-tests --test zbus_systemd_live --no-fail-fast`. Job is on the PR critical path (gates `rust-gate-summary.needs:`). Step summary emits a markdown table of pass/fail per test for the CI dashboard. | pending |
| NDB7 | Activation + docs + closeout. Flip `systemd-dbus` to a default feature on `nimbus-node`. Swap the default type parameter on `SystemdTransientUnitBackend<C>` from `UnavailableSystemdDbusClient` to `ZbusSystemdClient` *only on Linux* (cfg-gated default); other platforms keep the unavailable default. Add `docs/operating/node-dbus-binding.md` with: bus selection rationale, signal-completion semantics, error taxonomy, capability degradation matrix, privilege model. Refresh `docs/operating/node-lifecycle.md` + `docs/architecture/runtime/adapter-boundary.md` to point at the new operator doc. Flip every ledger row to `done`; append Execution Log with real SHAs; move plan to `docs/plans/archive/`; verifier's `plan_file()` accepts both paths; update routing in `AGENTS.md` + `docs/plans/README.md`. | pending |

## Completion Gate

`bash scripts/verify-node-dbus-binding.sh` exits 0 with summary line
`10 passed, 0 failed`. The 10 conditions:

1. Plan file exists (`docs/plans/node-dbus-client-binding-plan.md` or
   `docs/plans/archive/node-dbus-client-binding-plan.md`).
2. Routing entry exists in `CLAUDE.md` (= `AGENTS.md`) naming this
   plan.
3. NDB0 deliverables present: baseline proof at
   `docs/plans/proof/node-dbus-client-binding/ndb0-baseline.md` and
   research note at
   `docs/plans/research/systemd-dbus-binding-rust-2026.md`.
4. NDB1: `zbus_systemd` declared in workspace deps with feature set
   `systemd1` + `zbus-async-tokio`; `crates/nimbus-node/Cargo.toml`
   declares a `systemd-dbus` feature that pulls it in.
5. NDB2: `ZbusSystemdClient` type exists at
   `crates/nimbus-node/src/systemd_transient/zbus_client/mod.rs` (or
   `crates/nimbus-node/src/systemd_transient/zbus_client.rs`), is
   re-exported from `crates/nimbus-node/src/lib.rs`, and accepts a
   `BusKind` argument.
6. NDB3: signal-based completion — source contains a
   `receive_job_removed` (or equivalent
   `MatchRule::new().interface("org.freedesktop.systemd1.Manager").member("JobRemoved")`)
   call established *before* `StartTransientUnit`/`StopUnit` is
   invoked (lexical order in source).
7. NDB4: error taxonomy module exists at
   `crates/nimbus-node/src/systemd_transient/zbus_client/error.rs`
   with documented mapping for at least `Disconnected`,
   `AccessDenied`, `UnknownObject`, `InvalidArgs`.
8. NDB5: integration test file exists at
   `crates/nimbus-node/tests/zbus_systemd_live.rs` and is gated by
   both `target_os = "linux"` and the
   `systemd-dbus-integration-tests` feature.
9. NDB6: CI job `node-dbus-integration` exists in
   `.github/workflows/ci.yml`, runs on `ubuntu-24.04`, invokes the
   integration test, and is listed in `rust-gate-summary.needs:`.
10. NDB7: `systemd-dbus` is in the `default` feature list of
    `crates/nimbus-node/Cargo.toml`, operator doc at
    `docs/operating/node-dbus-binding.md` exists, every ledger row in
    this plan is marked `done`, and latest CI run on `main` is green.

## Trust targets

What this plan changes about the trust posture:

- **Before NDB**: "We have a typed D-Bus seam with mock-only tests."
  Enterprise-cautious — defensible architecture, no evidence of
  liveness.
- **After NDB3**: A real client speaks the systemd Manager1 D-Bus
  protocol, including signal-correlated job completion (no polling).
- **After NDB5**: Local integration tests prove start/stop/inspect
  round-trips against real systemd-user with negative-path coverage
  (ExecStart not found, permission denied).
- **After NDB6**: Every PR exercises the live binding in CI. A
  regression in the D-Bus surface or the upstream zbus_systemd crate
  fails fast.
- **After NDB7**: Production code paths construct
  `ZbusSystemdClient` by default on Linux. The trait still allows
  injecting `UnavailableSystemdDbusClient` or `FakeSystemdDbusClient`
  in tests, but the default is the live binding.

## Proof directory

`docs/plans/proof/node-dbus-client-binding/`:

- `ndb0-baseline.md` — starting state (no `zbus*` deps, trait
  abstract, tests use mocks), reason for the deferral (TSB7 quote),
  the decision matrix summary
- `ndb1-cargo-wiring.md` — `cargo metadata` output proving
  `zbus_systemd` is reachable from `nimbus-node` under
  `--features systemd-dbus`; `make deny` evidence
- `ndb2-skeleton.md` — module tree, constructor surface, capability
  probe sequence with sample trace
- `ndb3-signal-completion.md` — race-test results (signal arrival
  before/after method response); JobRemoved correlation diagram
- `ndb4-error-taxonomy.md` — error mapping table; mock-driven unit
  test evidence
- `ndb5-live-integration.md` — `cargo test` output showing all
  Linux-gated tests pass; teardown cleanup verified
- `ndb6-ci-lane.md` — first green CI run with `node-dbus-integration`
  job; per-test step summary
- `ndb7-closeout.md` — final state, default flip evidence, retro

## Execution Log

| NDB | Commit | Subject |
|-----|--------|---------|
| NDB0 | _pending_ | scaffold Node D-Bus Binding plan + verifier + research note |
| NDB1 | _pending_ | wire zbus_systemd workspace dep behind systemd-dbus feature |
| NDB2 | _pending_ | ZbusSystemdClient skeleton with bus selection + capability probe |
| NDB3 | _pending_ | signal-correlated job completion via JobRemoved subscription |
| NDB4 | _pending_ | zbus error taxonomy → nimbus_core::Error mapping |
| NDB5 | _pending_ | Linux-gated integration tests against systemctl --user |
| NDB6 | _pending_ | CI lane: node-dbus-integration on ubuntu-24.04 |
| NDB7 | _pending_ | activate systemd-dbus default + operator doc + closeout |

## Notes on staging order

NDB0 first because the verifier needs to exist before any band can
prove its work. NDB1 before NDB2 because the impl needs the deps in
the lockfile to compile. NDB2 before NDB3 because the connection
plumbing has to exist before signal subscription can be wired through
it. NDB3 before NDB4 because the error mapping needs real call sites
to verify against, not just stubs. NDB5 before NDB6 because tests
have to exist before CI can run them. NDB7 last because the default
activation depends on every other band being green.

Within each band: one commit, one entry in the Execution Log. The
plan stays on `main` continuously; there are no PR boundaries.
