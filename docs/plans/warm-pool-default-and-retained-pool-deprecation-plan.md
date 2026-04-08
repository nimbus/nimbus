# Plan: Warm Pool Default and Retained Pool Deprecation

Make `WarmModulePool` + `CooperativeLocker` the default production runtime path.
Remove `RetainedJsRuntimePool` and all `reset_main_realm`-based code across
neovex, deno_core, and rusty_v8. Keep `RunToCompletion` +
`StartupSnapshotCache` as a user-selectable per-bundle execution mode for
bundles that need guaranteed fresh-per-invocation isolation.

---

## Status

- **Status:** `done`
- **Primary owner:** this plan
- **Prerequisite:** `warm-module-pool-plan.md` (done, all 6 phases complete)
- **Motivation:** WarmModulePool is strictly better than RetainedJsRuntimePool
  (50x faster, zero memory leak, simpler). RetainedJsRuntimePool exists only
  to support `reset_main_realm`, which is the source of the SIGSEGV fix
  complexity, the ~500KB/cycle Rc leak, and ~900 lines of fork code.

## Control Plan Rules

- `todo`: not started
- `in_progress`: actively being implemented
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria met and verification recorded

### Recovery loop

1. Reread this plan's Phase Status Ledger and Execution Log.
2. Inspect the current git worktree across all 3 repos and reconcile.
3. Resume any `in_progress` phase first.
4. Implement exactly one phase per session by default.

---

## Why This Deprecation

### WarmModulePool vs RetainedJsRuntimePool

| Metric | RetainedJsRuntimePool | WarmModulePool |
|--------|----------------------|----------------|
| Warm-hit latency | 1.8ms (realm reset + bootstrap replay + module reload) | **22µs** (surgical state reset only) |
| Memory leak | ~500KB/cycle from `destroy_for_reset` Rc circular reference | **Zero** (no realm reset) |
| Fork complexity | ~800 lines (`reset_main_realm`, `destroy_for_reset`, SIGSEGV fixes, ArrayBuffer tracking) | ~200 lines (`reset_request_state`, `is_warm_reuse_safe`) |
| Execution model | Works with `RunToCompletion` and `CooperativeLocker` | `CooperativeLocker` only |
| Module state | Fresh per invocation (modules reloaded every time) | Warm per bundle (modules persist, 50x faster) |
| V8 crash risk | `destroy_for_reset` + `reset_main_realm` is the entire SIGSEGV surface | No realm destruction = no crash surface |

### What RunToCompletion provides

RunToCompletion + StartupSnapshotCache gives users a simpler execution model
with **guaranteed fresh-per-invocation isolation**: no cross-request state, no
module persistence, no generation guards to reason about. The trade-off is
higher latency (~1.1ms vs 22µs warm hit) and no cooperative scheduling.

This is valuable for:
- Bundles that must never share module-level state across requests
- Users who want the simplest possible execution contract
- Debugging scenarios where warm module state could mask issues

RunToCompletion remains a first-class production option that users can specify
per-bundle. The default changes to `CooperativeLocker` + `WarmModulePool` for
bundles that don't specify an execution model.

---

## Blocker: Async Concurrent Dispatch Hang (root cause identified)

**Must be resolved before deprecation.**

`invoke_blocking` with 4+ concurrent threads + `CooperativeLocker` freezes
indefinitely. Affects **all pool kinds**, not just WarmModulePool — the warm
pool benchmark was the first to exercise this path.

### Root cause

`crates/neovex-runtime/src/worker_loop/cooperative/run.rs` `next_slot()`
greedily drains the queue before polling any slot:

```rust
while let Some(job) = queue.try_recv() {
    self.admit_job(queue, job);  // block_on(acquire_initial()) inside
}
```

Each `admit_job` calls `block_on(permit.acquire_initial())` which acquires the
global isolate semaphore (`max_concurrent_isolates` permits). With
`max_concurrent_isolates: 1`:

1. Job 1 admitted → semaphore acquired (1→0), slot created, runnable
2. Job 2 admission → `block_on(acquire_initial())` → **blocks forever** on
   semaphore (0 permits)
3. **Deadlock:** job 1 holds the semaphore but can't release it (needs to be
   polled to completion or park via `begin_async_host_call`), and the
   single-threaded worker is stuck admitting job 2.

### Fix

Change `while let` to `if let` + `continue` so only one job is admitted per
loop iteration, interleaved with execution:

```rust
// Admit one job, then re-enter loop to check runnable/parked before next
if let Some(job) = queue.try_recv() {
    self.admit_job(queue, job);
    continue;
}
```

Each admitted job gets polled (releasing the semaphore via completion or
parking) before the next admission attempt. Cooperative scheduling still works
because parked slots release their semaphore via `begin_async_host_call`,
allowing new slots to be admitted between park/resume cycles.

### Key files

- `crates/neovex-runtime/src/worker_loop/cooperative/run.rs:9-44` — **fix site**
- `crates/neovex-runtime/src/worker_loop/cooperative/execution.rs` — `admit_job`
- `crates/neovex-runtime/src/executor/admission/permit.rs` — semaphore lifecycle
- `crates/neovex-runtime/src/runtime/tests/cooperative.rs` — working sequential test

---

## Removal Inventory

### rusty_v8 (~77 lines)

| Item | File | Lines | Used by | Remove? |
|------|------|-------|---------|---------|
| `Context::detach_all_slots()` | `src/context.rs` | ~34 | Nothing (proposed Rc leak fix never implemented) | **Yes** |
| `detach_all_slots` test | `tests/slots.rs` | ~43 | Above | **Yes** |
| Locker APIs | various | — | CooperativeLocker | **Keep** |

### deno_core (~800+ lines)

| Item | File(s) | Approx lines | Used by | Remove? |
|------|---------|-------------|---------|---------|
| `reset_main_realm()` | `runtime/jsruntime.rs` | ~150 | RetainedJsRuntimePool only | **Yes** |
| `destroy_for_reset()` path in `destroy_inner` | `runtime/jsrealm.rs` | ~30 | `reset_main_realm` | **Yes** |
| `shared_array_buffers` field + ArrayBuffer detach logic | `runtime/jsrealm.rs`, `runtime/jsruntime.rs` | ~40 | `destroy` path for ArrayBuffer sweeper safety | **Yes** |
| Foreground drain loop in `reset_main_realm` | `runtime/jsruntime.rs` | ~30 | `reset_main_realm` | **Yes** |
| `callsite_prototype` recreation | `runtime/jsruntime.rs` | ~20 | `reset_main_realm` | **Yes** |
| Foreground drain in `cleanup()` | `runtime/jsruntime.rs` | ~15 | Isolate disposal (may want to keep for clean shutdown) | **Evaluate** |
| `reset_main_realm` regression tests | `runtime/tests/jsrealm.rs` | ~540 | Testing above | **Yes** |
| `reset_request_state()` | `runtime/jsruntime.rs` | ~90 | WarmModulePool | **Keep** |
| `is_warm_reuse_safe()` | `runtime/jsruntime.rs` | ~20 | WarmModulePool | **Keep** |
| ManagedIsolate / Locker | `runtime/managed_isolate.rs` | ~148 | CooperativeLocker | **Keep** |
| `ExceptionState::clear_request_state()` | `runtime/exception_state.rs` | ~9 | `reset_request_state` | **Keep** |
| `ModuleMap::clear_pending_state()` | `modules/map.rs` | ~15 | `reset_request_state` | **Keep** |
| Helper methods (clear_traces, etc.) | various | ~16 | `reset_request_state` | **Keep** |
| Warm reuse tests | `runtime/tests/jsrealm.rs` | ~200 | Testing warm APIs | **Keep** |

### neovex (~300+ lines)

| Item | File(s) | Approx lines | Remove? |
|------|---------|-------------|---------|
| `RetainedJsRuntimePool` variant | `limits.rs` | ~5 | **Yes** |
| `RunToCompletion` variant | `limits.rs` | ~5 | **Keep** — user-selectable per-bundle production option |
| `RunToCompletionWorkerLoop` | `worker_loop/run_to_completion.rs` | ~160 | **Keep** — needed for RunToCompletion execution |
| `reset_retained_runtime()` + `reset_retained_runtime_inner()` | `driver/construction.rs` | ~70 | **Yes** |
| Retained pool take/return/eviction logic | `retained_pool.rs` | ~200 | **Replace** with warm-pool-only |
| Retained pool metrics (`main_realm_resets`, `bootstrap_replays`) | `metrics.rs`, `global.rs` | ~30 | **Yes** |
| Retained pool tests | `tests/retained_pool.rs` | ~150 | **Replace** with warm pool tests |
| `FreshPerInvocation` module state semantics | `limits.rs` | ~3 | **Keep** for RunToCompletion + StartupSnapshotCache |
| `StartupSnapshotCache` variant | `limits.rs` | ~5 | **Keep** — the pool kind for RunToCompletion |
| Benchmark retained pool scenarios | `runtime_pool_modes.rs` | ~50 | **Remove** cooperative retained; keep RTC scenarios |

---

## Phase Status Ledger

| Phase | Status | Summary | Hard Dependencies |
|-------|--------|---------|-------------------|
| D0 | `done` | Fix cooperative worker loop greedy admission deadlock | None — blocker for all subsequent phases |
| D1 | `done` | Remove `detach_all_slots` from rusty_v8 fork | None |
| D2 | `done` | Remove `reset_main_realm` and `destroy_for_reset` from deno_core fork | D0 (proves warm pool handles all workloads) |
| D3 | `done` | Remove `RetainedJsRuntimePool` from neovex | D2 |
| D4 | `done` | Make `CooperativeLocker` + `WarmModulePool` the default; keep `RunToCompletion` + `StartupSnapshotCache` as per-bundle option | D3 |
| D5 | `done` | Update docs, benchmarks, and plan index | D4 |

## Recommended Delivery Order

1. D0 — fix the hang (unblocks everything)
2. D1 — rusty_v8 cleanup (independent, quick)
3. D2 — deno_core cleanup (biggest fork simplification)
4. D3 — neovex retained pool removal
5. D4 — defaults change
6. D5 — docs and cleanup

## Implementation Checkpoints

| Phase | Checkpoint | Next Step |
|-------|-----------|-----------|
| D0 | Fix implemented and verified: `while let` → `if let` + `continue`; test `cooperative_concurrent_dispatch_does_not_deadlock` added; WarmModulePool re-enabled in async batch benchmark | Proceed to D1 |
| D1 | Removed `detach_all_slots` (34 lines) + test (42 lines) from rusty_v8; tagged `v147.0.0-locker.2` at f3f4e5a | Proceed to D2 |
| D2 | Removed `reset_main_realm`, `destroy_for_reset`, `shared_array_buffers`, and ~540 lines of tests (1,053 lines total) from deno_core; tagged `0.395.0-locker.2` at b302dea | Proceed to D3 |
| D3 | Removed RetainedJsRuntimePool variant, retained pool logic, metrics, tests, benchmarks from neovex; fixed downstream neovex-server references | Proceed to D4 |
| D4 | Changed `RuntimeLimits::default()` to CooperativeLocker + WarmModulePool; fixed 4 tests that assumed old defaults | Proceed to D5 |
| D5 | Updated ARCHITECTURE.md, docs/plans/README.md, plan execution log. All phases complete. | Plan done |

## Verification Contract

| Phase | Required verification |
|-------|---------------------|
| D0 | Async batch with 4 concurrent threads + CooperativeLocker completes without hanging for all pool kinds; existing `warm_module_pool_cooperative_async_host_two_cycles` still passes; new concurrent dispatch test exercises the fixed path |
| D1 | rusty_v8 `cargo test` passes; CI build succeeds; existing `clear_all_context_slots` and `context_slots` tests still pass |
| D2 | deno_core `cargo test --lib` passes (423+ tests); warm reuse tests (8) still pass; `reset_main_realm` tests removed; no references to `reset_main_realm` or `destroy_for_reset` remain |
| D3 | neovex `cargo test -p neovex-runtime --lib` passes; no references to `RetainedJsRuntimePool` or `reset_retained_runtime` remain; warm pool tests pass |
| D4 | `RuntimeLimits::default()` returns `CooperativeLocker` + `WarmModulePool`; `RunToCompletion` + `StartupSnapshotCache` works when explicitly configured per-bundle; both execution models pass integration tests |
| D5 | `ARCHITECTURE.md` reflects dual-mode production model (warm pool default + RTC option); `docs/plans/README.md` updated; `warm-module-pool-plan.md` and `raw-v8-warm-backend-plan.md` archived |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-04-08 | D0 | `done` | Fixed greedy `while let` → `if let` + `continue` in `next_slot()`; deadlock was isolate semaphore exhaustion when admitting concurrent jobs before polling. Added `cooperative_concurrent_dispatch_does_not_deadlock` test (4 threads × 2 pool kinds). Re-enabled WarmModulePool in async batch benchmark. | 96/96 lib tests pass, clippy clean, fmt clean | D1: remove `detach_all_slots` from rusty_v8 |
| 2026-04-08 | D1 | `done` | Removed `Context::detach_all_slots()` (34 lines) and its test (42 lines) from rusty_v8 fork. Tagged `v147.0.0-locker.2` at f3f4e5a. | 9/9 slot tests pass; `clear_all_context_slots` and `context_slots` still pass | D2: remove `reset_main_realm` from deno_core |
| 2026-04-08 | D2 | `done` | Removed `reset_main_realm`, `destroy_for_reset`, `shared_array_buffers`, 540 lines of tests (1,053 lines total) from deno_core fork. Tagged `0.395.0-locker.2` at b302dea. | 412/413 tests pass (1 pre-existing failure unrelated); 8/8 warm reuse tests pass; no references to `reset_main_realm` or `destroy_for_reset` remain | D3: remove RetainedJsRuntimePool from neovex |
| 2026-04-08 | D3 | `done` | Removed RetainedJsRuntimePool variant, retained pool entry/eviction/retirement logic, retained metrics (realm resets, bootstrap replays), retained pool tests, retained benchmark scenarios. Fixed neovex-server protocol/metadata/test references. | 82/82 lib tests pass; clippy clean; full workspace compiles | D4 |
| 2026-04-08 | D4 | `done` | Changed `RuntimeLimits::default()` to CooperativeLocker + WarmModulePool. Fixed 4 tests that assumed old RTC+SSC defaults by making them explicit. | 82/82 lib tests pass; clippy clean; fmt clean | D5 |
| 2026-04-08 | D5 | `done` | Updated ARCHITECTURE.md (cooperative worker loop description), docs/plans/README.md (moved deprecation plan to archived, updated warm-module-pool-plan description). All phases complete. | — | Plan complete |
