---
title: Convex usage rules
description: The contract for writing Convex functions that run correctly on Nimbus — function syntax, validators, database access, and scheduling rules.
sidebar:
  order: 3
---

Rules for writing Convex-style code that runs correctly on Nimbus. They
apply equally to humans and coding agents; paste this page into an agent's
context when it writes functions for a Nimbus backend.

## Function registration

- Always use the object syntax with `args` and `handler`:

  ```typescript
  import { v } from "convex/values";
  import { query } from "./_generated/server";

  export const getMessage = query({
    args: { id: v.id("messages") },
    handler: async (ctx, { id }) => await ctx.db.get(id),
  });
  ```

- Always declare argument validators, even when `args` is empty (`args: {}`).
- Add a `returns:` validator where practical; codegen reads it to produce
  precise client-facing types in `_generated/api.ts`.
- Use `internalQuery`, `internalMutation`, and `internalAction` for
  functions that should never be callable from clients.
- Import registrars from `./_generated/server`, values from
  `convex/values`, and function references from `./_generated/api`.

## Function calling

- `ctx.runQuery`, `ctx.runMutation`, and `ctx.runAction` exist only in
  actions and HTTP actions. Queries and mutations cannot call other
  functions — share logic through plain TypeScript helper functions
  instead.
- Pass function references (`api.messages.list`,
  `internal.messages.purge`), never the function value itself.
- Actions have no `ctx.db`. Read and write through `ctx.runQuery` and
  `ctx.runMutation`.

## Validators

- The supported set is `v.any`, `v.null`, `v.string`, `v.number`,
  `v.boolean`, `v.id`, `v.literal`, `v.array`, `v.object`, `v.optional`,
  and `v.union`. Do not use `v.int64`, `v.bytes`, `v.record`, or
  `v.float64`.
- Use `v.id("tableName")` for document IDs, not `v.string()`.
- Use `v.optional(...)` for optional fields and `v.union(v.literal(...),
  ...)` for enumerations.

## Database access

- `ctx.db.get`, `ctx.db.patch`, and `ctx.db.delete` take a document ID as
  their first argument — never a table name:

  ```typescript
  const task = await ctx.db.get(taskId);
  await ctx.db.patch(taskId, { completed: true });
  await ctx.db.delete(taskId);
  ```

- `ctx.db.insert(table, value)` takes the table name first.
- There is no `ctx.db.replace`. Update fields with `ctx.db.patch`.
- Select rows through an index, not a filter. Define the index in the
  schema and name it after its fields:

  ```typescript
  // schema: defineTable({...}).index("by_author", ["author"])
  const rows = await ctx.db
    .query("messages")
    .withIndex("by_author", (q) => q.eq("author", author))
    .collect();
  ```

- Bound result sizes: use `.take(n)` or pagination rather than unbounded
  `.collect()` on large tables.

## Pagination

- Validate pagination arguments with `paginationOptsValidator` from
  `convex/server` and pass them to `.paginate(...)`. The result has
  `page`, `isDone`, and `continueCursor`.
- For React's `usePaginatedQuery`, register the function with
  `paginatedQuery` (from `./_generated/server`) instead of `query`.

## Schema

- `convex/schema.ts` must `export default defineSchema({...})`.
- `defineTable` takes an object of field validators.
- Every index field must exist in the table definition; index names must
  be unique within a table.
- Do not declare `_id`, `_creationTime`, or `_updateTime` — system fields
  are automatic.

## HTTP actions

- Define routes only in `convex/http.ts`, and `export default` a router
  initialized with `httpRouter()`.
- Each route declares exactly one of `path` or `pathPrefix`, starting
  with `/`.
- Handlers are `httpAction` functions; they follow action rules (no
  `ctx.db`).

## Scheduling

- Only mutations can be scheduled. To run work after a mutation commits,
  schedule an internal mutation:

  ```typescript
  await ctx.scheduler.runAfter(0, internal.messages.cleanup, { id });
  ```

- `runAfter` takes a delay in milliseconds; `runAt` takes a timestamp in
  milliseconds.

## Node runtime

- Put `"use node"` at the top of a module to run it on the Node runtime.
  Such modules may contain only actions.
- Declare packages that must stay external to the bundle in `convex.json`
  under `node.externalPackages`.
- `fs` and `node:fs` style builtin specifiers are interchangeable.
