# Convex Compatibility

Neovex includes an in-repo Convex compatibility surface built on the V8 runtime
in `crates/neovex-runtime`, the `packages/convex` package, and generated
artifacts from `packages/codegen`.

This surface is intentionally partial and evolving. It is designed to support a
useful subset cleanly rather than claim blanket Convex parity.

This document covers the Convex-compatible authoring mode. Neovex also ships a
first-party native JS surface under `packages/neovex`; native apps may use a
`neovex/` source root, while upstream-style compatibility apps continue to use
`convex/`.

## What Exists Today

- `packages/convex` provides the in-repo `convex/browser`, `convex/react`,
  `convex/server`, and `convex/values` surfaces
- `packages/codegen` accepts `convex/` as the compatibility source root and
  `neovex/` as the native source root
- generated `_generated/*` files land under the selected source root and import
  from the matching package namespace (`convex/*` or `neovex/*`)
- `packages/codegen` emits `.neovex/convex/functions.json`,
  `.neovex/convex/bundle.mjs`, and `.neovex/convex/bundle.sha256`
- the runtime verifies the bundle hash before every invocation
- starting Neovex with `--app-dir` enables the Convex HTTP, WebSocket,
  scheduler, and runtime diagnostics routes

## Codegen Security Boundary

Codegen-time schema, server-definition, and resolver planning are handled by a
shared TypeScript AST interpreter with an explicit supported subset. That
compile-time path rejects unsafe global references and prototype-constructor
property access rather than executing user source in the Node.js codegen
process.

The generated runtime bundle still uses `new Function` to materialize
runtime-only handlers from the manifest. That use belongs to runtime execution:
the bundle is SHA-256 checked before invocation and runs inside the Neovex V8
runtime behind the `HostBridge` capability boundary. It is not part of
compile-time schema or planner extraction.

## Codegen And Migration Taste

For Convex-style projects, the shipped codegen entrypoints are now:

- `neovex codegen --app ./my-app`
- `npx convex codegen --app ./my-app`
- `npx neovex-codegen --app ./my-app`

All three invoke the same `@neovex/codegen` pipeline and produce the same
`_generated/*` plus `.neovex/convex/` outputs.

Generated files should still be checked into version control. That keeps
frontend typechecking and CI stable even when a developer has not run the CLI
yet.

For repo-owned JS verification, use the root workspace entrypoints:

- `npm run typecheck`
- `npm run test`
- `npm run build`

Those commands fan out to package-owned scripts across `@neovex/codegen`,
`convex`, and `neovex`. The `@neovex/codegen` typecheck lane is a JS syntax and
codegen-boundary guardrail check because that package is implemented as `.mjs`
rather than TypeScript.

`neovex start --app-dir ./my-app` now runs one codegen preflight pass before
startup unless `--skip-codegen` is set, but this is intentionally not a
replacement for Convex's watched `dev` loop. After the server starts, Neovex
does not watch source files or regenerate artifacts on later edits.

## Node Runtime Configuration

Neovex mirrors Convex's Node runtime selection shape for action modules:

```ts
"use node";

import fs from "node:fs";
import { action } from "./_generated/server";

export const read = action({
  args: {},
  handler: async () => fs.readFileSync("README.md", "utf8"),
});
```

Only actions may live in a `"use node"` file. Queries and mutations stay in the
default runtime and should move to a separate module if they share a file with
Node-specific code.

Project-level Node action version selection is read from `convex.json`:

```json
{
  "node": {
    "nodeVersion": "22"
  }
}
```

Supported values are `"20"`, `"22"`, and `"24"`. Neovex defaults to `"22"`
until a deliberate Node24-default migration. Node builtin imports are accepted
in both bare and `node:` forms, so `fs` and `node:fs` resolve to the same
runtime family. Use `neovex dev --once --debug-node-apis` or
`neovex codegen --app . --debug-node-apis` to diagnose default-runtime modules
that import Node builtins without `"use node"`.

`node.externalPackages` supports explicit package specifiers and Convex-style
`["*"]` for packages imported by `"use node"` action modules. Codegen validates
that each externalized package resolves from local `node_modules`, stages the
resolved package roots under `.neovex/convex/node_modules/`, materializes static
package imports into generated runtime bindings, and emits
`.neovex/convex/node_external_packages.json` with package roots, staged roots,
importers, size evidence, and Convex cloud limit references. Neovex records
those external-package limits but does not enforce the same zipped/unzipped
thresholds yet. Full Convex cloud-style dependency installation and
dependency-closure packaging are still narrower than Convex's cloud bundler, so
unexternalized npm package imports fail with a precise diagnostic instead of
being treated as silently bundled.

For the product-facing Node.js runtime guide, including selectable versions,
specifier rules, package staging, compatibility evidence, and current limits,
see `docs/runtimes/nodejs/`.

## Supported Areas

- generated refs for the supported declarative subset
- string-based named refs plus a first-slice `anyApi` proxy for query,
  mutation, action, scheduling, and standard live-query flows
- compiled `ctx.db.query(...)` flows including filter, index, ordering, and
  common result shapes such as `collect()`, `take()`, `first()`, and `unique()`
- compiled `ctx.db.get(id)` reads
- compiled `ctx.db.insert(...)`, `ctx.db.patch(...)`, and `ctx.db.delete(...)`
- compiled and runtime-backed `paginatedQuery` flows
- compiled `httpAction` routes through `httpRouter`
- generated and runtime-backed `ctx.runQuery(...)`, `ctx.runMutation(...)`, and
  `ctx.runAction(...)` composition
- Convex-compatible `"use node"` action modules with `convex.json`
  `node.nodeVersion` values `"20"`, `"22"`, and `"24"`
- Convex-compatible Node action `node.externalPackages` explicit and `["*"]`
  configuration backed by local package validation, generated staging, runtime
  bindings, and package evidence metadata
- scheduled Convex mutation execution through `runAfter` and `runAt`
- live query subscriptions over the Convex WebSocket transport
- runtime-backed dependency tracking that is narrower than plain table-level
  invalidation for supported read shapes

## Important Limits And Notes

- support is still partial; the compatibility surface should be treated as a
  supported subset, not full Convex parity
- `convex/` remains the compatibility-oriented user source root, while
  `neovex/` is the first-party native root; both still converge on the same
  internal runtime artifacts under `.neovex/convex/`
- compiled `patch`, `delete`, and `get` flows currently require ids declared
  with `v.id("table")` so codegen can preserve the Convex call shape while
  lowering into Neovex plans
- `convex/react` and `convex/browser` automatically reconnect and resubscribe
  after dropped sockets
- string refs and `anyApi` do not yet cover paginated live-query flows; use
  generated refs or `makePaginatedQueryReference(...)` there
- unchanged subscription payloads are suppressed to reduce unnecessary rerenders
- `useQueries` keeps failures local as `Error` values, while `useQuery` and
  `usePaginatedQuery` still throw into React error boundaries
- runtime diagnostics are available at `GET /debug/runtime/metrics`

## Differential Verification

The supported Convex subset now has a first external differential harness in
`packages/convex/src/differential.mjs`.

This first slice is intentionally narrow and explicit:

- mutations
- queries
- paginated queries
- subscriptions

The shared fixture app lives in
`packages/convex/fixtures/differential_app/convex/` and defines one stable
messages workload used for both targets.

The runner compares:

- Neovex plus the in-repo `packages/convex` client
- an external Convex deployment plus the official Convex browser client source

Result normalization is limited to documented transport-shape differences such
as pagination envelope fields. Unsupported surfaces are rejected explicitly so
the differential contract stays aligned with the documented supported subset.
The runner now compares named semantic slices independently and reports every
mismatch it finds in one pass instead of stopping at the first structural diff.

From the repo root:

```bash
npm run test:differential --workspace convex -- --neovex-only
```

To export the shared fixture app for external provisioning:

```bash
npm run test:differential --workspace convex -- --emit-fixture-dir /tmp/neovex-convex-differential-app
```

To run the full external comparison after provisioning that fixture app on a
Convex deployment:

```bash
CONVEX_SELF_HOSTED_URL=http://127.0.0.1:3210 \
npm run test:differential --workspace convex -- --require-external
```

If a nearby `convex-backend` checkout is available, the runner can also start
an official local Convex deployment automatically and compare against that
local oracle directly:

```bash
npm run test:differential --workspace convex -- --require-external
```

If the official Convex browser source is not available in a nearby
`convex-backend` checkout, set
`NEOVEX_CONVEX_DIFF_OFFICIAL_BROWSER_ENTRY=/absolute/path/to/npm-packages/convex/src/browser/index.ts`.

Scheduling and supported auth-shape differential cases are planned follow-on
coverage for this suite rather than part of the first landed slice.

## Demo Entry Points

From the repo root:

```bash
npm run convex:server:node
npm run convex:demo:node
```

```bash
npm run convex:server:html
npm run convex:demo:html
```

```bash
npm run convex:server:http
npm run convex:demo:http
```

For the plain HTML bundle variant:

```bash
npm run build --workspace convex
```

Then open
[http://localhost:8080/demos/convex/html/vanilla.html](http://localhost:8080/demos/convex/html/vanilla.html).

See [demos/README.md](../../demos/README.md)
for the demo layout and run flow.
