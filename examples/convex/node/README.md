# Convex — Node (`node`)

A Node script using generated refs with both `ConvexHttpClient` (point-in-time
reads) and `ConvexClient` (live subscriptions over an injected WebSocket
implementation). It ensures a `demo` tenant, then drives the `messages`
collection and prints subscription updates as they land.

## Run

```bash
npm run convex:server:node   # support server
npm run convex:example:node  # runs the script
```

Run in place from a checkout of this repository — see the
[copy-out warning](../README.md) in the Convex examples README before copying
this directory out.

`script.ts`'s `ensureTenant()` sends `NIMBUS_ADMIN_TOKEN` when set, but falls
back to an unauthenticated `POST /api/tenants` when it is not — a
local-development convenience, not a pattern to carry past your own
environment.

## Spec

This app is a per-surface demo, not the shared `tasks` example. See the Convex
[`tasks` spec support](../README.md#tasks-spec-support) and the
[`tasks` spec](../../specs/tasks.md), whose subset table is the target the
forthcoming `tasks` app will meet.
