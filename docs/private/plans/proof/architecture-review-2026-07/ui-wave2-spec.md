# UI Wave-2 Spec — UI1 (storage table split), UI2 (mutation client), UI6 (data-loading contract)

Design authority: `docs/private/plans/architecture-review-2026-07-plan.md`
Band UI rows UI1/UI2/UI6 + the 2026-07-07 UI-lane inventory. Package scope:
`packages/nimbus-ui` only. Sequencing: REQUIRES the wave-1 `ui-hygiene`
branch (UI3/UI4/UI5/UI7) merged first — this spec builds on its extracted
`Slideover`/`JsonEditorForm`, shared doc types, and Empty/Loading
standardization. Rebase and read those files before starting.

## Facts this rests on (2026-07-07 inventory)

- `src/routes/developer/storage_.$table.tsx` is 1,154 lines: one 548-line
  page component + 9 inline sub-components; NO `components/storage/`
  exists. Ten state slices; slices 1–5 (`page`, `loading`, `pageError`,
  `cursorStack`, `refreshTick`) plus `loadPage` (:91–140, POST
  `/api/tenants/{t}/query/paginated`, `PAGE_SIZE=25`), `reset`/`onNext`/
  `onPrev` (:147–163) form a self-contained pagination engine. Slices
  6–10 (`selected`, `showInsert`, `editing`, `confirmDelete`,
  `deletingDocs`) are page concerns. `tableMeta` is the file's only
  reactive read (`useQuery(api.tables.byName, ...)`).
- The file hand-rolls what shared primitives already provide: raw
  `<th>/<td>` instead of `data-table.tsx` `Th`/`Td`; a hand-rolled
  `<header>` (:298) instead of `PageHeader`; local `Empty`/`Loading`
  (wave-1 UI3 removes these).
- `lib/` has NO write client. Eleven hand-rolled mutation `fetch` sites
  repeat the same `!response.ok` → parse `body.error.message` → throw
  boilerplate: storage documents insert/update/delete (:176/:210/:244),
  schema PUT/DELETE (:857/:888), machines action/delete
  (`operator/machines.tsx:137`, `credentials:"same-origin"` outlier),
  token rotate + shutdown (`danger-zone.tsx:71/:193`; rotate is the only
  Authorization-header site), tenant create/delete
  (`operator/tenants.tsx:111/:146`). `function-runner.tsx:100` is an
  invocation POST with a richer `{code,message,remediation,requestId}`
  envelope — different semantics.
- Three data-loading patterns coexist: reactive `useQuery` (~40 sites,
  idiomatic), TanStack route `loader:` (6 routes, discriminated
  `{kind:"ok"|"error"}`, refetch via `router.invalidate()`), and
  `useEffect`+fetch outliers (`storage_.$table.tsx:143`,
  `graph-view.tsx:27`, `compute_.$function.tsx:348` SourceTab,
  `function-runner.tsx:382`, `settings/hooks.ts` ×3). THREE loading-state
  vocabularies: `{kind}`, `{status}`, and `LoadingValue<T>`
  (`shell/loading-value.ts` + `components/loading-cell.tsx`).
- THREE parallel tenant-list fetchers: `shell/tenants-fetch.ts`,
  `hooks/use-tenant-list.ts`, and an inline copy in
  `function-runner.tsx:382`.
- Conventions: Tailwind v4 semantic tokens (`bg-surface`, `text-muted`,
  …), `cn()` from `lib/cn.ts`, lucide icons, sonner toasts, heavy
  `data-testid`, colocated `*.spec.tsx` under vitest+happy-dom+Testing
  Library, coverage includes `lib/** components/** store/**` (not
  routes) — which is exactly where this spec moves logic.

## Target design (normative)

### UI2 — typed mutation client (do this FIRST; UI1 consumes it)

1. `lib/api-mutations.ts` (or `lib/api/` if it grows past ~300 lines):
   one private `apiFetch(path, init)` core — root-relative path,
   `credentials:"include"`, JSON content-type, response parse; returns
   `Promise<ApiResult<T>>` where
   `type ApiResult<T> = { ok: true; data: T } | { ok: false; error: string }`.
   Non-JSON and network failures map to `ok:false` with a readable
   message. NO exceptions for expected failures — callers branch on
   `ok`.
2. Domain-grouped typed helpers over that core, matching today's
   endpoints exactly: documents (insert/update/delete),
   schema (put/drop), machines (action/delete — fold the
   `same-origin` outlier into `include` unless a test proves the
   distinction matters; if it does, keep and comment why), tenants
   (create/delete), system (rotate — accepts a bearer token argument —
   and shutdown). Route ALL 11 write sites through these helpers;
   delete the inline boilerplate.
3. `function-runner.tsx`'s invocation POST is OUT OF SCOPE (richer
   envelope, invocation semantics) — do not fold it in.
4. Tenant-list dedup rides along: `hooks/use-tenant-list.ts` becomes the
   single tenant-list read; `shell/tenants-fetch.ts` and the
   `function-runner.tsx` inline copy are deleted/re-pointed.

### UI1 — storage-table decomposition

1. `hooks/use-table-documents.ts`: state slices 1–5 + `loadPage`/
   `reset`/`onNext`/`onPrev`/`refresh`, built ON the UI2 client
   (`loadPage` becomes a typed paginated-query call — note: it is a
   POST read; put it beside the mutation helpers as the one typed
   query). Signature: `useTableDocuments(tenant, table)` returning
   `{page, loading, pageError, cursorStack, refresh, onNext, onPrev, reset}`.
2. `components/storage/`: move `CellValue`, `InsertDrawer`, `EditDrawer`,
   `SchemaPanel`, `IndexPanel`, `PageError` there as props-driven
   components (wave-1's `Slideover`/`JsonEditorForm` replace the local
   `Drawer`; `PanelHeader` should already be gone or fold into
   `Slideover`'s header). `SchemaPanel`'s two write sites go through the
   UI2 schema helpers.
3. The route file keeps: route decl, `validateSearch`, the page
   component with slices 6–10, composition of the extracted pieces. Use
   shared `Th`/`Td` and `PageHeader`. Target: route file under ~400
   lines, nothing hand-rolled that a shared primitive provides.
4. Local types were moved by wave-1 UI5 — import from there; do not
   re-declare.

### UI6 — one data-loading contract

1. Write `packages/nimbus-ui/docs/data-loading.md` (~half page,
   normative): DEFAULT is reactive `useQuery` with `"skip"`; route
   `loader:` ONLY where preload-before-paint matters (keep the existing
   6); one-shot HTTP reads use a small `useApiRead` hook (below); the
   loading-state vocabulary is `LoadingValue<T>` + `LoadingCell` — the
   `{kind}`/`{status}` unions are legacy.
2. `hooks/use-api-read.ts`: `useApiRead<T>(path, deps)` over the UI2
   `apiFetch` core returning `LoadingValue<T>`, with cancellation on
   unmount (the `graph-view.tsx` `cancelled` flag generalized).
3. Migrate the outliers onto it: `graph-view.tsx`,
   `compute_.$function.tsx` SourceTab (its 404→"missing" state maps to
   a typed variant — extend `LoadingValue` per-call-site via a mapped
   value, not a fourth vocabulary), `settings/hooks.ts` ×3 (their
   `AsyncSnapshot` dies), `function-runner.tsx` tenant list (dies into
   the UI2 dedup). `storage_.$table.tsx:143` is owned by UI1's hook —
   don't double-migrate.

## Hard constraints

- User-facing surface: match existing idiom exactly (semantic tokens,
  `cn()`, `data-testid` on interactive elements, sonner for toasts).
- No behavior changes: same endpoints, same bodies, same toasts, same
  partial-failure semantics (the per-id delete tally in `runDelete`
  keeps its partial-success toast).
- No new dependencies (NO react-query — the reactive client + hooks
  cover it).
- Extracted lib/components/hooks land INSIDE the vitest coverage
  include set — they must arrive with specs (`*.spec.ts[x]`), not bare.
- Do not touch `convex/_generated`, the route-tree codegen, or biome
  config. NEVER run `biome check --write src`.

## Required tests (behavior-asserting)

1. `api-mutations`: MSW-backed specs per domain helper — success maps to
   `{ok:true,data}`; HTTP error with `body.error.message` maps to
   `{ok:false,error:msg}`; non-JSON body still yields readable error;
   rotate sends the bearer header.
2. `use-table-documents`: page load, next/prev cursor-stack semantics,
   refresh tick, error path (renderHook + MSW).
3. `use-api-read`: success, error, unmount-cancellation (no state
   update after unmount).
4. Component specs for `CellValue` (id chip, JSON fallback) and
   `SchemaPanel` (save → helper called, drop → confirm flow) at minimum.
5. Existing route-level specs keep passing unmodified except import
   paths.

## Verification gates (worktree root, report real counts)

```
npm install                       # fresh worktree
npm run typecheck
node_modules/.bin/vitest run --root packages/nimbus-ui --reporter=dot < /dev/null
npm run build
```

Plus `npm run lint:capability-boundary` if it covers nimbus-ui. Update
the plan ledger rows UI1/UI2/UI6 with evidence on completion.
