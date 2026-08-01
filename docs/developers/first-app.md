---
title: Build your first app
description: A complete tutorial — scaffold a Nimbus project, define a schema, write functions, and watch live queries update in real time.
sidebar:
  order: 2
---

In this tutorial, you will build a small chat backend and watch it update live.
You will add a schema, a query, a mutation, and a WebSocket subscription
script. The script receives new messages when clients write them. Everything
runs locally from one binary.

You need Nimbus installed (see the
[quickstart](/get-started/quickstart/)) and Node.js 22 or newer with `npm`.

## 1. Scaffold the project

```bash
nimbus init convex chat
cd chat
```

This creates a complete backend project:

```text
chat/
├── convex/
│   ├── schema.ts      # table definitions
│   └── messages.ts    # an example query and mutation
├── package.json
├── tsconfig.json
└── .gitignore
```

## 2. Look at what you got

The schema declares a `messages` table with two string fields and an index:

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

And `convex/messages.ts` defines one query and one mutation:

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

Queries read the database. Mutations write it. Both declare validators for
their arguments, so bad input is rejected before your code runs.

## 3. Start the dev server

```bash
nimbus dev
```

The dev server installs missing npm dependencies and runs codegen. Codegen
produces `convex/_generated/`. The server also creates a `demo` tenant and
serves on `http://localhost:3210`. Your deployment URL is
`http://localhost:3210/convex/demo`. Leave the server running. It watches your
files and redeploys functions after each save.

## 4. Add a function and watch it deploy

With `nimbus dev` still running, add a query that uses the index. Append to
`convex/messages.ts`:

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

Save the file. The dev loop runs codegen again and activates the new function
immediately. You do not need to restart or deploy.

## 5. Subscribe to live results

Create `script.ts` in the project root:

```typescript
// script.ts
import { ConvexClient } from "convex/browser";
import { api } from "./convex/_generated/api.ts";

const client = new ConvexClient("http://localhost:3210/convex/demo", {
  webSocket: globalThis.WebSocket,
});

client.onUpdate(api.messages.list, {}, (messages) => {
  console.log(`-- ${messages.length} message(s) --`);
  for (const m of messages) {
    console.log(`${m.author}: ${m.body}`);
  }
});

const name = process.argv[2] ?? "anonymous";
await client.mutation(api.messages.send, {
  author: name,
  body: `hello from ${name} at ${new Date().toLocaleTimeString()}`,
});

console.log("Listening for new messages. Press Ctrl+C to exit.");
await new Promise(() => {});
```

Run it in a second terminal:

```bash
node --experimental-strip-types ./script.ts ada
```

The script prints the current messages and sends one. The subscription then
receives the updated list. A React app can consume the same reactive query.

## 6. See reactivity across clients

Open a third terminal and send a message as someone else:

```bash
node --experimental-strip-types ./script.ts grace
```

Watch the first script's output. Grace's message appears there immediately.
The server pushes new query results to each subscriber whose data changed.
The clients do not poll or refresh.

## What you built

A typed backend has a schema, an indexed query, a mutation, and live
subscriptions. One local process serves the backend and stores data under
`.nimbus/dev/` in your project.

## Next steps

- [Build with the Convex API](/developers/convex/): the full function
  model, schema rules, and calling functions from React.
- [Authenticate users](/developers/auth/): wire in your identity provider.
- [Compatibility reference](/reference/convex/compatibility/): the
  supported API surface.
