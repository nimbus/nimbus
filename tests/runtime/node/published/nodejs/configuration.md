# Node.js Runtime Configuration

Nimbus mirrors the Convex-compatible Node runtime selection shape for action
modules while keeping the runtime target explicit and evidence-backed.

## Opt In With `"use node"`

Only modules that intentionally opt into Node execution should import Node
built-ins or staged Node packages:

```ts
"use node";

import { action } from "./_generated/server";
import { readFileSync } from "node:fs";
```

Queries and mutations stay in the default runtime. If a file needs Node APIs,
keep that file as an action module and move shared default-runtime code into a
separate module.

## Select A Node Version

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
| `"20"` | Run eligible Node action modules with the Node20 legacy-grace compatibility target |
| `"22"` | Run eligible Node action modules with the Node22 Maintenance LTS compatibility target |
| `"24"` | Run eligible Node action modules with the Node24 Active LTS compatibility target; current product default |
| `"26"` | Run eligible Node action modules with the Node26 Current/non-LTS compatibility target |

If no value is configured, Nimbus uses the product default from the lane
registry, currently Node24. Product default is a routing default, not an
evidence priority.

Node26 is a Current/non-LTS compatibility target, not a preview label and not
enterprise LTS support until Node itself enters LTS and supported-LTS gates
pass. The generated [compatibility](compatibility.md) page is the source of
truth for per-version support status.

## Debug Node API Usage

Use the Node API diagnostics path when default-runtime modules accidentally
import Node built-ins, or when a package needs to be moved into the Node action
bundle:

```bash
nimbus dev --once --debug-node-apis
nimbus codegen --app . --debug-node-apis
```

Diagnostics should point to the importing module, explain whether `"use node"`
is missing, and avoid silently bundling unsupported Node-only code into the
default runtime.

## Specifier Rules

Nimbus accepts both bare and `node:` forms for supported Node built-ins:

```ts
import fs from "fs";
import fsPromises from "node:fs/promises";
```

Specifier support does not imply full built-in compatibility. The supported
surface is bounded by the compatibility matrix and generated evidence.

Use [reference/node-apis.md](reference/node-apis.md) for API-family support and
service/microVM boundaries. Use [reference/packages.md](reference/packages.md)
for package canary support and diagnostic package rows.
