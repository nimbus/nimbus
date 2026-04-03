# Plan: V8 Locker Fork & Multi-Isolate Pooling

Canonical plan for forking `denoland/rusty_v8` and `denoland/deno_core` into
`agentstation/*` to merge the V8 Locker API (PR #1896), enabling multi-threaded
isolate pooling and cooperative scheduling architectures like Cloudflare Workers.

---

## Control Plan Rules

This document is the durable control plane for the Locker fork and cooperative
runtime workstream. The source of truth is:

1. the current git worktree
2. this plan's `Phase Status Ledger`, `Implementation Checkpoints`, and
   `Execution Log`
3. `/Users/jack/src/github.com/agentstation/neovex/ARCHITECTURE.md` for the
   landed EO5 worker-loop seam and shared runtime invariants
4. the referenced code and external sources called out in each phase

Do not rely on prior chat transcripts as progress state.

### Status model

- `todo`: not started; eligible when hard dependencies and gate notes are
  satisfied
- `in_progress`: actively being implemented; keep exactly one fork-plan phase in
  this state per autonomous execution run
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification has been recorded
- `deferred`: intentionally parked behind a product or benchmarking gate

### Activation gate

- Do not start **Phase 5** until the landed runtime architecture uses the EO5
  `WorkerLoop` / `WorkerLoopFactory` seam rather than a direct
  executor-to-`RuntimeBackend::invoke(...)` call.
- Phases 1-4 may proceed before cooperative scheduling once the team chooses to
  activate the fork path.
- This plan governs the Locker V8 path. It should keep future workerd and WASM
  backends in mind, but it should not try to implement those backends in the
  same phase unless explicitly promoted.

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

## Phase Status Ledger

| Phase | Status | Summary | Hard Dependencies | Gate Note |
|------|--------|---------|-------------------|-----------|
| 1 | `in_progress` | Fork `rusty_v8` and merge PR #1896 | none | fork path activated 2026-04-03; CI building |
| 2 | `todo` | Fork `deno_core` with Locker-aware `JsRuntime` | Phase 1 | include the feasibility spike before committing to the full fork |
| 3 | `todo` | Cargo dependency swap mechanism | Phase 1, Phase 2 | keep upstream path easy to restore |
| 4 | `todo` | Locker smoke tests in `neovex-runtime` | Phase 1, Phase 2, Phase 3 | smoke tests gate any cooperative implementation work |
| 5 | `todo` | Cooperative worker loop plus Locker-enabled deno runtime driver | Phase 1, Phase 2, Phase 3, Phase 4, EO5 | requires the landed EO5 worker-loop seam documented in `ARCHITECTURE.md` |
| 6 | `todo` | CI configuration for upstream vs fork paths | Phase 1, Phase 2, Phase 3, Phase 4, Phase 5 | none |
| 7 | `todo` | Upstream tracking and swap-back | Phase 1 | starts once the fork path begins |

## Implementation Checkpoints

| Phase | Checkpoint | Next Step |
|------|------------|-----------|
| 1 | fork created, PR #1896 cherry-picked, CI running (run 23960126519) | wait for CI to pass on all 5 targets, then mark Phase 1 done |
| 2 | none yet | start with the deno_core feasibility spike before the full fork |
| 3 | none yet | add the reversible `[patch.crates-io]` swap block |
| 4 | none yet | add Locker smoke tests once the forked dependencies build |
| 5 | none yet | start only after the EO5 worker-loop seam is present in the landed runtime; then keep the cooperative loop's integrated job-admission/I/O-completion model and tenant-affinity-first routing intact during implementation |
| 6 | none yet | add CI matrix coverage for upstream and fork paths |
| 7 | none yet | define the monthly rebase and swap-back workflow once the fork is live |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|------|---------|---------|--------------|-----------|
| 2026-04-03 | meta | documented | Added control-plan scaffolding and reconciled this plan with EO5 so the primary extensibility seam is now the worker-loop layer. Cooperative Locker scheduling remains a future `WorkerLoop` implementation instead of pretending it can fit behind a run-to-completion `RuntimeBackend::invoke(...)` seam. | document review against EO5 and the current fork-plan text | update Phase 2 and Phase 5 details to match the worker-loop architecture before implementation starts |
| 2026-04-03 | meta | documented | Tightened the fork plan into an implementation-grade control document: Phase 2 now has a mandatory deno_core feasibility spike and larger scope budget, Phase 5 now requires an integrated worker loop that admits new jobs and routes them via tenant-affinity-first policy, and the risk/dependency sections now consistently describe `WorkerLoopFactory` as the primary seam. | document review against EO5 and the revised fork-plan text | keep the fork plan dormant until the fork path is activated and EO5 is completed |
| 2026-04-03 | 1 | in_progress | Fork path activated. Created `agentstation/rusty_v8` fork, branch `locker-v147` from `v147.0.0`, cherry-picked 4 PR #1896 commits cleanly (no conflicts). Verified safety fixes: Lock→Enter ordering, panic safety, UnenteredIsolate !Deref, 3 compile-fail tests, Unlocker omitted. Streamlined CI to 5 release targets. Modified `build.rs` to default downloads to fork releases with `RUSTY_V8_VERSION` env var support for tag `v147.0.0-locker.1`. Set `locker-v147` as default branch. Pushed tag `v147.0.0-locker.1`, CI run 23960126519 building. | cherry-pick clean, safety audit pass, CI triggered | wait for CI green on all 5 targets to mark Phase 1 done |

## Context

### Why fork

- **rusty_v8 PR #1896** (by `max-lt`, opened 2026-01-10) is the most mature
  attempt at reintroducing the V8 Locker API to rusty_v8. It introduces
  `UnenteredIsolate`, `v8::Locker`, and `v8::Unlocker` with 298 lines of tests
  and has been deployed in production at OpenWorkers.
- **Upstream is stalled.** Core Deno maintainers (`ry`, `bartlomieju`,
  `littledivy`, `bnoordhuis`) have not reviewed PR #1896. Only `devsnek`
  provided initial feedback (2026-01-10). The `blocked` mergeable state
  indicates missing maintainer approval.
- **Neovex needs this.** The landed EO5 architecture now introduces a
  `WorkerLoopFactory` seam at the executor boundary, with the
  current deno_core model implemented as a run-to-completion worker loop.
  A Locker-enabled cooperative worker loop plugs into that seam and enables
  multiple JsRuntimes per thread with cooperative yielding at host I/O
  boundaries — the same architectural family as Cloudflare Workers.
- **The same seam should support future runtimes.** This plan covers the
  Locker-enabled V8/deno_core path. The surrounding worker-loop architecture
  should also leave room for workerd-backed or WASM-backed runtimes later,
  without making those backends a prerequisite for this fork.
- **The fork is temporary.** If/when PR #1896 merges upstream, Neovex switches
  back to crates.io dependencies. The fork is structured for easy swap-back.

### Current dependency chain

```text
neovex-runtime
  └─ deno_core 0.395.0 (crates.io)
       ├─ v8 147.0.0 (crates.io, = rusty_v8)
       └─ serde_v8 0.304.0 (crates.io)
```

Only `neovex-runtime` depends on `deno_core` directly. No `[patch]` sections
exist in the workspace `Cargo.toml` today.

### Prior art

| Project | Approach | Status |
|---------|----------|--------|
| [OpenWorkers](https://github.com/openworkers/openworkers-runtime-v8) | Fork of rusty_v8 with Locker, deployed in production. Recently **dropped deno_core** and rewrote on raw rusty_v8 after 2 years of using deno_core. | Active, by PR #1896 author |
| [unibg-seclab](https://github.com/unibg-seclab/rusty_v8_locker_API) | Academic rusty_v8 fork with Locker | Less active |
| [Cloudflare workerd](https://github.com/cloudflare/workerd) | C++ V8 Locker for cooperative scheduling. `Worker::takeAsyncLock()` + KJ event loop. Isolates pinned to threads, threads move between isolates. | Production, massive scale |
| [Convex](https://github.com/get-convex/convex-backend) | Permit suspend/resume (no Locker, 1 isolate/thread) | Production |
| [Supabase edge-runtime](https://github.com/supabase/edge-runtime) | 1 isolate/thread, deno_core | Production |

### Key references

- [rusty_v8 issue #643](https://github.com/denoland/rusty_v8/issues/643) —
  Locker reintroduction tracking (open since 2021)
- [rusty_v8 PR #1896](https://github.com/denoland/rusty_v8/pull/1896) — V8
  Locker API, rebased onto v146.4.0, head SHA `751c5c08`
- [rusty_v8 PR #272](https://github.com/denoland/rusty_v8/pull/272) — Original
  Locker removal by Ryan Dahl (2020), rationale: Deno's 1-thread-per-isolate
  model made Locker unnecessary overhead
- [deno_core issue #708](https://github.com/denoland/deno_core/issues/708) —
  Multiple JsRuntime on one thread segfaults (Locker would fix)
- [max-lt cooperative multitasking POC](https://gist.github.com/max-lt/2201664fbb7d6fef432da4f91b2eb004)
- [OpenWorkers rusty_v8 fork](https://github.com/openworkers/rusty-v8) —
  Production Locker fork

---

## Cross-Project Comparison

Detailed comparison of our proposed API against the two production reference
implementations: Cloudflare workerd (C++, massive-scale production) and
OpenWorkers (Rust, by the PR #1896 author).

### Reference implementations at a glance

| Dimension | workerd | OpenWorkers | Neovex (proposed) |
|-----------|---------|-------------|-------------------|
| Language | C++ | Rust (raw rusty_v8) | Rust (deno_core + rusty_v8 fork) |
| V8 binding | Direct V8 C++ API | rusty_v8 fork with Locker | rusty_v8 fork via deno_core fork |
| Isolate creation | `v8::Isolate::New()` | `UnenteredIsolate` (pooled) or `OwnedIsolate` (worker) | `UnenteredIsolate` wrapped in `JsRuntime` with `use_locker: true` |
| Lock primitive | `v8::Locker lock(isolate)` (C++ RAII) | `v8::Locker::new(&mut isolate)` (Rust RAII) | Same as OpenWorkers, wrapped in deno_core |
| Isolate↔thread | N:M (any thread can lock any isolate) | M:1 (thread-local pools, pinned) | M:1 (thread-local, deno_core `!Send`) |
| Scheduling | Per-isolate FIFO queue (`AsyncWaiter` linked list) | Per-isolate FIFO queue (`AsyncWaiter`) | Per-thread FIFO run queue |
| Isolate lifecycle | Long-lived per-script | Pooled per-thread with LRU eviction and owner tags | Pooled per-thread |
| Deferred destruction | `workerDestructionQueue` (worker.c++:619) | `DeferredDestructionQueue` | `DeferredV8Handle` queue in `CooperativeScheduler`, processed on lock acquisition (Phase 5) |
| Yield mechanism | RAII scope drop → `AsyncWaiter` destructor signals next waiter | RAII lock drop → re-acquire per poll tick | ~~`yield_v8_lock()` / `resume_v8_lock()`~~ → RAII (revised below) |

### Key findings from reference analysis

#### 1. Neither reference uses an explicit yield/resume API

Both workerd and OpenWorkers use **RAII-scoped lock guards**, not explicit
yield/resume methods:

- **workerd**: `Worker::Lock` constructor acquires (via `api->lock(stackScope)`),
  destructor releases. The `AsyncWaiter` destructor signals the next thread in
  the FIFO queue (`worker.c++:2789`).
- **OpenWorkers**: `v8::Locker` drop releases the lock. Re-acquired per event
  loop poll tick in `await_event_loop()`.

Our original `yield_v8_lock()` / `resume_v8_lock()` API was a custom invention
without precedent in either reference. **Revised:** adopt RAII-scoped locking.
The cooperative scheduler creates a lock scope, drives one poll tick, drops the
scope, then creates a new scope for the next runtime. See Phase 2 and 5 updates
below.

#### 2. Both references have deferred destruction queues

V8 handles (Global, Persistent) dropped outside the V8 lock cannot be destroyed
immediately. Both references solve this:

- **workerd**: `workerDestructionQueue` (`worker.c++:619,714`) — a
  `MutexGuarded<BatchQueue>` of worker impls. Processed on lock acquisition.
- **OpenWorkers**: `DeferredDestructionQueue` — Global handles dropped without
  the lock are queued and batch-destroyed on next lock acquisition. Paired with
  `Arc<AtomicI64>` memory delta tracking.

**This is a gap in our plan.** When the cooperative scheduler drops the Locker
between runtimes, any V8 handles that need destruction must be deferred. Added
to Phase 5.

#### 3. FIFO fairness, not round-robin

Both references use **per-isolate FIFO queues** (workerd's `AsyncWaiter` linked
list, OpenWorkers' `AsyncWaiter`). In our M:1 model, the equivalent is a
per-thread FIFO run queue where yielding runtimes go to the back.

Round-robin was our original proposal but FIFO is better:
- A runtime that just completed I/O and has results ready gets the same priority
  as one that just started — no starvation
- Matches both reference implementations
- Simpler to reason about than priority-based scheduling

**Revised:** Phase 5 uses FIFO run queue instead of round-robin index.

#### 4. Long-lived isolates per script (workerd) vs pooled per request

- **workerd** (`worker.h:326-331`): "each Script gets a V8 Isolate...multiple
  workers sharing the same script cannot execute concurrently." Isolates are
  long-lived, shared across requests to the same script. Contexts (global
  objects) provide per-zone separation within the isolate.
- **OpenWorkers**: Pooled per-thread with owner tags (`owner_id`) and LRU
  eviction. Warm context reuse achieves <10μs vs 3-5ms cold start.

For Neovex's multi-tenant model (each tenant has its own functions), the
OpenWorkers pattern (owner-tagged pools with LRU) is a better fit than workerd's
per-script model. A tenant's isolate can be reused across invocations of
different functions within that tenant.

#### 5. OpenWorkers dropped deno_core after 2 years

This is a significant data point. OpenWorkers initially used deno_core (their
`openworkers-runtime-deno` variant still exists) but built
`openworkers-runtime-v8` on raw rusty_v8 for the Locker-based pooling
architecture. Reasons likely include:

- deno_core's `JsRuntime` creates `OwnedIsolate` internally, making Locker
  integration awkward
- deno_core's op system is heavier than needed for a Worker-style runtime
- Raw rusty_v8 gives full control over isolate lifecycle

**Implication for our plan:** Our Phase 2 (Locker-aware deno_core fork) is the
riskiest phase. If the deno_core integration proves too fragile, we have the
same escape hatch OpenWorkers took. The `WorkerLoopFactory` seam from EO5 keeps
that swap contained at the runtime/worker-loop boundary instead of forcing a
full executor rewrite.

#### 6. workerd is cross-thread; we can't be (and don't need to be)

workerd's `takeAsyncLockImpl` (`worker.c++:2684-2724`) allows any thread to
queue for any isolate. This works because C++ V8 bindings don't have Rust's
`!Send` constraint. The FIFO queue ensures fairness across threads.

We can't do this because deno_core's `JsRuntime` is `!Send`. But OpenWorkers
proves that thread-local pools work at production scale. Thread-level load
balancing happens at the request routing layer (which isolate pool/thread
receives a new invocation), not the isolate layer.

---

## Two Execution Models

This plan targets two distinct execution models behind EO5's
`WorkerLoopFactory` seam. The executor chooses a worker-loop strategy; runtime-
specific helpers live below that seam.

### Model 1: M:1 Cooperative (primary target)

**Multiple JsRuntimes per thread, cooperative switching via Locker. JsRuntimes
never cross thread boundaries.**

This is the Cloudflare Workers model. workerd doesn't move isolates between
threads — the **thread moves between isolates**. Each isolate is pinned to its
creation thread. When one isolate hits I/O, the thread drops the V8 lock and
picks up another isolate from its local run queue.

```text
Thread 0                    Thread 1
┌─────────────────────┐     ┌─────────────────────┐
│ JsRuntime A (active)│     │ JsRuntime D (active)│
│ JsRuntime B (parked)│     │ JsRuntime E (parked)│
│ JsRuntime C (parked)│     │ JsRuntime F (parked)│
└─────────────────────┘     └─────────────────────┘

Thread 0 loop:
  Lock A → poll_event_loop → host I/O hit → drop Lock
  Lock B → poll_event_loop → completed → drop Lock
  Lock A → poll_event_loop → resume after I/O → completed → drop Lock
  Lock C → ...
```

**Why this works with `!Send` JsRuntime:** JsRuntimes stay on their creation
thread. `Rc<RefCell<...>>` is fine for single-threaded access. The Locker
ensures V8's internal state is correct when switching between isolates on the
same thread (fixes deno_core issue #708).

**What it requires from the deno_core fork:**
- JsRuntime creation via `UnenteredIsolate` + `Locker` (instead of `OwnedIsolate`)
- `poll_event_loop` for single-tick driving (already exists at
  `jsruntime.rs:2156`)
- RAII `V8LockGuard` for scoped lock acquisition/release (matching the RAII
  pattern used by both workerd's `Worker::Lock` and OpenWorkers' `v8::Locker`)

**deno_core fork scope:** treat this as a ~300-500 line fork **after** the
mandatory Phase 2 feasibility spike passes. The spike is the gate that proves
deno_core's constructor/event-loop ownership can tolerate sequential Locker
entry/exit with multiple `JsRuntime`s on one thread. No Rc→Arc changes are in
scope for this model, and public op signatures stay unchanged, but the fork is
still deeper than simple plumbing. See [Cross-Project Comparison](#cross-project-comparison)
and Phase 2 for the spike-first workflow.

### Model 2: N:M Cross-Thread (deferred stretch goal)

**JsRuntimes move between threads via work stealing.**

This would require making `JsRuntime` `Send` by replacing `Rc<RefCell<...>>`
with `Arc<Mutex<...>>` throughout deno_core. See [Appendix A](#appendix-a-rcrefc
ell-to-arcmutex-analysis) for the full analysis.

**Assessment: not recommended at this time.** The Rc→Arc patch is:
- **228 `Rc<` occurrences** across deno_core 0.395.0
- **Breaks public API:** `OpCtx::state: Rc<RefCell<OpState>>` is the signature
  for all 19 of neovex-runtime's async ops
- **35 Rc changes** in the inspector subsystem alone
- **Mutex overhead** on every single-threaded op dispatch path
- **Deadlock risk** replacing RefCell panics with mutex deadlocks
- **Maintenance nightmare** — every upstream deno_core update requires rebasing
  228+ changes

The M:1 model achieves Workers-style architecture without any of this. The N:M
model is only needed if benchmarks show that load imbalance across threads (one
thread's runtimes all blocked while another thread is idle) is a real problem. In
practice, workerd and other production systems handle this at the request routing
layer, not the isolate layer.

**Gate:** evaluate N:M only after M:1 benchmarks show thread-level load
imbalance is a measurable bottleneck.

---

## Phase 1: Fork rusty_v8 and Merge PR #1896

**Goal:** Create `github.com/agentstation/rusty_v8` with the Locker API merged
and building against the version pinned by our deno_core dependency.

### Steps

1. **Fork `denoland/rusty_v8`** into `github.com/agentstation/rusty_v8`.

2. **Create branch `locker-v147`** from the tag that matches our pinned version
   (`v147.0.0`, since deno_core 0.395.0 depends on `v8 = "147.0.0"`).

3. **Cherry-pick or rebase PR #1896** onto `locker-v147`.
   - PR #1896 head is on v146.4.0. The delta to v147.0.0 should be small, but
     scope conflicts are possible in `src/isolate.rs` and `src/scope.rs`.
   - If cherry-pick conflicts are non-trivial, rebase the 4 PR commits
     individually, resolving conflicts per-commit.

4. **Verify the safety fixes from PR #1896 are included:**
   - Lock → Enter ordering (commit `751c5c08`, fixes `entry_stack_` race)
   - Panic safety for `Locker::new()`
   - `UnenteredIsolate` does NOT deref into `Isolate` (only `Locker` does)
   - Compile-fail tests for misuse

5. **Omit `v8::Unlocker` initially.** Reviewer `devsnek` flagged Unlocker as "a
   huge can of worms wrt aliasing." We can add it in a follow-up once we design
   the Rust safety model. For Phase 1, cooperative yielding will use
   Lock-drop/Lock-reacquire rather than Unlocker.

6. **Run the upstream test suite plus PR #1896's `tests/test_locker.rs`:**
   ```bash
   cargo test --features test_locker
   ```

7. **Tag release** as `v147.0.0-locker.1` (semver pre-release, clearly
   distinguishable from upstream).

### Output

- `github.com/agentstation/rusty_v8` with branch `locker-v147` and tag
  `v147.0.0-locker.1`
- CI passing, upstream tests + Locker tests green

---

## Phase 2: Fork deno_core with Locker-Aware JsRuntime

**Goal:** Create `github.com/agentstation/deno_core` that depends on the forked
rusty_v8 and adds targeted Locker integration to JsRuntime — enabling multiple
JsRuntimes per thread with cooperative lock/unlock at event loop boundaries.

**Estimated scope:** ~300-500 LOC after a mandatory feasibility spike.

### Mandatory feasibility spike

Before committing to the full deno_core fork, do a narrow spike against the
forked dependencies that proves the architecture is actually viable:

1. Create a minimal `JsRuntime` through the existing upstream path.
2. Add the smallest possible Locker-aware construction path using
   `UnenteredIsolate`.
3. Create two `JsRuntime` instances on the same OS thread with Locker enabled.
4. Acquire/release lock scopes sequentially and run trivial JS in both.
5. Verify that:
   - there are no `entry_stack_`-style crashes
   - deno_core does not assume one always-entered isolate per thread in hidden
     places
   - the lock scope can be released and reacquired cleanly around event-loop
     driving

**Gate:** if this spike shows deno_core internals are more entangled than
expected, widen the fork scope immediately or reevaluate the raw-rusty_v8
OpenWorkers path before proceeding.

### Design principles

- **Minimal diff.** Only change what's needed for Locker-aware isolate creation
  and event loop driving. Do not touch Rc<RefCell<...>>, op signatures, or the
  inspector.
- **Backward compatible.** Existing code that creates JsRuntime without Locker
  options works identically to upstream. Locker is opt-in.
- **No public API changes to ops.** Ops still receive `Rc<RefCell<OpState>>` for
  async and `&mut OpState` for sync. The Locker integration is below the op
  layer.

### Changes to deno_core (~300-500 lines after the spike)

#### 2a. Locker-aware isolate creation (`runtime/jsruntime.rs`)

Add an option to `RuntimeOptions`:

```rust
/// If set, the JsRuntime creates a `v8::UnenteredIsolate` and acquires
/// a `v8::Locker` before entering, instead of using `v8::OwnedIsolate`
/// which auto-enters. This enables multiple JsRuntimes on the same thread
/// by locking/unlocking isolates cooperatively.
pub use_locker: bool,
```

When `use_locker: true`:
- Create `v8::UnenteredIsolate::new(params)` instead of
  `v8::Isolate::new(params)` (which returns `OwnedIsolate`)
- Store the `UnenteredIsolate` on the JsRuntime (new field)
- Acquire `v8::Locker` when entering the event loop
- Drop the Locker when exiting

When `use_locker: false` (default): existing `OwnedIsolate` path, identical to
upstream.

#### 2b. Lock/unlock around event loop (`runtime/jsruntime.rs`)

Modify `poll_event_loop` and `with_event_loop_promise`:

```rust
// Pseudocode — actual implementation depends on deno_core internals
pub async fn with_event_loop_promise<T>(&mut self, ...) -> ... {
    // Existing: just drives the event loop to completion
    // New with Locker: acquire lock at start, hold through completion
    // (single-invocation driving, same as today but Locker-aware)
}

pub fn poll_event_loop(&mut self, cx: &mut Context, options: ...) -> Poll<...> {
    // Existing: single-tick poll
    // New with Locker: caller is responsible for lock/unlock around calls
    // This method assumes the Locker is held by the caller
}
```

The key insight: for M:1 cooperative scheduling, neovex-runtime's cooperative
scheduler calls `poll_event_loop` in a loop, **dropping and re-acquiring the
Locker between runtimes**. The scheduler, not deno_core, decides when to switch.

#### 2c. RAII lock scope API (new public methods)

```rust
impl JsRuntime {
    /// Acquire the V8 Locker for this runtime's isolate, returning an RAII
    /// guard. While the guard is held, the runtime can drive its event loop.
    /// Dropping the guard releases the V8 lock, allowing other JsRuntimes
    /// on the same thread to acquire their Lockers.
    ///
    /// Only available when `use_locker: true`.
    ///
    /// # Panics
    /// Panics if `use_locker` was false at construction time, or if the
    /// lock is already held (recursive locking).
    pub fn acquire_v8_lock(&mut self) -> V8LockGuard<'_> { ... }

    /// Single-tick poll of the event loop. Requires the V8 lock to be held
    /// (caller must have a live `V8LockGuard`). Returns whether the
    /// invocation completed, is pending (more JS work), or yielded (host
    /// I/O initiated).
    ///
    /// This is the cooperative scheduling primitive: the caller polls one
    /// tick, then decides whether to continue or drop the lock and switch
    /// to another runtime.
    pub fn poll_event_loop_tick(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Error>> { ... }
}

/// RAII guard that holds the V8 Locker. Dropping releases the lock.
/// Matches the RAII pattern used by both workerd (Worker::Lock destructor)
/// and OpenWorkers (v8::Locker drop).
pub struct V8LockGuard<'a> { ... }
```

This matches the RAII scoping pattern used by both reference implementations.
The cooperative scheduler creates a lock guard, polls one tick, then drops the
guard before switching to the next runtime. No explicit yield/resume — the
lock lifetime is controlled by the RAII scope.

#### 2d. Dependency swap

Patch `Cargo.toml` and `serde_v8/Cargo.toml` to point `v8` at the forked
rusty_v8:
```toml
[dependencies]
v8 = { git = "https://github.com/agentstation/rusty_v8", tag = "v147.0.0-locker.1" }
```

### What we do NOT change

- **`Rc<RefCell<...>>` types** — untouched. JsRuntime stays `!Send`.
- **Op signatures** — `Rc<RefCell<OpState>>` for async ops, `&mut OpState` for
  sync ops. All 27 of neovex-runtime's ops work unchanged.
- **Inspector** — untouched (35 Rc types stay as-is).
- **FuturesUnorderedDriver** — untouched.
- **ManuallyDropRc** — untouched.
- **`with_event_loop_promise`** — untouched. The existing 1:1 backend can
  continue using it. The cooperative scheduler uses `poll_event_loop_tick`
  instead.

### Verification

```bash
cargo test -p deno_core
cargo test -p serde_v8
# Phase 2 spike: two Locker-enabled runtimes on one thread
cargo test -p deno_core locker_same_thread_spike -- --nocapture
# New: multiple JsRuntimes on same thread with Locker
cargo test -p deno_core multi_runtime_locker -- --nocapture
# New: RAII lock guard acquire/release cycle
cargo test -p deno_core v8_lock_guard_raii -- --nocapture
```

### Output

- `github.com/agentstation/deno_core` with branch `locker-v0.395` and tag
  `0.395.0-locker.1`
- ~300-500 lines changed once the feasibility spike passes (RuntimeOptions,
  isolate creation, RAII lock scope API, and the surrounding constructor/event-
  loop seams they actually touch)
- CI passing, all upstream tests green, new Locker tests green

---

## Phase 3: Cargo Dependency Swap Mechanism

**Goal:** Enable Neovex to switch between upstream crates.io deno_core and the
Locker-enabled fork with a single `[patch]` toggle.

### Design

Use Cargo's `[patch.crates-io]` in the workspace root `Cargo.toml`. This is the
idiomatic Cargo mechanism for replacing registry dependencies with git forks —
no workspace code changes required.

### Implementation

Add a commented-out `[patch]` section at the bottom of the root `Cargo.toml`:

```toml
# ── V8 Locker fork ──────────────────────────────────────────────────
# Uncomment the block below to switch from upstream crates.io deno_core
# to agentstation's fork with V8 Locker API support (PR #1896).
# To switch back to upstream: comment out the [patch] block and run
# `cargo update -p deno_core -p v8 -p serde_v8`.
#
# [patch.crates-io]
# deno_core = { git = "https://github.com/agentstation/deno_core", tag = "0.395.0-locker.1" }
# v8 = { git = "https://github.com/agentstation/rusty_v8", tag = "v147.0.0-locker.1" }
# serde_v8 = { git = "https://github.com/agentstation/deno_core", tag = "0.395.0-locker.1" }
```

### Why this approach

- **`[patch.crates-io]`** is Cargo's blessed mechanism for this exact use case.
  It transparently replaces the crates.io version throughout the entire
  dependency graph — no `workspace.dependencies` changes, no feature flags, no
  conditional compilation.
- **Commented-out by default** means `cargo build` on a fresh clone uses
  upstream. Developers opt in explicitly.
- **Swap back** is `comment out + cargo update` — fully reversible.
- **No Cargo features for fork selection.** Features are for runtime behavior,
  not dependency source. Using features here would be fighting Cargo's model.

### Verification

```bash
# With patch uncommented:
cargo check -p neovex-runtime  # Should resolve to forked deno_core
cargo tree -p neovex-runtime -i v8  # Should show agentstation/rusty_v8 source

# With patch commented out:
cargo update -p deno_core -p v8 -p serde_v8
cargo check -p neovex-runtime  # Should resolve to crates.io
```

---

## Phase 4: Locker Smoke Tests in neovex-runtime

**Goal:** Validate that the Locker API works correctly within Neovex's runtime
crate before building the cooperative scheduler.

### Tests (`crates/neovex-runtime/tests/locker_smoke.rs`)

```rust
//! Smoke tests for V8 Locker API via forked rusty_v8.
//! Only compiles when the [patch.crates-io] fork is active.

#[test]
fn unentered_isolate_is_send() {
    // Create UnenteredIsolate, send across threads, lock, execute JS
}

#[test]
fn locker_provides_isolate_access() {
    // Lock, create HandleScope, eval simple JS, drop Locker
}

#[test]
fn sequential_lock_unlock_across_threads() {
    // Thread A locks, executes, drops lock; Thread B locks same isolate
}

#[test]
fn concurrent_lock_contention() {
    // N threads contend on Arc<Mutex<UnenteredIsolate>>, each executes JS
}

#[test]
fn isolate_state_preserved_across_lock_cycles() {
    // Set global in lock 1, verify it persists in lock 2
}

#[test]
fn multiple_jsruntimes_same_thread_with_locker() {
    // Create 3 JsRuntimes with use_locker: true on one thread
    // Execute JS in each sequentially (lock A, run, unlock, lock B, run, ...)
    // Verify no segfaults (this is the deno_core issue #708 fix)
}

#[test]
fn jsruntime_raii_lock_scope() {
    // Create 2 JsRuntimes with use_locker: true on one thread
    // acquire_v8_lock on A, poll one tick, drop guard
    // acquire_v8_lock on B, poll one tick, drop guard
    // acquire_v8_lock on A again, verify state preserved
}

#[test]
fn jsruntime_recursive_lock_panics() {
    // Create JsRuntime with use_locker: true
    // acquire_v8_lock, then acquire_v8_lock again → should panic
    // (matches workerd's "Isolate lock taken recursively" assertion)
}
```

### Compilation guard

Tests that use `UnenteredIsolate` or `use_locker: true` fail to compile on
upstream deno_core (the types/options don't exist). This is acceptable since the
fork is opt-in via `[patch]`. No `#[cfg]` feature flag needed.

### Verification

```bash
cargo test -p neovex-runtime locker_smoke -- --nocapture
```

---

## Phase 5: M:1 Cooperative Worker Loop & Locker Deno Driver

**Goal:** Implement the Workers-style execution model — N JsRuntimes across M
threads (N >> M), with per-thread cooperative scheduling via Locker lock/unlock
cycles.

### Prerequisites

- **EO5 from the execution ownership hardening plan must be done first.** EO5
  introduces the `WorkerLoopFactory` seam, the current
  `RunToCompletionWorkerLoop`, worker/permit decoupling, and
  `SharedInvocationPermit` with suspend/resume.
- **Phases 1-4 of this plan** (forked rusty_v8 + deno_core with Locker).

### Worker-loop hierarchy

The primary runtime seam is the **worker loop**, not a per-invocation backend:

```text
WorkerLoopFactory
├── RunToCompletionWorkerLoopFactory          (EO5 / current deno_core path)
│   └── RunToCompletionWorkerLoop
│       └── DenoRuntimeBackend::invoke(...)
│
└── CooperativeWorkerLoopFactory             (this phase)
    └── CooperativeWorkerLoop
        ├── FIFO runnable / parked scheduling
        ├── thread routing + warm-runtime pools
        └── LockerDenoRuntimeDriver
            ├── create/reuse Locker-enabled JsRuntime
            ├── acquire_v8_lock()
            ├── poll_event_loop_tick()
            └── deferred destruction processing
```

`RuntimeBackend::invoke(...)` remains useful for the current run-to-completion
loop, but it is not the right abstraction for cooperative scheduling.

### Architecture

```text
┌─────────────────────────────────────────────────────────────────────┐
│                          Executor                                   │
│                                                                     │
│  M worker threads, each running a CooperativeWorkerLoop             │
│                                                                     │
│  ┌───────────────────── Thread 0 ──────────────────────┐            │
│  │                                                     │            │
│  │  Per-Thread Cooperative Scheduler                   │            │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐             │            │
│  │  │Runtime A│  │Runtime B│  │Runtime C│  ... (pool)  │            │
│  │  │ parked  │  │ ACTIVE  │  │ parked  │             │            │
│  │  └─────────┘  └─────────┘  └─────────┘             │            │
│  │                    ▲                                 │            │
│  │                    │ Locker held                     │            │
│  │                                                     │            │
│  │  Run queue: [A(waiting on I/O), C(ready)]           │            │
│  │                                                     │            │
│  │  Loop:                                              │            │
│  │    1. Pick next runnable runtime from queue          │            │
│  │    2. Locker::new(&mut runtime) → acquire V8 lock   │            │
│  │    3. poll_event_loop (single tick)                  │            │
│  │    4. If yielded (host I/O): drop Locker, enqueue   │            │
│  │    5. If completed: drop Locker, return result       │            │
│  │    6. If pending (more JS work): continue polling    │            │
│  │    7. Goto 1                                        │            │
│  └─────────────────────────────────────────────────────┘            │
│                                                                     │
│  ┌───────────────────── Thread 1 ──────────────────────┐            │
│  │  (same structure)                                   │            │
│  └─────────────────────────────────────────────────────┘            │
│                                                                     │
│  Invocation permit semaphore: controls total active across all      │
│  threads. A parked runtime (waiting on I/O) suspends its permit.    │
└─────────────────────────────────────────────────────────────────────┘
```

### Per-thread cooperative scheduler design

Each worker thread runs a cooperative scheduler that manages multiple
JsRuntimes. This is the core of the M:1 model.

Design aligned with both reference implementations:
- **FIFO run queue** (not round-robin) — matches workerd's `AsyncWaiter`
  linked list and OpenWorkers' `AsyncWaiter` FIFO
- **RAII lock scoping** — matches workerd's `Worker::Lock` destructor and
  OpenWorkers' `v8::Locker` drop
- **Deferred destruction queue** — matches workerd's
  `workerDestructionQueue` and OpenWorkers' `DeferredDestructionQueue`

#### Data structures

```rust
/// State of a single JsRuntime managed by the cooperative scheduler.
enum RuntimeSlot {
    /// Ready to execute JS. Locker can be acquired.
    Runnable {
        runtime: JsRuntime,      // use_locker: true
        permit: InvocationPermit,
    },
    /// Blocked on host I/O. V8 lock released, permit suspended.
    /// The oneshot receiver resolves when the host call completes.
    ParkedOnIo {
        runtime: JsRuntime,
        waker: oneshot::Receiver<HostCallResult>,
    },
    /// Invocation completed. Runtime can be reused or dropped.
    Completed(JsRuntime),
}

/// Per-thread cooperative scheduler.
struct CooperativeScheduler {
    /// FIFO run queue — runtimes ready to execute.
    /// Yielding runtimes go to the back. Newly runnable (I/O complete)
    /// runtimes go to the back. Front gets the next lock scope.
    /// Matches workerd's AsyncWaiter linked list ordering.
    run_queue: VecDeque<RuntimeSlot>,
    /// Runtimes parked on I/O — not in the run queue.
    parked: Vec<RuntimeSlot>,
    /// Deferred V8 handle destruction queue. Handles dropped while no
    /// Locker is held are queued here and batch-destroyed on next lock
    /// acquisition. Matches workerd's workerDestructionQueue and
    /// OpenWorkers' DeferredDestructionQueue.
    deferred_destruction: Vec<DeferredV8Handle>,
}
```

#### Deferred destruction queue

When the cooperative scheduler drops the V8 lock between runtimes, any V8
`Global` handles that need to be destroyed cannot be destroyed immediately
(V8 requires the lock to be held for handle operations). Both reference
implementations solve this:

- **workerd** (`worker.c++:618-625`): `workerDestructionQueue` is a
  `MutexGuarded<BatchQueue>`. On lock acquisition, queued worker impls are
  destroyed: `disposeContext(kj::mv(c))`.
- **OpenWorkers**: `DeferredDestructionQueue` queues Global handles dropped
  without the lock. Processed atomically on next lock acquisition. Paired
  with `Arc<AtomicI64>` for memory delta tracking.

Our implementation:

```rust
/// V8 handle that must be destroyed while the Locker is held.
struct DeferredV8Handle {
    handle: v8::Global<v8::Value>,
    runtime_id: usize,  // which runtime owns this handle
}

impl CooperativeScheduler {
    /// Called immediately after acquiring the V8 lock for a runtime.
    /// Destroys any deferred handles belonging to this runtime.
    /// Matches workerd worker.c++:618-625 pattern.
    fn process_deferred_destruction(&mut self, runtime_id: usize) {
        self.deferred_destruction.retain(|h| {
            if h.runtime_id == runtime_id {
                drop(h.handle);  // V8 lock is held, safe to destroy
                false
            } else {
                true
            }
        });
    }
}
```

#### Scheduling loop (integrated worker-loop pseudocode)

```rust
impl CooperativeScheduler {
    fn run(
        &mut self,
        jobs: &RuntimeWorkerJobReceiver,
        shutdown: &CancellationToken,
    ) {
        loop {
            if shutdown.is_cancelled() {
                break;
            }

            // Admit any newly assigned jobs before scheduling.
            while let Some(job) = self.try_recv_job(jobs) {
                self.admit_new_invocation(job);
            }

            // Check for I/O completions: move parked -> back of run_queue.
            self.check_io_completions();

            if let Some(slot) = self.run_queue.pop_front() {
                // RAII lock scope: acquire V8 lock, drive one tick, drop lock.
                {
                    let _guard = slot.runtime.acquire_v8_lock();

                    // Process deferred destruction (workerd pattern).
                    self.process_deferred_destruction(slot.runtime_id);

                    match slot.runtime.poll_event_loop_tick(cx) {
                        Poll::Ready(Ok(())) => {
                            self.complete(slot);
                        }
                        Poll::Pending if slot.runtime.has_pending_host_io() => {
                            slot.permit.suspend();
                            self.park(slot.to_parked());
                        }
                        Poll::Pending => {
                            self.run_queue.push_back(slot);
                        }
                    }
                }
                continue;
            }

            // No runnable work right now. Block for the next event:
            // a new job, an I/O completion, or shutdown.
            match self.block_until_any_event(jobs, shutdown) {
                SchedulerEvent::Job(job) => self.admit_new_invocation(job),
                SchedulerEvent::IoCompletion(completion) => {
                    self.handle_io_completion(completion);
                }
                SchedulerEvent::Shutdown | SchedulerEvent::ChannelClosed => break,
            }
        }
    }
}
```

#### Host I/O integration

When an async op (e.g., `op_neovex_ctx_db_get`) starts a host bridge call:

1. The op future is registered with deno_core's `FuturesUnorderedDriver`
2. On the next `poll_event_loop_tick`, the runtime yields `Poll::Pending`
3. The scheduler detects pending host I/O
4. The RAII `V8LockGuard` drops → V8 lock released (matches workerd/OpenWorkers)
5. The permit suspends (EO5)
6. The runtime moves to the `parked` set
7. The scheduler takes the next FIFO entry and acquires its V8 lock
8. When the host call completes (on a different Tokio task), it signals the
   scheduler via a waker/channel
9. The scheduler moves the runtime back to the FIFO run queue (at the back)
10. The permit resumes
11. When it reaches the FIFO front, the scheduler acquires its V8 lock and
    polls the next tick — the completed op result is available

The key convergence point is `op_neovex_async_host_call` (runtime.rs:1251) —
all async host ops go through this single function, which is where the permit
suspend/resume from EO5 already fires. The cooperative scheduler extends this
with V8 lock scope management via RAII guards.

#### Thread routing strategy

New invocations need an explicit routing policy before they ever reach a
specific cooperative worker loop. The default policy should be:

1. **Affinity first:** if a worker thread has a warm idle runtime whose
   affinity key matches the invocation, send the job there.
2. **Least-loaded fallback:** otherwise route to the worker whose
   `runnable + parked + queued_admissions` load is smallest.
3. **Stable tie-break:** if multiple workers tie, pick the least-recently-
   assigned worker (or round-robin among equals only).

The affinity key must be configurable over time:

```rust
enum RuntimeAffinityKey {
    Tenant(TenantId),                  // default in early phases
    Function(TenantId, FunctionName),  // future per-function isolation
    Script(RuntimeBundleId),           // workerd-style script pinning
}
```

Start with tenant affinity as the default because Neovex is multi-tenant and
OpenWorkers-style warm reuse is the closest production analog. But do not bake
tenant-only routing into the architecture; future per-function or per-script
policies need the same router seam.

### Runtime lifecycle

```text
New invocation arrives at thread
  │
  ▼
Create JsRuntime (use_locker: true) or reuse from pool
  │
  ▼
Acquire invocation permit (semaphore)
  │
  ▼
Add to FIFO run queue (back)
  │
  ▼
┌─► Scheduler takes from FIFO front
│     │
│     ▼
│   { let _guard = runtime.acquire_v8_lock() }  ← RAII scope
│   process_deferred_destruction()               ← workerd pattern
│   poll_event_loop_tick()
│     │
│     ├─ Poll::Ready → Completed → return result, release permit
│     │                             reclaim runtime to pool
│     │
│     ├─ Poll::Pending (host I/O)
│     │     │
│     │     ▼
│     │   _guard drops → V8 lock released        ← RAII release
│     │   suspend permit
│     │   move to parked set
│     │     │
│     │     ▼
│     │   (scheduler runs next FIFO entry)
│     │     │
│     │     ▼
│     │   Host I/O completes → resume permit
│     │   move to FIFO back
│     │     │
│     └─ Poll::Pending (more JS work)
│           │
│           ▼
│         _guard drops → V8 lock released        ← RAII release
│         push to FIFO back (yield to others)
│           │
└───────────┘
```

### Runtime pool (warm isolates)

Creating a JsRuntime is expensive (~3-5ms for V8 isolate creation + snapshot
loading). The cooperative scheduler should maintain a pool of warm runtimes.

Design aligned with OpenWorkers' `ThreadLocalPool` + `TaggedIsolate` pattern:

- **Pool per thread** (runtimes can't cross threads — `!Send`)
- **Tenant-tagged** — each runtime is tagged with its tenant ID (matches
  OpenWorkers' `owner_id` tagging). A tenant's runtime can be reused across
  invocations of different functions within that tenant.
- **LRU eviction** — when pool is full and a new tenant needs a runtime, evict
  the least-recently-used idle runtime (matches OpenWorkers' LRU strategy)
- **Pre-warmed** with the V8 snapshot and common module state
- **Reset between invocations** (clear globals, reset OpState)
- **Bounded** by `max_runtimes_per_thread` configuration
- **Lazy growth** — start with 1, grow on demand up to bound

OpenWorkers reports <10μs warm start with pooled isolates vs 3-5ms cold start.

**Comparison with workerd:** workerd uses one long-lived isolate per script
(not pooled). This works for Cloudflare's model where scripts are relatively
static. Neovex's multi-tenant model is closer to OpenWorkers — many tenants
with dynamic function sets benefit from pooling with LRU eviction.

### Future runtimes on the same seam

This worker-loop architecture should support more than one runtime family:

- **Current deno_core path:** `RunToCompletionWorkerLoopFactory` +
  `DenoRuntimeBackend`
- **Locker-enabled deno_core path:** `CooperativeWorkerLoopFactory` +
  `LockerDenoRuntimeDriver`
- **Future raw-rusty_v8 or workerd-like path:** `CooperativeWorkerLoopFactory`
  with a different runtime driver below it
- **Future WASM path (for example Wasmtime):**
  `RunToCompletionWorkerLoopFactory` with a `WasmtimeRuntimeBackend` unless
  benchmarks justify a different loop model

The executor should switch loop strategies, not assume all runtimes fit one
per-invocation `invoke(...)` interface.

### Files to create/modify

- `crates/neovex-runtime/src/worker_loop/cooperative.rs` (new) —
  `CooperativeWorkerLoop`, `CooperativeWorkerLoopFactory`,
  `CooperativeScheduler`, routing policy, and deferred destruction queue.
- `crates/neovex-runtime/src/backend/locker_deno.rs` (new) —
  `LockerDenoRuntimeDriver` and Locker-aware runtime slot/pool helpers that the
  cooperative worker loop uses below the scheduler layer.
- `crates/neovex-runtime/src/worker_loop.rs` or `worker_loop/mod.rs` —
  wire the new cooperative loop alongside EO5's
  `RunToCompletionWorkerLoopFactory`.
- `crates/neovex-runtime/src/executor.rs` — Select the worker-loop factory, not
  a per-invocation backend, for each worker thread.
- `crates/neovex-runtime/src/runtime.rs` — Add `use_locker` option to runtime
  creation. Integrate RAII lock scope at the `op_neovex_async_host_call`
  boundary (line 1251).

### Verification

```bash
# Smoke tests from Phase 4
cargo test -p neovex-runtime locker_smoke -- --nocapture

# Cooperative scheduler unit tests
cargo test -p neovex-runtime cooperative_scheduler -- --nocapture

# M:1 integration tests
cargo test -p neovex-runtime cooperative_multi_runtime -- --nocapture

# Key test: 4 invocations on 2 threads, each invocation does host I/O,
# verify all 4 complete (proves cooperative yielding works)
cargo test -p neovex-runtime cooperative_yield_under_load -- --nocapture

# Key test: runtime pool warm start is <1ms after first invocation
cargo test -p neovex-runtime cooperative_warm_start_latency -- --nocapture

# Full regression
cargo test -p neovex-runtime -- --nocapture
cargo test -p neovex-server convex_http_demo_ -- --nocapture

# Benchmark: M:1 cooperative vs 1:1 baseline
cargo bench -p neovex-runtime -- cooperative_vs_baseline
```

---

## Phase 6: CI Configuration

**Goal:** Ensure both upstream and fork dependency paths are tested in CI.

### Steps

1. **Add a CI matrix dimension** for the fork:
   ```yaml
   strategy:
     matrix:
       v8-backend: [upstream, locker-fork]
   ```

2. **For `locker-fork` jobs:**
   - Uncomment the `[patch.crates-io]` block before building.
   - Run the full test suite including `locker_smoke` and `cooperative_*` tests.

3. **For `upstream` jobs:**
   - Standard build with crates.io dependencies.
   - Locker-specific tests fail to compile (types don't exist). This is fine.
   - All existing tests pass unchanged.

4. **Nightly or weekly job** to rebase the fork branches onto upstream HEAD and
   report breakage early.

### Output

- Both dependency paths tested on every PR.
- Early warning when upstream changes break the fork.

---

## Phase 7: Upstream Tracking & Swap-Back

**Goal:** Stay close to upstream so the fork can be retired quickly when PR #1896
merges.

### Process

1. **Watch PR #1896** for maintainer review activity. Subscribe to the PR and
   issue #643.

2. **Monthly rebase** of `agentstation/rusty_v8:locker-v147` onto upstream's
   latest tag. If our pinned deno_core version bumps its `v8` dependency, create
   a new `locker-vN` branch targeting the new version.

3. **Monthly rebase** of `agentstation/deno_core:locker-v0.395` similarly.

4. **When PR #1896 merges upstream:**
   a. Wait for a tagged rusty_v8 release containing the Locker API.
   b. Wait for deno_core to bump to that rusty_v8 version.
   c. Contribute our `use_locker` / RAII lock scope changes upstream to
      deno_core (or maintain as a thin patch).
   d. Update Neovex's `deno_core` version in `Cargo.toml`.
   e. Comment out the `[patch.crates-io]` block.
   f. Run `cargo update -p deno_core -p v8 -p serde_v8`.
   g. Verify all tests pass.
   h. Archive the fork branches.

5. **If PR #1896 is abandoned upstream:**
   - Consider contributing directly to the PR or opening a new PR with the
     safety improvements we've made.
   - Continue maintaining the fork as long as Neovex needs it.
   - Evaluate the OpenWorkers path (drop deno_core, use raw rusty_v8) if the
     maintenance burden becomes unsustainable. OpenWorkers made this exact
     transition after 2 years of deno_core usage.

---

## Phase Order and Dependencies

```text
Phase 1 (fork rusty_v8, merge PR #1896)
  └─► Phase 2 (fork deno_core, add Locker-aware JsRuntime)
       └─► Phase 3 (Cargo [patch] swap mechanism)
            └─► Phase 4 (Locker smoke tests in neovex-runtime)
                 └─► Phase 5 (M:1 cooperative worker loop + Locker runtime driver)
                      └─► Phase 6 (CI matrix)

Phase 7 (upstream tracking) ─── ongoing from Phase 1

EO5 (WorkerLoopFactory seam + RunToCompletionWorkerLoop) ─── must be done before Phase 5
```

Phases 1-4 are straightforward fork + plumbing work. Phase 5 is the
architecturally significant phase that builds the cooperative worker loop and
Locker-aware runtime driver.

---

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| PR #1896 merge conflicts with v147.0.0 | Medium | Low | Cherry-pick per-commit, small delta v146.4→v147.0 |
| deno_core internals resist Locker integration | Medium | High | Treat Phase 2 as a 300-500 line fork gated by the mandatory same-thread feasibility spike. If the spike fails or the fork surface expands materially, pause and evaluate the raw rusty_v8 path before Phase 5. |
| Multiple JsRuntimes per thread causes deno_core-level bugs (not V8-level) | Medium | High | Do not assume safety from type structure alone. The Phase 2 spike and Phase 4 smoke tests explicitly validate sequential Locker entry/exit, multi-runtime same-thread execution, and RAII lock cycles before Phase 5 starts. |
| `poll_event_loop_tick` doesn't yield cleanly at host I/O boundaries | Medium | High | deno_core's op driver returns `Poll::Pending` when async ops are in flight. This is the natural yield point. If the granularity is wrong, we may need to add explicit yield hints in our ops. OpenWorkers re-acquires per poll tick — same granularity we're targeting. |
| Runtime pool memory overhead (N runtimes × V8 heap) | Medium | Medium | Bounded pool size. V8 heap limits per isolate already exist (configurable via `RuntimeLimits`). Start with 2-4 runtimes per thread, tune based on memory benchmarks. |
| Cooperative scheduler fairness/starvation | Low | Medium | FIFO runnable queue with bounded per-tick polling and explicit job-admission + I/O-completion handling in the same worker loop. This matches workerd/OpenWorkers fairness more closely than a round-robin index. |
| `v8::Unlocker` aliasing unsoundness | High | High | Omit Unlocker; use Lock-drop/Lock-reacquire pattern. This is equivalent to workerd's approach. |
| Upstream PR #1896 merges with different API shape | Low | Medium | The primary seam is `WorkerLoopFactory`; Locker-specific API churn stays in the cooperative worker loop and runtime-driver layers instead of forcing unrelated admission-control or executor-lifecycle rewrites. |
| Fork maintenance burden | Medium | Medium | Monthly rebase cadence, CI early-warning, structured swap-back plan. |

---

## Open Questions

1. **Should we engage with `max-lt` directly?** The PR #1896 author maintains
   OpenWorkers in production with this exact fork AND dropped deno_core in favor
   of raw rusty_v8. Their experience with deno_core + Locker integration (or
   the lack thereof) is directly relevant to Phase 2. Specifically:
   - Why did OpenWorkers drop deno_core? Was the Locker integration the blocker?
   - What were the pain points with deno_core's `OwnedIsolate`-based JsRuntime?
   - Would they be interested in collaborating on a Locker-aware deno_core fork?

2. **Should we contribute the `use_locker` / RAII lock scope API upstream to
   deno_core?** If we build it and it works well, contributing it upstream would
   benefit the ecosystem and reduce our maintenance burden. This is more likely
   to be accepted than the Rc→Arc patch since it's additive and backward
   compatible. Both deno_core issue #708 (multi-runtime segfaults) and issue
   #643 (Locker tracking) would be addressed.

3. **Runtime pool vs runtime-per-invocation?** The plan assumes a warm pool
   (aligned with OpenWorkers' `ThreadLocalPool` pattern). Alternatively, we
   could create a fresh JsRuntime per invocation (simpler, no state leakage
   risk) and rely on V8 snapshots for fast creation. OpenWorkers reports 790μs
   cold vs <10μs warm. Benchmark both approaches in Phase 5.

4. **`poll_event_loop` vs `with_event_loop_promise` for the scheduler?**
   `poll_event_loop` (jsruntime.rs:2156) is single-tick, ideal for cooperative
   scheduling. `with_event_loop_promise` (jsruntime.rs:2098) runs to
   completion. The scheduler needs `poll_event_loop`. OpenWorkers uses
   per-poll-tick lock re-acquisition — same granularity we're targeting.
   Verify that the single-tick API gives us the yield granularity we need.

5. **How many runtimes per thread?** Start with `max_runtimes_per_thread = 4`
   (matching typical CPU-bound:IO-bound ratio). OpenWorkers exposes
   `max_per_thread` and `max_per_owner` as separate config knobs. Tune based
   on workload characteristics and memory benchmarks.

6. **When to consider dropping deno_core (the OpenWorkers path)?** If Phase 2
   proves too fragile or the maintenance burden of the deno_core fork is
   unsustainable, we should evaluate building on raw rusty_v8. The
   `WorkerLoopFactory` seam from EO5 keeps that change contained at the
   worker-loop/runtime-driver boundary rather than forcing a full executor or
   admission-control redesign, but the cooperative loop implementation itself
   would still need to change. OpenWorkers' transition after 2 years of
   deno_core usage is a relevant data point. Gate this evaluation on the Phase
   2 spike and Phase 5 benchmarks.

---

## Appendix A: Rc<RefCell<...>> to Arc<Mutex<...>> Analysis {#appendix-a}

Full analysis of what it would take to make `JsRuntime` `Send` (Model 2: N:M
cross-thread). This is **deferred** — included for completeness and future
reference.

### Scale of change

**228 `Rc<` occurrences** across deno_core 0.395.0:

| File | Rc count | Key types |
|------|----------|-----------|
| `inspector.rs` | 35 | `Rc<V8Inspector>`, `Rc<RefCell<InspectorFlags>>`, `Rc<RefCell<SessionContainer>>`, `Rc<InspectorSession>` |
| `runtime/jsruntime.rs` | 30 | `ManuallyDropRc<JsRuntimeState>`, `Rc<RefCell<OpState>>`, `Rc<RefCell<SourceMapper>>`, `Rc<RefCell<FunctionTemplateData>>`, `Rc<PromiseFuture>` |
| `runtime/jsrealm.rs` | 17 | `Rc<OpDriverImpl>`, `Rc<ExceptionState>` |
| `runtime/op_driver/futures_unordered_driver.rs` | 7 | `Rc<RefCell<VecDeque<PendingOp>>>`, `Rc<UnsyncWaker>` |
| `ops.rs` | 4 | **PUBLIC: `OpCtx::state: Rc<RefCell<OpState>>`** |
| Other files | ~135 | Various internal state |

### Public API breakage

The critical public API that breaks:

```rust
// ops.rs line 92 — used by every async op
pub struct OpCtx {
    pub state: Rc<RefCell<OpState>>,  // → Arc<Mutex<OpState>>
    // ...
}
```

**neovex-runtime impact:** All 19 async ops have signature
`async fn op_*(state: Rc<RefCell<OpState>>, ...) → ...`. These would all need
to change to `Arc<Mutex<OpState>>`.

Additionally, `RuntimeCancellationState` in neovex-runtime contains
`Rc<CancelHandle>` which would need to become `Arc<CancelHandle>`.

### Performance implications

- `Arc` atomic refcount: ~2-5ns overhead per clone/drop (vs ~1ns for Rc)
- `Mutex` lock: ~15-25ns uncontended (vs ~0ns for RefCell borrow)
- On every op dispatch: `state.borrow()` becomes `state.lock().unwrap()`
- Impact: ~20-30ns per op dispatch. Negligible for I/O-bound ops, measurable
  for micro-ops called thousands of times per invocation.

### Deadlock risk

`RefCell` panics on double-borrow (detectable, debuggable). `Mutex` deadlocks
on double-lock from the same thread (silent hang, hard to debug). Every
`borrow()` → `lock()` conversion needs audit for re-entrant patterns.

### Maintenance burden

Every deno_core upstream update requires rebasing 228+ changes across multiple
files. The Rc→Arc conversion is **structural**, not localized — it touches the
most fundamental types in the crate.

### Verdict

**Not worth it for the Workers-style architecture.** The M:1 model achieves
the same isolate oversubscription pattern without any of these costs. Revisit
only if benchmarks show that per-thread load imbalance (a limitation of M:1)
is a measurable production bottleneck that cannot be solved by request routing.

---

## Verification (end-to-end)

```bash
# 1. Enable fork
#    Uncomment [patch.crates-io] in Cargo.toml

# 2. Verify dependency resolution
cargo tree -p neovex-runtime -i v8
# Expected: v8 v147.0.0-locker.1 (https://github.com/agentstation/rusty_v8...)

# 3. Locker smoke tests
cargo test -p neovex-runtime locker_smoke -- --nocapture

# 4. Cooperative scheduler tests
cargo test -p neovex-runtime cooperative -- --nocapture

# 5. Full test suite (nothing regressed)
make test

# 6. Clippy + fmt
cargo fmt --all --check
make clippy

# 7. Disable fork, verify clean swap-back
#    Comment out [patch.crates-io] in Cargo.toml
cargo update -p deno_core -p v8 -p serde_v8
make test
```
