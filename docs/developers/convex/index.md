---
title: Build with the Convex API
description: Scaffold a Convex-style project on Nimbus, define a schema, write queries and mutations, and call them from clients.
sidebar:
  label: Overview
  order: 1
---

Nimbus serves the Convex API from a single self-hosted binary. It supports the
same `convex/` project layout, function model, and client packages. You do not
need a hosted service. This guide starts with an empty directory and ends with
client calls to working functions.

New to Nimbus entirely? Start with the
[developer quickstart](/get-started/quickstart/). Migrating an existing
Convex project? See [Migrate a Convex app](/developers/convex/migrate/).

## Prerequisites

- Nimbus installed (see the [quickstart](/get-started/quickstart/)).
- Node.js 22 or newer with `npm`. Nimbus runs codegen inside its own binary.
  You need Node only to install your project's npm dependencies.

## 1. Scaffold a project

```bash
nimbus init convex my-app
cd my-app
```

This creates backend files only: a schema, an example query and mutation,
`package.json`, `tsconfig.json`, and `.gitignore`. Bring your own frontend.
Pass `--install` to run `npm install` during init. Otherwise, `nimbus dev`
installs missing packages on the first run. The full file list is in the
[project layout reference](/reference/convex/project-layout/).

## 2. Start the dev loop

```bash
nimbus dev
```

The dev server:

- Serves on `http://localhost:3210` and creates a `demo` tenant. Your
  deployment URL is `http://localhost:3210/convex/demo`.
- Watches your source files, runs codegen after changes, and activates the
  updated functions. Reactive subscriptions receive the new results.
- Stores its data under `.nimbus/dev/` in your project.

Pass `--port` to change the port and `--once` to start without watching.

## 3. Define a schema

```typescript
// convex/schema.ts
import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  messages: defineTable({
    author: v.string(),
    body: v.string(),
  }).index("by_author", ["author"]),
});
```

The schema file is optional: a table without a schema accepts documents of
any shape. Adding a schema enforces the declared fields and makes the
generated types precise. The
[compatibility reference](/reference/convex/compatibility/) lists the supported
validators.

## 4. Write functions

Register functions with `query`, `mutation`, and `action` from
`./_generated/server`. Always declare argument validators.

```typescript
// convex/messages.ts
import { v } from "convex/values";
import { query, mutation } from "./_generated/server";

export const list = query({
  args: {},
  handler: async (ctx) => await ctx.db.query("messages").take(50),
});

export const send = mutation({
  args: { author: v.string(), body: v.string() },
  handler: async (ctx, { author, body }) =>
    await ctx.db.insert("messages", { author, body }),
});
```

Use an index instead of a filter when selecting rows:

```typescript
export const byAuthor = query({
  args: { author: v.string() },
  handler: async (ctx, { author }) =>
    await ctx.db
      .query("messages")
      .withIndex("by_author", (q) => q.eq("author", author))
      .collect(),
});
```

Each function kind gets a different `ctx`:

- **Queries** read the database (`ctx.db.query`, `ctx.db.get`).
- **Mutations** read and write (`ctx.db.insert`, `ctx.db.patch`,
  `ctx.db.delete`) and can schedule work with `ctx.scheduler`.
- **Actions** have no database access. They call other functions through
  `ctx.runQuery`, `ctx.runMutation`, and `ctx.runAction`, and can use
  `"use node"` modules for npm packages.

Prefix any of them with `internal` (`internalQuery`, `internalMutation`,
`internalAction`) to keep a function callable only from other functions,
never from clients. HTTP endpoints go in `convex/http.ts` with `httpAction`
and `httpRouter`. The full rule set lives in the
[usage rules reference](/reference/convex/usage-rules/).

## 5. Call functions from a client

Function references come from the generated `api` object. From React:

```tsx
import { ConvexProvider, ConvexReactClient, useQuery } from "convex/react";
import { api } from "../convex/_generated/api";

const client = new ConvexReactClient("http://localhost:3210/convex/demo");

function Messages() {
  const messages = useQuery(api.messages.list, {});
  return <ul>{messages?.map((m) => <li key={m._id}>{m.body}</li>)}</ul>;
}
```

From Node.js or any server-side script:

```typescript
import { ConvexHttpClient } from "convex/browser";
import { api } from "./convex/_generated/api.js";

const client = new ConvexHttpClient("http://localhost:3210/convex/demo");
const messages = await client.query(api.messages.list, {});
```

WebSocket clients make queries reactive. Invoke mutations and actions with
`useMutation`, `useAction`, or `client.mutation`.

## Next steps

- [Example apps](/developers/convex/examples/): runnable React, browser, and
  Node apps, plus the shared tasks list.
- [Migrate a Convex app](/developers/convex/migrate/): bring an existing
  project over.
- [Project layout](/reference/convex/project-layout/): every file
  `nimbus init convex` creates and what codegen generates.
- [Compatibility](/reference/convex/compatibility/): the supported API
  surface, validator set, and known gaps.
- [Usage rules](/reference/convex/usage-rules/): the contract for writing
  functions that run correctly on Nimbus.
