---
title: Migrate a Convex app to Nimbus
description: Point an existing Convex project at self-hosted Nimbus — what to change, what works unchanged, and what to check first.
sidebar:
  order: 2
---

Nimbus runs Convex projects in place: it detects the `convex/` directory,
swaps the `convex` dependency, and runs codegen. Its provisioned package
serves the same function and client APIs from a local binary. Most projects
need only `nimbus dev` and a new deployment URL.

Before you start, scan the
[compatibility reference](/reference/convex/compatibility/) for the surfaces
your app uses. Nimbus does not support file storage, search indexes, or crons
today. An app that depends on them is not ready to migrate.

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
Nimbus-compatible package. The rewired dependency prevents them from
resolving to the hosted Convex package.

### 3. Update the deployment URL

Nimbus deployment URLs include a tenant segment:

```text
http://localhost:3210/convex/demo
```

Set whatever environment variable your clients read for the Convex
deployment URL (for example `VITE_CONVEX_URL` or `NEXT_PUBLIC_CONVEX_URL`)
to that value. You no longer need a `CONVEX_DEPLOYMENT` value from a hosted
dashboard. `nimbus dev` records its local deployment in `.env.local` as
`NIMBUS_DEPLOYMENT`.

### 4. Let codegen regenerate `_generated/`

`nimbus dev` rewrites `convex/_generated/` on every source change. For a
one-shot run without the dev server, use:

```bash
nimbus codegen --app .
```

### 5. Review your auth config

Keep exactly one auth config: `convex/auth.config.ts` or
`convex/auth.config.js`. Nimbus supports the same provider shapes:

- **OIDC**: `{ domain, applicationID }`. The token's audience must equal
  `applicationID`, and Nimbus rejects tokens with multiple audiences.
- **Custom JWT**: `{ type: "customJwt", issuer, jwks, algorithm }` with an
  optional `applicationID`. The algorithm must be `RS256` or `ES256`.

Codegen resolves `process.env` reads in the auth config. Set those variables
in the environment where `nimbus dev` executes.

Nimbus binds each generated auth configuration to one deployment silo.
The URL silo selects that trusted verifier before Nimbus examines a bearer.
There is no global subject- or issuer-to-silo lookup. When deploying outside
the dev loop, name the silo explicitly:

```bash
nimbus deploy [TARGET] --convex-silo demo
```

`NIMBUS_CONVEX_SILO` is the environment-variable equivalent. Nimbus refuses
activation for a Convex deploy without either value.

## What works unchanged

- **Function authoring:** `query`, `mutation`, `action`, `httpAction`, and
  their `internal` variants, with the same `args`/`handler` syntax.
- **Schema:** `defineSchema`, `defineTable`, indexes, and the core
  validator set (`v.string()`, `v.number()`, `v.id()`, `v.object()`,
  `v.union()`, and friends).
- **Database access:** `ctx.db.get`/`insert`/`patch`/`delete` and the query
  builder (`withIndex`, `filter`, `order`, `take`, `collect`, `first`,
  `unique`, `paginate`).
- **Scheduling:** `ctx.scheduler.runAfter` and `runAt` targeting mutations.
- **HTTP actions:** `httpRouter` routes in `convex/http.ts`, served under
  `{deploymentUrl}/http/...`.
- **Node actions:** `"use node"` action modules run on Nimbus's
  Node-compatible runtime (node-compat on V8, not a separate Node process).
  See [the two Convex runtimes](/developers/convex/runtimes/) for how the
  default and Node runtimes differ.
- **Clients:** the `convex/react` hooks and the `convex/browser` HTTP and
  WebSocket clients, including reactive query subscriptions.

## What to check

- **`convex.json`:** Nimbus reads and applies this file. `node.nodeVersion`
  selects the Node lane for your `"use node"` actions. Select `"20"`, `"22"`,
  `"24"`, or `"26"`. The default is `"24"`. Codegen applies
  `node.externalPackages` and rejects an undeclared npm import in a
  `"use node"` module. The `functions` field relocates the source directory.
  Setting `generateCommonJSApi` emits
  `convex/_generated/api_cjs.cjs` beside the ES module file.
- **File storage:** `ctx.storage` and the `_storage` system table are not
  available.
- **Search:** full-text and vector search (`withSearchIndex`) are not
  available.
- **Crons:** `cronJobs` from `convex/server` is not available.
- **`db.replace`:** not available. Use `ctx.db.patch` to update fields.
- **Validators:** `v.int64()`, `v.bytes()`, `v.record()`, and
  `v.float64()` are not available. See the
  [compatibility reference](/reference/convex/compatibility/) for the
  supported set.
- **`usePaginatedQuery`:** requires functions registered with the
  `paginatedQuery` registrar (a Nimbus extension exported from
  `./_generated/server`). Plain `query` functions that call `.paginate()`
  still work with direct client calls, but not with the React pagination
  hook.
- **System fields:** documents carry `_id` and `_creationTime` as on
  Convex, plus a Nimbus-specific `_updateTime`.

## Run beyond dev

`nimbus dev` is the local loop. To run a standalone server, use
`nimbus start`. It does not auto-create a tenant, so follow the
[self-host quickstart](/get-started/self-host/) to set one up. For the
broader picture of what compatibility means, see
[Coming from Convex](/get-started/from-convex/).
