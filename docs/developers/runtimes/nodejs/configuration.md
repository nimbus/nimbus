---
title: Configure the Node runtime
description: Select a Node version for action modules and debug Node API usage.
sidebar:
  order: 2
---

Nimbus mirrors the Convex-compatible Node runtime selection shape: action
modules opt in with `"use node"`, and `convex.json` selects the Node version
they run against.

## Opt in with `"use node"`

Only modules that intentionally opt into Node execution should import Node
built-ins or npm packages that require them:

```typescript
"use node";

import { action } from "./_generated/server";
import { readFileSync } from "node:fs";
```

Queries and mutations stay in the default runtime. If a file needs Node APIs,
keep that file as an action-only module and move shared default-runtime code
into a separate module.

## Select a Node version

Set `node.nodeVersion` in `convex.json`:

```json
{
  "node": {
    "nodeVersion": "24"
  }
}
```

Allowed values:

| Value | Meaning |
| --- | --- |
| `"20"` | Run eligible Node action modules against the Node20 legacy-grace target |
| `"22"` | Run eligible Node action modules against the Node22 Maintenance LTS target |
| `"24"` | Run eligible Node action modules against the Node24 Active LTS target; the default |
| `"26"` | Run eligible Node action modules against the Node26 Current/non-LTS target |

If no value is configured, Nimbus uses Node24. Any other value fails codegen
with an error listing the accepted versions.

The default is a routing default, not an evidence priority. The
[Node compatibility reference](/reference/runtimes/node-compat/) is the
source of truth for per-version support status.

## Debug Node API usage

If a default-runtime module accidentally imports a Node built-in, codegen
fails and names the importing module. Use the Node API diagnostics flag for
detail on what to move behind `"use node"`:

```bash
nimbus dev --once --debug-node-apis
nimbus codegen --app . --debug-node-apis
```

Diagnostics point to the importing module and explain whether `"use node"`
is missing, instead of silently bundling unsupported Node-only code into the
default runtime.

## Specifier rules

Nimbus accepts both bare and `node:` forms for supported Node built-ins:

```typescript
import fs from "fs";
import fsPromises from "node:fs/promises";
```

Specifier support does not imply full built-in compatibility. The supported
surface is bounded by the
[Node API reference](/reference/runtimes/node-apis/) and the
[compatibility contract](/reference/runtimes/node-compat/).

## Local authoring toolchain

Authoring flows (codegen and dependency install) run through your locally
installed Node toolchain, which is separate from the runtime compatibility
target your functions execute against. Nimbus verifies Node.js 22.x as the
authoring baseline: older versions are rejected, and newer majors proceed
with a best-effort compatibility warning.
