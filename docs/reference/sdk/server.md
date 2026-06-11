---
title: SDK Server Functions
description: Reference for @nimbus/nimbus/server and @nimbus/nimbus/values — function builders, context surfaces, schema, pagination, and validators.
sidebar:
  label: Server functions
  order: 2
---

The `@nimbus/nimbus/server` entry point defines the APIs available to code
that runs inside a Nimbus deployment: function builders, the `ctx` surfaces
they receive, schema builders, the HTTP router, and pagination helpers.
Validators live in the companion entry point `@nimbus/nimbus/values`; both are
documented on this page.

```typescript
import { query, mutation, action } from "@nimbus/nimbus/server";
import { v } from "@nimbus/nimbus/values";
```

## Function builders

Each builder takes a definition object with optional `args` validators, an
optional `returns` validator, and a `handler`:

```typescript
{
  args?: Record<string, Validator<unknown>>;
  returns?: Validator<unknown>;
  handler: (ctx, args) => unknown | Promise<unknown>;
}
```

| Builder | Handler context | Visibility |
| --- | --- | --- |
| `query(definition)` | `GenericQueryCtx` | `public` |
| `internalQuery(definition)` | `GenericQueryCtx` | `internal` |
| `paginatedQuery(definition)` | `GenericQueryCtx` | `public` |
| `internalPaginatedQuery(definition)` | `GenericQueryCtx` | `internal` |
| `mutation(definition)` | `GenericMutationCtx` | `public` |
| `internalMutation(definition)` | `GenericMutationCtx` | `internal` |
| `action(definition)` | `GenericActionCtx` | `public` |
| `internalAction(definition)` | `GenericActionCtx` | `internal` |

The builders return `RegisteredQuery`, `RegisteredPaginatedQuery`,
`RegisteredMutation`, and `RegisteredAction` values that carry the declared
visibility and the handler. Argument types are inferred from the `args`
validators; return types from `returns`.

```typescript
import { mutation, query } from "@nimbus/nimbus/server";
import { v } from "@nimbus/nimbus/values";

export const list = query({
  args: { channel: v.string() },
  handler: async (ctx, args) => {
    return await ctx.db
      .query("messages")
      .filter((q) => q.eq("channel", args.channel))
      .order("desc")
      .take(50);
  },
});

export const send = mutation({
  args: { channel: v.string(), body: v.string() },
  returns: v.string(),
  handler: async (ctx, args) => {
    return await ctx.db.insert("messages", {
      channel: args.channel,
      body: args.body,
    });
  },
});
```

`internal*` builders register functions that cannot be called from public
clients; they remain callable from other functions (for example through
`ctx.runQuery` in an action) and from the scheduler.

## Context surfaces

| Type | Fields |
| --- | --- |
| `GenericQueryCtx` | `db: GenericDatabaseReader`, `auth: Auth` |
| `GenericMutationCtx` | `db: GenericDatabaseWriter`, `scheduler: Scheduler`, `auth: Auth` |
| `GenericActionCtx` | `scheduler: Scheduler`, `auth: Auth`, `runQuery(ref, args?)`, `runMutation(ref, args?)`, `runAction(ref, args?)` |

Actions do not get direct database access; they read and write through
`runQuery` and `runMutation`.

## Database

### GenericDatabaseReader

| Method | Returns | Description |
| --- | --- | --- |
| `query(tableName)` | `QueryBuilder` | Start a query against a table. |
| `get(id)` | `Promise<unknown \| null>` | Fetch one document by `GenericId`. |

### GenericDatabaseWriter

Extends the reader with:

| Method | Returns | Description |
| --- | --- | --- |
| `insert(tableName, value)` | `Promise<string>` | Insert a document; resolves to the new document id. |
| `patch(id, value)` | `Promise<GenericId>` | Merge fields into an existing document. |
| `delete(id)` | `Promise<void>` | Delete a document. |

### QueryBuilder

| Method | Description |
| --- | --- |
| `withIndex(indexName, builder?)` | Use a named index; the optional builder receives an `IndexRangeBuilder`. |
| `filter(builder)` | Add filter conditions via a `FilterExpressionBuilder`. |
| `order("asc" \| "desc")` | Set result ordering (`QueryOrder`). |
| `collect()` | `Promise<unknown[]>` — all matching documents. |
| `take(limit)` | `Promise<unknown[]>` — at most `limit` documents. |
| `paginate(options)` | `Promise<PaginationResult<unknown>>` — one page. |
| `first()` | `Promise<unknown \| null>` — first match or `null`. |
| `unique()` | `Promise<unknown \| null>` — single match or `null`. |

`IndexRangeBuilder` and `FilterExpressionBuilder` both expose the comparison
operators `eq`, `neq`, `gt`, `gte`, `lt`, and `lte`. The filter builder
additionally has `field(name)`, which returns a `FilterField` usable as the
first argument of any comparison.

## Scheduler

Available on mutation and action contexts:

| Method | Returns | Description |
| --- | --- | --- |
| `runAfter(delayMs, mutationRef, args?)` | `Promise<string>` | Schedule a mutation after a delay; resolves to a job id. |
| `runAt(timestampMs, mutationRef, args?)` | `Promise<string>` | Schedule a mutation at an absolute time; resolves to a job id. |
| `cancel(jobId)` | `Promise<void>` | Cancel a scheduled job. |

## Auth

The `auth` field on every context implements:

| Method | Returns |
| --- | --- |
| `getUserIdentity()` | `Promise<UserIdentity \| null>` |
| `getVerifiedIdentity()` | `Promise<VerifiedIdentity \| null>` |

`UserIdentity` always carries `tokenIdentifier`, `subject`, and `issuer`, plus
optional OpenID Connect profile claims (`name`, `email`, `emailVerified`,
`pictureUrl`, `phoneNumber`, and others) and arbitrary extra claims.
`VerifiedIdentity` extends it with `kind: "oidc" | "custom_jwt"`
(`VerifiedIdentityKind`). `UserIdentityAttributes` and
`VerifiedIdentityAttributes` are the same shapes without the
server-controlled fields.

Auth providers are configured with `AuthConfig`:

```typescript
import type { AuthConfig } from "@nimbus/nimbus/server";

const authConfig: AuthConfig = {
  providers: [
    { applicationID: "my-app", domain: "https://issuer.example.com" },
    {
      type: "customJwt",
      issuer: "https://jwt.example.com",
      jwks: "https://jwt.example.com/.well-known/jwks.json",
      algorithm: "RS256",
    },
  ],
};
```

See [Authentication](/developers/auth/) for the full auth flow.

## HTTP actions and routing

`httpAction` wraps a handler that receives the action context and a standard
`Request`, and returns a `Response`:

```typescript
import { httpAction, httpRouter } from "@nimbus/nimbus/server";

const ping = httpAction(async (_ctx, request) => {
  return new Response(`pong ${new URL(request.url).pathname}`, { status: 200 });
});

const http = httpRouter().route({
  path: "/ping",
  method: "GET",
  handler: ping,
});
```

`httpAction` accepts either the handler function directly or `{ handler }`.
The result is a `PublicHttpAction` (`kind: "http_action"`, always `public`
visibility).

`httpRouter()` returns an `HttpRouter` whose `route(spec)` method appends an
`HttpRouteSpec` and returns the router for chaining. A route spec has a
`method` (`HttpRouteMethod`: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`,
`OPTIONS`, or `HEAD`), a `handler`, and either an exact `path` or a
`pathPrefix`.

## Schema

```typescript
import { defineSchema, defineTable } from "@nimbus/nimbus/server";
import { v } from "@nimbus/nimbus/values";

const schema = defineSchema({
  messages: defineTable({
    channel: v.string(),
    body: v.string(),
  }).index("by_channel", ["channel"]),
});
```

- `defineTable(fields)` returns a `TableDefinition` with the field validators
  and an `index(name, fields)` method that registers an index over one or
  more declared fields and returns the table for chaining.
- `defineSchema(tables)` returns a `SchemaDefinition` wrapping the table map.

Schema is optional in Nimbus: a table without a schema accepts any document.
Adding a schema introduces validation constraints but never removes the
ability to write to a table.

## Pagination

| Export | Description |
| --- | --- |
| `paginationOptsValidator` | Validator for `PaginationOptions`; use it as the `paginationOpts` argument of a paginated query. |
| `paginationResultValidator(itemValidator)` | Builds a validator for `PaginationResult<Item>`. |
| `PaginationOptions` | `{ numItems, cursor, endCursor?, id?, maximumRowsRead?, maximumBytesRead? }`. `cursor` is `Cursor \| null`. |
| `PaginationResult<Item>` | `{ page: Item[], isDone: boolean, continueCursor: Cursor, splitCursor?, pageStatus? }`. |
| `Cursor` | `string`. |
| `PaginationStatus` | `"SplitRecommended" \| "SplitRequired" \| null` (the `pageStatus` field of a result). |

Note that the React entry point exports a different `PaginationStatus` type
for hook loading states; see [React](/reference/sdk/react/).

## Validators — `@nimbus/nimbus/values`

The `v` namespace builds `Validator<T>` values:

| Constructor | Validates |
| --- | --- |
| `v.any()` | Any JSON value. |
| `v.null()` | `null`. |
| `v.string()` | Strings. |
| `v.number()` | Numbers. |
| `v.boolean()` | Booleans. |
| `v.id(tableName)` | A document id for the named table (`GenericId<TableName>`). |
| `v.literal(value)` | Exactly the given JSON value. |
| `v.array(element)` | Arrays of `element`. |
| `v.object(fields)` | Objects with the given field validators. |
| `v.optional(inner)` | `inner` or absent. |
| `v.union(...members)` | Any of the member validators. |

Type-level exports:

| Export | Description |
| --- | --- |
| `GenericId<TableName>` | Branded `string` type for document ids. |
| `Validator<T>` | The validator value type. |
| `Infer<Schema>` | Extracts the TypeScript type a validator accepts. |

```typescript
import { v, type Infer } from "@nimbus/nimbus/values";

const messageValidator = v.object({
  channel: v.string(),
  body: v.string(),
  pinned: v.optional(v.boolean()),
});

type Message = Infer<typeof messageValidator>;
// { channel: string; body: string; pinned?: boolean | undefined }
```

## Exports — `@nimbus/nimbus/server`

Functions: `query`, `internalQuery`, `paginatedQuery`,
`internalPaginatedQuery`, `mutation`, `internalMutation`, `action`,
`internalAction`, `httpAction`, `httpRouter`, `defineTable`, `defineSchema`,
`paginationResultValidator`, and the value `paginationOptsValidator` — all
documented above.

Types:

| Type | Description |
| --- | --- |
| `Auth` | Identity lookup interface on every `ctx`. |
| `AuthConfig` | `{ providers: AuthProvider[] }`. |
| `AuthProvider` | OIDC (`{ applicationID, domain }`) or custom JWT provider entry. |
| `Cursor` | Pagination cursor (`string`). |
| `DefaultFunctionArgs` | `Record<string, unknown>` — fallback args type. |
| `FilterExpressionBuilder` | Filter construction surface for `QueryBuilder.filter`. |
| `FilterField` | Field handle from `FilterExpressionBuilder.field`. |
| `FunctionReference` | Union of query/mutation/action/paginated-query references. |
| `GenericActionCtx` | Action context. |
| `GenericDatabaseReader` | Read-only database surface. |
| `GenericDatabaseWriter` | Read-write database surface. |
| `GenericMutationCtx` | Mutation context. |
| `GenericQueryCtx` | Query context. |
| `HttpRouteMethod` | HTTP method union for routes. |
| `HttpRouteSpec` | One route entry (`path`/`pathPrefix`, `method`, `handler`). |
| `HttpRouter` | Router returned by `httpRouter()`. |
| `IndexRangeBuilder` | Index range construction surface for `withIndex`. |
| `PaginationOptions` | Page request shape. |
| `PaginationResult` | Page response shape. |
| `PaginationStatus` | Page status union. |
| `PublicHttpAction` | Result of `httpAction`. |
| `QueryBuilder` | Fluent query surface. |
| `QueryOrder` | `"asc" \| "desc"`. |
| `RegisteredAction` | Result of `action` / `internalAction`. |
| `RegisteredMutation` | Result of `mutation` / `internalMutation`. |
| `RegisteredPaginatedQuery` | Result of `paginatedQuery` / `internalPaginatedQuery`. |
| `RegisteredQuery` | Result of `query` / `internalQuery`. |
| `Scheduler` | Scheduling surface on mutation and action contexts. |
| `SchemaDefinition` | Result of `defineSchema`. |
| `TableDefinition` | Result of `defineTable`. |
| `UserIdentity` | Identity claims from `getUserIdentity`. |
| `UserIdentityAttributes` | `UserIdentity` without `tokenIdentifier`. |
| `VerifiedIdentity` | Identity with verification `kind`. |
| `VerifiedIdentityAttributes` | `VerifiedIdentity` without `kind`/`tokenIdentifier`. |
| `VerifiedIdentityKind` | `"oidc" \| "custom_jwt"`. |

## Exports — `@nimbus/nimbus/values`

`v`, `GenericId`, `Validator`, `Infer` — all documented above.
