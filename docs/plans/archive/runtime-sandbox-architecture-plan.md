# Runtime And Sandbox Architecture Plan

Completed historical baseline for clarifying Neovex's execution-runtime and
sandbox-orchestration boundaries before adding more backends and isolation
tiers.

Reviewed against:

- `ARCHITECTURE.md`
- `docs/plans/README.md`
- `docs/plans/wasmtime-backend-plan.md`
- `docs/reference/microvm-service-baseline.md`
- `docs/plans/archive/vmm-infrastructure-plan.md`
- `docs/plans/archive/microvm-runtime-plan.md`
- `crates/neovex-runtime/src/lib.rs`
- `crates/neovex-runtime/src/limits.rs`
- `crates/neovex-runtime/src/backends/mod.rs`
- `crates/neovex-runtime/src/worker_loop/mod.rs`
- `crates/neovex-runtime/src/runtime.rs`
- `crates/neovex-runtime/src/host.rs`
- `crates/neovex-runtime/src/error.rs`
- `crates/neovex-runtime/src/module_loader.rs`
- `crates/neovex-server/src/execution/mod.rs`
- `crates/neovex-server/src/adapters/convex/mod.rs`
- `crates/neovex-server/src/adapters/convex/registry/loading.rs`

---

## Status

- Completed
- Owner: execution and sandbox architecture cleanup
- Last updated: 2026-04-11

## Purpose

Own the naming cleanup and seam verification needed so Neovex can evolve
cleanly in two independent directions:

- additional execution runtimes and language engines such as V8 today and
  wasmtime next
- additional sandbox and isolation backends such as a krun-backed path now and
  Firecracker or gVisor later

This plan exists to stop overloading the word `runtime` across too many layers
before those additions land. The goal is not a cosmetic rename pass. The goal
is to make the crate layout, module names, and public enums match the real
architecture so later backend work stays readable and maintainable.

## Relationship To Other Plans

- `docs/plans/wasmtime-backend-plan.md` is the deferred consumer for the
  execution-backend seam this plan clarifies.
- `docs/reference/microvm-service-baseline.md` is the stable summary of the
  sandbox-orchestration seam this plan helped clarify; the archived
  `vmm-infrastructure-plan.md` and `microvm-runtime-plan.md` hold the detailed
  historical execution record.
- `docs/plans/distribution-plan.md` should follow the crate and package naming
  settled here rather than inventing its own execution/sandbox terminology.
- `docs/plans/layered-admission-control-plan.md` remains separate. This plan
  does not reopen already-landed executor and admission behavior; it only
  clarifies the ownership boundaries those mechanisms live behind.

## Current Assessed State

- `neovex-runtime` is the right top-level crate for code execution, but its
  current internal layout now distinguishes generic execution ownership from
  V8-owned backend modules, but the crate is still single-backend in practice
  and the host ABI remains Convex-shaped.
- `RuntimeBackendKind` now exposes `V8` rather than `DenoCore`; deferred
  backend plans and architecture docs must keep that backend-family naming
  consistent.
- `crates/neovex-runtime/src/runtime/` now holds the generic execution facade,
  bundle contract, driver, and the current bootstrap glue, while V8-owned
  startup snapshot and warm-pool code lives under
  `crates/neovex-runtime/src/backends/v8/`.
- The remaining `runtime/bootstrap/` ops/source/state tree still depends on the
  V8 embedder surface (`Extension`, `JsRuntime`, `OpState`, and related
  types). Treat it as current V8-backed bootstrap glue, not as a proven
  backend-neutral layer.
- `crates/neovex-runtime/src/backends/` and `worker_loop/` now express the
  generic worker/backend seam. Backend-specific deferred-drop ownership and
  reusable-runtime pools should continue to stay behind those backend-owned
  modules as more runtimes land.
- The public `NeovexRuntime` struct is currently the crate-level execution
  facade, not a V8 isolate or a backend-specific runtime object. The plan must
  keep that distinction explicit while backend-specific owners move below it.
- `crates/neovex-server/src/execution/` is server-side execution glue and
  read-tracking, not the execution runtime crate itself. That rename removes
  the most confusing server/runtime naming collision.
- The host ABI still carries Convex-oriented semantics at the adapter layer,
  but the generic runtime crate no longer owns Convex wire names.
  `HostCallOperation` now serializes generic snake_case operation names, and
  Convex `convex.*` wire labels live behind adapter-owned encoding in
  `crates/neovex-server/src/adapters/convex/host_bridge/contract.rs`.
- `ConvexRuntime` and `ConvexRuntimeError` are no longer re-exported from the
  generic runtime crates, so Convex-specific naming stays adapter-owned instead
  of leaking through the public execution facade.
- `RuntimeBundle` and the current registry loading path are JavaScript bundle
  concepts tied to `.mjs` and `.sha256`, not yet generic artifacts for multiple
  backend families.
- `RestrictedModuleLoader` now uses execution-local vocabulary for bundle-root
  import restrictions, avoiding future ambiguity with a dedicated
  `neovex-sandbox` crate.
- `neovex-sandbox` now exists as the dedicated sandbox/orchestration crate,
  holding generic sandbox lifecycle nouns and published-endpoint types without
  pulling those concerns into `neovex-runtime`.
- The deferred `vmm-infrastructure-plan.md` and `microvm-runtime-plan.md` now
  target `neovex-sandbox` plus backend-owned `krun` internals, but they remain
  deferred until the first concrete backend work starts behind the landed
  `RS4` scaffold and server-facing sandbox seam.

## Current Review Findings

- Keep `neovex-runtime` as the execution crate. That name remains correct for
  V8, wasmtime, and future code-execution runtimes.
- Add a separate sandbox/orchestration crate. The canonical target name is
  `neovex-sandbox`, not `neovex-vmm`, because the scope is broader than one VMM
  implementation and should cover future isolation backends such as gVisor.
- Public execution-backend names should describe stable engine or runtime
  families, not helper libraries. The immediate rename target is
  `RuntimeBackendKind::DenoCore -> RuntimeBackendKind::V8`.
- Keep `NeovexRuntime` by default as the generic execution facade unless a
  later backend addition proves that the public contract no longer fits. The
  immediate cleanup target is to make backend-specific owners explicit beneath
  that facade, not to rename the facade preemptively.
- Internal implementation modules should start with `backends::v8`, not a
  deeper `backends::v8::deno_core` split yet. Add a second implementation layer
  only when the codebase actually supports more than one V8 host stack.
- RS3 should also shrink the V8 noun leakage across generic seams. Names such
  as `DenoRuntimeBackendFactory`, `RuntimeWorkerIsolatePool`, and other
  `isolate` or `snapshot`-centric types should become V8-local unless the
  corresponding concept is truly generic across backends.
- The server-side glue now belongs under `src/execution/`, which is clearer
  than the old `src/runtime/` path because it describes server composition work
  rather than the execution crate itself.
- `src/execution/` remains the preferred target over narrower names such as
  `dispatch/` or `invocation/`, because the current server glue includes host
  calls, read tracking, and subscriptions in addition to invocation dispatch.
- `RuntimeBundle` should stay stable until the first non-JS backend lands, but
  this plan owns the eventual split toward artifact names that reflect the
  actual language/runtime contract.
- The former Convex-shaped host ABI naming leak is now closed at the adapter
  boundary. Future host-ABI evolution can work from adapter-owned wire types
  instead of reopening generic runtime naming.
- Deferred sandbox plans should adopt generic public sandbox nouns such as
  `SandboxBackend`, `SandboxSpec`, `SandboxHandle`, and published-endpoint
  types, while keeping VM-specific implementation nouns under backend-owned
  modules such as `backends::krun` or a future `backends::firecracker`.
- `RS4` is now landed: the workspace includes `neovex-sandbox`, the `neovex`
  facade re-exports the sandbox surface, and `neovex-server` owns the first
  explicit `SandboxCatalog` seam with empty defaults and sandbox-aware router
  builder variants.

## Cleanup Invariants

- `neovex-runtime` must remain a zero-workspace-dependency execution crate.
- `neovex-sandbox` must not become a backdoor second server crate. It owns
  sandbox lifecycle, backend process/VM orchestration, networking exposure,
  checkpoint/restore, and related state only.
- `neovex-server` remains the integration point that wires engine, execution
  runtime, and sandbox backends together.
- Do not force sandbox concerns into the execution-backend seam.
- Do not force wasmtime or future runtime work into the sandbox seam.
- Preserve current Convex compatibility behavior, runtime diagnostics,
  cooperative scheduling, and bundle-integrity checks while names move.
- Prefer stable semantic names in public APIs. Keep library or embedder names
  internal unless a concrete operator-facing distinction truly depends on them.

## Feature Preservation Matrix

| Area | Must stay true during cleanup | Notes |
| --- | --- | --- |
| Convex JS bundle execution | existing `functions.json` + `bundle.mjs` + `bundle.sha256` flow still works | rename and seam extraction first, behavior changes later |
| Runtime diagnostics | `/debug/runtime/metrics` continues to report meaningful execution settings | metric field names may evolve only with compatible documentation updates |
| Executor and worker-loop behavior | current warm-pool and cooperative execution semantics remain intact | this plan is not an admission-control rewrite |
| Wasmtime path | deferred wasmtime work can target a cleaner execution-backend seam | avoid baking V8-only names deeper into generic modules |
| VMM / Firecracker path | deferred sandbox work can target a dedicated sandbox crate | avoid making krun-backed sandboxing or Firecracker look like language runtimes |
| Server integration | `neovex-server` remains the composition root | runtime crate should not gain engine/storage dependencies |

## Control Plane Rules

- Work one roadmap item at a time.
- Prefer behavior-neutral rename or seam-extraction changes before any new
  backend functionality.
- Update this plan, `docs/plans/README.md`, and any touched architecture docs
  in the same change set when a canonical name changes.
- If a generic name still points at one concrete implementation, either narrow
  the name or extract the real generic seam before adding another backend.
- When in doubt, name public types by engine or contract, and name internal
  modules by implementation.

## Canonical Design Decisions

### `neovex-runtime` remains the execution crate

The crate name is still right. It should own invocation contracts, host ABI,
executor policy, scheduling, metrics, and pluggable code-execution backends.

It should not own:

- Podman/buildah orchestration
- conmon/crun process management
- Firecracker process or jailer management
- VM endpoint publication
- checkpoint/restore orchestration

### `neovex-sandbox` is the canonical new sandbox crate

The sandbox layer is distinct from code execution. It owns isolation and
workload-instance lifecycle regardless of whether the backend is krun-backed,
Firecracker, or a future gVisor-based path.

This crate should expose stable concepts such as:

- sandbox backend selection
- sandbox instance lifecycle
- endpoint publication
- snapshot and checkpoint handles
- backend-specific health or status projection

### Public backend names should not leak `deno_core`

`deno_core` is the current V8 embedder implementation, not the product-level
backend concept.

The current rename target is:

- public config and diagnostics: `DenoCore` -> `V8`
- initial internal module naming: `backends::v8`

If Neovex later supports a second V8 host stack such as workerd-style hosting,
that distinction should first appear internally under `backends::v8`. Only
promote a second public dimension if operators truly need to choose between
multiple V8 host stacks.

### `NeovexRuntime` is the generic execution facade unless proven otherwise

`NeovexRuntime` currently acts as the public execution facade: it owns the
`HostBridge`, `RuntimePolicy`, executor access, and top-level invocation APIs.
That role can still make sense in a multi-backend world.

The immediate goal is therefore:

- keep `NeovexRuntime` as the generic facade by default
- make backend-specific runtime owners and backend factories explicit below it
- revisit the public name only if multiple backend families make the facade
  contract misleading in practice

This avoids churn before the wasmtime path proves whether the generic facade
still fits naturally.

### Do not pre-design around Bun

Bun is not a near-term naming anchor for this repo. If Neovex ever embeds a
JavaScriptCore-based execution path or a Bun-derived host stack, it should land
as a concrete new execution-backend design at that time rather than warping the
current naming around speculation.

Today the real concrete execution families are:

- V8
- wasmtime

That is enough to justify removing `DenoCore` from the public naming surface.

### Server glue should be named as glue

The server-side glue used to live under `src/runtime/`, which overloaded the
same word as the execution crate. The landed rename to `src/execution/` makes
that boundary clearer.

### Artifact naming should follow backend families over time

`RuntimeBundle` is acceptable as a transitional name while Neovex is still
single-backend in production, but once multiple backend families coexist the
artifact layer should become explicit enough to model JavaScript bundle
artifacts separately from any WASM/component artifacts. The target direction is
an explicit execution-artifact seam, for example `ExecutionArtifact` or
equivalent, with concrete JavaScript and WASM/backend-specific artifact types
below it. The exact public names do not need to be frozen before the second
artifact family lands.

### Module-loader naming should avoid sandbox ambiguity

The execution crate now uses `RestrictedModuleLoader`, which describes
bundle-root import restrictions without implying VM or process sandboxing. That
name should remain execution-local even after `neovex-sandbox` exists.

### Legacy Convex aliases should stay adapter-owned, not facade-owned

The generic crates no longer re-export `ConvexRuntime` or
`ConvexRuntimeError`. Adapter-owned Convex helper types may still use Convex
names where they are actually Convex-specific, but those names should not
define the generic runtime vocabulary again.

## Success Criteria

This plan is complete only when all of the following are true:

1. `neovex-runtime` is clearly the execution crate and its generic internal
   modules genuinely describe backend families rather than one V8
   implementation.
2. A dedicated `neovex-sandbox` crate or equivalent landed seam exists for VM,
   process, and checkpoint-oriented orchestration work.
3. Public backend naming no longer exposes `deno_core` as the primary product
   concept.
4. Server-side execution glue is named clearly enough that contributors do not
   confuse it with the execution crate.
5. Deferred wasmtime, VMM, Firecracker, and distribution plans all align with
   the settled naming and seams.

## Verification Contract

- Scope naming grep checks to live code and live architecture docs. Closed or
  historical plans may retain older names as research context.
- Documentation and naming review:
  - `rg -n "\\bDenoCore\\b|SandboxedModuleLoader|crates/neovex-server/src/runtime/|backend.rs" ARCHITECTURE.md crates/neovex-runtime crates/neovex crates/neovex-server docs/plans/README.md docs/plans/wasmtime-backend-plan.md docs/research`
  - `rg -n "retained_jsruntime_pool" ARCHITECTURE.md crates/neovex-runtime crates/neovex crates/neovex-server docs/plans/README.md docs/plans/wasmtime-backend-plan.md docs/research`
  - `rg -n "convex\\." crates/neovex-runtime/src`
  - `rg -n "\\bConvexRuntime\\b|\\bConvexRuntimeError\\b" crates/neovex-runtime crates/neovex`
- Required formatting:
  - `cargo fmt --all --check`
- Required static verification:
  - `make check`
  - `make clippy`
- Required focused runtime verification for code changes:
  - `cargo test -p neovex-runtime`
  - `cargo test -p neovex-server`
- If public diagnostics or config fields change:
  - update docs and any affected server tests in the same change set

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| RS1 | done | establish canonical terminology and create the active control plan | none |
| RS2 | done | renamed the public backend kind from `DenoCore` to `V8`, updated serialized diagnostics expectations to `v8`, and aligned the deferred wasmtime plan where it described backend-family values rather than the `deno_core` implementation detail | RS1 |
| RS3 | done | completed the first-wave runtime naming cleanup: backend-family module layout, V8-local pool/snapshot ownership, generic diagnostics vocabulary, removal of generic Convex aliases, and explicit deferral of the Convex-shaped host-op ABI cleanup | RS1 |
| RS4 | done | added `neovex-sandbox`, exposed generic sandbox lifecycle nouns and published-endpoint types, and landed the first stable server-facing `SandboxCatalog` seam plus sandbox-aware router builders | RS1 |
| RS5 | done | renamed server-side execution glue from `src/runtime/` to `src/execution/` so the server composition layer no longer reads like the execution crate | RS1 |
| RS6 | done | aligned deferred plans, architecture docs, and durable naming references with the landed execution/runtime cleanup | RS2, RS3, RS5 |

## Dependency Graph

- `RS1 -> RS2`
- `RS1 -> RS3`
- `RS1 -> RS4`
- `RS1 -> RS5`
- `RS2 -> RS6`
- `RS3 -> RS6`
- `RS5 -> RS6`

## Recommended Delivery Order

1. `RS1`
2. `RS2`
3. `RS3`
4. `RS5`
5. `RS6`
6. `RS4`

`RS2` through `RS5` fan out from `RS1` and may be reordered or landed in
separate change sets when that reduces merge risk. The default execution rule
still applies: take one roadmap item at a time per autonomous work burst.

## Implementation Checkpoints

- [x] Confirm that current crate and module names overload `runtime` across
  execution and server glue layers.
- [x] Decide that `neovex-runtime` remains the execution crate.
- [x] Decide that `neovex-sandbox` is the target sandbox/orchestration crate
  name.
- [x] Decide that public execution-backend naming should stop exposing
  `deno_core`.
- [x] Decide that `NeovexRuntime` stays the default generic facade unless later
  backend work proves otherwise.
- [x] Inventory the exact public rename set for `DenoCore -> V8`.
- [x] Land the public `RuntimeBackendKind::DenoCore -> V8` rename and update
  serialized diagnostics expectations.
- [x] Stage the `neovex-runtime` internal module move toward
  `backends::v8` and future `backends::wasmtime`, including
  `backend.rs -> backends/mod.rs`.
- [x] Inventory V8-specific nouns still leaking through generic seams, including
  backend factories, isolate-pool types, and diagnostics fields.
- [x] Stage the public isolate-shaped cleanup set so generic limits and
  diagnostics stop exposing `isolate` names where they really mean backend
  runtime instances or runtime-pool behavior.
- [x] Remove `ConvexRuntime` and `ConvexRuntimeError` from the generic facade
  crates so Convex naming stays adapter-owned.
- [x] Move `HostCallOperation` wire-name ownership behind adapter-specific
  encoding so generic runtime types stop carrying Convex `convex.*` labels.
- [x] Rename `SandboxedModuleLoader` to a less ambiguous execution-local name.
- [x] Stage a minimal `neovex-sandbox` crate scaffold and the first server-side
  trait seam it needs.
- [x] Stage the server `src/runtime/` glue rename.
- [x] Update `ARCHITECTURE.md` and the deferred consumer plans touched by the
  naming cleanup.

## Work Items

### RS2. Public Backend Naming Cleanup

Rename public enums, diagnostics fields, and related docs so the product-level
backend concept is `V8`, not `DenoCore`.

Current rename inventory:

| Surface | Current | Target | Notes |
| --- | --- | --- | --- |
| `crates/neovex-runtime/src/limits.rs` enum variant | `RuntimeBackendKind::DenoCore` | `RuntimeBackendKind::V8` | canonical public type rename; the acronym variant uses an explicit serde rename so diagnostics serialize as `v8` |
| `crates/neovex-runtime/src/limits.rs` default limits | `backend_kind: RuntimeBackendKind::DenoCore` | `backend_kind: RuntimeBackendKind::V8` | keeps diagnostics and defaults aligned with the renamed variant |
| `crates/neovex-runtime/src/lib.rs` and `crates/neovex/src/lib.rs` re-exports | re-exported enum includes `DenoCore` variant | re-exported enum includes `V8` variant | no path rename expected; consumers see the variant rename through the existing type |
| `crates/neovex-server/src/protocol.rs` diagnostics payload | `runtime_backend: RuntimeBackendKind` serializes as `deno_core` | same field serializes as `v8` | field name stays `runtime_backend`; only the payload value changes |
| `crates/neovex-server/src/http/metadata.rs` runtime diagnostics route | emits `limits.backend_kind` as `deno_core` today | emits `v8` | no route-path or response-shape rename required |
| `crates/neovex-server/src/tests/registry_and_license/runtime_metrics.rs` | asserts `runtime_backend == "deno_core"` | assert `runtime_backend == "v8"` | update alongside the code rename |

RS2 explicitly excludes:

- direct Rust imports of the `deno_core` crate
- research or architecture docs that discuss the actual `deno_core` fork as an
  implementation choice
- internal backend-factory and module-path cleanup such as
  `DenoRuntimeBackendFactory`, which belongs to `RS3`

RS2 follow-on doc rule:

- docs that describe the product/backend choice or serialized `backend_kind`
  values should switch to `V8` / `v8`
- docs that explain how the current V8 backend is implemented may still say
  `deno_core` where they mean the actual embedder library

### RS3. Runtime Internal Module Layout Cleanup

Refactor `neovex-runtime` so generic modules represent real backend families
and concrete implementation names live under implementation-specific module
paths.

This item owns:

- `backend.rs -> backends/mod.rs`
- an initial `backends::v8` layout without a speculative second V8 embedder
  level
- clarifying `NeovexRuntime` as the generic facade while backend-specific
  runtime owners move below it
- renaming backend-factory, pool, and worker-loop types that still expose V8
  implementation nouns through generic module paths
- removing `ConvexRuntime` and `ConvexRuntimeError` from the generic facade
  crates
- renaming `SandboxedModuleLoader` to `RestrictedModuleLoader`
- cleaning up or explicitly deferring the Convex-coupled `HostCallOperation`
  serde naming

RS3 landed slices so far:

- `backend.rs` moved to `backends/mod.rs`, with the concrete V8 implementation
  now under `backends::v8`
- `DenoRuntimeBackendFactory` became `V8RuntimeBackendFactory`
- `RuntimeWorkerIsolatePool`, `RuntimeStartupSnapshot`,
  `RuntimeConstructionMode`, and `ReusableRuntime` became
  `V8WorkerRuntimePool`, `V8StartupSnapshot`, `V8RuntimeConstructionMode`, and
  `ReusableV8Runtime`
- `SandboxedModuleLoader` became `RestrictedModuleLoader`
- public diagnostics and limit names now speak in runtime instances and runtime
  pools (`active_runtime_instances`, `runtime_pool_*`,
  `fallback_cross_runtime_dispatches`, `max_concurrent_runtime_instances`),
  with the server JSON payload, shared test helpers, and CLI flag/docs updated
  in the same slice
- `ConvexRuntime` and `ConvexRuntimeError` were removed from
  `neovex-runtime` and `neovex`
- V8 startup-snapshot and warm-pool ownership moved from generic
  `runtime/bootstrap` re-export paths into `backends::v8::{startup,warm_pool}`
- direct `deno_core` imports in production runtime code now route through
  `backends::v8::embedder`, keeping the embedder name behind the V8 boundary
- cooperative deferred runtime teardown now flows through
  `DeferredV8RuntimeDropQueue` instead of a raw `Vec<deno_core::JsRuntime>` in
  the generic worker-loop struct

Deferred follow-on beyond RS3:

None from the naming cleanup itself. `HostCallOperation` wire-name ownership now
lives behind adapter-specific encoding in
`crates/neovex-server/src/adapters/convex/host_bridge/contract.rs`, so future
host-ABI work can evolve that boundary without reopening generic runtime
naming.

RS3 first-wave boundary:

- treat backend-factory, worker-loop, snapshot, pool, and raw-`JsRuntime`
  naming as the first cleanup wave
- keep `RuntimeBundle`, `RuntimePoolKind::StartupSnapshotCache`, and
  `RuntimeModuleStateSemantics::WarmPerBundle` on the artifact/public-contract
  track for now so RS3 does not collapse into a mixed internal-plus-public
  contract rewrite

### RS4. Sandbox Crate Extraction

Introduce `neovex-sandbox` with the minimal stable concepts needed for future
krun and Firecracker work without coupling that lifecycle layer into
`neovex-runtime`.

RS4 owns the first generic sandbox seam:

- workspace scaffold for `crates/neovex-sandbox/`
- generic public nouns for sandbox lifecycle and published endpoints
- a server-facing integration seam that keeps `neovex-server` as the
  composition root
- backend-owned internal module namespaces for the first krun path, which may
  use OCI/buildah/conmon/crun tooling internally, and future Firecracker or
  gVisor paths

RS4 explicitly does not own:

- buildah/conmon/crun/libkrun implementation details
- Firecracker implementation details
- `ctx.services.*` exposure or server adapter policy
- Compose parsing or CLI UX

### RS5. Server Glue Naming Cleanup

Rename or restructure the server-side execution glue so contributors can
visually distinguish the execution crate from the server's adapter and
invocation wiring. This rename landed as `crates/neovex-server/src/execution/`;
future server composition work should use that path rather than recreating a
top-level `runtime/` directory.

### RS6. Documentation Alignment

Update deferred plans, architecture docs, and any other durable references so
future work starts from the corrected naming and ownership boundaries. The
current cleanup slice aligned the active architecture doc, the deferred
wasmtime plan, the plan index, and the bundle-distribution research note.

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-11 | RS1 | done | Reviewed the current runtime, server, and deferred backend plans; confirmed that the main problem is not the `neovex-runtime` crate name but the leakage of V8-specific and server-glue-specific details behind generic `runtime` names. Locked the canonical direction: keep `neovex-runtime` for execution, introduce `neovex-sandbox` for isolation/orchestration, target `V8` rather than `DenoCore` in public backend naming, and keep `NeovexRuntime` as the default generic facade unless later backend work proves that contract no longer fits. | document review against `ARCHITECTURE.md`, `docs/plans/README.md`, `docs/plans/wasmtime-backend-plan.md`, `docs/plans/vmm-infrastructure-plan.md`, `docs/plans/microvm-runtime-plan.md`, `crates/neovex-runtime`, and `crates/neovex-server` | Start RS2 with a concrete public rename inventory |
| 2026-04-11 | RS2 | in_progress | Inventoried the live `DenoCore` public surface. The actual product-facing rename is narrower than the repo-wide `deno_core` footprint: it is centered on `RuntimeBackendKind`, its serialized `runtime_backend` diagnostics value, and the tests/docs that assert those serialized names. The actual `deno_core` crate imports and implementation-rationale docs remain intentionally out of scope for this item. | `rg -n "DenoCore|deno_core|backend_kind|runtime_backend|RuntimeBackendKind" crates docs packages`; review against `crates/neovex-runtime/src/limits.rs`, `crates/neovex-runtime/src/lib.rs`, `crates/neovex/src/lib.rs`, `crates/neovex-server/src/protocol.rs`, `crates/neovex-server/src/http/metadata.rs`, and `crates/neovex-server/src/tests/registry_and_license/runtime_metrics.rs` | Land the code rename for `RuntimeBackendKind::DenoCore -> V8` and update serialized diagnostics expectations |
| 2026-04-11 | RS2 | done | Landed the public backend rename in code and the immediate deferred-plan consumer. `RuntimeBackendKind` now exposes `V8`, the runtime diagnostics payload serializes `runtime_backend` as `v8`, and the wasmtime backend plan now distinguishes backend-family naming (`V8`) from the `deno_core` implementation detail. | `cargo test -p neovex-runtime --lib limits`; `cargo test -p neovex-server runtime_metrics_route_returns_limits_and_metrics_when_convex_support_is_enabled -- --nocapture`; `rg -n "\\bDenoCore\\b|json!\\(\\\"deno_core\\\"\\)|runtime_backend.*deno_core|backend_kind: RuntimeBackendKind::DenoCore" crates docs/plans/wasmtime-backend-plan.md` | Start RS3 by inventorying and renaming V8-specific internal nouns leaking through generic seams |
| 2026-04-11 | RS3 | in_progress | Inventory pass identified the first internal cleanup wave: generic seams still carried `DenoRuntimeBackendFactory`, `RuntimeWorkerIsolatePool`, raw `deno_core::JsRuntime` ownership inside worker loops, and public diagnostics counters named around isolates. It also clarified a useful boundary: keep snapshot/pool/backend-factory renames in RS3, but leave `RuntimeBundle` and bundle-facing public contract names on the artifact track so this step stays readable. | `rg -n "DenoRuntimeBackendFactory|DenoRuntimeBackend|RuntimeWorkerIsolatePool|isolate_pool|active_isolates|cross_isolate|isolate_pool_hits|isolate_pool_misses|isolate_pool_replacements|JsRuntime|StartupSnapshot|snapshot" crates/neovex-runtime/src/backends/mod.rs crates/neovex-runtime/src/metrics.rs crates/neovex-runtime/src/worker_loop crates/neovex-runtime/src/runtime/bootstrap crates/neovex-server/src/tests/registry_and_license/runtime_metrics.rs`; review against `crates/neovex-runtime/src/backends/mod.rs`, `crates/neovex-runtime/src/worker_loop/run_to_completion.rs`, `crates/neovex-runtime/src/worker_loop/cooperative.rs`, `crates/neovex-runtime/src/backends/v8/warm_pool.rs`, and `crates/neovex-runtime/src/metrics.rs` | Choose the first RS3 code slice: backend/module-path renames first, or diagnostics vocabulary first |
| 2026-04-11 | RS3 | in_progress | Landed the first RS3 code slice: the generic backend seam now lives under `crates/neovex-runtime/src/backends/`, with the concrete V8 implementation split to `backends::v8` and `DenoRuntimeBackendFactory` renamed to `V8RuntimeBackendFactory`. This keeps behavior unchanged while moving the module vocabulary toward backend families instead of embedder-library names. | `rg -n "crate::backend|DenoRuntimeBackendFactory|DenoRuntimeBackend|mod backend;" crates/neovex-runtime/src`; review against `crates/neovex-runtime/src/backends/mod.rs`, `crates/neovex-runtime/src/backends/v8/mod.rs`, `crates/neovex-runtime/src/lib.rs`, and `crates/neovex-runtime/src/worker_loop/run_to_completion.rs`; `cargo check -p neovex-runtime` | Continue RS3 with the next V8-internal rename slice, ideally the V8 pool and snapshot vocabulary cleanup |
| 2026-04-11 | RS3 | in_progress | Landed the second RS3 code slice: the V8-owned snapshot and reusable-runtime vocabulary is now explicit (`V8WorkerRuntimePool`, `V8StartupSnapshot`, `V8RuntimeConstructionMode`, `ReusableV8Runtime`), and the execution-local module loader no longer uses the overloaded `SandboxedModuleLoader` name. This leaves the remaining public isolate-shaped diagnostics and limit names as the clearest next cleanup target. | `rg -n "RuntimeWorkerIsolatePool|RuntimeStartupSnapshot|RuntimeConstructionMode|ReusableRuntime|SandboxedModuleLoader" crates/neovex-runtime/src`; `cargo check -p neovex-runtime`; `cargo test -p neovex-runtime --lib` | Continue RS3 with the public diagnostics and limits rename slice (`active_isolates`, `isolate_pool_*`, `fallback_cross_isolate_dispatches`, `max_concurrent_isolates`) |
| 2026-04-11 | RS3 | in_progress | Landed the third RS3 code slice: the public diagnostics and limits surface now uses generic runtime-instance and runtime-pool vocabulary instead of `isolate` names. This includes `RuntimeMetricsSnapshot`, tenant metrics, `RuntimeLimits`, server diagnostics JSON, shared test helpers, the `neovex` CLI flag/docs, and the focused runtime/server tests that exercise those payloads. | `cargo check -p neovex-runtime -p neovex-server`; `cargo test -p neovex-runtime --lib`; `cargo test -p neovex-server --no-run`; `cargo test -p neovex-server runtime_metrics_route_returns_limits_and_metrics_when_convex_support_is_enabled -- --nocapture`; `cargo test -p neovex-server convex_runtime_only_query_reuses_same_isolate_for_ctx_run_query -- --nocapture` | Continue RS3 by deciding whether the next cleanup target is the remaining raw `deno_core::JsRuntime` ownership in cooperative worker loops or the fate of the `ConvexRuntime` / `ConvexRuntimeError` legacy aliases |
| 2026-04-11 | RS3 | done | Finished the remaining naming cleanup inside `neovex-runtime`: removed the generic `ConvexRuntime` / `ConvexRuntimeError` re-exports, moved V8 startup-snapshot and warm-pool ownership under `backends::v8`, and routed deferred V8-runtime teardown through `DeferredV8RuntimeDropQueue`. The final host-ABI cleanup also moved Convex wire-name ownership behind adapter-specific encoding: `HostCallOperation` now serializes generic snake_case names in `neovex-runtime`, while the Convex adapter owns `convex.*` wire labels for metrics, tracing, and serde contracts. | `cargo check -p neovex-runtime -p neovex-server -p neovex -p neovex-bin -p neovex-testing`; `cargo test -p neovex-runtime`; `cargo test -p neovex-server` | Continue with RS5 / RS6 closeout work for the server rename and durable-doc alignment |
| 2026-04-11 | RS5 | done | Renamed the server-side execution glue root from `crates/neovex-server/src/runtime/` to `crates/neovex-server/src/execution/` and updated the affected server imports and focused tests. The top-level server composition layer now reads as server execution glue rather than the runtime crate itself. | `cargo check -p neovex-runtime -p neovex-server -p neovex -p neovex-bin -p neovex-testing` | Continue with RS6 so the durable docs and deferred consumer plans describe the landed names |
| 2026-04-11 | RS6 | done | Aligned the durable docs with the landed naming cleanup. `ARCHITECTURE.md`, `docs/plans/README.md`, `docs/plans/wasmtime-backend-plan.md`, and `docs/research/bundle-distribution-from-object-storage.md` now describe `backends::v8`, `src/execution/`, `RestrictedModuleLoader`, `RuntimeBackendKind::V8`, and the removed generic Convex aliases consistently. A final review also tightened the remaining bootstrap wording so `runtime/bootstrap/` is described as current V8-backed glue rather than a proven backend-neutral layer, and narrowed the naming verification scope to live docs instead of historical plans. | `rg -n "\\bDenoCore\\b|SandboxedModuleLoader|crates/neovex-server/src/runtime/|backend.rs" ARCHITECTURE.md crates/neovex-runtime crates/neovex crates/neovex-server docs/plans/README.md docs/plans/wasmtime-backend-plan.md docs/research`; `rg -n "retained_jsruntime_pool" ARCHITECTURE.md crates/neovex-runtime crates/neovex crates/neovex-server docs/plans/README.md docs/plans/wasmtime-backend-plan.md docs/research`; `rg -n "\\bConvexRuntime\\b|\\bConvexRuntimeError\\b" crates/neovex-runtime crates/neovex` | Continue with RS4 when sandbox/orchestration extraction work is ready |
| 2026-04-11 | RS6 | done | Rebased the deferred sandbox consumer plans onto the settled architecture decisions: both the VMM infrastructure and microVM runtime plans now target `neovex-sandbox`, keep public sandbox nouns generic, use a backend-owned `krun` internal module path for the first krun-backed stack, and keep `neovex-server` as the owner of the `ctx.services.*` projection. The microVM plan also now treats Compose/CLI work as a follow-on phase after core runtime and recovery verification. | `rg -n "neovex-vmm|process.signal|VmServiceManager" docs/plans/vmm-infrastructure-plan.md docs/plans/microvm-runtime-plan.md`; review against `docs/plans/archive/runtime-sandbox-architecture-plan.md`, `docs/plans/vmm-infrastructure-plan.md`, and `docs/plans/microvm-runtime-plan.md`; `cargo fmt --all --check` | Continue with RS4 and then start VMM Phase V3 or the first `neovex-sandbox` scaffold slice |
| 2026-04-11 | RS6 | done | Clarified the first sandbox backend family naming after the initial seam extraction. `neovex-sandbox` now uses `SandboxBackendKind::Krun`, and the deferred sandbox plans now target a backend-owned `krun` module while keeping OCI/buildah/conmon/crun vocabulary as implementation-detail language inside that backend. | `rg -n "SandboxBackendKind::Krun|backends/krun" crates/neovex-sandbox docs/plans ARCHITECTURE.md`; `cargo check -p neovex-sandbox -p neovex -p neovex-server` | Start the first concrete krun backend slice when sandbox backend implementation work begins |
| 2026-04-11 | RS4 | done | Landed the missing sandbox seam in code. Added the `neovex-sandbox` workspace crate with generic sandbox backend, spec, handle, status, endpoint, and error types; re-exported that surface from the `neovex` facade crate; and introduced the first explicit server-facing sandbox seam via `neovex-server::SandboxCatalog`, `EmptySandboxCatalog`, sandbox-aware router builders, and `AppState` sandbox-catalog injection. | `cargo fmt --all --check`; `cargo check -p neovex-sandbox -p neovex-server -p neovex`; `cargo test -p neovex-server empty_catalog_returns_none_for_unknown_service -- --nocapture` | Mark this plan complete and treat it as the historical baseline for future runtime or sandbox follow-on plans |
