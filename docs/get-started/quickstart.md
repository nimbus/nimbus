---
title: Developer quickstart
description: Scaffold a Nimbus app, write TypeScript functions, and get reactive queries in about five minutes.
sidebar:
  order: 2
---

Build an app on Nimbus with server-side TypeScript functions and reactive
queries. If you'd rather connect existing drivers (MongoDB, Firestore, plain
HTTP), see the [self-host quickstart](/get-started/self-host/).

## 1. Install Nimbus

```bash
brew install nimbus/tap/nimbus
```

Other platforms ship via the install script and release binaries — see the
[install options](https://github.com/nimbus/nimbus#install).

For Convex-style authoring, also install Node.js 22 with `npm`: codegen runs
through the external Node toolchain and verifies `node --version` against the
`22.x` baseline.

## 2. Scaffold an app

```bash
nimbus init convex my-app
cd my-app
```

`nimbus init convex` scaffolds backend files only: a schema, an example query
and mutation, `package.json`, `tsconfig.json`, and `.gitignore`. Add your own
frontend, or point an existing one at the local deployment URL.

## 3. Start the dev server

```bash
nimbus dev
```

`nimbus dev` auto-runs `npm install` when declared packages are missing,
creates a `demo` tenant, and serves on `localhost:3210`. It watches the
TypeScript files, re-runs codegen on change, and activates updated functions
with reactive subscriptions.

## 4. Write functions

```typescript
// convex/messages.ts
import { query, mutation } from "./_generated/server";
import { v } from "convex/values";

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

```tsx
// In your React app — data updates in real time
const messages = useQuery(api.messages.list);
```

No REST endpoints, no GraphQL, no polling — your frontend gets reactive
queries and mutations from a single local process.

## Next steps

- [Developers](/developers/) — functions, schema, scheduling, file storage,
  auth, and the per-adapter guides.
- [Concepts](/concepts/) — how the engine, data model, and tenancy work.
- [Reference](/reference/) — CLI commands and configuration.
