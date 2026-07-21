---
title: Native example apps
description: Runnable apps on the native @nimbus/nimbus SDK — a browser playground and the shared tasks list, with reactive documents over HTTP writes and live WebSocket reads.
sidebar:
  label: Examples
  order: 2
---

Runnable apps built on Nimbus's own SDK, `@nimbus/nimbus`: reactive documents
written over HTTP and read through live WebSocket subscriptions, with no
compatibility layer in between. Read the [overview](/developers/native/) first
if you have not made a native request yet.

The apps live under
[`examples/nimbus/`](https://github.com/nimbus/nimbus/tree/main/examples/nimbus)
in the source repository. Run them in place from a checkout.

## Run any example

```bash
nimbus dev
```

`nimbus dev` starts a local server for the apps to talk to. These apps drive
Nimbus directly over the native SDK's HTTP and WebSocket transports — there is
no Nimbus function bundle to deploy, so there is no `nimbus deploy` step. The
`html` app also runs on a standalone Vite dev server with
`npm run nimbus:example:html`.

## The apps

- **[Browser (`html`)](https://github.com/nimbus/nimbus/tree/main/examples/nimbus/html)**
  — a single-page playground built with Vite on the SDK's REST and subscription
  transports (`@nimbus/nimbus/transports/rest`). It creates a tenant, installs a
  schema, inserts documents, schedules a mutation with
  `ctx.scheduler.runAfter(...)`, and watches live subscription results update in
  place.
- **[Tasks](https://github.com/nimbus/nimbus/tree/main/examples/nimbus/tasks)**
  — the shared [tasks](https://github.com/nimbus/nimbus/blob/main/examples/specs/tasks.md)
  task list. It installs the `tasks` schema, writes over HTTP, and keeps the
  browser list current through a WebSocket subscription. It implements the full
  spec — create, list, toggle, delete, and the live update.

## Agent examples

Two native-SDK examples build agents on the same document store and scheduler,
documented in the Agents section:

- [Agent chat](/agents/agent-chat/) — a durable chat agent that remembers facts
  and schedules its own follow-ups.
- [Agent worker](/agents/agent-worker/) — a headless worker that runs a batch of
  jobs to completion server-side.
