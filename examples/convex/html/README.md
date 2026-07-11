# Convex — React (`html`)

A React single-page app built on `convex/react` and generated
`_generated/api.ts` refs over Nimbus's Convex transport. It operates on a
`messages` collection and exercises live inserts, a paginated query with live
invalidation, a scheduled mutation via `ctx.scheduler.runAfter(...)`,
patch/delete, a live `ctx.db.get(id)` detail query, and React error-boundary
behavior for query errors.

## Run

```bash
npm run convex:server:html   # support server
npm run convex:demo:html     # Vite dev server for this app
```

Run in place from a checkout of this repository — see the
[copy-out warning](../README.md) in the Convex examples README before copying
this directory out.

## Spec

This app is a per-surface demo, not the shared `tasks` example. See the Convex
[`tasks` spec support](../README.md#tasks-spec-support) and the
[`tasks` spec](../../specs/tasks.md), whose subset table is the target the
forthcoming `tasks` app will meet.
