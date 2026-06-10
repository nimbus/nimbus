# Before-state cross-reference

This wave's `before/` is the predecessor's sealed `after/` bundle:

`docs/plans/proof/desktop-ui-followup-hardening/after/` — 11 captures
with `h7-` prefix, sealed at 2026-05-18 plan closure.

No PNG duplication: the predecessor's after-state IS this wave's
before-state. The mapping below records what each predecessor capture
is the before-shot for in this wave.

## Cast inventory at A0 (re-grepped 2026-05-18)

14 cast sites; matches the plan promotion-time count exactly.

| File | Line | Cast |
|------|------|------|
| `packages/nimbus-ui/src/routes/admin/index.tsx` | 46 | `as ServiceDoc[] \| undefined` |
| `packages/nimbus-ui/src/routes/admin/machines.tsx` | 455 | `as ServiceDoc[] \| undefined` |
| `packages/nimbus-ui/src/routes/admin/services_.$service.tsx` | 68 | `id: serviceId as never` |
| `packages/nimbus-ui/src/routes/admin/services_.$service.tsx` | 69 | `as ServiceDoc \| null \| undefined` |
| `packages/nimbus-ui/src/routes/admin/services_.$service.tsx` | 76 | `as ServiceDoc[] \| undefined` |
| `packages/nimbus-ui/src/routes/admin/services.tsx` | 25 | `as ServiceDoc[] \| undefined` |
| `packages/nimbus-ui/src/routes/app/compute_.runs_.$runId.tsx` | 44 | `id: runId as never` |
| `packages/nimbus-ui/src/routes/app/services_.$service.tsx` | 70 | `id: serviceId as never` |
| `packages/nimbus-ui/src/routes/app/services_.$service.tsx` | 71 | `as ServiceDoc \| null \| undefined` |
| `packages/nimbus-ui/src/routes/app/services_.$service.tsx` | 78 | `as ServiceDoc[] \| undefined` |
| `packages/nimbus-ui/src/routes/app/services.tsx` | 41 | `as ServiceDoc[] \| undefined` |
| `packages/nimbus-ui/src/shell/primary-drawer.tsx` | 107 | `entry.countQuery as never` |
| `packages/nimbus-ui/src/shell/primary-drawer.tsx` | 108 | `(entry.countArgs ?? undefined) as never` |
| `packages/nimbus-ui/src/shell/tenant-selector.tsx` | 193 | `search: { create: 1 } as never` |

Breakdown by root cause (drives A1 implementation):

- **5 sites are codegen-discarded return type** (`as ServiceDoc[]`,
  `as ServiceDoc | null | undefined`). The codegen *has* return-type
  inference (`inferFunctionResultType` in
  `packages/codegen/src/emit/type_inference.mjs`) but every handler
  declares `returns: v.array(v.any())` or `returns: v.any()`, which
  short-circuits to `JsonValue` via `renderValidatorType` in
  `schema_types.mjs:37-38`. Fix lives in the codegen: when the
  return validator is trivial (`v.any()` or `v.array(v.any())`),
  fall through to plan-based inference (`inferQueryResultType`),
  which already produces `Doc<"services">[]` from the registered
  query plan.
- **3 sites are `Id<T>` branded-string mismatch** (`id: serviceId as never`,
  `id: runId as never`). `Route.useParams()` returns `string`; the
  codegen-emitted `Args` type declares `id: Id<"services">`. Fix is a
  narrower cast — `as Id<"services">` — at each consumer; the brand
  is correct because TanStack Router's typed params already validate
  the path segment.
- **1 site is heterogeneous query-registry typing**
  (`entry.countQuery as never` + `entry.countArgs as never` in
  `primary-drawer.tsx`). The `NavEntry` registry holds
  `{ countQuery: QueryRef<unknown, unknown[]> | undefined,
  countArgs: unknown }`. Fix is a `QueryEntry<Args, Return>`
  discriminated wrapper at the producer side.
- **1 site is TanStack Router search-param shape**
  (`search: { create: 1 } as never` in `tenant-selector.tsx`). The
  `/admin/tenants` route either lacks `create` in its
  `validateSearch` or the navigation path is unnecessary. Fix is to
  add `create?: number` to the tenants-route search schema.

## File-size inventory at A0 (re-grepped 2026-05-18)

| File | LOC | Threshold band |
|------|-----|----------------|
| `routes/admin/settings.tsx` | 1608 | **warning (1500-1999)** — A3 target |
| `routes/app/storage_.$table.tsx` | 1154 | under threshold — A7 audit |
| `routes/app/observability.tsx` | 978 | under threshold — A7 audit |
| `routes/admin/machines.tsx` | 739 | under threshold — A7 audit |
| `routes/app/index.tsx` | 545 | under threshold |
| `routes/admin/index.tsx` | 193 | under threshold |

## Cross-persona type-leak inventory at A0 (drives A2)

| Importer | Imported symbol | Source |
|----------|-----------------|--------|
| `routes/admin/services.tsx:13` | `type ServiceDoc, ServicesTable` | `../app/services` |
| `routes/admin/services_.$service.tsx:21` | `type ServiceDoc` | `../app/services` |
| `routes/admin/index.tsx:27` | local re-declaration | `type ServiceDoc = { _id: string }` |

The duplicate local declaration at `admin/index.tsx:27` is the
canary — drift already started.

## TanStack Router data-loading inventory at A0 (drives A4)

- 2 routes use `beforeLoad`: `routes/index.tsx:4`,
  `routes/admin/network.tsx:30`.
- 0 routes use `loader`.
- 56 `useQuery(api...)` call sites across the SPA.
- Router is configured at `src/main.tsx:10` with
  `defaultPreload: "intent"` but no router-level `context`. A4 will
  need to pass the nimbus client through `context` so loaders can
  call `client.query(api.foo.bar, args)` without a React render.

## Predecessor → this-wave before-mapping

| Predecessor capture (`h7-` prefix) | This wave's before-state of |
|-----------------------------------|------------------------------|
| `h7-app-overview-tile-states.png` | A4 loader migration target (Overview is in scope only if it migrates cleanly; primary A4 targets are services + tenants) |
| `h7-app-services-scope-chip.png` | A1 + A2 — `/app/services` post-codegen typing and post-`ServiceDoc` lift |
| `h7-app-storage-breadcrumb.png` | A7 large-file audit candidate (`storage_.$table.tsx` 1154 LOC) |
| `h7-admin-overview-tile-states.png` | A1 — `/admin` post-codegen typing (`as ServiceDoc[]` cast at index.tsx:46) |
| `h7-admin-tenants-404-envelope.png` | A4 loader migration — diagnostic envelope becomes route-level error |
| `h7-admin-network-default-section.png` | Unchanged this wave (predecessor closed this gap) |
| `h7-admin-services-detail.png` | A1 + A2 + A4 — service-detail post-codegen, post-`ServiceDoc` lift, post-loader migration |
| `h7-observability-disabled-chip.png` | A7 audit candidate (`observability.tsx` 978 LOC) |
| `h7-cmdk-modes-and-scroll.png` | Unchanged this wave |
| `h7-lens-separator.png` | Unchanged this wave |
| `h7-status-bar-tenant.png` | Unchanged this wave |
