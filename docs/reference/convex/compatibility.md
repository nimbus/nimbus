---
title: Convex API compatibility
description: The Convex surface Nimbus supports today — functions, database, validators, schema, scheduling, auth, clients — and what is not available.
sidebar:
  order: 2
---

The Convex API surface Nimbus supports today. Anything not listed as
supported here should be assumed unsupported.

## Functions

| Registrar | Status |
| --- | --- |
| `query` | Supported |
| `internalQuery` | Supported |
| `mutation` | Supported |
| `internalMutation` | Supported |
| `action` | Supported |
| `internalAction` | Supported |
| `httpAction` | Supported |
| `paginatedQuery` | Nimbus extension |
| `internalPaginatedQuery` | Nimbus extension |

## Function context

| Capability | Query | Mutation | Action / HTTP action |
| --- | --- | --- | --- |
| `ctx.db` reads | Yes | Yes | No |
| `ctx.db` writes | No | Yes | No |
| `ctx.scheduler` | No | Yes | Yes |
| `ctx.runQuery` / `ctx.runMutation` / `ctx.runAction` | No | No | Yes |
| `ctx.auth` | Yes | Yes | Yes |

## Database

| Surface | Status |
| --- | --- |
| `db.get(id)` | Supported |
| `db.insert(table, value)` | Supported |
| `db.patch(id, value)` | Supported |
| `db.delete(id)` | Supported |
| `db.replace(id, value)` | Not supported |
| `db.system` | Not supported |
| `db.query(table)` | Supported |
| `.withIndex(name, builder?)` with `eq`, `neq`, `gt`, `gte`, `lt`, `lte` | Supported |
| `.filter(builder)` | Supported |
| `.order("asc" \| "desc")` | Supported |
| `.take(n)`, `.collect()`, `.first()`, `.unique()` | Supported |
| `.paginate(paginationOpts)` | Supported |

## Validators

| Validator | Status |
| --- | --- |
| `v.any()` | Supported |
| `v.null()` | Supported |
| `v.string()` | Supported |
| `v.number()` | Supported |
| `v.boolean()` | Supported |
| `v.id(table)` | Supported |
| `v.literal(value)` | Supported |
| `v.array(element)` | Supported |
| `v.object(fields)` | Supported |
| `v.optional(inner)` | Supported |
| `v.union(...members)` | Supported |
| `v.int64()` | Not supported |
| `v.bytes()` | Not supported |
| `v.record(keys, values)` | Not supported |
| `v.float64()` | Not supported (use `v.number()`) |

## Schema

- `defineSchema` and `defineTable(fields).index(name, fields)` are
  supported. `defineTable` takes an object of field validators; a top-level
  union table definition is not supported.
- The schema file is optional. A table without a schema accepts documents
  of any shape; adding a schema enforces the declared constraints.
- The schema module must `export default defineSchema(...)`.
- Index fields must exist in the table definition, and index names must be
  unique per table.
- System fields are `_id`, `_creationTime`, and `_updateTime`.
  `_updateTime` is a Nimbus addition.

## Scheduling

- `ctx.scheduler.runAfter(delayMs, ref, args)`,
  `ctx.scheduler.runAt(timestampMs, ref, args)`, and
  `ctx.scheduler.cancel(jobId)` are supported.
- Only mutations can be scheduled.
- Crons (`cronJobs` from `convex/server`) are not supported.

## HTTP actions

- Routes are read from `convex/http.ts` only, which must `export default` a
  router initialized with `httpRouter()`.
- Each route declares exactly one of `path` or `pathPrefix`; both forms must
  start with `/`.
- Routes are served under `{deploymentUrl}/http/{path}`.

## Authentication

| Provider | Shape |
| --- | --- |
| OIDC | `{ domain, applicationID }` |
| Custom JWT | `{ type: "customJwt", issuer, jwks, algorithm }` with optional `applicationID` |

- Provider config lives in `convex/auth.config.ts` or `.js` (exactly one).
- OIDC metadata is discovered at
  `{issuer}/.well-known/openid-configuration`.
- An OIDC token's `aud` must equal the provider's `applicationID`;
  multi-audience tokens are rejected.
- Custom JWT algorithms are limited to `RS256` and `ES256`.
- `ctx.auth.getUserIdentity()` is supported; identities carry
  `tokenIdentifier` in the form `issuer|subject`. Nimbus additionally
  exposes `ctx.auth.getVerifiedIdentity()`.

## Clients

The `convex` npm package Nimbus provisions exposes `convex/react`,
`convex/browser`, `convex/server`, and `convex/values`.

| Export | Status |
| --- | --- |
| `ConvexProvider`, `ConvexProviderWithAuth` | Supported |
| `ConvexReactClient` | Supported |
| `useQuery`, `useMutation`, `useAction`, `useQueries` | Supported |
| `useConvex`, `useConvexAuth`, `useConvexConnectionState` | Supported |
| `usePaginatedQuery` | Supported for functions registered with `paginatedQuery` |
| `ConvexHttpClient`, `ConvexClient` (browser) | Supported |
| `paginationOptsValidator` | Supported |
| `ConvexError` | Not exported |

WebSocket clients reconnect automatically and serve reactive query
subscriptions.

## Deployment endpoints

Each tenant's deployment URL is `{host}/convex/{tenantId}`. Under it:

| Path | Purpose |
| --- | --- |
| `POST /query` | Run a query |
| `POST /query/paginated` | Run a paginated query |
| `POST /mutation` | Run a mutation |
| `POST /action` | Run an action |
| `/http/{path}` | HTTP actions |
| `POST /schedule/run_after`, `POST /schedule/run_at` | Schedule a mutation |
| `DELETE /schedule/{jobId}` | Cancel a scheduled job |
| `/ws` | WebSocket subscriptions |

## Node runtime

- `"use node"` modules are supported and may contain only actions.
- The Node version is set by `convex.json` `node.nodeVersion`: `"20"`,
  `"22"`, `"24"`, or `"26"` (default `"24"`).
- External packages are declared in `convex.json`
  `node.externalPackages` — an explicit list, or `["*"]` alone.
- Node builtin imports are accepted in both bare and `node:` forms; `fs`
  and `node:fs` resolve identically.
- `"use bun"` modules fail closed unless a Bun runtime adapter is linked
  into the server build.

## Not supported

- File storage: `ctx.storage` and the `_storage` system table.
- Search: full-text (`withSearchIndex`) and vector search.
- Crons: `cronJobs` from `convex/server`.
- `db.replace` and `db.system`.
- Validators `v.int64()`, `v.bytes()`, `v.record()`, `v.float64()`.
- The `ConvexError` export.
