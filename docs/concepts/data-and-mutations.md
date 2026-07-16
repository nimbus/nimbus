---
title: Data and mutations
description: Documents, schema-optional tables, the single engine-owned mutation path, storage atomicity, and how reactive queries observe commits.
sidebar:
  label: Data & mutations
  order: 3
---

Nimbus stores JSON documents in tables and pushes query results to
subscribers when data changes. The guarantees behind that — what a write
means, when it becomes durable, when it becomes visible, and why every
protocol surface behaves identically — come from a small set of
deliberately rigid rules in the engine and storage layers. This page
explains those rules from the perspective of someone deciding whether to
trust them.

## Documents and tables

A document is a JSON object — a map of fields — that lives in exactly one
table within one tenant. Nimbus manages three system fields on every
document: `_id` (a unique, validated identifier), `_creationTime`, and
`_updateTime`. You never set these; they appear on every document the API
returns.

Tables are not declared up front. Writing to a table name that doesn't
exist yet creates it in the same transaction as the write. A table is a
namespace with a stable internal identity, not an object you must
provision — which is what makes the next property possible.

## Schema is optional

A table without a schema accepts any document. That is the default state
of every table, and it is a real mode of operation, not a temporary
loophole.

When you do set a schema for a table, you get, per table:

- **Required fields** — writes missing a required field are rejected.
- **Type checks** — declared fields must match their declared type;
  fields the schema doesn't mention are still allowed.
- **Indexes** — single-field and composite indexes the query planner can
  use.
- **Access policy** — optional declarative read/write rules the engine
  enforces for every surface.

Two properties of schema changes matter for trust:

1. **Adding a schema constrains; it never revokes.** A schema adds
   validation to future writes. It never makes a previously schemaless
   table read-only — conforming writes always remain possible, and
   documents already in the table stay readable.
2. **Schema changes are atomic.** Replacing a table's schema and
   rebuilding its index entries happen in one storage transaction. There
   is no window where the schema is new but the indexes reflect the old
   definition.

Validation happens at write time, inside the mutation path described
next — so it is enforced identically no matter which protocol or runtime
originated the write.

## One mutation path

Every mutation in Nimbus — an insert from the native HTTP API, a write
arriving over a WebSocket session, a scheduled job firing, a
`ctx.db.insert(...)` inside a function running in the V8 runtime, or a
write translated from the Firestore, MongoDB, DynamoDB, Cloud Functions,
or Convex dialects — flows through one engine-owned execution path.
There is no fast lane and no internal bypass.

That single path is where the guarantees attach:

- **Schema validation** runs against the table's schema, if one exists.
- **Authorization** enforces the table's access policy against the
  caller's normalized principal.
- **Index maintenance** stays in lockstep with the document write.
- **Subscription fan-out** is triggered for every committed change.

Because adapters and the function runtime converge on the same path,
none of these can be skipped by choosing a different door into the
system. The protocol adapters translate dialects; they do not own write
semantics. The runtime's host bridge calls the same engine methods an
HTTP handler does.

Two variants of the path deserve a note:

- **Scheduled mutations** carry an execution id that is recorded in the
  same transaction as the write. If the server crashes and the scheduler
  re-runs a claimed job, the write applies at most once.
- **Function mutations are transactional units.** A mutation function
  that performs several reads and writes executes against a stable
  snapshot, stages its writes locally, and commits once — after
  validating that nothing it depended on changed underneath it
  (optimistic concurrency validation). On conflict the unit fails as a
  whole rather than committing an interleaved partial result.

## First committer wins

Every write surface has the same concurrency contract: **first committer
wins**. A mutation prepares against a snapshot. If a commit that lands first
changes something the mutation depends on, Nimbus rejects that prepared result
instead of overwriting the newer state. Nimbus does not promise
last-write-wins behavior on any adapter.

Nimbus transparently reruns retryable conflicts from a fresh snapshot within a
bounded server-side budget. The default budget is four total attempts, with
jittered exponential backoff starting at 100 ms and capped at 2 seconds. The
operator knobs are `NIMBUS_MUTATION_OCC_MAX_RETRIES`,
`NIMBUS_MUTATION_OCC_INITIAL_BACKOFF_MS`, and
`NIMBUS_MUTATION_OCC_MAX_BACKOFF_MS`. If the budget is exhausted, the client
receives a typed conflict and may retry the complete mutation. This contract
applies to immediate, journaled, and transactional writes even while immediate
and journaled writes use a serial committer internally.

Do not retry every failure indiscriminately. The
[error taxonomy](/reference/native/errors/#commit-path-taxonomy) distinguishes
immediate retries, backoff, transaction restarts, and terminal request errors.

## Derived dependency semantics

The conflict set includes dependencies derived from writes, not only values
the function explicitly read. These rules are normative:

- **Written document id.** Staging a write derives a dependency on the exact
  table identity and document id. A later commit to that id conflicts even if
  the mutation never read the document first. A table that did not exist at
  the snapshot instead derives a missing-table dependency, so concurrent table
  creation conflicts.
- **Schema.** Every table reached by a read or derived write dependency also
  depends on that table's schema epoch and definition. Creating, replacing, or
  removing the schema after the snapshot conflicts before any staged write is
  persisted.
- **Generated id.** Nimbus chooses a generated document id before staging the
  insert. That concrete id becomes the written-document dependency: a commit
  that claims it after the snapshot conflicts, while an id already in use
  makes the atomic insert fail. A retry starts a new mutation execution and
  must not reuse effects from the failed attempt.

A conflict therefore means that committing the prepared result could violate
one of the proposal's observed or derived assumptions. It does not mean merely
that two mutations wrote somewhere in the same tenant.

## Mutation caps and tenant write rate

Prepare accounts for user read bytes, write bytes, documents scanned,
documents written, and index-range calls. Proposed limits are enabled in
**shadow mode** by default: Nimbus records and samples would-be violations but
does not reject them. Setting the corresponding non-`PROPOSED` knob enables
enforcement; values exactly at the limit are accepted and values above it
return the deterministic `cap_exceeded` error.

| Resource | Shadow knob and default | Enforcement knob (default) |
| --- | --- | --- |
| Read bytes | `NIMBUS_PROPOSED_MUTATION_READ_BYTES=16777216` | `NIMBUS_MUTATION_READ_BYTES` (unset) |
| Write bytes | `NIMBUS_PROPOSED_MUTATION_WRITE_BYTES=16777216` | `NIMBUS_MUTATION_WRITE_BYTES` (unset) |
| Documents scanned | `NIMBUS_PROPOSED_MUTATION_DOCUMENTS_SCANNED=32000` | `NIMBUS_MUTATION_DOCUMENTS_SCANNED` (unset) |
| Documents written | `NIMBUS_PROPOSED_MUTATION_DOCUMENTS_WRITTEN=16000` | `NIMBUS_MUTATION_DOCUMENTS_WRITTEN` (unset) |
| Index-range calls | `NIMBUS_PROPOSED_MUTATION_INDEX_RANGE_CALLS=4096` | `NIMBUS_MUTATION_INDEX_RANGE_CALLS` (unset) |
| System write bytes | `NIMBUS_PROPOSED_SYSTEM_MUTATION_WRITE_BYTES=134217728` | `NIMBUS_SYSTEM_MUTATION_WRITE_BYTES` (unset) |
| System documents written | `NIMBUS_PROPOSED_SYSTEM_MUTATION_DOCUMENTS_WRITTEN=40000` | `NIMBUS_SYSTEM_MUTATION_DOCUMENTS_WRITTEN` (unset) |

System bookkeeping has separate, higher write limits so application-controlled
usage and Nimbus-owned effects are not charged against one another.

Tenant protection is separate from per-mutation caps. Nimbus keeps a sliding
window of committed and admitted write bytes for each tenant. The default
shadow posture is `NIMBUS_PROPOSED_TENANT_WRITE_BYTES_PER_SEC=1048576` over
`NIMBUS_TENANT_WRITE_RATE_WINDOW_MS=1000`; sampled warnings use
`NIMBUS_TENANT_WRITE_RATE_REPORT_EVERY=100`. Enforcement remains off until
`NIMBUS_TENANT_WRITE_BYTES_PER_SEC` is set. An enforced overflow returns
`rate_limited` with a `retryAfterMs` hint; clients should wait for that delay.

## Durability, then visibility

Nimbus separates "your write is safe" from "your write is readable", and
sequences them strictly:

1. **Admission.** Asynchronous mutations enter a per-tenant admission
   gate. Under overload, shedding happens here — before any durable
   work — so a mutation that is admitted keeps its commit guarantee.
2. **Durable append.** The mutation is appended to the tenant's durable
   journal in commit order. This is the acknowledgement point: a
   mutation is only acknowledged after its journal record is durably
   appended.
3. **Ordered apply.** A journal worker materializes the document and
   index changes and advances the tenant's applied watermark.
4. **Visibility.** Reads and subscriptions consult applied state only.
   A read never observes a write that is durable but not yet applied,
   and never observes apply effects out of order.

One consequence worth internalizing: once a mutation crosses the durable
commit point, it is committed even if the client disconnects before
seeing the response. Disconnection before the commit point can cancel
the work; disconnection after it is a transport failure, not a rollback.

## Storage atomicity

At the bottom of the write path, three effects always travel together in
a single storage transaction:

- the document write itself,
- the index entries that write implies, and
- the commit-log (journal) record describing it.

If the transaction commits, all three exist; if it fails, none do. This
holds on every storage provider — the embedded engines and the external
SQL backends alike, each using its own native transaction to enclose all
three effects. There is no state in which a document exists without its
index entries, or a commit record exists without its document write.
That invariant is what lets indexes serve queries without
cross-checking, and lets the journal act as the authoritative history
for recovery and replay.

## How subscriptions observe commits

A subscription is a registered query. When you subscribe, the engine
evaluates the query once and delivers the initial result; after that,
the engine — not the client — decides when you hear about changes:

- After each applied commit (or batch of commits), the engine computes
  which registered subscriptions a change could affect and enqueues
  re-evaluation work on a tenant-local delivery worker, off the
  mutation's return path.
- Invalidation is **conservative by design**: inserts and deletes are
  checked against the changed document, while updates conservatively
  wake any subscription on that table whose result might be affected. A
  spurious wakeup costs a re-evaluation; a missed one would cost
  correctness, so Nimbus only errs in the first direction.
- Delivery is **monotonic per subscription**. The engine tracks the
  latest sequence delivered to each subscription and never publishes
  older state after newer state.
- Under write storms, overlapping wakeups for the same subscription are
  **coalesced**: subscribers receive the latest applied result rather
  than a backlog of intermediate states. You see the current truth, not
  necessarily every transition.

Because fan-out is computed from applied commits — the output of the
single mutation path — every write from every surface reaches
subscribers. A document changed through the MongoDB wire protocol
updates a live Convex query, because both are just commits to the same
engine.

## What this adds up to

- A write acknowledged is a write durably journaled, validated, and
  authorized — regardless of which API produced it.
- A write observed is a write whose document, indexes, and history are
  consistent with each other.
- A subscription reflects applied state, in order, without regressions.
- A schema is a constraint you add when ready, not a ceremony the
  database imposes.

## Related pages

- [How Nimbus works](/concepts/how-nimbus-works/) — the architecture
  these guarantees live inside.
- [Tenant isolation](/concepts/tenant-isolation/) — the boundary around
  each tenant's data and execution.
- [Developers](/developers/) — build against any of the protocol
  surfaces.
- [Storage backends](/operators/storage-backends/) — the persistence
  providers behind the storage contract.
