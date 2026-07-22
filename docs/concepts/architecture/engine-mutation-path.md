---
title: Engine and the mutation path
description: A crate-level tour of nimbus-engine — the Engine coordinator, per-tenant runtimes, the single mutation path, the durable journal, execution units, the scheduler, and subscription delivery.
sidebar:
  label: "Engine & mutations"
  order: 4
---

`crates/nimbus-engine` is the decision-making center of Nimbus. Transport
crates accept requests, adapter crates translate dialects, and the runtime
executes JavaScript — but every one of them ends up calling the engine,
and the engine is where validation, authorization, durability, ordering,
and fan-out actually happen. The user-facing guarantees are explained in
[data and mutations](/concepts/data-and-mutations/); this page is the
crate-level tour of the machinery behind them.

## The Engine and per-tenant runtimes

The coordinator is the `Engine` struct in
`crates/nimbus-engine/src/engine/mod.rs`. It owns the cross-tenant state:
a map of live tenants behind a read–write lock, the persistence provider
that opens tenant storage, the control-plane provider, wall and monotonic
time sources, the scheduler wakeup signal, trigger and committed-mutation observer
registries, and the executors that move storage work off async threads.
Tenant creation is serialized through a load gate
(`crates/nimbus-engine/src/engine/tenant_load_gate.rs`) so two requests
racing to open the same tenant cannot construct it twice.

Everything tenant-scoped lives in `TenantRuntime`
(`crates/nimbus-engine/src/tenant.rs`): the tenant's persistence handle,
a read executor, the subscription registry, the current schema behind an
atomic swap, a document cache, the subscription delivery worker, and —
central to this page — the mutation admission gate and the durable
mutation journal. One tenant's queues, caches, and watermarks are
invisible to every other tenant, which is what makes the tenant the
isolation and scaling unit described in [scaling](/concepts/scaling/).

## Tenant lifecycle interfaces

Tenant admission is an engine-owned async lifecycle. Explicit administrative
creation uses `Arc<Engine>::create_tenant_async` and reports `AlreadyExists`
when the durable tenant is already present. Request-facing adapters use
`ensure_tenant_ready_async`: an existing durable tenant is not considered
ready until Nimbus has opened it and registered a complete `TenantRuntime`.
That distinction makes retry safe if cancellation or process failure happens
after provider storage is created but before runtime publication.

The blocking `Engine::create_tenant` interface is deliberately restricted to
embedded redb and SQLite compositions. It fails before touching tenant state
when called on PostgreSQL, MySQL, libSQL, or another provider composition.
Provider-reachable callers must retain an `Arc<Engine>` and await the async
lifecycle; MongoDB, DynamoDB, Cloudflare KV, automatic CLI tenant creation,
object admission, the system tenant, and DynamoDB's scheduled TTL maintenance
all follow that rule. This keeps provider namespace creation, runtime catch-up,
observer bootstrap, and publication behind one ordering and error contract.

## One mutation path

The engine's public write surface is a family of document methods —
`insert_document`, `insert_document_with_id`, `update_document`,
`delete_document`, and their `_async` and principal-carrying `_with`
variants — defined in
`crates/nimbus-engine/src/engine/mutations/direct/api.rs`. All of them
funnel into one private implementation, the `apply_mutation_with_mode`
family in `crates/nimbus-engine/src/engine/mutations/direct/execution.rs`.
That function is deliberately not public: callers outside the mutation
module cannot reach the apply step without passing through the surface
that enforces the rules.

On that path, every mutation — regardless of which protocol produced
it — gets the same treatment:

- schema validation against the table's current schema, if one exists;
- authorization against the table's access policy for the caller's
  principal;
- a storage call that writes the document and its index effects together
  (`insert_with_indexes`, `update_with_indexes_validated`, and friends),
  so indexes can never drift from documents.

The mode parameter distinguishes two origins: `Immediate` for direct API
writes, and a scheduled mode that carries an execution id, which storage
records in the same transaction as the write so a re-run scheduled job
applies at most once.

## The queued journal path

Asynchronous mutations take a second hop before that apply step: the
per-tenant durable journal. The pipeline is:

```text
admission gate → construction-selected committer arm → durable append (ack)
               → ordered apply → applied watermark
```

**Admission.** Each tenant has a bounded admission queue
(`crates/nimbus-engine/src/tenant/mutation/admission.rs`). A full queue
rejects new work with a resource-exhausted error, and a CoDel controller
(5 ms target delay, 100 ms interval) sheds queued work under sustained
overload — before any durable effect, so an admitted mutation keeps its
commit guarantee.

**Construction-time committer arm.** A tenant runtime selects exactly one
immutable `CommitterArm`. Memory, redb, and SQLite use the ordered publisher;
libSQL, PostgreSQL, and MySQL currently use the serial actor. This is a static
orchestration choice, separate from the dynamic question of whether a
process-local write-log window is authoritative. Direct commits, execution
units, journal-progress synchronization, opaque internal jobs, publisher-task
ownership, and observer shutdown all consult the same selection. There is no
runtime arm switch or drain transition.

The provider ordered-publisher adapter is exercised before it is enabled in
the production mapping. It must acquire the provider-backed committer lease
while holding the assignment/recovery gate, publish recovered durable progress,
and only then capture its assignment baseline. Acquisition failure,
cancellation, or shutdown therefore occurs before any sequence or write-log
suffix is staged. The later provider-enable change can alter the construction
policy without changing callers or granting provider snapshots process-local
window authority.

Cancellation and no-op outcomes discovered before assignment still attempt a
bounded publisher response fence. Once the fence is accepted, they cannot
answer ahead of an older accepted batch. If the queue remains full through its
send deadline, the request instead fails with the typed publisher-overload
error and has no durable effect; ordering is not claimed for rejected work. If
shutdown has already closed the publisher queue, the actor waits for the
publisher's explicit finished signal before completing the original outcomes;
the task drops all accepted messages before raising that signal, including
during unwind.

**Durable append.** The selected committer arm
(`crates/nimbus-engine/src/engine/mutations/journal.rs`) drains admitted
mutations in batches of up to 32, plans them against an in-memory overlay
of the batch so earlier writes are visible to later ones, and appends the
batch of durable mutation records to the tenant's commit log in one
storage write. This advances the **durable head** — the acknowledgement
point.

**Ordered apply.** The same arm then materializes document and index
effects in journal order and advances the **applied head**. If the
process crashes between the two steps, startup recovery replays
durable-but-unapplied records, so the acknowledgement made at append time
survives a crash.

**Visibility.** Reads gate on the applied watermark: the document query
path (`crates/nimbus-engine/src/engine/queries/documents.rs`) captures
the tenant's durable head and waits for the applied head to reach it
before serving, which is what gives clients read-your-writes without ever
exposing a durable-but-unapplied record. The watermark bookkeeping lives
in `crates/nimbus-engine/src/tenant/mutation/journal.rs`.

**Causal diagnostics.** The mutation interface reports six ordered frontiers:
`assigned_high_water >= active_assigned_head >= durable_head >=
storage_applied_head >= published_head >= applied_head`. The first retains all
assignment history; the active assigned head alone may retract when Nimbus
proves that a provisional suffix did not commit. Each later phase is causal
proof that every earlier phase reached at least the same sequence. The
frontier module (`crates/nimbus-engine/src/tenant/mutation/frontiers.rs`)
therefore reconciles the write-log and journal observations around one
concurrent sample into a causally ordered snapshot without mutating either.
The adjacent differences identify assignment, storage-apply, contiguous
publication, and reader-visibility lag rather than collapsing distinct stalls
into one number.

## Multi-step mutations: execution units

A mutation function that performs several reads and writes cannot use the
one-shot path, so the engine provides execution units
(`crates/nimbus-engine/src/engine/execution_units/mod.rs`). An execution
unit captures a schema snapshot, a persistence snapshot, and the snapshot
sequence at begin time. Reads are served from the snapshot and recorded
into a dependency set; writes are validated and staged locally, touching
no shared state.

Commit (`crates/nimbus-engine/src/engine/execution_units/commit.rs`)
takes the tenant's mutation-sequence lock and validates optimistically:
it checks that no touched table's schema changed since the snapshot, then
reads every commit that landed after the snapshot sequence and intersects
each against the unit's dependency set. Dependencies are precise — the
kinds in `crates/nimbus-core/src/dependency.rs` cover whole tables,
individual documents, index ranges, predicates, observed-missing tables,
and paginated windows. Any intersection fails the whole unit with a
conflict error ("transaction conflict detected; retry the mutation");
otherwise the staged writes apply as one batch and both journal heads
advance together. This is the path behind `ctx.db` in the V8 runtime
(via `crates/nimbus-bridge`) and behind the transactional surfaces of
the protocol adapters — see
[adapter crates](/concepts/architecture/adapters/) and
[runtime and isolates](/concepts/architecture/runtime-isolates/).

## When commit visibility outruns acknowledgement

A storage provider can make a transaction durable and then lose the response
before Nimbus receives it. Nimbus therefore does not equate a returned storage
error with rollback. Each of the three engine-owned commit routes — queued
journal publication, direct `apply_mutation_with_mode*`, and
`MutationExecutionUnit` — classifies the error against durable journal
progress before deciding whether a caller may retry. An unchanged head keeps
the original definitive error. An advanced head, or progress that cannot be
read, is ambiguous: the tenant runtime is evicted and rebuilt by replay before
later work can continue. A lease-fence rejection is the exception because its
compare-and-swap proves that transaction did not commit.

Provider lease ownership belongs to the engine process, not to a loaded tenant
runtime. Rebuilding a tenant after an ambiguous result therefore reuses that
engine's owner identity and can resume the same still-live lease epoch
immediately. A different engine has a different identity and remains fenced;
lease expiry and takeover still advance the provider's durable epoch.

Lease renewal has two distinct time contracts. The engine schedules normal
attempts every ten seconds using monotonic elapsed time, so local wall-clock
steps can neither postpone a renewal nor trigger one early; shutdown explicitly
wakes and joins a worker parked before its deadline. Transient failures retry
with bounded deterministic jitter and a local safety deadline derived from the
requested provider duration. Exhausting that budget drains the committer rather
than asserting that its lease remains valid. PostgreSQL, MySQL, and libSQL still
decide validity using their database clocks inside the atomic `(owner_id,
epoch)` renewal CAS. Bounded-cardinality diagnostics label absolute expiry as
provider time and separately expose renewal/failure counts, consecutive failure
streak, monotonic age since the last success, and worker state without owner ids.

The storage contract exposes a deterministic fault boundary immediately after
commit visibility and before success returns. The redb, SQLite, libSQL,
PostgreSQL, and MySQL adapters all exercise that same boundary, so tests can
prove that acknowledgement loss preserves one durable record, accepts only an
identical replay, rejects different-content sequence reuse, and never reports
post-commit cancellation as safe to retry. The fault boundary is test control,
not a second production commit path. libSQL's simulation constructor can
separate faults in the remote primary from faults in its nested SQLite replica
cache, preventing a cache refresh from masquerading as a remote
acknowledgement-loss checkpoint.

## Committed mutations fan out

After any commit — direct, journaled, or execution-unit — the engine runs
commit processing
(`crates/nimbus-engine/src/engine/mutations/commit_processing.rs`): it
computes affected document ids, hands subscription work to the tenant's
delivery worker, collects trigger candidates, and notifies registered
committed-mutation observers
(`crates/nimbus-engine/src/engine/committed_mutations.rs`). Each observer event
names its affected tables and carries a projection token: the applied journal
frontier plus the durable committer-lease epoch that authorized provider
writes. Embedded stores use epoch zero. Provider catch-up reconciles the
journal before refreshing schema, and retains schema or lifecycle table scopes
even when a durable record has no document writes. Because this hook hangs off
the single mutation path, no write from any surface can skip fan-out.

The built-in `_nimbus.tables` observer keeps callback admission non-blocking.
Its scheduling policy lives behind the small installation seam in
`crates/nimbus-system/src/projection.rs`; bounded work ownership, cancellation,
retry, and overload recovery live in
`crates/nimbus-system/src/projection/work.rs`. Overlapping work for one table
coalesces into one dirty scope carrying the maximum projection token rather
than an event backlog. A task owns that scope until publication succeeds, so
an error, panic, or cancellation restores it before the in-flight slot is
released. Retries use one delayed scheduler with capped exponential backoff;
they do not block the mutation acknowledgement path or abandon a scope after a
fixed number of attempts.

Publication legality is owned by
`crates/nimbus-system/src/projection/publication.rs`. A source token is ordered
lexicographically as `(tenant incarnation, lease epoch, durable sequence)`.
The incarnation is a monotonic lifecycle generation retained outside the
deletable tenant database: external providers own it in shared provider
metadata, while embedded engines own it in the durable control database. A
same-id recreation therefore dominates every fence from the deleted tenant,
and late work from that deleted incarnation remains stale. One
active tenant without this creation-time authority is corrupt and fails closed;
the pre-launch lifecycle does not synthesize a legacy default while opening.
A `MutationExecutionUnit` compares that token with a private durable
`_projection_fences` row and atomically stages the visible `_nimbus.tables`
row (or deletion), its indexes, the surviving fence/tombstone, and the system
journal entry. Equal or older work is an idempotent stale no-op. Only an OCC
conflict retries inline from a fresh snapshot; acknowledgement loss and every
other error return to the retained-work scheduler. The private fence is
internal durability metadata: it is not a `SystemTable`, runtime-system-bundle
surface, or operator UI record.

Tenant-runtime load is also an observer seam. After an embedded restart or a
provider takeover, reconciliation reads the current applied source token and
coalesces active table identities with private fence rows for previously
projected or deleted scopes. It feeds those scopes through the same bounded
publication interface, so work lost with a process converges without waiting
for another source mutation. The scan is O(current and previously projected
table scopes), not O(journal length). Process-local observer generations only
cancel obsolete local tasks; they never decide cross-process publication
order.

## The scheduler

The scheduler loop (`crates/nimbus-engine/src/scheduler.rs`) sleeps until
the next due job or a wakeup signal — schedule-affecting writes wake it
immediately — and ticks all tenants concurrently, bounded by available
parallelism. Job claiming and execution live in
`crates/nimbus-engine/src/engine/scheduler/`: cron definitions expand
into scheduled runs, claimed jobs execute through the same mutation path
as everything else, and each run carries an execution id derived from the
job id. Storage refuses a duplicate execution id inside the write
transaction, so a job that was claimed, run, and re-claimed after a crash
deduplicates instead of double-applying. Startup recovery re-queues jobs
that were left in a running state by a previous process.

## Subscription delivery

Live queries are registered in a per-tenant registry and re-evaluated by
a delivery worker (`crates/nimbus-engine/src/subscriptions.rs` and its
submodules). The design trades spurious work for correctness:

- **Conservative invalidation.** A commit wakes every subscription whose
  dependency set it might intersect; a wakeup re-evaluates the full query
  rather than patching the previous result.
- **Monotonic delivery.** Each subscription tracks its last delivered
  sequence and the worker discards results that would publish older state
  after newer state.
- **Coalescing.** Overlapping wakeups for the same subscription merge
  into one re-evaluation at the latest applied sequence, so write storms
  produce current results, not backlogs.
- **Bounded channels.** Delivery channels have a fixed capacity (256); a
  subscriber that cannot keep up is removed rather than allowed to grow
  an unbounded queue inside the server.

Policy changes are handled at the same layer: replacing a table's access
policy terminates affected subscriptions so a client can never keep
streaming results under a revoked policy.

## Related pages

- [Data and mutations](/concepts/data-and-mutations/) — the user-facing
  contract this machinery implements.
- [Storage](/concepts/architecture/storage/) — the providers beneath the
  persistence calls on this page.
- [Server and transport](/concepts/architecture/server-transport/) — how
  requests reach the engine.
- [Tenancy](/concepts/architecture/tenancy/) — the per-tenant boundary
  the runtime map enforces.
