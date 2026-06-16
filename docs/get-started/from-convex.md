---
title: Coming from Convex
description: What Convex compatibility means in Nimbus — what carries over, what differs, and which route to take for a fresh or existing project.
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

## Pick your route

- **Starting fresh?** Follow the
  [5-minute quickstart](/get-started/quickstart/) to scaffold a new Convex
  app on Nimbus and get reactive queries running.
- **Have an existing Convex project?** Follow
  [Migrate from Convex](/developers/convex/migrate/) to point its `convex/`
  directory at Nimbus and rewire the `convex` dependency to the
  provisioned drop-in package.

## What differs

Compatibility is per-surface: some are complete, some are bounded.

- **Node-runtime coverage is per-version and bounded.** `"use node"`
  actions run on the Node compatibility runtime, whose support is
  documented per Node major — see
  [current capabilities](/reference/current-capabilities/).
- **A deployment is a single process today.** There is no clustering layer;
  one Nimbus binary serves the workload — see
  [Scaling](/concepts/scaling/).

The [Convex compatibility matrix](/reference/convex/compatibility/) is the
source of truth for what is supported today.

## Next steps

- [Developer quickstart](/get-started/quickstart/) — the five-minute path.
- [Developers](/developers/) — per-feature guides as you build.
- [Concepts](/concepts/) — how Nimbus's engine differs under the hood.
