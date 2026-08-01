---
title: Convex example apps
description: Runnable Convex apps on Nimbus — a React client, a browser client without React, a Node script, the shared tasks list, a developer-console showcase, and a two-runtime demo.
sidebar:
  label: Examples
  order: 3
---

Runnable Convex apps that build on Nimbus's Convex surface. Each one authors
functions with `convex/_generated/server` and a `convex/schema.ts` and drives
them from a stock Convex client. The apps use `convex/react`, `convex/browser`,
or the Node client against a local Nimbus server. Read the
[overview](/developers/convex/) first if you have not scaffolded a project yet.

The apps live under
[`examples/convex/`](https://github.com/nimbus/nimbus/tree/main/examples/convex)
in the source repository. Run them in place from a checkout. Read the copy-out
note below before moving one elsewhere.

## Run any example

Every example uses the same two commands. Start the local development server,
which watches your code and serves the app:

```bash
nimbus dev
```

Then deploy the app:

```bash
nimbus deploy [TARGET] --convex-silo demo
```

`TARGET` is a URL or a configured target name. Omit it to target your local
server. Each app also has standalone npm scripts. A `convex:server:*` support
server pairs with a `convex:example:*` dev server. Each README lists the
scripts.

## The apps

**[React (`html`)](https://github.com/nimbus/nimbus/tree/main/examples/convex/html).**
This single-page app uses `convex/react` and generated `_generated/api.ts`
refs. It operates on a `messages` collection. It covers live inserts and a
paginated query with live invalidation. It also covers a scheduled mutation
through `ctx.scheduler.runAfter(...)`, patch, delete, and a live
`ctx.db.get(id)` detail query. It shows React error-boundary behavior for query
errors.

**[Browser client (`http`)](https://github.com/nimbus/nimbus/tree/main/examples/convex/http).**
This app uses `convex/browser` (`ConvexHttpClient`) without React. It operates
on the same `messages` collection. The composer submits through a Convex-style
action that delegates to an internal mutation. It can schedule that mutation,
call compiled `httpAction` routes, and reload a message through
`ctx.db.get(id)`.

**[Node (`node`)](https://github.com/nimbus/nimbus/tree/main/examples/convex/node).**
This script uses generated refs with two clients. `ConvexHttpClient` provides
point-in-time reads. `ConvexClient` provides live subscriptions over an
injected WebSocket implementation. The script prints each subscription
update.

**[Tasks](https://github.com/nimbus/nimbus/tree/main/examples/convex/tasks).**
This app implements the shared
[tasks](https://github.com/nimbus/nimbus/blob/main/examples/specs/tasks.md)
task list. It uses `convex/_generated/server` and `schema.ts`. Convex mutations
create, toggle, and delete tasks. A reactive Convex query keeps the newest-first
list current without polling. The app implements create, list, toggle, delete,
and live update behavior.

**[Developer console](https://github.com/nimbus/nimbus/tree/main/examples/convex/showcase).**
The `showcase` app explores the function source view. It shows
syntax-highlighted source and a symbol strip with `DEFINES` and `CALLS`
navigation. It also shows type-hover tooltips. Deploy it, then open a
function's **Source** tab in the console.

**[Runtimes](https://github.com/nimbus/nimbus/tree/main/examples/convex/runtimes).**

This app compares the two runtimes.

`digests.ts` uses `crypto.subtle.digest` on the default runtime.

`nodeDigests.ts` uses `node:crypto` with `"use node"` and produces the same
hash.

`shareIds.ts` uses the browser-safe npm package `nanoid` from a default-runtime
function. See
[the two Convex runtimes](/developers/convex/runtimes/) for each runtime's
guarantees.

## Run these in place, not copied out

Every app in this directory declares `"convex": "*"`. Inside this repository,
that value resolves to Nimbus's Convex compatibility package. The compatibility
package uses the official `convex` package name and `convex` binary. Your code
therefore runs unchanged. Outside this repository, `npm install` resolves
`"convex": "*"` to the official Convex Cloud package. That package also
replaces Nimbus's `convex` binary.

That breakage is visible. The app scripts run `convex codegen --app .`.
The official `convex` CLI rejects the Nimbus-only `--app` flag, so codegen
fails. The apps do not connect to Convex Cloud. The React client pins
`http://localhost:8080/convex/demo` and sets
`skipConvexDeploymentUrlCheck`. The other apps keep their local-server
defaults.

Until a scaffolder that rewrites the `convex` dependency to a published
Nimbus package ships, run every app in this directory from a checkout of the
repository.
