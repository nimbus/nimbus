# Node.js Runtime

Nimbus supports a Node.js-compatible runtime for code that intentionally opts
into Node APIs. Node24 is the product default and active LTS target, Node22 is
a supported Maintenance LTS peer, Node26 is a Current/non-LTS compatibility
target, and Node20 remains
selectable only as legacy-grace regression coverage after its 2026-04-30 EOL.

This is a measured compatibility surface, not a blanket claim that every Node
built-in or CLI behavior is available.

Node compatibility is orthogonal to permission posture. Selecting Node20,
Node22, Node24, or Node26 chooses the JavaScript compatibility target;
filesystem, network, environment, subprocess, secret, identity, service, FFI,
worker, and tool access still come from the active runtime mode and explicit
grants.

## Start Here

- [Fundamentals](fundamentals.md) explains when to use `"use node"`, which
  Node targets are selectable, and how the permission boundary works.
- [Compatibility](compatibility.md) is the generated support contract.
- [Node API reference](reference/node-apis.md) lists supported, denied, and
  service-routed API families.
- [Package reference](reference/packages.md) lists package canaries and
  host-heavy diagnostic boundaries.

## Quick Example

Use `"use node"` at the top of a Convex-compatible action module:

```ts
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

Bare and `node:` built-in specifiers resolve to the same Node runtime family
when the module is eligible for Node execution:

```ts
import fs from "fs";
import nodeFs from "node:fs";
```

## Supported Versions

| Node target | Product role | Upstream fixture line | Current evidence |
| --- | --- | --- | --- |
| Node20 | Legacy-grace selectable target; EOL | `v20.20.2` | [Compatibility](compatibility.md) |
| Node22 | Supported Maintenance LTS | `v22.22.3` | [Compatibility](compatibility.md) |
| Node24 | Product default; Active LTS | `v24.16.0` | [Compatibility](compatibility.md) |
| Node26 | Current/non-LTS compatibility target | `v26.2.0` | [Compatibility](compatibility.md) |

Product default is a routing default, not an evidence priority. Current lane
support phase, release metadata, and evidence policy come from
`docs/private/architecture/runtime/node-lts-compat/node-lts-lanes.json`.

## Configure The Node Target

Use `convex.json` for Convex-compatible projects:

```json
{
  "node": {
    "nodeVersion": "24"
  }
}
```

See [configuration](configuration.md) for allowed values, diagnostics, and
debugging commands.

## Packages And Bundling

Node action modules can use local packages through the staged package pipeline.
See [packages and bundling](packages-and-bundling.md) for
`node.externalPackages`, local `node_modules` behavior, current limits, and
the generated [package reference](reference/packages.md).

## Compatibility Evidence

The current compatibility contract is summarized in
[compatibility](compatibility.md). API and package support are generated in
[reference/node-apis.md](reference/node-apis.md) and
[reference/packages.md](reference/packages.md). Generated evidence snapshots
live under [evidence](evidence/latest.md).

Maintainers refresh lane evidence with the workflow in
[refreshing Node.js runtime evidence](evidence/refreshing.md).

Deep engineering evidence remains available in
`docs/private/architecture/runtime/node-compat-evidence/latest/` and
`docs/private/architecture/runtime/node-compat-surface-matrix.md`.
