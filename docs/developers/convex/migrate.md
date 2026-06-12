---
title: Migrate a Convex app to Nimbus
description: Point an existing Convex project at self-hosted Nimbus — what to change, what works unchanged, and what to check first.
sidebar:
  order: 2
---

Nimbus runs Convex projects in place: it detects the `convex/` directory,
swaps the `convex` dependency to its own provisioned package, runs codegen,
and serves the same function and client APIs from a local binary. Most
projects need only `nimbus dev` and a new deployment URL.

Before you start, scan the
[compatibility reference](/reference/convex/compatibility/) for the surfaces
your app uses. File storage, search indexes, and crons are not supported
today — if your app depends on them, it is not ready to migrate.

## What to change

### 1. Run the dev server from your project root

```bash
# in your existing Convex app directory
nimbus dev
```

`nimbus dev` detects the `convex/` directory, provisions the
Convex-compatible npm package into `.nimbus/packages/`, runs codegen, and
serves on `http://localhost:3210` with an auto-created `demo` tenant.

### 2. Let it swap the `convex` dependency

There is no manual dependency edit. When `nimbus dev` provisions, it also
rewires the `convex` entry in your `package.json` from the registry spec to
the provisioned copy and reinstalls dependencies:

```json
{
  "dependencies": {
    "convex": "file:./.nimbus/packages/convex"
  }
}
```

The regenerated `convex/_generated/` files import from `convex/server`,
`convex/browser`, and `convex/values`, and those imports must resolve to the
Nimbus-compatible package rather than the hosted-Convex one — which the
rewired dependency guarantees.

### 3. Update the deployment URL

Nimbus deployment URLs include a tenant segment:

```text
http://localhost:3210/convex/demo
```

Set whatever environment variable your clients read for the Convex
deployment URL (for example `VITE_CONVEX_URL` or `NEXT_PUBLIC_CONVEX_URL`)
to that value. You no longer need a `CONVEX_DEPLOYMENT` value from a hosted
dashboard; `nimbus dev` records its local deployment in `.env.local` as
`NIMBUS_DEPLOYMENT`.

### 4. Let codegen regenerate `_generated/`

`nimbus dev` rewrites `convex/_generated/` on every source change. For a
one-shot run without the dev server, use:

```bash
nimbus codegen --app .
```

### 5. Review your auth config

`convex/auth.config.ts` (or `.js` — exactly one, not both) carries over with
the same provider shapes:

- **OIDC**: `{ domain, applicationID }`. The token's audience must equal
  `applicationID`, and tokens with multiple audiences are rejected.
- **Custom JWT**: `{ type: "customJwt", issuer, jwks, algorithm }` with an
  optional `applicationID`. The algorithm must be `RS256` or `ES256`.

`process.env` reads in the auth config are resolved when codegen runs, so
set those variables in the environment where `nimbus dev` executes.

## What works unchanged

- **Function authoring** — `query`, `mutation`, `action`, `httpAction`, and
  their `internal` variants, with the same `args`/`handler` syntax.
- **Schema** — `defineSchema`, `defineTable`, indexes, and the core
  validator set (`v.string()`, `v.number()`, `v.id()`, `v.object()`,
  `v.union()`, and friends).
- **Database access** — `ctx.db.get`/`insert`/`patch`/`delete` and the query
  builder (`withIndex`, `filter`, `order`, `take`, `collect`, `first`,
  `unique`, `paginate`).
- **Scheduling** — `ctx.scheduler.runAfter` and `runAt` targeting mutations.
- **HTTP actions** — `httpRouter` routes in `convex/http.ts`, served under
  `{deploymentUrl}/http/...`.
- **Node actions** — `"use node"` modules and `convex.json` settings
  (`node.nodeVersion`, `node.externalPackages`).
- **Clients** — the `convex/react` hooks and the `convex/browser` HTTP and
  WebSocket clients, including reactive query subscriptions.

## What to check

- **File storage** — `ctx.storage` and the `_storage` system table are not
  available.
- **Search** — full-text and vector search (`withSearchIndex`) are not
  available.
- **Crons** — `cronJobs` from `convex/server` is not available.
- **`db.replace`** — not available; use `ctx.db.patch` to update fields.
- **Validators** — `v.int64()`, `v.bytes()`, `v.record()`, and
  `v.float64()` are not available; see the
  [compatibility reference](/reference/convex/compatibility/) for the
  supported set.
- **`usePaginatedQuery`** — requires functions registered with the
  `paginatedQuery` registrar (a Nimbus extension exported from
  `./_generated/server`). Plain `query` functions that call `.paginate()`
  still work with direct client calls, but not with the React pagination
  hook.
- **System fields** — documents carry `_id` and `_creationTime` as on
  Convex, plus a Nimbus-specific `_updateTime`.

## Run beyond dev

`nimbus dev` is the local loop. To run a standalone server, use
`nimbus start` — it does not auto-create a tenant, so follow the
[self-host quickstart](/get-started/self-host/) to set one up. For the
broader picture of what compatibility means, see
[Coming from Convex](/get-started/from-convex/).
