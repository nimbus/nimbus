# Desktop UI — Architecture Hardening

Status: active
Owner: desktop-ui workstream
Predecessor (closed, archived): `docs/plans/archive/desktop-ui-followup-hardening-plan.md`
Source: 2026-05-18 post-closure critical-reflection review of the Followup-Hardening wave (H0–H7). The wave closed cleanly on its own scope, but the review surfaced architecture debt that the wave intentionally did not touch.
Promoted: 2026-05-18

Related current references:

- `CLAUDE.md` "Modularity thresholds" (1500–1999 warning band, ≥2000 must
  decompose) — drives phase A3 and the A7 audit.
- `DESIGN.md` (canonical operator-console design system — left unchanged
  by this plan; the dual-persona Services ratification landed in the
  predecessor's H1).
- `docs/plans/archive/desktop-ui-followup-hardening-plan.md` and its
  proof bundle `docs/plans/proof/desktop-ui-followup-hardening/`
  (this wave's before-state).
- `packages/codegen/` (the JS codegen tool that emits
  `packages/nimbus-ui/convex/_generated/api.ts`; phase A1 amends the
  emit, not the consumers' shape).
- `docs/adapters/convex/ai-guidelines.md` (Convex API guidance for A1).

## Why this plan exists

The Followup-Hardening wave landed three load-bearing wins on the
type-safety front: `TenantScope` discriminated union, `LoadingValue<T>`
envelope, and `SubDrawerItem<TId>` generic. It also extracted shared
helpers (`fetchTenants`, `ROUTE_FILE_IGNORE_PATTERN`), backfilled spec
coverage on the abort-controller path and section-nav narrowing, and
brought the proof bundle up to date. But the wave deliberately deferred
six items the review flagged as architectural rather than visible-debt:

1. **CRITICAL — Convex codegen returns `JsonValue` for every query
   result.** Five `as never` casts (`shell/tenant-selector.tsx:193`,
   `shell/primary-drawer.tsx:107-108`, `routes/app/compute_.runs_.$runId.tsx:44`,
   `routes/app/services_.$service.tsx:70`,
   `routes/admin/services_.$service.tsx:68`) and five
   `as ServiceDoc[]` casts (`routes/app/services.tsx:41`,
   `routes/app/services_.$service.tsx:78`,
   `routes/admin/services.tsx:25`, `routes/admin/index.tsx:46`,
   `routes/admin/machines.tsx:455`, `routes/admin/services_.$service.tsx:76`)
   exist because `makeQueryReference<Args, JsonValue>` makes the
   compiler see every result as `JsonValue` and every id-arg as
   `unknown`. The casts hide a real seam: the codegen knows the
   handler's argument schema (it emits the `Args` shape) but discards
   the return shape. Every consumer paying the cost in `as` casts is
   a symptom; the fix lives in the codegen. This is the highest-
   leverage move because it removes every cast in one place and any
   future codegen-typed route inherits the fix.

2. **HIGH — `ServiceDoc` cross-persona type leak.** `routes/admin/*`
   imports `ServiceDoc` from `routes/app/services` (see
   `routes/admin/services.tsx:13`, `routes/admin/services_.$service.tsx:21`).
   `routes/admin/index.tsx:27` independently re-declares a local
   `type ServiceDoc = { _id: string }` to avoid the leak — drift
   already started. Both consoles render the same Service entity;
   the type belongs to neither console. Lift it to a shared module
   (`lib/types/`) and the import direction flattens.

3. **HIGH — `routes/admin/settings.tsx` is 1608 LOC.** That sits in
   the CLAUDE.md 1500–1999 warning band and requires an explicit
   justification in the owning plan if it remains unsplit. The file
   has at least six distinct ownership stories (identity,
   appearance, encryption, license, system status, adapter
   capabilities) — these are concept-owned children, not arbitrary
   slicing.

4. **MEDIUM — No router-level data loading.** TanStack Router exposes
   `loader` and `beforeLoad` precisely so route components don't have
   to be the data-fetching unit. Today every data route does its own
   `useQuery` inside the component. There are 56 `useQuery(api...)`
   call sites across the SPA; many would migrate cleanly to loaders,
   which gives parallel fetch + suspense + 404 routing for free. This
   is medium because the current pattern works; it bites later when
   we add per-route loading skeletons or transitions.

5. **MEDIUM — No component catalog.** Eleven reusable components
   (`StateChip`, `StateDot`, `EmptyState`, `CopyChip`, `LoadingCell`,
   `RelativeTime`, `Uptime`, `Breadcrumb`, `UpgradePopover`,
   `AppearanceSection`, `SubDrawer*`) are exercised across 20+ route
   files but have no isolated catalog. Storybook (or a lightweight
   alternative like Ladle) gives us a fixture-driven surface for
   visual regression and design-review without spinning the dev
   server with a real tenant.

6. **MEDIUM — No CI browser-smoke harness.** chrome-devtools-mcp is
   one-shot at plan close. A persistent harness (playwright-cli on a
   CI lane, or a tiny Make target wrapping a deterministic walk)
   catches regressions like the `/admin/services/$service` evidence
   gap, the duplicate `ServiceDoc` declaration, or the
   `Operator-only` blurb in `DESIGN.md` before they ship.

Plus two LOW-priority items recorded for completeness:

7. **LOW — Large-file audit on `routes/app/storage_.$table.tsx`
   (1154), `routes/app/observability.tsx` (978),
   `routes/admin/machines.tsx` (739).** All under the CLAUDE.md
   threshold today but trending; surface ownership review while we're
   in the area for A3.

8. **LOW — Re-evaluate the predecessor's deferrals.** F15
   theme-matrix smoke (Light/Dark/System × Blue/Mono/Warm) and the
   F6 restoration of service-detail Restarts/Density/Drift tabs both
   still depend on owning plans that haven't landed. Optional pull-in
   only if the dependency clears mid-wave; otherwise restate the
   deferral with named owning plans.

Pre-launch policy applies: prefer breaking changes; no compat shims;
no feature flags for legacy behavior. The codegen emit shape changes,
the `ServiceDoc` import path changes, the settings file splits — every
consumer migrates in the same commit.

## Outcome

After this plan:

- `grep -rn 'as never' packages/nimbus-ui/src` returns zero hits.
  Every `useQuery(api.<table>.<handler>, args)` returns the
  table's `Doc` shape directly. The codegen `api.ts` declares the
  return type per-handler, not `JsonValue`.
- `grep -rn 'as ServiceDoc\[\] | undefined' packages/nimbus-ui/src`
  returns zero hits. `ServiceDoc` lives at
  `packages/nimbus-ui/src/lib/types/service.ts`. The local
  re-declaration in `routes/admin/index.tsx` is gone. Every
  `routes/admin/*` file that needs the type imports it from
  `lib/types/`, never from `routes/app/services`.
- `routes/admin/settings.tsx` is below the CLAUDE.md warning band
  (target: ≤900 LOC root + concept-owned children). The body that
  remains is a thin composition root: sub-drawer wire-up, route-level
  state, child render. Identity, appearance, encryption, license,
  system status, and adapter-capabilities each live in their own
  file under `routes/admin/settings/` (or a concept-owned sibling
  layout if TanStack Router file-routing forbids the nested form).
- TanStack Router `loader` lands on the routes where the migration is
  clean: at least `/admin/tenants`, `/admin/services`,
  `/admin/services/$service`, `/app/services`,
  `/app/services/$service`. Page components receive route data from
  `Route.useLoaderData()` instead of calling `useQuery` themselves.
  Loading and error states are owned by the route, not the
  component.
- A component catalog ships under `packages/nimbus-ui/.storybook/` (or
  Ladle equivalent) with stories for the eleven reusable components.
  `npm run catalog` (or the picked vehicle's command) renders the
  catalog locally; `npm run catalog -- --build` produces a static
  bundle.
- A CI browser-smoke harness runs under
  `make verify-desktop-ui` (or equivalent). The walk hits the
  Developer Overview, Operator System, Services on both personas,
  and the disabled-tab affordance; the run asserts zero
  `console.error`/`console.warn` (allowing the named `/api/tenants`
  404 only in the auth-less capture context). The harness is wired
  to a GitHub Actions lane.
- `routes/app/storage_.$table.tsx`, `routes/app/observability.tsx`,
  and `routes/admin/machines.tsx` either drop below 700 LOC each or
  receive an explicit per-file justification in this plan's
  execution log (per CLAUDE.md threshold guidance).
- F15 and F6-restore deferrals are restated with named owning plans,
  or pulled in if their dependencies cleared mid-wave.

## Out of scope

- **Server-side schema changes.** Phase A1 amends the codegen emit
  to read the *registered* return shape from
  `packages/codegen/` — it does not change the server-side
  function signature surface. If a function's registered return
  shape is genuinely `unknown`/`JsonValue` because the handler
  returns dynamic data, the codegen falls back to `JsonValue` and the
  consumer keeps a narrower local type as today; the goal is to
  remove the cast for the 90% of handlers that do declare a shape.
- **Migrating every `useQuery` call site to a loader.** Phase A4 is
  bounded: data routes with one or two queries and a route-owned
  loading state. Pages that fan out into many parallel queries or
  that intentionally render before all data arrives stay on inline
  `useQuery`.
- **Choosing a runtime UI testing library beyond playwright-cli or
  chrome-devtools-mcp.** Phase A6 picks one of those two (cost
  constraint per `MEMORY.md`); it does not introduce
  `@playwright/mcp` or a third runner.
- **Storybook addon stack beyond the basics.** Phase A5 ships the
  catalog and stories for the eleven reusable components. Visual
  diffing tools (Chromatic, Percy), interaction tests, and a11y
  scanners are explicit follow-on work — they get a named owning
  plan, not this one.
- **`EventDoc.tenantId` backend surfacing.** Inherited from the
  predecessor's H2(b) "Out of scope" — still belongs to a backend
  plan.
- **F15 theme-matrix smoke** and **F6 restoration of admin
  service-detail Restarts/Density/Drift tabs.** Both are tracked
  under phase A8 only for closure-state restatement; the actual
  work continues to live with their owning plans.

## Phase status ledger

| Phase | Slice | Status |
|-------|-------|--------|
| A0 | Read-in + before-state confirmation | pending |
| A1 | Convex codegen typing (CRITICAL) | pending |
| A2 | Lift `ServiceDoc` + cross-persona type-leak audit (HIGH) | pending |
| A3 | Decompose `routes/admin/settings.tsx` (HIGH) | pending |
| A4 | Router-level loaders for data routes (MEDIUM) | pending |
| A5 | Component catalog (MEDIUM) | pending |
| A6 | CI browser-smoke harness (MEDIUM) | pending |
| A7 | Large-file audit pass (LOW) | pending |
| A8 | Verification + close + archive | pending |

## Roadmap detail

### A0 — Read-in + before-state confirmation

Goal: orient against the predecessor's after-state, confirm the cast
inventory hasn't drifted, and seed the proof directory.

Touch list (reads only):

- `docs/plans/archive/desktop-ui-followup-hardening-plan.md` end-to-end.
- `docs/plans/proof/desktop-ui-followup-hardening/README.md` and
  `after/` walk-through (this wave's before-state).
- `packages/codegen/src/` (whatever entrypoint emits `api.ts`).
- `packages/nimbus-ui/convex/_generated/api.ts` (the current emit).
- `packages/nimbus-ui/convex/_generated/dataModel.d.ts` if it exists,
  otherwise note its absence — A1 may need to emit it.
- The five `as never` sites and six `as ServiceDoc[] | undefined`
  sites listed under "Why" §1.
- `packages/nimbus-ui/src/routes/admin/settings.tsx` end-to-end (1608
  LOC) — note section boundaries by heading and sub-drawer item id.
- `packages/nimbus-ui/src/routes/admin/index.tsx:27` (the duplicate
  local `ServiceDoc`).
- One example of each TanStack Router pattern already in use:
  `routes/index.tsx:4` (`beforeLoad`) and `routes/admin/network.tsx:30`
  (`beforeLoad` with search params).

Done when:

- `docs/plans/proof/desktop-ui-architecture-hardening/` directory
  exists with a `before.md` that cross-references the predecessor's
  `after/` directory by path (no PNG copying — the predecessor's
  after-state IS this wave's before-state).
- Cast inventory re-grepped at A0 start: any drift from the counts
  above (5/5/6) is recorded in execution log entry (a).
- Scope confirmed: zero edits this phase.

### A1 — Convex codegen typing (CRITICAL)

Goal: emit per-handler return types from the codegen so consumers
can drop the `as` casts at the source.

Background: `packages/nimbus-ui/convex/_generated/api.ts` currently
emits:

```ts
list: makeQueryReference<{
  "tenantId": string | null;
  "machineId": string | null;
  "state": string | null;
  "limit": number | null;
}, (JsonValue)[]>("services:list", "public")
```

The second type parameter is the return shape, hard-coded to
`JsonValue` (or `(JsonValue)[]` for list-returning handlers). The
codegen has the registered handler signature available — that is
where the `Args` shape comes from. The same source can produce the
return shape.

Touch list:

- `packages/codegen/` — locate the emit step that produces the
  `makeQueryReference<Args, Return>` line. Change the return
  parameter from a hard-coded `JsonValue`/`(JsonValue)[]` to the
  shape derived from the handler's registered return validator. If
  a handler has no registered return validator, fall back to
  `JsonValue` and record the handler name in a generated comment
  block at the top of `api.ts` so we can audit which handlers
  still need typing on the server side.
- Emit `packages/nimbus-ui/convex/_generated/dataModel.d.ts` (already
  imported by `api.ts` per `import type { Doc, Id } from "./dataModel"`).
  The `Doc<T>` and `Id<T>` types should reference the schema-known
  tables. If the codegen already has table metadata, this is a small
  template add.
- Re-run codegen (`npm run codegen` at repo root, or whatever the
  registered script is — find via `package.json` at A0).
- Consumer migration (in the same wave, no shims):
  - `routes/app/services.tsx:41`: drop `as ServiceDoc[] | undefined`.
    The `useQuery` return is now `Doc<"services">[] | undefined`. If
    the local `ServiceDoc` type still needs to exist for prop
    typing in the same file, alias it to `Doc<"services">`.
  - `routes/app/services_.$service.tsx:70`: drop
    `id: serviceId as never`. The arg type now matches what
    `useParams()` produces — likely a `string` mapped to `Id<"services">`.
  - `routes/app/services_.$service.tsx:78`: drop the second
    `as ServiceDoc[] | undefined` cast.
  - `routes/admin/services.tsx:25`,
    `routes/admin/index.tsx:46`,
    `routes/admin/machines.tsx:455`,
    `routes/admin/services_.$service.tsx:68` and `:76`: mirror.
  - `routes/app/compute_.runs_.$runId.tsx:44`: drop
    `id: runId as never`.
  - `shell/tenant-selector.tsx:193`: drop
    `search: { create: 1 } as never`. This is a TanStack Router
    search-param shape issue, not a codegen issue — investigate.
    Likely the route's `validateSearch` schema needs a `create`
    field or the cast becomes unnecessary once the route
    declarations are correct. If genuinely TanStack-side, the fix
    still lands here.
  - `shell/primary-drawer.tsx:107-108`: drop the two
    `entry.countQuery as never` and
    `(entry.countArgs ?? undefined) as never` casts. This is a
    higher-rank polymorphic store of `(args, return)` pairs;
    introducing a `QueryEntry<Args, Return>` discriminated wrapper
    at the producer side is the right fix. If that turns into more
    than ~50 LOC, scope it to a follow-on plan and leave a comment
    naming the deferral — but try the wrapper first.

Specs:

- `packages/codegen/` spec: add a test that emits a fixture handler
  with a registered return validator and asserts the emitted
  `makeQueryReference<…, ReturnShape>` carries the validator's
  shape, not `JsonValue`.
- `packages/codegen/` spec: same fixture without a return validator
  → emitted line still carries `JsonValue` AND the audit-comment
  block names the handler.
- `packages/nimbus-ui` consumer specs: existing specs adapt; the
  consumer-side change is a deletion, not a behavior change. Any
  spec that currently exercises a fallback through `undefined`/
  `JsonValue` paths must continue to pass after the cast removal.

Done when:

- `grep -rn 'as never' packages/nimbus-ui/src` returns zero hits.
- `grep -rn 'as ServiceDoc\[\] | undefined\|as ServiceDoc | null | undefined' packages/nimbus-ui/src`
  returns zero hits.
- Codegen-emit spec covers both the typed and untyped fallback.
- `npm run typecheck` clean across the workspace.
- `cd packages/nimbus-ui && npx vitest run` — all pass; count
  recorded.
- `cd packages/nimbus-ui && npm run build` clean.

### A2 — Lift `ServiceDoc` + cross-persona type-leak audit (HIGH)

Goal: every shared route-level type lives at a shared path; no
`routes/admin/*` import reaches into `routes/app/*` (and vice
versa).

Touch list:

- `packages/nimbus-ui/src/lib/types/service.ts` (new): export
  `ServiceDoc` from this file, defined as
  `Doc<"services">` (post-A1) or as the structural type that
  currently lives at `routes/app/services.tsx:21` if A1 left the
  structural form. Re-export from `lib/types/index.ts` if a barrel
  exists; otherwise import from the specific path everywhere.
- `routes/app/services.tsx:21-32`: delete the local `ServiceDoc`
  declaration; import from `lib/types/service`.
- `routes/admin/services.tsx:13`: replace
  `import { type ServiceDoc, ServicesTable } from "../app/services"`
  with two imports: `ServiceDoc` from `lib/types/service`,
  `ServicesTable` either stays imported from `routes/app/services`
  (component reuse is fine) or moves to a shared place if the import
  direction itself feels wrong — decide based on whether other
  surfaces also import `ServicesTable`.
- `routes/admin/services_.$service.tsx:21`: mirror.
- `routes/admin/index.tsx:27`: delete the duplicate local
  `type ServiceDoc = { _id: string }`; import the shared type.
- Audit pass: `grep -rn 'from "../app/\|from "../../routes/app/'
  packages/nimbus-ui/src/routes/admin/`. Every match that imports a
  *type* (not a *component*) is a leak candidate — either lift the
  type or document that the component reuse is intentional.
- Audit pass mirror: `grep -rn 'from "../admin/\|from "../../routes/admin/'
  packages/nimbus-ui/src/routes/app/`. Same triage.

Specs:

- Type-only smoke test in `lib/types/service.spec.ts`: a compile-only
  `Equal<ServiceDoc, Doc<"services">>` (or whichever ground truth A1
  produced) — fails the typecheck if drift sneaks back.
- No runtime-behavior specs in this phase; the existing route specs
  cover the consumer side.

Done when:

- `grep -rn 'import.*ServiceDoc.*from ".*routes/app' packages/nimbus-ui/src`
  returns zero hits.
- `grep -rn 'type ServiceDoc' packages/nimbus-ui/src` returns
  exactly one hit (the canonical definition in `lib/types/service.ts`).
- Cross-persona route-to-route type imports either lifted to
  `lib/types/` or documented in the execution log as intentional
  component reuse.
- vitest + typecheck + build clean.

### A3 — Decompose `routes/admin/settings.tsx` (HIGH)

Goal: bring `routes/admin/settings.tsx` below the CLAUDE.md warning
band by extracting concept-owned children.

Background: 1608 LOC. Six identifiable sections (find via heading /
sub-drawer-item id walk during A0): identity, appearance, encryption,
license, system status, adapter capabilities.

Touch list (proposed; refine after A0 walk):

- `routes/admin/settings/identity.tsx` (new) — identity section
  (user, tenant, system tenant chip).
- `routes/admin/settings/appearance.tsx` (new) — wraps
  `<AppearanceSection>` plus any settings-specific framing.
- `routes/admin/settings/encryption.tsx` (new) — encryption row +
  `StateDot` (already in place from predecessor H4).
- `routes/admin/settings/license.tsx` (new) — license snapshot +
  `UpgradePopover` integration.
- `routes/admin/settings/system-status.tsx` (new) — system status,
  uptime, version chip.
- `routes/admin/settings/adapter-capabilities.tsx` (new) — adapter
  capability table.
- `routes/admin/settings.tsx` becomes a thin composition root: route
  declaration, sub-drawer contribution (the sub-drawer items map
  1:1 to the children), section dispatch via `?section=` (mirror the
  `/admin/network` pattern from H4). Target ≤900 LOC. If TanStack
  Router file-routing forbids deeply-nested children under
  `routes/admin/settings/`, use the flat sibling form
  `routes/admin/settings-identity.tsx` etc. (file-router naming
  decision: confirm at A0).

Spec impact:

- Existing `routes/admin/settings.spec.tsx` (if present — confirm at
  A0) tests adapt to import children directly where the test
  exercises a single section.
- New per-child specs only where the child carries non-trivial
  branching logic (license snapshot, system status). The
  composition root itself doesn't need a new spec — sub-drawer
  contribution is already covered.

Done when:

- `wc -l packages/nimbus-ui/src/routes/admin/settings.tsx` ≤900.
- No new child file exceeds 500 LOC. If one does, recurse: that
  section had hidden sub-sections.
- `cd packages/nimbus-ui && npx vitest run` — all pass.
- Live walk of `/admin/settings` shows zero regressions vs. the
  predecessor's after-shot.

### A4 — Router-level loaders for data routes (MEDIUM)

Goal: data routes with one or two queries move to TanStack Router
`loader` / `beforeLoad`; the route component reads from
`Route.useLoaderData()` instead of `useQuery`.

Bounded scope — the routes that migrate cleanly:

- `routes/admin/tenants.tsx` — single tenants fetch, single error
  path. Loader returns `{ tenants } | { error }`; the diagnostic
  envelope from H3(b) becomes a route-level error component.
- `routes/admin/services.tsx` and `routes/app/services.tsx` — the
  services list. Loader fetches; component renders.
- `routes/admin/services_.$service.tsx` and
  `routes/app/services_.$service.tsx` — the service detail. Loader
  fetches the single service + the tenant-grouped sibling list;
  component renders. The 404 path becomes a route-level error,
  matching the predecessor's diagnostic envelope.

Out of bounds for this wave (continue to use inline `useQuery`):

- `routes/app/observability.tsx` and `routes/admin/observability.tsx`
  — they fan out into per-tab queries with cross-tab filtering;
  loader migration would force a per-tab route split. Leave for a
  later wave.
- `routes/admin/settings.tsx` (post-A3 children) — every child
  fetches its own slice; co-locating with the child is fine.
- Anything that uses live WebSocket subscriptions rather than
  point-in-time fetches.

Specs:

- Per migrated route, a small loader test using the same
  abort-controller pattern that already works for
  `use-tenant-bootstrap.spec.tsx` — fixture the fetch, exercise
  success and error.
- An end-to-end loader contract: navigating to a migrated route
  shows the route-owned loading state, not the component-owned
  one.

Done when:

- The five routes above use `Route.loader` (or `beforeLoad` if the
  fetch must happen before search-param validation) and
  `Route.useLoaderData()`.
- The component bodies no longer call `useQuery(api...)` for the
  primary data; `useQuery` is preserved for derived/secondary data
  if any.
- The diagnostic-envelope path from predecessor H3(b) renders via
  the route-level error component, not a component-level branch.
- vitest + typecheck + build clean.
- Live walk shows the loading transition is visible on slow
  network (throttle via chrome-devtools-mcp).

### A5 — Component catalog (MEDIUM)

Goal: a fixture-driven catalog for the eleven reusable components,
runnable locally and buildable as a static bundle.

Vehicle decision (pick at A5 start; record in execution log):

- **Storybook**: heavier dependency, richer ecosystem.
- **Ladle**: lighter, Vite-native, no addon ecosystem.

Default unless a constraint surfaces: **Ladle** (Vite-native fits
the current build, smaller surface area to maintain). Switch to
Storybook if a downstream need (Chromatic, addons) surfaces during
A5 setup.

Touch list:

- `packages/nimbus-ui/.ladle/config.mjs` (or
  `packages/nimbus-ui/.storybook/main.ts` if Storybook is picked).
- `packages/nimbus-ui/src/components/state-chip.stories.tsx` (new)
  — variants: ok / loading / offline / error / unavailable.
- `state-dot.stories.tsx` — same axes.
- `empty-state.stories.tsx` — with and without CTA, both title
  styles.
- `copy-chip.stories.tsx` — short id, long id, multi-line value.
- `loading-cell.stories.tsx` — all `LoadingValue<T>` kinds.
- `time.stories.tsx` — `RelativeTime` and `Uptime`, fixed clock.
- `breadcrumb.stories.tsx` — 1/2/4/8 segments, per-segment
  copy-chip behavior.
- `upgrade-popover.stories.tsx` — open + closed states.
- `appearance-section.stories.tsx` — each appearance preset.
- One sub-drawer story exercising both `kind: "static"` and
  `kind: "dynamic"` specs without the router context — uses a
  story wrapper that fakes the sub-drawer host.
- `package.json` script `catalog` → `ladle serve` (or
  `storybook dev`); `catalog:build` → `ladle build`.

Specs:

- No vitest specs in this phase; the stories are the spec
  surface.
- Smoke check: `npm run catalog:build` from `packages/nimbus-ui/`
  produces a static bundle without errors.

Done when:

- The eleven stories exist and render in the local catalog.
- `catalog:build` is clean.
- `README.md` (or `CATALOG.md`) under `packages/nimbus-ui/`
  documents how to run the catalog.
- typecheck + build (the main app build, not the catalog build)
  remain clean.

### A6 — CI browser-smoke harness (MEDIUM)

Goal: a deterministic browser walk runnable in CI, asserting zero
console errors and key visible affordances.

Vehicle decision (pick at A6 start; record in execution log):

- **playwright-cli**: external dependency, well-documented, easy
  CI integration.
- **chrome-devtools-mcp via a thin Node runner**: same dependency
  set as the proof-bundle workflow, but the runner has to be
  written.

Default unless a constraint surfaces: **playwright-cli** (CI lane
maturity, smaller delta to set up). Per `MEMORY.md`:
playwright-cli is allowed; `@playwright/mcp` is not.

Touch list:

- `packages/nimbus-ui/tests/e2e/smoke.spec.ts` (new) — a single
  walk:
  1. Boot the embedded build (or Vite dev — pick at A6 start).
  2. Visit `/app/` — assert tile envelopes render
     (`data-testid` hooks added where needed; check counts).
  3. Visit `/admin/` — same.
  4. Visit `/app/services` — assert ScopeChip reads
     `TENANT <tenant>`.
  5. Visit `/admin/services` — assert tenant-grouped sub-drawer.
  6. Visit `/admin/services/<id>` — assert single Placement tab.
  7. Visit `/admin/tenants` — assert the diagnostic-envelope
     renders (allow the named `/api/tenants` 404; fail on other
     network errors).
  8. Visit `/app/observability` — assert disabled-tab chips on
     `EVENTS`/`ERRORS`.
  9. Open command palette via ⌘K — assert listbox + modes.
  10. End: dump `console.error`/`console.warn` count; assert
      ≤1 (the named 404) or 0 (if running against a real
      server).
- `Makefile` target `verify-desktop-ui`:
  ```
  verify-desktop-ui:
  	$(MAKE) -C packages/nimbus-ui e2e
  ```
- `packages/nimbus-ui/Makefile` (or npm script) `e2e` →
  `playwright test tests/e2e/smoke.spec.ts`.
- `.github/workflows/desktop-ui.yml` (new) — lane on
  push/pull_request that runs `make verify-desktop-ui` against a
  fresh build. Run on `ubuntu-latest`; install Playwright
  browsers; cache the install.

Specs:

- The e2e file itself is the spec.
- No new vitest specs; the contract is the e2e walk.

Done when:

- `make verify-desktop-ui` passes locally against a fresh build.
- A no-op PR (whitespace change) triggers the lane and the lane
  passes.
- The e2e file's assertion set is documented in a comment block
  at the top so future plans know what's covered without reading
  the spec.

### A7 — Large-file audit pass (LOW)

Goal: re-check the next-tier hotspot files; decompose only where
concept-owned boundaries exist and the file is trending toward the
CLAUDE.md band.

Touch list:

- `routes/app/storage_.$table.tsx` (1154 LOC). Likely natural
  boundaries: table rendering, row drawer, filter bar, query
  composition. Decompose only if clean.
- `routes/app/observability.tsx` (978 LOC). Likely boundaries: per-
  tab body (logs / runs / events / errors). The tab strip stays
  in the root; each tab body becomes its own file.
- `routes/admin/machines.tsx` (739 LOC). Likely boundaries: the
  machines table itself vs. the create-machine drawer/dialog.

For each: walk the file end-to-end, identify concept-owned units,
extract only where the unit reads cleanly on its own. If a file
resists clean decomposition, record the resistance in the
execution log per CLAUDE.md "explicit justification" guidance and
leave it as-is.

Done when:

- Per-file decision recorded in execution log: split (with new file
  paths and resulting LOC), or kept (with justification).
- No file exceeds the CLAUDE.md threshold post-decomposition.
- vitest + typecheck + build clean.

### A8 — Verification + close + archive

Goal: prove every gate, capture proof, archive.

Steps:

1. Re-run all repo verification:
   - `cd packages/nimbus-ui && npx vitest run` — record count.
   - `npm run typecheck` — clean.
   - `cd packages/nimbus-ui && npm run build` — clean.
   - `make verify-desktop-ui` — clean (added in A6).
   - `make ci` if it doesn't take prohibitively long; otherwise
     `make check && make clippy` minimum.
2. Grep gates (all must return zero hits):
   - `grep -rn 'as never' packages/nimbus-ui/src`
   - `grep -rn 'as ServiceDoc\[\] | undefined\|as ServiceDoc | null | undefined' packages/nimbus-ui/src`
   - `grep -rn 'import.*ServiceDoc.*from ".*routes/app' packages/nimbus-ui/src/routes/admin`
   - `grep -c '^' packages/nimbus-ui/src/routes/admin/settings.tsx`
     ≤900.
3. Capture this wave's after-shots with prefix `a8-`. Use
   chrome-devtools-mcp at 1440×900 against the Vite dev server (or
   the embedded build via `nimbus start`). Captures:
   - `a8-app-services-no-cast.png` — `/app/services` rendered with
     typed services (visual identical to predecessor; proof is the
     diff, not the pixels). Optional; the grep gate covers it.
   - `a8-admin-settings-decomposed.png` — `/admin/settings` rendered
     post-A3; visual identical to predecessor.
   - `a8-admin-tenants-loader.png` — `/admin/tenants` showing the
     loader-driven diagnostic envelope (route-level error).
   - `a8-catalog-state-chip.png` — local catalog showing
     `StateChip` story.
   - `a8-catalog-empty-state.png` — local catalog showing
     `EmptyState` story.
   - `a8-e2e-lane.png` — GitHub Actions UI showing the
     `verify-desktop-ui` lane green on a sample PR.
4. Re-evaluate A8 deferrals:
   - F15 theme-matrix smoke: if the verification-tooling plan has
     landed, pull in; otherwise restate the deferral here with the
     owning plan named.
   - F6 service-detail Restarts/Density/Drift: same triage; owning
     plan is the placement-controller / restart-audit-log work.
5. Write `docs/plans/proof/desktop-ui-architecture-hardening/README.md`
   mirroring the predecessor's structure: before/after sections,
   A-phase → after-evidence mapping table.
6. Flip plan status to `done`; append execution log entry; `git mv`
   to `docs/plans/archive/`; update `docs/plans/README.md` (remove
   from active; add archived-baseline entry).

Done when:

- Ledger A0–A8 all `done`.
- Plan lives under `docs/plans/archive/`.
- Proof bundle exists with the mapping table covering every Outcome
  bullet.
- `docs/plans/README.md` lists this plan as an archived baseline and
  no longer lists it as active.
- Console hygiene: zero `error`/`warn` across the e2e walk and the
  catalog build (allow the same named `/api/tenants` 404 in the
  auth-less capture context, named in the proof README).

## Verification approach

- Use chrome-devtools-mcp (workspace-allowed dirs only) for live
  page inspection and screenshots during execution. Use
  playwright-cli for the persistent CI lane (A6 default). Per
  `MEMORY.md`: `@playwright/mcp` is not in scope.
- For every phase with a unit-testable gate, the test lands in the
  same commit as the implementation. The codegen change (A1) lands
  with both the emit-spec and the consumer-side cast removals in
  the same wave — no shim period.
- Pre-launch policy applies. When a type or API shape changes
  (codegen emit, `ServiceDoc` import path, settings file split,
  loader signatures), refactor every consumer in the same wave.
- Cross-cutting console-hygiene gate: zero `error`/`warn` across the
  e2e walk in CI. The known dev-server `/api/tenants` 404 is
  acceptable only if named in the e2e spec's allowlist and in the
  proof README.

## Execution log

(a) 2026-05-18 — Plan promoted from a 2026-05-18 post-closure
critical-reflection review of the Followup-Hardening wave. The
predecessor closed cleanly on its scope (H0–H7 done; 32 files / 222
tests passing; four grep gates zero hits; visual evidence gap
backfilled). The reflection surfaced six architectural items the
wave deliberately did not touch: codegen typing (CRITICAL),
`ServiceDoc` cross-persona type leak (HIGH), `routes/admin/settings.tsx`
at 1608 LOC (HIGH), router-level loaders (MEDIUM), component
catalog (MEDIUM), CI browser-smoke harness (MEDIUM), plus two LOW
items (next-tier large-file audit and the F15/F6 deferral
restatement). Cast inventory at promotion: 5× `as never`, 6×
`as ServiceDoc[] | undefined`, plus 1× `as ServiceDoc | null | undefined`.
Phases A0–A8 prepared. Pre-launch policy continues; the codegen
emit shape changes break every consumer in one wave with no shim.
