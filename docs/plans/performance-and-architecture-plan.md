# Performance And Architecture Master Plan

Verified against clean repo state at `7fa907d` on 2026-04-01.

This is the canonical execution roadmap for the next architecture and
performance cycle. It replaces the old split-plan setup as the single source of
truth for implementation sequencing, agent execution, and verification.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/plans/archive/runtime-http-cancellation-and-storage-plan.md` for
  historical cancellation context
- `docs/plans/archive/async-storage-and-service-rewrite-plan.md` for the
  absorbed async rewrite content
- `docs/research/reactive-database-research-guide.md`
- `docs/research/horizontal-scaling-architecture-spec.md`
- the current Rust crate layout under `crates/`

---

## Purpose

This document translates the research and architecture docs into a phased,
implementation-ready roadmap for the current codebase.

Use it as:

- the execution roadmap for upcoming architecture work
- the canonical sequencing guide when multiple agents are working in parallel
- the verification contract for tests, metrics, and acceptance criteria

Every roadmap item below is written so an agent can:

1. find the right code quickly
2. implement the change without guessing the intended semantics
3. verify it with deterministic tests
4. preserve the repo invariants in `ARCHITECTURE.md` and `AGENTS.md`

---

## Canonical Plan Rules

1. This document is the only canonical execution spec for the work it covers.

2. `docs/plans/archive/async-storage-and-service-rewrite-plan.md` and
   `docs/plans/archive/runtime-http-cancellation-and-storage-plan.md` are
   historical context, not competing execution plans.

3. When this roadmap and a historical plan disagree, this roadmap wins.

4. When a roadmap item changes architecture-level behavior, update
   `ARCHITECTURE.md` in the same PR.

---

## What This Revision Fixes

This revision does more than merge documents. It also resolves the major issues
from the last review round:

1. The async storage rewrite is now fully specified here instead of split across
   multiple "canonical" documents.

2. Post-commit semantics are now stated at the correct boundary:
   engine/storage must not surface `Cancelled` after durable commit, but a
   disconnected HTTP or WebSocket client still may not observe the success.

3. Runtime isolate pooling is constrained so it does not accidentally change
   JavaScript module-state semantics across invocations.

4. The earlier metadata-keyed integrity-cache idea is removed as an execution
   item. Path-backed mutable bundles continue to use content hashing on every
   invocation unless and until immutable signed deployment artifacts exist.

5. The async rewrite is explicitly scoped to the current redb-based
   architecture behind an async boundary first. Swapping the storage backend is
   out of scope for this roadmap and would require a separate architecture
   decision.

---

## Alignment With Research And Spec Docs

These are the principles that connect the research docs to the current codebase.
Every work item below should preserve them.

1. The tenant boundary remains the scaling boundary.
   The horizontal-scaling spec is explicit here, and the code already reflects
   it: one `TenantStore` per tenant, lazy-opened inside `Service`. None of the
   work below should introduce cross-tenant coordination on the mutation,
   subscription, or runtime hot path.

2. The commit log remains the universal ordering primitive.
   The research and scaling docs treat ordered per-tenant history as the basis
   for invalidation, replay, replication, and recovery. Later phases should
   move closer to that model, not away from it.

3. Full re-evaluation remains the default correctness path until narrower
   invalidation is proven safe.
   The research guide recommends Convex-style full re-evaluation first. The
   engine already uses table-level invalidation, and runtime-backed Convex
   subscriptions already have a narrower read-set-aware skip path. New
   precision must not introduce false negatives.

4. The engine keeps the single mutation path.
   Every mutation continues to flow through `Service::apply_mutation(...)` or
   its async successor. No work item should create a bypass around validation,
   indexing, commit-log append, or subscription fan-out.

5. `neovex-runtime` stays independent.
   The runtime crate still has zero workspace dependencies. Pooling, fairness,
   metrics, and async-bridge work must preserve that boundary.

6. Redb remains the storage engine for this roadmap.
   Phase 5 is an execution-model rewrite, not a storage-engine replacement. The
   first async implementation keeps the existing redb-backed storage semantics
   behind an async boundary. Backend replacement is a separate future project.

7. Explicit commit semantics matter more than transport delivery.
   Engine and storage code must define a durable commit point. Before that
   point, cancellation may abort the write. After that point, the engine/storage
   boundary must not surface `Cancelled`. However, an HTTP or WebSocket client
   that disconnects after commit still may not observe the success response.

8. Runtime bundle integrity remains strict.
   For mutable, path-backed bundles, the runtime continues to verify content
   integrity on every invocation. Stable bundle identity may be used for pooling
   or provenance bookkeeping, but not as a reason to skip content verification.

9. Performance work must be measurable.
   If an item claims a win, it should either add deterministic metrics or extend
   existing tests so the benefit is visible without relying on noisy wall-clock
   timings.

10. One plan document must stay authoritative.
    Agents should not have to reconcile multiple partial plans before they can
    start implementation.

---

## Out Of Scope For This Roadmap

The following are intentionally excluded unless a later revision explicitly adds
them:

- replacing redb with a different storage engine
- changing tenant isolation into cross-tenant sharding on the hot path
- changing JavaScript runtime semantics so top-level module state persists
  across otherwise independent invocations
- making unsigned or mutable bundle metadata caches part of the trust boundary
- multi-process execution pools as part of the near-term runtime roadmap

---

## Execution And Verification Contract

Use this section as the default operating procedure for every work item below.

### General rules

- Prefer symbol references over line numbers in implementation notes and review
  comments.
- Preserve crate invariants from `ARCHITECTURE.md` and `AGENTS.md`.
- Keep public API churn to the minimum required by the protocol or CLI.
- Prefer clean replacements over compatibility layers. This repo is still
  pre-launch.
- If a plan item changes observable semantics, state that explicitly in code
  comments, tests, and docs rather than letting the change happen implicitly.

### Where to add tests

- runtime unit tests:
  `crates/neovex-runtime/src/*.rs`
- engine behavior tests:
  `crates/neovex-engine/src/tests.rs`
- storage behavior tests:
  `crates/neovex-storage/src/tests.rs`
- server and router tests:
  `crates/neovex-server/src/tests/`
- Convex adapter integration tests:
  `crates/neovex-server/src/adapters/convex/tests/`

### Verification workflow per work item

1. Capture a baseline.
   Use existing targeted tests plus either `RuntimePolicy::metrics_snapshot()`
   or `/debug/runtime/metrics` when the item touches runtime behavior.

2. Add or extend deterministic tests first.
   For performance work, verify semantics and instrumentation rather than raw
   timing.

3. Implement the change.

4. Re-run targeted tests for touched crates.

5. For code changes, run:
   - `cargo fmt --all --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`

6. For larger architectural changes, run the relevant crate suites at minimum:
   - `cargo test -p neovex-runtime`
   - `cargo test -p neovex-engine`
   - `cargo test -p neovex-storage`
   - `cargo test -p neovex-server`

### Baseline verification commands

- `cargo fmt --all --check`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo deny check`
- `npm run test --workspaces --if-present`
- `npm run build --workspaces --if-present`
- `make ci`

Not every item needs the full matrix during development, but any architectural
PR should aim to finish there.

---

## Recent Completed Foundation Work

The roadmap below builds on work that is already in the tree:

- request-scoped runtime HTTP cancellation is already wired through the server
  and runtime-backed Convex paths
- direct non-runtime read paths already have a cancellable blocking parity path
- `HostCallCancellationCause` already distinguishes explicit cancellation from
  client disconnect
- `RuntimeMetrics` already tracks:
  - per-operation host-call outcomes
  - per-tenant queue-wait and execution distributions
  - recent request correlations
- `RequestCancellationGuard` already exists in
  `crates/neovex-server/src/state.rs`
- runtime-backed named subscription re-evaluation already has read-set-aware
  skipping in
  `crates/neovex-server/src/adapters/convex/subscriptions/transforms/runtime/reeval.rs`
- typed-op hardening is already in place and must stay in place

Those are baseline capabilities, not future work.

---

## Phase 1: Low-Risk Incremental Improvements

These items are intended to be independently shippable and should not require a
cross-crate architecture rewrite.

### 1A. Reuse one Tokio runtime per worker thread

**Priority:** high  
**Expected impact:** removes repeated Tokio runtime construction from runtime
hot paths.

#### Current verified state

- `RuntimeExecutor::new(...)` in `crates/neovex-runtime/src/executor.rs`
  builds a new current-thread Tokio runtime inside the worker loop before every
  job runs.
- `RuntimeExecutor::invoke_blocking_with_cancellation(...)` also builds a fresh
  current-thread runtime per blocking invocation.

#### Implementation plan

1. Move current-thread runtime creation in `RuntimeExecutor::new(...)` so each
   worker thread builds its Tokio runtime once before entering the receive loop.

2. Reuse that worker-local runtime for every `invoke_job(...)` call handled on
   that worker.

3. Remove per-call runtime construction from
   `invoke_blocking_with_cancellation(...)`.

4. Replace the async-to-sync bridge there with a true blocking submission path,
   such as `mpsc::Sender::blocking_send(...)` plus a blocking receive for the
   result channel, or an equivalent std-channel design.

5. Preserve the existing pre-cancel short-circuit and request-correlation
   recording behavior.

#### Files to change

- `crates/neovex-runtime/src/executor.rs`

#### Existing tests to extend

- `crates/neovex-runtime/src/executor.rs`
- `crates/neovex-server/src/tests/convex_runtime/cancellation/request_drops/`

#### New tests

- add a unit test that submits multiple jobs through the worker path and proves
  they all complete correctly using the same worker-local Tokio runtime
- add a sync-entrypoint test covering
  `invoke_blocking_with_cancellation(...)` without a Tokio runtime already
  present on the calling thread

#### Acceptance criteria

- there is no `tokio::runtime::Builder::new_current_thread()` call inside the
  worker job loop
- there is no per-call Tokio runtime creation in the blocking path
- cancellation and request-correlation behavior remain unchanged

---

### 1B. Add shared runtime-bundle identity without weakening integrity checks

**Priority:** medium  
**Expected impact:** enables safe pooling and provenance work while preserving
the current bundle trust model.

#### Current verified state

- `RuntimeBundle::verify_integrity()` in
  `crates/neovex-runtime/src/runtime.rs` recomputes the SHA-256 digest on every
  invocation.
- `RuntimeBundle` is `Clone`, but clones do not currently share a richer stable
  internal identity.
- pool design, metrics, and registry code all benefit from a stable bundle
  identity that survives cloning.

#### Implementation plan

1. Move `RuntimeBundle` to shared internal state, or add a shared internal
   struct such as `Arc<RuntimeBundleShared>`, so clones can share:
   - canonicalized entrypoint path
   - normalized expected digest
   - future provenance metadata

2. Define a stable bundle identity from canonical path plus expected digest.
   This identity is for pooling, metrics, and provenance bookkeeping only.

3. Keep path-backed mutable bundles on strict per-invocation content hashing in
   `verify_integrity()`. Do not introduce file-metadata caches that skip hashing
   based on mtime, size, or similar heuristics.

4. If a later phase introduces immutable content-addressed bundle artifacts or
   immutable verified in-memory bundle bytes, only that immutable
   representation may amortize verification work.

5. Make the trust boundary explicit in comments and tests: stable identity does
   not authorize skipping content verification for mutable paths.

#### Files to change

- `crates/neovex-runtime/src/runtime.rs`
- optionally
  `crates/neovex-server/src/adapters/convex/registry/loading.rs` if canonical
  path normalization belongs in registry loading

#### Existing tests to extend

- runtime bundle integrity tests in `crates/neovex-runtime/src/runtime.rs`
- registry loading tests in
  `crates/neovex-server/src/tests/registry_and_license/registry.rs`

#### New tests

- verify clones of the same `RuntimeBundle` share the same normalized bundle
  identity
- verify a bundle that passed one invocation still detects later content
  tampering on the next invocation
- verify canonical-path normalization does not change bundle-integrity results

#### Acceptance criteria

- bundle clones share stable identity metadata
- path-backed bundles still perform content-hash verification on every
  invocation
- no file-metadata cache becomes part of the integrity trust model

---

### 1C. Relax atomic ordering on diagnostic counters

**Priority:** low  
**Expected impact:** small but free improvement, especially on ARM.

#### Current verified state

`crates/neovex-runtime/src/metrics.rs` uses `Ordering::SeqCst` for diagnostic
counters that are only consumed by snapshots and diagnostics.

#### Implementation plan

1. Change diagnostic counters in `RuntimeMetrics` from `SeqCst` to `Relaxed`
   for `fetch_add`, `fetch_sub`, and `load`.

2. Add a short comment explaining why relaxed ordering is sufficient there.

3. Do not change correctness-sensitive cancellation state in
   `crates/neovex-runtime/src/host.rs`. `HostCallCancellationState` remains
   `SeqCst`.

#### Files to change

- `crates/neovex-runtime/src/metrics.rs`

#### Existing tests to extend

- `crates/neovex-runtime/src/metrics.rs`

#### New tests

- add or extend a metrics snapshot test that exercises increment, decrement, and
  snapshot reads so the ordering change is covered by deterministic assertions

#### Acceptance criteria

- diagnostic-only runtime counters no longer use `SeqCst`
- cancellation atomics retain their stronger ordering

---

### 1D. Deduplicate mutation planning across direct and scheduled paths

**Priority:** low  
**Expected impact:** maintainability and lower bug risk for future mutation
changes.

#### Current verified state

`apply_mutation(...)` and `execute_scheduled_mutation(...)` in
`crates/neovex-engine/src/service/mutations.rs` still duplicate:

- schema lookup
- validation rules
- index lookup
- indexed vs non-indexed store dispatch
- delete special-casing

#### Implementation plan

1. Extract per-variant helpers rather than a generic validation-closure plan
   object. A good first shape is:
   - `apply_insert_like(...)`
   - `apply_update_like(...)`
   - `apply_delete_like(...)`

2. Thread an execution mode into those helpers, for example:

   ```rust
   enum MutationExecutionMode<'a> {
       Immediate,
       Scheduled { execution_id: &'a str },
   }
   ```

3. Keep deletion as an explicit branch so the deleted-document snapshot flow
   used by `process_commit(...)` remains intact.

4. Keep `Service::apply_mutation(...)` and
   `Service::execute_scheduled_mutation(...)` as the public semantic entry
   points, but make them thin wrappers over the shared helpers.

#### Files to change

- `crates/neovex-engine/src/service/mutations.rs`

#### Existing tests to extend

- `crates/neovex-engine/src/tests.rs`

#### New tests

- none are strictly required if the existing direct-write and scheduled-write
  suites still cover all branches, but add focused unit coverage if helper
  extraction leaves any branch less directly exercised

#### Acceptance criteria

- schema and index resolution live in one place per mutation kind
- direct and scheduled mutation paths still produce identical externally visible
  behavior except for scheduled deduplication semantics

---

### 1E. Stream filter evaluation through fallback scans

**Priority:** medium  
**Expected impact:** lowers peak memory use for unindexed query fallbacks.

#### Current verified state

- `evaluate_query_cancellable(...)` in `crates/neovex-engine/src/evaluator.rs`
  calls `TenantStore::scan_table_cancellable(...)`, which collects every
  document in the table before filtering.
- `evaluate_paginated_cancellable(...)` does the same for paginated fallback
  scans.

#### Important clarification

With the current storage format and `Document::from_msgpack(...)` path, this
item can reduce peak allocation, but it cannot honestly claim to skip
deserialization for non-matching rows unless a separate partial MessagePack
field-extraction path is added later.

#### Implementation plan

1. Make the filter matcher reusable from the scan path by either:
   - making `matches_filters(...)` `pub(crate)`, or
   - moving the filter-matching logic into a small shared helper module

2. Add a storage API that iterates rows one at a time and collects only matching
   documents.

3. Route full-scan query evaluation through that API when no index plan is
   available.

4. Keep the current "scan then sort then apply limit" behavior where ordering
   requires materializing the matching set, but only materialize matching rows,
   not the full table.

5. Leave partial MessagePack decoding as a separate future optimization.

#### Files to change

- `crates/neovex-storage/src/store.rs`
- `crates/neovex-engine/src/evaluator.rs`

#### Existing tests to extend

- query and pagination tests in `crates/neovex-engine/src/tests.rs`

#### New tests

- add a fallback-query test with a large table and a selective filter to prove
  correctness still holds when filtering is performed during scan
- add a paginated fallback-scan test to ensure cursor and ordering behavior are
  unchanged

#### Acceptance criteria

- full-scan fallback queries no longer allocate a `Vec<Document>` containing
  every row in the table before filtering
- query and paginated semantics are unchanged
- this item does not claim or require partial MessagePack decoding

---

### 1F. Make the scheduler event-driven

**Priority:** low  
**Expected impact:** removes unconditional 1-second wakeups when no work is
due.

#### Current verified state

- `crates/neovex-engine/src/scheduler.rs` drives the scheduler with
  `tokio::time::interval(...)`
- the scheduler is spawned and shut down by `crates/neovex-bin/src/main.rs`
- the storage layer exposes `claim_due_jobs(...)` and `load_cron_jobs(...)`, but
  it does not yet expose "next due at" helpers

#### Implementation plan

1. Add "next due work" helpers to the storage/service layers, either:
   - `TenantStore::next_scheduled_work_at(...)`, or
   - explicit scheduled-job and cron helpers if that splits the code more cleanly

2. Add service wrappers for those helpers in
   `crates/neovex-engine/src/service/scheduler.rs`.

3. Replace the unconditional ticker in `run_scheduler_with_interval(...)` with a
   sleep-until loop keyed off the minimum next-due timestamp across loaded
   tenants.

4. Add a `Notify`-based wakeup path on the engine side so:
   - `schedule_mutation(...)`
   - `create_cron_job(...)`
   - any future rescheduling path
   can wake the scheduler immediately when earlier work arrives.

5. Keep watch-channel shutdown ownership in `neovex-bin` for now.

#### Files to change

- `crates/neovex-storage/src/scheduler.rs`
- `crates/neovex-engine/src/service/scheduler.rs`
- `crates/neovex-engine/src/service/mod.rs`
- `crates/neovex-engine/src/scheduler.rs`

#### Existing tests to extend

- scheduling tests in `crates/neovex-engine/src/tests.rs`
- scheduler storage tests in `crates/neovex-storage/src/tests.rs`

#### New tests

- add coverage for the new "next due" helpers
- add an async scheduler test that proves newly scheduled earlier work wakes the
  sleeping scheduler promptly

#### Acceptance criteria

- the scheduler no longer wakes every second when no jobs or crons are due
- new earlier work wakes the scheduler promptly
- shutdown behavior in `crates/neovex-bin/src/main.rs` remains correct

---

### 1G. Add RAII cleanup handles without losing stable subscription ids

**Priority:** low  
**Expected impact:** reduces leak risk on disconnect and error paths.

#### Current verified state

- `SubscriptionRegistry::register(...)` in
  `crates/neovex-engine/src/subscriptions.rs` returns only a numeric id
- manual unsubscribe/cleanup exists in:
  - `crates/neovex-engine/src/service/subscriptions.rs`
  - `crates/neovex-server/src/ws/socket.rs`
  - `crates/neovex-server/src/runtime/subscriptions.rs`
  - Convex subscription forwarding and teardown code

#### Implementation plan

1. Introduce a cleanup handle that owns unregister-on-drop behavior, but do not
   replace the numeric id.

2. Change `SubscriptionRegistry::register(...)` to return both the stable id and
   the drop-based cleanup handle.

3. Keep explicit `unsubscribe(...)` behavior. Dropping the handle is a safety
   net, not the only unregister path.

4. Thread the handle through the server-side owners that actually control
   subscription lifetime:
   - generic WebSocket route
   - runtime subscription bridges
   - Convex subscription state

#### Files to change

- `crates/neovex-engine/src/subscriptions.rs`
- `crates/neovex-engine/src/service/subscriptions.rs`
- `crates/neovex-server/src/ws/socket.rs`
- `crates/neovex-server/src/runtime/subscriptions.rs`
- `crates/neovex-server/src/adapters/convex/subscriptions/`

#### Existing tests to extend

- subscription tests in `crates/neovex-engine/src/tests.rs`
- reactive loop and WebSocket tests in `crates/neovex-server/tests/reactive_loop/`

#### New tests

- add a unit test proving that dropping the handle unregisters the subscription
- add an integration test covering connection teardown without an explicit
  unsubscribe message

#### Acceptance criteria

- connection teardown cannot leak subscriptions merely because explicit cleanup
  was skipped on one server path
- numeric subscription ids remain stable and protocol-compatible

---

## Phase 2: Runtime Execution Performance

### 2A. Add worker-local isolate pooling without changing invocation semantics

**Priority:** highest  
**Expected impact:** largest likely latency win on runtime-backed requests
without changing the JavaScript contract.

#### Current verified state

For each runtime invocation, `NeovexRuntime::invoke_bundle_unmanaged(...)` in
`crates/neovex-runtime/src/runtime.rs` currently:

- verifies bundle integrity
- creates a fresh `JsRuntime`
- installs heap-limit behavior
- boots the runtime environment
- loads and evaluates the bundle
- invokes the exported runtime entrypoint
- tears the isolate down when the call ends

That means top-level JavaScript module state does not persist across requests
today.

#### Design constraints

- isolates are thread-affine and cannot move between worker threads
- `RuntimeHostState` and `RuntimeCancellationState` are per-invocation state
  embedded in `OpState`
- timeout and external-cancellation termination can poison an isolate
- runtime-backed read-set tracing must continue to work
- top-level user-module state must remain per-invocation unless a later,
  explicitly documented contract change says otherwise

#### Implementation plan

1. Put the pool under the worker threads, not under `NeovexRuntime`.
   `RuntimeExecutor` already owns the long-lived worker threads and is the
   natural owner for thread-affine isolate reuse.

2. Pool a bootstrapped isolate shell in the first pass, not an already-evaluated
   user bundle.
   Reuse should cover the expensive runtime/bootstrap setup, but the user bundle
   should still be loaded per invocation so top-level module state does not leak
   across requests.

3. Introduce a worker-local pooled slot type, for example `PooledIsolate`,
   containing:
   - the bootstrapped `JsRuntime`
   - bundle identity compatibility metadata if needed
   - a poisoned flag or replacement marker

4. On checkout, reset all per-invocation state explicitly:
   - host bridge reference
   - cancellation signal and cancel handle
   - request-scoped JS context
   - any request-local state used by read-set tracing

5. On each invocation:
   - run `verify_integrity()`
   - load/evaluate the user bundle for that invocation
   - invoke the exported runtime entrypoint

6. On return, keep the isolate only if the invocation completed cleanly.
   If the isolate hit:
   - timeout
   - heap limit termination
   - external cancellation that terminates execution
   - any other unrecoverable V8 failure
   mark the slot poisoned and replace it.

7. If a later phase wants to reuse already-evaluated user bundles, it must add
   an explicit module-state reset mechanism or declare the contract change in
   docs and tests. That is out of scope for this phase.

8. Extend `RuntimeMetrics` with deterministic counters, for example:
   - `isolate_pool_hits`
   - `isolate_pool_misses`
   - `isolate_pool_replacements`

9. Surface those counters through the existing diagnostics endpoint rather than
   inventing a separate reporting path.

#### Files to change

- `crates/neovex-runtime/src/executor.rs`
- `crates/neovex-runtime/src/runtime.rs`
- `crates/neovex-runtime/src/metrics.rs`
- `crates/neovex-runtime/src/limits.rs` only if new config is actually needed

#### Existing tests to extend

- runtime invocation tests in `crates/neovex-runtime/src/runtime.rs`
- runtime executor tests in `crates/neovex-runtime/src/executor.rs`
- nested runtime tests in
  `crates/neovex-server/src/tests/convex_functions/runtime_queries/nested_runtime.rs`
- runtime cancellation tests in
  `crates/neovex-server/src/tests/convex_runtime/cancellation/request_drops/`

#### New tests

- verify the second invocation of the same bundle on the same worker records a
  pool hit
- verify timed-out isolates are replaced and do not get reused
- verify per-invocation auth, cancellation, and request state do not leak
  across pooled invocations
- add a runtime test bundle that mutates top-level global/module state and prove
  a second invocation still observes fresh first-invocation semantics
- verify runtime read-set tracing still returns correct snapshots after pooling

#### Acceptance criteria

- steady-state runtime invocations reuse a warmed isolate shell on the same
  worker
- top-level user-module state still does not persist across independent
  invocations
- cancellation, timeout, and heap-limit semantics remain correct
- pooled reuse is visible in diagnostics
- runtime read-set tracking remains correct

---

## Phase 3: Query And Subscription Optimization

### 3A. Narrow engine subscription invalidation conservatively

**Priority:** medium  
**Expected impact:** fewer unnecessary engine-side subscription re-evaluations.

#### Current verified state

- `SubscriptionRegistry::affected(...)` in
  `crates/neovex-engine/src/subscriptions.rs` matches subscriptions only by
  table name
- engine subscription fan-out in
  `crates/neovex-engine/src/service/mutations.rs` re-evaluates every affected
  subscription at table granularity
- runtime-backed named subscription re-evaluation already has a narrower skip
  path using `RuntimeReadSet`

#### Implementation plan

1. Scope this item to engine subscriptions only. Do not disturb the existing
   runtime read-set path except where shared helpers make sense.

2. Reuse the engine's filter matcher for invalidation checks.

3. Extend `process_commit(...)` so it can receive candidate document snapshots
   where available:
   - inserts: newly inserted document
   - deletes: deleted document snapshot already exists
   - updates: keep table-level behavior in the first pass unless storage update
     APIs are expanded to return old and/or new document snapshots

4. Narrow invalidation only when that can be done with zero false negatives:
   - no filters: always affected
   - insert/delete with a matching/non-matching document snapshot: narrow safely
   - ambiguous update: keep current behavior

5. Keep the existing runtime read-set machinery as the higher-fidelity path for
   runtime-backed named subscriptions.

#### Files to change

- `crates/neovex-engine/src/subscriptions.rs`
- `crates/neovex-engine/src/service/mutations.rs`
- optionally `crates/neovex-storage/src/store.rs` and
  `crates/neovex-storage/src/index.rs` if a follow-up expands update APIs to
  return document snapshots

#### Existing tests to extend

- `crates/neovex-engine/src/tests.rs`

#### New tests

- two subscriptions on the same table with different equality filters; insert a
  document that matches only one and verify only one is re-evaluated
- delete a document that matches only one subscription and verify the same
  narrowing works
- update behavior remains conservative in the first pass

#### Acceptance criteria

- insert/delete invalidation is narrower than table-level invalidation when a
  document snapshot makes that safe
- there are no false negatives
- runtime read-set-aware subscription behavior is unchanged

---

### 3B. Formalize query planning without violating evaluator purity

**Priority:** low  
**Expected impact:** easier testing and safer future extension of index
selection.

#### Current verified state

Index selection logic still lives inline in
`crates/neovex-engine/src/service/queries.rs`.

#### Important invariant

The evaluator is pure. Query planning may be pure, but plan execution that
touches storage must remain in the service layer, not `evaluator.rs`.

#### Implementation plan

1. Add a private `QueryPlan` enum in
   `crates/neovex-engine/src/service/queries.rs`.

2. Add a pure `plan_query(...)` helper that selects among:
   - full scan
   - exact index scan
   - range index scan
   - residual filters

3. Keep `execute_plan(...)` in the same service-layer file because it needs
   storage/index access and cancellation hooks.

4. Keep `evaluator.rs` responsible only for pure in-memory operations over
   `Vec<Document>` and pagination state.

5. Keep `QueryPlan` engine-private unless it later becomes a stable public type.

#### Files to change

- `crates/neovex-engine/src/service/queries.rs`

#### Existing tests to extend

- query planner and index behavior tests in `crates/neovex-engine/src/tests.rs`

#### New tests

- pure planner tests for exact vs range vs full-scan selection
- tests covering residual filter retention after index selection

#### Acceptance criteria

- plan selection is pure and directly unit-testable
- evaluator purity is preserved
- query results remain unchanged

---

### 3C. Add a per-tenant document cache in the engine

**Priority:** medium  
**Expected impact:** fewer repeated store reads and MessagePack decodes for hot
documents.

#### Current verified state

- `TenantRuntime` in `crates/neovex-engine/src/tenant.rs` does not cache reads
- engine query and get paths always read through `TenantStore`
- subscription re-evaluation re-reads the same documents repeatedly

#### Implementation plan

1. Add an engine-local, per-tenant document cache to `TenantRuntime`.

2. Keep the storage crate cache-unaware. This cache is an engine optimization,
   not a new storage layer.

3. Invalidate cache entries in `process_commit(...)` before subscription
   re-evaluation runs. That ordering matters: re-evaluation must not observe
   stale cache state.

4. Start with document caching, not query-result caching.

5. Populate the cache on:
   - `get_document(...)`
   - indexed lookups that already return specific documents
   - full query results after evaluation

6. Add deterministic hit/miss counters so usefulness is testable without timing.

#### Files to change

- `crates/neovex-engine/src/tenant.rs`
- `crates/neovex-engine/src/service/queries.rs`
- `crates/neovex-engine/src/service/mutations.rs`

#### Existing tests to extend

- `crates/neovex-engine/src/tests.rs`

#### New tests

- verify repeated `get_document(...)` calls record a cache hit
- verify a mutation invalidates the cached document before the next read
- verify subscription re-evaluation after a mutation sees fresh data

#### Acceptance criteria

- cached reads never return stale data
- hit/miss behavior is observable deterministically
- the cache remains tenant-scoped and engine-local

---

## Phase 4: Server-Layer Cleanup And Lifecycle Tightening

These items are lower priority than the data-path and runtime-path performance
work above. They are worth doing, but they should not preempt core path work.

### 4A. Consolidate repeated request concerns into middleware or extractors

**Priority:** low  
**Expected impact:** less repetitive handler code and fewer route-specific
slips.

#### Current verified state

- `RequestCancellationGuard` already exists in `crates/neovex-server/src/state.rs`
- Convex handlers already share `registry_and_auth(...)` in
  `crates/neovex-server/src/adapters/convex/handlers/common.rs`
- request cancellation, auth lookup, and usage recording are still repeated
  across several handler paths

#### Implementation plan

1. Treat this as consolidation work, not greenfield work.

2. Introduce typed request extractors and/or subtree middleware for the Convex
   router that provide:
   - verified `InvocationAuth`
   - the request-scoped cancellation token
   - the resolved `ConvexRegistry`

3. Keep usage recording tied to successful auth verification.

4. Compose Convex-only middleware in `crates/neovex-server/src/router.rs`, not
   `lib.rs`.

5. Keep generic Neovex-native routes separate. Do not accidentally require
   Convex auth middleware on routes that should remain auth-agnostic.

#### Files to change

- `crates/neovex-server/src/state.rs`
- `crates/neovex-server/src/router.rs`
- `crates/neovex-server/src/adapters/convex/handlers/`
- `crates/neovex-server/src/adapters/convex/http_actions/`

#### Existing tests to extend

- auth tests in `crates/neovex-server/src/tests/auth/`
- request-drop cancellation tests in
  `crates/neovex-server/src/tests/convex_runtime/cancellation/request_drops/`

#### New tests

- add an integration test proving the shared Convex middleware or extractor path
  preserves auth, usage-recording, and cancellation behavior across query,
  mutation, action, and `httpAction` routes
- add a regression test proving Neovex-native routes remain reachable without
  accidentally requiring Convex-specific middleware

#### Acceptance criteria

- Convex handlers stop re-implementing the same auth/cancellation setup logic
- route composition in `router.rs` makes middleware ownership obvious
- existing auth and cancellation behavior is unchanged

---

### 4B. Replace detached socket-child tasks with structured ownership

**Priority:** low  
**Expected impact:** cleaner teardown and fewer orphaned tasks.

#### Current verified state

Detached child tasks are spawned in several connection-scoped paths:

- generic WebSocket path:
  `crates/neovex-server/src/ws/socket.rs`
- Convex subscription forwarding:
  `crates/neovex-server/src/adapters/convex/subscriptions/socket/forwarding.rs`
- runtime subscription bridge:
  `crates/neovex-server/src/runtime/subscriptions.rs`

Top-level scheduler lifecycle is separate and owned by
`crates/neovex-bin/src/main.rs`.

#### Implementation plan

1. Use local structured concurrency for connection-scoped children.
   Prefer a `JoinSet`, paired cancellation token, or equivalent explicit parent
   ownership inside the socket/session handler.

2. Keep top-level scheduler lifecycle in the binary crate for now.

3. Audit the runtime subscription bridge as part of the same work.

4. Only introduce a process-wide `TaskTracker` if, after local cleanup, there
   are still detached server-wide tasks that genuinely outlive any single
   request or socket.

#### Files to change

- `crates/neovex-server/src/ws/socket.rs`
- `crates/neovex-server/src/adapters/convex/subscriptions/socket/forwarding.rs`
- `crates/neovex-server/src/runtime/subscriptions.rs`
- optionally `crates/neovex-bin/src/main.rs` for top-level lifecycle cleanup

#### Existing tests to extend

- reactive loop socket tests in `crates/neovex-server/tests/reactive_loop/`
- Convex subscription tests in `crates/neovex-server/src/tests/convex_functions/`

#### New tests

- add a connection-teardown test proving socket-scoped child tasks are canceled
  and joined when the parent session exits
- add a runtime-subscription-bridge teardown test proving child tasks do not
  outlive the owning session or socket handler

#### Acceptance criteria

- connection teardown owns and joins its child tasks deterministically
- there is no orphaned task class left hiding behind `tokio::spawn(...)` in
  socket/session code
- scheduler shutdown semantics remain correct

---

### 4C. Add observability export adapters without replacing `RuntimeMetrics`

**Priority:** low  
**Expected impact:** easier external monitoring without destabilizing the
current in-process metrics model.

#### Current verified state

- `RuntimeMetrics` is already wired through runtime, server, and diagnostics
- `/debug/runtime/metrics` already exposes runtime limits plus a metrics snapshot
- tracing spans already exist for async host calls

#### Implementation plan

1. Keep `RuntimeMetrics` as the low-overhead in-process source of truth.

2. If external export is needed, add an adapter layer that translates snapshots
   into Prometheus or OpenTelemetry metrics. Do not replace the internal metrics
   implementation in the first pass.

3. Reuse existing request-correlation ids so exported metrics and traces can be
   connected without a second correlation mechanism.

#### Files to change

- `crates/neovex-server/src/http/metadata.rs`
- new exporter module location if external export is added
- `crates/neovex-runtime/src/metrics.rs` only if a small export helper is needed

#### Existing tests to extend

- diagnostics endpoint tests in `crates/neovex-server/src/tests/`
- runtime metrics tests in
  `crates/neovex-server/src/adapters/convex/tests/metrics.rs`

#### New tests

- add an adapter unit test proving one `RuntimeMetrics` snapshot maps
  deterministically to the exported metric names, labels, and values
- if an export endpoint is added, add an integration test proving it reflects
  the same underlying counters exposed by `/debug/runtime/metrics`

#### Acceptance criteria

- the existing diagnostics endpoint continues to work unchanged
- any new exporter reads from the same canonical runtime metrics source

---

## Phase 5: Async Storage And Service Rewrite

This phase is fully governed by this document. It absorbs the remaining rewrite
work that was previously split into a standalone async plan.

### Phase 5 scope rules

- The first implementation keeps redb as the storage engine.
- The rewrite changes execution and cancellation semantics, not tenancy model.
- Every mutation still flows through `Service::apply_mutation(...)` or a single
  async successor.
- The runtime crate remains workspace-independent.
- Transport-layer disconnect behavior must not be confused with the
  engine/storage commit contract.

### 5A. Introduce async storage traits and migrate read paths first

**Priority:** highest within Phase 5  
**Expected impact:** replaces blocking wrapper futures with real async read
execution and real cancellation propagation.

#### Current verified state

- `Service::call_blocking(...)` and
  `Service::call_blocking_cancellable(...)` in
  `crates/neovex-engine/src/service/mod.rs` still wrap synchronous engine work
  in `spawn_blocking(...)`
- those wrappers are still used throughout service read and control paths:
  - `crates/neovex-engine/src/service/queries.rs`
  - `crates/neovex-engine/src/service/tenants.rs`
  - `crates/neovex-engine/src/service/schema.rs`
  - `crates/neovex-engine/src/service/scheduler.rs`
  - `crates/neovex-engine/src/service/subscriptions.rs`
  - `crates/neovex-engine/src/service/usage.rs`
- the underlying storage implementation is still synchronous and concrete:
  - `crates/neovex-storage/src/store.rs`
  - `crates/neovex-storage/src/index.rs`
  - `crates/neovex-storage/src/scheduler.rs`
  - `crates/neovex-storage/src/schema_store.rs`
  - `crates/neovex-storage/src/usage_store.rs`

#### Design rules

1. Introduce an explicit internal async trait hierarchy.
   The first version should include:
   - `StorageEngine`
   - `TenantReadStorage`
   - `TenantWriteStorage`
   - `TenantWriteTransaction`
   - `UsageStorage`

2. Keep the first implementation statically dispatched and redb-backed.
   Prefer native `async fn` trait methods and concrete wiring over early
   object-safety or plugin-style abstraction.

3. Preserve the tenant-vs-global split:
   - `StorageEngine` is global control-plane
   - `TenantReadStorage`, `TenantWriteStorage`, and
     `TenantWriteTransaction` are per-tenant data-plane
   - `UsageStorage` remains a separate global control-plane store

4. Keep read APIs cancelable throughout execution.
   The new async boundary must preserve the cooperative checkpoints already used
   in scan and index loops rather than burying them behind an uncancelable async
   wrapper.

5. Do not reopen the storage-backend choice in this phase.
   The concrete implementation is "redb behind an async execution boundary"
   first.

#### Implementation plan

1. Add internal async storage traits in `neovex-storage` or an adjacent
   storage-boundary module, using native async traits for internal boundaries.

2. Implement a redb-backed async adapter behind a dedicated async execution
   boundary.
   This may be an actor, dedicated executor, or similarly explicit ownership
   model, but the design must keep tenant ordering and cancellation behavior
   clear.

3. Migrate read APIs first:
   - point reads
   - query and pagination support reads
   - commit-log reads
   - latest-sequence reads
   - tenant open/load/list control paths
   - usage-store reads

4. Convert engine service methods used by the server/runtime read paths into
   real async methods that await the new storage boundary directly.

5. Preserve evaluator purity.
   The evaluator remains a pure in-memory component even after the service layer
   becomes async.

6. Keep temporary compatibility shims internal only and remove them once the
   relevant callers are migrated.

#### Files to change

- `crates/neovex-storage/src/`
- `crates/neovex-engine/src/service/mod.rs`
- `crates/neovex-engine/src/service/queries.rs`
- `crates/neovex-engine/src/service/tenants.rs`
- `crates/neovex-engine/src/service/schema.rs`
- `crates/neovex-engine/src/service/scheduler.rs`
- `crates/neovex-engine/src/service/subscriptions.rs`
- `crates/neovex-engine/src/service/usage.rs`

#### Existing tests to extend

- query and pagination tests in `crates/neovex-engine/src/tests.rs`
- storage read and scheduler tests in `crates/neovex-storage/src/tests.rs`
- request-drop and cancellation tests in
  `crates/neovex-server/src/tests/convex_runtime/cancellation/request_drops/`

#### New tests

- cancel an in-flight table scan and prove the read stops without finishing the
  full scan
- cancel an in-flight index scan and prove the same behavior
- verify a queued canceled read request never begins real storage execution
- verify async read-path results match the previous synchronous implementation
  for point reads, queries, pagination, commit-log reads, and latest-sequence
  reads

#### Acceptance criteria

- read paths no longer depend on `call_blocking(...)` or
  `call_blocking_cancellable(...)`
- cancellation propagates into real storage work for reads
- evaluator purity and existing read semantics are preserved
- the first async storage implementation still uses redb

---

### 5B. Introduce an explicit async transaction model and migrate write paths

**Priority:** highest within Phase 5  
**Expected impact:** gives writes a real async boundary, explicit commit
semantics, and correct cancellation behavior.

#### Current verified state

- write methods today inline `begin_write(...)`, mutate tables, append commit
  records, and commit inside synchronous service/storage calls
- scheduler and schema mutation paths also rely on synchronous storage plumbing
- the current bridge between async callers and writes is cancellation around a
  blocking task, not true async preemption

#### Design rules

1. Define an explicit durable commit point.
   Before commit, cancellation may abort the write. After the durable commit
   point, the engine/storage boundary must not surface `Cancelled`.

2. Distinguish engine/storage outcomes from transport outcomes.
   A post-commit engine result may be "committed" even if an HTTP or WebSocket
   client disconnects before receiving the response.

3. Preserve storage atomicity.
   Document write, index update, and commit-log append remain one atomic durable
   transaction.

4. Dropping an uncommitted transaction must roll it back or abort it.

5. Keep the single mutation path.
   `Service::apply_mutation(...)` stays the semantic owner of validation,
   indexing, commit-log append, and subscription fan-out, even if its internal
   implementation becomes async.

#### Implementation plan

1. Introduce `TenantWriteTransaction` with explicit async methods for:
   - document mutations
   - index maintenance
   - scheduler state transitions
   - schema and index rebuild mutations
   - commit
   - rollback or abort

2. Define a write outcome model that distinguishes at least:
   - canceled before commit
   - committed durably
   - failed before commit for non-cancellation reasons

3. Move mutation-path storage plumbing in
   `crates/neovex-engine/src/service/mutations.rs` onto that transaction model.

4. Move scheduler state transitions and schema/index rebuild writes onto the
   same async transaction model where they mutate durable state.

5. Make the commit point explicit in code comments, tracing, and tests so agents
   do not accidentally reintroduce post-commit `Cancelled` outcomes later.

6. Keep subscription fan-out behind the same visibility boundary:
   reactive updates only happen after the durable write is committed and visible
   to subsequent reads.

#### Files to change

- `crates/neovex-storage/src/store.rs`
- `crates/neovex-storage/src/index.rs`
- `crates/neovex-storage/src/scheduler.rs`
- `crates/neovex-storage/src/schema_store.rs`
- `crates/neovex-engine/src/service/mutations.rs`
- `crates/neovex-engine/src/service/scheduler.rs`
- `crates/neovex-engine/src/service/schema.rs`

#### Existing tests to extend

- mutation, scheduler, and schema tests in `crates/neovex-engine/src/tests.rs`
- storage atomicity tests in `crates/neovex-storage/src/tests.rs`

#### New tests

- cancel before commit and verify no durable document, index, or commit-log
  change is written
- trigger cancellation after the durable commit point and verify the engine
  returns a committed outcome instead of `Cancelled`
- add a transport-level integration test showing a disconnected client may fail
  to observe that committed outcome even though the write is durable
- verify scheduler claim, cancel, completion, and result recording remain atomic
  under the async transaction model
- verify schema replacement and index rebuild work remains bounded and does not
  leak partially committed state

#### Acceptance criteria

- writes have explicit pre-commit and post-commit semantics
- the engine/storage boundary never reports `Cancelled` after durable commit
- transport disconnect behavior remains correctly decoupled from durable commit
- document, index, and commit-log writes remain atomic

---

### 5C. Remove blocking adaptation layers and complete async server/runtime integration

**Priority:** high after 5A and 5B  
**Expected impact:** removes the last major blocking wrappers from the engine
and runtime host-call path.

#### Current verified state

- `execute_async_blocking_host_call(...)` in
  `crates/neovex-server/src/runtime/host_calls/async_calls.rs` still wraps host
  work in `spawn_blocking(...)`
- engine service methods still expose blocking-wrapper helpers in
  `crates/neovex-engine/src/service/mod.rs`
- tenant lifecycle still synchronously touches filesystem and store state in
  `crates/neovex-engine/src/service/tenants.rs`

#### Implementation plan

1. Convert server/runtime bridge code so async host operations await real async
   engine and storage futures directly.

2. Remove `Service::call_blocking(...)` and
   `Service::call_blocking_cancellable(...)` once all internal callers have been
   migrated.

3. Remove `execute_async_blocking_host_call(...)` and any remaining sync-to-async
   host-call bridge wrappers.

4. Rework tenant lifecycle around async store handles:
   - async tenant open/load
   - async-safe lifecycle guards
   - delete/shutdown behavior coordinated with in-flight async storage work

5. Keep top-level scheduler lifecycle ownership in `crates/neovex-bin/src/main.rs`,
   but make the scheduler call true async service/storage methods.

6. Update `ARCHITECTURE.md` once the server no longer treats the engine/storage
   subsystem as blocking work hidden behind Tokio wrappers.

#### Files to change

- `crates/neovex-engine/src/service/mod.rs`
- `crates/neovex-engine/src/service/tenants.rs`
- `crates/neovex-engine/src/scheduler.rs`
- `crates/neovex-server/src/runtime/host_calls/async_calls.rs`
- `crates/neovex-server/src/adapters/convex/`
- `crates/neovex-server/src/http/`
- `crates/neovex-bin/src/main.rs`
- `ARCHITECTURE.md`

#### Existing tests to extend

- direct HTTP query tests in `crates/neovex-server/src/tests/`
- runtime-backed request-drop tests in
  `crates/neovex-server/src/tests/convex_runtime/cancellation/request_drops/`
- tenant lifecycle tests in `crates/neovex-engine/src/tests.rs`

#### New tests

- verify runtime-backed request cancellation now cancels live async storage work
  instead of only canceling a blocking wrapper task
- verify direct HTTP read cancellation parity still holds after removing the
  blocking helpers
- verify tenant deletion still waits for in-flight work and new operations after
  deletion begin fail with `TenantNotFound`
- add a codebase-level grep/assertion test or review checklist item ensuring no
  hot-path `call_blocking(...)` or `execute_async_blocking_host_call(...)`
  remains

#### Acceptance criteria

- blocking adaptation helpers are removed from the engine and runtime host path
- server routes and runtime bridges call real async engine/storage futures
- tenant lifecycle semantics remain correct
- request cancellation now reaches real storage work on both direct and
  runtime-backed paths

---

## Phase 6: Durable Log And Storage Performance

These items bridge today's redb-backed mutation path and the longer-term
commit-log-centric architecture described in the research docs.

### 6A. Introduce a durable mutation journal with group commit

**Priority:** medium, after Phase 5  
**Expected impact:** lower write amplification from per-mutation fsync while
preserving durability.

#### Current verified state

- each write currently performs document/index mutation plus commit-log append
  in one redb transaction
- the current `CommitEntry` is not rich enough to replay document state because
  it does not contain document payloads
- subscription fan-out today happens after the redb transaction commits

#### Important invariant

This phase must not acknowledge a mutation or publish a reactive update from
state that is not durably ordered.

#### Implementation plan

1. Introduce a richer durable mutation record.
   The existing `CommitEntry` is not enough for replay. Before any WAL-style
   architecture can work, we need a record type that contains enough data to
   reconstruct writes.

2. Keep `Service::apply_mutation(...)` as the semantic entrypoint.

3. Append the richer mutation record durably before acknowledging the write.
   Group commit may batch multiple append requests into a single durable flush,
   but durability still comes before acknowledgment.

4. Materialize into redb document/index tables in strict commit order.

5. Decide on one read-consistency strategy before implementation:
   - overlay pending materialized records onto reads, or
   - block reads until the materializer has applied the acknowledged sequence

6. Keep subscription fan-out behind the same visibility boundary.

7. Add recovery logic so startup can replay durable-but-unapplied records.

#### Files to change

- `crates/neovex-core/src/mutation.rs` or a new adjacent durable-record module
- `crates/neovex-storage/src/`
- `crates/neovex-engine/src/service/mutations.rs`
- `crates/neovex-engine/src/service/queries.rs`

#### Existing tests to extend

- storage tests in `crates/neovex-storage/src/tests.rs`
- engine mutation and commit-log tests in `crates/neovex-engine/src/tests.rs`

#### New tests

- verify a write is not acknowledged before durable append
- verify recovery replays durable-but-unapplied records on startup
- verify read-your-own-writes semantics across the materialization boundary

#### Acceptance criteria

- there is no visibility of non-durable writes
- write acknowledgments are durable even when materialization is deferred
- startup recovery can reapply durable journal entries safely

---

### 6B. Promote the durable journal to the authoritative per-tenant history

**Priority:** low, after 6A  
**Expected impact:** replay, CDC, replication, and edge-sync foundation.

#### Current verified state

The commit log is still a transactional side effect, not the authoritative
source of replayable state.

#### Implementation plan

1. Promote the durable journal introduced in 6A to the canonical ordered history
   for each tenant.

2. Treat redb document/index tables as a materialized view maintained from that
   history.

3. Add replay and snapshot boundaries:
   - bootstrap from snapshot plus journal tail
   - rebuild from journal for verification
   - define compaction and snapshot cut points explicitly

4. Add CDC or streaming APIs only after replay and recovery semantics are solid.

5. Use consumer cursors and retention rules that align with the horizontal
   scaling spec's commit-log model.

#### Files to change

- `crates/neovex-core/src/mutation.rs` or the adjacent durable-record module
- `crates/neovex-storage/src/`
- `crates/neovex-engine/src/service/mutations.rs`
- `crates/neovex-engine/src/service/queries.rs`
- `ARCHITECTURE.md` once the durable journal becomes the documented canonical
  per-tenant history

#### Existing tests to extend

- storage tests in `crates/neovex-storage/src/tests.rs`
- engine mutation, replay, and commit-log tests in
  `crates/neovex-engine/src/tests.rs`

#### New tests

- rebuild state from snapshot plus journal and verify it matches live state
- verify journal consumers observe strictly ordered entries
- verify point-in-time replay to a chosen sequence number

#### Acceptance criteria

- durable journal order is the canonical order for downstream consumers
- document/index state can be rebuilt from journal plus snapshot boundaries
- CDC and future replication work have a concrete, testable foundation

---

## Phase 7: Multi-Tenant Runtime Hardening

### 7A. Enforce per-tenant executor fairness

**Priority:** medium, after 2A  
**Expected impact:** prevents one tenant from monopolizing runtime capacity.

#### Current verified state

- runtime concurrency is enforced primarily through the global isolate semaphore
  in `crates/neovex-runtime/src/limits.rs`
- queue wait and execution time are already tracked per tenant
- nested runtime calls can bypass the policy limit via
  `RuntimeConcurrencyMode::BypassPolicyLimit`

#### Implementation plan

1. Put fairness inside the runtime executor or runtime policy, not in HTTP
   handlers. This must apply regardless of whether work originated from:
   - runtime-backed HTTP
   - runtime-backed WebSocket bootstrap
   - Convex `httpAction`
   - any other top-level runtime entrypoint

2. Start with top-level tenant admission control:
   - per-tenant in-flight cap
   - per-tenant queue-depth cap

3. Treat nested runtime calls separately.
   The first pass should keep the current bypass semantics for nested `ctx.run*`
   calls to avoid self-deadlock, but metrics should still make nested load
   visible.

4. Add per-tenant rejection accounting to runtime metrics.

5. Map rejections to the transport at the boundary:
   - HTTP routes can return `429` or `503`
   - WebSocket bootstrap can send a structured error frame
   - internal runtime paths should return a typed runtime/core error

#### Files to change

- `crates/neovex-runtime/src/executor.rs`
- `crates/neovex-runtime/src/limits.rs`
- `crates/neovex-runtime/src/metrics.rs`
- `crates/neovex-server/src/adapters/convex/execution/`
- `crates/neovex-server/src/runtime/invocations/`

#### Existing tests to extend

- runtime executor tests in `crates/neovex-runtime/src/executor.rs`
- runtime metrics tests in
  `crates/neovex-server/src/adapters/convex/tests/metrics.rs`
- request-drop and runtime HTTP tests in
  `crates/neovex-server/src/tests/convex_runtime/`

#### New tests

- a two-tenant saturation test proving one tenant cannot starve another
- transport-level rejection tests for HTTP and WebSocket bootstrap
- metrics tests for per-tenant rejected invocation counts

#### Acceptance criteria

- no single tenant can consume all top-level executor capacity
- per-tenant rejection behavior is observable in diagnostics
- nested runtime behavior remains safe and non-deadlocking

---

### 7B. Add signed deployment provenance for runtime bundles

**Priority:** low  
**Expected impact:** stronger runtime bundle provenance and a safe foundation for
future immutable-artifact verification optimizations.

#### Current verified state

- runtime bundle loading currently relies on `.sha256` sidecars read in
  `crates/neovex-server/src/adapters/convex/registry/loading.rs`
- `RuntimeBundle::verify_integrity()` still performs content verification at
  invocation time

#### Prerequisite

This remains blocked on deploy identity and auth design.

#### Implementation plan

1. Introduce a separate deployment-provenance manifest type and file. Do not
   overload the existing Convex function manifest structures.

2. The provenance manifest should bind:
   - content hash
   - deployment identity
   - deployment timestamp or version
   - signature

3. Keep `RuntimeBundle::verify_integrity()` responsible for content-hash
   verification at invocation time for mutable path-backed bundles.

4. Move signature verification into the registry-loading path so the registry
   only hands normalized, verified bundle identity and digest data to the
   runtime.

5. If a later phase introduces immutable content-addressed bundle artifacts,
   that future work may build on this provenance layer. Do not silently fold
   that optimization into this item.

6. Allow explicit development-mode opt-out rather than silent fallback.

#### Files to change

- `crates/neovex-server/src/adapters/convex/registry/loading.rs`
- new deployment-provenance manifest module
- `crates/neovex-runtime/src/runtime.rs` only as needed to accept verified
  identity metadata

#### Existing tests to extend

- registry loading tests in
  `crates/neovex-server/src/tests/registry_and_license/registry.rs`
- runtime bundle integrity tests in `crates/neovex-runtime/src/runtime.rs`

#### New tests

- valid signature accepted
- tampered manifest rejected
- unsigned bundles rejected in production mode
- explicit development opt-out works only when configured

#### Acceptance criteria

- runtime bundle provenance is stronger than a writable sidecar hash alone
- local development remains possible with an explicit opt-out
- this work does not weaken current per-invocation content verification for
  mutable bundles

---

## Standing Architectural Guidelines

These are continuous review rules, not one-time deliverables.

1. Keep generated bundles and fixtures pinned to typed ops only.
   Do not reintroduce a generic host-call escape hatch.

2. Reject unknown operations explicitly.
   Maintain the current contract-error behavior in the Convex host bridge.

3. Preserve the tenant boundary on the hot path.
   No item in this roadmap should add cross-tenant coordination to mutation,
   query, subscription, or runtime execution paths.

4. Keep caches and journals subordinate to ordering rules.
   A cache is never authoritative. A journal is not authoritative until a later
   phase explicitly makes it so with replay, recovery, and durability
   semantics.

5. Do not weaken durability for speed.
   No best-effort write buffer should become visible to clients before it is
   durably ordered.

6. Keep the evaluator pure and the runtime crate independent.
   Those boundaries are part of the architecture, not just the current layout.

7. Keep transport semantics honest.
   "Committed" at the engine boundary does not guarantee an already-disconnected
   client observed the response.

---

## Dependency Graph

```text
Phase 1 (mostly independent):
  1A  1B  1C  1D  1E  1F  1G

Phase 2:
  2A depends on 1A and benefits from 1B

Phase 3:
  3A independent
  3B independent
  3C benefits from 3A and 3B

Phase 4:
  4A independent
  4B independent
  4C optional and independent

Phase 5:
  5A -> 5B -> 5C
  5 benefits from 1D and 1F

Phase 6:
  6A depends on Phase 5
  6B depends on 6A

Phase 7:
  7A depends on the runtime executor shape from 2A
  7B remains blocked on deploy identity/auth
```

---

## Recommended Delivery Order

1. 1A, 1B, 1C
2. 1D, 1E, 1F, 1G
3. 2A
4. 7A
5. 3A
6. 3B
7. 3C
8. 4A and 4B if handler/lifecycle pain is still active
9. 5A
10. 5B
11. 5C
12. 6A
13. 6B
14. 7B when deploy identity exists
