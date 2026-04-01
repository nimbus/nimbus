# Performance And Architecture Master Plan

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
- `docs/research/tigerbeetle-code-reference.md`
- redb design docs and README for copy-on-write MVCC and
  single-writer/multi-reader behavior
- TigerBeetle safety docs for source-of-truth WAL discipline, recovery, and
  deterministic durability testing
- OpenRaft `RaftLogStorage` docs for append, flush-callback, and no-hole log
  invariants
- RocksDB and Fjall WAL/journal docs for storage-engine-internal log and
  materialization behavior
- Convex OCC docs for read-set-oriented transaction and invalidation direction
- Electric Postgres Sync and Shapes docs for log-driven fan-out and embedded
  replica direction
- PostgREST schema-isolation docs for schema-owned generated API boundaries
- Hasura permissions docs for role-aware declarative access policy over a
  generated API surface
- Wasmtime and WebAssembly Component Model docs for a typed, capability-scoped
  Rust-native plugin runtime
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

## Codex Execution Protocol

This section turns the roadmap into a durable control plane for Codex-style
agent execution across long runs, handoffs, and context compactions.

For autonomous implementation, the source of truth is:

1. the current git worktree
2. this roadmap's status ledger and execution log
3. the referenced architecture docs

The source of truth is not the prior chat transcript.

### Status model

- `todo`: not started; eligible when hard dependencies are `done` and any gate
  note is satisfied
- `in_progress`: actively being implemented; for a single autonomous Codex run,
  keep exactly one roadmap item in this state at a time
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and the required verification has been
  recorded in the execution log
- `deferred`: intentionally parked behind a product, platform, or operator gate
  and not eligible for autonomous pickup yet

### Recovery loop for every new session or post-compaction resume

1. Reread `Canonical Plan Rules`, this section, `Roadmap Status Ledger`,
   `Execution Log`, `Dependency Graph`, and `Recommended Delivery Order`.
   Inspect the current git worktree state too.

2. If any roadmap item is already `in_progress`, resume that item first.
   Do not start a new `todo` item while any older item remains
   `in_progress`.

3. If the git worktree is dirty, reconcile it before choosing new scope.
   Identify which roadmap item owns the current changes, then update that
   item's status, `Implementation checkpoint`, and `Execution Log` if the
   roadmap is stale relative to the code.

4. If there is no active `in_progress` item after reconciliation, pick the
   first eligible item in `Recommended Delivery Order` whose status is `todo`,
   whose hard dependencies are `done`, and whose gate note is already
   satisfied in this document.
   When one delivery-order row lists multiple items, a single autonomous Codex
   run should process them in lexical item order unless this roadmap later
   records a different explicit batch plan.

5. Read the full section for that item and only the immediately relevant code,
   tests, and architecture references needed to implement it correctly.

6. Mark the item `in_progress` before or at the same time as the first
   implementation patch for that item.

7. Implement exactly one roadmap item by default.
   Only batch multiple items when this roadmap explicitly groups them as a safe
   batch and the work can still satisfy every listed acceptance criterion
   without creating partial semantics.

8. Run the targeted verification for the touched crates and then the repo-level
   checks required by `Execution And Verification Contract`.

9. Update `Roadmap Status Ledger`, `Implementation checkpoint` if needed, and
   `Execution Log` in the same change set as the code.

10. Continue to the next eligible item.
    If blocked, record the blocker in this document before stopping.

### Dirty-worktree reconciliation rules

- A dirty worktree outranks remembered intent.
  If code and roadmap disagree, reconcile the roadmap to the actual partial
  implementation before picking a new item.
- If changed files clearly belong to one roadmap item, resume that item and
  keep it `in_progress` until it is either `done` or explicitly `blocked`.
- If changed files span multiple roadmap items unexpectedly, stop, record the
  conflict in `Execution Log`, and add or update `Implementation checkpoint`
  notes before continuing.
- Do not treat unstaged or uncommitted changes as disposable scratch state.
  They are part of the durable execution state until reconciled.

### Compaction and handoff safety rules

- Do not wait for an explicit compaction warning to checkpoint progress.
  The roadmap must be durable even if compaction or interruption happens
  without advance notice.
- Before ending a work burst, handing off, or leaving an item partially
  implemented, write back the current status, partial progress, and remaining
  steps to this roadmap.
- If the worktree materially diverges from the last recorded roadmap state,
  update the roadmap before doing more speculative work.
- Prefer small, frequent write-backs over large undocumented stretches of work.

### Non-deviation rules

- Do not skip an existing `in_progress` item to start a later `todo` item.
- Do not pick a new roadmap item from the queue while the worktree is dirty and
  not yet reconciled to an owning roadmap item.
- Do not skip ahead to a later eligible item while an earlier eligible item is
  still `todo`, unless this roadmap explicitly marks the later item as safe to
  parallelize.
- Do not reinterpret an item's goal on the fly. If implementation reveals a
  better approach, amend the roadmap item or add a scoped note before changing
  the intended behavior.
- Do not mark an item `done` until the listed acceptance criteria are met and
  the executed verification commands are written down in `Execution Log`.
- Do not rely on remembered progress from earlier chat turns. Reconstruct state
  from this file and the worktree every time.
- If one roadmap item turns out to require multiple sessions or PRs, keep the
  item `in_progress` and add a short `Implementation checkpoint` subsection
  directly under that item rather than creating a competing plan document.

### Required write-back after each work session

- update the item's status in `Roadmap Status Ledger`
- append a row to `Execution Log` with the date, item id, outcome, verification
  run, and any remaining follow-up
- if the session changes architecture-level behavior, update `ARCHITECTURE.md`
  in the same PR
- if the session leaves an item partially complete, record the remaining
  sub-steps under that item's `Implementation checkpoint` subsection
- if compaction, interruption, or handoff seems likely, checkpoint before
  stopping rather than after

### Recommended autonomous prompt

Use this exact loop for long-running Codex execution:

```text
Use docs/plans/performance-and-architecture-plan.md as the control plane.
Reread the Codex Execution Protocol, Roadmap Status Ledger, Execution Log,
Dependency Graph, Recommended Delivery Order, and current git worktree state.
If any item is in_progress, resume it first. Reconcile dirty worktree changes
to the owning roadmap item before picking new scope. Implement exactly one
roadmap item, run the required verification, update the ledger, checkpoint,
and execution log, and then continue. If blocked, record the blocker in the
roadmap before stopping. Do not rely on chat history as progress state.
```

---

## Roadmap Status Ledger

Update this ledger in every PR or work session that materially advances,
blocks, or completes a roadmap item.

Hard dependencies only are listed here. Soft "benefits from" relationships stay
in the item text and dependency graph.

### Active sequence

These items are eligible for autonomous execution when their listed hard
dependencies are `done`.

| Item | Status | Hard dependencies | Gate or unblock note |
| --- | --- | --- | --- |
| 1A | done | none | completed 2026-04-01 |
| 1B | done | none | completed 2026-04-01 |
| 1C | done | none | completed 2026-04-01 |
| 1D | done | none | completed 2026-04-01 |
| 1E | done | none | completed 2026-04-01 |
| 1F | done | none | completed 2026-04-01 |
| 1G | done | none | completed 2026-04-01 |
| 2A | done | 1A | completed 2026-04-01 |
| 7A | done | 2A | completed 2026-04-01 |
| 3A | done | none | completed 2026-04-01 |
| 3B | done | none | completed 2026-04-01 |
| 3C | done | none | completed 2026-04-01 |
| 3D | done | 3A, 3B | completed 2026-04-01 |
| 3E | done | 3B | completed 2026-04-01 |
| 4D | done | none | completed 2026-04-01 |
| 3F | done | 3D, 3E | completed 2026-04-01 |
| 5A | todo | none | async rewrite start |
| 5B | todo | 5A | write-path transaction model |
| 5C | todo | 5A, 5B | remove blocking wrappers after async read/write migration |
| 6A | todo | 4D, 5A, 5B, 5C | durable journal begins after Phase 5 and deterministic seam groundwork |
| 6B | todo | 6A | promote journal after 6A |
| 8A | todo | 6B | external journal streaming after authoritative journal |
| 8B | todo | 8A | embedded replica path after streaming path |
| 9A | todo | 4D, 6B | shadow materializer after simulation seams and authoritative journal |
| 9B | todo | 9A | robustness testing after shadow materializer |

### Conditional and gated items

These items stay out of the autonomous queue until their gate note is updated
and their status is explicitly changed away from `deferred` or `blocked`.

| Item | Status | Hard dependencies | Gate or unblock note |
| --- | --- | --- | --- |
| 4A | deferred | none | promote only if request-concern duplication is still causing active handler pain |
| 4B | deferred | none | promote only if socket lifecycle ownership remains an active maintenance problem |
| 4C | deferred | none | promote only if external metrics export becomes a concrete requirement |
| 7B | blocked | none | requires deploy identity and signing/auth design to exist first |
| 10A | deferred | 3F, 6B | promote only if snapshot or historical reads become a product requirement |
| 11A | deferred | 3B, 3E | promote only if the Neovex-native generated API becomes a product priority |
| 11B | deferred | 11A | promote only if a typed WASM plugin ABI becomes a product priority |

### Status transition rules

- `todo` -> `in_progress` when an agent starts active implementation
- `in_progress` -> `done` only after acceptance criteria and verification are
  satisfied
- `todo` or `in_progress` -> `blocked` when work cannot continue without a
  resolved external dependency or plan amendment
- `deferred` -> `todo` only by an explicit roadmap edit that records why the
  gate is now open
- `done` items stay `done`; if follow-up work appears, add a new roadmap item
  or checkpoint note instead of silently reopening completed scope

---

## Execution Log

Append new rows at the top of this table. Keep entries short and factual so a
future Codex run can reconstruct progress without chat history.

| Date | Item | Outcome | Summary | Verification | Follow-up |
| --- | --- | --- | --- | --- | --- |
| 2026-04-01 | 4D | done | Added first-class deterministic simulation seams in `neovex-storage` for clocks and named fault points, threaded them through `TenantStore` and `Service` via `*_with_simulation(...)` constructors, guarded storage commit visibility with reproducible injected-fault hooks, added shared deterministic harness support in `neovex-test-support`, and extended storage plus engine tests to prove seeded fault reproducibility and manual-clock scheduler execution without wall-clock sleeps. | `cargo check -p neovex-storage -p neovex-engine -p neovex-test-support`; `cargo test -p neovex-storage injected_fault_before_visibility_rolls_back_the_write_deterministically`; `cargo test -p neovex-storage seeded_fault_injector_reproduces_the_same_schedule_for_the_same_seed`; `cargo test -p neovex-storage`; `cargo test -p neovex-engine manual_clock_advances_scheduled_work_without_wall_clock_sleep`; `cargo test -p neovex-engine scheduled_mutation_executes_and_triggers_reactive_update`; `cargo test -p neovex-engine`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | next eligible item is 5A |
| 2026-04-01 | 3F | done | Added a stable-snapshot `MutationExecutionUnit` in the engine, layered serializable OCC validation over the shared dependency model, reused the same dependency vocabulary for runtime read tracking and conflict checks, routed runtime mutation `ctx.db.*`, `ctx.scheduler.*`, and direct `ctx.runQuery`/`ctx.runMutation` paths through staged execution-unit state, and committed staged document plus scheduler writes atomically in one redb transaction with bridge and engine regressions for conflicts, authorization-shaped visibility, read-your-own-writes, and scheduler-side-effect rollback on conflict. | `cargo test -p neovex-storage`; `cargo test -p neovex-engine mutation_execution_unit`; `cargo test -p neovex-engine`; `cargo test -p neovex-server read_tracking::tests::`; `cargo test -p neovex-server adapters::convex::tests::authorization::`; `cargo test -p neovex-server convex_named_mutation_can_use_bootstrapped_ctx_scheduler_api`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | next eligible item is 4D |
| 2026-04-01 | 3E | done | Added declarative principal and access-policy types in `neovex-core`, enforced planner-aware authorization in engine reads and atomic authorization in writes, stored principal snapshots plus policy revisions on subscriptions, threaded normalized Convex auth through runtime and handler entrypoints, and added engine plus server coverage for filtered reads, denied writes, policy-change teardown, auth-change teardown, and runtime non-bypass behavior. | `cargo test -p neovex-engine`; `cargo test -p neovex-server tests::auth::`; `cargo test -p neovex-server adapters::convex::tests::authorization::`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | next eligible item is 3F |
| 2026-04-01 | 3E | checkpointed | Added declarative principal and access-policy types in `neovex-core`, threaded normalized principals through engine query, mutation, subscription, and Convex runtime paths, stored principal snapshots plus policy revisions on subscriptions, and made policy or auth-context changes invalidate cached or live views conservatively. | `cargo check --workspace` | Add targeted engine and server auth tests, rerun roadmap-required verification, and then mark 3E done |
| 2026-04-01 | 3D | done | Introduced a shared `DependencySet` model plus commit-intersection helper in `neovex-core`, moved engine subscriptions onto that normalized dependency vocabulary, translated runtime `RuntimeReadSet` values into the same shared form for re-evaluation skip checks, and added core, engine, and runtime tests for shared dependency matching and coarse engine fallback behavior. | `cargo test -p neovex-core`; `cargo test -p neovex-engine`; `cargo test -p neovex-server runtime_read_set_converts_to_shared_dependency_set_without_losing_skip_behavior`; `cargo test -p neovex-server convex_runtime_nested_query_subscription_tracks_inner_runtime_reads`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | 3E is now in progress |
| 2026-04-01 | 3C | done | Added a tenant-local document cache to `TenantRuntime`, invalidated cache entries before subscription re-evaluation on every committed mutation, populated cached documents from `get_document(...)`, indexed lookups, and evaluated query results, and added deterministic hit or miss coverage for repeated gets, mutation invalidation, and subscription refreshes. | `cargo test -p neovex-engine`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | 3D is now in progress |
| 2026-04-01 | 3B | done | Extracted pure query planning into a private `QueryPlan` plus `plan_query(...)` helper in `service/queries.rs`, kept storage access in service-layer execution helpers, preserved paginated query shape while using residual filters for non-paginated exact-index scans, and added direct planner tests for full-scan, exact-index, range-index, and residual-filter behavior. | `cargo test -p neovex-engine`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | 3C is now in progress |
| 2026-04-01 | 3A | done | Narrowed engine-side subscription invalidation for insert and delete commits by reusing the evaluator filter matcher against candidate document snapshots, kept updates conservatively table-level to avoid false negatives, and added targeted subscription tests for matching inserts, matching deletes, conservative updates, and the indexed re-evaluation path. | `cargo test -p neovex-engine`; `cargo test -p neovex-server convex_runtime_nested_query_subscription_tracks_inner_runtime_reads`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | 3B is now in progress |
| 2026-04-01 | 7A | done | Added per-tenant top-level executor admission control with in-flight and queue-depth caps, recorded rejected invocation metrics globally and per tenant, mapped queue-limit failures to typed core/runtime errors plus `429` HTTP and structured WebSocket bootstrap errors, and extended runtime plus server coverage for fairness, rejection accounting, and cancellation safety. | `cargo test -p neovex-runtime`; `cargo test -p neovex-server convex_runtime_http_rejections_return_too_many_requests`; `cargo test -p neovex-server convex_runtime_websocket_bootstrap_rejections_send_error_frames`; `cargo test -p neovex-server runtime_metrics_route_returns_limits_and_metrics_when_convex_support_is_enabled`; `cargo test -p neovex-server runtime_metrics_snapshot_surfaces_rejected_invocation_counts`; `cargo test -p neovex-server convex_runtime_only_query_reuses_same_isolate_for_ctx_run_query`; `cargo test -p neovex-server convex_runtime_only_query_enforces_nested_runtime_budget`; `cargo test -p neovex-server dropped_runtime_http_request_cancels_runtime_invocation`; `cargo test -p neovex-server dropped_queued_runtime_request_never_starts_mutation`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | 3A followed next in the same work session |
| 2026-04-01 | 2A | done | Added worker-local warmed runtime shells via bootstrap startup snapshots owned by `RuntimeExecutor` workers, preserved per-invocation bundle/auth/session semantics by reinitializing runtime state and reloading user bundles each call, surfaced deterministic isolate pool counters in runtime diagnostics, and extended runtime plus server coverage for reuse, replacement on cancellation/timeout, nested dispatch, and runtime read-set tracking. | `cargo test -p neovex-runtime`; `cargo test -p neovex-server convex_runtime_only_query_reuses_same_isolate_for_ctx_run_query`; `cargo test -p neovex-server dropped_runtime_http_request_cancels_runtime_invocation`; `cargo test -p neovex-server dropped_queued_runtime_request_never_starts_mutation`; `cargo test -p neovex-server convex_runtime_nested_query_subscription_tracks_inner_runtime_reads`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | 7A is now in progress |
| 2026-04-01 | 1G | done | Added drop-based subscription cleanup handles while preserving stable numeric ids, threaded those handles through generic, Convex, and runtime-backed websocket ownership paths, and added engine plus reactive-loop coverage for drop-based unregister on disconnect without an explicit unsubscribe message. | `cargo test -p neovex-engine`; `cargo test -p neovex-server websocket_disconnect_drops_subscription_without_explicit_unsubscribe`; `cargo test -p neovex-server`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | next eligible item is 2A |
| 2026-04-01 | 1F | done | Replaced the scheduler's unconditional interval polling with a next-due sleep loop across loaded tenants, added storage and service helpers for earliest scheduled or cron work, and introduced `Notify`-based wakeups so newly scheduled earlier work resumes promptly without changing bin-owned shutdown behavior. | `cargo test -p neovex-storage`; `cargo test -p neovex-engine`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | next eligible item is 1G |
| 2026-04-01 | 1E | done | Routed full-scan fallback query and pagination evaluation through a row-at-a-time storage scan that only materializes matching documents, while preserving existing sort, cursor, and limit semantics and explicitly avoiding any partial MessagePack decoding claims. | `cargo test -p neovex-engine`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | next eligible item is 1F |
| 2026-04-01 | 1D | done | Deduplicated direct and scheduled mutation planning behind shared per-variant helpers in `service/mutations.rs`, keeping delete snapshot handling explicit while leaving the public direct and scheduled entrypoints as thin wrappers over the shared execution-mode path. | `cargo test -p neovex-engine`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | next eligible item is 1E |
| 2026-04-01 | 1C | done | Relaxed diagnostic-only atomic counters in `RuntimeMetrics`, documented why relaxed ordering is safe there, and expanded snapshot assertions while leaving `HostCallCancellationState` on `SeqCst`. | `rg -n "Ordering::SeqCst" crates/neovex-runtime/src/metrics.rs crates/neovex-runtime/src/host.rs`; `cargo test -p neovex-runtime metrics`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | next eligible item is 1D |
| 2026-04-01 | 1B | done | Moved `RuntimeBundle` onto shared internal state, added a stable canonical-path-plus-digest bundle identity for pooling/provenance bookkeeping, and kept path-backed bundles on strict per-invocation SHA-256 verification with regression tests for clone identity, tamper-after-success detection, and canonical-path normalization. | `cargo test -p neovex-runtime runtime_bundle`; `cargo test -p neovex-runtime runtime_rejects_bundle_integrity_mismatch`; `cargo test -p neovex-server convex_registry_requires_runtime_bundle_hash_sidecar`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | next eligible item is 1C |
| 2026-04-01 | 1A | done | Reused one Tokio current-thread runtime per runtime worker, replaced the blocking entrypoint's async bridge with blocking worker submission/result handling, and added deterministic executor tests for worker-runtime reuse and sync entrypoint coverage. | `cargo test -p neovex-runtime executor`; `cargo test -p neovex-server convex_runtime::cancellation::request_drops`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | next eligible item is 1B |
| 2026-04-01 | meta | refined | Tightened the Codex protocol so resumed runs must reconcile dirty worktrees, resume `in_progress` items first, and checkpoint roadmap state before likely compaction, interruption, or handoff. | document review | keep using the roadmap as the durable progress log during implementation |
| 2026-04-01 | meta | documented | Added Codex execution protocol, status ledger, status transitions, and execution log so the roadmap can survive autonomous compactions and handoffs. | document review | start updating this log when code work begins |

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
   behind an async boundary. The async boundary and later durable journal should
   preserve room for a future write-optimized layer or alternate storage engine,
   and should avoid baking redb page-layout assumptions into higher-level
   contracts, but backend replacement is a separate future project.

7. Explicit commit semantics matter more than transport delivery.
   Engine and storage code must define a durable commit point. Before that
   point, cancellation may abort the write. After that point, the engine/storage
   boundary must not surface `Cancelled`. However, an HTTP or WebSocket client
   that disconnects after commit still may not observe the success response.
   When Phase 6 separates durable ordering from later materialization, serving
   reads should wait for the required applied sequence rather than overlay
   journal-only records onto the read path unless a later roadmap phase
   explicitly changes that contract.

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

11. Dependency tracking should converge, not fork further.
    Near-term heuristics may differ between engine and runtime paths, but the
    roadmap should move toward one normalized dependency-set model that can feed
    invalidation, replay consumers, and future OCC-style read/write-set work.

12. If Neovex adds a custom journal-driven materializer, it should favor
    deterministic compaction and replay semantics inspired by TigerBeetle.
    Compaction and recovery behavior should be driven by journal state,
    checkpoint state, and explicit configuration rather than wall-clock timing
    or nondeterministic scheduling. The first implementation should run in
    shadow mode against redb before it is allowed onto any serving path.

13. Authentication and authorization must stay separate.
    Authentication is transport- and provider-specific and should stay at the
    server or adapter boundary. Authorization must move into the engine and
    planner so reads, writes, subscriptions, and runtime host calls cannot
    bypass policy by using a different route shape.

14. Runtime compatibility and database-native extensibility are different
    goals.
    The current V8 and `deno_core` runtime remains first-class for Convex
    compatibility and JavaScript portability. Future schema-generated CRUD and
    WASM plugin support should complement that path, not silently replace it.

15. Deterministic simulation must expand with durability-critical features.
    The database already exists, so the roadmap cannot literally "build the
    simulator first". From this point forward, new journal, materializer, and
    transaction work should add swappable time, storage, and fault-injection
    seams before it is trusted on any serving path.

---

## Research Recommendation Status

This section records how the research guide maps onto Neovex's actual project
decisions so agents do not have to infer intent from multiple documents.

### Adopted in this roadmap

1. Start with `redb`, not an early storage-engine swap.

2. Use full re-evaluation plus dependency tracking as the correctness path
   before more advanced invalidation or materialization work.

3. Build one Neovex-owned logical durable journal that can later serve replay,
   invalidation, CDC, and replica consumers.

4. Keep the server authoritative for v1 sync while preserving a later path to
   streaming and embedded replicas.

5. Treat TigerBeetle as the primary reference for durability discipline,
   checkpoint plus replay rebuilds, deterministic compaction, and harsh
   robustness testing.

### Deferred but explicitly scheduled here

1. Planner-enforced declarative authorization is scheduled in Phase `3E`.
   Adapter-layer authentication remains, but authorization stops living in
   route-specific code.

2. Serializable OCC-style read/write-set validation is scheduled in Phase
   `3F`, built on the shared dependency model from Phase `3D`.

3. Deterministic simulation seams for time, storage, and injected faults are
   scheduled in Phase `4D` so later journal and materializer work can be
   verified under controlled failures.

4. MVCC-style snapshot and time-travel reads remain a later extension after OCC
   is in place. This roadmap preserves room for that layer, but does not make
   it a near-term implementation target.

5. A Neovex-native schema-generated API and typed WASM plugin ABI are scheduled
   in Phase `11`, using battle-tested patterns from PostgREST, Hasura, and
   Wasmtime while staying additive to the Convex compatibility surface.

### Intentional deviations from the research guide

1. Neovex will not replace the V8 runtime with WASM in this roadmap.
   The research guide's "schema-generated CRUD plus WASM plugins" direction is
   still valuable, but this project deliberately keeps V8 and `deno_core` as a
   first-class execution surface for Convex compatibility. If WASM is added, it
   will be a complementary database-native extension path rather than a forced
   migration off the compatibility runtime. Phase `11` is where that additive
   path should be made concrete.

2. TigerBeetle is not the primary reference for every layer.
   It is the closest implementation reference for ordered durability,
   materialization, recovery, and robustness testing. Query-engine
   authorization, schema-driven API generation, and OCC semantics should still
   follow the research guide's Postgres, Firebase Rules, Convex, and
   FoundationDB references.

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

## Phase 3: Query, Subscription, And Policy Model

These items tighten the long-term query, subscription, authorization, and
transaction substrate before the deeper async and journal work.

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

### 3D. Introduce a shared dependency-set model for engine and runtime subscriptions

**Priority:** medium  
**Expected impact:** aligns subscription invalidation with the research target
and provides the substrate for journal-driven matching later.

#### Current verified state

- engine subscriptions currently rely on table-level matching in
  `crates/neovex-engine/src/subscriptions.rs`, with Phase 3A only narrowing that
  path conservatively
- runtime-backed named subscriptions already capture `RuntimeReadSet` and use
  `commit_intersects_runtime_read_set(...)` in the server crate
- there is no single normalized dependency-set type shared across engine and
  runtime subscription paths

#### Implementation plan

1. Introduce a normalized dependency-set model in a workspace-shared layer,
   `neovex-core`, that can represent current coarse-to-fine
   dependency forms such as:
   - table-level reads
   - document-level reads
   - index-range or predicate reads
   - paginated or ordered window reads where needed by the current runtime path

2. Keep coarse dependencies valid.
   Table-level tracking remains a legal fallback representation; the first pass
   does not need perfect precision everywhere.

3. Teach engine subscription registration and re-evaluation paths to store this
   normalized dependency-set form rather than relying on ad-hoc table-only
   matching forever.

4. Teach runtime-backed subscription re-evaluation to translate
   `RuntimeReadSet` into the same normalized dependency-set model rather than
   treating runtime invalidation as a permanently separate subsystem.

5. Add shared helpers that match a commit or future durable-journal write set
   against a normalized dependency set.

6. Shape the model so it can later support OCC-style read/write-set validation
   and possible MVCC-aware dependency work, even though transaction retry or
   validation semantics remain out of scope for this phase.

#### Files to change

- `crates/neovex-core/src/` for the shared dependency-set types
- `crates/neovex-engine/src/subscriptions.rs`
- `crates/neovex-engine/src/service/subscriptions.rs`
- `crates/neovex-engine/src/service/mutations.rs`
- `crates/neovex-server/src/runtime/read_tracking/`
- `crates/neovex-server/src/adapters/convex/subscriptions/transforms/runtime/`

#### Existing tests to extend

- engine subscription tests in `crates/neovex-engine/src/tests.rs`
- runtime read-tracking tests in `crates/neovex-server/src/runtime/read_tracking/tests.rs`
- Convex reactive subscription tests in `crates/neovex-server/tests/reactive_loop.rs`

#### New tests

- add shared intersection tests for table, document, index-range, and paginated
  window dependencies
- add a test proving runtime-backed subscription tracking can be converted into
  the shared dependency-set form without losing current skip behavior
- add a test proving engine subscriptions can store coarse table-level
  dependencies in the same normalized model when finer-grained tracking is not
  yet available

#### Acceptance criteria

- engine and runtime subscriptions no longer rely on permanently separate
  invalidation models
- coarse table-level tracking remains a valid fallback
- the normalized dependency-set model is usable by later journal-driven
  invalidation work

### 3E. Move authorization into the query engine and planner while keeping authentication at the boundary

**Priority:** high  
**Expected impact:** closes a major research gap by making data access policy
part of the query path instead of a route-specific convention.

#### Current verified state

- the current codebase authenticates Convex requests in
  `crates/neovex-server/src/adapters/convex/auth/` and passes
  `InvocationAuth` into runtime handlers
- Neovex-native routes do not currently prescribe a built-in authentication or
  authorization model
- there is no schema-level declarative authorization layer enforced inside
  engine reads, writes, or subscription re-evaluation
- active subscriptions and engine caches do not yet define what happens when
  policy definitions change or when a principal's claims snapshot changes

#### Implementation plan

1. Make the architecture boundary explicit.
   Authentication remains a server or adapter responsibility that validates
   credentials and normalizes them into a principal context. Authorization
   becomes an engine and planner responsibility that cannot be bypassed by
   route shape.

2. Introduce declarative access-policy types directly in `neovex-core` so
   policy can be defined alongside schema rather than inside ad hoc handler
   code.

3. Start with a constrained rule model.
   The first pass should support policy predicates over:
   - the authenticated principal
   - the candidate document
   - for writes, the existing document where relevant
   Avoid arbitrary user code as the enforcement mechanism.

4. Teach the query planning and evaluation path to incorporate authorization
   rules before results are returned. A policy may compile into planner filters,
   residual evaluator predicates, or both, but it must not be a best-effort
   post-filter layered outside the engine.

5. Teach the mutation path to enforce create, update, and delete authorization
   atomically with validation and commit.

6. Route all subscription re-evaluation and runtime host calls through the same
   authorization-aware engine entrypoints so Convex compatibility paths and
   native paths see the same policy outcomes.

7. Make policy and identity changes explicit invalidation inputs.
   The first implementation does not need fine-grained live reauthorization.
   It does need a safe rule: if a policy revision changes or if the principal
   context for a live subscription changes, the engine must conservatively
   re-evaluate or terminate the affected subscription and invalidate any
   affected cache entries rather than continuing to serve previously authorized
   results.

8. Record enough auth context to avoid painting the architecture into a corner.
   Subscription registrations, cache keys, or adjacent metadata should carry a
   principal snapshot and policy revision identifier so future finer-grained
   auth invalidation can be implemented without redesigning the registry model.

9. Keep the Convex auth shim as the compatibility-layer authenticator.
   Its job is identity verification and normalization, not owning long-term
   authorization semantics.

#### Files to change

- `crates/neovex-core/src/schema.rs`
- `crates/neovex-core/src/` for new principal and policy types
- `crates/neovex-engine/src/service/queries.rs`
- `crates/neovex-engine/src/service/mutations.rs`
- `crates/neovex-engine/src/service/subscriptions.rs`
- `crates/neovex-engine/src/subscriptions.rs`
- `crates/neovex-engine/src/tenant.rs`
- `crates/neovex-engine/src/evaluator.rs`
- `crates/neovex-server/src/adapters/convex/auth/`
- `crates/neovex-server/src/adapters/convex/host_bridge/`
- `ARCHITECTURE.md`

#### Existing tests to extend

- engine query tests in `crates/neovex-engine/src/tests.rs`
- auth tests in `crates/neovex-server/src/tests/auth/`
- reactive loop tests in `crates/neovex-server/tests/reactive_loop.rs`

#### New tests

- verify indexed queries, fallback scans, pagination, and subscriptions never
  return rows that policy forbids for the active principal
- verify unauthorized create, update, and delete attempts fail before commit
- verify the same normalized principal yields the same authorization result
  through native and Convex-backed request paths
- verify a policy revision change causes affected subscriptions or cached views
  to be revalidated or dropped before another result is delivered
- verify a principal-context change for a live subscription does not continue to
  receive results authorized under the old claims snapshot
- verify runtime host calls cannot bypass engine authorization by calling the
  mutation or query path indirectly

#### Acceptance criteria

- authorization rules are enforced inside the engine and planner rather than in
  route-specific middleware
- authentication remains at the adapter or transport boundary
- subscriptions and caches do not continue serving data across policy or
  principal-context changes without revalidation
- no supported query shape can observe rows that policy forbids
- Convex compatibility auth continues to work through identity normalization
  rather than a separate authorization subsystem

#### Implementation checkpoint

- Started 2026-04-01 after completing 3D.
- Completed 2026-04-01.
- `PrincipalContext`, principal snapshots, declarative table access policies,
  and policy-revision hashing now live in `neovex-core`.
- Engine reads compile policy into planner-aware filters plus residual
  predicates, `get` hides unauthorized rows as not-found, mutation
  authorization is enforced atomically around storage writes, and subscriptions
  store both the principal snapshot and the active policy revision.
- Convex auth remains the boundary authenticator, while runtime host calls,
  direct query or mutation helpers, HTTP actions, and socket subscriptions all
  normalize `InvocationAuth` into engine-owned principal context before data
  access.
- Policy revision changes clear the tenant document cache and terminate stale
  subscriptions, and websocket auth changes conservatively tear down active
  subscriptions so old claims snapshots cannot continue receiving data.

---

### 3F. Add serializable OCC validation on top of the shared dependency model

**Priority:** medium, after 3D and 3E  
**Expected impact:** aligns the transaction model with the research guide by
using read and write sets for both invalidation and conflict detection.

#### Current verified state

- the current engine centers on single-operation mutations through
  `Service::apply_mutation(...)`
- Phase `3D` introduces a shared dependency-set model, but current code still
  lacks serializable OCC validation built on those dependency forms
- there is no explicit transaction boundary that captures a multi-step
  read/write execution unit and validates it at commit time

#### Implementation plan

1. Introduce an explicit transaction or execution-unit boundary in the engine
   for any path that needs multi-step read plus write behavior, without
   bypassing the existing validation and commit path.

2. Capture normalized read and write sets using the shared dependency model
   from Phase `3D`, including planner-enforced authorization predicates where
   they affect the visible read set.

3. Validate the transaction at commit against writes that became durable after
   the transaction's read snapshot, aborting or retrying on conflict rather
   than blocking readers.

4. Keep reads non-blocking in the first implementation.
   Do not fall back to a lock-based transaction manager just to avoid retries.

5. Ensure subscription invalidation, durable-journal write metadata, and OCC
   conflict checking all reuse the same dependency vocabulary rather than
   evolving separate representations.

6. Shape the implementation so later snapshot or time-travel read support can
   layer on top of the same model without replacing OCC as the baseline
   transaction mechanism.

#### Files to change

- `crates/neovex-core/src/` for shared transaction and dependency metadata
- `crates/neovex-engine/src/service/mutations.rs`
- `crates/neovex-engine/src/service/queries.rs`
- `crates/neovex-engine/src/service/` for new transaction coordination code
- `crates/neovex-server/src/adapters/convex/host_bridge/`
- `ARCHITECTURE.md`

#### Existing tests to extend

- engine service tests in `crates/neovex-engine/src/tests.rs`
- runtime read-tracking tests in
  `crates/neovex-server/src/runtime/read_tracking/tests.rs`

#### New tests

- verify conflicting transactions with overlapping document or range reads
  cause deterministic abort or retry behavior
- verify non-conflicting transactions can commit concurrently without blocking
  reads
- verify authorization-filtered reads still produce correct conflict detection
  when policies affect the visible result set
- verify dependency metadata used for invalidation is the same metadata used
  for OCC validation

#### Acceptance criteria

- Neovex has a concrete serializable OCC path rather than only "future-ready"
  dependency tracking
- read and write sets drive both invalidation and conflict detection
- read paths remain non-blocking
- the OCC implementation preserves room for later snapshot-style MVCC reads
  without replacing the core model

---

## Phase 4: Testability And Server-Layer Cleanup

Phase `4A` through `4C` are lower-priority cleanup work. Phase `4D` is more
important: it establishes deterministic test seams that should land before the
later journal, materializer, or replica work is trusted.

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

### 4D. Introduce deterministic simulation seams for time, storage, and injected faults

**Priority:** high, before Phase 6 and serving-path materializer work  
**Expected impact:** brings the roadmap closer to the research guide's
simulation-first recommendation and gives later durability work a harsh but
reproducible verification environment.

#### Current verified state

- the project already has conventional unit and integration tests, but no
  shared deterministic simulation harness for storage, clock, or failure
  injection
- the later journal and materializer phases now depend on stronger replay,
  corruption, and shadow-parity testing than the current test scaffolding can
  express cleanly
- TigerBeetle and FoundationDB treat this style of deterministic failure
  testing as part of the architecture, not a bolt-on after implementation

#### Implementation plan

1. Add explicit seams for the new durability-critical subsystems rather than
   abstracting the entire existing codebase at once. Start with:
   - clock and time progression
   - journal append, flush, and reopen boundaries
   - checkpoint and manifest persistence boundaries
   - injected crash or fault hooks around visibility transitions

   Before the journal or checkpoint subsystems exist, keep the production
   implementations narrow. In the first pass these seams may be thin wrappers
   around existing clock, task, and test-support boundaries that later expand
   as Phase 6 and Phase 9 land. Do not build speculative framework layers that
   guess at final journal internals too early.

2. Provide both production implementations and seeded deterministic test
   implementations of those seams.

3. Add a lightweight single-process harness that can drive scripted failures,
   restarts, and replays deterministically. The goal is not a distributed
   cluster simulator in the first pass; it is precise control over local
   storage and execution failure modes.

4. Add BUGGIFY-style or failpoint-style hooks for rare but critical boundaries
   such as:
   - append before durable flush
   - durable flush before visibility
   - checkpoint publish before manifest update
   - compaction start before compaction publish

5. Require new journal, materializer, and replica-path code to expose these
   seams before serving-path promotion.

6. Keep the design close to TigerBeetle and FoundationDB in spirit:
   deterministic replay, seeded reproducibility, and harsh failure injection.
   Do not copy their distributed simulators literally.

#### Files to change

- `crates/neovex-storage/src/` for new journal and checkpoint seam types
- `crates/neovex-engine/src/` where new time or fault seams are needed
- `crates/neovex-test-support/src/`
- `crates/neovex-server/tests/` if socket or transport replay harnesses are added
- `ARCHITECTURE.md`

#### Existing tests to extend

- storage tests in `crates/neovex-storage/src/tests.rs`
- engine tests in `crates/neovex-engine/src/tests.rs`
- any later journal or materializer tests added under `crates/neovex-storage/src/`

#### New tests

- verify the same seed and failure schedule reproduce the same crash or replay
  result exactly
- verify injected crash points around append, flush, checkpoint, and compaction
  boundaries either recover correctly or fail loudly and deterministically
- verify simulated clocks can advance retries, deadlines, and background work
  without wall-clock sleeps
- verify deterministic failure tests can run in CI without relying on timing
  races

#### Acceptance criteria

- new durability-critical subsystems expose deterministic clock, storage, and
  fault-injection seams
- failure schedules are reproducible by seed
- later Phase `6`, `8`, and `9` work can be tested under crash and replay
  conditions without depending on wall-clock timing or flaky races

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

5. Preserve redb's multiple-reader behavior.
   The async rewrite must not force all reads and writes for a tenant through a
   single FIFO actor queue. Writes may remain serialized, but reads should still
   be able to make concurrent progress against stable snapshots.

6. Do not reopen the storage-backend choice in this phase.
   The concrete implementation is "redb behind an async execution boundary"
   first.

#### Implementation plan

1. Add internal async storage traits in `neovex-storage` or an adjacent
   storage-boundary module, using native async traits for internal boundaries.

2. Implement a redb-backed async adapter behind a dedicated async execution
   boundary.
   This may be an actor, dedicated executor, or similarly explicit ownership
   model, but the design must keep tenant ordering and cancellation behavior
   clear while preserving multiple concurrent readers. If an actor boundary is
   used, reads must not be forced through the same single serialized lane as
   writes.

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
- add a concurrency test proving one blocked read does not force a second read
  on the same tenant to wait behind a single actor-style FIFO queue
- verify async read-path results match the previous synchronous implementation
  for point reads, queries, pagination, commit-log reads, and latest-sequence
  reads

#### Acceptance criteria

- read paths no longer depend on `call_blocking(...)` or
  `call_blocking_cancellable(...)`
- cancellation propagates into real storage work for reads
- the async boundary does not reduce tenant reads to one-at-a-time FIFO
  execution
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

#### Important clarification

redb is already crash-safe via copy-on-write atomic commit. This phase is not
compensating for a missing storage WAL. It introduces an application-level
durable mutation journal because we need an ordered history for replay,
dependency-driven invalidation, streaming, and later replica consumption.

If Neovex later adds a custom write-optimized layer such as an LSM-style
memtable plus SST materializer, this durable journal should become the
log-before-materialization contract for that layer. The roadmap should not
assume a second Neovex-owned WAL by default. A future third-party storage engine
may still keep its own internal WAL or journal, but the Neovex architecture
should prefer one logical ordered-history contract rather than two competing
application-level logs.

#### Architectural decision

For this roadmap, Neovex will implement and own the durable logical journal
inside the existing `neovex-core` and `neovex-storage` architecture. Agents
must not reopen this decision while executing Phase 6 work.

Why this is the project decision:

- redb already provides crash-safe atomic commit, so Phase 6 is not trying to
  patch a missing storage-engine WAL
- Neovex needs a logical ordered history for replay, invalidation, streaming,
  and future replicas, not just a physical recovery log
- that logical history must align with Neovex mutation semantics, dependency
  matching, visibility rules, and tenant-scoped replay
- building the journal as a Neovex-owned layer keeps the storage contract
  stable even if a future materializer or storage engine changes underneath it

External systems are references, not substitutes:

- TigerBeetle is the main durability and verification reference
- OpenRaft provides useful append, flush-notification, truncation, and no-hole
  log invariants
- RocksDB and Fjall are references for engine-internal WAL or journal
  lifecycle, batching, recovery, and materialization behavior

Explicit non-decisions for Phase 6:

- do not adopt OpenRaft as the local journal implementation; it is the wrong
  abstraction layer for a single-node per-tenant logical journal
- do not swap the storage engine to Fjall, RocksDB, or another LSM as part of
  this phase; that is a separate architecture project
- do not reduce the journal to a thin byte-log crate that lacks Neovex replay,
  visibility, and dependency-tracking semantics

#### Implementation plan

1. Implement the project-level durable-journal decision.
   Build the Phase 6 journal as a Neovex-owned logical ordered-history layer in
   the current redb-backed architecture. Do not spend execution PRs revisiting
   storage-engine adoption or journal ownership.

2. Introduce a richer durable mutation record.
   The existing `CommitEntry` is not enough for replay or for the scaling spec's
   commit-log-driven invalidation model. Before any journal-driven replication
   or materialization architecture can work, we need a record type that
   contains enough data to serve both:
   - replay and materialization
   - write-set-against-dependency-set matching for invalidation and streaming

   At minimum the durable record should include:
   - normalized logical write-set metadata such as table, op kind, document id,
     and any key/range metadata required by the shared dependency-set matcher
   - replay payload data such as post-image, patch, or tombstone data sufficient
     to reconstruct state in order
   - record versioning and integrity metadata suitable for durable streaming and
     recovery

   The journal contract must remain logical and storage-engine-agnostic.
   Do not encode redb page identifiers, physical table layout, or other
   backend-specific details into the durable record format.

3. Keep `Service::apply_mutation(...)` as the semantic entrypoint.

4. Append the richer mutation record durably before acknowledging the write.
   Group commit may batch multiple append requests into a single durable flush,
   but durability still comes before acknowledgment.

5. Materialize into redb document/index tables in strict commit order and track
   both a durable sequence head and an applied sequence head per tenant.
   If a future custom LSM-style layer is introduced, it should consume this same
   durable journal rather than defining a second application-level write log.

6. Keep one authoritative read-visible state in Phase 6A: the applied
   materialized state in redb.
   Do not overlay pending journal records onto point reads, scans,
   subscriptions, or cache lookups in this phase. The first implementation
   should preserve one serving read path rather than inventing a second
   correctness-critical overlay engine.

7. Gate reads on the applied sequence watermark.
   Reads should execute only when `applied_sequence >= required_sequence`.
   In the first implementation:
   - a normal "latest" read should wait for at least the durable head observed
     when the read is admitted
   - a read-your-own-write or causally-following read should wait for at least
     the caller's acknowledged sequence
   - the engine should expose metrics for durable head, applied head, apply lag,
     and read wait time so lag is observable and backpressure can be added
     before correctness is weakened

8. Keep subscription fan-out and cache publication behind the same applied
   visibility boundary.
   A mutation may be durably committed before it is read-visible, but reactive
   reevaluation and cache publication must not run ahead of the applied
   sequence.

9. Add recovery logic so startup can replay durable-but-unapplied records and
   restore the correct applied watermark before serving reads.

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
- verify the visible durable sequence never contains holes across batched appends
  and recovery
- verify durable journal serialization preserves both replay payload and
  normalized write-set metadata
- verify representative dependency-set intersections can be decided from durable
  journal metadata for table, document, and index-range cases
- verify recovery replays durable-but-unapplied records on startup
- verify normal "latest" reads wait for the admitted durable head before
  returning
- verify read-your-own-writes and causally-following reads wait for the
  acknowledged sequence across the materialization boundary
- verify reads do not synthesize overlay results from journal-only records that
  are not yet applied to materialized state
- verify subscription fan-out and cache publication happen only after the
  applied sequence has advanced past the mutation sequence
- verify durable head, applied head, apply lag, and read wait time metrics are
  emitted for the new visibility boundary

#### Acceptance criteria

- there is no visibility of non-durable writes
- write acknowledgments are durable even when materialization is deferred
- there is one authoritative read-visible state in Phase 6A: applied
  materialized state
- serving reads wait on the applied sequence watermark rather than using a
  journal-overlay read path
- no journal-only write becomes query-visible, subscription-visible, or
  cache-visible before its sequence is applied
- journal records support both replay/materialization and dependency-driven
  invalidation use cases
- the implementation follows the project-level decision to keep the journal
  Neovex-owned and does not silently replace the storage engine architecture
- journal records remain logical enough to support future alternate
  materializers or storage engines without redefining the ordered-history
  contract
- the roadmap remains compatible with a future custom LSM-style materializer
  without requiring a second Neovex-owned application WAL
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
   Even after the journal becomes the canonical ordered history, serving reads
   should still come from applied materialized state unless a later roadmap
   phase explicitly promotes another read path.

3. Add replay and snapshot boundaries:
   - bootstrap from snapshot plus journal tail
   - rebuild from journal for verification
   - define compaction and snapshot cut points explicitly
   - persist enough sequence metadata to prove which journal entries are
     already reflected in materialized serving state

4. Add CDC or streaming APIs only after replay and recovery semantics are solid.

5. Use consumer cursors and retention rules that align with the horizontal
   scaling spec's commit-log model.

6. Treat this phase as the end of the server-internal foundation, not the full
   end-state architecture.
   The next phase should make that journal consumable by streaming and replica
   paths rather than letting the roadmap stop at internal CDC readiness.

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
- verify rebuilt materialized state reports the same applied sequence boundary
  as the live serving path

#### Acceptance criteria

- durable journal order is the canonical order for downstream consumers
- document/index state can be rebuilt from journal plus snapshot boundaries
- serving reads still obey applied materialized visibility rather than direct
  journal visibility
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

## Phase 8: Log Streaming And Replica Paths

This phase carries the roadmap beyond server-side re-evaluation toward the
scaling spec's longer-term C+D+E architecture: database-per-tenant plus
commit-log-driven invalidation plus embedded replicas.

### 8A. Stream the authoritative per-tenant journal to external consumers

**Priority:** low, after 6B  
**Expected impact:** turns the durable journal into a concrete sync and replica
feed instead of stopping at internal CDC readiness.

#### Current verified state

- the roadmap in Phase 6 moves the journal toward authoritative ordered history,
  but there is not yet a concrete consumer-facing streaming path
- clients today still receive server-computed subscription results rather than
  consuming the ordered history directly

#### Implementation plan

1. Introduce a tenant-scoped journal streaming API using ordered sequence
   cursors.

2. Keep the first consumer model read-only and replay-friendly:
   - resume from sequence cursor
   - at-least-once delivery semantics
   - explicit duplicate-tolerant replay contract

3. Reuse the authoritative durable journal format from 6A and 6B. Do not invent
   a second replication log for consumers.

4. Support internal and infrastructure consumers first, such as:
   - backup or audit pipelines
   - an edge or embedded-replica feeder
   - verification or state-rebuild tooling

5. Define retention, cursor, and bootstrap rules before broad adoption:
   - snapshot plus journal tail bootstrap
   - retention cut points
   - cursor invalidation behavior after compaction

#### Files to change

- `crates/neovex-storage/src/`
- `crates/neovex-engine/src/service/queries.rs`
- `crates/neovex-server/src/http/`
- `crates/neovex-server/src/protocol.rs`
- `ARCHITECTURE.md`

#### Existing tests to extend

- commit-log and sequence-read tests in `crates/neovex-engine/src/tests.rs`
- server HTTP tests in `crates/neovex-server/src/tests/`

#### New tests

- verify journal streaming resumes correctly from a sequence cursor
- verify consumers observe strictly ordered entries with documented replay
  semantics
- verify bootstrap from snapshot plus journal tail reconstructs the same state as
  live reads

#### Acceptance criteria

- there is a concrete ordered journal stream for tenant-scoped consumers
- consumers can resume from sequence cursors without a second log format
- the roadmap now has an explicit bridge from authoritative journal history to
  sync and replica consumers

---

### 8B. Add an embedded-replica path for local or edge query evaluation

**Priority:** low, after 8A  
**Expected impact:** establishes a concrete path from server-evaluated pushes to
replica-local reads for selected workloads.

#### Current verified state

- the server remains responsible for re-evaluating subscriptions and pushing
  results directly
- there is no embedded replica or edge-consumption path in the current codebase
- the scaling spec's north star explicitly calls for embedded replicas as a
  complement to database-per-tenant plus commit-log invalidation

#### Implementation plan

1. Start with a narrow replica target:
   - one tenant at a time
   - read-only replica
   - supported by snapshot plus journal-tail bootstrap

2. Build a replica consumer that applies authoritative journal records into a
   local materialized view or embedded store.

3. Evaluate a representative subset of query and subscription workloads against
   that local replica and compare the results with the current server-evaluated
   path.

4. Keep writes server-authoritative in the first pass.
   Replica work is about read-path offload and local evaluation, not multi-master
   mutation.

5. Use this phase to prove correctness and economics before productizing wider
   offline or edge features.

#### Files to change

- new replica or sync-consumer module location
- `crates/neovex-engine/src/service/queries.rs` as needed for replica-comparison
  helpers
- `crates/neovex-server/src/` for bootstrap or streaming integration
- `ARCHITECTURE.md`

#### Existing tests to extend

- engine replay and rebuild tests in `crates/neovex-engine/src/tests.rs`
- server integration tests in `crates/neovex-server/src/tests/`

#### New tests

- bootstrap a replica from snapshot plus journal and verify it matches the live
  tenant state
- verify representative subscription or query results computed from the replica
  match server-evaluated results
- verify a temporarily disconnected replica can catch up from the journal after
  reconnection

#### Acceptance criteria

- the roadmap no longer stops at internal CDC foundations
- there is a validated path for replica-local query evaluation on top of the
  authoritative journal
- server-side re-evaluation is clearly documented as the near-term path, not the
  final architecture endpoint

---

## Phase 9: Deterministic Materializer And Robustness

This phase puts a TigerBeetle-inspired deterministic materializer explicitly in
scope, but only on top of the durable journal introduced in Phase 6. It is not
a Phase 6 storage-engine swap and it does not replace redb as the durability
base for this roadmap.

### 9A. Build a shadow journal-driven materializer with deterministic compaction

**Priority:** low, after 6B  
**Expected impact:** establishes a concrete path to a custom write-optimized
materializer without weakening the current redb-backed correctness path.

#### Current verified state

- redb document and index tables remain the only materialized serving state
- there is no custom Neovex materializer consuming the durable journal
- TigerBeetle's LSM and checkpoint model are now an explicit design reference,
  but not yet represented in the codebase

#### Architectural decision

If Neovex builds a custom materializer, the first version should:

- consume the authoritative durable journal from Phase 6
- use deterministic compaction principles inspired by TigerBeetle
- run in shadow mode first, with redb remaining the serving path and
  correctness oracle

Deterministic compaction here means:

- compaction inputs and outputs are chosen from journal state, checkpoint
  state, and explicit configuration
- compaction behavior does not depend on wall-clock timing, racing background
  tasks, or incidental execution order
- given the same checkpoint, journal suffix, and configuration, rebuild and
  compaction should converge on the same logical materialized state

#### Implementation plan

1. Add a journal-driven materializer module under `crates/neovex-storage/src/materializer/`
   or a closely adjacent storage-owned location. Keep the first version clearly
   subordinate to the redb-backed serving path.

2. Build the materializer from snapshot plus journal tail and support full
   replay from an explicit checkpoint boundary.

3. Adopt deterministic compaction rules inspired by TigerBeetle:
   - compaction triggers depend on explicit thresholds and state, not timers
   - compaction input selection is deterministic for a given state
   - checkpoint and manifest updates are explicit and versioned

4. Keep the first implementation in shadow mode.
   Every selected query or lookup path should be able to compare materializer
   results against the current redb-backed path before any serving promotion.

5. Keep the journal contract logical and storage-engine-agnostic even if the
   materializer uses LSM-style internal structures.

6. Document the compaction invariants and checkpoint semantics in
   `ARCHITECTURE.md` when the shadow implementation lands.

#### Files to change

- `crates/neovex-storage/src/materializer/`
- `crates/neovex-storage/src/`
- `crates/neovex-engine/src/service/queries.rs`
- `crates/neovex-engine/src/tests.rs`
- `ARCHITECTURE.md`

#### Existing tests to extend

- storage replay and commit-log tests in `crates/neovex-storage/src/tests.rs`
- engine query tests in `crates/neovex-engine/src/tests.rs`

#### New tests

- rebuild the materializer from checkpoint plus journal and verify it matches
  the live redb-backed state
- verify the same replay input and compaction configuration produce the same
  logical materialized state across repeated rebuilds
- verify crash-recovery from a checkpoint boundary plus journal tail restores
  the same materialized state
- verify representative shadow queries return the same results as the redb
  serving path

#### Acceptance criteria

- a custom materializer exists only in shadow mode at first
- deterministic compaction rules are explicit, documented, and testable
- materialized state can be rebuilt from checkpoint plus durable journal
- redb remains the correctness oracle until shadow verification proves parity

---

### 9B. Add TigerBeetle-style robustness testing for journal and materializer

**Priority:** low, after 9A  
**Expected impact:** ensures any future materializer is introduced with replay,
crash, and corruption robustness rather than benchmark-first optimism.

#### Current verified state

- the roadmap already calls for durable-journal tests, but not yet for a
  TigerBeetle-style robustness harness around a custom materializer
- there is no dedicated failure-injection or crash-replay test harness for a
  journal-driven materializer

#### Implementation plan

1. Add deterministic replay tests that exercise:
   - replay from clean checkpoints
   - replay after interrupted materialization
   - replay after interrupted compaction

2. Add corruption and partial-write-style tests for journal segments,
   checkpoints, manifests, or equivalent materializer metadata.

3. Add fuzz or property-style tests looking for:
   - replay crash loops
   - divergence between redb and the shadow materializer
   - no-hole or out-of-order journal visibility bugs
   - compaction behaviors that depend on nondeterministic scheduling

4. Keep redb-vs-materializer shadow comparison in the test matrix until the
   materializer is promoted for any serving path.

5. Require any serving promotion to document:
   - which query classes are now allowed to read from the materializer
   - what shadow-validation evidence justified the promotion
   - what rollback path returns serving reads to redb if divergence is detected

#### Files to change

- `crates/neovex-storage/src/materializer/`
- `crates/neovex-storage/src/tests.rs`
- `crates/neovex-engine/src/tests.rs`
- `crates/neovex-test-support/`
- `ARCHITECTURE.md`

#### Existing tests to extend

- storage replay tests in `crates/neovex-storage/src/tests.rs`
- engine query parity tests in `crates/neovex-engine/src/tests.rs`

#### New tests

- verify interrupted compaction plus replay converges to the same materialized
  state
- verify injected corruption is detected and does not silently yield divergent
  query answers
- verify repeated replay of the same journal suffix is idempotent
- verify shadow divergence is surfaced deterministically in tests

#### Acceptance criteria

- the materializer has a TigerBeetle-inspired robustness harness before serving
  promotion
- replay, corruption detection, and shadow-parity guarantees are testable
- no serving-path promotion occurs without documented shadow-validation
  evidence

---

## Phase 10: Snapshot And Historical Read Evolution

This phase is intentionally after the core OCC, journal, and materializer work.
It exists to preserve the research guide's MVCC extension path without
pretending Neovex needs time-travel reads immediately.

### 10A. Layer optional snapshot and historical reads on top of OCC and the durable journal

**Priority:** low, after 3F and 6B  
**Expected impact:** preserves a future path to MVCC-style snapshot reads or
time-travel queries without replacing OCC as the primary transaction model.

#### Current verified state

- `redb` already provides internal copy-on-write MVCC snapshots for storage
  correctness
- the Neovex engine does not yet expose snapshot, historical, or time-travel
  query semantics as a product feature
- the roadmap now commits to OCC as the primary concurrency model, with the
  durable journal as the longer-term ordered history substrate

#### Implementation plan

1. Treat this as an additive read feature layered on top of OCC, not as a
   replacement concurrency model.

2. Define which snapshot forms are actually supported before implementation,
   such as:
   - read-at-transaction-start snapshots
   - bounded historical reads by sequence number
   - bounded historical reads by timestamp if sequence-to-time mapping is
     explicit enough

3. Use the durable journal, checkpoints, and retained materialized history as
   the basis for any historical visibility contract rather than leaking raw
   storage-engine internals into the public API.

4. Keep planner-enforced authorization semantics defined for historical reads
   as well as current reads.

5. Add explicit retention and garbage-collection rules so historical reads do
   not become an accidental infinite-history promise.

#### Files to change

- `crates/neovex-core/src/query.rs`
- `crates/neovex-engine/src/service/queries.rs`
- `crates/neovex-storage/src/`
- `ARCHITECTURE.md`

#### Existing tests to extend

- engine query tests in `crates/neovex-engine/src/tests.rs`
- storage replay tests in `crates/neovex-storage/src/tests.rs`

#### New tests

- verify a snapshot query sees a stable view while concurrent writes continue
- verify bounded historical reads reconstruct the correct view from retained
  history
- verify authorization rules still apply to snapshot and historical queries
- verify retention rules reject history requests that fall outside the retained
  window

#### Acceptance criteria

- snapshot or historical reads are clearly additive to the OCC model
- visibility rules are explicit and testable
- retention boundaries are documented and enforced
- the feature does not rely on implicit redb internals as the public contract

---

## Phase 11: Schema-Generated Native API And Typed Extension ABI

This phase operationalizes the research guide's schema-driven API direction
without replacing the Convex compatibility surface. It is intentionally late in
this roadmap because the engine must first gain planner-enforced authorization,
shared dependency semantics, and a stable enough internal contract to expose a
generated native API safely.

### 11A. Add a schema-generated Neovex-native API surface

**Priority:** low, after 3B and 3E  
**Expected impact:** gives Neovex a first-class schema-generated developer
surface for CRUD, pagination, and subscriptions instead of relying only on
hand-written native routes or the Convex compatibility path.

#### Current verified state

- Neovex-native routes are currently hand-written server endpoints
- Convex compatibility exposes a function-oriented JavaScript surface through
  V8 and generated manifests, but Neovex does not yet expose a schema-generated
  native API
- planner-enforced authorization is scheduled earlier in the roadmap and should
  become the policy substrate for any generated API

#### Architectural decision

Follow battle-tested generated-API patterns rather than inventing a bespoke
opaque layer:

- like PostgREST, the generated API should expose a schema-owned public model
  rather than leaking raw storage internals directly
- like Hasura, role- or principal-aware policy should remain declarative and
  planner-enforced rather than moving authorization logic into generated route
  glue
- like Convex, compatibility-specific function execution remains a distinct
  surface rather than the only way to reach database behavior

There is no single Rust library that solves this entire product surface for
Neovex. The generated API contract, schema exposure rules, and auth semantics
should remain Neovex-owned.

#### Implementation plan

1. Define an explicit public schema exposure model.
   The generated API should be derived from the schema objects Neovex marks as
   public or exposed, not from every internal storage structure automatically.

2. Generate a typed API contract for:
   - CRUD operations
   - pagination and filtering
   - subscription registration shapes
   - schema-derived metadata needed by SDKs or tooling

3. Keep generated endpoints planner-first.
   Generated reads and writes should call the same engine entrypoints as manual
   routes, with planner-enforced authorization and dependency tracking.

4. Preserve versionability.
   The generated surface should make it possible to evolve the internal schema
   and storage layout without making every internal change a wire-breaking API
   change.

5. Keep the generated Neovex-native API distinct from the Convex compatibility
   layer.
   Shared engine semantics are desirable; forced contract unification is not.

#### Files to change

- `crates/neovex-core/src/schema.rs`
- `crates/neovex-server/src/`
- `packages/codegen/` or a new Neovex-native codegen package
- `packages/neovex/`
- `ARCHITECTURE.md`

#### Existing tests to extend

- server route tests in `crates/neovex-server/src/tests/`
- JS package tests in `packages/neovex/`

#### New tests

- verify the same schema produces a deterministic generated API contract
- verify generated CRUD and subscription routes enforce the same planner-level
  authorization as manual engine calls
- verify generated pagination and filtering behavior matches the engine's query
  semantics exactly
- verify internal schema or storage-only changes do not accidentally leak into
  the generated public contract

#### Acceptance criteria

- Neovex has a schema-generated native API surface distinct from the Convex
  compatibility layer
- generated routes remain planner-enforced and authorization-safe
- the public generated contract is explicit, versionable, and deterministic

---

### 11B. Add a typed WASM plugin ABI for Neovex-native extensions

**Priority:** low, after 11A  
**Expected impact:** creates a database-native extension surface for tightly
bounded custom logic without overloading the Convex compatibility runtime.

#### Current verified state

- the current extensibility surface is the V8-based Convex compatibility path
- there is no Wasmtime- or WIT-based plugin ABI for Neovex-native extensions
- the roadmap already treats future WASM support as additive rather than a
  replacement for V8

#### Architectural decision

Use battle-tested typed-component practices rather than a generic "run wasm"
escape hatch:

- use Wasmtime as the Rust-native runtime for plugin execution
- define the ABI in WIT or the Component Model so imports and exports are typed
  and versionable
- expose explicit capabilities rather than broad engine internals
- keep authorization and query planning in the engine; plugins consume narrow
  authorized capabilities instead of deciding policy for themselves

#### Implementation plan

1. Define a narrow first plugin scope, such as:
   - computed fields
   - validation helpers
   - deterministic transforms
   Avoid an unrestricted general-purpose serverless runtime in the first pass.

2. Define a typed WIT interface for the first plugin scope, including explicit
   capability handles for the host services a plugin may use.

3. Embed Wasmtime at the server or engine integration boundary, not inside
   `neovex-runtime`.
   Keep the existing V8 compatibility runtime independent and separate.

4. Version the plugin ABI explicitly so host and plugin mismatches fail fast
   instead of silently reinterpreting memory layouts or semantics.

5. Keep planner-enforced authorization in the engine.
   Plugins should observe only the capabilities and data their caller is
   already authorized to access.

#### Files to change

- new Wasmtime integration module location
- `crates/neovex-core/src/` for plugin ABI metadata
- `crates/neovex-server/src/` or `crates/neovex-engine/src/` at the chosen host
  boundary
- `ARCHITECTURE.md`

#### Existing tests to extend

- engine tests in `crates/neovex-engine/src/tests.rs`
- server integration tests in `crates/neovex-server/src/tests/`

#### New tests

- verify ABI version mismatches fail explicitly
- verify a plugin cannot access host capabilities it was not granted
- verify plugin execution remains deterministic for the same inputs and host
  state
- verify planner-enforced authorization still governs any data reached through
  plugin calls

#### Acceptance criteria

- Neovex has a typed, versioned WASM plugin ABI that is separate from the V8
  compatibility runtime
- Wasmtime is used as the plugin runtime rather than a generic untyped wasm
  loader
- plugin capabilities are narrow, explicit, and authorization-safe

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
   semantics. In Phase 6, serving reads still come from applied materialized
   state rather than direct journal overlay.

5. Do not weaken durability for speed.
   No best-effort write buffer should become visible to clients before it is
   durably ordered.

6. Keep the evaluator pure and the runtime crate independent.
   Those boundaries are part of the architecture, not just the current layout.

7. Keep transport semantics honest.
   "Committed" at the engine boundary does not guarantee an already-disconnected
   client observed the response.

8. Preserve Convex compatibility unless the roadmap explicitly says otherwise.
   Changes that affect the Convex adapter, runtime bridge, generated bundles,
   or protocol semantics should keep existing compatibility tests passing unless
   a roadmap item records an intentional compatibility change.

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
  3D depends on 3A and 3B, and benefits from 3C
  3E depends on 3B and benefits from 3D
  3F depends on 3D and 3E

Phase 4:
  4A independent
  4B independent
  4C optional and independent
  4D independent

Phase 5:
  5A -> 5B -> 5C
  5 benefits from 1D and 1F

Phase 6:
  6A depends on Phase 5 and 4D, and benefits from 3D
  6B depends on 6A

Phase 7:
  7A depends on the runtime executor shape from 2A
  7B remains blocked on deploy identity/auth

Phase 8:
  8A depends on 6B
  8B depends on 8A

Phase 9:
  9A depends on 6B and 4D, and benefits from 3D
  9B depends on 9A

Phase 10:
  10A depends on 3F and 6B

Phase 11:
  11A depends on 3B and 3E
  11B depends on 11A and benefits from 3E
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
8. 3D
9. 3E
10. 4D
11. 3F
12. 4A and 4B if handler or lifecycle pain is still active
13. 4C if external metrics export becomes a concrete requirement
14. 5A
15. 5B
16. 5C
17. 6A
18. 6B
19. 8A
20. 8B
21. 9A
22. 9B
23. 10A if snapshot or historical reads become a product requirement
24. 11A if the Neovex-native generated API becomes a product priority
25. 11B after 11A if a typed WASM plugin ABI becomes a product priority
26. 7B when deploy identity exists
