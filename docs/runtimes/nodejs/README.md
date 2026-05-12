# Node.js Runtime

Nimbus supports a Node.js-compatible runtime for code that intentionally opts
into Node APIs. Node22 is the default target today. Node20, Node22, and Node24
are selectable, evidence-backed lanes.

This is a measured compatibility surface, not a blanket claim that every Node
built-in or CLI behavior is available.

Node compatibility is orthogonal to permission posture. Selecting Node20,
Node22, or Node24 chooses the JavaScript compatibility target; filesystem,
network, environment, subprocess, secret, identity, service, FFI, worker, and
tool access still come from the active runtime mode and explicit grants. The
default product lane is `Standard` mode with an internal `Application` preset,
which grants only the bounded runtime roots and Node-compatible local loopback
surface documented by the evidence below.

## Quick Example

Use `"use node"` at the top of a Convex-compatible action module:

```ts
"use node";

import fs from "node:fs";
import { action } from "./_generated/server";

export const readReadme = action({
  args: {},
  handler: async () => {
    return fs.readFileSync("README.md", "utf8");
  },
});
```

Bare and `node:` built-in specifiers resolve to the same Node runtime family
when the module is eligible for Node execution:

```ts
import fs from "fs";
import nodeFs from "node:fs";
```

## Supported Versions

| Node target | Product role | Upstream fixture line | Current evidence |
| --- | --- | --- | --- |
| Node20 | Supported selectable target | `v20.20.2` | [Compatibility](compatibility.md) |
| Node22 | Default selectable target | `v22.15.0` | [Compatibility](compatibility.md) |
| Node24 | Supported selectable target | `v24.15.0` | [Compatibility](compatibility.md) |

Node22 remains the default until a deliberate Node24-default migration.

## Configure The Node Target

Use `convex.json` for Convex-compatible projects:

```json
{
  "node": {
    "nodeVersion": "22"
  }
}
```

See [configuration](configuration.md) for allowed values, diagnostics, and
debugging commands.

## Packages And Bundling

Node action modules can use local packages through the staged package pipeline.
See [packages and bundling](packages-and-bundling.md) for
`node.externalPackages`, local `node_modules` behavior, and current limits.

## Compatibility Evidence

The current compatibility contract is summarized in
[compatibility](compatibility.md). Generated evidence snapshots live under
[evidence](evidence/latest.md).

Maintainers refresh lane evidence with the workflow in
[refreshing Node.js runtime evidence](evidence/refreshing.md).

Deep engineering evidence remains available in
`docs/architecture/runtime/node-compat-evidence/latest/` and
`docs/architecture/runtime/node-compat-surface-matrix.md`.
