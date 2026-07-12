# Convex examples — internal parity notes

Internal notes on the Convex compiled/runtime subset the Convex examples in this
directory exercise. This is contributor status, not user-facing documentation —
it tracks which Convex authoring shapes Nimbus supports today so the example apps
stay within the verified surface. Final placement of these notes is owned by a
later cleanup band; they were moved out of the user-facing `examples/README.md`.

## Compiled / runtime subset exercised

- compiled `ctx.db.patch(...)` and `ctx.db.delete(...)` are supported when the handler arg uses `v.id("table")`
- compiled `ctx.db.get(id)` is supported when the handler arg uses `v.id("table")`
- compiled `httpAction` routes are supported for the 4B declarative subset through `httpRouter`, `convex/http.ts`, request placeholders, and the tenant-scoped convex HTTP transport
- compiled `ctx.db.query(...).filter(...)` is supported for declarative filter chains
- compiled `ctx.db.query(...).first()` is supported for single-document query results
- compiled `ctx.db.query(...).unique()` is supported and returns an error when multiple documents match
- mixed `ctx.db.query(...).withIndex(...).filter(...).unique()` plans are supported for exact indexed lookups with residual filters
- runtime-only named query/mutation/action handlers execute through the V8 bundle path
- runtime-only named `paginatedQuery` handlers execute through that bundle path when they return a live query builder
- named `paginatedQuery` refs work with the convex WebSocket path, so `usePaginatedQuery` can refresh its loaded window after live invalidations
- compiled `paginatedQuery` handlers can return `ctx.db.query(...)` directly, which is a closer match to natural Convex authoring
- the convex browser client suppresses unchanged subscription payloads so React apps do not rerender on no-op invalidations
- reconnect/resubscribe also suppresses an unchanged initial replay payload, which avoids extra rerenders after transient socket drops
- generated `convex/_generated/api.ts` refs carry typed args and common inferred result shapes, so the apps can lean on inference instead of manual casts
- generated action refs infer common delegated return shapes too, so example actions that call generated queries or mutations often do not need explicit `returns`
- `convex/react` masks stale values and stale errors across arg changes and `"skip"` transitions, so hook loading/error behavior is close to Convex
- `useQueries` keeps failures local as `Error` values, while `useQuery` and `usePaginatedQuery` throw into React error boundaries
- the React example's error-boundary panel recovers automatically when the live underlying data stops violating `unique()`

## Upstream shapes these examples adapt

- Official Convex demos: <https://github.com/get-convex/convex-demos>
- Convex backend: <https://github.com/get-convex/convex-backend>

These are Nimbus ports and adaptations, not the official Convex demos running unchanged.
