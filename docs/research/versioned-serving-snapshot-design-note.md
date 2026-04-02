# Versioned Serving Snapshot Design Note

This note is the `MH5` design gate from
`docs/plans/materialized-serving-hardening-plan.md`.

It describes the canonical next step after `MH1` through `MH4`: evolve the
current hardened materialized-serving slice into a versioned serving snapshot
subsystem, not a wider mutable cache.

## Current State

Today Neovex has a narrow but hardened materialized-serving path:

- the authoritative source of truth remains redb plus the durable journal
- reads still wait for `applied_sequence >= required_sequence`
- selected full-scan `get`, query, pagination, and subscription re-evaluation
  can reuse tenant-local materialized table state
- those promoted full-scan reads now acquire an explicit internal
  `ServingSnapshot` stitched from published table state instead of reading
  directly from the warmed-table map
- subscription bootstrap now carries the exact covered apply sequence of its
  initial result through activation
- warmed tables publish atomically as
  `{generation, covered_sequence, documents}` state
- published table documents are now `Arc`-backed and updated with clone-on-write
  semantics, so a pinned serving snapshot continues to reflect the frontier it
  captured even after later applies advance the current publication
- the in-memory backend now retains a bounded history of published table
  versions, so acquiring a serving snapshot for an older required sequence can
  prefer the oldest published version that still covers that frontier instead
  of always collapsing to the newest publication
- the current implementation is now tenant-scoped at the API boundary but
  table-scoped in retention: a snapshot can span multiple loaded tables, while
  version retention still lives inside each table's in-memory publication state
- the serving layer now also retains a bounded tenant-level window of published
  serving snapshots, wakes exact-frontier waiters when a newer snapshot is
  published, and prunes old tenant snapshots only when they are outside policy
  and no longer pinned by a reader
- concurrent readers now deduplicate in-flight table warm loads behind that
  manager layer, and a first load that sees newer applied commits before it
  publishes loops once more and publishes the newest safe frontier instead of a
  stale intermediate table image
- subscription bootstrap and re-evaluation now also reuse the same serving
  snapshot seam on promoted full-scan shapes, while still pinning the current
  applied frontier for bootstrap instead of waiting for the latest durable head
- runtime-backed subscription transforms now retain their last emitted runtime
  value and suppress duplicate pushes when a broad wakeup or delayed catch-up
  re-evaluation produces the same externally visible result
- the surface is bounded by table count and byte budget and exposes residency,
  reuse, bypass, eviction, and in-flight load metrics

This is a good tactical optimization. It is not yet the most maintainable
long-term serving architecture for Neovex.

## Goals

- preserve one authoritative write path and one visibility rule:
  journal-first, apply-ordered, exact covered frontiers
- unify `get`, query, pagination, subscriptions, runtime host reads, and
  scheduler reads behind one serving contract
- make published serving state immutable from a reader's point of view
- retain only a bounded version window while allowing readers to pin an exact
  covered frontier
- keep the abstraction independent from the backing implementation so the
  current in-memory surface can later give way to the shadow materializer or
  another serving replica without another semantic rewrite
- improve enterprise trust by making lag, retention, publish progress,
  backpressure, memory, and bypass reasons explicit

## Non-Goals

- no new authoritative source of truth
- no broadening of the current warmed-table cache as an ad hoc permanent
  database inside `TenantRuntime`
- no CockroachDB-style range replication, leaseholder routing, or distributed
  follower-read protocol
- no TigerBeetle-style whole-program static allocation requirement
- no Convex-specific public token API in the same slice
- no requirement that every query shape immediately move onto the first serving
  snapshot implementation

## Why This Fits Neovex

Neovex is not a generic distributed storage system. It is a single binary with:

- per-tenant embedded storage
- embedded V8 runtime execution
- server-authoritative reactive live queries
- cron and scheduling in the same process

That makes an in-process serving snapshot unusually attractive:

- there is no process hop between runtime execution and serving state
- there is no network hop between the server's invalidation loop and the read
  surface it feeds
- the same apply frontier already governs HTTP, WebSocket, runtime, and
  scheduler behavior

The right optimization for this shape is not "teach more call sites about a
cache". It is "publish a trustworthy read snapshot at an exact frontier and let
all supported readers pin to it".

## Reference Posture

This design note is reference-driven, not reference-copied.

### Convex: semantic model

Local references:

- `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/snapshot_manager.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/token.rs`

The relevant ideas to borrow are:

- keep a bounded `VecDeque`-style window of published versions
- make `latest()` return a typed frontier plus a concrete snapshot handle
- wake waiters when a higher published frontier becomes available
- treat "what exact snapshot did this read observe?" as a first-class concept
- carry an exact covered frontier alongside the read result

Neovex should borrow those semantics even if the first internal handle is not a
user-visible token. The important local pattern is not just "have snapshots",
but "have a bounded published version window, explicit `latest()`, and direct
waiters for frontier advancement".

### TigerBeetle: discipline model

Local reference:

- `/Users/jack/src/github.com/tigerbeetle/tigerbeetle/docs/ARCHITECTURE.md`

The relevant ideas to borrow are:

- authoritative log first, derived state second
- checkpoints and snapshots are derived state, not truth
- replay from checkpoint plus suffix must reconstruct exact serving state
- bounds are part of correctness, not an optional optimization

Neovex should borrow this discipline for any serving snapshot or serving
replica. A published snapshot is only valid because ordered applied commits made
it valid.

### CockroachDB: frontier and observability model

Local references:

- `/Users/jack/src/github.com/cockroachdb/cockroach/docs/RFCS/20191108_closed_timestamps_v2.md`
- `/Users/jack/src/github.com/cockroachdb/cockroach/pkg/ccl/changefeedccl/tableset/watcher.go`
- `/Users/jack/src/github.com/cockroachdb/cockroach/pkg/ccl/changefeedccl/metrics.go`

The relevant ideas to borrow are:

- reads only become valid at explicit safe frontiers tied to applied state
- waiters should block on those frontiers directly, not on guessed nearby heads
- publication should notify exact frontier waiters when resolved state advances
- resolved/checkpoint progress and backpressure should be operator-visible

Neovex should borrow that frontier rigor without adopting CockroachDB's
distributed replication architecture. The local `Watcher` code is useful here
because it buffers changes until a resolved frontier advances, then updates the
resolved frontier and wakes waiters keyed to that frontier. That is the right
serving-subsystem posture even in a single binary.

## Recommendation

The canonical next step is:

1. introduce a first-class `ServingSnapshotManager` abstraction
2. make the serving contract versioned and immutable
3. keep the current warmed-table implementation only as an initial backend
4. later swap the backend to a stronger serving materializer or serving replica
   without changing reader semantics

For Neovex's server path, the preferred direction is:

- **first**: versioned serving snapshots
- **later, if justified**: a serving-materializer or serving-replica backend

That ordering matters. Enterprise trust comes from freezing the semantics first,
then evolving the backing implementation behind them.

## Proposed Architecture

### 1. `ServingSnapshotManager`

Introduce one tenant-local owner for published serving versions.

Illustrative shape:

```rust
struct ServingSnapshotManager {
    versions: VecDeque<Arc<ServingSnapshot>>,
    latest_published: SequenceNumber,
    earliest_retained: SequenceNumber,
    waiters: BTreeMap<SequenceNumber, Vec<Notify>>,
    retention: ServingRetentionPolicy,
    metrics: ServingSnapshotMetrics,
}
```

Responsibilities:

- publish new serving snapshots at exact covered sequences
- let readers wait for or acquire a snapshot covering a required sequence
- retain a bounded version window
- prune only versions that are both outside policy and no longer pinned
- expose progress, lag, and retention metrics

Concrete borrowing:

- Convex-style `latest()` plus wait-for-higher-frontier behavior
- Cockroach-style waiter notification when the published frontier advances
- TigerBeetle-style insistence that every published version is derived from
  ordered applied state, not speculative state

This is the semantic center of the design. The backing implementation can
change later.

### 2. `ServingSnapshot`

A published serving version is immutable to readers and explicitly tagged with
its frontier.

Illustrative shape:

```rust
struct ServingSnapshot {
    covered_sequence: SequenceNumber,
    tables: Arc<HashMap<TableName, Arc<ServingTableSnapshot>>>,
    resident_estimated_bytes: usize,
}

struct ServingTableSnapshot {
    documents: Arc<HashMap<DocumentId, Document>>,
    estimated_bytes: usize,
}
```

Important rule:

- immutable publication does not require cloning the whole tenant on every
  commit

Snapshots should share unchanged table state through `Arc` ownership and
replace only the tables touched by a published step. The important change is
reader-visible immutability, not brute-force copying.

### 3. `ServingHandle`

Readers should pin a published version explicitly.

Illustrative shape:

```rust
struct ServingHandle {
    snapshot: Arc<ServingSnapshot>,
    covered_sequence: SequenceNumber,
}
```

This is the internal Neovex equivalent of a read token:

- bootstrap queries return results derived from a pinned handle
- subscriptions activate at that handle's exact covered sequence
- runtime host reads and scheduler reads can use the same handle type
- future resumable or externalized read-state can build on this shape if
  Neovex ever wants that API

### 4. Publication Rule

Publication must remain derived from authoritative apply.

The publication sequence should be:

1. authoritative commit `N` becomes applied
2. the next serving state is derived privately from the last published snapshot
   plus the applied change set
3. once the derived state fully reflects `N`, publish one immutable snapshot
   tagged `covered_sequence = N`
4. notify waiters for `required_sequence <= N`
5. prune old unpinned versions under explicit retention policy

Readers must never observe in-progress assembly.

### 5. Read Contract

All supported serving surfaces should follow one contract:

1. determine the minimum required sequence for the read
2. wait until authoritative apply reaches that sequence
3. acquire a `ServingHandle` covering that same sequence, if one exists for the
   query shape
4. evaluate on that pinned snapshot
5. otherwise fall back to the authoritative storage path and record the bypass
   reason

That contract should be shared by:

- `get_document`
- query
- pagination
- subscription bootstrap
- runtime host reads
- scheduler reads

Not every shape must be snapshot-backed on day one, but every shape should
follow the same exact-frontier rule.

### 6. Subscription Contract

Subscriptions should move from "bootstrap result plus later best-effort live
work" to "bootstrap result from one pinned serving version, then live delivery
strictly after that same version".

Required rules:

- bootstrap evaluates against a `ServingHandle`
- registration records `covered_sequence = handle.covered_sequence`
- live re-evaluation and invalidation only deliver work strictly after that
  sequence
- reconnect and resubscribe reuse the same handoff contract

This keeps the bootstrap/live boundary identical across HTTP, native WebSocket,
and runtime-backed subscription paths.

### 7. Retention And Bounds

Retain a bounded version window, not a single mutable current image.

Recommended initial policy:

- retain the latest published version unconditionally
- retain a small bounded count of historical versions, for example `2-8`
- also enforce a byte budget across retained versions
- allow pinned readers to temporarily extend retention until they release their
  handles
- expose the earliest retained sequence as a first-class metric

This is enough to support in-flight readers, bootstrap handoff, and
future-proofing for resumable reads without turning the subsystem into an
unbounded MVCC store.

### 8. Metrics

The subsystem should expose metrics that operators can use to decide whether it
is healthy and worth using:

- latest published covered sequence
- earliest retained covered sequence
- applied head minus latest published lag
- snapshot publish latency
- snapshot count retained
- resident tables, documents, and bytes
- reader pin count
- waiter count
- bypass count by reason
- pruning count and pruned bytes
- warm/build failures or retries

Metrics are part of the architecture here, not an optional debugging feature.

## Migration Plan

### Stage 1. Freeze the interface

Introduce `ServingSnapshotManager`, `ServingSnapshot`, and `ServingHandle`
behind the current narrowed serving shapes.

The current warmed-table surface becomes an implementation detail of the new
manager instead of a reader-facing cache.

Current status:

- the first interface-freezing slice is now in place for the promoted full-scan
  read shapes
- reads no longer depend directly on the warmed-table map; they acquire a
  pinned `ServingSnapshot`
- the current backend is still the hardened in-memory warmed-table surface, but
  it now behaves like a clone-on-write serving backend from the reader's point
  of view

### Stage 2. Publish immutable versions

Replace reader-visible in-place table mutation with immutable snapshot
publication that reuses unchanged table state by `Arc`.

The current `{generation, covered_sequence, documents}` table state becomes one
input to snapshot construction, not the final public abstraction.

Current status:

- the first retained-version slice is now in place for the in-memory backend
- published table documents are immutable to readers and later applies publish
  a new current version instead of mutating the one a reader already pinned
- each loaded table now retains a bounded history of older published versions,
  which is enough for the serving layer to reacquire the oldest published
  version that still covers an earlier required frontier when such a retained
  version exists
- the promoted full-scan read paths now build tenant-level serving snapshots on
  top of those retained table versions, so query/get/pagination no longer pin a
  raw table image directly
- a first tenant-level `ServingSnapshotManager` slice is now in place: it
  retains published tenant snapshots, wakes waiters for newly covered
  frontiers, and only prunes old snapshots once they fall outside the retained
  window and no reader still pins them
- the in-memory backend now deduplicates concurrent warm loads per table, so
  waiting readers share one catch-up build instead of rebuilding the same table
  independently
- first-load publication now rechecks the applied frontier immediately before
  publish and catches up again if necessary, so it no longer intentionally
  publishes a stale table image that the next reader must bypass
- subscription bootstrap and live re-evaluation now participate in the same
  serving-snapshot adoption story as promoted full-scan `get`, query, and
  pagination, with bootstrap still anchored to the current applied frontier so
  lagged durable commits are delivered through the existing catch-up handoff
  instead of being waited into the initial result
- read-only runtime host operations now sit on the same promoted contract for
  the supported full-scan shapes: server-level regressions prove that
  runtime-only full-scan query, `ctx.db.get`, and paginated query flows warm
  and reuse the serving layer through the public service APIs rather than
  bypassing it
- runtime mutation reads intentionally remain outside that serving path: when a
  runtime mutation is executing inside a `MutationExecutionUnit`, reads still
  use the OCC snapshot plus staged writes rather than the warmed serving
  surface, and the regression suite now proves that stays true even when a
  serving snapshot is already warm for the same table
- there is still no separate scheduler-side runtime read surface to adopt
  today: schedulable Convex mutations resolve manifest `Mutation` plans, so
  runtime-only handlers are rejected at schedule time instead of being run by
  the scheduler through a second read path
- the remaining gap to the full north star is backend maturity, not the reader
  contract: retention capacity is still shared with the current in-memory
  backend, and tenant snapshots are still rebuilt from per-table publications
  rather than from a stronger serving-materializer backend

### Stage 3. Unify reader adoption

Route the existing promoted serving paths through pinned handles:

- warmed `get_document`
- full-scan query
- full-scan pagination
- subscription bootstrap and re-evaluation
- runtime host reads on the same supported shapes

Unsupported shapes still fall back cleanly, but the serving contract stays
uniform.

### Stage 4. Swap the backend when ready

Once shadow-materializer parity and robustness evidence are strong enough,
replace the initial in-memory builder with a stronger backend:

- first choice for server-side serving: the shadow materializer promoted into a
  serving materializer
- future optional choice for local-first or offloaded reads: an embedded or
  colocated serving replica

The `ServingSnapshotManager` contract stays the same either way.

## Best Follow-Up For Speed, Reliability, And Maintainability

For Neovex specifically, the most canonical follow-up after `MH5` is:

1. implement a versioned `ServingSnapshotManager` over the current promoted
   full-scan shapes
2. prove it with the same deterministic harness style already used elsewhere
3. promote the shadow materializer into the first durable serving backend only
   after parity, replay, compaction, and corruption evidence remain strong

That is a better enterprise story than continuing to enrich `TenantRuntime`
with special-case caches:

- **speed**: published snapshots eliminate repeated decode and planning work on
  hot supported shapes
- **reliability**: exact frontiers and bounded retained versions make serving
  state auditable
- **maintainability**: one serving abstraction can survive a backend swap from
  in-memory tables to a materializer-backed snapshot

In short:

- canonical abstraction: `ServingSnapshotManager`
- canonical near-term implementation: versioned in-process serving snapshots
- canonical later backend: serving materializer built from the authoritative
  apply stream

## Main Risks

- overbuilding a generic snapshot framework before the first supported serving
  shapes use it
- accidentally cloning too much state per publish instead of sharing unchanged
  tables structurally
- letting fallback paths drift into a different visibility contract than the
  snapshot-backed paths
- promoting a materializer-backed serving path before parity and recovery
  evidence are strong enough

The mitigation is the same throughout this note: freeze the semantics first,
keep the first supported shape narrow, and widen only after deterministic
verification stays green.
