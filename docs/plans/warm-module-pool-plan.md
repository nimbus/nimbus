# Plan: Warm Module Pool

Canonical execution plan for adding a `WarmModulePool` runtime pool kind to
`neovex-runtime`. Warm module pooling keeps evaluated user modules alive across
invocations on the same worker-local isolate, skipping realm reset, bootstrap
replay, and module reload on warm hits.

This plan supersedes `docs/plans/raw-v8-warm-backend-plan.md` as the primary
path to warm execution semantics. The raw-V8 backend plan remains as a deferred
fallback if a fundamental `deno_core` limitation blocks this approach.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** promote only after the Locker fork plan
  (`docs/plans/v8-locker-fork-plan.md`) completes Phase 5 and the retained
  pool path (`RetainedJsRuntimePool`) is proven green with `reset_main_realm()`
  on the repaired remote fork
- **Gate status (2026-04-07):** `reset_main_realm()` is now proven green —
  all 4 previously-crashing 32-cycle snapshot-born reset tests pass 20/20.
  The root cause was eagerly dropping V8 Global handles during context
  teardown while V8's NativeContext references were still live. The fix
  (`destroy_for_reset` skips all V8 handle cleanup, leaks Rc refs) is
  committed on the `locker-v0.395` branch. This plan is now eligible for
  promotion once Locker fork Phase 5 completes.

## How To Use This Plan

- Read this before starting any warm-module-pool implementation work.
- Treat the current git worktree plus this plan's ledger as progress state.
- Resume any `in_progress` phase before starting a new one.
- Checkpoint state here before stopping, handing off, or likely context loss.

## Control Plan Rules

### Status model

- `todo`: not started; eligible when hard dependencies and gate notes are met
- `in_progress`: actively being implemented; keep exactly one phase in this
  state per autonomous execution run
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification has been recorded
- `deferred`: intentionally parked behind a product or benchmarking gate

### Recovery loop for every new session

1. Reread `Control Plan Rules`, `Phase Status Ledger`, `Implementation
   Checkpoints`, and `Execution Log`.
2. Inspect the current git worktree and reconcile against this plan.
3. If any phase is `in_progress`, resume it first.
4. If the worktree is dirty, identify which phase owns the changes before
   starting new work.
5. Implement exactly one phase by default.
6. Record verification in `Execution Log` before marking a phase `done`.

---

## Why This Approach Instead of a Raw V8 Backend

The `raw-v8-warm-backend-plan.md` proposed building an entire second engine
(raw V8 module loader, event loop, op dispatch, promise resolution) to avoid
modifying the `deno_core` fork. A six-agent audit verified that the fork
changes are surgical and the raw backend is unnecessary:

- `deno_core` already handles "module already loaded" gracefully —
  `RecursiveModuleLoad` finds the root module ID, skips the loader, returns
  `Done`. `mod_evaluate()` on an already-evaluated module returns `Ok(())`
  immediately.
- `OpState` lives in `JsRuntimeState`, not in the realm — already at the
  right level for per-request swapping via the existing `put()` API.
- The entire deno_core event loop (ops, timers, promises, microtask
  checkpoints, Tokio integration) is inherited for free.
- The original `~103`-line fork estimate is a plausible lower bound for
  production code only, but a mergeable WM1 fork patch should be budgeted more
  honestly at roughly `120-180` production lines and `250-400` total lines once
  focused negative tests and guard rails are included — still far smaller than
  a raw V8 backend.
- Even if warm pooling later fails the benchmark/promotion gates, the fork work
  remains useful: `is_warm_reuse_safe()` and `reset_request_state()` are
  reusable quiescence/reset primitives that improve retained-runtime honesty and
  preserve the earned diagnostic surface.

### Latency comparison (estimated)

| Path | Per-request cost | Key operations |
|------|-----------------|----------------|
| `StartupSnapshotCache` | ~4-12ms | new JsRuntime + module load + evaluate + invoke |
| `RetainedJsRuntimePool` | ~2-5ms | realm reset + bootstrap replay + module reload + invoke |
| `WarmModulePool` | ~0.1-0.3ms | request-state swap + invoke stored handler |

### Decision rule against the raw-V8 fallback

Continue with this plan unless one of two things becomes true:

1. **WM1 proves the fork reset cannot be made sound.** If
   `reset_request_state()` cannot preserve evaluated modules while clearing
   request-local/event-loop state safely on the live fork, promote the raw-V8
   fallback.
2. **WM5 proves the forked warm path still misses the target badly enough to
   justify a second backend.** A raw-V8 backend is justified only if the
   measured latency/throughput gap remains material after the fork path is
   implemented and benchmarked.

---

## Architecture

### Execution model constraint

**The first `WarmModulePool` implementation is `CooperativeLocker`-only.** The
current runtime explicitly caps `RunToCompletion` to one retained idle runtime
per worker (`snapshot.rs:458`). Warm pooling inherits that constraint: it must
not enable multi-entry warm pools under run-to-completion execution. The warm
pool requires Locker-based cooperative scheduling for correct multi-entry
retention and FIFO waiter fairness.

If a future implementation extends warm pooling to `RunToCompletion`, it must
enforce the same single-entry-per-worker invariant that `RetainedJsRuntimePool`
does today.

### Configuration surface

No new backend kind. One new pool kind alongside the existing two:

```text
RuntimePoolKind
  - startup_snapshot_cache        (default: fresh JsRuntime per invocation)
  - retained_jsruntime_pool       (opt-in: retained JsRuntime, realm reset per invocation)
  - warm_module_pool              (opt-in: retained JsRuntime, NO realm reset, surgical cleanup)
```

Validation rules:
- `warm_module_pool` requires `execution_model = cooperative_locker`
- `warm_module_pool` with `execution_model = run_to_completion` must fail fast

Existing `deno_core` behavior must remain unchanged. Warm reuse should be
introduced through separate additive APIs and guards, not by widening or
changing the meaning of existing surfaces like `reset_main_realm()` or
`EventLoopPendingState::is_pending()`.

A new module-state semantics variant makes the contract explicit:

```text
RuntimeModuleStateSemantics
  - FreshPerInvocation            (existing: modules are reloaded every invocation)
  - WarmPerBundle                 (new: modules persist across invocations by contract)
```

### Invocation path and ABI

The invocation ABI is unchanged. User bundles define
`globalThis.__neovexInvoke = async function(request) { ... }` and the runtime
calls it via `execute_script("globalThis.__neovexInvoke({request_json})")`.
Inside `__neovexInvoke`, user code calls `__neovexCreateContext(options)` to
build a `ctx`. The warm path re-calls the same expression — no new handler ABI
is needed.

```text
Cold miss (first invocation for a bundle on this worker):
  create JsRuntime (snapshot or unsnapshotted)
  → load_main_es_module() + mod_evaluate() + run_event_loop()
  → execute_script("globalThis.__neovexInvoke({request_json})")
  → with_event_loop_promise() to resolve handler result
  → verify_warm_reuse_safe()        [all event loop state must be idle]
  → store bundle identity in WarmPoolEntry
  → return WarmPoolEntry to pool

Warm hit (same bundle identity on same worker):
  take WarmPoolEntry from pool (match by bundle identity + affinity key)
  → reset_request_state()          [deno_core fork: surgical per-request cleanup]
  → reset_runtime_invocation_state [existing: swap OpState resources]
  → reset_warm_invocation_state    [new: bump generation + __neovexNextSessionId = 1]
  → execute_script("globalThis.__neovexInvoke({request_json})")
    ↳ __neovexInvoke internally calls __neovexCreateContext(options)
    ↳ ctx closures capture current generation, guard stale on every call
  → with_event_loop_promise() to resolve handler result
  → verify_warm_reuse_safe()        [all event loop state must be idle]
  → return WarmPoolEntry to pool    [or discard if not quiescent]
```

### Pool structure

The warm pool is a **parallel structure** alongside the existing retained pool
in `RuntimeWorkerIsolatePool`. It cannot extend the retained pool because
take/return semantics differ fundamentally (bundle identity matching vs
affinity-only matching, no `reset_main_realm()` on take).

```rust
// In RuntimeWorkerIsolatePool
struct WarmPoolEntry {
    runtime: JsRuntime,
    bundle_identity: RuntimeBundleIdentity,
    affinity_key: Option<RuntimeAffinityKey>,
    reuse_count: usize,
    last_used_sequence: u64,
    construction_mode: RuntimeConstructionMode,
}
```

The first implementation intentionally does **not** cache a
`v8::Global<v8::Function>` for `__neovexInvoke`. Warm hits keep the existing
`execute_script("globalThis.__neovexInvoke({request_json})")` path so the
invocation ABI stays unchanged and WM2/WM3 do not need extra function-handle
capture/bookkeeping. If WM5 later shows that string-based dispatch is a
material fixed warm-hit cost, a direct-call optimization can be added as a
separate follow-up.

---

## Blocker: Bootstrap Request-Local Closure Capture

**This is a merge blocker that must be resolved before WM3 (warm invocation
path).**

### The problem

`__neovexCreateContext()` (`source.rs:251`) captures request-bound state into
JS closures at ctx creation time:

- `sessionId` (line 252) — flows into every `syncHostValue`/`asyncHostValue`
- `requestAuth` (line 256) — passed to `__neovexRunNamedFunction()` for nested
  `runQuery`/`runMutation`/`runAction` (lines 387, 399, 411)
- `authIdentity` (line 263) — returned by `ctx.auth.getUserIdentity()` (304)
- `verifiedAuthIdentity` (line 269) — returned by
  `ctx.auth.getVerifiedIdentity()` (307)
- `throwOnMissingIdentity` (line 275)

In a warm pool, module globals persist. User code that stashes a prior
request's `ctx` (e.g., `let savedCtx = null; savedCtx = ctx;`) would reuse it
on a later request with the old auth/session still captured. This is a
**security boundary violation**: previous-request auth identity leaks into a
subsequent request.

### The fix: invocation-generation guard on the entire ctx surface

Add a monotonic generation counter to the bootstrap. Each warm reset bumps the
counter. `__neovexCreateContext()` captures the current generation. A
`guardStale()` check wraps **every ctx method** — not just host calls, but also
`ctx.auth.getUserIdentity()` and `ctx.auth.getVerifiedIdentity()`, which
return captured request identity directly without going through
`syncHostValue`/`asyncHostValue`:

```javascript
let __neovexInvocationGeneration = 0;

globalThis.__neovexCreateContext = function(options = {}) {
  const myGeneration = __neovexInvocationGeneration;

  const guardStale = () => {
    if (__neovexInvocationGeneration !== myGeneration) {
      throw new Error(
        "This ctx object is from a previous invocation and cannot be reused"
      );
    }
  };

  const syncHostValue = (opName, payload) => {
    guardStale();
    return globalThis.__neovexSyncHostValue(opName, {
      session_id: sessionId,
      ...(payload ?? {}),
    });
  };

  const asyncHostValue = (opName, payload) => {
    guardStale();
    return globalThis.__neovexAsyncHostValue(opName, {
      session_id: sessionId,
      ...(payload ?? {}),
    });
  };

  // Auth methods also guarded — they return captured identity directly
  // without going through host ops
  return {
    auth: Object.freeze({
      async getUserIdentity() {
        guardStale();
        return cloneAuthIdentityOrThrow(authIdentity);
      },
      async getVerifiedIdentity() {
        guardStale();
        return cloneAuthIdentityOrThrow(verifiedAuthIdentity);
      },
    }),
    // ... rest of ctx construction with guardStale in syncHostValue/asyncHostValue ...
  };
};
```

The warm reset script becomes:

```javascript
__neovexNextSessionId = 1; __neovexInvocationGeneration++;
```

This fails loud, prevents any use of stale ctx objects (including auth data
that never touches host ops), and requires zero changes to the host bridge or
OpState.

### Why not an indirection layer instead?

An alternative is to make closures read from a mutable
`globalThis.__neovexCurrentInvocation` slot at call time. That also works, but
the generation guard is simpler (6 lines of JS), fails louder (explicit error
vs silent wrong-auth), and requires no restructuring of the existing
`syncHostValue`/`asyncHostValue` closure pattern.

### Phase assignment

This fix is assigned to **WM0** as a prerequisite. It can land independently
of warm pooling — it improves safety for any future retained-runtime path.

---

## Blocker: Full Quiescence Boundary

**This is a merge blocker that must be resolved before WM1 (fork reset API).**

### The problem

The plan originally used `pending_ops.len() == 0` as the warm-reset boundary.
That is necessary but not sufficient. The deno_core event loop tracks 12
sources of pending state in `EventLoopPendingState`, and the existing
`is_pending()` method only checks 9 of them:

**Checked by `is_pending()`:**
`has_pending_refed_ops`, `has_pending_dyn_imports`,
`has_pending_dyn_module_evaluation`, `has_pending_module_evaluation`,
`has_pending_background_tasks`, `has_tick_scheduled`,
`has_pending_promise_events`, `has_pending_external_ops`,
`has_uv_alive_handles`

**NOT checked by `is_pending()`:**
- `has_pending_ops` — includes unrefed ops that `is_pending()` ignores
- `has_outstanding_immediates` — `setImmediate` callbacks
- `has_pending_timers` — `setTimeout`/`setInterval`

Using `!is_pending()` as the warm boundary would allow a runtime with live
timers or outstanding immediates to be returned to the pool.

`invoke_loaded_bundle_with_trace()` uses `with_event_loop_promise()`, which
returns as soon as the handler promise resolves — even if other event-loop
state is still live. A warm pool that returns the runtime to the pool at that
point would carry forward live timers, pending promises, or scheduled ticks
into the next request.

### The fix: new `is_warm_reuse_safe()` method in fork + drain loop

The warm boundary must be stricter than `is_pending()`. WM1 adds a new
`EventLoopPendingState::is_warm_reuse_safe()` method in the fork that checks
**all 12 fields**:

```rust
/// Returns true only if no event loop state is live — safe to return
/// this runtime to a warm pool without carrying forward request state.
/// Stricter than is_pending(): also rejects pending timers,
/// outstanding immediates, and unrefed ops.
pub fn is_warm_reuse_safe(&self) -> bool {
    !self.has_pending_ops
        && !self.has_pending_refed_ops
        && !self.has_pending_dyn_imports
        && !self.has_pending_dyn_module_evaluation
        && !self.has_pending_module_evaluation
        && !self.has_pending_background_tasks
        && !self.has_tick_scheduled
        && !self.has_pending_promise_events
        && !self.has_pending_external_ops
        && !self.has_outstanding_immediates
        && !self.has_pending_timers
        && !self.has_uv_alive_handles
}
```

The full quiescence boundary is:

1. After the handler promise resolves, run a drain loop:
   `poll_event_loop()` until `is_warm_reuse_safe() == true`.
2. If quiescence is not reached within a bounded tick count (e.g., 100 ticks),
   **discard the runtime** instead of returning it to the pool. Record a
   `warm_pool_discard_unquiesced` metric.
3. `reset_request_state()` in the fork must guard on
   `is_warm_reuse_safe()`, not `is_pending()` or `pending_ops.len() == 0`.

Additionally, WM1 must include **negative tests** that prove:
- A runtime with a live `setTimeout` callback is rejected by
  `reset_request_state()`.
- A runtime with a pending unresolved promise is rejected.
- A runtime with a pending dynamic import is rejected.
- A runtime with an outstanding `setImmediate` callback is rejected.

### Phase assignment

The quiescence contract is part of **WM1** (fork reset API). The drain-loop
and discard logic are part of **WM3** (warm invocation path).

---

## Required deno_core Fork Changes

### Fork lineage and Locker compatibility

WM1 changes are applied to the `agentstation/deno_core` fork **on top of**
tag `0.395.0-locker.1` (or its Phase 5 successor) to produce a new tag
`0.395.0-locker.2` rather than repairing `0.395.0-locker.1` in place again.
The base tag includes the Locker-aware `ManagedIsolate`
abstraction, `use_locker` runtime option, and the public RAII lock-handoff API
(`acquire_v8_lock`, `release_v8_lock`, `is_v8_lock_held`).

Since the warm pool is `CooperativeLocker`-only, warm runtimes use
`use_locker: true`. The V8 lock checks in `ensure_v8_lock_held()` are **not
no-ops** on this path — they go through `ManagedIsolate::Lockable` and
actually verify/acquire the `v8::Locker`. All new methods must follow the
fork's established Locker patterns:

- `reset_request_state(&mut self)` must ensure the V8 lock is held before any
  V8 state access. An explicit `self.ensure_v8_lock_held()` at the top is the
  clearest pattern and matches `reset_main_realm()` at `jsruntime.rs:1566`.
- `is_warm_reuse_safe(&mut self)` takes `&mut self` (not `&self`) because it
  must read V8-backed event-loop state through helpers that ensure the lock is
  held (`scope!`, `v8_isolate()`, or `v8_isolate_ptr()`). The important
  contract is "lock held before V8 access," not a specific internal call order.
- The `scope!` macro already routes through `v8_isolate()` which calls
  `ensure_v8_lock_held()` internally, so scope creation in the new methods
  works correctly for both `Owned` and `Lockable` paths.
- No additional lock management is needed once the method has entered the
  held-lock path — the lock remains held for the duration of the method unless
  the implementation explicitly releases it, which WM1 should not do.

### Scope note

Realistic scope note: the per-item counts below are useful as lower-bound
production-code estimates, not as a full merge budget. The semantic risk is
larger than the raw line count, especially around proving that the first reset
pass clears all request-local state that must not survive a warm boundary.

The op-driver reset was previously identified as the "primary friction point."
A deep trace of the `Rc<OpDriverImpl>` sharing topology (shared by
`ContextState` + every `OpCtx`, refcount ≥ N+2) confirmed that
`*self = Self::default()` is not viable. However, the same trace showed that
**no explicit driver reset is needed at quiescence**: the entire driver uses
interior mutability (`Cell`, `RefCell`) and is immediately reusable for new ops
when `is_warm_reuse_safe()` confirms all pending state is drained. This
eliminates the friction point and reduces both implementation risk and line
count.

### 1. `EventLoopPendingState::is_warm_reuse_safe()` (~15 lines)

New method that checks all 12 `EventLoopPendingState` fields. Unlike
`is_pending()`, this also rejects `has_pending_ops` (including unrefed),
`has_outstanding_immediates`, and `has_pending_timers`.

### 2. `JsRuntime::is_warm_reuse_safe()` (~8-12 lines)

Public wrapper for Neovex. Creates a scope, snapshots current
`EventLoopPendingState`, and returns whether the runtime is safe to return to a
warm pool. WM3 needs this surface for the post-handler drain loop.

### 3. `JsRuntime::reset_request_state()` (~40-70 lines)

Public entry point. Guards on `is_warm_reuse_safe()` (not `is_pending()`),
drains foreground tasks and microtasks, and clears fork-managed
per-request/event-loop state while preserving evaluated modules and
bootstrap/user globals. It does not, by itself, define or clear arbitrary
embedder-specific `OpState`; warm users must separately refresh any
additional request-local `OpState` they add outside the current Neovex
bootstrap contract.

**Important operational requirements** (learned from the `reset_main_realm`
investigation, 2026-04-07):

- **Foreground task drain.** V8 background threads (TurboFan, code cache,
  GC finalization) enqueue foreground tasks during async op yields. These
  must be drained (from `foreground_tasks` queue) before the microtask
  checkpoint, within a ContextScope for the current context. Running V8
  foreground tasks without an active ContextScope is undefined behavior.
- **TryCatch for microtask checkpoint.** On snapshot-born contexts,
  `perform_microtask_checkpoint()` must be called within a TryCatch scope.
  Without TryCatch, V8's internal exception state from deserialized promise
  reactions can corrupt the isolate. This applies to both `reset_main_realm`
  and `reset_request_state`.
- **Drain loop.** A single drain pass is insufficient — foreground tasks
  may enqueue microtasks, and microtasks may trigger background work that
  enqueues more foreground tasks. Loop until both queues are empty (bounded
  to prevent infinite loops).

### 4. `ExceptionState::clear_request_state()` (~10 lines)

Clears `dispatched_exception`, `dispatched_exception_is_promise`,
`pending_promise_rejections`, `pending_handled_promise_rejections`. Preserves
bootstrap-installed JS callbacks (`js_build_custom_error_cb`, etc.).

### 5. `ModuleMap::clear_pending_state()` (~16-22 lines)

Clears 9 in-flight fields (`dynamic_import_map`, `preparing_dynamic_imports`,
`pending_dynamic_imports`, `pending_dyn_mod_evaluations`,
`pending_tla_waiters`, `pending_mod_evaluation`, `evaluating_top_level`,
`code_cache_ready_futs`, `dyn_module_evaluate_idle_counter`). Preserves
`data` (the module registry) and `loader`.

### 6. ContextState field resets (~15-25 lines)

Zero `tick_info`, `immediate_info`, `timer_info`, `timer_expiry`. Clear
`active_timers`, `unrefed_ops`, `event_loop_phases`, `activity_traces`.
Reset `user_timer` and `external_ops_tracker`.

### 7. Op driver: no explicit reset needed at quiescence

The `pending_ops` field is `Rc<OpDriverImpl>` shared by `ContextState` and
every `OpCtx` (one per registered op — refcount ≥ N+2). `Rc::get_mut()` is
impossible, so `*self = Self::default()` is not viable from outside the driver.

However, a deep trace of the driver topology shows **no explicit reset is
needed** if `is_warm_reuse_safe()` confirms quiescence:

- `len() == 0` — no in-flight ops (Cell<usize>)
- `completed_ops` is empty — all results drained by the event loop
  (Rc<RefCell<VecDeque>>)
- The background poll task remains live and idles once the submission queue is
  empty; warm reuse should keep that handle alive rather than calling
  `shutdown()` in the reuse path
- The submission queue is empty (interior mutability via RefCell)
- The arena is a memory pool, safe to reuse without reset

The entire driver is built on interior mutability (Cell, RefCell). At
quiescence it is immediately reusable for new ops without any explicit
`reset()` method in the expected warm path. This eliminates the "primary
friction point" identified in earlier revisions. Existing
`runtime::op_driver::tests::test_driver_yield` already exercises repeated
submit → drain → submit reuse on the same driver instance.

If WM1 implementation discovers residual driver state that survives
quiescence (e.g., stale waker registrations), a targeted `clear_residual()`
method using `&self` through interior mutability is straightforward — no
`&mut self` or Rc replacement needed. `shutdown()` remains a teardown-only
operation: in the current driver it aborts the handle without resetting
`task_set`, so warm reset must not rely on post-shutdown auto-respawn.

### 8. Helper methods (~12-20 lines)

`V8TaskSpawnerFactory::clear()` (~5 lines) and
`ExternalOpsTracker::reset()` (~3 lines) plus
`RuntimeActivityTraces::clear_traces()` (~5 lines). These are preferable to
changing existing field visibility.

**Fork production-code total: likely ~120-180 lines in practice.** The
original `~103`-line figure remains a plausible lower bound, but a mergeable
WM1 fork patch including focused negative tests and guard rails is more
realistically budgeted at `250-400` total lines. All additive. No existing
behavior changes are required if the new warm-reset APIs remain separate from
existing `deno_core` semantics.

---

## Required Neovex-Runtime Changes

### 1. `src/runtime/bootstrap/source.rs` (~20 lines) — WM0

- Add `__neovexInvocationGeneration` counter to `BOOTSTRAP_SOURCE`
- Wrap every ctx method in `guardStale()` check: `syncHostValue`,
  `asyncHostValue`, `ctx.auth.getUserIdentity()`,
  `ctx.auth.getVerifiedIdentity()`
- Update `RESET_BOOTSTRAP_INVOCATION_STATE_SOURCE` to also bump generation

### 2. `src/limits.rs` (~24 lines) — WM2

- `WarmModulePool` variant on `RuntimePoolKind`
- `WarmPerBundle` variant on `RuntimeModuleStateSemantics`
- Update `module_state_semantics()`, `reset_capabilities()`, `normalized()`
- Validation: `WarmModulePool` requires `CooperativeLocker`; fail fast
  otherwise
- New limit fields: `max_warm_module_pool_entries_per_worker`,
  `max_warm_module_reuses`

### 3. `src/runtime/bootstrap/snapshot.rs` (~138 lines) — WM2

- `WarmPoolEntry` struct
- `warm_pool: Vec<WarmPoolEntry>` field on `RuntimeWorkerIsolatePool`
- `WarmModulePool` arm in `take_runtime_with_options_for_invocation()` —
  match by bundle identity, skip `reset_retained_runtime()`, return warm entry
- `WarmModulePool` arm in `return_runtime_with_affinity()` — store with
  bundle identity, enforce bounds
- `take_warm_pool_entry()` — find by identity + affinity key
- `enforce_warm_pool_bounds()` — idle-only LRU eviction

### 4. `src/runtime.rs` (~40 lines) — WM3

- Warm-hit branch in `invoke_bundle_unmanaged()` — skip
  `load_bundle_with_trace()` and keep the existing
  `execute_script("globalThis.__neovexInvoke({request_json})")` dispatch path
- Same branch in `start_cooperative_locker_runtime_slot()`
- Post-handler quiescence drain loop: poll event loop until
  `is_warm_reuse_safe() == true` or tick limit, discard on timeout

### 5. `src/worker_loop/cooperative.rs` (~4 lines) — WM3

- `WarmModulePool` arm in `retain_or_defer_runtime_drop()`

### 6. `src/metrics.rs` + `src/metrics/global.rs` (~50 lines) — WM4

- `warm_pool_hits`, `warm_pool_misses`, `warm_pool_retirements`,
  `warm_pool_discard_unquiesced` counters
- Recorder methods, snapshot reads, diagnostics exposure

**Neovex total: ~276 lines.**

---

## Verified Invariants

These were validated by the six-agent audit and must hold throughout
implementation:

1. **Full event loop quiescence before warm reset.** Guard on
   `EventLoopPendingState::is_warm_reuse_safe()`, not `is_pending()` (which
   skips timers/immediates/unrefed-ops) or `pending_ops.len() == 0`. Discard
   the runtime if quiescence cannot be reached.
2. **Stale ctx objects must throw on use.** The generation guard prevents any
   host call through a ctx captured in a prior invocation.
3. **Foreground task drain + microtask checkpoint before clearing state.**
   V8 background threads enqueue foreground tasks during async op yields;
   V8 microtasks are isolate-level under `Explicit` policy. Both must be
   drained in a loop (within a ContextScope + TryCatch) before warm reset.
   The TryCatch is required because `perform_microtask_checkpoint` on
   snapshot-born contexts can encounter V8 exception state from deserialized
   promise reactions — without TryCatch, V8's exception propagation corrupts
   isolate state. (Confirmed 2026-04-07 during `reset_main_realm` investigation.)
4. **`unrefed_ops` must be cleared on warm reset.** Stale op IDs cause
   incorrect `EventLoopPendingState` calculations.
5. **Module re-loading is already idempotent.** `load_main_es_module()` on
   an already-loaded module returns the existing `ModuleId` without re-parsing.
   `mod_evaluate()` on an already-evaluated module returns `Ok(())`.
6. **Bootstrap JS callbacks persist in the warm context.** The 6 bootstrap
   callbacks (`js_event_loop_tick_cb`, etc.) are `v8::Global<v8::Function>`
   handles created in the context — they survive naturally when the context
   is not destroyed.
7. **Warm-hit invocation ABI stays unchanged.** The warm path can keep calling
   `globalThis.__neovexInvoke(request)` directly. `__neovexInvoke` from user
   code and `__neovexCreateContext` from bootstrap both persist naturally when
   the main realm is retained.
8. **Existing `deno_core` behavior is preserved if warm reset stays additive.**
   `reset_main_realm()`, `EventLoopPendingState::is_pending()`, and the normal
   event-loop / module-loading semantics do not need to change for warm pooling
   to work.
9. **Current Neovex `OpState` usage is narrow and explicit.** Today the
   bootstrap stores persistent `RuntimeHostState` in `OpState`, refreshes
   `RuntimeCancellationState` and `SharedInvocationPermit` per invocation, and
   does not otherwise rely on `resource_table`, `gotham_state`, or
   `unrefed_resources` for request-local Neovex runtime state.
10. **No user-facing `v8::Weak` references exist in deno_core.** Eliminates
   an entire class of warm-persistence bugs (weak callback timing, phantom
   resurrection). Note: rusty_v8's internal `ContextAnnex` creates a
   `Weak<Context>` with a guaranteed finalizer for each context, but this is
   transparent to deno_core and does not affect warm-pool semantics since the
   warm pool keeps contexts alive (the Weak is only relevant during context
   destruction in the `reset_main_realm` path).
11. **Bundle identity match is required for warm hits.** Entrypoint path +
   SHA-256 hash must match. Integrity verification must run before reuse.
12. **Module-level side effects persist by contract.** Top-level `let counter
   = 0` in user code accumulates across requests. This is intentional and
   matches Cloudflare Workers, Deno Deploy, and Vercel Edge Runtime semantics.
   Must be documented and exposed via `module_state_semantics = warm_per_bundle`.
13. **Warm pool entries must be retired after `max_warm_module_reuses`.** Limits
    heap fragmentation and long-tail state accumulation risk.
14. **Warm pool eviction must be idle-only.** Never evict active or parked
    contexts.
15. **Warm pool is `CooperativeLocker`-only.** `RunToCompletion` must not
    enable multi-entry warm pools. Fail fast on invalid combination.

---

## Maintenance Invariants

These are ongoing discipline requirements that survive beyond the initial
implementation. Violating them silently degrades warm-pool safety.

1. **Every new ctx method must include `guardStale()`.** When adding a new
   method or accessor to the object returned by `__neovexCreateContext()`, it
   must call `guardStale()` before accessing any captured state or calling
   host ops. This includes methods that return captured data directly (like
   `ctx.auth.getUserIdentity()`) — not just methods that route through
   `syncHostValue`/`asyncHostValue`. A ctx method without `guardStale()` is a
   stale-auth leak.

2. **`is_warm_reuse_safe()` must track every `EventLoopPendingState` field.**
   When the deno_core fork adds a new field to `EventLoopPendingState` (e.g.,
   for a new async primitive or libuv handle type), `is_warm_reuse_safe()` must
   be updated to reject it. Unlike `is_pending()`, which intentionally ignores
   some fields for event-loop keepalive semantics, `is_warm_reuse_safe()` must
   be exhaustive. A field missing from `is_warm_reuse_safe()` is a
   cross-request state leak. The WM1 test suite should include a compile-time
   or structural assertion that `is_warm_reuse_safe()` references every field
   in `EventLoopPendingState`.

3. **Additional request-local `OpState` requires an explicit warm policy.**
   The current Neovex warm design relies on a narrow `OpState` contract:
   `RuntimeHostState` persists, while `RuntimeCancellationState` and
   `SharedInvocationPermit` are refreshed per invocation. If future runtime ops
   start storing request-local state in generic `OpState` slots such as
   `resource_table`, `gotham_state`, or `unrefed_resources`, warm reuse must
   either clear that state explicitly or reject reuse.

4. **V8 Global handles must not be eagerly dropped during context teardown.**
   The `reset_main_realm` investigation (2026-04-07) proved that calling
   `v8__Global__Reset` on handles from an old context while V8 internally
   still references the NativeContext causes nondeterministic SIGSEGV. The
   warm pool avoids this entirely by keeping contexts alive — this is not
   just a performance advantage but a correctness advantage over
   `reset_main_realm`. If warm pool entries are evicted/retired, they must
   go through `destroy()` (isolate disposal path), not `destroy_for_reset()`.

5. **Warm-pool feasibility is design-validated, not production-proven.** The
   six-agent audit and two review rounds confirmed the approach is architecturally
   sound. The real proof depends on WM1 fork validation (does `reset_request_state()`
   actually work on the live fork?) and WM5 benchmarks (does the latency
   improvement justify the maintenance surface?). Do not treat the plan as
   proven until both gates pass.

---

## Known Risks

| Risk | Severity | Mitigation |
|------|----------|-----------|
| Bootstrap ctx closures capture request auth | **BLOCKER** | WM0: generation guard invalidates stale ctx objects including auth accessors |
| Event loop not fully quiesced before pool return | **BLOCKER** | WM1/WM3: guard on `is_warm_reuse_safe()`, drain loop, discard on timeout |
| Future ctx methods added without `guardStale()` | MEDIUM | Maintenance invariant #1; WM0 tests should cover a representative sample |
| Future `EventLoopPendingState` fields not in `is_warm_reuse_safe()` | MEDIUM | Maintenance invariant #2; WM1 structural assertion |
| Future request-local Neovex state added to generic `OpState` | MEDIUM | Maintenance invariant #3; if runtime ops begin using `resource_table`, `gotham_state`, `unrefed_resources`, or other request-local `OpState` slots, warm reuse must clear them explicitly or reject reuse |
| Op driver residual state at quiescence | LOW | Existing `test_driver_yield` already exercises repeated submit/drain/reuse on the same driver; keep `shutdown()` teardown-only in the warm path, rely on quiescent reuse, and add targeted `clear_residual(&self)` only if WM1 testing discovers stale state |
| Hidden per-request state survives the first `reset_request_state()` pass | MEDIUM | WM1 negative tests plus repeated warm-cycle validation; discard the runtime on any uncertainty instead of reusing it |
| V8 microtask queue stale entries on warm reset | MEDIUM | Mandatory foreground task drain + `perform_microtask_checkpoint()` in a drain loop (within ContextScope + TryCatch) before warm reset; TryCatch required on snapshot-born contexts (confirmed 2026-04-07) |
| V8 foreground tasks from background threads survive between requests | MEDIUM | Drain `foreground_tasks` queue inside a ContextScope before microtask checkpoint; tasks execute with undefined behavior without an active scope (confirmed 2026-04-07) |
| V8 API call during partial `reset_request_state()` triggers internal GC while request-local state is half-cleared | MEDIUM | Order field clears so V8-touching operations (foreground drain, microtask checkpoint) happen first while state is still consistent; Rust-side field clears (exception state, timer info, unrefed_ops) happen after, when no further V8 calls are needed. WM1 stress test with 32+ warm cycles to expose any ordering sensitivity. |
| `unrefed_ops` stale entries affecting event loop pending state | MEDIUM | Clear the set during warm reset |
| `v8::External` closures from `watch_promise` leak on abandoned promises | LOW | GC collects them after context cleanup; heap growth bounded by reuse limit |
| `task_spawner_factory` internal fields are private | LOW | Add `pub(crate) clear()` method in fork |

---

## Phase Status Ledger

| Phase | Status | Summary | Hard Dependencies | Gate Note |
|-------|--------|---------|-------------------|-----------|
| WM0 | `todo` | Bootstrap security: generation guard for stale ctx objects | none — can land independently as safety improvement | ~20 lines in `source.rs`; improves safety for any retained-runtime path |
| WM1 | `todo` | deno_core fork: `is_warm_reuse_safe(&mut self)` + `reset_request_state(&mut self)` with full quiescence guard, Locker-compatible (`ensure_v8_lock_held()` pattern) | Locker fork plan Phase 5 complete; `RetainedJsRuntimePool` proven green with `reset_main_realm()` on remote fork | applied to `0.395.0-locker.1` (or Phase 5 successor) to produce a new tag `0.395.0-locker.2`; production code likely ~120-180 lines; mergeable fork patch with negative tests more like ~250-400 total; must include negative tests for live timers, pending promises, pending dynamic imports, outstanding immediates; must verify Locker path (`use_locker: true`) not just standard path |
| WM2 | `todo` | Neovex warm pool infrastructure: `WarmPoolEntry`, pool take/return, eviction, `CooperativeLocker`-only validation | WM0, WM1 | ~162 lines across `limits.rs` and `snapshot.rs` |
| WM3 | `todo` | Neovex warm invocation path: skip `load_bundle_with_trace()` on warm hit, post-handler quiescence drain, discard on timeout | WM0, WM2 | ~44 lines across `runtime.rs` and `cooperative.rs` |
| WM4 | `todo` | Observability: warm pool metrics including `discard_unquiesced` | WM2 | ~50 lines across metrics files |
| WM5 | `todo` | Benchmark validation and promotion decision | WM3, WM4 | use `runtime_pool_modes.rs` harness; `CooperativeLocker` only; compare fresh vs retained vs warm; record absolute latency and throughput |

## Recommended Delivery Order

1. WM0 — bootstrap generation guard (can land now, no fork dependency)
2. WM1 — fork changes (can be done in `agentstation/deno_core` independently)
3. WM2 — pool infrastructure
4. WM3 + WM4 in parallel — invocation path + metrics
5. WM5 — benchmarks and default decision

## Implementation Checkpoints

| Phase | Checkpoint | Next Step |
|-------|-----------|-----------|
| WM0 | none yet | add `__neovexInvocationGeneration` + `guardStale()` to bootstrap, update reset script, add test proving stale ctx throws |
| WM1 | none yet | land `is_warm_reuse_safe()` + `JsRuntime::is_warm_reuse_safe()` + `reset_request_state()` with full quiescence guard + negative tests in fork, tag, update Neovex patch |
| WM2 | none yet | implement `WarmPoolEntry` and pool take/return in `snapshot.rs`, add `CooperativeLocker` validation in `limits.rs` |
| WM3 | none yet | wire warm-hit branch into `invoke_bundle_unmanaged`, add quiescence drain loop, discard metric |
| WM4 | none yet | add warm pool counters including `discard_unquiesced` to runtime metrics |
| WM5 | none yet | run `runtime_pool_modes.rs` benchmark under `CooperativeLocker`, compare all three pool kinds |

## Verification Contract

Each phase must demonstrate:

| Phase | Required verification |
|-------|---------------------|
| WM0 | Neovex test: ctx from invocation N throws on any host call during invocation N+1; ctx.auth.getUserIdentity() from invocation N throws during invocation N+1; ctx.auth.getVerifiedIdentity() from invocation N throws during invocation N+1; ctx from invocation N works normally during invocation N; generation counter increments on warm reset |
| WM1 | Fork test: `reset_request_state()` preserves evaluated modules, clears per-request state; `EventLoopPendingState::is_warm_reuse_safe()` returns false for all 12 state sources; `JsRuntime::is_warm_reuse_safe()` reports the same boundary to Neovex; **negative tests**: reset is rejected with a live timer, with a pending promise, with a pending dynamic import, with an outstanding immediate; **Locker tests**: all of the above must pass with `use_locker: true` (not just standard path), proving the implementation works on the `ManagedIsolate::Lockable` path and keeps V8 state access on a held-lock path |
| WM2 | Neovex test: warm pool take/return round-trips with correct bundle identity matching; LRU eviction fires at bounds; retirement fires at reuse limit; `WarmModulePool` + `RunToCompletion` fails fast |
| WM3 | Neovex test: warm-hit invocation returns correct result without module reload; cold-miss followed by warm-hit shows expected latency drop; module-level state persists across warm hits (intentional); unquiesced runtime is discarded, not pooled |
| WM4 | Diagnostics test: warm_pool_hits / misses / retirements / discard_unquiesced counters are accurate; `module_state_semantics = warm_per_bundle` is exposed |
| WM5 | Benchmark report: absolute warm-hit latency, comparison against `StartupSnapshotCache` and `RetainedJsRuntimePool`, throughput under concurrent load, all under `CooperativeLocker` execution model; explicitly call out whether per-invocation bundle integrity re-hash is a dominant remaining fixed warm-hit cost |

## Promotion Criteria

`WarmModulePool` should remain opt-in until WM5 proves:

1. Warm-hit latency is at least 5x better than `StartupSnapshotCache` on a
   representative workload.
2. No correctness regressions in the warm-hit test suite.
3. Stale-ctx generation guard test suite passes.
4. Warm pool metrics are visible and trustworthy under load.
5. The fork changes are tagged and consumed from the remote fork (not local
   vendored paths).

`StartupSnapshotCache` remains the default until benchmarks justify changing it.

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-04-05 | meta | documented | Initial plan authored from six-agent verified audit of fork feasibility. Scope: ~103 lines deno_core fork + ~276 lines neovex-runtime. Supersedes raw-v8-warm-backend-plan.md as primary warm execution path. | document review against current fork surface, ARCHITECTURE.md, and agent audit findings | activate after Locker fork plan Phase 5 completes |
| 2026-04-05 | meta | revised | Addressed three review findings (round 1): (1) P1 BLOCKER — bootstrap `__neovexCreateContext()` captures `requestAuth`, `authIdentity`, `verifiedAuthIdentity`, `sessionId` in JS closures; stashed ctx leaks previous request's auth across warm hits. Fix: add `__neovexInvocationGeneration` guard to all host call closures (WM0). (2) P1 BLOCKER — `pending_ops.len() == 0` insufficient as warm-reset boundary; event loop tracks timers, dynamic imports, ticks, promises, UV handles beyond ops. Fix: guard on full `EventLoopPendingState::is_pending() == false`, add quiescence drain loop, discard on timeout, add negative tests (WM1/WM3). (3) P2 — plan did not state execution-model constraint. Fix: `WarmModulePool` is `CooperativeLocker`-only in first implementation; `RunToCompletion` fails fast (WM2). | review of `source.rs:251-419` (closure captures), `jsruntime.rs:3069` (EventLoopPendingState), `snapshot.rs:458` (single-entry RTC constraint) | WM0 can land independently as a safety improvement; WM1+ blocked on Locker fork Phase 5 |
| 2026-04-05 | meta | revised | Addressed three review findings (round 2): (1) P1 — generation guard only wrapped `syncHostValue`/`asyncHostValue` but `ctx.auth.getUserIdentity()` and `ctx.auth.getVerifiedIdentity()` return captured identity directly without host calls. Fix: `guardStale()` now explicitly wraps every ctx method including auth accessors. (2) P1 — `is_pending()` in the fork only checks 9 of 12 `EventLoopPendingState` fields; it skips `has_pending_ops`, `has_outstanding_immediates`, `has_pending_timers`. Fix: new `is_warm_reuse_safe()` method checks all 12 fields; `reset_request_state()` guards on that instead of `is_pending()`. (3) P2 — invocation path described a new ctx-first handler ABI that does not exist. Fix: clarified that warm path re-calls `globalThis.__neovexInvoke(request)` via `execute_script()` unchanged; `__neovexCreateContext()` is called internally by user code as today; no new ABI. | verification of `source.rs:303-308` (auth accessors bypass host ops), `jsruntime.rs:3129-3139` (`is_pending()` skips 3 fields) | plan is now implementation-ready pending activation gate |
| 2026-04-05 | meta | revised | Final feasibility pass against the live fork and the deferred raw-V8 backend plan. Adjusted the fork estimate upward to a more realistic merge budget (`120-180` production lines, `250-400` total with focused tests), documented the in-place op-driver reset and hidden-state audit as the primary WM1 friction points, and made the backend decision rule explicit: stay on the fork path unless WM1 proves `reset_request_state()` unsound or WM5 proves the warm path still misses the target badly enough to justify a second backend. Also recorded that the fork work remains useful even if warm pooling later fails the promotion gate because the quiescence/reset primitives are reusable on their own. | document review against `warm-module-pool-plan.md`, `raw-v8-warm-backend-plan.md`, `runtime/bootstrap/source.rs`, `runtime/bootstrap/snapshot.rs`, and the live `agentstation/deno_core` fork surfaces (`jsruntime.rs`, `modules/map.rs`, `exception_state.rs`, `futures_unordered_driver.rs`) | keep raw-V8 deferred; execute WM0/WM1 only after activation gate is opened |
| 2026-04-05 | meta | revised | Verified follow-up insights from an independent implementation sketch audit. Kept the additive/no-existing-functionality-loss conclusion, clarified that WM3 needs a public `JsRuntime::is_warm_reuse_safe()` wrapper rather than only the `EventLoopPendingState` method, recorded that the current `Rc<OpDriverImpl>` topology rules out treating `*self = Self::default()` as the assumed reset strategy from outside the driver, and added WM5 guidance to measure whether per-invocation `bundle.verify_integrity()` becomes the dominant remaining fixed warm-hit cost. Did **not** adopt the claimed exact `~159` production-line count because the final op-driver reset shape is still design-dependent. | document review against the live fork (`extension_set.rs`, `ops.rs`, `jsruntime.rs`, `bundle.rs`, `runtime.rs`) and the updated warm-module-pool control plane | keep exact line count flexible; carry the verified API/benchmark insights into WM1/WM5 |
| 2026-04-05 | meta | revised | Final validation pass: (1) End-to-end warm-hit trace — no showstoppers found across all 11 steps; globalThis functions survive, OpState swap works, event loop drives new ops correctly after reset, heap limit callback re-registration is safe. (2) Fork vs raw-V8 comparison — raw V8 gives no capability the fork cannot match for Neovex's workload; fork overhead on warm hit is ~1-10 microseconds (1-3%); fork fails cheap (~159 lines) while raw V8 fails expensive (months). (3) V8 module persistence safety — all 8 items verified SAFE; Evaluated is terminal, namespace is stable, Global handles are sufficient GC roots, compiled code persists. (4) Op driver resolution — deep trace of `Rc<OpDriverImpl>` sharing topology (ContextState + every OpCtx, refcount ≥ N+2) confirmed `*self = Self::default()` is impossible, BUT also confirmed **no explicit reset is needed at quiescence**. The entire driver uses interior mutability (Cell, RefCell) and is immediately reusable for new ops when `is_warm_reuse_safe()` confirms all pending state is drained. This eliminates the previously identified "primary friction point" and downgrades the op-driver risk from MEDIUM to LOW. | end-to-end invocation trace against `runtime.rs`, `state.rs`, `source.rs`; `Rc<OpDriverImpl>` topology trace against `jsrealm.rs:93`, `extension_set.rs:164`, `ops.rs:91`; V8 module status machine against `map.rs:1412-1462` | plan confirmed as implementation-ready pending activation gate |
| 2026-04-06 | meta | revised | Refined the op-driver conclusion after local verification. Confirmed the live `Rc<OpDriverImpl>` topology still rules out replacement from outside the driver, but also confirmed that warm reuse does **not** need an explicit driver reset in the expected path because the live driver is already exercised across repeated submit → drain → submit cycles by `runtime::op_driver::tests::test_driver_yield`. Corrected the earlier overstatement that an aborted driver task would auto-respawn: in the current implementation `shutdown()` is teardown-only and warm reset must not call it because it aborts the task handle without resetting `task_set`. Also kept the earlier `~159` production-line figure and `~1-10μs` fork-overhead figure out of the active control plane because they remain implementation/benchmark estimates rather than locally verified facts. | document review against `futures_unordered_driver.rs` plus focused `cargo test test_driver_yield -- --nocapture` in the live fork worktree | keep quiescent driver reuse as the WM1 assumption; treat `shutdown()` as teardown-only and add a targeted residual-state helper only if WM1 testing exposes a real survivor |
| 2026-04-06 | meta | revised | Re-reviewed the full plan against the live Neovex bootstrap/runtime surfaces. Resolved the remaining WM2/WM3 inconsistency by removing the stored `handler_fn` / `module_id` warm-hit path from the first implementation and keeping the unchanged `execute_script("globalThis.__neovexInvoke({request_json})")` dispatch contract as the only active plan. Also tightened the fork-reset contract around `OpState`: the current design is valid because Neovex only persists `RuntimeHostState` and refreshes `RuntimeCancellationState` + `SharedInvocationPermit` per invocation, but future request-local `OpState` usage (`resource_table`, `gotham_state`, `unrefed_resources`, or other generic slots) must either be explicitly cleared or make the runtime ineligible for warm reuse. | document review against `warm-module-pool-plan.md`, `runtime/bootstrap/state.rs`, `runtime/bootstrap/ops.rs`, and live fork `ops.rs` | keep first implementation on the unchanged `__neovexInvoke` ABI; treat additional request-local `OpState` as an explicit maintenance gate for warm reuse |
| 2026-04-06 | meta | revised | Verified WM1 fork changes against the live `0.395.0-locker.1` Locker fork surface. The warm pool is `CooperativeLocker`-only, so warm runtimes use `use_locker: true` — `ensure_v8_lock_held()` is NOT a no-op on this path. Added fork lineage section documenting that WM1 is applied to `0.395.0-locker.1` (or Phase 5 successor) to produce a new `0.395.0-locker.2` tag instead of repairing `0.395.0-locker.1` again. `reset_request_state(&mut self)` must ensure the V8 lock is held before any V8 access, with an explicit top-of-method `ensure_v8_lock_held()` as the clearest pattern. `is_warm_reuse_safe(&mut self)` also needs `&mut self` on the Locker path, but the requirement is outcome-based: it must keep the lock held before scope creation / V8-backed state reads, whether that happens via an explicit `ensure_v8_lock_held()` call or via helpers like `scope!`, `v8_isolate()`, and `v8_isolate_ptr()` that already perform the same check. WM1 verification contract now requires all negative tests to pass with `use_locker: true`, not just the standard path. | audit of `ensure_v8_lock_held()` at `jsruntime.rs:713`, `ManagedIsolate::Lockable` lock path at `managed_isolate.rs:132-137`, `reset_main_realm()` Locker pattern at `jsruntime.rs:1564-1566`, `scope!` macro at `jsruntime.rs:670-678`, `v8_isolate()` / `v8_isolate_ptr()` lock checks at `jsruntime.rs:1644-1651`, `EventLoopPendingState::new_from_scope()` scope requirement at `jsruntime.rs:3123-3127`, and cooperative runtime startup using `use_locker: true` at `runtime.rs:507` | WM1 implementation must test on the Locker path; fork tag versioning is explicit |
| 2026-04-07 | meta | revised | Updated plan with findings from the `reset_main_realm` root cause investigation. The true root cause of the nondeterministic SIGSEGV was eagerly dropping V8 Global handles (via `v8__Global__Reset`) during context teardown while V8 internally still references the old context's NativeContext through compiled code, feedback vectors, and microtask queue metadata. The fix: `destroy_for_reset()` skips all V8 handle cleanup and intentionally leaks ContextState/ModuleMap Rc refs from embedder slots, keeping Global handles alive until V8's GC collects the old context. All 4 previously-crashing 32-cycle tests now pass 20/20. Plan updates: (1) Activation gate dependency is now satisfied. (2) WM1 `reset_request_state()` must drain foreground tasks (within ContextScope) before microtask checkpoint, use TryCatch for the checkpoint, and loop until both queues are empty. (3) Invariant #3 expanded to include foreground drain and TryCatch requirements. (4) Invariant #10 refined: rusty_v8's ContextAnnex creates internal `Weak<Context>` handles, but these don't affect warm-pool semantics since contexts are kept alive. (5) New maintenance invariant #4: V8 Global handles must not be eagerly dropped during context teardown. (6) New known risk: V8 foreground tasks from background threads must be drained between requests. The warm pool's design decision to avoid context destruction is now validated as both a performance and correctness advantage over the `reset_main_realm` path. | 20/20 test pass confirmation across all 4 snapshot-born 32-cycle tests; heap stats analysis showing ~500KB/cycle leak without GC (stable with GC); separate-runtime control experiment proving same-isolate reset is the trigger | activation gate met; plan eligible for promotion after Locker Phase 5 |
