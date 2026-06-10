---
title: Node.js runtime
description: Opt action modules into Nimbus's Node.js-compatible runtime with "use node".
sidebar:
  label: Overview
  order: 1
---

Nimbus runs your functions in a web-standard JavaScript isolate by default.
When an action needs Node built-ins — `node:crypto`, `node:buffer`, an npm SDK
that expects Node APIs — opt that module into the Node.js-compatible runtime
with `"use node"`.

## Opt in with `"use node"`

Put `"use node"` as the first statement of a Convex-compatible action module:

```typescript
"use node";

import { createHash } from "node:crypto";
import { action } from "./_generated/server";

export const digest = action({
  args: {},
  handler: async () => {
    return createHash("sha256").update("nimbus").digest("hex");
  },
});
```

The Node runtime is available to actions only. If a `"use node"` module
exports a query or mutation, codegen rejects it and tells you to move those
functions to a default-runtime module.

Bare and `node:` specifiers both resolve for supported built-ins:

```typescript
import fs from "fs";
import nodeFs from "node:fs";
```

## When to choose it

Stay on the default isolate runtime unless a module actually needs Node APIs:

- **Default isolate runtime** — queries, mutations, and any action that gets
  by with web-standard APIs (`fetch`, `Request`, `Response`, `URL`, Web
  Crypto, streams). Queries and mutations always run here.
- **Node runtime** — actions that import Node built-ins or npm packages that
  require them, such as HTTP and SaaS SDKs.

Keep shared code that does not need Node APIs in separate modules so the
runtime boundary stays obvious.

## Supported Node versions

Node24 is the default target. Node22 is a supported Maintenance LTS peer,
Node26 is a Current/non-LTS compatibility target, and Node20 remains
selectable only as a legacy-grace target after its 2026-04-30 end of life.
Select a version with `node.nodeVersion` in `convex.json` — see
[configuration](/developers/runtimes/nodejs/configuration/).

Node support is a measured compatibility surface, not a blanket claim that
every Node built-in is available. The
[Node compatibility reference](/reference/runtimes/node-compat/) is the
contract: what each version's status means and the evidence behind it.

## Permissions stay separate

Selecting a Node version chooses the JavaScript compatibility target. It does
not grant host access: filesystem, network, environment, subprocess, and
other resource access come from the runtime's permission mode and explicit
grants, which are configured independently. See
[how the Node runtime works](/concepts/nodejs-runtime/) for the model.

## Next steps

- [Configuration](/developers/runtimes/nodejs/configuration/) — select a
  Node version and debug Node API usage.
- [Packages and bundling](/developers/runtimes/nodejs/packages-and-bundling/)
  — use npm packages in Node actions.
- [Node API reference](/reference/runtimes/node-apis/) — supported, denied,
  and service-routed API families.
- [Package reference](/reference/runtimes/packages/) — the package-support
  matrix.
