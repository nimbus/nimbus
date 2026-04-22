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
- starting Neovex with `--convex-app-dir` enables the Convex HTTP, WebSocket,
  scheduler, and runtime diagnostics routes

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
