# Scalability And Architecture Follow-On Plan

This is the canonical execution roadmap for the remaining follow-on
architecture work identified by the April 2026 architecture review pass after
the completed performance cycle and the completed verification-harness cycle.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/archive/performance-and-architecture-plan.md`
- `docs/plans/archive/verification-harness-plan.md`
- `crates/nimbus-core/src/`
- `crates/nimbus-engine/src/`
- `crates/nimbus-server/src/`

---

## Purpose

Nimbus now has a much stronger baseline than it had when the follow-on
architecture review started:

- execution-unit table materialization no longer does repeated full-vector
  filtering
- schema snapshots are shared behind `Arc` instead of deep-cloned per read
- owned response paths can move documents into JSON instead of cloning the full
  field map every time
- the per-tenant document cache is bounded and evicts instead of growing
  forever
- dependency dedup for predicates, index ranges, and paginated windows is
  hash-backed rather than linear
- subscription delivery and WebSocket forwarding channels are bounded end to
  end, so slow consumers no longer imply unbounded memory growth

Those fixes closed the clear, reproducible low-risk issues. What remains is a
set of larger follow-on items that change write-path latency, query-planning
capability, task lifecycle structure, and scan behavior. That work is too large
to bury in the completed master roadmap and too different from harness work to
fit the verification plan, so it gets its own execution control plane here.

---

## Relationship To Other Plans

1. `docs/plans/archive/performance-and-architecture-plan.md` is the archived
   execution record for the completed architecture cycle it covered.
2. `docs/plans/archive/verification-harness-plan.md` is the archived
   execution plan for deterministic simulation, generated-history
   verification, differential testing, and consistency verification.
3. This document owns the remaining follow-on performance, scalability, query,
   and task-lifecycle work from the April 2026 architecture review.
4. When a change in this plan alters architecture-level behavior, update
   `ARCHITECTURE.md` in the same change set.

---

## Scope

This plan covers the still-open items from the review that are architectural
changes rather than already-fixed defects:

- decouple subscription re-evaluation from the synchronous write return path
- coalesce invalidation across batched durable-apply work
- replace ad hoc detached task lifetimes with structured concurrency
- expand the planner beyond single-field exact and range scans
- reduce scan cost by avoiding full document deserialization when simple
  predicates can be rejected early
- harden `MutationExecutionUnit` lifecycle constraints
- evaluate deeper read-path storage formats only after the nearer-term work is
  complete

This plan does not reopen the items that were already fixed and verified in the
baseline above unless a later item explicitly depends on revisiting them.

---

## Success Criteria

This plan is successful only when all of the following are true:

1. Subscription-heavy tenants no longer pay full subscription re-evaluation
   latency directly on the synchronous mutation return path.
2. Batched journal apply work performs one coalesced invalidation pass per
   batch rather than repeating equivalent scans per record where semantics
   allow coalescing.
3. Socket, subscription, and runtime background work is scoped to parent
   lifetimes so disconnects, auth changes, and shutdowns do not leave detached
   workers behind.
4. The planner and index layer support the most important multi-field query
   patterns explicitly rather than falling back to residual in-memory work for
   all of them.
5. Large selective scans can reject obvious non-matches before full document
   materialization when the predicate shape allows it.
6. Execution-unit lifecycle misuse becomes harder by construction, either
   through type-state or a comparably strong API restriction.
7. Operators can observe write-path, query-path, cache, and subscription-path
   behavior through explicit metrics rather than inferring system health from
   broad request success alone.
8. Every completed item lands with deterministic regressions or benchmarks that
   prove the new behavior rather than relying on narrative justification.

---

## Current Verified State

As of the baseline for this plan:

- `cargo test -p nimbus-core -p nimbus-engine -p nimbus-server` is green
- `cargo fmt --all --check` is green
- `cargo clippy --workspace --all-targets -- -D warnings` is green
- the write path still performs subscription re-evaluation inline after apply
- durable batch application still calls commit processing one commit at a time
- the planner still only chooses exact single-field index scan, single-field
  range scan, or full table scan
- scans still fully deserialize MessagePack documents before many simple
  filters can reject them
- execution-unit lifecycle remains runtime-disciplined rather than
  type-disciplined

---

## Execution Contract

Use this section as the default operating procedure for every item below.

### General rules

- Prefer targeted deterministic regressions and measurable acceptance criteria
  over broad speculative rewrites.
- Preserve the already-landed fixes from the baseline above while building the
  larger follow-on work.
- Keep each item independently shippable. Do not start a wide rewrite that
  spans planner, storage, server, and runtime layers without a committed owning
  item in this plan.
- If a later item reveals that an earlier item needs a different seam, amend
  this plan before implementing the alternative.
- For any item that changes steady-state performance or background behavior,
  add or extend the metrics needed to validate that change in production-like
  operation.

### Status model

- `todo`: not started
- `in_progress`: actively being implemented; keep exactly one item in this
  state during a single autonomous run unless this plan explicitly allows a
  safe batch
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification is recorded in the
  execution log
- `deferred`: intentionally parked behind a stronger prerequisite or a design
  gate

### Recovery loop for every new session or post-compaction resume

1. Reread this plan's `Execution Log`, `Roadmap Status Ledger`, `Dependency
   Graph`, and `Recommended Delivery Order`, then inspect the current git
   worktree.
2. If any item is `in_progress`, resume it first.
3. Reconcile dirty worktree changes to an owning item before choosing new
   scope.
4. Implement exactly one roadmap item by default.
5. Add or extend deterministic tests first.
6. Update this plan's ledger and execution log in the same change set as the
   code or docs.

### Minimum verification per implementation item

- targeted tests for the touched crate or subsystem
- targeted regressions or benchmarks for the claimed improvement
- relevant metrics or observability checks for the changed path
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

For cross-cutting engine or server items, also run:

- `cargo test -p nimbus-engine`
- `cargo test -p nimbus-server`

For planner or storage-read changes, also run:

- `cargo test -p nimbus-core`

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| SA0 | completed | Introduced a thin owned-task primitive and migrated the native WebSocket session path to explicit shutdown-and-drain child task ownership | none |
| SA1 | completed | Moved subscription re-evaluation onto a tenant-local async delivery worker with bounded queueing, monotonic sequence guards, and explicit overflow fallback metrics | SA0 |
| SA2 | completed | Coalesce cache invalidation and subscription fanout across durable-apply batches, with explicit merged-batch delivery semantics and observability | SA1 |
| SA3 | completed | Retrofit owned task lifetimes across the remaining Convex socket, subscription forwarder, and runtime bridge flows | SA0 |
| SA4 | completed | Add composite indexes and planner support for multi-field query shapes behind a design-note gate, including tuple-capable pagination payloads and observable composite-vs-fallback plan stats | none |
| SA5 | completed | Add conservative scan-time predicate pushdown before full document deserialization, with observable pushdown-vs-decode scan stats | none |
| SA6 | completed | Reduced remaining obvious owned-path cloning in journal planning and direct Convex execution-unit reads while preserving existing subscription invalidation semantics | none |
| SA7 | completed | Hardened `MutationExecutionUnit` into a single-use API that rejects reads, writes, and repeat commits after any commit attempt finalizes the unit | none |
| SA8 | completed | Measured the current read formats, kept zero-copy deferred, and landed the first serving-path promotion onto tenant-local materialized documents for warmed full-scan reads | SA4, SA5 |

---

## Dependency Graph

- `SA0` is the foundation for any new background-task ownership.
- `SA1` depends on `SA0` so the new subscription delivery workers are born into
  an owned task model instead of introducing another detached-task seam.
- `SA2` depends on `SA1` because batch coalescing should build on the new
  asynchronous subscription delivery architecture instead of the current inline
  path.
- `SA3` depends on `SA0` and retrofits the same task-ownership primitives
  across the remaining existing socket and runtime flows.
- `SA4`, `SA5`, `SA6`, and `SA7` can proceed independently once selected.
- `SA5` is intentionally independent of `SA4`.
  It can improve table and fallback scans even before composite indexes land,
  though the two items should coordinate on shared scan seams if they run near
  each other.
- `SA8` is intentionally deferred until the nearer-term planner and scan work
  stabilizes.

---

## Recommended Delivery Order

1. `SA0`
2. `SA1`
3. `SA2`
4. `SA3`
5. `SA4`
6. `SA5`
7. `SA6`
8. `SA7`
9. `SA8`

---

## Work Items

### SA0. Introduce scoped task primitives for background workers and session-owned tasks

**Priority:** highest prerequisite  
**Expected impact:** gives `SA1` and later task-lifecycle work a shared parent
ownership model instead of adding more ad hoc spawned tasks first and cleaning
them up later.

#### Current verified state

- WebSocket, subscription forwarder, and runtime bridge paths still rely on ad
  hoc spawned tasks
- bounded channels help memory safety, but task lifetimes are still largely
  manual

#### Implementation plan

1. Introduce `JoinSet`, `TaskTracker`, or an equivalent scoped-task owner for
   tenant-scoped workers and socket-session child tasks.
2. Define the minimal ownership API needed by `SA1`:
   - spawn child work under an owning parent
   - stop or drain children on parent shutdown
   - observe completion or cancellation deterministically in tests
3. Land the primitive first, even if only one representative path uses it in
   the initial slice.
4. Keep the primitive intentionally thin.
   Do not build a generalized task-management framework beyond what `SA1` and
   `SA3` need for owned worker and session-task lifetimes.

#### Files likely to change

- `crates/nimbus-engine/src/tenant.rs`
- `crates/nimbus-server/src/ws/socket.rs`
- `crates/nimbus-server/src/runtime/subscriptions.rs`
- `crates/nimbus-server/src/adapters/convex/subscriptions/socket/`
- `crates/nimbus-server/tests/reactive_loop.rs`

#### Acceptance criteria

- a shared owned-task primitive exists for the follow-on work
- at least one representative session or background path already uses it
- disconnect or shutdown tests prove task cleanup instead of relying on drop
- the primitive stays narrow enough that later items can reuse it without
  inheriting an unnecessary abstraction layer

---

### SA1. Move subscription re-evaluation off the synchronous write return path

**Priority:** highest  
**Expected impact:** removes subscription fanout latency as a direct multiplier
on mutation completion time.

#### Current verified state

- `Service::process_commit(...)` still computes affected subscriptions and
  re-evaluates them inline before the write path returns
- bounded channels now prevent unbounded memory growth, but they do not remove
  the inline CPU and query cost

#### Pinned subscription consistency contract

The first implementation slice for `SA1` must preserve this semantic contract:

1. **Applied-state boundary:** subscribers may observe only applied state, never
   durable-but-unapplied state.
2. **Per-subscription monotonicity:** a subscription must never observe older
   visible state after newer visible state. Delivered results must correspond to
   a nondecreasing applied sequence per subscription.
3. **Coalescing is allowed:** subscribers are guaranteed convergence to the
   latest affected applied state, not one notification per commit. Consumers
   must not infer commit counts from update counts.
4. **Commit metadata is not a per-commit delivery guarantee:** if work is
   coalesced, intermediate commit identities may be omitted from subscriber
   events or collapsed to the latest represented commit. Tests should assert
   visible-state and monotonic-sequence semantics rather than one-event-per
   commit behavior.
5. **Freshness is observable, not hard-SLA-backed, in the first slice:** the
   implementation must expose queue depth, queue age, and re-evaluation
   latency, but it does not promise a wall-clock notification SLA yet.

#### Implementation plan

1. Introduce a tenant-scoped or service-scoped subscription work queue with a
   bounded capacity and explicit overflow behavior, built on the task-ownership
   primitive from `SA0`.
2. Make the write path responsible only for:
   - durable append
   - applied materialization
   - invalidating caches
   - enqueueing subscription work
3. Run re-evaluation in dedicated background workers after apply visibility is
   guaranteed.
4. Encode the pinned contract above directly in tests before or alongside the
   implementation.
5. Add metrics for queue depth, dropped work, coalesced work, and re-evaluation
   latency.

#### Files likely to change

- `crates/nimbus-engine/src/service/mutations.rs`
- `crates/nimbus-engine/src/service/subscriptions.rs`
- `crates/nimbus-engine/src/subscriptions.rs`
- `crates/nimbus-engine/src/tenant.rs`
- `crates/nimbus-engine/src/tests.rs`
- `crates/nimbus-server/tests/reactive_loop.rs`

#### Acceptance criteria

- synchronous mutation completion no longer waits on full subscription
  re-evaluation
- the pinned subscription consistency contract above is explicitly tested
- queue overflow behavior is explicit, tested, and observable through metrics

---

### SA2. Coalesce invalidation and fanout across durable-apply batches

**Priority:** highest after `SA1`  
**Expected impact:** reduces repeated invalidation and affected-subscription
scans when the journal worker applies multi-record batches.

#### Current verified state

- journal batch apply already exists
- commit processing still executes per commit instead of per coalesced batch

#### Implementation plan

1. Add a batch-aware commit-processing path that can merge:
   - affected tables
   - candidate documents
   - deleted documents
   - subscription wakeups
2. Coalesce only where semantics are preserved.
   If per-commit metadata still has to be retained for subscriber payloads,
   document the exact contract and split only the parts that are safely
   mergeable.
3. Make the journal worker hand one coalesced invalidation unit to the new
   subscription-delivery pipeline.
4. Add metrics for coalesced batch size, merged subscription wakeups, and any
   dropped or squashed work.

#### Files likely to change

- `crates/nimbus-engine/src/service/mutations/journal.rs`
- `crates/nimbus-engine/src/service/mutations.rs`
- `crates/nimbus-engine/src/subscriptions.rs`
- `crates/nimbus-engine/src/tests.rs`

#### Acceptance criteria

- batch apply does not re-run equivalent invalidation work once per record when
  the work can be merged safely
- subscription outputs remain semantically correct under coalescing
- coalescing behavior is visible through metrics and deterministic tests

---

### SA3. Retrofit structured concurrency across remaining socket, subscription, and runtime flows

**Priority:** high  
**Expected impact:** removes detached task lifetime drift and makes disconnect,
auth-change, and shutdown cleanup deterministic.

#### Current verified state

- `SA0` introduces the shared ownership primitive, but the rest of the existing
  socket and runtime flows will still need to be migrated onto it

#### Implementation plan

1. Move the remaining WebSocket, subscription forwarder, and runtime bridge
   paths onto the shared ownership primitive from `SA0`.
2. Make disconnect, auth-change, and shutdown explicitly terminate child tasks.
3. Remove detached task patterns where the parent no longer owns cleanup.
4. Add regressions proving that child tasks exit when their parent session or
   request ends.
5. Add metrics or debug counters where needed so lingering task leaks would be
   visible in testing and operation.

#### Files likely to change

- `crates/nimbus-server/src/ws/socket.rs`
- `crates/nimbus-server/src/runtime/subscriptions.rs`
- `crates/nimbus-server/src/adapters/convex/subscriptions/socket/`
- `crates/nimbus-server/tests/reactive_loop.rs`

#### Acceptance criteria

- parent socket and runtime flows own their spawned child tasks
- disconnect and auth-change tests prove cleanup instead of relying on drops
- task ownership is visible enough to diagnose leak regressions

---

### SA4. Add composite indexes and planner support for multi-field query shapes

**Priority:** high  
**Expected impact:** unlocks the most important missing indexed query patterns
instead of forcing residual in-memory filtering and sorting.

#### Current verified state

- composite indexes are now first-class schema objects with
  `IndexDefinition { name, fields }`
- storage maintenance and schema backfill understand composite index keys and
  only emit entries when every indexed field is present
- non-paginated query planning now supports:
  - longest exact-prefix composite scans
  - exact-prefix plus one-range-on-the-next-field composite scans
  - preferring the composite plan when it satisfies the requested order better
- paginated reads now reuse the same composite exact-prefix planning shapes
  when the existing user-visible cursor semantics remain sufficient
- cursor payloads and dependency boundary payloads now use tuple-capable
  vector payloads while the current evaluator still populates one visible
  order slot today
- query planning stats now distinguish composite-index selections from
  single-field and full-scan fallback plans in deterministic service tests

#### Design-note gate

Do not start broad implementation until a short design note exists covering:

- composite index schema shape and validation
- multi-field key encoding and prefix-scan ordering
- write-path maintenance and backfill behavior
- planner matching rules and residual-filter stripping
- pagination cursor encoding and ordering stability
- dependency tracking and invalidation semantics for composite range reads

Design note:

- `docs/research/composite-index-design-note.md`

If Stage A lands cleanly but Stage B or Stage C proves materially larger than
expected, split `SA4` into explicit follow-on sub-items in this plan before
continuing instead of carrying hidden multi-session scope inside one `in_progress`
item.

#### Implementation plan

1. Stage A: extend schema and storage metadata to define composite indexes
   explicitly, including backfill behavior for existing tables.
2. Stage B: add planner support for:
   - exact prefix matches
   - filter-on-A plus order-by-B when supported by index layout
   - residual filters only when necessary
3. Stage C: extend query, pagination, cursor, and dependency tests to prove
   stable semantics on composite indexed reads.
4. Add metrics or benchmarks that let us compare composite-index plans against
   equivalent fallback plans.

#### Files likely to change

- `crates/nimbus-core/src/schema.rs`
- `crates/nimbus-engine/src/service/queries.rs`
- `crates/nimbus-storage/src/`
- `crates/nimbus-engine/src/tests.rs`
- `crates/nimbus-server/src/tests/`
- `ARCHITECTURE.md`

#### Acceptance criteria

- composite indexes are first-class schema objects
- planner can pick composite indexes for supported multi-field shapes
- query and pagination semantics remain deterministic
- the design note exists before broad implementation starts

---

### SA5. Add scan-time predicate pushdown before full document deserialization

**Priority:** high  
**Expected impact:** lowers CPU cost on large selective scans by rejecting clear
non-matches before building full document objects.

#### Current verified state

- fallback table scans now compile a conservative pushdown subset for
  `Eq/Gt/Gte/Lt/Lte` filters over scalar values
- storage probes only the needed MessagePack field values and rejects definite
  non-matches before full `Document` deserialization
- unsupported filters or unsupported field-value domains defer to the full
  decode-and-evaluate path with unchanged semantics
- storage scan stats now expose scanned row count, decoded row count, and
  pushdown-rejected row count so deterministic tests can measure pushdown hits

#### Coordination note

`SA5` is independent of `SA4` and may proceed before composite indexes land.
If both items are active near each other, coordinate on shared scan and
planner seams instead of treating either item as blocked on the other.

#### Implementation plan

1. Define the initial safe predicate subset for pushdown:
   - simple equality
   - simple scalar ranges
   - no authorization-sensitive shortcut that changes semantics
2. Add a raw MessagePack field probe or equivalent partial decode path for that
   subset.
3. Keep the fallback to full deserialization for unsupported predicates.
4. Add microbenchmarks or deterministic performance assertions for representative
   selective scans.
5. Add metrics or counters that distinguish pushdown hits from full-decode
   fallback cases.

#### Files likely to change

- `crates/nimbus-storage/src/`
- `crates/nimbus-engine/src/service/queries.rs`
- `crates/nimbus-engine/src/tests.rs`
- `ARCHITECTURE.md`

#### Acceptance criteria

- supported simple filters can reject non-matching rows before full
  deserialization
- unsupported predicates still take the full-deserialize path with unchanged
  semantics
- pushdown hit rate or fallback rate is observable in tests or metrics

---

### SA6. Opportunistically reduce remaining hot-path cloning in journal planning and owned runtime responses

**Priority:** medium  
**Expected impact:** trims avoidable allocation and copy cost now that the
largest obvious JSON-clone issue is already fixed.

#### Current verified state

- `Document::into_json()` is now used across the remaining direct Convex
  execution-unit read path, not just the earlier HTTP and query helpers
- journal planning now keeps only the minimal queued-request state needed after
  planning, borrows durable-record batches across append and apply, and derives
  subscription deleted-document payloads from committed writes instead of
  carrying duplicate owned vectors through the batch pipeline

#### Delivery note

This item is intentionally narrower than the rest of the plan and may be
batched opportunistically with nearby work that already touches the same files.
It remains in this plan because the review identified it as a measurable
hot-path cleanup item, but it does not need to block the larger architectural
changes.
In particular, if `SA1` or `SA2` already has `journal.rs` or nearby owned
response paths open, prefer batching the relevant `SA6` cleanup in that same
change set rather than forcing a separate pass later.

#### Implementation plan

1. Audit remaining owned-document call sites and switch them to move-based
   conversion where ownership already exists.
2. Simplify journal planning to avoid redundant clones when a value is already
   owned and no longer reused.
3. Add focused benchmarks or allocation-sensitive regressions where possible.
4. Add metrics, benchmarks, or allocation assertions that make the win visible.

#### Files likely to change

- `crates/nimbus-engine/src/service/mutations/journal.rs`
- `crates/nimbus-server/src/adapters/convex/host_bridge/`
- `crates/nimbus-server/src/adapters/convex/execution/`
- `crates/nimbus-engine/src/tests.rs`

#### Acceptance criteria

- remaining obvious owned-path clones are removed
- behavior remains unchanged and verified by existing query and mutation tests
- the reduced-clone path is measurable through focused verification

---

### SA7. Harden `MutationExecutionUnit` lifecycle constraints

**Priority:** medium  
**Expected impact:** makes incorrect execution-unit usage harder by
construction, reducing the surface for future correctness regressions.

#### Current verified state

- execution units are now single-use: once `commit()` is attempted, the unit is
  finalized whether the commit succeeds, conflicts, or becomes a no-op
- post-finalization reads, writes, scheduler calls, dependency recording, and
  repeat commits now fail clearly instead of silently reusing stale state
- focused runtime bridge regressions and the full workspace suite both pass

#### Implementation plan

1. Decide whether type-state is the right full solution or whether a smaller
   API restriction gives most of the value with less churn.
2. Make illegal phase transitions impossible or clearly unrepresentable where
   feasible.
3. Preserve existing OCC, auth, and commit regressions while reshaping the API.
4. Add or update diagnostics so misuse is easier to identify if a smaller API
   restriction is chosen instead of full type-state.

#### Files likely to change

- `crates/nimbus-engine/src/service/execution_units.rs`
- `crates/nimbus-engine/src/lib.rs`
- `crates/nimbus-engine/src/tests.rs`
- `ARCHITECTURE.md`

#### Acceptance criteria

- the execution-unit API exposes fewer invalid call orders
- existing conflict and lifecycle tests still pass
- the chosen restriction is documented clearly enough for embedders and tests

---

### SA8. Evaluate zero-copy or materializer-native read formats

**Priority:** completed  
**Expected impact:** potentially large read-path gains, but only after the
nearer-term planner and scan work is complete and measured.

#### Gate

Do not start this item until `SA4` and `SA5` are done and their measured impact
is recorded. This item is intentionally deferred.

#### Current verified state

- a release-mode measurement prototype now exists at
  `crates/nimbus-storage/benches/read-format-evaluation.rs`
- the first concrete serving-path promotion now also exists at
  `crates/nimbus-engine/benches/materialized-surface-evaluation.rs`
- the engine now keeps a tenant-local materialized table surface for warmed
  full-scan tables and reuses it for:
  - full-scan query reads
  - full-scan paginated reads
  - warmed `get_document` lookups on already-loaded tables
  - subscription re-evaluation for those same query shapes
- the local materialized read surface is now updated before the applied
  watermark advances, so "applied-visible" reads cannot observe stale warmed
  tables after a mutation acknowledgement
- subscription activation now records the bootstrap floor sequence it already
  covered, so reconnects that begin during apply lag do not replay the same
  commit twice once the initial catch-up result arrives
- on a 20,000-row dataset with 1 KiB payloads:
  - selective fallback full-deserialize scan averaged about `16.50 ms`
  - selective `SA5` pushdown averaged about `10.36 ms` (`1.59x` faster)
  - selective materialized-document reads averaged about `0.33 ms`
    (`50.39x` faster)
  - broad full-deserialize scan averaged about `17.58 ms`
  - broad materialized-document reads averaged about `0.21 ms`
    (`84.52x` faster)
- on the first promoted service-level serving path
  (`3` warmed tables, `2,000` rows per table, `1 KiB` payloads, `keep_every=97`):
  - cold full-scan service query averaged `6.530611 ms`
  - warm materialized-surface service query averaged `1.485014 ms`
    (`4.40x` faster, `-77.26%` change)
- the dominant remaining win is "query already-materialized documents" rather
  than "teach redb-backed MessagePack rows a new zero-copy format"

#### Decision

Keep zero-copy or materializer-native format work deferred for now. If Nimbus
needs another major read-latency step after `SA4` and `SA5`, the next promoted
investment should continue serving selected read classes from already-
materialized documents first. The first tenant-local warmed full-scan
promotion already validated that direction with a measured `4.40x` service-
level improvement, so future work should keep climbing that ladder through
shadow-materializer or embedded-replica serving surfaces before inventing a
new binary format inside the redb-backed store. Only revisit a new binary
format if those higher-leverage serving-path moves still leave MessagePack
decode as the dominant cost.

#### Implementation plan

1. Compare:
   - current MessagePack full-deserialize path
   - partial-decode path after `SA5`
   - zero-copy or materializer-native formats for derived read paths
2. Produce a short design note or prototype before any large rewrite.
3. Choose only if the measured gains justify the format and evolution cost.

#### Acceptance criteria

- a concrete design decision is documented from measurements, not guesswork

---

## Execution Log

| Date | Item | Outcome | Notes |
| --- | --- | --- | --- |
| 2026-04-02 | baseline | created | Created this plan after landing the reproducible follow-on fixes from the April 2026 architecture review pass: execution-unit table-view optimization, schema `Arc` snapshots, move-based JSON conversion, bounded document cache, hash-backed dependency dedup, and bounded subscription or socket channels. Verified with `cargo test -p nimbus-core -p nimbus-engine -p nimbus-server`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings`. |
| 2026-04-02 | plan | refined | Incorporated review feedback: added `SA0` task-scoping prerequisite, pinned the `SA1` subscription consistency contract, removed the hard `SA5 -> SA4` dependency, expanded `SA4` behind a design-note gate, strengthened observability requirements across items, and clarified that `SA6` is an opportunistic hot-path cleanup item rather than the plan's architectural center of gravity. |
| 2026-04-02 | plan | polished | Incorporated final minor observations: tightened `SA0` against framework drift, documented the `SA4` split point if its staged work grows larger than expected, and made `SA6` explicitly batchable with `SA1` or `SA2` when those files are already open. |
| 2026-04-02 | SA0 | completed | Added `OwnedTaskSet` in `nimbus-server` as the narrow shared task-ownership primitive for follow-on socket and worker lifetimes, then migrated the native `/ws` session path to own its forwarder and sender tasks through explicit `shutdown_and_drain()` cleanup instead of ad hoc `tokio::spawn` handles. Verified with `cargo test -p nimbus-server shutdown_and_drain_aborts_pending_children_deterministically -- --nocapture`, `cargo test -p nimbus-server websocket_disconnect_drops_subscription_without_explicit_unsubscribe -- --nocapture`, `cargo test -p nimbus-server`, `cargo fmt --all --check`, and `cargo clippy -p nimbus-server --all-targets -- -D warnings`. |
| 2026-04-02 | SA1 | completed | Replaced inline subscription re-evaluation on the write return path with a tenant-local delivery worker and bounded queue. `process_commit()` now invalidates caches, computes affected subscription ids, and enqueues work after the applied watermark advances; the worker re-evaluates deliveries in sequence order, guards per-subscription monotonicity with delivered-sequence tracking, and records queue depth, queue age, overflow fallback, coalesced stale-work, and re-evaluation latency stats. The first slice keeps overflow behavior explicit by falling back to immediate delivery when the queue is saturated, while older queued results are skipped if a newer sequence has already been delivered. Verified with `cargo test -p nimbus-engine service_mutation_returns_while_subscription_delivery_worker_is_blocked -- --nocapture`, `cargo test -p nimbus-engine subscription_delivery_queue_overflow_falls_back_without_regressing_monotonicity -- --nocapture`, `cargo test -p nimbus-engine`, `cargo test -p nimbus-server`, `cargo test --workspace`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings`. |
| 2026-04-02 | SA2 | completed | The durable-journal worker now processes applied batches through one batch-aware post-apply path instead of replaying `process_commit()` once per record. Cache invalidation runs once per batch, the subscription registry scans active subscriptions once across the whole applied batch, repeated wakeups for the same subscription are merged into a single queued delivery unit, and merged batches intentionally omit per-commit metadata in subscriber payloads while retaining the latest applied sequence for monotonic delivery. The queue now records coalesced batch count, merged commit count, and merged wakeup count, and the deterministic journal pause seam was made async-friendly so batch regressions do not block the Tokio runtime. Verified with `cargo test -p nimbus-engine journal_batch_coalesces_subscription_delivery_into_one_update -- --nocapture`, `cargo test -p nimbus-engine`, `cargo test --workspace`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings`. |
| 2026-04-02 | SA3 | completed | Migrated the remaining Convex WebSocket child tasks and runtime subscription bridge tasks onto `OwnedTaskSet`, so the session now explicitly owns its forwarder and sender loops and each runtime-backed active subscription owns the bridge tasks that rewrite underlying engine subscription events. Disconnect, auth-change, unsubscribe, and end-of-session teardown now explicitly unsubscribe active subscriptions and drain owned child tasks instead of relying on detached `JoinHandle`s or dropped cleanup handles. Added a small `OwnedTaskSet` debug count to make task ownership easier to inspect in tests. Verified with `cargo test -p nimbus-server websocket_disconnect_drops_subscription_without_explicit_unsubscribe -- --nocapture`, `cargo test -p nimbus-server convex_websocket_disconnect_releases_runtime_subscription_children -- --nocapture`, `cargo test -p nimbus-server convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed -- --nocapture`, `cargo test -p nimbus-server`, `cargo test --workspace`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings`. |
| 2026-04-02 | SA4 | design gate created | Added `docs/research/composite-index-design-note.md` to pin the first composite-index slice before broad implementation. The note covers the proposed schema shape (`IndexDefinition { name, fields }`), composite key encoding, missing-field semantics, transactional backfill behavior, supported planner shapes, tuple-based cursor generalization, and the decision to keep dependency tracking conservative while planner support lands. Verified the current baseline against the imported review carry-over with `cargo test -p nimbus-engine waiting_for_applied_visibility -- --nocapture`, `cargo test -p nimbus-core durable_mutation_record_roundtrips_and_verifies_integrity -- --nocapture`, and `cargo test -p nimbus-engine mutation_execution_unit_ -- --nocapture` before starting `SA4`. |
| 2026-04-02 | SA4 | Stage A completed | Landed the schema and storage half of composite-index support without yet widening planner behavior. `IndexDefinition` now uses `fields`, validation rejects empty or duplicate field lists, the storage layer builds and backfills composite keys only when every indexed field is present, and Stage A keeps the query planner conservative by ignoring composite indexes until the next slice proves planner semantics, cursors, and dependency tracking. Updated HTTP and reactive-loop schema fixtures to the new canonical `indexes[].fields` payload shape instead of preserving the removed single-field compatibility format. Verified with `cargo check -p nimbus-core -p nimbus-storage -p nimbus-engine -p nimbus-server`, `cargo test -p nimbus-core schema_rejects_index -- --nocapture`, `cargo test -p nimbus-storage composite_index_ -- --nocapture`, `cargo test -p nimbus-engine composite_indexes -- --nocapture`, `cargo test --workspace`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings`. |
| 2026-04-02 | SA4 | Stage B slice 1 completed | Added the first planner-visible composite-index support without widening cursor semantics yet. Non-paginated query paths now choose longest exact-prefix composite scans and exact-prefix-plus-one-range composite scans, prefer the composite candidate when it satisfies the requested order better, and strip exact-prefix plus satisfied range filters from the residual query. Storage now exposes explicit composite prefix scans and exact-prefix plus one-range scans, and the planner keeps paginated reads on the single-field-only path until Stage C lands the cursor and dependency follow-through. Verified with `cargo test -p nimbus-storage composite_index_ -- --nocapture`, `cargo test -p nimbus-engine service::queries::tests::plan_ -- --nocapture`, `cargo test -p nimbus-engine`, `cargo test --workspace`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings`. |
| 2026-04-02 | SA4 | Stage C slice 1 completed | Lifted the conservative paginated-planner restriction for the already-supported composite exact-prefix shapes. Paginated reads now reuse the composite planner when it is only narrowing candidate documents and the existing cursor semantics still page on the user-visible order field after in-memory sorting, so no tuple-cursor rewrite was required for this slice. Added targeted planner and service regressions to prove second-page cursor progress on a composite index while keeping the cursor and dependency payload shapes unchanged. Verified with `cargo test -p nimbus-engine service::queries::tests::plan_ -- --nocapture`, `cargo test -p nimbus-engine paginated_query_uses_composite_index_for_exact_prefix_and_cursor_progress -- --nocapture`, `cargo test -p nimbus-engine`, `cargo test --workspace`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings`. |
| 2026-04-02 | SA4 | Stage C slice 2 completed | Finished the cursor/dependency and observability follow-through for the supported composite pagination shapes. Pagination cursors, execution-unit dependency boundaries, runtime read-tracking boundaries, and Convex pagination helpers now all use tuple-capable sort-value vectors while preserving the current one-visible-order-slot comparison semantics. The engine also now records query-plan stats that distinguish composite-index selections from single-field index and full-scan fallback paths, so deterministic service tests can prove the planner is actually choosing the composite path. Verified with `cargo test -p nimbus-engine service::queries::tests::plan_ -- --nocapture`, `cargo test -p nimbus-engine paginated_query_uses_composite_index_for_exact_prefix_and_cursor_progress -- --nocapture`, `cargo test -p nimbus-engine query_planning_stats_distinguish_composite_single_field_and_fallback_paths -- --nocapture`, `cargo test --workspace`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings`. |
| 2026-04-02 | SA5 | completed | Added the first conservative scan-time predicate pushdown slice for fallback table scans. `nimbus-storage` now probes only the targeted MessagePack field values for supported `Eq/Gt/Gte/Lt/Lte` filters, rejects definite non-matches before full `Document` deserialization, and records scan stats that expose scanned rows, decoded rows, and pushdown-rejected rows. `nimbus-engine` routes fallback query and paginated scan paths through that filtered storage seam, while unsupported filters and unsupported value domains still defer to the full decode-and-evaluate path so externally visible semantics stay unchanged. Verified with `cargo test -p nimbus-storage scan_pushdown_rejects_selective_rows_before_full_decode -- --nocapture`, `cargo test -p nimbus-storage range_scan_pushdown_rejects_out_of_range_rows_before_full_decode -- --nocapture`, `cargo test -p nimbus-storage unsupported_scan_filters_fall_back_to_full_decode -- --nocapture`, `cargo test -p nimbus-engine fallback_query_filters_during_scan_for_selective_match -- --nocapture`, `cargo test -p nimbus-engine paginated_fallback_scan_preserves_cursor_and_ordering -- --nocapture`, `cargo test -p nimbus-engine evaluator_range_filter_on_unsupported_field_type_still_errors_after_pushdown_defers -- --nocapture`, `cargo test --workspace`, `cargo fmt --all --check`, and `cargo clippy --workspace --all-targets -- -D warnings`. |
| 2026-04-02 | SA6 | completed | Removed the remaining obvious owned-path clone seams without widening the public semantics surface. The Convex direct execution-unit read path now uses `Document::into_json()` anywhere it already owns `Document` values, so staged runtime reads no longer deep-clone field maps just to add system fields. The journal mutation path now keeps only minimal post-plan request state, borrows one durable-record batch across append and apply instead of cloning the record vector, and derives inserted/deleted subscription payload inputs from `CommitEntry` writes rather than carrying duplicate candidate/deleted document vectors through the batch. The follow-up regression also pinned the intended invalidation contract: inserts keep the inserted document as a candidate, deletes keep the deleted document as a candidate, and updates intentionally stay conservative by carrying no candidate document so maybe-affected filtered subscriptions still wake up. Verified with `cargo test -p nimbus-engine journal_batch_ -- --nocapture`, `cargo test -p nimbus-engine service_delete_only_notifies_filtered_subscriptions_for_matching_documents -- --nocapture`, `cargo test -p nimbus-engine service_updates_remain_conservative_for_filtered_subscriptions -- --nocapture`, `cargo test -p nimbus-server runtime_mutation_bridge_stages_writes_until_commit_and_reads_its_own_writes -- --nocapture`, `cargo fmt --all --check`, `make clippy`, and `git diff --check`. `make test` remains partially blocked in this sandbox because many `nimbus-server` tests bind loopback listeners and fail here with `Operation not permitted`; the focused server path above passed locally inside the sandbox, and the bind failures are environmental rather than caused by this slice. |
| 2026-04-02 | SA7 | completed | Hardened `MutationExecutionUnit` with a smaller single-use lifecycle restriction instead of a full type-state rewrite. Once `commit()` is attempted, the unit transitions out of the active phase permanently: successful, conflicting, and no-op commit attempts all finalize the unit, and any later reads, writes, scheduler staging, dependency recording, or repeat commits now return a clear invalid-input error instead of reusing stale state. This closes the easy-to-misuse reuse seam without widening the public API surface or disturbing existing OCC and bridge semantics. Verified with `cargo test -p nimbus-engine mutation_execution_unit_ -- --nocapture`, `cargo test -p nimbus-server runtime_mutation_bridge_ -- --nocapture`, `cargo fmt --all --check`, `make clippy`, `make test`, and `git diff --check`. |
| 2026-04-02 | SA8 | completed | Added a release-mode read-format measurement prototype at `crates/nimbus-storage/benches/read-format-evaluation.rs` and used it to compare three concrete paths on the same 20,000-row synthetic workload with 1 KiB payloads: redb-backed fallback full MessagePack deserialization, the `SA5` selective pushdown path, and queries over already-materialized `Document` values. The measured shape was decisive: pushdown improved the selective scan by about `1.59x`, but materialized-document reads were about `50.39x` faster on the selective workload and `84.52x` faster on the broad scan. The chosen design decision is therefore to keep zero-copy format work deferred and spend the next major read-path effort on promoting selected serving paths onto already-materialized documents instead of inventing a new binary format inside the current redb-backed store. Verified with `cargo bench -p nimbus-storage --bench read-format-evaluation`, `cargo test -p nimbus-storage`, `cargo clippy -p nimbus-storage --all-targets -- -D warnings`, and `cargo fmt --all --check`. |
| 2026-04-02 | SA8 | refined | Landed the first concrete serving-path promotion onto already-materialized documents instead of starting a zero-copy format project. The engine now maintains a tenant-local warmed table surface for full-scan tables, reuses it for full-scan query and pagination reads, warmed `get_document` lookups, and subscription re-evaluation, and updates that local read state before advancing the applied watermark so warmed reads stay causally aligned with mutation visibility. Subscription activation also now stamps the bootstrap floor sequence it already covered so reconnects that begin during apply lag do not receive a duplicate replay of the same catch-up commit. The release-mode service benchmark at `crates/nimbus-engine/benches/materialized-surface-evaluation.rs` measured the resulting service-level win on `3` tables with `2,000` rows each and `1 KiB` payloads: cold full-scan queries averaged `6.530611 ms`, while warm materialized-surface queries averaged `1.485014 ms` (`4.40x` faster, `-77.26%` change). Verified with `cargo test -p nimbus-engine async_paginated_full_scans_reuse_and_refresh_materialized_surface_after_async_writes -- --nocapture`, `cargo test -p nimbus-server convex_mutation_dispatches_existing_document_operations -- --nocapture`, `cargo test -p nimbus-server convex_named_unique_query_returns_document_null_or_error -- --nocapture`, `cargo test -p nimbus-server convex_named_mutation_dispatches_compiled_patch_and_delete -- --nocapture`, `cargo test -p nimbus-server embedded_replica_matches_server_results_and_catches_up_after_http_writes -- --nocapture`, `cargo test -p nimbus-server --test reactive_loop websocket_reconnect_and_resubscribe_catches_up_after_apply_lag_and_keeps_pushing -- --nocapture`, `make test`, and `cargo bench -p nimbus-engine --bench materialized-surface-evaluation`. |
