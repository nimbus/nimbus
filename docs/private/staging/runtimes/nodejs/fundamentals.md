# Node.js Runtime Fundamentals

Nimbus Node.js support is an explicit compatibility contract for
functions-as-a-service and Convex-compatible `"use node"` actions. It is not a
claim that every Node CLI feature, process-wide API, native extension, or host
tooling workflow is available inside the production in-process runtime.

## Use `"use node"` Deliberately

Put `"use node"` at the top of an action module when that module needs Node
built-ins or staged Node packages:

```ts
"use node";

import { action } from "./_generated/server";
import { randomUUID } from "node:crypto";

export const issueId = action({
  args: {},
  handler: async () => randomUUID(),
});
```

Queries and mutations should stay in the default runtime. Shared code that does
not need Node APIs should live in separate modules so the runtime boundary stays
obvious.

## Select A Compatibility Target

Node24 is the product default. Product default is a routing default, not an
evidence priority. Node22 and Node24 are supported LTS targets with lane-local
evidence. Node26 is Current/non-LTS compatibility evidence and is not
enterprise LTS support until Node itself enters LTS and supported-LTS gates
pass. Node20 remains selectable only as legacy-grace regression coverage after
its 2026-04-30 EOL.

Configure the target in `convex.json`:

```json
{
  "node": {
    "nodeVersion": "24"
  }
}
```

See [configuration](configuration.md) for allowed values and diagnostics.

## Permissions Stay Separate

Selecting a Node target does not grant ambient host access. Filesystem,
network, environment, subprocess, secret, identity, service, FFI, worker, and
tool access still come from the active runtime mode and explicit grants.

Production in-process application profiles are optimized for deterministic
function invocation. Host-heavy behavior such as child processes, worker
threads, inspector, REPL, `node --test`, native addons, package-owned
binaries, raw server listeners, and persistent host filesystem assumptions must
use a service or microVM route when that behavior is required.

## Packages Are Evidence-Backed

Node packages are staged during codegen; runtime invocation does not install or
fetch dependencies. HTTP SDKs and Convex-compatible `"use node"` packages are
supported where the generated package reference shows passing Application
canaries on the supported LTS lanes.

Use [packages and bundling](packages-and-bundling.md) for configuration and
[reference/packages.md](reference/packages.md) for the generated package
support matrix.

## Read The Generated Contract

The public support contract is generated from checked-in manifests and
evidence:

- [compatibility](compatibility.md)
- [Node API reference](reference/node-apis.md)
- [package reference](reference/packages.md)
- [evidence summary](evidence/latest.md)

Those generated pages distinguish release phase, Nimbus support promise,
service/microVM routing, diagnostic canaries, and official fixture
classification. Treat expected failures, known gaps, skips, and diagnostic
denials as boundaries, not as positive in-process support.
