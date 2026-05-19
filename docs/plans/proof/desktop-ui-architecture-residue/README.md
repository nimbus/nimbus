# Desktop UI — Architecture Residue · Proof Bundle

Status at close: all twelve R-phase ledger rows `done`; plan archived to
`docs/plans/archive/desktop-ui-architecture-residue-plan.md`.

Predecessor visual baseline (unchanged by this wave):
`docs/plans/proof/desktop-ui-followup-hardening/after/` (11 `h7-*`
captures, sealed at the H wave's 2026-05-18 close). This wave's surface
is code/architecture + tests; no new PNGs were produced.

## Before → after evidence (R0–R12)

| Phase | Before-state (R0 grep snapshot) | After-state (close-time evidence) |
|-------|---------------------------------|-----------------------------------|
| R0 | `before.md` records 9× `as unknown as CountQuery`, 3× sibling-`useQuery` on `_.$service.tsx`-class routes, 2× `useQuery` on `compute_.runs_.$runId.tsx`, 3× `JsonValue` decls in `_generated/`, 2× `useEffect → router.invalidate()`, 4× dead `dialogRef` lines, 2× parallel `ObservabilityTab` unions, 3× smoke spec `if (count) {...}` bypasses, missing `Cargo.{toml,lock}` + `rust-toolchain*` in workflow paths. | Snapshot frozen at HEAD `e11cc9ef`; promotion commit recorded. Zero source edits this phase. |
| R1 | 9× `as unknown as CountQuery` in `shell/nav-entries.ts` (lines 50, 59, 68, 77, 95, 134, 143, 152, 161) plus 1× `as unknown as WindowWithNimbus` in `lib/desktop-bridge.ts`. | Producer-side `QueryEntry<TArgs, TReturn>` + `queryEntry()` constructor added in `packages/nimbus/src/internal/shared.ts`; consumed by `nav-entries.ts`. `desktop-bridge.ts` switched to ambient `declare global` augmentation. `shell/query-entry.spec.ts` adds four `expectTypeOf` assertions. Grep `as unknown as ` in non-spec files → **0**. |
| R2 | 3× sibling-`useQuery` calls on `routes/admin/services_.$service.tsx:88,200` and `routes/app/services_.$service.tsx:100`. | Sibling queries folded into `Route.loader` with `Promise.all`; loader payloads (`{ service, services, bundles, machines }` admin / `{ service, services, bundles, activeTenant }` app) drive components via `useLoaderData()`. `BundleDoc` / `MachineDoc` local shape decls removed in favor of `Doc<"bundles">` / `Doc<"machines">`. Specs extended to mock four / three parallel queries. Grep `useQuery` in the three loaderized routes → **0**. |
| R3 | 3× `JsonValue` decls in `_generated/{api,dataModel.d,scheduled_functions}.ts`; `isTrivialValidator` did not unwrap `union(v.any(), v.null())`; `inferMutationResultType` silently emitted `Id<"unknown">` on missing `plan.table`; convention-inferred fallbacks never wrote audit comments. | `JsonValue` collapsed to `dataModel.d.ts` (1 decl); other consumers `import type`. `isTrivialValidator` now unwraps `union` and widens trivial-member unions. `inferMutationResultType` throws at codegen on missing `plan.table`. `reference_helpers.helperCall` adds an audit entry on `convention-inferred` in addition to fallback. `selftest/type_inference_fixtures.mjs` adds 10 unit cases. Audit block lists 14 convention-inferred entries (visible, tightenable). Grep `export type JsonValue` in `_generated/` → **1** (in `dataModel.d.ts` only). |
| R4 | `routes/app/compute_.runs_.$runId.tsx` not loaderized — 2× page-level `useQuery` (lines 23, 27); only the `as never` → `as Id<"runs">` cast got renamed at A wave. | Route fully loaderized: loader runs `compute.runByIdForTenant` + `compute.listRunsForTenant` in parallel; component reads `useLoaderData()`. `useQuery` import dropped. Spec extended to mock the loader. Grep `useQuery` in this file → **0**. |
| R5 | Loader-error envelope spec coverage only on `routes/admin/tenants.spec.tsx:139-178`; four other A4 routes lacked `Route.errorComponent` + loader-rejects spec. | `routes/admin/services.tsx`, `routes/admin/services_.$service.tsx`, `routes/app/services.tsx`, `routes/app/services_.$service.tsx` each gained an exported `*LoaderError` component wired via `Route.errorComponent`. Four loader-rejects specs added (one per route) asserting the diagnostic envelope renders the loader error message and a retry CTA. |
| R6 | `FilterSelect` / `FilterInput` duplicated across `routes/app/observability/logs.tsx:249-319` and `runs.tsx:195-306`; `Th` / `Td` duplicated across `runs.tsx:267-306` and `routes/admin/tenants.tsx:391-430`. | Extracted to `routes/app/observability/_filters.tsx` (single source) plus shared `components/table-cell.tsx` (`Th` / `Td` helpers). Two observability tabs + tenants page consume the shared primitives. |
| R7 | 2× `useEffect → router.invalidate()` in `routes/app/services.tsx:39` and `routes/app/services_.$service.tsx:96` reacted to active-tenant changes outside the router. | Both routes adopted `Route.loaderDeps: () => ({ activeTenant: useUiStore.getState().activeTenant })`; the loader fans the tenant through into `services.list` etc. The `useEffect` blocks deleted. Tenant switch now invalidates via the router cache key, not an explicit imperative call. Grep `router.invalidate` in `routes/app/services*.tsx` → **0**. |
| R8 | 4× dead `dialogRef` lines in `routes/admin/settings/danger-zone.tsx` (declared at 52, 182; forwarded at 101, 223 to a `DialogShell` that manages its own focus return); `settings/sub-drawer.ts` typed plain `SubDrawerSpec` instead of `as const satisfies StaticSubDrawerSpec<...>`. | All four `dialogRef` references removed (component focus stays on the trigger, identical to `DialogShell`'s built-in return). `settings/sub-drawer.ts` adopted the typed-const `as const satisfies StaticSubDrawerSpec<SettingsSection>` pattern used by `observability.tsx`. |
| R9 | `crates/nimbus-server/src/http/ui.rs::inline_fouc_script_hash_matches_csp` extracted the first `<script>...</script>` literal — a second inline script or `<script attr="...">` open tag would silently bypass the CSP hash pin. Workflow `.github/workflows/desktop-ui.yml` path filter missing `Cargo.{toml,lock}` + `rust-toolchain*`. | `UI_CSP` rewritten as `concat!(...)`. `inline_fouc_script_hash_matches_csp` rewritten with `inline_script_bodies` + `has_src_attr` helpers to walk every `<script` opening tag and assert exactly one inline script. Workflow `push.paths` + `pull_request.paths` extended with `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`. |
| R10 | Smoke spec `tests/e2e/smoke.spec.ts` had three `if (await count()) { assert }` bypass sites (lines 87, 91, 111) — ScopeChip, services-table, operator service-detail navigation. | Added `seedSmokeFixture` helper: `POST /api/tenants {id: "smoke"}` (status 201 asserted with body in failure message) then `POST /convex/_nimbus/mutation` with raw `Mutation::Insert` on the `_nimbus.services` system table (status 200 asserted). Three bypass branches replaced with unconditional assertions including `services-row-smoke-svc` and `sub-drawer-item-op-service-smoke-svc` link + placement-tab assertions. Grep `if (await` in this file → **0**. |
| R11 | Defensive `service?.bundleId` chain in `routes/admin/services_.$service.tsx:114`; parallel `ObservabilityTab` unions across `routes/app/observability/types.ts:1` and `routes/app/observability.tsx:70`; debug-residue `aria-live="polite"` span + unused `useRouterState` import in `shell/tenant-selector.tsx`; three unconsumed type re-exports (`ActionReference`, `MutationReference`, `PaginatedQueryReference`) in `packages/nimbus/src/browser.ts`. Story state coverage gaps across 5 catalog stories. | Defensive optional-chain removed (loader's `if (!service) throw notFound()` narrow is enough). `types.ts` made the canonical source: `ACTIVE_OBSERVABILITY_TABS` + `DISABLED_OBSERVABILITY_TABS` arrays, with `ObservabilityTab` derived via `(typeof ARRAY)[number]`; `observability.tsx` rebuilds `OBSERVABILITY_SUB_DRAWER` programmatically and re-exports `ObservabilityTab`. `tenant-selector.tsx` dropped `useRouterState` + `pathname` + sr-only aria-live span. `browser.ts` dropped the three unconsumed type re-exports (six remaining are all real consumers). Five story variants added: `copy-chip → ClipboardDenied`, `breadcrumb → LongPathTruncation`, `time → RelativeFutureSkew / RelativeFarPast / RelativeFarFuture`, `upgrade-popover → NotAvailable / StaleCheck / ErrorCheck`, `appearance-section → Frame` snapshots + restores `useUiStore` on unmount. |
| R12 | Plan still `Status: active`; ledger row 12 `pending`; close-time grep gate uncaptured; proof README absent. | Plan flipped to `Status: done`, archived under `docs/plans/archive/`; `docs/plans/README.md` active list no longer references the plan; close-time verifications + grep gates captured below. |

## Close-time verification (R12)

Run from repo root.

```
$ cd packages/nimbus-ui && npx vitest run
 Test Files  37 passed (37)
      Tests  248 passed (248)
   Duration  2.39s
$ cd packages/nimbus-ui && npx tsc -p tsconfig.json --noEmit
(exit 0, zero output)
$ cd packages/nimbus-ui && npm run build
dist/index.html                                                   4.69 kB │ gzip:   1.27 kB
dist/assets/index-… .css                                          ~3 kB
dist/assets/index-CXDrUZtp.js                                   571.77 kB │ gzip: 179.60 kB
(exit 0; pre-existing chunk-size warning unchanged)
$ cargo build -p nimbus-bin
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.28s
$ cargo test -p nimbus-server
test result: ok. 32 passed; 0 failed; 0 ignored
$ cd packages/nimbus-ui && npx playwright test tests/e2e/smoke.spec.ts
  ✓  1 [chromium] › tests/e2e/smoke.spec.ts:112:3 › desktop UI smoke walk › 10-step deterministic walk asserts envelopes and console hygiene (5.4s)
  1 passed (6.0s)
$ make check
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.13s
```

## Close-time grep-gate output

```
$ grep -rn 'as never' packages/nimbus-ui/src packages/nimbus/src \
    --include='*.ts' --include='*.tsx' \
    --exclude='*.spec.ts' --exclude='*.spec.tsx' | wc -l
0

$ grep -rn 'as unknown as ' packages/nimbus-ui/src packages/nimbus/src \
    --include='*.ts' --include='*.tsx' \
    --exclude='*.spec.ts' --exclude='*.spec.tsx' | wc -l
0

$ grep -rn 'as ServiceDoc\[\] | undefined\|as ServiceDoc | null | undefined' \
    packages/nimbus-ui/src --include='*.ts' --include='*.tsx' | wc -l
0

$ grep -rn 'import.*ServiceDoc.*from ".*routes/app' \
    packages/nimbus-ui/src/routes/admin --include='*.ts' --include='*.tsx' | wc -l
0

$ grep -rn 'type ServiceDoc' packages/nimbus-ui/src \
    --include='*.ts' --include='*.tsx' | wc -l
1
  (sole canonical source: packages/nimbus-ui/src/lib/types/service.ts:3)

$ grep -n 'useQuery' \
    packages/nimbus-ui/src/routes/admin/services_.\$service.tsx \
    packages/nimbus-ui/src/routes/app/services_.\$service.tsx \
    packages/nimbus-ui/src/routes/app/compute_.runs_.\$runId.tsx
(no output — 0 hits)

$ grep -n 'router.invalidate' packages/nimbus-ui/src/routes/app/services*.tsx
(no output — 0 hits)

$ grep -c 'export type JsonValue' \
    packages/nimbus-ui/convex/_generated/api.ts \
    packages/nimbus-ui/convex/_generated/dataModel.d.ts \
    packages/nimbus-ui/convex/_generated/scheduled_functions.ts
packages/nimbus-ui/convex/_generated/api.ts:0
packages/nimbus-ui/convex/_generated/dataModel.d.ts:1
packages/nimbus-ui/convex/_generated/scheduled_functions.ts:0
  (single canonical declaration in dataModel.d.ts; the other two import type from there)

$ grep -n 'if (await' packages/nimbus-ui/tests/e2e/smoke.spec.ts
(no output — 0 hits)
```

All gates green. The widened cast gate (now matching `as unknown as` in
addition to `as never`) is the lesson recorded from the A2 vanity-grep
failure: scanning for the cast spelling alone is not enough; the
producer-side wrapper that makes the cast unnecessary is the
load-bearing contract, and the gate must catch every spelling that
would re-introduce one.

## Where the visual baseline lives

This wave shipped no visual changes. The dashboard's pixel identity is
still the predecessor's sealed bundle at
`docs/plans/proof/desktop-ui-followup-hardening/after/` (h7-prefixed
captures from 2026-05-18). The A wave's close pointed at the same
bundle; this wave inherits it unchanged.
