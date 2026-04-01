# Convex Compatibility

Neovex includes an in-repo Convex compatibility surface built on the V8 runtime
in `crates/neovex-runtime`, the `packages/convex` package, and generated
artifacts from `packages/codegen`.

This surface is intentionally partial and evolving. It is designed to support a
useful subset cleanly rather than claim blanket Convex parity.

## What Exists Today

- `packages/convex` provides the in-repo `convex/browser`, `convex/react`,
  `convex/server`, and `convex/values` surfaces
- `packages/codegen` emits `.neovex/convex/functions.json`,
  `.neovex/convex/bundle.mjs`, and `.neovex/convex/bundle.sha256`
- the runtime verifies the bundle hash before every invocation
- starting Neovex with `--convex-app-dir` enables the Convex HTTP, WebSocket,
  scheduler, and runtime diagnostics routes

## Supported Areas

- generated refs for the supported declarative subset
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
- compiled `patch`, `delete`, and `get` flows currently require ids declared
  with `v.id("table")` so codegen can preserve the Convex call shape while
  lowering into Neovex plans
- `convex/react` and `convex/browser` automatically reconnect and resubscribe
  after dropped sockets
- unchanged subscription payloads are suppressed to reduce unnecessary rerenders
- `useQueries` keeps failures local as `Error` values, while `useQuery` and
  `usePaginatedQuery` still throw into React error boundaries
- runtime diagnostics are available at `GET /debug/runtime/metrics`

## Demo Entry Points

From the repo root:

```bash
npm run convex:server:html
npm run convex:demo:html
```

```bash
npm run convex:server:http
npm run convex:demo:http
```

See [demos/README.md](../../demos/README.md)
for the demo layout and run flow.
