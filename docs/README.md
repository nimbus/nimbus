# Documentation

This directory holds Neovex's deeper technical docs. The root
[README.md](../README.md) stays focused on what Neovex is, how to install it,
and where to go next.

## Start Here

- [README.md](../README.md):
  product entrypoint, install, verification, licensing, docs map
- [ARCHITECTURE.md](../ARCHITECTURE.md):
  stable architecture, crate map, and invariants
- [Current capabilities](reference/current-capabilities.md):
  snapshot of the current implemented surface
- [MicroVM and service-control baseline](reference/microvm-service-baseline.md):
  concise current baseline for the landed krun-backed microVM runtime,
  `ctx.services.*` integration, and `neovex service ...` control surface
- [HTTP and WebSocket API](reference/http-api.md):
  native and optional Convex route catalog
- [CLI reference](reference/cli.md):
  server flags and runtime-limit defaults
- [krun VMM host validation](reference/krun-vmm-host-validation.md):
  Linux-side build, install, and evidence capture runbook for historical
  patched-crun VMM validation and regression reruns
- [Convex compatibility](convex/compatibility.md):
  current Convex-surface scope, limits, and demo entrypoints
- [Runtime execution architecture rationale](research/runtime-execution-architecture-rationale.md):
  why Neovex embeds V8 via deno_core fork, why the workerd model is not
  pursued, and what future paths exist
- [macOS host-vs-guest control-plane rationale](research/macos-host-vs-guest-control-plane-rationale.md):
  why macOS should prefer a guest-resident authoritative Neovex server over a
  host-resident hybrid control plane for v1
- [Versioned serving snapshot design note](research/versioned-serving-snapshot-design-note.md):
  implementation-grade north-star for the next server-side read-surface
  promotion after the `SA8` materialized-serving slice
- [Demos](../demos/README.md):
  demo layout and run commands
- [Plans](plans/README.md):
  plan index for active execution control planes, deferred design work, and
  archived execution history

## Layout

- `reference/`: stable operator and developer reference docs
- `convex/`: Convex-surface behavior and compatibility notes
- `plans/`: plan index plus active and deferred execution plans
- `plans/archive/`: completed historical plans; not active control planes
- `research/`: background research and north-star direction, not the execution
  control plane

## Verification Harness

The deterministic verification harness now has explicit local and CI modes:

- `make verify-harness`
  runs the focused named corpora for storage, engine, native HTTP, runtime,
  and server transport-liveness campaigns
- `make verify-harness SURFACE=runtime`
  runs one focused harness surface
- `make verify-harness-runtime`
  runs the runtime focused harness surface through a dedicated convenience
  target
- `make verify-harness-nightly`
  runs the heavier adversarial named corpora
- `make verify-harness-nightly SURFACE=server`
  runs one focused nightly harness surface
- `make verify-harness-repro SURFACE=<storage|engine|server|runtime> MODE=<pr|nightly> CASE=<case-id>`
  reruns one exact failing seed from the corpus

The underlying `bash scripts/verification-harness.sh ...` launcher still
exists, but `make` is the preferred operator and contributor interface.

These harness entrypoints and the main `make check`, `make test`, and
`make clippy` targets are single-flight guarded locally, so an accidental
duplicate invocation exits quickly instead of starting a second overlapping
verification session.

Named seeds live in `neovex-storage::simulation` and flow through
`neovex-testing`. Runtime harness cases live in
`neovex-runtime::runtime::tests::verification_harness` and route through local
runtime `test_support` metadata plus subprocess isolation helpers. New
historically valuable bug-finding seeds or runtime case ids should be checked
in with stable names so failure output stays reproducible.

Server transport-liveness campaigns live in
`neovex-server::tests::verification_harness` and reuse the named websocket,
scheduler, and fairness scenarios from the ordinary server test tree instead of
forking separate harness-only behavior. Exact server repro commands such as
`make verify-harness-repro SURFACE=server MODE=pr CASE=websocket-auth-change-resubscribe`
and exact runtime repro commands such as
`make verify-harness-repro SURFACE=runtime MODE=pr CASE=runtime-queue-limit-rejection-accounting`
now route through the same case ids that appear in failure output.

The hardened harness topology is now:

- the ordinary workspace suite keeps the heavier generated-history corpora
  ignored by default
- runtime V8-sensitive tests use explicit harness-owned subprocess isolation
  instead of crate-wide single-thread containment
- the dedicated verification-harness lanes own the named focused and nightly
  corpora
- runtime now has a first-class harness surface for queue-accounting,
  cooperative-dispatch, integrity-after-success, and fairness/accounting
  campaigns
- server now has a first-class transport-liveness harness corpus layered on top
  of the existing generated-history server corpus
- only the server harness corpus currently narrows to `--test-threads=1`
  because it boots multiple ephemeral HTTP fixtures that still need serialized
  binding

When adding new runtime or server liveness campaigns, prefer the shared
`DeterministicTestCase` and eventual-wait helpers in `neovex-testing`
plus the shared fault-gate primitives (`BlockingFaultInjector` and
`ArmedBlockingFaultInjector`) and explicit runtime test profiles over ad hoc
sleeps or implicit defaults.
