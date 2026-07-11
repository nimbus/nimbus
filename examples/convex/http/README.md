# Convex — browser client (`http`)

A browser app built on `convex/browser` (`ConvexHttpClient`) and generated refs,
without React. It operates on a `messages` collection: the composer submits
through a Convex-style action that delegates to an internal mutation, can
schedule that mutation with `ctx.scheduler.runAfter(...)`, calls compiled
`httpAction` routes, and reloads a message through `ctx.db.get(id)`.

## Run

```bash
npm run convex:server:http   # support server
npm run convex:example:http  # Vite dev server for this app
```

Run in place from a checkout of this repository — see the
[copy-out warning](../README.md) in the Convex examples README before copying
this directory out.

Tenant creation in browser code (`src/main.ts`'s unauthenticated
`POST /api/tenants`) is a local-development convenience. Provision tenants
separately before deploying beyond your own environment.

## Spec

This app is a per-surface demo, not the shared `tasks` example. See the Convex
[`tasks` spec support](../README.md#tasks-spec-support) and the
[`tasks` spec](../../specs/tasks.md), whose subset table is the target the
forthcoming `tasks` app will meet.
