# Runtime HTTP Cancellation And Storage Rewrite Plan

Verified against repo state on 2026-03-31.

## Goal

Close the remaining HTTP request-teardown cancellation hole for runtime-backed
Convex endpoints, then use that work to drive the larger async/preemption
rewrite of the engine and storage stack.

This document is intentionally split into:

1. a small, high-value runtime-backed HTTP cancellation fix
2. a follow-on parity pass for direct non-runtime HTTP reads
3. the full storage/service rewrite required for true preemption

## Cross-Cutting Constraints

- request authentication and `InvocationAuth` propagation stay unchanged across
  Workstreams 1 and 2
- cancellation stops execution; it does not alter auth verification or usage
  accounting semantics
- when a canceled request still produces an HTTP response, keep the existing
  `Error::Cancelled` mapping / request-timeout-style behavior already used by
  the server today
- if the client has already disconnected, the connection may close before any
  response is observed; the important invariant is teardown and execution
  cancellation, not forcing a response onto a dead socket

---

## Current Verified State

### What is already working

- Runtime async host calls already accept and honor a live cancellation token
  once they receive one:
  - `ConvexRuntimeBridge::call_async(...)` forwards the token into the
    cancellable bridge dispatcher in
    `crates/neovex-server/src/convex/runtime_bridge.rs:943-960`
  - the runtime host executor drops queued jobs and returns `Cancelled` when
    the token fires in `crates/neovex-runtime/src/host_executor.rs:41-139`
  - `op_neovex_async_host_call(...)` already ties JS async host ops to runtime
    cancellation state in `crates/neovex-runtime/src/runtime.rs:1136-1185`
- WebSocket runtime subscriptions already own a real live
  `HostCallCancellation` and cancel it on teardown in
  `crates/neovex-server/src/convex/subscriptions.rs:461` and
  `crates/neovex-server/src/convex/subscriptions.rs:767`
- Cooperative cancellation already exists for long read loops in the engine:
  - `query_documents_cancellable(...)` in
    `crates/neovex-engine/src/service/queries.rs:47-56`
  - `paginate_documents_cancellable(...)` in
    `crates/neovex-engine/src/service/queries.rs:64-76`
  - table scans in `crates/neovex-storage/src/store.rs:247-292`
  - index scans in `crates/neovex-storage/src/index.rs:410-566`

### What is still open

- The runtime-backed HTTP entrypoint still creates a detached default token
  instead of receiving a request-scoped one:
  - `invoke_named_convex_function_async(...)` in
    `crates/neovex-server/src/convex/dispatch.rs:129-138`
  - `invoke_named_convex_function_with_trace_async(...)` creates
    `HostCallCancellation::default()` in
    `crates/neovex-server/src/convex/dispatch.rs:187-201`
- Runtime-backed HTTP handlers still call that detached async helper directly:
  - query route in `crates/neovex-server/src/convex/mod.rs:663-677`
  - paginated query route in
    `crates/neovex-server/src/convex/mod.rs:711-725`
  - mutation route in `crates/neovex-server/src/convex/mod.rs:758-772`
  - action route in `crates/neovex-server/src/convex/mod.rs:810-825`
  - runtime-backed `httpAction` dispatch in
    `crates/neovex-server/src/convex/http_actions.rs:45-75`
- Direct non-runtime HTTP routes still use `run_blocking(...)` and therefore
  have no request-scoped teardown story today:
  - `crates/neovex-server/src/state.rs:100-110`

### What recent bridge work did help with

Recent changes improved cancellation once execution is already inside the
runtime bridge:

- async `ctx.db.get` now has a cancellable bridge path in
  `crates/neovex-server/src/convex/runtime_bridge.rs:451-477`
- async `http_route` now has a cancellable bridge path in
  `crates/neovex-server/src/convex/runtime_bridge.rs:1186-1218`
- the `InvocationKind::Mutation` / `InvocationKind::Action` branches inside
  `convex.invoke` now use cancellable direct helpers in
  `crates/neovex-server/src/convex/runtime_bridge.rs:1157-1181`
- those direct helpers live in
  `crates/neovex-server/src/convex/dispatch.rs:338-370`
- there are regression tests for the above in
  `crates/neovex-server/src/convex/mod.rs:1325-1408`

That work is useful, but it does **not** fix the HTTP request-teardown hole,
because the HTTP handlers still do not own and pass a request-scoped token into
the runtime-backed async dispatch helper.

---

## Workstream 1: Runtime-Backed HTTP Request Cancellation

### Objective

Make runtime-backed HTTP requests cancel the runtime invocation when the Axum
handler future is dropped, instead of letting the invocation continue with a
detached token.

### Scope

This workstream covers only runtime-backed HTTP execution paths:

- named query
- named paginated query
- named mutation
- named action
- runtime-backed `httpAction`

It does **not** attempt to solve direct non-runtime `run_blocking(...)` paths
yet.

### Implementation Plan

#### 1. Add a cancellable async dispatch helper

Add:

- `invoke_named_convex_function_async_cancellable(...)`

in `crates/neovex-server/src/convex/dispatch.rs`.

Requirements:

- make it a thin wrapper over
  `invoke_named_convex_function_with_trace_async_cancellable(...)` at
  `dispatch.rs:203-230`
- return just `Result<Value, Error>`
- accept a caller-owned `HostCallCancellation`
- remain the single async entrypoint used by HTTP handlers

#### 2. Add a request-scoped cancellation guard for HTTP handlers

Introduce a small server-side guard type, for example:

- `HttpRequestCancellation`
- `RequestCancellationGuard`

Recommended shape:

- owns a `HostCallCancellation`
- exposes a cheap `clone_token()` or `token()` accessor
- calls `cancel()` in `Drop`

Implementation note:

- prefer a tiny guard tied to the handler future lifetime instead of coupling
  the first implementation to a framework-specific disconnect API
- if Axum exposes a first-class disconnect signal in a way that is convenient
  for these handlers, the guard may wrap that signal internally later
- the important invariant is that dropping the handler future cancels the token

The important behavior is:

- if Axum drops the handler future because the client disconnects, the guard is
  dropped
- drop triggers token cancellation automatically
- the runtime invocation then sees a real live request-scoped token

#### 3. Thread the token through all runtime-backed HTTP routes

Update these call sites to use the new cancellable async helper:

- `crates/neovex-server/src/convex/mod.rs:663-677`
- `crates/neovex-server/src/convex/mod.rs:711-725`
- `crates/neovex-server/src/convex/mod.rs:758-772`
- `crates/neovex-server/src/convex/mod.rs:810-825`
- `crates/neovex-server/src/convex/http_actions.rs:45-75`

Each handler should:

- create a request-scoped guard before calling the runtime path
- pass the guard's token into
  `invoke_named_convex_function_async_cancellable(...)`
- keep the guard alive for the entire async request lifetime

#### 4. Handle runtime-backed `httpAction` separately

`httpAction` has a different shape from the JSON Convex endpoints because it
returns HTTP response parts rather than a plain Convex value.

Requirements:

- the request-scoped guard must be created before the runtime-backed branch in
  `crates/neovex-server/src/convex/http_actions.rs:45-75`
- the same live token must be passed into
  `invoke_named_convex_function_async_cancellable(...)`
- the guard must stay alive until the runtime value has been decoded and
  `build_http_response_parts(...)` has completed
- cancellation should govern the runtime invocation and response planning work;
  the final local response-building step is synchronous and should remain a
  short in-process operation

#### 5. Use existing runtime metrics explicitly

No new metric names are needed for the first pass.

This workstream should rely on the existing counters in
`crates/neovex-runtime/src/metrics.rs:16-106` and verify that dropped HTTP
requests move:

- `canceled_invocations`
- `canceled_host_ops`

#### 6. Preserve existing WebSocket handling

Do not rework subscription teardown in this pass.

The WebSocket path already has a live cancellation token in:

- `crates/neovex-server/src/convex/subscriptions.rs:461`
- `crates/neovex-server/src/convex/subscriptions.rs:767`

That path should stay as-is unless the HTTP cancellation wiring reveals a shared
abstraction worth extracting later.

### Tests

Add server/runtime tests for:

1. dropped runtime query request cancels in-flight async host work
2. dropped queued HTTP request never starts execution once queued
3. canceled request increments runtime cancellation metrics

Recommended assertions:

- request resolves as `Cancelled` / request-timeout style server error
- no late success value is written after disconnect
- queued job test proves the runtime invocation body never starts
- `canceled_invocations` / `canceled_host_ops` metrics move in the expected
  direction

### Acceptance Criteria

- every runtime-backed HTTP endpoint owns a request-scoped token
- disconnecting the client cancels the runtime invocation
- queued runtime work for a dropped request never starts
- there is no regression in WebSocket subscription cancellation behavior
- new regression tests cover the behavior

---

## Workstream 2: Direct Non-Runtime HTTP Cancellation Parity

### Objective

Make teardown behavior more uniform for non-runtime HTTP reads that still go
through `run_blocking(...)`.

### Important Constraint

This is **not** the full storage rewrite. It is only a parity pass for the
direct execution path where we already have cooperative read cancellation hooks.

### Scope

Good candidates:

- direct query execution
- direct paginated query execution
- any read-only direct route that can already accept a cancellation callback

Bad candidates for a quick parity pass:

- mutation writes
- scheduler writes
- schema writes

Those need stronger commit semantics and belong in Workstream 3.

### Implementation Plan

#### 1. Add cancellable blocking wrappers only for read paths

Introduce a server helper that can coordinate:

- a caller-owned `HostCallCancellation`
- a blocking worker closure
- an engine callback that polls cancellation cooperatively

Recommended shape:

- add a helper alongside `run_blocking(...)` in
  `crates/neovex-server/src/state.rs:100-110`, for example
  `run_blocking_cancellable(...)`
- inside the helper, start `spawn_blocking(...)` and race it with
  `cancellation.cancelled()`
- pass a cloned cancellation token into the blocking closure
- inside the blocking closure, use a `check_cancel` callback that returns
  `Error::Cancelled` when `cancellation.is_cancelled()`

Important nuance:

- `spawn_blocking(...)` itself cannot preempt a running thread
- the helper therefore returns early to the async caller when the token fires,
  while the blocking task exits cooperatively on its next cancellation poll
- this is acceptable only for read paths that already have cooperative scan
  checkpoints
- when the handler is still able to return an HTTP response, cancellation should
  continue to surface through the existing `Error::Cancelled` path rather than a
  new status mapping

The direct query/paginated code paths should be updated to use:

- `query_documents_cancellable(...)` from
  `crates/neovex-engine/src/service/queries.rs:47-56`
- `paginate_documents_cancellable(...)` from
  `crates/neovex-engine/src/service/queries.rs:64-76`

#### 2. Do not fake write preemption

Do not return `Cancelled` after a durable write has committed.

In particular, do not try to make post-commit mutation fanout arbitrarily
cancelable unless there is a separate resync strategy for subscriptions. A
committed write paired with a canceled response is worse than the current known
limitation.

### Tests

Add direct-path server tests for:

- dropped direct query request cancels a long table/index scan
- dropped direct paginated request cancels long scan work

### Acceptance Criteria

- direct non-runtime reads behave similarly to runtime-backed reads on client
  disconnect
- no attempt is made to pretend writes are preemptible before the storage
  rewrite exists

---

## Workstream 3: Full Storage And Service Rewrite

### Objective

Replace the current synchronous storage/service boundary with a truly async,
preemptible one so runtime cancellation stops real storage work instead of just
worker-thread jobs wrapped around synchronous code.

### Why This Is A Rewrite

Today the storage surface is synchronous and concrete:

- document storage in `crates/neovex-storage/src/store.rs`
- index maintenance in `crates/neovex-storage/src/index.rs`
- scheduler persistence in `crates/neovex-storage/src/scheduler.rs`
- schema persistence in `crates/neovex-storage/src/schema_store.rs`
- usage accounting in `crates/neovex-storage/src/usage_store.rs`

The engine service is also synchronous and directly coupled to those concrete
stores:

- reads in `crates/neovex-engine/src/service/queries.rs`
- writes in `crates/neovex-engine/src/service/mutations.rs`
- scheduler in `crates/neovex-engine/src/service/scheduler.rs`
- schema in `crates/neovex-engine/src/service/schema.rs`
- tenant lifecycle in `crates/neovex-engine/src/service/tenants.rs`

The server then compensates with `spawn_blocking(...)` in
`crates/neovex-server/src/state.rs:100-110`, and the runtime compensates again
with worker threads in `crates/neovex-runtime/src/host_executor.rs:41-139`.

That means the real work is still synchronous underneath the bridge.

### Rewrite Shape

#### 1. Introduce an explicit trait hierarchy

Split the boundary into:

- a global storage control-plane trait for tenant lifecycle work
- a per-tenant data-plane trait for document/index/scheduler/schema operations
- a separate control-plane usage/accounting trait

Recommended shape:

- `StorageEngine`
  - tenant open/load/list/delete
  - returns a tenant-scoped handle
- `TenantReadStorage`
  - point reads
  - table scans
  - index scans
  - commit-log reads
  - latest-sequence reads
- `TenantWriteStorage`
  - begins a tenant-scoped write transaction
- `TenantWriteTransaction`
  - document mutations
  - index maintenance
  - scheduler state transitions
  - schema/index rebuild mutations
  - commit / rollback
  - drop without commit implies rollback / abort
- `UsageStorage`
  - monthly active user recording and reporting

Tenant-vs-global boundary:

- `StorageEngine` is global
- `TenantReadStorage` / `TenantWriteStorage` / `TenantWriteTransaction` are
  per-tenant
- `UsageStorage` remains a separate global control-plane store

#### 2. Prefer native async trait methods for the new boundary

Prefer native `async fn` trait methods for the first internal boundary, keeping
the engine generically typed over storage implementations rather than forcing
trait-object dispatch on day one.

Guidance:

- prefer native async traits / RPITIT-style internal boundaries over
  `async_trait` unless object-safety becomes a real blocker
- keep the first version statically dispatched through concrete storage types or
  generic parameters
- only introduce boxed trait objects if the engine/service wiring truly needs
  dynamic dispatch

#### 3. Split read APIs from write transaction APIs

Reads and writes need different cancellation semantics.

Read APIs should cover:

- point reads
- scans
- query/index evaluation support

- should be cancelable throughout
- should expose async iteration or async batched collection where practical
- should preserve existing cooperative checkpoints from the current scan/index
  loops

Write transaction APIs should cover:

- write transactions
- scheduler queue operations
- schema operations that mutate persisted state

Write APIs:

- must define an explicit durable commit point
- should be cancelable before commit
- must not report `Cancelled` after the commit point has passed
- should return success once durability is guaranteed, even if later response
  shaping is interrupted
- dropping or abandoning an uncommitted transaction must roll it back without
  exposing partial durable state

#### 4. Add an explicit async transaction model

The current write methods inline their own `begin_write(...)`, mutate tables,
append commit log entries, and commit in one synchronous call. The rewrite
should replace that with an explicit async transaction boundary.

Requirements:

- atomic document + index + commit-log updates remain intact
- scheduler/job state transitions remain atomic wherever the rewritten API says
  they are atomic
- scheduler claim / cancel / result / completion semantics are explicitly
  documented by the new transaction boundary rather than left implicit in
  service-layer call ordering
- schema/index rebuild operations become async and bounded
- the service layer no longer owns raw storage transaction plumbing
- dropping a transaction without `commit()` rolls it back implicitly

#### 5. Move the engine service layer to async

Convert the service methods used by server/runtime paths to async:

- read paths
- scheduler paths
- schema paths
- tenant open/load paths where practical
- eventually mutation paths once async transaction semantics exist

This is the point where server routes and runtime host ops can stop treating the
engine as a blocking subsystem.

#### 6. Rework tenant lifecycle around async store handles

Tenant loading/open/delete currently synchronously touches filesystem/store
state in `crates/neovex-engine/src/service/tenants.rs:13-105`.

The rewrite should define:

- async tenant open/load
- async-safe lifecycle guards
- shutdown/delete behavior that coordinates with in-flight async storage work

#### 7. Remove blocking adaptation layers once async storage exists

Only after the service/storage rewrite is in place should we remove:

- server `run_blocking(...)` usage for engine/storage work
- the temporary Workstream 2 `run_blocking_cancellable(...)` helper
- runtime host executor usage as a wrapper around synchronous storage closures

At that point the runtime bridge can dispatch real async storage futures rather
than sending sync work to worker threads.

### Open Design Decision

There are two viable implementation strategies:

1. keep redb behind a dedicated async storage executor/actor boundary first
2. switch to an async-native storage backend

This document does not force that decision, but the engine-facing API should be
designed so either backend can satisfy it.

Selection criteria:

- cancellation latency for long reads and queued writes
- ability to preserve atomic document/index/commit-log updates
- migration risk for existing `.redb` tenant data on disk
- operational complexity and deployment footprint
- implementation risk for scheduler/cron/schema rebuild behavior
- observability and debugging ergonomics under contention/cancellation

### Tests

Add rewrite-level tests for:

- timeout cancels in-flight storage reads
- queued canceled request never begins storage execution
- cancel before commit does not write durable state
- cancel after commit still returns a durable success outcome
- scheduler claims/cancels remain atomic under cancellation
- schema/index rebuilds remain bounded and do not leak work

### Acceptance Criteria

- runtime cancellation propagates into real storage work, not just bridge jobs
- server routes no longer need blocking wrappers for engine/storage paths
- reads are truly cancelable
- writes have explicit, documented pre-commit/post-commit semantics

---

## Recommended Delivery Order

1. Workstream 1: runtime-backed HTTP request-scoped cancellation
2. Workstream 2: direct non-runtime read cancellation parity
3. Workstream 3a: async storage traits and read-path migration
4. Workstream 3b: async write transaction model
5. Workstream 3c: server/runtime removal of blocking adaptation layers

This ordering gives the fastest correctness win first, while still keeping the
full storage rewrite clearly defined and intentionally staged.

---

## Carried-Forward Follow-On Work

If Workstreams 1 through 3 are completed, the core implementation goals from
the superseded async bridge/executor spec are effectively covered:

- top-level runtime work enters through the shared executor
- real store-backed runtime work uses typed async ops
- cancellation propagates from timeout/request teardown into real async work
- same-isolate nested dispatch remains the default path
- cross-isolate fallback remains explicit and bounded

The following items should still remain as add-on or future work. They are
important, but they are not blockers for declaring the core runtime/executor
implementation complete.

### 1. Signed Deployment Manifests And Artifact Provenance

Once deploy identity/auth exists, add a control-plane hardening track for:

- signed deployment manifests rather than hash-sidecar-only verification
- authenticated upload/publish flows
- immutable deployment version identity
- clearer separation between runtime integrity checks and deployment provenance

This is the right place to carry forward the old spec's "signed deployment
manifest" follow-up without treating it as part of the runtime cancellation
rewrite itself.

### 2. Richer Per-Tenant Executor Fairness

The current executor work is about bounded concurrency, queueing, and
cancellation. A later pass can improve fairness if one tenant can dominate the
queue.

Candidate follow-on work:

- per-tenant admission buckets or semaphores
- per-tenant queue depth caps
- weighted scheduling for paid/prioritized tenants
- starvation-prevention rules for noisy-neighbor scenarios
- metrics and dashboards that expose tenant-level contention

This should remain a separate optimization/control-plane policy layer on top of
the core executor, not a blocker for Workstreams 1 through 3.

### 3. Deeper Async Bridge Instrumentation

The runtime already exposes useful counters, but a later observability pass can
make the async bridge materially easier to operate and debug.

Initial instrumentation now includes:

- separate queued-vs-in-flight cancellation counters for runtime invocations
- separate pre-start-vs-in-flight cancellation counters for host ops
- per-host-operation started / succeeded / failed / canceled counts in runtime
  diagnostics
- tracing spans for async host-op enqueue / start / finish / cancel transitions
- per-tenant queue wait and execution distributions
- clearer timeout vs disconnect vs explicit-cancel attribution

Candidate follow-on work:

- correlation between server request ids and runtime invocation ids

This is the natural place to carry forward the old spec's "deeper async bridge
instrumentation" item.

### 4. Typed-Op Hardening After Raw Host-Call Removal

The legacy generic raw host-call surface has been removed. Normal
generated/runtime flows now execute through typed runtime ops and local
same-isolate dispatch only.

Candidate follow-on work:

- keep generated bundles and fixtures pinned to typed ops only
- avoid reintroducing generic catch-all host-call surfaces in new runtime APIs
- continue tightening runtime contracts around explicit typed operations

This is now cleanup and hardening after the raw host-call removal rather than a
separate migration track.

### 5. Explicit Non-Goals For The Core Runtime Rewrite

The following items were called out in the superseded spec as things that
should not block the core runtime/executor phase and should remain non-goals
for the main delivery sequence here as well:

- multi-process runtime pools
- moving every in-memory helper op to async
- per-tenant weighted scheduling as a phase gate
- signed deployment manifests as a phase gate

Those may all be worthwhile later, but they should not be allowed to dilute the
main completion path defined in this document.
