---
title: Convex example apps
description: Runnable Convex apps on Nimbus — a React client, a browser client without React, a Node script, the shared tasks list, a developer-console showcase, and a two-runtime demo.
sidebar:
  label: Examples
  order: 3
---

Runnable Convex apps that build on Nimbus's Convex surface. Each one authors
functions with `convex/_generated/server` and a `convex/schema.ts` and drives
them from a stock Convex client — `convex/react`, `convex/browser`, or the Node
client — against a local Nimbus server. Read the
[overview](/developers/convex/) first if you have not scaffolded a project yet.

The apps live under
[`examples/convex/`](https://github.com/nimbus/nimbus/tree/main/examples/convex)
in the source repository. Run them in place from a checkout — see the copy-out
note below before moving one elsewhere.

## Run any example

Every example uses the same two commands. Start the local development server,
which watches your code and serves the app:

```bash
nimbus dev
```

Then deploy the app:

```bash
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. Each app also has standalone npm scripts — a `convex:server:*` support
server paired with a `convex:example:*` dev server — listed in its README.

## The apps

- **[React (`html`)](https://github.com/nimbus/nimbus/tree/main/examples/convex/html)**
  — a single-page React app on `convex/react` and generated `_generated/api.ts`
  refs. It operates on a `messages` collection and exercises live inserts, a
  paginated query with live invalidation, a scheduled mutation via
  `ctx.scheduler.runAfter(...)`, patch and delete, a live `ctx.db.get(id)`
  detail query, and React error-boundary behavior for query errors.
- **[Browser client (`http`)](https://github.com/nimbus/nimbus/tree/main/examples/convex/http)**
  — the same `messages` collection driven from `convex/browser`
  (`ConvexHttpClient`) without React. The composer submits through a
  Convex-style action that delegates to an internal mutation, can schedule that
  mutation, calls compiled `httpAction` routes, and reloads a message through
  `ctx.db.get(id)`.
- **[Node (`node`)](https://github.com/nimbus/nimbus/tree/main/examples/convex/node)**
  — a Node script using generated refs with both `ConvexHttpClient` for
  point-in-time reads and `ConvexClient` for live subscriptions over an injected
  WebSocket implementation. It prints subscription updates as they land.
- **[Tasks](https://github.com/nimbus/nimbus/tree/main/examples/convex/tasks)**
  — the shared [tasks](https://github.com/nimbus/nimbus/blob/main/examples/specs/tasks.md)
  task list, authored with `convex/_generated/server` and `schema.ts`. Creates,
  toggles, and deletes are Convex mutations; the newest-first list is a reactive
  Convex query that updates without polling. It implements the full spec —
  create, list, toggle, delete, and the live update.
- **[Showcase](https://github.com/nimbus/nimbus/tree/main/examples/convex/showcase)**
  — a small app for exploring the developer console's function source view:
  syntax-highlighted source, a symbol strip with `DEFINES`/`CALLS` navigation,
  and type-hover tooltips. Deploy it, then open a function's **Source** tab in
  the console.
- **[Runtimes](https://github.com/nimbus/nimbus/tree/main/examples/convex/runtimes)**
  — the two-runtime story side by side. `digests.ts` (default runtime) hashes
  with `crypto.subtle.digest`; `nodeDigests.ts` (`"use node"`) hashes the same
  input with `node:crypto`, and both agree. `shareIds.ts` uses the browser-safe
  npm package `nanoid` from a default-runtime function. See
  [the two Convex runtimes](/developers/convex/runtimes/) for what each runtime
  guarantees.

## Run these in place, not copied out

React, the browser client, Node, Tasks, and Runtimes depend on
`"convex": "*"`. Inside
this repository that resolves to Nimbus's Convex compatibility package, which
deliberately takes the official `convex` package name and `convex` binary so
your code runs unchanged. Copy one of those apps out of the repository and
`npm install`, and `"convex": "*"` instead resolves to the official Convex
Cloud package from the npm registry, replacing Nimbus's — including its
`convex` binary.

That breakage is visible, not silent. The apps' scripts run
`convex codegen --app .`, but `--app` is a Nimbus-only flag that the official
`convex` CLI rejects, so codegen fails loudly. And nothing quietly talks to
Convex Cloud: the React client pins `http://localhost:8080/convex/demo` (with
`skipConvexDeploymentUrlCheck` set), and the other apps keep their own
local-server defaults.

Showcase is different: it pins `"convex": "file:./.nimbus/packages/convex"`,
a workspace-relative local-file dependency, not `"convex": "*"`. Copy it out
and `npm install` fails at dependency resolution — a missing local path, not
a silent swap to the Convex Cloud package — because that path only exists
inside a built monorepo checkout.

Until a scaffolder that rewrites the `convex` dependency to a published
Nimbus package ships, run every app in this directory from a checkout of the
repository.
