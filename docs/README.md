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
- [HTTP and WebSocket API](reference/http-api.md):
  native and optional Convex route catalog
- [CLI reference](reference/cli.md):
  server flags and runtime-limit defaults
- [Convex compatibility](convex/compatibility.md):
  current Convex-surface scope, limits, and demo entrypoints
- [Versioned serving snapshot design note](research/versioned-serving-snapshot-design-note.md):
  implementation-grade north-star for the next server-side read-surface
  promotion after the `SA8` materialized-serving slice
- [Demos](../demos/README.md):
  demo layout and run commands
- [Plans](plans/README.md):
  plan index for active control planes, deferred design work, and archived
  execution history

## Layout

- `reference/`: stable operator and developer reference docs
- `convex/`: Convex-surface behavior and compatibility notes
- `plans/`: plan index plus active and deferred execution plans
- `plans/archive/`: completed historical plans; not active control planes
- `research/`: background research and north-star direction, not the execution
  control plane

## Verification Harness

The deterministic verification harness now has explicit local and CI modes:

- `bash scripts/verification-harness.sh pr`
  runs the focused named-seed corpus for storage, engine, and native HTTP
- `bash scripts/verification-harness.sh nightly`
  runs the heavier adversarial named-seed corpus
- `bash scripts/verification-harness.sh repro <storage|engine|server> <pr|nightly> <case-id>`
  reruns one exact failing seed from the corpus

These harness entrypoints and the main `make check`, `make test`, and
`make clippy` targets are single-flight guarded locally, so an accidental
duplicate invocation exits quickly instead of starting a second overlapping
verification session.

Named seeds live in `neovex-storage::simulation` and flow through
`neovex-test-support`. New historically valuable bug-finding seeds should be
checked in there with stable case ids so failure output stays reproducible.
