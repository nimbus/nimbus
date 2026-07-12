---
title: Run a headless agent worker
description: Run an unattended, multi-step worker that processes a batch of jobs to completion server-side, driven by the native SDK's document store and scheduler.
sidebar:
  label: Agent worker
  order: 7
---

This how-to runs a headless, autonomous worker built on the native
`@nimbus/nimbus` SDK. There is no UI: it takes a batch of jobs and runs each one
to completion inside your own Nimbus deployment, driven by the same scheduler
and document store as any other mutation.

The example lives at
[`examples/nimbus/agent-worker`](https://github.com/nimbus/nimbus/tree/main/examples/nimbus/agent-worker).
Run it in place from a checkout of the repository.

## What it does

`runWorker` takes a batch of job ids and schedules one `processJob` hop per job
through `ctx.scheduler.runAfter`, staggered by a fixed interval. Every job then
transitions from `pending` to `done` entirely server-side — no client polling,
no cron, no sandbox execution, just the database and the scheduler.

The reference driver, `run.ts`, shows the intended shape: it enqueues a batch of
jobs, kicks the worker off with one call, and then only *observes* completion
through a live subscription. It never asks the worker to do anything further.

## Run it

Start the local development server:

```bash
nimbus dev
```

Then deploy the app:

```bash
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. To run the headless driver against a running server:

```bash
npm run nimbus:example:agent-worker
```

The driver exits once every job it enqueued reaches `done`.

## The scheduling boundary it works within

`ctx.scheduler.runAfter` can only target a mutation that compiles to a single
database operation — the scheduler replays a stored plan, and that plan has no
"schedule another call" variant. So a scheduled target can never itself call
`ctx.scheduler.runAfter`: there is no self-rescheduling worker loop against this
surface, by construction. There is also no public cron API to fall back on.

This worker fits that boundary honestly. `runWorker` — which is never itself a
scheduled target — schedules every `processJob` hop up front in one unrestricted
call, staggered by an interval, rather than attempting an open-ended reschedule
chain. Scheduling the whole batch at once is how you get unattended, multi-step,
scheduler-driven work out of this surface today.
