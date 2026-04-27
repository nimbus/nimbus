# Documentation

This directory holds Neovex's deeper technical docs. The root
[README.md](../README.md) stays focused on what Neovex is, how to install it,
and where to go next.

## Start Here

- [README.md](../README.md):
  product entrypoint, install, verification, licensing, docs map
- [ARCHITECTURE.md](../ARCHITECTURE.md):
  stable architecture, crate map, and invariants
- [Storage encryption architecture](architecture/storage/encryption.md):
  stable design baseline for optional encryption at rest across Neovex-owned
  local persistence
- [Current capabilities](reference/current-capabilities.md):
  snapshot of the current implemented surface
- [Provider topology reference](reference/provider-topologies.md):
  deeper reference for external-provider and replica-topology shapes that
  extend the stable persistence architecture
- [Persistence engine baseline](reference/persistence-engine-baseline.md):
  deeper reference for backend layouts, durable-journal ownership, serving
  snapshot direction, and the current control-plane usage-store split
- [Verification architecture](reference/verification-architecture.md):
  deeper reference for simulation seams, harness ownership, and verification
  corpora that extend the stable testing architecture
- [Reliability posture](reference/reliability-posture.md):
  stable proof-discipline reference for semantic waits, bounded budgets,
  deterministic hardship, and helper ownership
- [CI failure investigation](reference/ci-failure-investigation.md):
  evidence-first playbook for reproducing, classifying, and fixing CI failures
  without cargo-cult timeout increases
- [MicroVM and service-control baseline](reference/microvm-service-baseline.md):
  concise current baseline for the landed krun-backed microVM runtime,
  `ctx.services.*` integration, and `neovex compose ...` control surface
- [macOS machine image and control flows](reference/macos-machine-flow.md):
  current source-backed diagrams for the pinned Podman/Quay macOS bring-up
  image, the later Neovex-owned image track, macOS image pull/materialization,
  and host/guest service execution flow
- [HTTP and WebSocket API](reference/http-api.md):
  native and optional Convex route catalog
- [WebSocket protocol](reference/websocket-protocol.md):
  canonical version negotiation, handshake, framing, ordering, and reconnect
  contract for native and Convex WebSocket clients
- [Structured errors](reference/errors.md):
  canonical public error envelope, code taxonomy, severity model, and channel
  wrapping rules
- [Firebase WebSocket Listen](reference/firebase-websocket-listen.md):
  browser-facing Firestore `Listen` framing, origin policy, and close-code
  contract
- [Firebase compatibility](reference/firebase-compatibility.md):
  current Firestore SDK/runtime support matrix, transport boundaries, and
  first-party `@neovex/firebase` scope
- [Firebase application auth contract](reference/firebase-auth-contract.md):
  current Firebase route-family auth inputs, settled principal-resolution
  truth, and the server-edge auth-entry contract used by the completed
  boundary-hardening waves
- [Runtime capability and adapter boundary](reference/runtime-adapter-boundary.md):
  canonical ownership model and landed baseline for adapter-owned runtime
  compatibility shims versus provider-neutral `runtime_host/*` capabilities
- [Server auth and runtime trust](reference/server-auth-runtime-trust.md):
  completed post-Firebase trust baseline for server-owned auth,
  provider-family sharing, runtime bootstrap ownership, and trusted metadata
  contracts
- [Firebase migration guide](reference/firebase-migration-guide.md):
  pragmatic import/config/transport migration path from Firestore apps to the
  current `@neovex/firebase` and Neovex server surface
- [Cloud Functions artifact contract](reference/cloud-functions-artifact-contract.md):
  sibling `.neovex/firebase/` artifact family, runtime bundle envelope, and
  exact import-resolution strategy for the first Cloud Functions slice
- [Cloud Functions target binding contract](reference/cloud-functions-target-binding-contract.md):
  `targets.json` schema, deploy-time target metadata, binding validation, and
  first-slice explicit rejections for Cloud Functions-compatible handlers
- [Cloud Functions root defaults contract](reference/cloud-functions-root-defaults-contract.md):
  `firebase-functions/v2` root import behavior, `setGlobalOptions()`
  inheritance order, and first-slice fail-fast boundaries
- [Cloud Functions app-root and admin contract](reference/cloud-functions-app-root-and-admin-contract.md):
  shared Firebase and standalone app-root discovery rules, `.neovex/firebase/`
  artifact ownership, and the first covered `firebase-admin` method matrix
- [Cloud Functions compatibility](reference/cloud-functions-compatibility.md):
  current Firebase-v2 and standalone Functions Framework support matrix,
  durable delivery semantics, covered admin/option surfaces, and explicit
  non-goals
- [Cloud Functions migration guide](reference/cloud-functions-migration-guide.md):
  practical migration path for covered `firebase-functions/v2` and
  `@google-cloud/functions-framework` apps onto the current Neovex server
  surface
- [Deploy admin API](reference/deploy-admin-api.md):
  authenticated staging, diffing, validation, and generation activation
  contract behind `neovex deploy`
- [CLI reference](reference/cli.md):
  server flags plus the current service, machine, and encryption command surface
- [Encryption at rest reference](reference/encryption.md):
  operator guide for local encryption setup, key providers, migration, and
  recovery workflows
- [krun VMM host validation](reference/krun-vmm-host-validation.md):
  Linux-side build, install, and evidence capture runbook for historical
  patched-crun VMM validation and regression reruns
- [Convex compatibility](convex/compatibility.md):
  current Convex-surface scope, limits, and demo entrypoints
- [Runtime execution architecture rationale](research/runtime-execution-architecture-rationale.md):
  why Neovex embeds V8 via deno_core fork, why the workerd model is not
  pursued, and what future paths exist
- [macOS host-vs-guest control-plane rationale](research/macos-host-vs-guest-control-plane-rationale.md):
  historical tradeoffs behind rejected macOS control-plane shapes and why the
  current host-resident hybrid control plane won for v1
- [Versioned serving snapshot design note](research/versioned-serving-snapshot-design-note.md):
  implementation-grade north-star for the next server-side read-surface
  promotion after the `SA8` materialized-serving slice
- [Demos](../demos/README.md):
  demo layout and run commands
- [Plans](plans/README.md):
  plan index for active execution control planes, deferred design work, and
  archived execution history

## Layout

- `architecture/`: stable architecture deep dives that extend
  `ARCHITECTURE.md`
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
