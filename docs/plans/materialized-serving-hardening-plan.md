# Materialized Serving Hardening Plan

This is the canonical execution roadmap for hardening the first promoted
materialized-document serving path from `SA8` into something enterprise-safe,
measurable, and extensible.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/scalability-and-architecture-follow-on-plan.md`
- `crates/neovex-engine/src/tenant.rs`
- `crates/neovex-engine/src/service/queries.rs`
- `crates/neovex-engine/src/service/mutations.rs`
- `crates/neovex-engine/src/service/mutations/journal.rs`
- `crates/neovex-engine/src/service/subscriptions.rs`
- `crates/neovex-engine/src/subscriptions.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/snapshot_manager.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/token.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/metrics.rs`
- `/Users/jack/src/github.com/tigerbeetle/tigerbeetle/docs/ARCHITECTURE.md`
- `/Users/jack/src/github.com/cockroachdb/cockroach/docs/RFCS/20191108_closed_timestamps_v2.md`
- `/Users/jack/src/github.com/cockroachdb/cockroach/docs/RFCS/20230328_low_latency_changefeeds.md`
- `/Users/jack/src/github.com/cockroachdb/cockroach/pkg/kv/kvserver/kvserverpb/state.proto`
- `/Users/jack/src/github.com/cockroachdb/cockroach/pkg/kv/txn.go`
- `/Users/jack/src/github.com/cockroachdb/cockroach/pkg/ccl/changefeedccl/tableset/watcher.go`
- `/Users/jack/src/github.com/cockroachdb/cockroach/pkg/ccl/changefeedccl/metrics.go`

---

## Purpose

`SA8` proved the central thesis correctly: promoting selected reads onto
already-materialized document state can beat another round of redb-side binary
format work by a wide margin.

What `SA8` did not yet do is turn that first serving-path promotion into a
canonical long-term subsystem. The current tenant-local materialized surface is
useful and measurably faster, but it is still closer to a warmed mutable read
cache than to a versioned serving snapshot with explicit enterprise-grade
correctness boundaries.

This plan exists to close that gap.

The immediate objective is to harden four specific areas:

1. exact bootstrap snapshot sequencing for subscriptions and other bootstrap
   readers
2. atomic publication of warmed materialized tables with explicit covered
   sequence metadata
3. bounded memory and operator-visible metrics for the materialized surface
4. deterministic adversarial verification of the race boundaries that matter

The secondary objective is to document the most canonical next step after these
four land, so this work does not accidentally evolve into an ad hoc permanent
cache layer.

---

## Why This Fits Neovex

This plan is intentionally specific to Neovex's product shape:

- single binary
- per-tenant embedded storage
- embedded V8 runtime for user code
- server-authoritative reactive live queries
- scheduling and cron in the same process

For this architecture, promoting selected hot reads onto already-materialized
document state is advantageous because Neovex pays no network hop or process
boundary to do so. Every extra scan, decode, or repeated query evaluation
happens inside the same binary that already owns the authoritative journal,
apply loop, runtime host bridge, and subscription fan-out.

That makes the `SA8` promotion the right tactical optimization. It does **not**
mean Neovex should evolve a second ad hoc database inside `TenantRuntime`.
The right long-term pattern for this product is:

1. journal-first authoritative writes
2. explicit apply-ordered visibility
3. derived serving state published at exact covered frontiers
4. reactive bootstrap and live delivery anchored to the same frontier
5. bounded, observable in-memory serving state

This is closer to Convex's explicit snapshot/timestamp model and Cockroach's
served-at frontier discipline than to a generic mutable cache, while still
matching TigerBeetle's core rule that derived state is only valid because the
authoritative ordered apply made it valid.

---

## Why These References Matter

The local reference implementations point in the same direction even though
they live in different systems:

- Convex uses explicit snapshot timestamps and read tokens rather than
  "best-effort current state". In
  `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/snapshot_manager.rs`,
  `SnapshotManager` keeps a bounded `VecDeque<(Timestamp, Snapshot)>`, exposes
  `latest()` and `wait_for_higher_ts(...)`, and treats snapshot advancement as
  an explicit timestamped publication event.
- Convex also externalizes read-state as a token carrying both the read set and
  the exact timestamp in
  `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/token.rs`.
  That is the right conceptual model for bootstrap correctness even when we do
  not expose a user-facing token yet.
- Convex already measures bootstrap and subscription invalidation lag in
  `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/metrics.rs`,
  which is the right observability posture for reactive serving paths.
- TigerBeetle's architecture document is useful here not because it has the
  same query model, but because it is uncompromising about the order between
  committed log application and derived state. In
  `/Users/jack/src/github.com/tigerbeetle/tigerbeetle/docs/ARCHITECTURE.md`,
  committed prepares are applied in sequence order, checkpoints are only a
  derived serving state, and replay reconstructs exact state after crash.
- CockroachDB is useful here not because Neovex should adopt per-range Raft,
  but because it is disciplined about exact read frontiers and change
  propagation. In
  `/Users/jack/src/github.com/cockroachdb/cockroach/docs/RFCS/20191108_closed_timestamps_v2.md`
  and
  `/Users/jack/src/github.com/cockroachdb/cockroach/pkg/kv/kvserver/kvserverpb/state.proto`,
  follower reads are only valid at or below an exact closed timestamp tied to
  applied state. In
  `/Users/jack/src/github.com/cockroachdb/cockroach/pkg/ccl/changefeedccl/tableset/watcher.go`,
  reactive consumers wait for a resolved timestamp before treating schema state
  as valid at a target point. In
  `/Users/jack/src/github.com/cockroachdb/cockroach/pkg/ccl/changefeedccl/metrics.go`,
  progress, checkpoint, and backpressure are operator-visible first-class
  metrics, not incidental debug counters.

Taken together, these references support one principle:

**derived serving state must be versioned, bounded, and causally tied to the
authoritative apply boundary.**

---

## Reference Posture

This plan is intentionally **reference-driven, not reference-copied**.

Neovex should borrow the strongest fitting ideas from each system while
remaining specific to its own architecture. The references are not peers for
every concern:

- **Convex is the primary semantic reference** for reactive bootstrap,
  snapshot-covered reads, and server-authoritative live-query correctness.
  When `MH1` asks "what exactly did this bootstrap result cover?" Convex is the
  closest model.
- **TigerBeetle is the primary discipline reference** for derived-state
  ordering, replay correctness, and bounded internal structures. When `MH2` and
  `MH3` ask "when is derived state valid?" and "what keeps this subsystem
  bounded?", TigerBeetle is the clearest guide.
- **CockroachDB is the primary frontier and observability reference** for exact
  served-at frontiers, wait-until-safe mechanics, checkpoint/resolved progress,
  and backpressure metrics. When `MH2`, `MH3`, and `MH4` ask "how do we name,
  wait for, and observe a safe serving frontier?", Cockroach provides the
  strongest pattern.

If references disagree, prefer the one that best fits Neovex's actual product
shape: single binary, per-tenant embedded storage, embedded V8 runtime,
server-authoritative reactive queries, and scheduling in the same process.

That means this plan should **not** drift into:

- CockroachDB-style per-range replication, leaseholder routing, or descriptor
  leasing as an architectural goal
- TigerBeetle-style whole-program static allocation as a direct requirement
- Convex-specific public API surfaces unless Neovex independently wants them

What Neovex should take is the principle behind those mechanisms, not the
mechanisms themselves.

---

## Relationship To Other Plans

1. `docs/plans/scalability-and-architecture-follow-on-plan.md` remains the
   canonical execution record for the `SA*` architecture cycle that produced
   the first materialized-serving slice.
2. This document owns the follow-on hardening needed to make that slice
   trustworthy as a long-lived architecture direction.
3. If this plan changes architecture-level behavior, update `ARCHITECTURE.md`
   in the same change set.
4. If this plan discovers that the current tenant-local materialized surface
   should be replaced rather than extended, record that here before widening
   the implementation.

---

## Scope

This plan covers:

- exact bootstrap sequence capture and activation for subscription-like readers
- explicit sequence-tagged publication for materialized table warming
- bounded capacity, eviction, and memory/coverage metrics for materialized
  tables
- deterministic race and replay verification around bootstrap, publication,
  and warm-path reuse
- the architectural decision for the post-hardening north star

This plan does not cover:

- widening the current surface to every query shape
- moving the authoritative source of truth away from redb plus journal replay
- a fresh on-disk format redesign
- a full shadow-materializer serving promotion in the same item set
- CockroachDB-style range replication, leaseholders, or distributed read
  negotiation
- TigerBeetle-style global static-allocation requirements across the whole
  binary
- adopting Convex-specific public bootstrap/read token APIs in the same item
  set

---

## Success Criteria

This plan is successful only when all of the following are true:

1. Every bootstrap path that activates a live reactive reader records the exact
   covered apply sequence of its initial result, not an inferred nearby head.
2. A warmed materialized table becomes visible to readers only as a fully
   caught-up publication with an explicit covered sequence.
3. Materialized serving state is bounded in memory and exposes enough metrics
   for operators and tests to understand coverage, churn, hit rate, eviction,
   and memory usage.
4. Deterministic adversarial tests cover concurrent first-load readers,
   bootstrap-plus-write races, reconnect/resubscribe transitions, and repeated
   warm/load invalidation races.
5. The plan ends with a documented architectural recommendation for the next
   canonical serving step after these hardening items are done.

---

## Canonical Design Rules

The following rules are the implementation contract for `MH1` through `MH4`.

1. **Every served result has an exact coverage frontier.** A reactive bootstrap,
   warmed table, or serving read is only valid if Neovex can name the exact
   apply sequence it reflects. "Nearby durable head" and "current best effort"
   are not acceptable substitutes.
2. **Derived publication follows authoritative apply.** Materialized serving
   state is derived only from ordered applied commits. It must never advance
   ahead of the authoritative applied frontier, and it must never be visible at
   a frontier lower than its contents actually reflect.
3. **Bootstrap and live delivery switch at the same frontier.** The last point
   covered by the bootstrap result is the first point excluded from the live
   stream. There is no separate guessed handoff marker.
4. **Readers consume published state, not in-progress assembly.** Building,
   replaying, and warming may be incremental internally, but readers must only
   observe explicitly published state carrying its coverage metadata.
5. **Bounds and metrics are part of correctness.** Enterprise trust requires a
   bounded resident surface and operator-visible coverage, lag, hit, bypass,
   eviction, and memory signals.
6. **Serving semantics stay uniform across surfaces.** `get`, query,
   pagination, and subscriptions must all follow the same exact-coverage
   contract even if their implementation paths differ.

---

## Current Verified State

As of the current verified state:

- the engine has a tenant-local warmed materialized table surface in
  `crates/neovex-engine/src/tenant.rs`
- full-scan query, pagination, warmed get, and subscription re-evaluation can
  reuse that surface through `crates/neovex-engine/src/service/queries.rs`
- the materialized surface is now updated before `applied_head` advances, which
  closed the reproduced stale-read regression after async mutation ack
- subscription bootstrap now carries the exact covered apply sequence of its
  initial snapshot result, and activation queues one catch-up re-evaluation if
  newer applied commits landed while the subscription was still inactive during
  bootstrap
- warmed tables now publish as explicit `{table, covered_sequence, generation,
  documents}` state, and readers only reuse that state when it covers the
  sequence they already waited for
- the materialized surface is now bounded by table count plus byte budget, with
  deterministic table-level eviction and explicit residency, bypass, eviction,
  and in-flight warm-load metrics
- `make test`, `cargo fmt --all --check`, and `make clippy` are green

What still appears architecturally weaker than the local Convex and
TigerBeetle reference patterns:

- the current test matrix proves several important regressions, but it does not
  yet systematically attack all of the new concurrent warm-path races

---

## Execution Contract

### General rules

- Prefer exact sequence semantics over "close enough head" heuristics.
- Treat the materialized surface as derived serving state, not a second source
  of truth.
- Preserve the current measured win while hardening semantics; do not "fix"
  correctness by silently disabling the surface on hot paths.
- Add observability in the same change set as any behavior change.
- When in doubt, prefer the Convex-style explicit snapshot/timestamp pattern
  over ad hoc mutable cache coordination.

### Status model

- `todo`: not started
- `in_progress`: actively being implemented; keep exactly one item here
- `blocked`: cannot proceed until a recorded blocker is resolved
- `done`: acceptance criteria met and verification recorded
- `deferred`: intentionally parked behind a stronger prerequisite

### Recovery loop for every new session

1. Reread this plan's `Execution Log`, `Roadmap Status Ledger`, and
   `Recommended Delivery Order`, then inspect the git worktree.
2. Resume any `in_progress` item first.
3. Reconcile dirty worktree changes to an owning item before starting new
   scope.
4. Implement exactly one roadmap item by default.
5. Add deterministic regressions before widening the implementation.
6. Update this plan and `ARCHITECTURE.md` in the same change set when behavior
   changes.

### Minimum verification per implementation item

- targeted deterministic regressions for the touched race or invariant
- targeted metrics assertions where observability is part of the item
- `cargo test -p neovex-engine`
- `cargo test -p neovex-server`
- `cargo fmt --all --check`
- `make clippy`

For items touching replay, recovery, or bootstrap semantics, also run:

- `make test`

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| MH1 | done | Subscription bootstrap now carries an exact snapshot-covered sequence through activation and enqueues one catch-up delivery if applied commits landed during the inactive bootstrap window | none |
| MH2 | done | Warmed tables now publish as explicit covered-sequence state, readers gate reuse on required coverage, and concurrent first-load races cannot expose partial or stale publication | MH1 |
| MH3 | done | The materialized surface now has bounded table/byte limits, deterministic table-level eviction, and metrics for residency, evictions, bypasses, in-flight warm loads, and coverage range | none |
| MH4 | done | Deterministic engine and reactive-loop regressions now cover repeated warm/evict/rewarm cycles, exact coverage-frontier assertions, and disconnect-during-bootstrap reconnects; the generic websocket route now owns pending bootstrap work correctly and subscription-delivery shutdown avoids self-join teardown | MH1, MH2, MH3 |
| MH5 | done | Authored the versioned serving snapshot design note and set the canonical next step: a `ServingSnapshotManager` abstraction first, with serving-materializer or serving-replica backends only behind that stable contract | MH1, MH2, MH3, MH4 |

---

## Recommended Delivery Order

1. `MH1`
2. `MH2`
3. `MH3`
4. `MH4`
5. `MH5`

---

## Work Items

### MH1. Carry exact bootstrap-covered sequences through registration and activation

**Priority:** highest  
**Expected impact:** closes the remaining semantic gap between "the bootstrap
result was computed around this time" and "the bootstrap result definitely
reflects sequence N".

**Primary references:** Convex first, CockroachDB second

#### Why this matters

At plan creation, the system recorded a bootstrap floor derived from a nearby
journal head. That was enough to fix one reproduced duplicate-delivery bug,
but it was still weaker than a true explicit snapshot contract.

Convex's local model is the reference point here:

- `SnapshotManager::latest()` returns a typed repeatable timestamp, not just a
  mutable latest view.
- `Token` externalizes both read set and exact timestamp.

We should copy the principle even if we do not expose a public token yet.

#### Implementation plan

1. Define an internal bootstrap result type for reactive readers:
   - initial documents
   - exact covered sequence
   - query shape metadata if needed later
   - enough identity to reuse the same type for reconnect and resubscribe
2. Make the bootstrap-producing query path return that type directly so
   subscription registration does not perform a separate `durable_head()` or
   equivalent side lookup.
3. Activate subscriptions with that exact covered sequence and treat it as
   "already delivered" for subsequent async delivery decisions.
4. Audit sync and async subscribe paths, reconnect flows, and any runtime-backed
   subscription wrappers so they all use the same bootstrap contract.
5. Add a deterministic seam for tests to force a write between registration and
   activation, then prove that the exact covered sequence still prevents both
   duplicate and skipped delivery.
6. Add coverage/lag counters if they are useful during rollout, but do not
   block implementation on an HTTP surface for them.

#### Files likely to change

- `crates/neovex-engine/src/service/subscriptions.rs`
- `crates/neovex-engine/src/subscriptions.rs`
- `crates/neovex-engine/src/service/queries.rs`
- `crates/neovex-engine/src/tests.rs`
- `crates/neovex-server/tests/reactive_loop/socket/subscriptions.rs`

#### Acceptance criteria

- subscription activation is keyed to the exact covered sequence of the initial
  result
- no subscription bootstrap path relies on a separately sampled durable or
  applied head as a surrogate for bootstrap coverage
- reconnect/resubscribe tests prove no duplicate or missing transition around
  the bootstrap boundary
- the exact covered sequence is observable in tests without inspecting private
  timing assumptions

---

### MH2. Publish warmed tables atomically with explicit covered-sequence metadata

**Priority:** high  
**Expected impact:** turns the current warmed-table load from "mutable cache
fill plus catch-up" into a real publication step with a visibility contract.

**Primary references:** TigerBeetle first, Convex second, CockroachDB third

#### Why this matters

The local TigerBeetle reference is the key guide here: derived state is only
safe because it is tied to ordered committed application and replay. Our
materialized table surface should follow the same principle even though it is a
per-tenant in-memory structure.

The local Convex `SnapshotManager` is the best shape reference:

- bounded versions
- explicit timestamped publication
- readers wait for a known point and then read a known snapshot

#### Implementation plan

1. Replace anonymous `HashMap<TableName, HashMap<DocumentId, Document>>` table
   entries with an explicit published table state, for example:
   - documents
   - covered sequence
   - load generation or install epoch so stale concurrent warmers can
     self-discard
2. Build a warmed table privately, replay it to a target apply sequence, and
   publish the table contents and covered sequence atomically as one visible
   state transition.
3. Make readers determine the minimum required covered sequence for the read
   they are serving, then:
   - use the published table only if `covered_sequence >= required_sequence`
   - otherwise bypass or warm again without exposing partial state
4. Ensure concurrent warmers for the same table cannot race to publish an older
   generation over a newer one.
5. Ensure post-publication apply updates preserve the invariant that contents
   and covered sequence move forward together.
6. Document the new publication contract in `ARCHITECTURE.md`.

#### Files likely to change

- `crates/neovex-engine/src/tenant.rs`
- `crates/neovex-engine/src/service/queries.rs`
- `crates/neovex-engine/src/service/mutations.rs`
- `crates/neovex-engine/src/service/mutations/journal.rs`
- `ARCHITECTURE.md`

#### Acceptance criteria

- warmed tables are published with explicit covered sequence metadata
- readers only use a warmed table when it is known to cover the required
  sequence
- concurrent stale warmers cannot overwrite newer published state
- deterministic concurrent first-load tests prove that readers never observe a
  partially caught-up publication

---

### MH3. Add bounded capacity, eviction, and materialized-surface observability

**Priority:** high  
**Expected impact:** moves the surface from an unbounded optimization to an
   operable subsystem that enterprise users can reason about.

**Primary references:** TigerBeetle first, CockroachDB second

#### Why this matters

Enterprise trust requires bounded resource behavior. TigerBeetle is relevant
here less for API shape and more for discipline: it is explicit about limits,
runway, and not letting internal structures grow without bound.

Convex's metrics are also the right pattern: bootstrap and invalidation lag are
measured explicitly, not guessed from generic request timing.

#### Implementation plan

1. Add a configurable capacity model for materialized tables:
   - per-tenant table count
   - per-tenant document count or byte budget
   - deterministic eviction policy
2. Prefer an explicit byte-oriented budget over only counting tables.
3. Add metrics and test accessors for:
   - loaded tables
   - documents resident
   - estimated bytes resident
   - in-flight warm loads
   - earliest and latest covered sequence across loaded tables
   - evictions
   - load count
   - hit count
   - bypass count due to insufficient covered sequence
   - optional warm load age if cheap to expose
4. Keep the counters shaped like a real serving subsystem, not just debug
   instrumentation: progress, coverage, churn, and memory should all be
   inferable from the exposed values.
5. Ensure eviction and invalidation interact cleanly with mutation apply and
   concurrent warm loads.
6. Add one operator-facing route or debug surface later only if needed; do not
   block this item on HTTP exposure.

#### Files likely to change

- `crates/neovex-engine/src/tenant.rs`
- `crates/neovex-engine/src/service/queries.rs`
- `crates/neovex-engine/src/tests.rs`
- `ARCHITECTURE.md`

#### Acceptance criteria

- the materialized surface has a bounded capacity model
- eviction is deterministic and covered by tests
- tests and metrics can distinguish load, hit, eviction, bypass, and coverage
  behavior

---

### MH4. Add deterministic adversarial verification for bootstrap and publication races

**Priority:** high  
**Expected impact:** proves that the hardened semantics hold under the exact
interleavings most likely to fail in production.

**Primary references:** all three, for different reasons

#### Why this matters

This is where enterprise trust is earned. The current suite is strong, but the
hardening items above introduce more nuanced sequencing semantics that need
adversarial proof, not just happy-path coverage.

The local reference posture comes from both sides:

- Convex treats reactive correctness as explicit timestamped state, then
  measures invalidation lag.
- TigerBeetle treats replay and deterministic ordering as fundamental, not an
  afterthought.

#### Implementation plan

1. Add deterministic tests for subscription bootstrap during concurrent writes.
2. Add deterministic tests for concurrent first-load readers of the same warm
   table, including one stale warmer and one newer warmer.
3. Add deterministic tests for repeated warm/load, eviction, invalidation, and
   rewarm cycles.
4. Add reconnect/resubscribe races that span:
   - apply lag
   - disconnect before bootstrap completion
   - reconnect before and after publication
5. Add coverage-frontier assertions so tests verify exact covered sequences, not
   just observed document sets.
6. Prefer shared deterministic pause points and harness seams over sleeps or
   timing loops.
7. Run full workspace verification after each slice, not only crate-local
   tests.

#### Files likely to change

- `crates/neovex-engine/src/tests.rs`
- `crates/neovex-server/tests/reactive_loop/socket/subscriptions.rs`
- `crates/neovex-test-support/src/`
- `docs/plans/verification-harness-plan.md` if shared harness seams need a
  documented extension

#### Acceptance criteria

- the new races are covered by deterministic regressions
- at least one test exercises concurrent warmers for the same table
- at least one test exercises eviction plus rewarm plus fresh write visibility
- no new test relies on wall-clock sleeps to force the intended interleaving
- `make test` remains green

---

### MH5. Canonical next step after hardening: versioned serving snapshots first, stronger backends later

**Priority:** deferred north star  
**Expected impact:** provides the most maintainable and enterprise-credible
architecture direction once the current narrow slice is hardened.

**Primary references:** Convex first, TigerBeetle second, CockroachDB third

#### Recommendation

After `MH1` through `MH4`, the most canonical next move is **not** to keep
growing the tenant-local mutable cache into a second ad hoc database.

The best-practice follow-up is to move toward a **versioned serving snapshot**
model first, with stronger backends only behind that stable contract:

1. authoritative writes remain journal-first and apply-ordered
2. a tenant-local `ServingSnapshotManager` publishes immutable serving
   snapshots derived deterministically from that apply stream
3. readers pin to an exact covered sequence or timestamp
4. subscriptions bootstrap from one exact snapshot and then transition to a
   live invalidation stream after that same floor
5. old snapshots are retained only within an explicit bounded window

This is much closer to the local Convex `SnapshotManager` model than the
current mutable table cache, while still preserving the TigerBeetle discipline
that derived state only exists because ordered applied state made it valid.
CockroachDB's closed timestamp and resolved/checkpoint progress model sharpens
the same recommendation further: Neovex should evolve toward explicit serving
frontiers and published serving state, not broader best-effort cache reuse.

#### What this should look like in Neovex

- a first-class `ServingSnapshotManager`, documented in
  `docs/research/versioned-serving-snapshot-design-note.md`
- a `ServingSnapshot` or equivalent sequence-stamped read state
- immutable publication, not in-place reader-visible mutation
- exact-sequence internal serving handles for reactive and resumable reads
- bounded retained versions instead of only one mutable current table image
- a clear path to a shadow-materializer-backed serving backend for server-side
  reads once parity evidence is strong enough
- optional serving-replica or embedded-replica backends later without changing
  the serving contract

#### What this should not look like

- ever-wider special cases inside `TenantRuntime` for each read path
- separate correctness rules for query, pagination, get, and subscription
- unbounded resident document copies
- best-effort sequence guesses in place of explicit snapshot metadata

#### Acceptance criteria

- this recommendation is kept as the architectural north star after `MH1`-
  `MH4`
- `docs/research/versioned-serving-snapshot-design-note.md` exists and is the
  required starting point for any later serving-promotion implementation
- any later promotion work starts from that design note rather than another ad
  hoc cache expansion

---

## Execution Log

| Date | Item | Outcome | Notes |
| --- | --- | --- | --- |
| 2026-04-02 | baseline | created | Created this plan after the first `SA8` serving-path promotion landed and then passed a retrospective architecture review. The review concluded that the direction is good but not yet canonical enough for enterprise trust: the next hardening steps are exact bootstrap sequencing, atomic warmed-table publication with covered-sequence metadata, bounded memory plus observability, and stronger adversarial verification. Local reference points used for the plan were Convex's `SnapshotManager`, read `Token`, and subscription/bootstrap metrics, plus TigerBeetle's strict ordering between committed log application and derived state. |
| 2026-04-02 | refinement | updated | Refined the plan after a second retrospective review against the local CockroachDB checkout. Added explicit served-at frontier language, strengthened the implementation contract for `MH1` through `MH4`, and incorporated Cockroach's closed timestamp, resolved timestamp, checkpoint-progress, and backpressure posture as additional reference guidance. The result is intended to be implementation-ready rather than only roadmap-ready. |
| 2026-04-02 | reference posture | updated | Re-reviewed the local Convex, TigerBeetle, and CockroachDB sources specifically to ensure the plan was not overweighting CockroachDB. Clarified the reference posture: Convex is the primary semantic model for bootstrap and reactive correctness, TigerBeetle is the primary discipline model for derived-state validity and boundedness, and CockroachDB is the primary frontier/observability model. Also made the non-goals explicit so the plan cannot be misread as a distributed-storage rewrite. |
| 2026-04-02 | MH1 | done | Implemented exact bootstrap-covered sequence tracking for subscription registration and activation. Bootstrap queries now evaluate against a consistent `TenantReadSnapshot`, return the exact `applied_sequence()` they covered, warm the document cache from that result, and activate the subscription at that exact sequence instead of inferring from a nearby head. To close the inactive bootstrap window, activation now enqueues one coalesced catch-up re-evaluation when newer applied commits landed after the bootstrap snapshot but before activation. Verified with targeted sync and async bootstrap-race regressions, `cargo test -p neovex-engine`, and the reactive reconnect/resubscribe server regression. |
| 2026-04-02 | MH2 | done | Replaced anonymous warmed-table entries with explicit published table state carrying `{generation, covered_sequence, documents}`. Full-scan query, paginated query, and warmed `get_document` paths now sample a required sequence up front, wait for that frontier, and only reuse a warmed table when its published coverage meets or exceeds the required sequence. New first-load publication logic builds privately, replays to a target applied sequence, pauses publication behind a deterministic test seam, and only then publishes atomically; once published, apply updates move table contents and covered sequence forward together. Verified with targeted coverage-frontier and concurrent first-load race regressions, `cargo test -p neovex-engine`, `make test`, `cargo fmt --all --check`, and `make clippy`. |
| 2026-04-02 | MH3 | done | Added a bounded capacity model around the published materialized surface: per-tenant table count and byte limits, deterministic table-level LRU eviction, in-flight warm-load tracking, and metrics for resident tables/documents/bytes, earliest/latest covered sequence, load count, query/paginated/get hits, evictions, and coverage bypasses. The new tests force byte-budget eviction and an under-covered published-table bypass so the metrics describe real behavior instead of incidental debug state. Verified with focused materialized-surface regressions, `cargo test -p neovex-engine`, `make test`, `cargo fmt --all --check`, and `make clippy`. |
| 2026-04-02 | MH4 | done | Added deterministic adversarial coverage for the remaining serving-race boundaries. The engine now has a repeated warm/load/evict/rewarm regression that proves resident-frontier advancement in place, eviction under a bounded table budget, and fresh-write visibility after rewarming an evicted table. The reactive-loop suite now covers disconnect-before-bootstrap-activation on the generic `/ws` route and proves that a dropped client cancels pending bootstrap work, releases the inactive subscription promptly, and reconnects cleanly. Landing that test exposed two real implementation issues that were fixed in the same slice: the generic websocket route no longer awaits subscription bootstrap inline in the socket read loop, and subscription-delivery teardown now guards against self-join when shutdown runs on the worker thread. A narrow `neovex-engine` `test-hooks` feature exposes just the bootstrap-pause seam needed by downstream workspace tests without widening the rest of the engine's test-only surface. Verified with `cargo test -p neovex-engine -- --nocapture`, `cargo test -p neovex-server --test reactive_loop socket::subscriptions:: -- --nocapture`, plus the final workspace checks below. |
| 2026-04-02 | MH5 | done | Re-read the local Convex, TigerBeetle, and CockroachDB sources against the hardened Neovex serving slice and converted the north-star recommendation into `docs/research/versioned-serving-snapshot-design-note.md`. The note pins the canonical next abstraction as a tenant-local `ServingSnapshotManager` with immutable published versions, exact frontier-pinned serving handles, bounded retained versions, waiter-based frontier advancement, and operator-visible lag/retention metrics. It also narrows the implementation direction for Neovex specifically: versioned serving snapshots come first, and stronger backends such as a shadow-materializer-backed serving surface or a serving replica come later behind that stable contract rather than by growing `TenantRuntime` into a second ad hoc database. |
| 2026-04-02 | MH5 slice | done | Implemented the first reader-facing serving-snapshot slice behind that design note. Promoted full-scan `get`, query, and pagination paths now pin a tenant-scoped `ServingSnapshot` assembled from published table versions instead of reading directly from the warmed-table map or a raw table handle. The in-memory backend still retains versions per table, but snapshot selection now prefers the oldest published version that still covers the reader's required frontier, which preserves retained exact-frontier behavior when available and still allows first-load publication to satisfy older readers safely. Verified with focused retained-version, multi-table, and concurrent-first-load regressions plus `cargo test -p neovex-engine`. |
| 2026-04-02 | MH5 manager slice | done | Lifted the serving seam from per-read snapshot assembly to a real tenant-level manager. The materialized serving layer now publishes and retains tenant-scoped `ServingSnapshot` versions, wakes exact-frontier waiters when newer snapshots are published, and prunes old tenant snapshots only after they fall outside the retained window and are no longer pinned by a reader. Promoted full-scan reads now acquire the earliest tenant snapshot that safely covers their required frontier for the target table, while the current in-memory per-table retained versions remain the initial backend under that manager. Verified with new waiter and pin-aware pruning regressions plus `cargo test -p neovex-engine`, `cargo clippy -p neovex-engine --all-targets -- -D warnings`, `cargo fmt --all --check`, and `git diff --check`. |
| 2026-04-02 | MH5 backend slice | done | Deduplicated in-flight table warm loads behind the new serving snapshot manager and tightened first-load publication to catch up to the newest applied frontier before publishing. Concurrent readers for the same cold table now share one warm load instead of rebuilding the same table independently, and a loader that sees newer applied commits after its initial catch-up loop will replay once more before publishing rather than emitting a stale table image. The old stale-first-publication test was replaced by stronger "catch up before publication" and "one concurrent load serves both readers" regressions. Verified with focused warm-load concurrency regressions plus `cargo test -p neovex-engine`, `cargo clippy -p neovex-engine --all-targets -- -D warnings`, `cargo fmt --all --check`, and `git diff --check`. |
| 2026-04-02 | MH5 subscription adoption | done | Extended the serving-snapshot contract to promoted full-scan subscriptions. Subscription bootstrap now reuses the serving snapshot for supported shapes instead of always reading directly from storage, and later re-evaluation continues to reuse the same materialized-serving path. The important semantic guardrail was preserved during the refactor: bootstrap still pins the current applied frontier rather than waiting for the latest durable head, so lagged durable commits continue to surface through the existing catch-up handoff instead of being silently folded into the initial result. Verified with new sync and async full-scan bootstrap regressions, a full-scan subscription re-evaluation regression, the pre-existing lagged-apply bootstrap regression, plus `cargo test -p neovex-engine`, `cargo clippy -p neovex-engine --all-targets -- -D warnings`, `cargo fmt --all --check`, and `git diff --check`. |
| 2026-04-02 | MH5 runtime and scheduler closeout | done | Closed the remaining adoption ambiguity with server-level proof instead of another speculative refactor. Read-only runtime host operations now demonstrably inherit the serving contract on promoted full-scan shapes: new `neovex-server` regressions prove that runtime-only full-scan query, `ctx.db.get` after warmup, and runtime paginated full-scan queries warm and reuse the serving layer through the public service APIs. A companion host-bridge regression proves the transactional boundary stayed correct: runtime mutation reads still use the `MutationExecutionUnit` snapshot plus staged writes even when a serving snapshot is already warm for the same table. The scheduler story is now explicit too: schedulable Convex mutations still resolve manifest `Mutation` plans, so runtime-only handlers are rejected at schedule time rather than creating a second scheduler-side runtime read path. Verified with focused `cargo test -p neovex-server ...` regressions, then the final workspace checks below. |
| 2026-04-02 | MH5 reactive transport closeout | done | Closed the last reactive-loop gap in the runtime subscription layer. Plan-backed runtime subscriptions can still wake conservatively because their base subscription may be broader than the runtime read set, and delayed bootstrap catch-up can legitimately trigger a re-evaluation after the request-scoped bootstrap result has already been sent. Runtime subscription transforms now retain the last emitted runtime value and suppress duplicate pushes when re-evaluation produces the same externally visible payload, which keeps websocket delivery aligned with the intended bootstrap-then-catch-up contract without inventing a second ad hoc sequencing model. The reactive-loop transport tests were tightened to assert the real applied-visibility contract consistently: an initial empty/null bootstrap is allowed while apply lags, but the subscription must converge correctly and stay quiet once no visible change occurred. Verified with `cargo test -p neovex-server --test reactive_loop runtime_queries::get_and_query:: -- --nocapture`, `cargo test -p neovex-server --test reactive_loop -- --nocapture`, `make test`, `make clippy`, `cargo fmt --all --check`, and `git diff --check`. |
