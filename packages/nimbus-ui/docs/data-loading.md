# Data loading in the console

One contract for reading data, so a reader never has to guess which of three
patterns a given surface uses. Pick by how the data is served, not by habit.

## The default: reactive `useQuery`

Everything backed by the engine's live query surface uses
`useQuery(api.x.y, args)` from `@nimbus/nimbus/react`. Gate a query that is not
yet ready to run with the `"skip"` sentinel instead of a conditional hook:

```ts
const tables = useQuery(
  api.tables.list,
  tenant ? { tenantId: tenant, limit: 200 } : "skip",
);
```

`undefined` means "still loading"; a value means "loaded and live". This is the
first choice for any list, detail, or count the server can push updates for.

## Route `loader:` — only for preload-before-paint

A TanStack route `loader:` is for the handful of routes that must have their
data resolved *before* the component paints (no loading flash on entry), and
that refetch with `router.invalidate()`. Keep the existing six; do not add a
loader just to fetch on mount. Loaders return the discriminated
`{ kind: "ok" | "error" }` result the routes already share.

## One-shot HTTP reads: `useApiRead`

For a plain non-reactive `GET` (call graph, module source, `/debug/*`
diagnostics) use `useApiRead<T>(path, deps)` from `hooks/use-api-read`. It runs
through the shared `apiFetch` core, cancels the in-flight read on unmount or a
`deps` change, and returns a `LoadingValue<T>`:

```ts
const graph = useApiRead<GraphData>("/api/console/graph", []);
```

When a call site needs to fold a specific status into a typed value — e.g. the
Source tab treating `404` as a "missing" variant — pass a `select` that maps
the `ApiResult` to a `LoadingValue<T>`. Extend the value per call site; do not
invent a new state union.

## Loading vocabulary: `LoadingValue<T>`

`LoadingValue<T>` (`shell/loading-value`) plus `LoadingCell` is the one loading
vocabulary. Branch on `.kind` (`"loading" | "ok" | "offline" | "error"`) and
read `.value` on the `ok` arm. The older ad-hoc `{ status }` / `{ kind }` /
`AsyncSnapshot<T>` unions are legacy — do not add new ones.
