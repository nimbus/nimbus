# Before-state cross-reference

This wave's `before/` is the predecessor's sealed `after/` bundle plus
the additional residue surfaced by the 2026-05-19 code review:

`docs/plans/proof/desktop-ui-followup-hardening/after/` — 11 captures
with `h7-` prefix, sealed at the H wave's 2026-05-18 close. The A
wave's close (2026-05-18) added no new screenshots and pointed at
this same bundle for visual identity.

No PNG duplication: the predecessor's after-state IS this wave's
before-state for the visual surface. The mapping below records the
architecture-level before-state — the counts the wave must drive to
zero (or to the canonical-one shape).

Pre-wave HEAD references:

- `e11cc9ef` — Promote desktop-ui architecture-residue plan (this
  wave's promotion commit).
- `5be6bb48` — A8 followup #2: type-uniqueness grep gate recorded in
  proof README.
- `0e74b9ee` — A8 followup #1: plan ledger flipped to done.
- `74a5a139` — A8: close + archive desktop-ui
  architecture-hardening plan.

## Architecture-level before-state (re-grepped 2026-05-19)

### Cast inventory

| Site | Lines | Count | Notes |
|------|-------|-------|-------|
| `packages/nimbus-ui/src/shell/nav-entries.ts` — `as unknown as CountQuery` | 50, 59, 68, 77, 95, 134, 143, 152, 161 | **9** | R1's BLOCKER target. Review claimed 10; actual is 9. Drift noted in execution log entry (a). |
| `as never` anywhere under `packages/nimbus-ui/src` + `packages/nimbus/src` | — | **0** | A2 grep gate clean — kept clean by R1. |
| `as ServiceDoc[] \| undefined` / `as ServiceDoc \| null \| undefined` | — | **0** | A2 grep gate clean. |
| `type ServiceDoc` declarations | `lib/types/service.ts:3` | **1** | A2 lift outcome — single canonical source. |
| cross-persona `import.*ServiceDoc.*from ".*routes/app` under `routes/admin` | — | **0** | A2 grep gate clean. |
| `as unknown as` in test files | — | **15** | Legitimate test-fixture pattern (`fetch as unknown as typeof fetch`, `Route as unknown as {...}` for route internals); not in scope for R1's gate. Recorded for completeness. |

Note on `as unknown as` test sites: 15 hits across
`use-tenant-bootstrap.spec.tsx`, `use-staleness.spec.tsx`,
`desktop-bridge.spec.ts/ts`, `routes/admin/tenants.spec.tsx`,
`routes/admin/services.spec.ts`,
`routes/admin/services_.$service.spec.ts`, `routes/app/services.spec.ts`,
`routes/app/services_.$service.spec.ts`. R1's widened gate will
narrow scope to `packages/nimbus-ui/src/**/!(*.spec.ts*)` and
`packages/nimbus/src/**` to allow this test idiom.

### Inline `useQuery` on routes flagged for loaderization (R2 + R4)

| Route | Total `useQuery` count (incl. import) | Sibling-query lines |
|-------|---------------------------------------|---------------------|
| `routes/admin/services_.$service.tsx` | 3 | 88 (`bundles`), 200 (`machines`) |
| `routes/app/services_.$service.tsx` | 2 | 100 (`bundles`) |
| `routes/app/compute_.runs_.$runId.tsx` | 3 | 23, 27 (page-level — entire route not loaderized) |

### Tenant-switch via `useEffect → router.invalidate()` (R7 target)

| Site | Line |
|------|------|
| `routes/app/services.tsx` | 39 — `void router.invalidate();` |
| `routes/app/services_.$service.tsx` | 96 — `void router.invalidate();` |

### Codegen `JsonValue` declarations (R3 dedup target)

Three identical `export type JsonValue` definitions in the generated
surface:

| File | Line |
|------|------|
| `packages/nimbus-ui/convex/_generated/api.ts` | 5 |
| `packages/nimbus-ui/convex/_generated/dataModel.d.ts` | 4 |
| `packages/nimbus-ui/convex/_generated/scheduled_functions.ts` | 5 |

R3 collapses to one owner (`dataModel.d.ts`); the other two `import
type`.

### Codegen audit-comment gap (R3)

`packages/codegen/src/emit/schema_types.mjs:35-49` — `isTrivialValidator`
does not unwrap `union(v.any(), v.null())`. The textbook fallback case
(`system:status` handler returning `JsonValue | null`) is therefore
treated as `explicit` and the audit comment never fires in
`packages/nimbus-ui/convex/_generated/api.ts`.

### Codegen convention-inference layer (R3 decision)

`packages/codegen/src/emit/type_inference.mjs:138-139` —
`LIST_EXPORT_NAMES` and `SINGLETON_EXPORT_NAMES` are hard-coded
suffix sets. A query named `services:byTenant` (or any name not
ending in one of the listed conventions) falls back to `JsonValue`
without an audit comment. R3 either keeps the layer and emits audit
comments when it fires, or drops the layer entirely. Decision
recorded in execution log entry (c).

### Codegen specs missing (R3)

`packages/codegen/src/selftest.mjs` covers existing fixture suites
only. Zero specs for:

- `inferFunctionResultType` explicit path.
- `inferFunctionResultType` plan-inferred path (query / mutation /
  action).
- `inferFunctionResultType` convention-inferred path.
- `inferFunctionResultType` fallback path (audit-comment emission).

R3 adds coverage for all four plus an assertion that the audit
comment emits in `union(v.any(), v.null())`-shaped emissions.

### Loader-error envelope coverage gap (R5)

Spec coverage for the loader-rejects path exists only on
`routes/admin/tenants.spec.tsx:139-178`. The four other A4 routes
(`routes/admin/services.tsx`,
`routes/admin/services_.$service.tsx`, `routes/app/services.tsx`,
`routes/app/services_.$service.tsx`) have happy-path coverage but
no loader-error spec and no `Route.errorComponent` rendering the
diagnostic envelope.

### Filter / table-cell duplication (R6)

Pixel-for-pixel duplicates across the observability tabs and
`/admin/tenants`:

| Function | Sites |
|----------|-------|
| `FilterSelect` / `FilterInput` | `routes/app/observability/logs.tsx:249-319`, `routes/app/observability/runs.tsx:195-306` |
| `Th` / `Td` | `routes/app/observability/runs.tsx:267-306`, `routes/admin/tenants.tsx:391-430` |

R6 consolidates to single-source `_filters.tsx` + shared table-cell
helpers.

### A3 settings residue (R8)

Dead `dialogRef` declarations carried verbatim from the original
1608-LOC `settings.tsx` monolith:

| File | Lines |
|------|-------|
| `routes/admin/settings/danger-zone.tsx` | 52 (declared), 101 (forwarded), 182 (declared), 223 (forwarded) |

Both refs are passed to `DialogShell ref={dialogRef}` but never read;
`DialogShell` manages its own focus return. R8 removes both.

Plus `routes/admin/settings/sub-drawer.ts` typed as plain
`SubDrawerSpec` rather than the `as const satisfies
StaticSubDrawerSpec<...>` typed-const pattern used by
`routes/app/observability.tsx:65`. R8 adopts the typed-const
pattern.

### CSP test gap (R9)

`crates/nimbus-server/src/http/ui.rs:362-374` —
`inline_fouc_script_hash_matches_csp` extracts the first
`<script>...</script>` literal from the embedded `index.html`. A
second inline `<script>` (or a `<script attr="...">` open tag) would
silently bypass the CSP hash pin. R9 asserts *exactly one* inline
script element and tolerates attribute-bearing tags.

### CI workflow path filter gap (R9)

`.github/workflows/desktop-ui.yml` push + pull-request triggers
cover: `crates/**`, `packages/nimbus-ui/**`, `packages/codegen/**`,
`packages/nimbus/**`, `packages/convex/**`, `Makefile`,
`.github/workflows/desktop-ui.yml`.

Missing: `Cargo.toml`, `Cargo.lock`, `rust-toolchain*`. A dep bump
that affects `nimbus-bin` (e.g. axum) won't trigger the smoke lane.

### Smoke spec conditional assertions (R10)

`packages/nimbus-ui/tests/e2e/smoke.spec.ts` uses `if (await
<locator>.count()) { assert }` patterns at lines:

- **87** — `if (await scopeChip.count())` — ScopeChip assertion
  bypassed on a fresh fixture.
- **91** — `if (await servicesTable.count())` — services-table
  assertion bypassed on a fresh fixture.
- **111** — `if (await firstServiceLink.count())` — service-detail
  navigation bypassed on a fresh fixture.

Three bypass sites (review noted two). R10 seeds the fixture (one
tenant + one service) so all three assertions fire unconditionally.

### Parallel `ObservabilityTab` unions (R11 nit)

| File | Declaration |
|------|-------------|
| `routes/app/observability/types.ts:1` | `ActiveObservabilityTab = "logs" \| "runs"` (narrow) |
| `routes/app/observability.tsx:70` | `ObservabilityTab = (typeof OBSERVABILITY_SUB_DRAWER.items)[number]["id"]` (wide — includes `events`, `errors`) |

Consumers split: `DisabledTab` (`:130`) takes wide;
`ActiveTabLink` (`:160`) takes narrow. R11 collapses to one canonical
source.

### Other R11 nit sites

| Site | Issue |
|------|-------|
| `routes/admin/services_.$service.tsx:121, 142` / `routes/app/services_.$service.tsx:120-145` | Loader-typed `service` is `ServiceDoc \| null` even though loader throws `notFound()` first. R11 returns `service: service!` so consumers narrow correctly. |
| `shell/tenant-selector.tsx:316-317` | `aria-live="polite"` sr-only pathname looks like debug residue; announces every navigation to AT users. R11 removes or commits to it. |
| `packages/codegen/src/emit/type_inference.mjs:96` | `inferMutationResultType` emits `Id<"unknown">` when `plan.table` is missing. R11 throws at codegen time. |
| `packages/nimbus/src/browser.ts` (+9 lines from A1+A2 wave) | 6 type re-exports added; only `QueryReference` is consumed in `nimbus-ui`. R11 audits and drops unused. |

### Catalog story state-coverage gaps (R11)

Older stories cover happy-path props only — no
disabled/loading/error/edge variants:

- `stories/copy-chip.stories.tsx` — no clipboard-denied variant.
- `stories/breadcrumb.stories.tsx` — no truncation variant.
- `stories/time.stories.tsx` — no past/future/skew variants.
- `stories/upgrade-popover.stories.tsx` — no `available: false`
  (no-upgrade) story.
- `stories/appearance-section.stories.tsx` — `useUiStore` mutation
  has no cleanup; side-by-side catalog renders race.

## Plan accuracy delta found at R0

- **Cast count delta (1 site).** The plan body and review say "10×
  `as unknown as CountQuery`"; actual is **9**. Recorded in execution
  log entry (a). Outcome contract unchanged — R1 still drives the
  count to zero in non-spec files.
- **Smoke conditional-bypass site count delta.** Plan body says
  "steps 3 and 5" (two bypass sites); actual is **three** sites at
  lines 87, 91, 111. R10's scope unchanged — all three must be
  unconditional after R10.

All other counts match the review.
