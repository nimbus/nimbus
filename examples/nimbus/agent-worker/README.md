# Nimbus agent worker

A headless, autonomous worker built with the native `@nimbus/nimbus` SDK.
There is no UI: `runWorker` takes a batch of job ids and schedules one
`processJob` hop per job via `ctx.scheduler.runAfter`, then every job
transitions from `pending` to `done` entirely server-side, with no client
polling, no cron, and no sandbox execution — just the database and the
scheduler.

This is the sovereignty story made concrete a second way: an unattended,
multi-step agent workload runs to completion inside your own Nimbus
deployment, driven by the same scheduler and database primitives as any other
mutation, with nothing fabricated on top. `run.ts` is the reference driver —
it enqueues a batch of jobs, kicks the worker off with one call, and then
only *observes* completion via a live subscription; it never asks the worker
to do anything further.

## Design notes

### No self-chaining scheduled functions

`ctx.scheduler.runAfter` can only target a mutation whose handler compiles to
a single, statically-analyzable database operation that returns that
operation's own result directly — the scheduler replays a stored plan, not a
V8 closure, and that plan (`nimbus_core::Mutation`) has no "schedule another
call" variant. So a scheduled target can never itself call
`ctx.scheduler.runAfter` — there is no such thing as a self-rescheduling
worker loop against this boundary, by construction, not by omission. This app
uses a fixed-cadence batch instead: `runWorker` (unrestricted, never itself a
scheduled target) schedules every `processJob` hop up front, staggered by
`intervalMs`, rather than attempting an open-ended reschedule chain. There is
also no public cron API to fall back on (see `docs/private/plans/
examples-and-target-resolution-plan.md`, EX0.3) — scheduling the whole batch
in one unrestricted call is the honest way to get unattended, multi-step,
scheduler-driven work out of this surface.

### Job ids are threaded explicitly, not re-queried

Generated `_generated/server.ts` files re-export the SDK's generic
`mutation`/`query` helpers; there is no per-app retyping of `ctx.db` itself,
so a `ctx.db.query(...).collect()` result inside a handler body is untyped
(`Promise<unknown[]>`). Rather than have `runWorker` re-query "pending" jobs
and read `._id` off that untyped result, it takes `jobIds: v.array(v.id
("jobs"))` as an explicit, validator-typed argument — the caller (`run.ts`,
`smoke.ts`) gets real ids back from `enqueue`, which are then passed straight
through.

### A real product bug this app found and fixed

Every client-facing document id is table-scoped (`"jobs:<rawId>"`); the
engine stores and looks up documents by their bare, unscoped id. Direct
`ctx.db.get`/`patch`/`delete` calls un-scope the id first
(`resolve_convex_document_id`, in `crates/nimbus-server/src/adapters/convex/
host_bridge/db_ops/documents.rs`). The scheduled-mutation resolution path —
what runs when a `ctx.scheduler.runAfter` timer fires — did not: it went
straight from raw template substitution to deserializing a `Mutation`, so any
`v.id("table")`-typed argument threaded through the scheduler into an
Update/Delete/Insert-with-id target resolved to a `DocumentId` that still
carried its table prefix, which could never match a stored document. The
failure was silent to the caller — `ctx.scheduler.runAfter` itself succeeded
immediately — and only showed up as a server-side `WARN
nimbus_engine::scheduler: scheduled job failed ... error=document not found`
log line. Fixed in `crates/nimbus-convex/src/registry/resolution/functions/
writes.rs` by routing the resolved `Mutation` through the same
`resolve_convex_document_id` step the runtime host bridge already applies.

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. For the headless driver, run `npm run nimbus:example:agent-worker` (or
`npm run run -w nimbus-agent-worker` directly) with Nimbus running.

## Smoke verification

With Nimbus running at `http://localhost:8080`:

```bash
NIMBUS_ADMIN_TOKEN="$(nimbus auth token)" npm run smoke -w nimbus-agent-worker
```

Set `NIMBUS_NATIVE_URL` to exercise another Nimbus URL. The smoke enqueues a
fresh, timestamp-suffixed batch of jobs per run and prints one `PASS` line
per flow anchor, including a real `NimbusClient.onUpdate` push proving every
job reaches `done` with no client action beyond the initial `runWorker` call.
A server that does not require local admin authentication can omit
`NIMBUS_ADMIN_TOKEN`.
