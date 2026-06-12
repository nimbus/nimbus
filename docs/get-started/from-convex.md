---
title: Coming from Convex
description: What Convex compatibility means in Nimbus — what works today, what differs, and how to try an existing project.
sidebar:
  order: 4
---

Nimbus implements a Convex-compatible backend: the same function model
(queries, mutations, actions, HTTP actions), the same `convex/` project
layout, schema and validators, scheduling and crons, and reactive
subscriptions — self-hosted in a single binary.

## What carries over

- **Authoring.** `convex/` directory, `defineSchema`/`defineTable`, the `v`
  validator builder, `query`/`mutation`/`action`/`httpAction`, internal
  functions, and `"use node"` actions on the Node compatibility runtime.
- **Clients.** The Convex-compatible npm surface (`convex/react` hooks,
  browser and HTTP clients) pointed at your local deployment URL.
- **Workflow.** `nimbus dev` watches, runs codegen, and serves reactive
  updates the way `npx convex dev` does — entirely locally.

## Try it

With an existing Convex project, run Nimbus from its root:

```bash
# in your existing Convex app directory
nimbus dev
```

`nimbus dev` recognizes the `convex/` directory, provisions the
Convex-compatible package from inside the binary, and rewires the app's
`convex` dependency in `package.json` to the provisioned copy — no
registry access and no manual dependency edit.

Starting fresh instead? Scaffold a new app:

```bash
nimbus init convex my-app
cd my-app
nimbus dev
```

## What differs

Nimbus is pre-launch and tracks compatibility honestly rather than claiming
parity: some surfaces are complete, some are bounded, and Node-runtime
coverage is documented per version. The compatibility matrix in
[Reference](/reference/) is the source of truth for what is supported today.

## Next steps

- [Developer quickstart](/get-started/quickstart/) — the five-minute path.
- [Developers](/developers/) — per-feature guides as you build.
- [Concepts](/concepts/) — how Nimbus's engine differs under the hood.
