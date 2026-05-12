# Node.js Runtime Configuration

Neovex mirrors the Convex-compatible Node runtime selection shape for action
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
    "nodeVersion": "22"
  }
}
```

Allowed values:

| Value | Meaning |
| --- | --- |
| `"20"` | Run eligible Node action modules with the Node20 compatibility target |
| `"22"` | Run eligible Node action modules with the Node22 compatibility target; current default |
| `"24"` | Run eligible Node action modules with the Node24 compatibility target |

If no value is configured, Neovex uses Node22.

## Debug Node API Usage

Use the Node API diagnostics path when default-runtime modules accidentally
import Node built-ins, or when a package needs to be moved into the Node action
bundle:

```bash
neovex dev --once --debug-node-apis
neovex codegen --app . --debug-node-apis
```

Diagnostics should point to the importing module, explain whether `"use node"`
is missing, and avoid silently bundling unsupported Node-only code into the
default runtime.

## Specifier Rules

Neovex accepts both bare and `node:` forms for supported Node built-ins:

```ts
import fs from "fs";
import fsPromises from "node:fs/promises";
```

Specifier support does not imply full built-in compatibility. The supported
surface is bounded by the compatibility matrix and generated evidence.
