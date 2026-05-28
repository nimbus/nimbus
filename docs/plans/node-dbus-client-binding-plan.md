# Node D-Bus Client Binding Plan (NDB)

The TSB7 wave of `docs/plans/archive/tenant-domain-and-node-enforcement-boundary-plan.md`
designed the `SystemdDbusClient` trait and built `FakeSystemdDbusClient` +
`UnavailableSystemdDbusClient` to satisfy its completion gate. The gate
deliberately required only typed request construction, property
allowlisting, status mapping, and fail-closed behavior — not a live
D-Bus connection.

The TSB7 proof note recorded the deferral explicitly: *"A live zbus
adapter can be added behind `SystemdDbusClient` when product packaging
chooses the concrete dependency."* That dependency choice has now been
made (see `docs/plans/research/systemd-dbus-binding-rust-2026.md`); this
plan executes the binding. Use the local upstream checkout at
`~/src/github.com/lucab/zbus_systemd` as implementation reference; as of
the plan audit it is at `81ac9452` (`v0.26000.0-3-g81ac945`) and contains
package version `0.26000.0`.

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
- `crates/nimbus-core/src/error.rs` (extend the `Error` enum with two
  new variants — `Transport(String)` and `NotFound(String)` — so the
  D-Bus taxonomy maps to honest names instead of overloading
  `Internal`/`InvalidInput`; the enum is **not** `#[non_exhaustive]`, so
  this band must also update every exhaustive `match` on
  `nimbus_core::Error` across the workspace)
- `crates/nimbus-node/src/systemd_transient/zbus_client/signals.rs`
  (new module — JobRemoved correlation)
- `crates/nimbus-node/src/systemd_transient/zbus_client/properties.rs`
  (new module — typed transient-unit property marshalling to
  `zbus_systemd::zvariant::OwnedValue`)
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

## Upstream API anchors

The coding agent should verify method names and signatures against
`~/src/github.com/lucab/zbus_systemd/src/systemd1/generated.rs`, not from
memory:

- `zbus_systemd::systemd1::ManagerProxy` exposes `subscribe()`,
  `receive_job_removed()`, `start_transient_unit(name, mode, properties,
  aux)`, `stop_unit(name, mode)`, `get_unit(name)`, and
  `reset_failed_unit(name)`.
- `JobRemoved` args are `(id, job, unit, result)`; correlate completion by
  the `job` object path returned from `start_transient_unit` or `stop_unit`.
- `start_transient_unit` takes `Vec<(String, OwnedValue)>` properties and
  `Vec<(String, Vec<(String, OwnedValue)>)>` auxiliary units. Add a focused
  property encoder rather than scattering `OwnedValue` construction through
  the client.
- Unit/service inspection can use generated `UnitProxy` and `ServiceProxy`
  property accessors for `load_state`, `active_state`, `sub_state`,
  `main_pid`, `control_group`, and `result`; use direct generated getters
  unless a bulk `GetAll` helper proves cleaner in code review.
- `zbus_systemd::lib.rs` re-exports both `zbus` and `zbus::zvariant`; prefer
  those re-exports inside the binding so the generated crate and direct
  `zbus` usage cannot drift.

## Ledger

| NDB | Description | Status |
|-----|-------------|--------|
| NDB0 | Scaffold this plan + verifier at `scripts/verify-node-dbus-binding.sh` (10 conditions, mostly FAIL until later bands flip them); baseline proof at `docs/plans/proof/node-dbus-client-binding/ndb0-baseline.md` recording the starting state (no `zbus*` deps, trait abstract, all tests use mocks); research note at `docs/plans/research/systemd-dbus-binding-rust-2026.md` recording the dependency decision (zbus_systemd vs alternatives, pin strategy, signal-vs-polling, bus selection, authorization model); routing entry in `AGENTS.md`. | done |
| NDB1 | Workspace + crate Cargo wiring. Add `zbus_systemd` (pin `=0.26000.0` — the latest published stable as of 2026-03-28, default-features off, features `["systemd1", "zbus-async-tokio"]`) and direct `zbus = "5.15"` (latest 5.x; the version `zbus_systemd 0.26000.0` itself selects, `default-features = false` + `features = ["tokio"]`; root dep required for test-bus utilities and any hand-rolled proxy escape hatch) to root `Cargo.toml` `[workspace.dependencies]`. Add `systemd-dbus` and `systemd-dbus-test-bus` features to `crates/nimbus-node/Cargo.toml`; add `systemd-dbus-integration-tests` as an explicit opt-in feature layered on `systemd-dbus`. Both new deps are `optional = true` on `nimbus-node`, gated by `systemd-dbus`; default is OFF until NDB7. `deny.toml` gets a comment explaining the 0.26000.x scheme. **Deny gate (verified at plan time):** the zbus subtree resolves with all-MIT/Apache licenses (already allowlisted) and adds **zero** new duplicate-version bans — `cargo deny check bans` reports the identical set with and without it. Note: the base branch currently carries an *unrelated* red `make deny` from a stale `deno_core@0.400.0` `skip-tree` root (the deno lane bumped to `0.401.0`); NDB does not own that fix. NDB1's evidence is the with-vs-without diff, not an absolute green. | pending |
| NDB2 | `ZbusSystemdClient` skeleton + capability detection. New module tree under `crates/nimbus-node/src/systemd_transient/zbus_client/`: `mod.rs` exposes `ZbusSystemdClient` and `BusKind::{System, Session}`. The async, fallible constructor accepts `BusKind` and an internal test-only connection injection hook, opens `zbus::Connection`, caches `ManagerProxy<'static>`. **The capability probe runs in the constructor, not in `capabilities()`** — the trait method `fn capabilities(&self) -> SystemdTransientCapabilities` is synchronous and returns an owned struct, so it cannot do async D-Bus I/O. The constructor issues a cheap `GetUnit("init.scope")`, maps `Disconnected` → `dbus_available=false`, `UnknownMethod`/`InterfaceNotFound` → `transient_units=false`, and sets `service_units` (the third field of `SystemdTransientCapabilities`); the result is cached and `capabilities()` returns the cached copy. Implement the `SystemdDbusClient` trait methods behind temporary "not implemented" errors only in this band. Unit tests use a mocked zbus peer-to-peer or test-bus connection shape gated by `systemd-dbus-test-bus` to prove constructor and probe paths. | pending |
| NDB3 | Signal-based completion and property marshalling. Add `signals.rs` plus `properties.rs`. Correct flow is: call `ManagerProxy::subscribe()`, create `receive_job_removed()` stream, then call `start_transient_unit`/`stop_unit`; correlate by the `job` object path returned from the method call; resolve only when the matching `JobRemoved` signal arrives. Classify `result`: `"done"` → success; `"failed"`/`"canceled"`/`"timeout"`/`"dependency"` → error; `"skipped"` → already in target state. systemd can also emit `"once"`/`"merged"`/`"assert"`/`"unsupported"`/`"collected"`, so the classifier has an explicit catch-all that maps any unrecognized result string to an error (never silently treats it as success). Drop semantics for the subscription on cancel are verified. `properties.rs` owns all `OwnedValue` construction for transient unit properties, including `Description`, `Type`, `Slice`, `WorkingDirectory`, `Environment`, and write-side `ExecStart`; a test asserts the encoded values round-trip through zvariant signatures expected by systemd's `StartTransientUnit`. The `inspect_unit` impl uses `GetUnit` to resolve the unit object path, then builds generated `UnitProxy`/`ServiceProxy` property reads. `SystemdUnitStatus` has eight fields: `workload_id` and `unit_name` come from the request; `active_state`, `sub_state` come from `UnitProxy`; `main_pid` (`Option<u32>`) and `cgroup_path` (D-Bus `ControlGroup`) come from `ServiceProxy`; `job_path` (`Option<String>`) and `journal_selectors` are populated from the request/known unit identity. `load_state` and the service `Result` property are read for diagnostics/classification but are **not** stored status fields — do not invent a `result` field on `SystemdUnitStatus`. Race tests prove no signal loss when the unit transitions faster than the method response arrives. | pending |
| NDB4 | Error taxonomy. **First** extend `nimbus_core::Error` (crates/nimbus-core/src/error.rs) with two new variants — `Transport(String)` (`#[error("transport error: {0}")]`) and `NotFound(String)` (`#[error("not found: {0}")]`) — and update every exhaustive `match` on `Error` across the workspace (the enum is not `#[non_exhaustive]`; `make check` is the gate that finds them all). **Then** add the new `error.rs` submodule under `zbus_client/` and map the general `zbus::Error` shape and method-error names explicitly: `Disconnected`/`InputOutput` → `Transport`; authentication or access-denied failures (`org.freedesktop.DBus.Error.AccessDenied`, `AuthFailed`) → `PermissionDenied`; `org.freedesktop.systemd1.NoSuchUnit`, `org.freedesktop.DBus.Error.UnknownObject`, and equivalent method-error names → `NotFound`; invalid-argument method errors (`org.freedesktop.DBus.Error.InvalidArgs`, systemd `Failed`) → `InvalidInput`; capability-missing per NDB2 → `ResourceExhausted`; unknown-method errors → the capability path only during capability probing and `Internal` elsewhere; any unmapped error → `Internal`. (These are the *actual* `nimbus_core::Error` variant names — verified against the enum; there is no `Permission`/`Invariant` variant.) Unit tests instantiate each mapped path via mock errors and assert the Nimbus error. Every D-Bus call in NDB3 flows through this mapper. | pending |
| NDB5 | Linux-gated integration tests. New `crates/nimbus-node/tests/zbus_systemd_live.rs` gated on `#[cfg(all(target_os = "linux", feature = "systemd-dbus-integration-tests"))]`. Each test builds `ZbusSystemdClient` against the session bus (`systemctl --user`), starts a unique UUID-suffixed transient unit with `Type=exec` and a deterministic shell-free executable such as `/usr/bin/sleep`, observes JobRemoved with `"done"` for start, reads generated properties to verify active/running state, calls `stop_unit`, observes JobRemoved with a classified terminal result, verifies the unit reaches `inactive`/`dead`, and calls `ResetFailedUnit` in teardown. Additional cases: ExecStart-not-found path verifies `"failed"` result mapping, and permission-denied path forces system bus when not root. **No silent skips:** when the feature+target gate is on, an unreachable session bus or absent user manager is a test **failure**, not a skip — a misconfigured environment must never masquerade as green. Test value comes from observing real state transitions (`active`/`running` → `inactive`/`dead`) and real `JobRemoved` results, not from the binding merely not panicking. | pending |
| NDB6 | CI lane. New `node-dbus-integration` job in `.github/workflows/ci.yml` on `ubuntu-24.04`. Pre-steps use `sudo apt-get update` and `sudo apt-get install -y dbus-user-session systemd-container`; enable or verify a user systemd session with `loginctl enable-linger "$USER"`, `XDG_RUNTIME_DIR=/run/user/$(id -u)`, `DBUS_SESSION_BUS_ADDRESS=unix:path=${XDG_RUNTIME_DIR}/bus`. A `systemctl --user is-system-running` reporting `degraded`/`starting` is acceptable **as a bus-reachability signal only** (those states are benign on a fresh runner); it must NOT be used to skip or soft-pass the tests. A pre-flight step that hard-fails the job if `systemctl --user` cannot reach the bus is required, so a broken systemd bootstrap surfaces as a red job rather than a vacuous green. Uses the `setup-rust-cached` composite. Runs `cargo test -p nimbus-node --features systemd-dbus,systemd-dbus-integration-tests --test zbus_systemd_live --no-fail-fast`. Step summary emits a markdown table of pass/fail per test. **Gating order:** add the lane to `rust-gate-summary.needs:` only once the bootstrap recipe is proven to actually launch and observe a transient unit on the runner (prove it green on this branch first); wiring a flaky systemd-in-CI bootstrap into the merge gate before it is reliable would block all merges. | pending |
| NDB7 | Activation + docs + closeout. Flip `systemd-dbus` to a default feature on `nimbus-node`. Because `ZbusSystemdClient::new(BusKind::System)` is async/fallible, add an explicit Linux constructor/factory such as `SystemdTransientUnitBackend::linux_systemd_default().await` instead of pretending a generic default type parameter can construct the live client by itself; other platforms keep the unavailable constructor path. Add `docs/operating/node-dbus-binding.md` with: bus selection rationale, `Manager.Subscribe` + JobRemoved signal-completion semantics, property encoding contract, error taxonomy, capability degradation matrix, privilege model. Refresh `docs/operating/node-lifecycle.md` + `docs/architecture/runtime/adapter-boundary.md` to point at the new operator doc. Flip every ledger row to `done`; append Execution Log with real SHAs; move plan to `docs/plans/archive/`; verifier's `plan_file()` accepts both paths; update routing in `AGENTS.md` + `docs/plans/README.md`. | pending |

## Completion Gate

`bash scripts/verify-node-dbus-binding.sh` exits 0 with summary line
`10 passed, 0 failed`. The 10 conditions:

1. Plan file exists (`docs/plans/node-dbus-client-binding-plan.md` or
   `docs/plans/archive/node-dbus-client-binding-plan.md`).
2. Routing entries exist in both `CLAUDE.md` (= `AGENTS.md`) and
   `docs/plans/README.md` naming this plan.
3. NDB0 deliverables present: baseline proof at
   `docs/plans/proof/node-dbus-client-binding/ndb0-baseline.md` and
   research note at
   `docs/plans/research/systemd-dbus-binding-rust-2026.md`.
4. NDB1: `zbus_systemd` declared in workspace deps with feature set
   `systemd1` + `zbus-async-tokio`; direct `zbus` workspace dep
   present; `crates/nimbus-node/Cargo.toml` declares `systemd-dbus`,
   `systemd-dbus-test-bus`, and `systemd-dbus-integration-tests`
   features.
5. NDB2: `ZbusSystemdClient` type exists at
   `crates/nimbus-node/src/systemd_transient/zbus_client/mod.rs` (or
   `crates/nimbus-node/src/systemd_transient/zbus_client.rs`), is
   re-exported from `crates/nimbus-node/src/lib.rs`, and accepts a
   `BusKind` argument.
6. NDB3: signal-based completion — source contains `subscribe()`
   established before a `receive_job_removed` stream (or equivalent
   `MatchRule::new().interface("org.freedesktop.systemd1.Manager").member("JobRemoved")`)
   and both appear *before* `StartTransientUnit`/`StopUnit` is invoked
   (lexical order in source). Source also contains centralized
   `OwnedValue` property encoding for `StartTransientUnit`.
7. NDB4: `nimbus_core::Error` carries the new `Transport` and
   `NotFound` variants, and the error taxonomy module exists at
   `crates/nimbus-node/src/systemd_transient/zbus_client/error.rs`
   with documented mapping for at least `Disconnected`,
   `AccessDenied`, `UnknownObject`, `NoSuchUnit`, `InvalidArgs`.
8. NDB5: integration test file exists at
   `crates/nimbus-node/tests/zbus_systemd_live.rs` and is gated by
   both `target_os = "linux"` and the
   `systemd-dbus-integration-tests` feature.
9. NDB6: CI job `node-dbus-integration` exists in
   `.github/workflows/ci.yml`, runs on `ubuntu-24.04`, bootstraps
   user-mode systemd with `sudo apt-get`, `loginctl`, and
   `systemctl --user`, invokes the integration test, and is listed in
   `rust-gate-summary.needs:`.
10. NDB7: `systemd-dbus` is in the `default` feature list of
    `crates/nimbus-node/Cargo.toml`, Linux live-client factory or
    constructor is present, operator doc at
    `docs/operating/node-dbus-binding.md` exists, every ledger row in
    this plan is marked `done`, and latest CI run on `main` is green.

## Trust targets

What this plan changes about the trust posture:

- **Before NDB**: "We have a typed D-Bus seam with mock-only tests."
  Enterprise-cautious — defensible architecture, no evidence of
  liveness.
- **After NDB3**: A real client speaks the systemd Manager1 D-Bus
  protocol, including `Manager.Subscribe` plus signal-correlated job
  completion (no polling).
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
  before/after method response); `Manager.Subscribe` + JobRemoved
  correlation diagram; `OwnedValue` property encoder evidence
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
| NDB0 | 7686d55b | scaffold Node D-Bus Binding plan + verifier + research note |
| NDB1 | _pending_ | wire zbus_systemd workspace dep behind systemd-dbus feature |
| NDB2 | _pending_ | ZbusSystemdClient skeleton with bus selection + capability probe |
| NDB3 | _pending_ | signal-correlated job completion via Manager.Subscribe + JobRemoved |
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
