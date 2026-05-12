# Plan: Wasmtime Backend

Canonical deferred design and execution plan for adding a wasmtime-based WASM
backend to `neovex-runtime` alongside the existing V8 backend implemented via
`deno_core`.

This document owns the durable forward-looking context for WASM Component Model
execution via wasmtime: backend abstraction refactor, WIT interface definitions,
cooperative fuel-based scheduling, module compilation/caching, bundle format
extension, and the phased roadmap for promotion.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** promote only after `docs/plans/v8-locker-fork-plan.md`
  Phase 5 (cooperative worker loop) reaches `done` status, so the cooperative
  scheduling seam is stable before this plan introduces a second backend beneath
  it

## How To Use This Plan

- Read this before starting any wasmtime or WASM backend implementation work.
- Treat it as the canonical control plane for the wasmtime workstream once
  promoted.
- Do not start implementation until the activation gate is met.
- When promoted, implement exactly one phase at a time and record verification
  in the Execution Log before marking a phase `done`.

## Control Plan Rules

This document is the durable control plane for the wasmtime backend workstream.
The source of truth is:

1. the current git worktree
2. this plan's `Phase Status Ledger`, `Implementation Checkpoints`, and
   `Execution Log`
3. `ARCHITECTURE.md` for the landed `WorkerLoopFactory` / `WorkerLoop` seam and
   shared runtime invariants
4. `docs/plans/v8-locker-fork-plan.md` for the current V8 cooperative scheduling
   surface this plan must not break

Do not rely on prior chat transcripts as progress state.

### Status model

- `todo`: not started; eligible when hard dependencies and gate notes are
  satisfied
- `in_progress`: actively being implemented; keep exactly one phase in this
  state per autonomous execution run
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification has been recorded
- `deferred`: intentionally parked behind a product or benchmarking gate

### Recovery loop for every new session

1. Reread this `Control Plan Rules` section, `Phase Status Ledger`,
   `Implementation Checkpoints`, `Phase Order and Dependencies`, and
   `Execution Log`.
2. Inspect the current git worktree and reconcile it against this plan before
   picking new scope.
3. If any phase is already `in_progress`, resume that phase first.
4. If the worktree is dirty, identify which phase owns the changes and update
   that phase's checkpoint or log entry before starting new work.
5. Implement exactly one phase by default.
6. Record verification in `Execution Log` before marking a phase `done`.
7. If blocked, record the blocker here before stopping.

---

## Current Architecture Boundary

- `ARCHITECTURE.md` is the source of truth for the landed `WorkerLoopFactory` /
  `WorkerLoop` seam.
- `docs/plans/v8-locker-fork-plan.md` is the source of truth for the current V8
  backend implementation via `deno_core` + Locker cooperative scheduling.
- `docs/plans/archive/raw-v8-warm-backend-plan.md` is the source of truth for the
  future raw-V8 warm backend.
- This plan does **not** reopen the decision to keep scheduling above the
  runtime. That seam is already the right one.
- This plan does **not** replace the V8 path. Wasmtime is additive.

`ARCHITECTURE.md` already names this direction:

> _"a database-native WASM plugin ABI for tightly scoped extensions. WASM is
> therefore an additive path for Neovex, not a planned replacement for the
> Convex compatibility runtime."_

> _"a schema-owned public API contract, planner-enforced policy, and a typed,
> capability-scoped plugin ABI rather than an untyped general escape hatch."_

The WASI Component Model is that typed, capability-scoped ABI.

## Why Wasmtime As A Separate Backend

The V8 and wasmtime backends serve different execution contracts, type
constraints, and scheduling models:

| Dimension | `V8` (implemented via `deno_core`) | `wasmtime` (WASM) |
|-----------|------------------|-------------------|
| Language | JavaScript / TypeScript | Any language → WASM component |
| Thread safety | `!Send` (`JsRuntime`) — worker-local only | `Send + Sync` — cross-thread capable |
| Cooperative yield | Locker acquire/release per poll tick | Fuel exhaustion trap |
| Host interface | `deno_core` ops (untyped JSON FFI) | WASI Component Model (typed WIT interfaces) |
| Module caching | Worker-local code cache + snapshot | Process-wide pre-compiled module cache |
| Memory limits | V8 heap limits (sandbox-coupled) | Per-Store `ResourceLimiter` (independent) |
| Determinism | Non-deterministic yield points | Deterministic fuel boundaries |
| Isolation | V8 isolate boundary | WASM sandbox + capability-based imports |

The key rules are:

- **Do not force wasmtime semantics into the V8 path.**
- **Do not force V8 constraints onto the wasmtime path.** (`!Send`, Locker,
  deferred destruction — none apply to wasmtime.)
- **Both backends share the same outer scheduler/admission/metrics seam.**
- Keep the V8 path honest as the Convex compatibility runtime.
- Put WASM execution behind `RuntimeBackendKind::Wasmtime` and explicit
  settings.

## Reference Implementations

| System | What it contributes | Review reference |
|--------|---------------------|------------------|
| Cloudflare workerd | C++ reference for multi-backend scheduling at scale; workers can run JS or WASM | `https://github.com/cloudflare/workerd` |
| Fermyon Spin | Rust + wasmtime reference for Component Model hosting, WIT-based host interfaces, per-request Store lifecycle, pre-compiled module caching | `https://github.com/fermyon/spin` |
| Fastly Viceroy | Rust + wasmtime reference for local WASI development; shows host function binding patterns | `https://github.com/fastly/Viceroy` |
| Bytecode Alliance wasmtime | Canonical WASM runtime; fuel, epochs, Component Model, ResourceLimiter | `https://github.com/bytecodealliance/wasmtime` |
| componentize-js | SpiderMonkey compiled to WASM component; shows how JS can target the Component Model (reference only, not a dependency) | `https://github.com/bytecodealliance/componentize-js` |
| Convex backend | Prior art for V8 + optional WASM execution in a reactive database context | `https://github.com/get-convex/convex-backend` |

### Strengths and fit matrix

| System | Best at | Weak fit for | Net lesson for Neovex |
|--------|---------|--------------|----------------------|
| Spin | Component Model hosting, WIT-driven host surfaces, trigger-based dispatch | reactive subscriptions, multi-tenant V8 | copy WIT interface design and Store lifecycle patterns |
| Viceroy | host function binding, WASI polyfill, local development | production scheduling, multi-tenant | copy host binding patterns |
| wasmtime | engine configuration, fuel/epoch, module caching, ResourceLimiter | application-level scheduling | use directly as a dependency, not a reference |
| workerd | multi-backend scheduling under one admission layer | Rust implementation template | copy the scheduling-above-runtime pattern (already landed in Neovex) |

## Proposed Public Shape

These names extend the existing configuration surface. The V8-side enums remain
unchanged.

```text
RuntimeBackendKind
  - v8                 (existing)
  - wasmtime           (new)

RuntimeExecutionModel
  - run_to_completion  (existing — works for both backends)
  - cooperative_locker (existing — V8 only)
  - cooperative_fuel   (new — wasmtime only)

RuntimePoolKind
  - startup_snapshot_cache       (existing — V8 only)
  - warm_pool                    (existing — V8 only)
  - precompiled_module_cache     (new — wasmtime, fresh Store per invoke)
  - retained_store_pool          (new — wasmtime, retained Store with reset)
```

Validation rules — reject at construction, not at runtime:

| `backend_kind` | Allowed `execution_model` | Allowed `pool_kind` |
|----------------|---------------------------|---------------------|
| `v8` | `run_to_completion`, `cooperative_locker` | `startup_snapshot_cache`, `warm_pool` |
| `wasmtime` | `run_to_completion`, `cooperative_fuel` | `precompiled_module_cache`, `retained_store_pool` |

## Proposed Internal Shape

### Backend abstraction refactor

The current `RuntimeBackendInvocation` contains `NeovexRuntime`, which should
remain the generic execution facade rather than being treated as V8-specific.
The canonical refactor hardens that boundary like this:

```text
RuntimeBackendInvocation (backend-agnostic envelope)
  - runtime: NeovexRuntime
  - watchdog: WatchdogTimer
  - bundle: RuntimeBundle
  - request: InvocationRequest
  - context: RuntimeInvocationContext
  - cancellation: Option<HostCallCancellation>
  - permit: SharedInvocationPermit

RuntimeBackendFactory (trait, receives host + policy at construction)
  -> create() -> RuntimeBackend

RuntimeBackend (trait, per-worker)
  -> invoke(RuntimeBackendInvocation) -> Result<Value>
```

The concrete V8 backend owns V8-local runtime pools and deferred runtime-drop
state below this envelope. The scheduling layer should not own those
backend-specific details.

### Cooperative backend driver abstraction

The `CooperativeScheduler<T>` is already generic. The V8-specific coupling is in
`CooperativeWorkerLoop` itself. The canonical refactor extracts a trait:

```text
CooperativeBackendDriver (trait, per-worker)
  - type Slot: CooperativeSlot
  - start_slot(envelope, activity_signal) -> Slot
  - finish_slot(slot, envelope)
  - idle_maintenance()

CooperativeSlot (trait, per-invocation)
  - poll_once() -> Runnable | Parked | Completed
  - is_ready_to_resume() -> bool
  - finish() -> Result<Value>

CooperativeWorkerLoop<D: CooperativeBackendDriver>
  - scheduler: CooperativeScheduler<CooperativeInvocation<D::Slot>>
  - driver: D
```

V8 implementation:

```text
V8LockerDriver
  - v8_runtime_pool: V8WorkerRuntimePool
  - deferred_runtime_drops: DeferredV8RuntimeDropQueue
  - Slot = CooperativeLockerRuntimeSlot
```

Wasmtime implementation:

```text
WasmtimeFuelDriver
  - engine: wasmtime::Engine (shared, cloned from factory)
  - module_cache: HashMap<BundleHash, Arc<Component>>
  - store_pool: VecDeque<ReusableStore>
  - linker: wasmtime::component::Linker<InvocationHostState>
  - Slot = WasmtimeFuelSlot
```

### Wasmtime engine and Store hierarchy

```text
WasmtimeBackendFactory (shared across all workers)
  - engine: wasmtime::Engine        (Send + Sync + Clone)
  - linker: component::Linker       (pre-built from WIT, shared)
  - module_cache: WasmtimeModuleCache (Send + Sync, DashMap)
  - host: Arc<dyn HostBridge>

WasmtimeBackend (per-worker)
  - engine: wasmtime::Engine        (cloned, cheap)
  - module_cache: Arc<WasmtimeModuleCache>
  - store_pool: VecDeque<ReusableStore>
  - linker: component::Linker       (cloned from factory)

InvocationHostState (per-Store, per-invocation)
  - bridge: Arc<dyn HostBridge>
  - context: RuntimeInvocationContext
  - cancellation: Option<HostCallCancellation>
  - limiter: StoreLimiter           (maps max_heap_mb)
  - waker: Option<Arc<CooperativeRuntimeWakeFlag>>
```

### Module cache

```text
WasmtimeModuleCache (process-wide, Send + Sync)
  - engine: wasmtime::Engine
  - compiled: DashMap<[u8; 32], Arc<component::Component>>
  - get_or_compile(bundle) -> Arc<Component>
    - if precompiled: Component::deserialize (sub-ms)
    - else: Component::new (compile, 10-100ms)
    - cache by bundle SHA-256
```

Unlike V8's worker-local `V8WorkerRuntimePool`, the wasmtime module cache
is process-wide because `wasmtime::Engine` and `Component` are `Send + Sync`.
One compilation per bundle, ever, until engine config changes.

### Fuel-based cooperative scheduling

```text
WasmtimeFuelSlot
  - store: Store<InvocationHostState>
  - instance: component::Instance
  - fuel_budget_per_tick: u64
  - wake_flag: Arc<CooperativeRuntimeWakeFlag>

poll_once():
  1. store.set_fuel(fuel_budget_per_tick)
  2. Resume or start the exported handler call
  3. If OutOfFuel trap → Runnable (requeue in FIFO)
  4. If Interrupt (epoch) → ExecutionTimeout error
  5. If async host import suspends → Parked
  6. If completed → Completed with result value
```

### Bundle format extension

```text
RuntimeBundle
  - identity: BundleIdentity
    - sha256: [u8; 32]
    - entrypoint: String
    - content_kind: BundleContentKind (JavaScript | WasmComponent)
  - content: BundleContent

BundleContent (enum)
  - JavaScript { source, source_map, code_cache }        (existing)
  - WasmComponent { bytes, precompiled, target_world }   (new)

ComponentWorld (enum)
  - NeovexFunction     neovex:host interfaces only
  - NeovexAgent        neovex:host + neovex:agent interfaces
```

`ComponentWorld` is enforced at deploy time — a tenant without agent
capabilities cannot deploy a `NeovexAgent` component.

### WIT interface definitions

The WIT interfaces are the stable contract between WASM components and the
Neovex host. They are a typed projection of the existing `HostBridge` /
`HostCallOperation` surface.

```text
package neovex:host@0.1.0

interface database
  - get, insert, patch, delete
  - query-start, query-with-index, query-filter, query-order
  - query-collect, query-first, query-paginate

interface scheduler
  - run-after, run-at, cancel

interface runtime
  - run-query, run-mutation, run-action

interface context
  - tenant-id, function-name, invocation-id, invocation-kind, identity

world neovex-function
  - import database, scheduler, runtime, context
  - export handler: func(args: string) -> result<string, string>
```

Each WIT import in the `neovex:host` package maps to a `HostCallRequest` →
`HostBridge::call()` or `HostBridge::call_async()` invocation. The adapter is
built once in the `component::Linker` at engine creation time.

### `Send + Sync` advantage

wasmtime `Store<T>` is `Send` when `T: Send`. This permits cross-thread Store
migration as a future optimization:

```text
RuntimeRoutingAffinity
  - none, tenant, function, script    (existing, both backends)
  - cross_thread                      (future, wasmtime only)
```

This plan does **not** implement cross-thread scheduling. Worker-local pools
with tenant affinity are the right default. The architecture permits
cross-thread migration because routing happens in `RuntimeWorkerRouter` before
dispatch — a future cross-thread scheduler would replace the router, not the
worker loop.

## Required Invariants

- The wasmtime backend must not break the V8 backend. Both must pass their
  respective test suites after every phase.
- `HostBridge` remains the single Rust-side host integration point. WIT
  interfaces are the WASM-side projection of the same contract.
- Bundle integrity checks (SHA-256) must apply to WASM components with the same
  rigor as JS bundles.
- Per-Store memory limits must map to `RuntimeLimits::max_heap_mb` and
  `initial_heap_mb`.
- Fuel budget and epoch deadline must map to
  `RuntimeLimits::execution_timeout`.
- Module cache must be keyed by `(bundle_sha256, engine_config_hash)` so engine
  upgrades invalidate stale compiled modules.
- The cooperative fuel path must integrate with the existing
  `CooperativeScheduler<T>` FIFO run queue and park/resume semantics.

## Promotion Criteria

Promote this plan only if all of the following are true:

1. `docs/plans/v8-locker-fork-plan.md` Phase 5 (cooperative worker loop) has
   reached `done` status.
2. The `WorkerLoopFactory` / `WorkerLoop` / `CooperativeScheduler<T>` seam is
   stable and the backend abstraction refactor does not require changes to the
   cooperative scheduler itself.
3. The product direction confirms WASM components as an intended execution
   surface (per `ARCHITECTURE.md`).

## Phase Status Ledger

| Phase | Status | Summary | Hard Dependencies | Gate Note |
|-------|--------|---------|-------------------|-----------|
| W1 | `todo` | Backend abstraction refactor | Locker fork plan Phase 5 `done` | keep `NeovexRuntime` as the generic execution facade while pushing V8- and wasmtime-owned state below `RuntimeBackend` / `CooperativeBackendDriver`; V8 path must remain green |
| W2 | `todo` | wasmtime engine, WIT definitions, and `neovex:host` linker | W1 | add `wasmtime` dependency; create `wasmtime::Engine` config; define `neovex:host` WIT package; build `component::Linker<InvocationHostState>` mapping WIT imports to `HostBridge` calls |
| W3 | `todo` | Run-to-completion wasmtime backend | W1, W2 | `WasmtimeBackendFactory`, `WasmtimeBackend`, `WasmtimeModuleCache`, Store lifecycle, `PrecompiledModuleCache` pool kind; end-to-end invocation of a `neovex-function` world component through the existing `RunToCompletionWorkerLoop` |
| W4 | `todo` | Bundle format extension | W2, W3 | `BundleContent::WasmComponent`, `ComponentWorld` enum, integrity checks on WASM components, pre-compilation pipeline, codegen integration for WASM component bundles |
| W5 | `todo` | Cooperative fuel-based scheduling | W1, W2, W3 | `WasmtimeFuelDriver`, `WasmtimeFuelSlot`, `CooperativeFuel` execution model; fuel-based yield/resume through the generic `CooperativeWorkerLoop<WasmtimeFuelDriver>`; park on async host imports, resume on I/O completion |
| W6 | `todo` | Retained Store pool | W3, W5 | `RetainedStorePool` pool kind; worker-local Store reuse with selective `InvocationHostState` reset; bounded pool with LRU eviction and retirement cap; same pool invariants as the V8 warm-pool path |
| W7 | `todo` | Observability, validation, and benchmark comparison | W3, W5 | runtime metrics for wasmtime backend (compilation time, fuel consumed, Store pool hits/misses, evictions); benchmark harness extension in `runtime_pool_modes.rs`; comparison report against V8 path for representative workloads |

## Phase Order and Dependencies

```text
Locker fork plan Phase 5 (done)
  └── W1 backend abstraction refactor
        ├── W2 wasmtime engine + WIT + linker
        │     ├── W3 run-to-completion backend
        │     │     ├── W4 bundle format extension
        │     │     ├── W5 cooperative fuel scheduling
        │     │     │     └── W6 retained Store pool
        │     │     └── W7 observability + benchmarks
        │     └── (W3, W5 both need W2)
        └── (W3, W5 both need W1)
```

Recommended delivery order: W1 → W2 → W3 → W4 → W5 → W6 → W7

W4 (bundle format) can run in parallel with W5 (cooperative scheduling) after
W3 lands.

## Implementation Checkpoints

| Phase | Checkpoint | Next Step |
|-------|------------|-----------|
| W1 | none yet | promote after Locker fork plan Phase 5 completes |
| W2 | none yet | |
| W3 | none yet | |
| W4 | none yet | |
| W5 | none yet | |
| W6 | none yet | |
| W7 | none yet | |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-04-05 | meta | documented | Initial plan authored. Covers backend abstraction refactor, wasmtime engine/WIT/linker, run-to-completion and cooperative-fuel backends, bundle format, retained Store pool, and observability. References existing `WorkerLoopFactory` / `CooperativeScheduler<T>` seams and `ARCHITECTURE.md` WASM direction. | document review against `ARCHITECTURE.md`, `v8-locker-fork-plan.md`, `docs/plans/archive/raw-v8-warm-backend-plan.md`, and current `neovex-runtime` source | keep deferred until Locker fork plan Phase 5 reaches `done` |

## Verification Expectations

When promoted, the wasmtime backend should not be considered viable without:

- focused WASM component invocation tests (neovex-function world)
- WIT import → HostBridge round-trip tests (database, scheduler, runtime)
- fuel exhaustion yield/resume correctness tests
- epoch-based timeout tests
- module cache correctness tests (hit, miss, invalidation)
- Store pool lifecycle tests (create, reuse, evict, retire)
- memory limit enforcement tests (ResourceLimiter)
- multi-tenant fairness tests under mixed V8 + WASM workloads
- benchmark comparisons against V8 path in `runtime_pool_modes.rs`
- V8 backend regression suite green after every phase

## Relationship To Other Plans

- **`v8-locker-fork-plan.md`**: hard prerequisite. This plan activates after
  Phase 5 completes. The backend abstraction refactor (W1) must not break the
  landed cooperative Locker scheduling.
- **`docs/plans/archive/raw-v8-warm-backend-plan.md`**: parallel deferred plan. Both this plan and
  the raw-V8 plan slot new backends beneath the same `WorkerLoopFactory` seam.
  The backend abstraction refactor (W1) should benefit both.
- **`wasi-agent-capabilities-plan.md`**: downstream consumer. The WASI agent
  capabilities plan builds on top of this plan's WIT interface surface and
  wasmtime linker to add agent OS primitives.
- **`ARCHITECTURE.md`**: this plan implements the WASM direction already named
  there. Update `ARCHITECTURE.md` when each phase lands.
