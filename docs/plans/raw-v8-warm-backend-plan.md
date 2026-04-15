# Plan: Raw V8 Warm Backend

> **Superseded and closed.** The warm module pool plan
> (`docs/plans/warm-module-pool-plan.md`) achieved 50x warm-hit speedup (22µs vs
> 1.1ms) through surgical `deno_core` fork changes without building a separate
> raw-V8 engine. The activation gate for this plan — "fork approach proves
> infeasible" — was never met. The fork approach succeeded.
>
> This plan is preserved as research context only. Do not promote it.

Deferred design plan for a future raw-V8 warm-execution backend in
`neovex-runtime`.

This document preserves the reference implementation analysis, architecture
boundary research, and proposed configuration surface from the original raw-V8
investigation. The reference inventory and strengths matrix remain valuable
context for any future warm-execution work.

Historical sections below retain some pre-rename vocabulary from when this plan
was authored, especially `deno_core` and `retained_jsruntime_pool`. Read those
as historical equivalents of today's V8 backend and older warm-pool naming, not
as the current public runtime surface.

---

## Status

- **Status:** `closed` (superseded by `warm-module-pool-plan.md`, which completed
  all 6 phases and validated 50x warm-hit speedup on 2026-04-08)
- **Primary owner:** `docs/plans/warm-module-pool-plan.md` (done)
- **Activation gate:** never met — the fork approach succeeded

## How To Use This Plan

- Start with `docs/plans/warm-module-pool-plan.md` for warm execution work.
- Use this plan's reference inventory and strengths matrix as research context.
- Do not promote this plan unless the fork approach is proven infeasible.
- Treat local reference implementations, upstream/runtime constraints, and
  measured Neovex behavior as equal inputs.
- If the activation gate is met, promote exactly one implementation slice at a
  time and keep this plan as the canonical control plane for that workstream.

## Current Architecture Boundary

- `ARCHITECTURE.md` is the source of truth for the landed `WorkerLoopFactory` /
  `WorkerLoop` seam from EO5.
- `docs/plans/v8-locker-fork-plan.md` is the source of truth for the current
  `deno_core` + Locker backend.
- This plan does **not** reopen the decision to keep scheduling above the
  runtime. That seam is already the right one.

## Why This Must Be A Separate Backend

The current `deno_core` backend and the future raw-V8 warm backend serve
different execution contracts:

| Backend | Contract | Warm unit | Reset boundary |
|---------|----------|-----------|----------------|
| `deno_core` (current) | `fresh_per_invocation` | `JsRuntime` / main realm reuse | `reset_main_realm()` + bootstrap replay + bundle reload |
| raw V8 warm backend (future) | warm loaded code | persistent execution context on a pooled isolate | selective request-local reset only |

The key rule is:

- **Do not change `retained_jsruntime_pool` semantics in place.**
- Keep the current `deno_core` path honest as fresh-realm reuse.
- Put true warm execution behind a new backend kind and explicit settings.

## Reference Implementations

## Review Inventory

This section is the canonical inventory of the implementations already reviewed
for this plan. When adding or revisiting references, extend this table instead
of leaving the evidence only in chat.

| System | Local review paths | GitHub / public review URLs | What it contributes |
|--------|--------------------|-----------------------------|---------------------|
| OpenWorkers runtime | `/Users/jack/src/github.com/openworkers/openworkers-runtime-v8/README.md`, `/Users/jack/src/github.com/openworkers/openworkers-runtime-v8/docs/execution_modes.md`, `/Users/jack/src/github.com/openworkers/openworkers-runtime-v8/src/pool.rs`, `/Users/jack/src/github.com/openworkers/openworkers-runtime-v8/src/execution_context.rs`, `/Users/jack/src/github.com/openworkers/openworkers-runtime-v8/src/async_waiter.rs` | `https://github.com/openworkers/openworkers-runtime-v8`, `https://github.com/openworkers/openworkers-runtime-v8/blob/main/docs/execution_modes.md` | strongest Rust reference for warm execution contexts, worker-local pools, FIFO waiter fairness, warm-hit reset, and code-cache-first user-code startup |
| OpenWorkers runner | `/Users/jack/src/github.com/openworkers/openworkers-runner/src/task_executor.rs`, `/Users/jack/src/github.com/openworkers/openworkers-runner/src/worker_pool.rs` | `https://github.com/openworkers/openworkers-runner` | concrete dispatch and warm-hit host-state refresh pattern; useful as a reference for request-local ops patching but not a direct fit for Neovex routing |
| OpenWorkers core / V8 forks | `/Users/jack/src/github.com/openworkers/openworkers-core/Cargo.toml`, `/Users/jack/src/github.com/openworkers/rusty-v8/Cargo.toml`, `/Users/jack/src/github.com/openworkers/serde-v8`, `/Users/jack/src/github.com/openworkers/glue-v8` | `https://github.com/openworkers/openworkers-core`, `https://github.com/openworkers/rusty-v8`, `https://github.com/openworkers/serde-v8`, `https://github.com/openworkers/glue-v8` | shows the current stack split: raw-V8 runtime path, helper crates around V8 interop, and the fact that their main runtime no longer centers on `deno_core` |
| Cloudflare workerd | `/Users/jack/src/github.com/cloudflare/workerd/src/workerd/io/worker.c++` | `https://github.com/cloudflare/workerd`, `https://github.com/cloudflare/workerd/blob/main/src/workerd/io/worker.c%2B%2B` | canonical large-scale reference for RAII async lock handoff, waiter fairness, deferred cleanup, and long-lived loaded workers |
| Upstream `deno_core` | `/Users/jack/src/github.com/denoland/deno_core/ARCHITECTURE.md`, `/Users/jack/src/github.com/denoland/deno_core/core/runtime/jsruntime.rs`, `/Users/jack/src/github.com/denoland/deno_core/core/runtime/jsrealm.rs` | `https://github.com/denoland/deno_core`, `https://github.com/denoland/deno_core/blob/main/ARCHITECTURE.md` | keeps the current backend honest: great embedder runtime engine, snapshots, module-loader code cache, and extension model, but not the canonical place to force warm loaded-code semantics |
| Agentstation `deno_core` fork | local cargo checkout under `/Users/jack/.cargo/git/checkouts/deno_core-*/` and the repaired tag consumed by Neovex | `https://github.com/agentstation/deno_core/tree/locker-v0.395`, `https://github.com/agentstation/deno_core/releases/tag/0.395.0-locker.1` | proves the current locker-enabled fresh-realm reuse surface we should preserve while building any future raw backend separately |
| Agentstation `rusty_v8` fork | local cargo checkout under `/Users/jack/.cargo/git/checkouts/rusty_v8-*/` and the repaired tag consumed by Neovex | `https://github.com/agentstation/rusty_v8/tree/locker-v147`, `https://github.com/agentstation/rusty_v8/releases/tag/v147.0.0-locker.2` | current V8 substrate for Neovex; useful both for the current `deno_core` path and any future raw-V8 backend |
| Official V8 guidance | not stored locally in this repo; review externally when tuning startup paths | `https://v8.dev/`, `https://v8.dev/blog/custom-startup-snapshots`, `https://v8.dev/blog/code-caching-for-devs` | official reference for code cache vs startup snapshots and where each is appropriate |

## Strengths And Fit Matrix

| System | Best at | Weak fit for | Net lesson for Neovex |
|--------|---------|--------------|-----------------------|
| `deno_core` | embedder ergonomics, extensions/ops, startup snapshots, module-loader hooks, stable fresh-realm lifecycle | true warm loaded-code execution | keep as the default safe backend |
| OpenWorkers | warm context reuse, warm-hit request-state refresh, worker-local pools, code-cache-first user code, bounded warm reuse | direct drop-in dependency for Neovex host/runtime contract | copy patterns, not crate boundaries |
| workerd | battle-tested fairness, long-lived loaded workers, RAII lock handoff, deferred destruction discipline | direct implementation template in Rust | copy scheduling and lifecycle ideas |
| current Neovex | worker-loop seam, admission controls, affinity routing, metrics/diagnostics, dual backend vocabulary | low-level raw-V8 warm runtime details | keep this as the top-level control plane and slot a raw backend underneath it |

### What each implementation is especially good for

- **`deno_core`** is best when the product wants the cleanest embedder story,
  fresh invocation semantics, minimal fork surface, extension/bootstrap
  ergonomics, and upstream swapability.
- **OpenWorkers** is best as the concrete Rust reference for what a warm-hit
  backend should look like when loaded user code is allowed to persist on
  purpose.
- **workerd** is best as the long-term concurrency and lifecycle north star:
  fairness, waiter discipline, deferred cleanup, and clear separation between
  scheduling and request-local execution state.
- **Neovex itself** already has the right outer architecture: worker-loop seam,
  admission/metrics policy, and runtime-agnostic settings should remain the
  top-level contract.

### OpenWorkers

Local references:

- `/Users/jack/src/github.com/openworkers/openworkers-runtime-v8/README.md`
- `/Users/jack/src/github.com/openworkers/openworkers-runtime-v8/docs/execution_modes.md`
- `/Users/jack/src/github.com/openworkers/openworkers-runtime-v8/src/pool.rs`
- `/Users/jack/src/github.com/openworkers/openworkers-runtime-v8/src/execution_context.rs`
- `/Users/jack/src/github.com/openworkers/openworkers-runtime-v8/src/async_waiter.rs`
- `/Users/jack/src/github.com/openworkers/openworkers-runner/src/task_executor.rs`

What to copy:

- worker-local pools only
- RAII Locker scopes
- fair FIFO waiter queue for multiplexing
- selective request-local reset
- warm-hit callback for request-local host state refresh
- bounded warm reuse with retirement
- idle-only LRU eviction

What **not** to copy blindly:

- their host API surface
- their exact runner/thread-dispatch layer
- their crate boundaries as direct Neovex dependencies

### Cloudflare workerd

Local references:

- `/Users/jack/src/github.com/cloudflare/workerd/src/workerd/io/worker.c++`

What to copy:

- long-lived worker/loaded-code model
- RAII async lock handoff
- waiter-queue fairness
- explicit separation between scheduling and request-local execution state

What **not** to copy literally:

- C++ object model
- KJ event-loop ownership patterns
- Cloudflare-specific product/runtime concepts that do not map to Neovex host
  semantics

### Upstream `deno_core`

Local reference:

- `/Users/jack/src/github.com/denoland/deno_core/ARCHITECTURE.md`

Why it still matters:

- It remains the right engine substrate for the current fresh-realm backend.
- It is a constraint reference for what the raw-V8 backend should **not** force
  onto the `deno_core` path.

## Recommendation: Build, Do Not Adopt

### Do not adopt OpenWorkers crates as the primary backend dependency

Reasoning:

- Neovex has a different host bridge, policy surface, bundle lifecycle, and
  diagnostics contract.
- Depending directly on OpenWorkers would import another project's runtime
  lifecycle, request semantics, and compatibility constraints into Neovex.
- The safer path is to copy proven ideas and keep the Neovex worker-loop seam
  as the top-level contract.

### Do not push warm-loaded-context semantics into the `deno_core` fork

Reasoning:

- That would blur two different execution contracts into one backend.
- It would make `retained_jsruntime_pool` semantically ambiguous.
- It would increase fork divergence in the path we currently want to keep
  upstream-swappable.

### Do build a new raw-V8 backend under the existing seam

Reasoning:

- EO5 already gave Neovex the right seam for this: `WorkerLoopFactory`,
  `WorkerLoop`, and explicit runtime settings.
- A raw backend can adopt warm execution on purpose without weakening the
  current `deno_core` guarantees.

## Recommended Hybrid For Neovex

The best combination for Neovex is not “pick one reference and copy it
verbatim.” It is a hybrid:

### Keep from current Neovex

- `WorkerLoopFactory` / `WorkerLoop` as the primary runtime seam
- existing admission controls and tenant-aware routing policy
- runtime diagnostics and backend/pool settings
- explicit policy ownership above the runtime engine

### Keep from the current `deno_core` backend

- `startup_snapshot_cache` as the default low-latency mode
- `retained_jsruntime_pool` as fresh-realm reuse only
- module-loader code cache for user modules
- extension/bootstrap and host-bridge ergonomics

### Adopt from OpenWorkers for the future raw backend

- persistent warm execution context as the warm unit
- selective request-local reset, not full realm rebuild
- warm-hit callback to patch host/request-local state
- bounded worker-local warm pool with retirement
- code cache for user code rather than relying on heap snapshots

### Adopt from workerd for the future raw backend

- waiter-queue fairness as the canonical lock handoff discipline
- explicit deferred destruction / cleanup queue
- strong separation between long-lived loaded worker state and per-request
  execution state

### Resulting recommendation

For Neovex’s use case, the best combined architecture is:

1. **`deno_core` backend remains the default** for the current product
   contract.
2. **Raw-V8 warm backend becomes an opt-in second backend** for workloads that
   explicitly want warm loaded-code semantics.
3. **Both backends share the same outer scheduler/admission/metrics seam.**
4. **No cross-thread shared isolate pool** in either backend.

This gives Neovex the strengths of all three systems without inheriting their
mismatched assumptions wholesale.

## Hard Technical Risks To Solve Explicitly

The raw backend is only worth promoting if these risks are handled in the
design itself rather than discovered late in implementation:

### HostBridge adaptation without `deno_core` ops

The future raw backend must preserve the current `HostBridge` contract from
`crates/neovex-runtime/src/host.rs` instead of inventing a second host API.
That means:

- raw V8 bindings should still terminate in the same `HostBridge::call`,
  `HostBridge::call_cancellable`, and `HostBridge::call_async` surface
- the backend should provide a thin raw-V8 binding layer that converts
  JavaScript host calls into `HostCallRequest` values and converts the returned
  `serde_json::Value` back into V8 values
- synchronous host calls should use direct native callback bindings
- asynchronous host calls should create a request-local promise capability,
  start `HostBridge::call_async`, park the warm context, and resolve/reject the
  promise on the next scheduler tick after wakeup
- request-local host state must not live in the warm context itself; it should
  be reapplied on every warm hit from a request-local adapter object

The recommended implementation direction is:

1. keep `HostBridge` and `HostCallRequest` as the stable Neovex contract
2. build a small raw-V8 callback layer around `v8::FunctionCallbackInfo`
3. store request-local bridge state beside the active invocation, not in the
   persistent warm context
4. treat "warm-hit callback" as the mechanism that reattaches request-local
   bridge state before execution resumes

### Module and bundle loading without `deno_core`'s loader

The raw backend should continue consuming the same `RuntimeBundle` contract from
`crates/neovex-runtime/src/runtime/bundle.rs`:

- same entrypoint identity
- same SHA-256 integrity checks
- same bundle-local code-cache ownership

Do **not** invent a second bundle artifact format for the first raw backend
slice. Start with the same `RuntimeBundle` shape and load it through raw V8
module APIs.

The recommended loading model is:

1. use a bootstrap-only startup snapshot for runtime API shims and stable host
   bindings
2. load user code from the existing `RuntimeBundle` entrypoint as ESM source
   text modules
3. keep user-code code cache bundle-scoped and keyed by bundle identity plus
   source hash
4. keep module resolution local to the bundle root and explicit in the backend,
   rather than creating a second general-purpose loader contract

This keeps the raw backend comparable with the current `deno_core` path and
avoids a second user-visible packaging format.

### Locker lifetime and parked warm contexts

The raw backend should follow the same broad rule as the current cooperative
Locker path and the reference runtimes:

- do **not** let an idle or parked warm context continue owning the V8 lock
- reacquire the Locker for each scheduler-driven execution tick
- release the Locker whenever the context transitions to parked or idle
- run deferred destruction/cleanup only at well-defined safe points

This keeps scheduler ownership explicit and avoids hidden lock retention across
park/resume cycles.

## Proposed Public Shape

These names are recommendations, not yet implemented API:

```text
RuntimeBackendKind
  - deno_core
  - raw_v8

RuntimeExecutionModel
  - run_to_completion
  - cooperative_locker

RuntimePoolKind
  - startup_snapshot_cache
  - retained_jsruntime_pool
  - warm_execution_context_pool   (raw_v8 backend only)
```

Rules:

- `raw_v8` must never silently reuse the `deno_core` retained-pool
  semantics.
- `warm_execution_context_pool` must be rejected on `deno_core`.
- `startup_snapshot_cache` remains the default until the raw backend proves a
  better default by measurement.

## Proposed Internal Shape

```text
WorkerLoopFactory
  -> CooperativeWorkerLoopFactory
    -> CooperativeWorkerLoop
      -> RuntimeBackendFactory
        -> RawV8RuntimeBackend
          -> WorkerLocalIsolatePool
            -> WarmExecutionContext
            -> RequestContext / per-request host state
```

Key boundaries:

- scheduler remains above runtime
- backend swap happens beneath the same `CooperativeWorkerLoop`, not via a
  second worker-loop abstraction
- pools remain worker-local
- no cross-thread shared isolate pool
- request-local state reset is explicit and observable
- loaded code stays warm by contract in this backend only
- Locker acquisition remains scheduler-scoped; warm contexts do not retain the
  lock while parked or idle

## Recommended Starting Defaults

These are starting points for experiments, not committed product defaults:

- `runtime_backend = raw_v8` only when explicitly requested
- `execution_model = cooperative_locker`
- `routing_affinity = tenant`
- `max_retained_isolates_per_worker = 4`
- `max_warm_contexts_per_affinity_key_per_worker = 1`
- idle-only LRU eviction
- `max_context_reuses = 1000` before forced retirement
- no cross-thread pool sharing
- fail-fast on pool saturation; rely on existing Neovex admission controls and
  queueing above the backend

Notes:

- `max_warm_contexts_per_affinity_key_per_worker = 1` is a conservative spike
  default, not a claimed production optimum. It intentionally exposes waiter
  fairness and reset correctness first.
- If benchmarks show hot same-tenant concurrency serializing too aggressively,
  the first production tuning pass should evaluate `2-4` warm contexts per
  affinity key before considering more complex scheduler changes.
- bootstrap/runtime API setup should prefer a startup snapshot even on the raw
  backend; only user-code warmth should move to the warm-context pool.

## Candidate Modern Patterns And Upgrades

These are grouped by confidence level so the plan stays grounded.

### Proven patterns to adopt early

| Pattern | Source | Why it fits Neovex |
|---------|--------|--------------------|
| Worker-local pools only | OpenWorkers, workerd, current Neovex seam | avoids cross-thread contention and keeps ownership simple |
| FIFO waiter fairness | OpenWorkers `AsyncWaiter`, workerd `AsyncWaiter` | battle-tested way to serialize V8 lock ownership without starvation |
| Idle-only LRU eviction | OpenWorkers, current Neovex retained pool | simple, observable, and good enough at low pool sizes |
| Reuse retirement cap | OpenWorkers | limits fragmentation and long-tail retained-state risk |
| Warm-hit callback for request-local host state | OpenWorkers | exactly the mechanism we will need to refresh per-request host bindings cleanly |
| Code cache for user code | OpenWorkers, official V8 guidance, current Neovex `deno_core` path | best near-term compile-cost optimization without changing semantics |
| Bootstrap snapshot for runtime API boot only | current Neovex, `deno_core`, V8 guidance | good cold-start optimization without coupling user code into snapshot format |
| Deferred destruction queue | workerd, OpenWorkers | protects cleanup ordering around V8 handle destruction |

### Strong candidates after the first raw backend slice

| Pattern | Why it may be worth adopting | Caution |
|---------|------------------------------|---------|
| Memory-pressure-aware eviction | can discard warm contexts before OOM pressure becomes pathological | needs trustworthy metrics first |
| Affinity-aware warm-context matching beyond tenant | may improve locality for hot functions/scripts | can overfit and fragment the pool if added too early |
| Reuse retirement by both count and age | reduces risk from very long-lived contexts | keep simple at first; count-only is enough to start |
| Separate cold-start compile queue | may smooth bursty compile spikes | only if compile storms show up in benchmarks |

### Experimental patterns worth evaluating only if metrics justify them

| Pattern | Potential upside | Why not default |
|---------|------------------|-----------------|
| W-TinyLFU / SLRU-style admission for warm contexts | better cache admission under high churn and many one-hit scripts | too complex for the initial bounded pool; LRU is easier to reason about |
| Weighted fair queueing / deficit round robin across runnable warm contexts | more nuanced fairness than FIFO under mixed tenants or long-running contexts | Neovex already has admission controls; do not stack complexity before proving FIFO is insufficient |
| Speculative prewarming by tenant/function popularity | lower tail latency for hot tenants | can waste memory and hide bugs in reset semantics |
| Cross-worker compile artifact sharing | could reduce duplicate compile work across threads | easier to get wrong than bundle-local cache; start worker-local first |

### What is innovative for Neovex specifically

The potentially innovative part is not a novel scheduler algorithm by itself.
It is the combination:

- Neovex’s runtime-agnostic worker-loop seam
- a safe `deno_core` backend for fresh semantics
- a separate raw-V8 warm backend for hot loaded-code workloads
- shared admission/routing/metrics above both

That combination keeps enterprise trust and runtime flexibility while still
allowing the hot path to evolve toward workerd/OpenWorkers-style warm
execution.

## Required Invariants

The raw backend is only worth promoting if it can make the warm contract
explicit and trustworthy:

- per-request host state must be fully refreshed on every warm hit
- timer, stream, callback, and cancellation state must not leak across
  requests
- user code loaded into a warm context must be clearly documented as persistent
- warm-hit behavior must be observable in runtime metrics
- discarded contexts must be removed on any execution/reset/cleanup error
- idle eviction must never touch active or parked contexts

## Promotion Criteria

Promote this plan only if all of the following are true:

1. `docs/plans/v8-locker-fork-plan.md` has completed the current `deno_core`
   backend workstream.
2. Benchmarks show the `deno_core` backend, even with code cache and current
   tuning, is still materially behind target warm-hit latency or throughput for
   the intended workload.
3. The product/runtime contract wants true warm loaded-code semantics rather
   than fresh-per-invocation semantics.
4. The team is willing to accept a second runtime backend with its own
   maintenance surface.

## Activation Report Requirements

`RV1` is not complete until it names the concrete workload, the target, the
current measured gap, and the reason the current `deno_core` path cannot close
that gap without changing semantics.

Minimum report contents:

- benchmark inputs, including the existing
  `crates/neovex-runtime/benches/runtime_pool_modes.rs` harness plus any added
  warm-hit workload benchmark
- the exact Neovex configuration under test
- absolute target metrics for the intended workload
- measured `deno_core` results after Phase 5 reliability/code-cache work
- the remaining relative gap that justifies a raw backend

The activation bar should be concrete:

- either the current `deno_core` backend is still at least `2x` slower than the
  required warm-hit latency target on a representative workload
- or it delivers less than `60%` of the required steady-state throughput after
  code cache and current tuning are applied

The report should also say whether the bottleneck is compile cost, warm-hit
reset cost, host-bridge overhead, scheduler overhead, or memory pressure.

## Roadmap

| Item | Status | Summary | Hard Dependencies | Gate Note |
|------|--------|---------|-------------------|-----------|
| RV1 | `todo` | Activation report and benchmark gate | Locker plan complete enough to compare against | use `runtime_pool_modes.rs` plus an explicit warm-hit workload and record the remaining gap in absolute and relative terms |
| RV2 | `todo` | Raw V8 runtime core, module loading, and host bridge adapter | RV1 | worker-local only; no cross-thread pool; preserve `HostBridge` and `RuntimeBundle` contracts |
| RV3 | `todo` | Warm execution context lifecycle and selective reset contract | RV2 | define explicit persistent-vs-reset state surface, warm-hit callback semantics, and per-tick lock ownership before adding reuse |
| RV4 | `todo` | Cooperative scheduler integration and waiter fairness | RV2, RV3 | reuse the existing worker-loop seam; do not bypass it |
| RV5 | `todo` | Observability, retirement, eviction, and failure discard rules | RV3, RV4 | metrics and diagnostics must land before default discussions |
| RV6 | `todo` | Benchmark/soak validation and default-position decision | RV4, RV5 | default remains `deno_core` unless this phase proves otherwise |

## Recommended Delivery Order

1. `RV1` activation report
2. `RV2` raw runtime core
3. `RV3` warm-context reset contract
4. `RV4` scheduler integration
5. `RV5` reliability/observability guardrails
6. `RV6` benchmark and default decision

## Verification Expectations

When promoted, the raw backend should not be considered viable without:

- focused warm-hit correctness tests
- failure-discard tests
- multi-tenant fairness tests
- async host-I/O interleaving tests
- benchmark comparisons against the completed `deno_core` path
- explicit diagnostics proving warm hits, evictions, retirements, and discard
  reasons

## Current Decision (Updated 2026-04-08)

This plan is **closed**. The warm module pool plan achieved warm execution
semantics through `deno_core` fork changes, proving the raw-V8 backend
unnecessary:

- **`WarmPool`** delivers 22µs warm-hit latency (50x over snapshot cache)
  without a second backend
- **`reset_request_state()`** in the fork provides surgical per-request cleanup
  while preserving evaluated modules — the exact contract this plan proposed
  building from scratch on raw V8
- **`RetainedJsRuntimePool`** is a candidate for deprecation — the warm pool is
  strictly better (faster, no memory leak from `destroy_for_reset`)
- The raw-V8 backend's ~3-6 month estimated build cost was avoided entirely

### What this plan got right

The research and reference inventory remain valuable:
- The workerd/OpenWorkers patterns (FIFO fairness, warm-hit callbacks, bounded
  retirement, selective reset) were implemented in the fork approach
- The separation between scheduling and request-local state was preserved
- The "build, do not adopt" recommendation held — Neovex copied patterns, not
  crate dependencies

### What this plan got wrong

The core assumption — "do not push warm-loaded-context semantics into the
`deno_core` fork" — was wrong. The fork changes were surgical (206 production
lines) and the existing `deno_core` event loop, op system, and module loader
all worked correctly for warm reuse. The plan overestimated the fork divergence
risk and underestimated how much of the warm contract could be built additively
on top of existing `deno_core` surfaces.
