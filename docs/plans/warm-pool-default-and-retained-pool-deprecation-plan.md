# Plan: Warm Pool Default and Retained Pool Deprecation

Make `WarmModulePool` + `CooperativeLocker` the only production runtime path.
Remove `RetainedJsRuntimePool`, `RunToCompletion` (production), and all
`reset_main_realm`-based code across neovex, deno_core, and rusty_v8.

---

## Status

- **Status:** `todo`
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

Nothing that CooperativeLocker + WarmModulePool doesn't already provide better.
RunToCompletion was the safe default before cooperative scheduling was proven.
Now that CooperativeLocker is production-tested with FIFO fairness, generation
guards, and full quiescence checks, RunToCompletion has no remaining advantage.

`StartupSnapshotCache` stays as a development/compatibility mode for local
testing where developers want fresh-per-invocation semantics.

---

## Blocker: Async Concurrent Dispatch Hang

**Must be resolved before deprecation.**

`invoke_blocking` with 4+ concurrent threads + `WarmModulePool` +
`CooperativeLocker` freezes indefinitely. Sequential warm async works (proven
by `warm_module_pool_cooperative_async_host_two_cycles` unit test).

The hang is in the executor admission/dispatch layer — not the warm pool.
After the first invocation completes and the runtime is warm-returned to the
pool, subsequent queued jobs do not execute.

Likely root cause: the isolate semaphore permit or admission wake signal is
not properly released/fired during the warm pool return path in the
cooperative worker loop.

### Key files for investigation

- `crates/neovex-runtime/src/executor/invoke.rs` — `invoke_on_worker_blocking`
- `crates/neovex-runtime/src/worker_loop/cooperative.rs` — worker loop job processing
- `crates/neovex-runtime/src/worker_loop/cooperative/retention.rs` — warm return
- `crates/neovex-runtime/src/executor/admission.rs` — permit lifecycle
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
| `RunToCompletion` variant (production use) | `limits.rs` | ~5 | **Deprecate** (keep for StartupSnapshotCache dev mode) |
| `reset_retained_runtime()` + `reset_retained_runtime_inner()` | `driver/construction.rs` | ~70 | **Yes** |
| Retained pool take/return/eviction logic | `retained_pool.rs` | ~200 | **Replace** with warm-pool-only |
| Retained pool metrics (`main_realm_resets`, `bootstrap_replays`) | `metrics.rs`, `global.rs` | ~30 | **Yes** |
| Retained pool tests | `tests/retained_pool.rs` | ~150 | **Replace** with warm pool tests |
| `FreshPerInvocation` module state semantics | `limits.rs` | ~3 | **Keep** for StartupSnapshotCache |
| Benchmark retained pool scenarios | `runtime_pool_modes.rs` | ~50 | **Remove** cooperative retained; keep RTC retained if RunToCompletion stays |

---

## Phase Status Ledger

| Phase | Status | Summary | Hard Dependencies |
|-------|--------|---------|-------------------|
| D0 | `todo` | Fix async concurrent dispatch hang | None — blocker for all subsequent phases |
| D1 | `todo` | Remove `detach_all_slots` from rusty_v8 fork | None |
| D2 | `todo` | Remove `reset_main_realm` and `destroy_for_reset` from deno_core fork | D0 (proves warm pool handles all workloads) |
| D3 | `todo` | Remove `RetainedJsRuntimePool` from neovex | D2 |
| D4 | `todo` | Make `CooperativeLocker` + `WarmModulePool` the default `RuntimeLimits` | D3 |
| D5 | `todo` | Update docs, benchmarks, and plan index | D4 |

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
| D0 | none yet | Investigate executor admission/dispatch for warm pool return wake signal |
| D1 | none yet | Remove `detach_all_slots` + test from rusty_v8, tag `v147.0.0-locker.2` |
| D2 | none yet | Remove `reset_main_realm`, `destroy_for_reset`, `shared_array_buffers`, and ~540 lines of tests from deno_core; tag `0.395.0-locker.3` |
| D3 | none yet | Remove `RetainedJsRuntimePool`, `reset_retained_runtime`, retained pool logic from neovex; update Cargo.toml to new fork tags |
| D4 | none yet | Change default `RuntimeLimits` to `CooperativeLocker` + `WarmModulePool` |
| D5 | none yet | Update `ARCHITECTURE.md`, `docs/plans/README.md`, archive completed plans |

## Verification Contract

| Phase | Required verification |
|-------|---------------------|
| D0 | Async batch benchmark with 4 concurrent threads + WarmModulePool completes without hanging; existing `warm_module_pool_cooperative_async_host_two_cycles` still passes |
| D1 | rusty_v8 `cargo test` passes; CI build succeeds; existing `clear_all_context_slots` and `context_slots` tests still pass |
| D2 | deno_core `cargo test --lib` passes (423+ tests); warm reuse tests (8) still pass; `reset_main_realm` tests removed; no references to `reset_main_realm` or `destroy_for_reset` remain |
| D3 | neovex `cargo test -p neovex-runtime --lib` passes; no references to `RetainedJsRuntimePool` or `reset_retained_runtime` remain; warm pool tests pass |
| D4 | `RuntimeLimits::default()` returns `CooperativeLocker` + `WarmModulePool`; `StartupSnapshotCache` still works for explicit dev-mode configuration |
| D5 | `ARCHITECTURE.md` reflects warm-pool-only production model; `docs/plans/README.md` updated; `warm-module-pool-plan.md` and `raw-v8-warm-backend-plan.md` archived |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| (none yet) | | | | | |
