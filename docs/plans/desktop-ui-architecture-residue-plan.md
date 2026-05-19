# Desktop UI — Architecture Residue

Status: active
Promoted: 2026-05-19
Owner: desktop-ui workstream
Predecessor (closed, archived): `docs/plans/archive/desktop-ui-architecture-hardening-plan.md`
Source: 2026-05-19 four-reviewer pass against the A wave's outputs at
HEAD (`0e74b9ee` + `5be6bb48`). Surfaced three blockers the A2/A4 grep
gates did not catch, ~12 cleanup items the wave deliberately deferred
or didn't notice, and four nits.

Related current references:

- `CLAUDE.md` "Modularity thresholds" (still in force; this wave does
  not introduce any new files above 1500 LOC).
- `DESIGN.md` (canonical operator-console design system — left
  unchanged by this plan).
- `docs/plans/archive/desktop-ui-architecture-hardening-plan.md` and
  its proof bundle
  `docs/plans/proof/desktop-ui-architecture-hardening/` (this wave's
  before-state, plus the predecessor `h7-*` visual baseline it points
  to).
- `packages/codegen/src/emit/` (where R1 and R3 land).
- `packages/nimbus-ui/src/routes/{admin,app}/services_.$service.tsx`
  and `packages/nimbus-ui/src/routes/app/compute_.runs_.$runId.tsx`
  (R2 + R4 loader migrations).
- `docs/adapters/convex/ai-guidelines.md` (Convex API guidance still
  applies to R1 + R3).

## Why this plan exists

The Architecture Hardening wave closed on 2026-05-18 with all four
grep gates clean and 236/236 vitest passing. A 2026-05-19 four-stream
code review found three classes of regression the gates didn't detect:

1. **CRITICAL — Vanity grep gate.** The A2 grep `'as never' == 0`
   passed because the cast spelling changed in
   `packages/nimbus-ui/src/shell/nav-entries.ts`. Ten
   `as unknown as CountQuery` double-casts replaced the original
   `as never` calls without delivering the typed
   `QueryEntry<Args, Return>` wrapper A2 promised at the producer
   side. The double-cast is strictly worse than the original cast: it
   routes through `unknown` first, so even the producer's typed args
   are erased. The grep gate became a vanity metric — the contract
   the cast was supposed to enforce was not delivered, only the
   spelling that the gate scanned for. The fix lives in
   `packages/codegen/` and `packages/nimbus/src/browser.ts` (producer
   side) plus `nav-entries.ts` (consumer side); the new gate must
   catch `as unknown as` and any other double-cast spelling.

2. **HIGH — Partial loader migration.** A4 successfully moved
   page-level queries onto `Route.loader`, but the
   `_.$service.tsx` routes (`admin/services_.$service.tsx` at lines
   88 and 200; `app/services_.$service.tsx` at line 100) still inline
   `useQuery` for sibling queries (bundles, machines). The page-header
   bundle chip (admin at lines 153 and 157) renders missing on first
   paint, then pops in — the exact render-then-fetch flicker A4
   advertised it had fixed. Separately,
   `routes/app/compute_.runs_.$runId.tsx` was not loaderized at all
   (still uses `useQuery` at lines 23 and 27); only its
   `id: runId as never` cast got renamed to `as Id<"runs">`. A4's
   claim that "loading and error states are owned by the route" only
   landed for `/admin/tenants`; the other four routes throw raw and
   rely on an outer boundary that isn't asserted in any spec.

3. **HIGH — Inference layer un-specced.** A1 shipped ~150 lines of
   new type-inference logic in
   `packages/codegen/src/emit/type_inference.mjs` with zero unit
   tests for `inferFunctionResultType`, convention-inferred,
   plan-inferred, fallback, or audit-comment emission. Convention-
   fallback (`LIST_EXPORT_NAMES` / `SINGLETON_EXPORT_NAMES` at lines
   138–139) is silently typed-or-not — a query named
   `services:byTenant` falls back to `JsonValue` with no warning and
   no audit comment. The audit-comment block exists but
   `isTrivialValidator` (`schema_types.mjs:35-49`) doesn't unwrap
   `union(v.any(), v.null())`, so the textbook case (`system:status`)
   is treated as `explicit` and slips through audit. Three identical
   `JsonValue` type declarations live in `api.ts`,
   `scheduled_functions.ts`, and `dataModel.d.ts` — one owner, the
   rest should `import type`.

Plus a tail of smaller items the review surfaced:

- `compute_.runs_.$runId.tsx` was *not* loaderized; the cast got
  renamed to `as Id<"runs">` but the route still uses inline
  `useQuery`.
- Four of five A4 routes lack loader-error-envelope spec coverage.
  Only `/admin/tenants` matches the diagnostic-envelope claim.
- `logs.tsx` and `runs.tsx` duplicate `FilterSelect` / `FilterInput`
  (pixel-for-pixel at `logs.tsx:249-319` vs `runs.tsx:195-306`);
  `Th` / `Td` in `runs.tsx:267-306` duplicate the pair in
  `admin/tenants.tsx:391-430`.
- `app/services*.tsx` use `useEffect → router.invalidate()` for
  tenant-switch refetch (lines 37–41 and 94–98), producing a
  one-frame stale render after every switch instead of the proper
  `Route.loaderDeps` subscription.
- `danger-zone.tsx:52, 182` carries two dead `dialogRef`
  declarations verbatim from the 1608-line settings monolith.
- `crates/nimbus-server/src/http/ui.rs:362-374` inline-script
  extraction grabs only the first `<script>` tag — if a future
  inline `<script>` is added it will silently bypass the CSP hash
  pin.
- `.github/workflows/desktop-ui.yml` path filter omits top-level
  `Cargo.toml` / `Cargo.lock` / `rust-toolchain*` — a dep bump that
  affects `nimbus-bin` won't trigger the smoke lane.
- `tests/e2e/smoke.spec.ts` steps 3 and 5 use `if (count) { assert }`
  patterns that pass as no-ops on a fresh fixture; ScopeChip,
  services-table, and placement-tab are not actually verified by the
  smoke gate.
- The five older Storybook stories (`copy-chip`, `breadcrumb`,
  `kbd`, `state-dot`, `state-chip`, `time`) cover happy-path props
  only — no disabled/loading/error/edge variants. The new
  `appearance-section` story mutates the shared `useUiStore` in a
  way that can race across catalog renders.
- Loader-typed `service` in `services_.$service.tsx` reads
  `ServiceDoc | null` even though the loader throws `notFound()`
  first; consumers access `.name`/`.kind` without a guard
  (`:121, :142`). Works at runtime; the types lie.
- `routes/app/observability/types.ts` declares
  `ActiveObservabilityTab = "logs" | "runs"` while the root widens to
  the four-tab union — two parallel unions in one module.
- `inferMutationResultType` emits `Id<"unknown">` when `plan.table` is
  missing (`type_inference.mjs:96`); should throw at codegen time.
- `tenant-selector.tsx:316-317` renders pathname into an
  `aria-live="polite"` sr-only span — looks like debug residue and
  announces every navigation to AT users.

## Outcome

After this wave closes:

- **Zero double-casts** in `packages/nimbus-ui/src` and
  `packages/nimbus/src`. The producer-side wrapper (whether
  `QueryEntry<Args, Return>` or an equivalent typed surface — R1
  picks the exact shape) is the single load-bearing mechanism.
  `shell/nav-entries.ts` consumes it without casts. The grep gate is
  widened from `'as never' == 0` to `'as unknown as ' == 0` plus
  `'as never' == 0` plus the existing typed-cast gates, so the next
  drift in this dimension trips the gate.
- **Five secondary-query sites loaderized**: admin `bundles` and
  `machines` in `routes/admin/services_.$service.tsx`, app `bundles`
  in `routes/app/services_.$service.tsx`, and the full
  `routes/app/compute_.runs_.$runId.tsx` route. Zero inline
  `useQuery` for sibling data on these routes; no `undefined → data`
  flicker on first paint.
- **`packages/codegen/src/emit/type_inference.mjs` has unit-test
  coverage** for the four inference paths (explicit, plan-inferred,
  convention-inferred, fallback) plus audit-comment emission.
  `system:status` (the union-of-trivials case) emits the audit
  comment. Convention fallback either becomes documented contract
  (with an audit comment when it fires) or is dropped — R3 makes the
  call and records it in the execution log.
- **Loader-error envelope** (matching `/admin/tenants` diagnostic
  shape — `storage-server-error-envelope` testid) on all five A4
  routes, with spec coverage for the error path that asserts the
  envelope renders.
- **Shared `_filters.tsx`** (FilterSelect + FilterInput) and shared
  table-cell helpers (`Th` / `Td`) extracted to
  `routes/app/observability/_filters.tsx` and
  `components/table-cells.tsx` (or equivalent). `logs.tsx`,
  `runs.tsx`, and `admin/tenants.tsx` consume the shared primitives.
- **`Route.loaderDeps`** used for tenant-switch invalidation in
  `app/services*.tsx`. Zero `useEffect → router.invalidate()`
  patterns remain.
- **A3 residue**: dead `dialogRef`s removed from `danger-zone.tsx`;
  `settings/sub-drawer.ts` typed `as const satisfies
  StaticSubDrawerSpec<…>` matching the `observability.tsx` pattern.
- **CSP hardening**: `inline_fouc_script_hash_matches_csp` asserts
  *exactly one* inline `<script>...</script>` in the served
  `index.html`; tolerates attribute-bearing tags.
- **CI workflow path filter** includes `Cargo.toml`, `Cargo.lock`,
  and `rust-toolchain*`.
- **Smoke spec is deterministic**: fixture data is seeded before
  the walk so steps 3 and 5 always exercise ScopeChip + services
  table + placement-tab assertions. No `if (count) { assert }`
  conditionals.
- **Catalog stories** cover loading / empty / error / disabled
  variants for the components that have those states. The
  `appearance-section` story cleans up `useUiStore` mutations on
  unmount; `upgrade-popover` includes the `available: false`
  state.
- **Nit pass**: loader return shape is asserted non-null where the
  loader throws `notFound()`, so `useLoaderData()` types match
  runtime guarantees; `ObservabilityTab` is a single union;
  `inferMutationResultType` throws on missing `plan.table`;
  `tenant-selector.tsx` debug `aria-live` is removed or
  commented;`nimbus/browser.ts` re-exports are audited and the
  unused ones dropped.

## Out of scope

- **Production catalog visual-regression / a11y tooling** (Chromatic,
  Percy, axe-core). A5 deferred these as named follow-on; restate
  when the catalog-visual-regression plan promotes.
- **F15 theme-matrix smoke** (inherited from the predecessor). Owning
  plan is the verification-tooling plan.
- **F6 admin service-detail Restarts/Density/Drift tabs** — owned by
  the placement-controller / restart-audit-log work.
- **`EventDoc.tenantId` backend surfacing** — inherited from H2(b);
  belongs to a backend plan.
- **Server-side schema rewrites.** R3 amends the codegen emit and
  adds tests; it does not change registered handler signatures.
- **Loaderizing every `useQuery` call site.** This wave is bounded to
  the four `_.$service.tsx` sibling queries and the
  `compute_.runs_.$runId.tsx` route. Other multi-query fan-out pages
  (Observability `RunsTab`, the various overview pages) stay on
  inline `useQuery` until a dedicated loader-completion plan
  promotes. R-roadmap explicitly enumerates the five sites that
  *are* in scope; the plan does not implicitly grow.
- **Migrating to a Chromatic / Percy ladle replacement.** A5's
  Storybook ↔ Chromatic upgrade path remains the named follow-on.
- **Sub-drawer abstraction across routes.** Each route still owns
  its own `*_SUB_DRAWER` const (settings/observability/etc.). R8's
  typed-const cleanup tightens the settings instance but does not
  unify them.

## Phase status ledger

| Phase | Slice | Status |
|-------|-------|--------|
| R0 | Read-in + before-state freeze | done |
| R1 | Discriminated query wrapper — eliminate `as unknown as` (BLOCKER) | pending |
| R2 | Loaderize `_.$service.tsx` sibling queries (BLOCKER) | pending |
| R3 | Codegen specs + audit-comment fix + `JsonValue` dedup + convention decision (BLOCKER) | pending |
| R4 | Loaderize `compute_.runs_.$runId.tsx` | pending |
| R5 | Loader-error envelope coverage on the four A4 routes | pending |
| R6 | Extract shared filter + table-cell primitives | pending |
| R7 | `Route.loaderDeps` for tenant-switch invalidation | pending |
| R8 | A3 residue cleanup (dead refs, typed sub-drawer) | pending |
| R9 | CSP test tightening + workflow path filter widening | pending |
| R10 | Smoke spec — deterministic fixture seeding | pending |
| R11 | Polish — catalog story state coverage + nit pass | pending |
| R12 | Verification + close + archive | pending |

## Roadmap detail

### R0 — Read-in + before-state freeze

Goal: re-grep the casts/duplications/inline-`useQuery` sites at HEAD,
confirm the review's counts haven't drifted, and seed the proof
directory.

Touch list (reads only):

- `docs/plans/archive/desktop-ui-architecture-hardening-plan.md`
  end-to-end (review the A2/A4 contracts that this wave is
  re-grounding).
- `docs/plans/proof/desktop-ui-architecture-hardening/{README,before}.md`
  (predecessor's after-state IS this wave's before-state for the
  visual surface; for the architecture surface, the casts and
  inline-`useQuery` counts below are the before-state).
- `packages/nimbus-ui/src/shell/nav-entries.ts` end-to-end.
- `packages/nimbus-ui/src/routes/admin/services_.$service.tsx`,
  `packages/nimbus-ui/src/routes/app/services_.$service.tsx`,
  `packages/nimbus-ui/src/routes/app/compute_.runs_.$runId.tsx`.
- `packages/codegen/src/emit/{generated_files,reference_helpers,schema_types,type_inference}.mjs`.

Done when:

- `docs/plans/proof/desktop-ui-architecture-residue/` directory
  exists with a `before.md` recording the exact line numbers and
  counts: 10× `as unknown as CountQuery` in `nav-entries.ts`; 3×
  inline `useQuery` on `_.$service.tsx`-class sibling queries; 2×
  inline `useQuery` on `compute_.runs_.$runId.tsx`; 3× `JsonValue`
  declarations across generated files; etc.
- Zero edits this phase.
- Execution log entry (a) records any drift from the review counts.

### R1 — Discriminated query wrapper (BLOCKER)

Goal: replace all 10 `as unknown as CountQuery` casts in
`shell/nav-entries.ts` with a producer-side typed wrapper that nav
entries consume without casts.

Background: `nav-entries.ts` builds an array of nav entries; each
entry may carry a `countQuery` + `countArgs` pair used by
`primary-drawer.tsx` to render badge counts. The current shape is
`QueryReference<unknown, unknown>` flattened through
`as unknown as CountQuery`, which discards both the args type and the
return type. The natural fix:

```ts
// packages/nimbus/src/browser.ts (producer side)
export type QueryEntry<TArgs, TReturn> = {
  ref: QueryReference<TArgs, TReturn>;
  args: TArgs;
};

export function entry<TArgs, TReturn>(
  ref: QueryReference<TArgs, TReturn>,
  args: TArgs,
): QueryEntry<TArgs, TReturn> {
  return { ref, args };
}
```

with `primary-drawer.tsx` consuming `QueryEntry<unknown, number>`
(unifying over heterogeneous arg shapes by erasing TArgs at the
consumer, not at the call site). R1 picks the exact shape — the goal
is producer-side type safety, not a particular API shape.

Touch list:

- `packages/nimbus/src/browser.ts` — add the wrapper type and a small
  constructor helper. Drop dead re-exports identified by the R14 nit
  audit if low risk (or carry to R11).
- `packages/nimbus-ui/src/shell/nav-entries.ts` — rewrite to use the
  wrapper. Remove all 10 `as unknown as CountQuery` casts.
- `packages/nimbus-ui/src/shell/primary-drawer.tsx` — consume the
  wrapper. Verify no `as never` / `as unknown` returned at the badge
  rendering site.
- Add a new vitest spec asserting that an entry built via the
  producer wrapper preserves both `TArgs` and `TReturn` at the type
  level (a `expectTypeOf(...).toEqualTypeOf<...>` style check, or a
  compile-time `assertType` helper).
- Widen the grep gate documented in the proof bundle to include
  `as unknown as `, `as any`, and the existing `as never`.

Done when:

- `grep -rn 'as unknown as ' packages/nimbus-ui/src packages/nimbus/src`
  → 0 hits.
- `grep -rn 'as never' packages/nimbus-ui/src packages/nimbus/src`
  → 0 hits (unchanged from A2).
- `npx tsc -p packages/nimbus-ui/tsconfig.json --noEmit` clean.
- Vitest covers the new wrapper typing assertion.
- `make check` clean.

### R2 — Loaderize `_.$service.tsx` sibling queries (BLOCKER)

Goal: fold the three sibling `useQuery` calls into the route loader
on the two `_.$service.tsx` routes, eliminating render-then-fetch
flicker on the bundle chip and the placement-machines list.

Touch list:

- `packages/nimbus-ui/src/routes/admin/services_.$service.tsx`
  - Move `bundles` (line 88) and `machines` (line 200) into
    `Route.loader`. Adjust `useLoaderData()` consumers (header at
    lines 153/157; placement tab) to read from the loader result.
- `packages/nimbus-ui/src/routes/app/services_.$service.tsx`
  - Move `bundles` (line 100) into `Route.loader`. Adjust the header
    consumer.
- `packages/nimbus-ui/src/routes/admin/services_.$service.spec.ts`
  - Add a spec that asserts the loader returns
    `{ service, services, bundles, machines }` in one shot and that
    `useLoaderData()` consumers don't flicker through `undefined`.
- `packages/nimbus-ui/src/routes/app/services_.$service.spec.ts`
  - Same shape for `{ service, services, bundles }`.

Done when:

- `grep -rn 'useQuery' packages/nimbus-ui/src/routes/admin/services_.\$service.tsx packages/nimbus-ui/src/routes/app/services_.\$service.tsx`
  → 0 hits.
- New specs cover the loader-returns-all-siblings path.
- Visual check: page-header bundle chip renders without
  `undefined → data` flicker on cold load (manual or via the smoke
  spec at R10).
- `make check` clean.

### R3 — Codegen specs + audit-comment + JsonValue dedup + convention decision (BLOCKER)

Goal: spec the A1 inference layer, fix the `system:status` audit
gap, deduplicate the `JsonValue` declarations, and make the
convention-fallback layer either typed-with-audit or removed.

Background: A1 shipped ~150 lines of inference logic with zero unit
tests outside its one downstream consumer. The audit-comment
infrastructure exists but `isTrivialValidator` doesn't unwrap
`union(v.any(), v.null())`, so the only real-world handler that
should audit (`system:status`) slips through. Three `JsonValue`
declarations live in `api.ts`, `scheduled_functions.ts`, and
`dataModel.d.ts`.

Touch list:

- `packages/codegen/src/emit/type_inference.mjs`
  - Widen `isQueryShape` if needed (the structural test currently
    requires both `order` and `limit` — brittle).
  - Decision: keep the convention-inference layer (`LIST_EXPORT_NAMES`
    / `SINGLETON_EXPORT_NAMES`) and emit an audit comment when it
    fires, OR drop the layer and always read from the plan shape.
    R3 picks one and records the rationale in the execution log.
  - Fix `inferActionResultType` to preserve inference source when
    recursing into a wrapped fallback handler (currently
    `type_inference.mjs:125-126` discards source).
  - Throw at codegen time when `inferMutationResultType` sees a
    missing `plan.table` (currently emits `Id<"unknown">` — see N4).
- `packages/codegen/src/emit/schema_types.mjs`
  - Widen `isTrivialValidator` (lines 35–49) to unwrap unions whose
    members are all trivial. After the change,
    `union(v.any(), v.null())` is treated as trivial and the audit
    comment fires for `system:status`.
- `packages/codegen/src/emit/generated_files.mjs`
  - Move `JsonValue` to live in `dataModel.d.ts` only.
  - `api.ts` and `scheduled_functions.ts` import it via
    `import type { JsonValue } from "./dataModel"`.
- `packages/codegen/src/selftest.mjs` (or a new
  `packages/codegen/src/emit/type_inference.spec.mjs` if the harness
  takes spec files) — add coverage:
  - Explicit return type (validator-typed).
  - Plan-inferred for query (with `isQueryShape`).
  - Plan-inferred for mutation (insert / update / patch).
  - Convention-inferred (if the layer is kept).
  - Fallback to `JsonValue` (or `unknown` per the R3 decision).
  - Audit-comment emission for the union-of-trivials case.
- `packages/nimbus-ui/convex/_generated/api.ts` — regenerated. The
  expected diff: audit-comment block on `system:status`; one
  `JsonValue` definition removed; imports added.

Done when:

- New codegen specs cover all four inference paths plus audit-comment
  emission. Each spec asserts an exact emitted-text shape — not just
  "didn't throw".
- `grep -c 'export type JsonValue' packages/nimbus-ui/convex/_generated/*.{ts,d.ts}`
  → 1.
- `packages/nimbus-ui/convex/_generated/api.ts` contains an
  audit-comment block adjacent to the `system:status` emission.
- Decision recorded in execution log entry (c).
- `npm run typecheck` clean. `make check` clean.

### R4 — Loaderize `compute_.runs_.$runId.tsx`

Goal: migrate the run-detail route to `Route.loader` and drop the
`as Id<"runs">` cast where the typed param flows from the route
match.

Touch list:

- `packages/nimbus-ui/src/routes/app/compute_.runs_.$runId.tsx`
  - Move `useQuery(api.runs.byId, { runId })` (current
    `useQuery` at line 23) and the related companion query (line 27)
    into the route loader.
  - Consume via `Route.useLoaderData()`.
  - Drop the `as Id<"runs">` cast at line 24; the typed loader
    surfaces the right ID type.
- Spec: add a happy-path + not-found loader spec under
  `routes/app/compute_.runs_.$runId.spec.ts`.

Done when:

- `grep -n 'useQuery' packages/nimbus-ui/src/routes/app/compute_.runs_.\$runId.tsx`
  → 0 hits.
- Loader spec covers happy + not-found paths.
- Visual check: no `undefined → data` flicker on first paint.
- `make check` clean.

### R5 — Loader-error envelope coverage

Goal: bring the four A4 routes that lack diagnostic-envelope coverage
up to the `/admin/tenants` standard, both in render code and in
specs.

Background: only `routes/admin/tenants.spec.tsx:139-178` asserts the
diagnostic envelope renders when the loader rejects. The other four
A4 routes have a happy-path spec but no error spec; the routes
themselves rely on whatever outer boundary surrounds the router
without asserting the rendered output.

Touch list:

- `packages/nimbus-ui/src/routes/admin/services.tsx` — add an error
  component (`Route.errorComponent`) or a `try/catch` in the loader
  that emits the diagnostic envelope shape.
- `packages/nimbus-ui/src/routes/admin/services_.$service.tsx` —
  same.
- `packages/nimbus-ui/src/routes/app/services.tsx` — same.
- `packages/nimbus-ui/src/routes/app/services_.$service.tsx` — same.
- Specs: extend each of the four `.spec.ts` files (and
  `.spec.tsx` for the parent) with a loader-rejects test asserting
  the diagnostic envelope renders, mirroring
  `tenants.spec.tsx:139-178`.

Done when:

- All five A4 routes (the four above plus the existing
  `/admin/tenants`) render an identifiable diagnostic envelope on
  loader rejection.
- Specs exercise the loader-error path on each.
- `make check` clean.

### R6 — Extract shared filter + table-cell primitives

Goal: deduplicate `FilterSelect`, `FilterInput`, `Th`, `Td` across
`logs.tsx`, `runs.tsx`, and `admin/tenants.tsx`.

Touch list:

- Create `packages/nimbus-ui/src/routes/app/observability/_filters.tsx`
  exporting `FilterSelect` + `FilterInput` (the
  observability-specific variants; the `_` prefix matches the
  TanStack convention for non-route siblings).
- Create `packages/nimbus-ui/src/components/table-cells.tsx`
  (or reuse an existing primitives module) exporting `Th` + `Td`.
- `logs.tsx`, `runs.tsx`, `admin/tenants.tsx` consume the shared
  primitives.
- Verify no behavior or styling regressions; the smoke spec at R10
  catches gross regressions.

Done when:

- `grep -n 'function FilterSelect' packages/nimbus-ui/src/routes/app/observability/`
  → exactly 1 (in `_filters.tsx`).
- Same for `FilterInput`. Same for `Th` / `Td` (1 source).
- `make check` clean.

### R7 — `Route.loaderDeps` for tenant-switch invalidation

Goal: replace `useEffect → router.invalidate()` patterns with
`Route.loaderDeps` so loader reruns are driven by the router's
subscription model, not a post-mount side effect.

Touch list:

- `packages/nimbus-ui/src/routes/app/services.tsx` (lines 37–41).
- `packages/nimbus-ui/src/routes/app/services_.$service.tsx`
  (lines 94–98).
- For each, define `Route.loaderDeps: () => ({ activeTenant: ... })`
  reading from `useUiStore.getState()` (or whichever source the
  current `useEffect` reads). The router will rerun the loader on
  `activeTenant` change without the one-frame stale render.
- Confirm that no other consumer site depends on the `useEffect`
  being there for non-tenant reasons.

Done when:

- `grep -n 'router.invalidate' packages/nimbus-ui/src/routes/app/services*.tsx`
  → 0 hits.
- Loader reruns on tenant change; verified in spec or via the smoke
  spec.
- `make check` clean.

### R8 — A3 residue cleanup

Goal: close out the small bits A3 carried forward.

Touch list:

- `packages/nimbus-ui/src/routes/admin/settings/danger-zone.tsx`
  - Delete `dialogRef = useRef<HTMLDivElement>(null)` and its forward
    on `<DialogShell ref={dialogRef} ...>` at lines 52 and 182. The
    DialogShell manages its own focus return via
    `previouslyFocusedRef`.
- `packages/nimbus-ui/src/routes/admin/settings/sub-drawer.ts`
  - Retype the export as
    `as const satisfies StaticSubDrawerSpec<"general"|"endpoints"|...>`
    matching the `observability.tsx:65` pattern. The exact union
    members come from the existing item ids.
- Optional consideration (record decision in execution log entry
  (f)): rename `settings/hooks.ts` → `settings/debug-snapshots.ts`
  to be concept-named per CLAUDE.md guidance, and inline `Cell` /
  `DialogShell` into their single-consumer siblings if R8 chooses
  to. The plan does not mandate either rename; the execution log
  records the call.

Done when:

- Both `dialogRef` declarations removed.
- `sub-drawer.ts` carries the typed-const annotation.
- `make check` clean.

### R9 — CSP test tightening + workflow path filter widening

Goal: prevent the two CI-side blind spots the review flagged.

Touch list:

- `crates/nimbus-server/src/http/ui.rs`
  - Adjust the test (currently
    `inline_fouc_script_hash_matches_csp`) to assert *exactly one*
    inline `<script>...</script>` element in the embedded
    `index.html`, and to tolerate attribute-bearing open tags
    (`<script ...>...</script>`).
  - Add a comment on the `style-src 'unsafe-inline'` line explaining
    the Tailwind/runtime-styles rationale so a future reviewer
    doesn't tighten it blindly.
- `.github/workflows/desktop-ui.yml`
  - Add `Cargo.toml`, `Cargo.lock`, `rust-toolchain*` (and any other
    files that affect a `nimbus-bin` build but currently don't
    trigger the workflow) to the on-push/on-pr `paths:` filter.

Done when:

- Adding a second inline `<script>...</script>` to `index.html`
  fails the Rust test (verified manually by temporary edit, reverted
  before commit).
- A dep bump in `Cargo.toml` triggers the workflow on a test branch.
- `cargo test -p nimbus-server` clean. `make check` clean.

### R10 — Smoke spec — deterministic fixture seeding

Goal: remove the `if (count) { assert }` patterns from
`tests/e2e/smoke.spec.ts` so the smoke gate actually verifies the
envelopes it walks past.

Touch list:

- `packages/nimbus-ui/tests/e2e/smoke.spec.ts`
  - Seed one tenant and one service via `page.request.post(...)`
    (or via direct HTTP to the running `nimbus-server` instance)
    before the walk begins.
  - Replace the conditional assertions at steps 3 and 5 with
    unconditional assertions on the seeded state.
- `packages/nimbus-ui/tests/e2e/fixtures/nimbus-server.ts`
  - If `NIMBUS_E2E_BIN` resolution turns out to be flaky, resolve it
    relative to `import.meta.url` instead of `process.cwd()`.
- Optional: detect platform (`process.platform`) and press the
  correct meta/ctrl key for `⌘K` instead of relying on the
  meta-then-ctrl fallback.

Done when:

- No `if (count)` / `await expect(...).toBeVisible({ timeout: ... })`
  conditional bypass patterns remain in the smoke spec.
- A fresh fixture run exercises ScopeChip, services table, and
  placement-tab assertions without skipping.
- `make verify-desktop-ui` clean.

### R11 — Polish — catalog story state coverage + nit pass

Goal: tighten the remaining items.

Touch list — catalog (covers MAJORs N1–N4 from the catalog reviewer):

- `packages/nimbus-ui/src/stories/copy-chip.stories.tsx` — add
  clipboard-denied variant.
- `packages/nimbus-ui/src/stories/breadcrumb.stories.tsx` — add
  long-path / truncation variant.
- `packages/nimbus-ui/src/stories/time.stories.tsx` — add past +
  future + far-future skew variants.
- `packages/nimbus-ui/src/stories/upgrade-popover.stories.tsx` —
  add `available: false` (no-upgrade) story and a `checkStatus:
  "stale"` / `"error"` variant.
- `packages/nimbus-ui/src/stories/appearance-section.stories.tsx`
  — reset `useUiStore` mutations on unmount so side-by-side catalog
  renders don't race.

Touch list — nit pass:

- `packages/nimbus-ui/src/routes/admin/services_.$service.tsx`
  and `packages/nimbus-ui/src/routes/app/services_.$service.tsx`
  — assert non-null in the loader return so `useLoaderData()` shows
  `service: ServiceDoc` (not `ServiceDoc | null`) in consumers. The
  loader already throws `notFound()` when service is missing; the
  type-level guarantee follows from a `return { service: service!,
  ... }` after the guard.
- `packages/nimbus-ui/src/routes/app/observability/types.ts` —
  collapse `ActiveObservabilityTab` and the implicit four-tab union
  into one canonical union; consumers (`DisabledTab` /
  `ActiveTabLink`) narrow as needed via `disabled` predicate.
- `packages/nimbus-ui/src/shell/tenant-selector.tsx` lines 316–317 —
  decide on the `aria-live` pathname announcement: remove if it's
  debug residue, or add a comment explaining the accessibility
  rationale.
- `packages/nimbus/src/browser.ts` — drop the unused re-exports
  identified at the R0 audit (anything in the +9-line export block
  not actually consumed by any of `packages/nimbus-ui/src`,
  `packages/convex/src`, or any other workspace consumer).

Done when:

- Older stories have at least one non-happy-path variant per
  component where state semantics make one meaningful.
- `useLoaderData()` on `services_.$service.tsx` (both) types
  `service` as non-nullable in route consumers.
- `ActiveObservabilityTab` has one canonical declaration in
  `types.ts`.
- `nimbus/browser.ts` re-export block is audited; only consumed
  symbols remain.
- `make check` clean.

### R12 — Verification + close + archive

Goal: run the full close gate, write the proof bundle, archive the
plan.

Touch list:

- `docs/plans/proof/desktop-ui-architecture-residue/README.md` —
  verification commands at close, R0–R11 → after-evidence mapping
  (most rows are code/architecture; visual identity remains the
  predecessor's sealed `h7-*` bundle).
- `docs/plans/proof/desktop-ui-architecture-residue/before.md` —
  finalize from R0.
- `docs/plans/desktop-ui-architecture-residue-plan.md` — flip
  `Status: active` → `Status: done`, ledger rows all `done`, append
  execution log entry (m) describing close.
- `git mv` into `docs/plans/archive/`.
- `docs/plans/README.md` — remove the active entry; add the
  archived-baseline entry under the A wave's blurb.

Done when:

- All twelve R-phase ledger rows are `done`.
- Verification artifacts recorded:
  - `cd packages/nimbus-ui && npx vitest run` → pass count recorded
    (expected ≥236).
  - `cd packages/nimbus-ui && npx tsc -p tsconfig.json --noEmit`
    clean.
  - `cd packages/nimbus-ui && npm run build` clean.
  - `cargo build -p nimbus-bin` clean.
  - `cargo test -p nimbus-server` clean (includes the tightened
    inline-script test from R9).
  - `cd packages/nimbus-ui && npx playwright test
    tests/e2e/smoke.spec.ts` → pass.
  - `make check` clean.
  - Workspace grep gates (all should be zero hits):
    - `grep -rn 'as never' packages/nimbus-ui/src packages/nimbus/src`
      → 0
    - `grep -rn 'as unknown as ' packages/nimbus-ui/src packages/nimbus/src`
      → 0
    - `grep -rn 'as ServiceDoc\[\] | undefined\|as ServiceDoc | null | undefined' packages/nimbus-ui/src`
      → 0
    - `grep -rn 'import.*ServiceDoc.*from ".*routes/app' packages/nimbus-ui/src/routes/admin`
      → 0
    - `grep -rn 'type ServiceDoc' packages/nimbus-ui/src` → 1
    - `grep -n 'useQuery' packages/nimbus-ui/src/routes/admin/services_.\$service.tsx packages/nimbus-ui/src/routes/app/services_.\$service.tsx packages/nimbus-ui/src/routes/app/compute_.runs_.\$runId.tsx`
      → 0
    - `grep -n 'router.invalidate' packages/nimbus-ui/src/routes/app/services*.tsx`
      → 0
    - `grep -c 'export type JsonValue' packages/nimbus-ui/convex/_generated/*.{ts,d.ts}`
      → 1
- Plan moved to archive; README pointer updated.

## Verification approach

This wave is code/architecture + tests. Visual identity is unchanged
from the A wave's close, which itself points to the predecessor's
sealed `h7-*` bundle. R10's smoke spec is the only place where
rendered output is asserted on a live server; everything else is
unit-level (vitest), type-level (tsc), or static (grep + cargo
test).

The grep gates above are the wave's contract; R12 makes them the
close-time checklist. Crucially, R1 widens the cast gate from
`'as never' == 0` to also catch `'as unknown as '` — the lesson
from A2 was that a gate that scans for one cast spelling can be
satisfied by changing the spelling. The widened gate makes the
producer-side wrapper load-bearing.

R3's codegen specs are the contract that prevents the inference
layer from silently regressing. The audit-comment emission test in
particular is the gate against another `system:status`-shaped
fallback slipping through.

R5's loader-error envelope specs are the contract that makes the
"loading and error states owned by the route" claim from A4
actually verifiable.

## Stop / re-plan triggers

Pause and re-plan if any of the following:

- R1's producer-side wrapper turns out to require a TanStack Router
  or Convex SDK change. The wave's scope is consumer-side; if a
  vendor change is needed, surface it and decide whether to lift R1
  out of this wave.
- R2's loader migration uncovers a tenant-scoping bug in the bundle
  query (the consumer renders against the active tenant; the loader
  needs to thread the tenant in). If so, R2 may need to fold the
  `loaderDeps` pattern earlier than R7.
- R3's convention-inference decision (keep with audit vs. drop)
  changes the emitted shape of `api.ts` in a way that ripples into
  any route's typing. The change should still be zero-cast at the
  consumer; if it isn't, R3 has missed a case.
- R5's diagnostic-envelope pattern doesn't compose with TanStack
  Router's error-component model in some way the predecessor didn't
  exercise. If so, document the workaround in the proof bundle.
- Any phase exceeds a 1500-LOC file growth in the touch list.
  CLAUDE.md modularity thresholds still apply.

## Execution log

(a) **R0 — Read-in + before-state freeze (2026-05-19).** Re-grepped
the cast/duplication/inline-`useQuery`/conditional-assertion counts
at HEAD `e11cc9ef`. Two minor deltas from the plan body:

- `shell/nav-entries.ts` `as unknown as CountQuery` count is **9**,
  not 10. R1's contract is unchanged — drive to zero in non-spec
  files. The nine lines are 50, 59, 68, 77, 95, 134, 143, 152, 161.
- Smoke spec conditional-bypass sites are **three**, not two: lines
  87, 91, 111 of `tests/e2e/smoke.spec.ts`. R10 unconditionalizes
  all three.

All other counts match the review:

- `as never` count zero (A2 gate clean).
- `type ServiceDoc` single source at `lib/types/service.ts:3`.
- 3× inline `useQuery` on `routes/admin/services_.$service.tsx`
  (lines 88, 200 sibling queries + import).
- 1× sibling-query inline `useQuery` on
  `routes/app/services_.$service.tsx:100` (plus import — total 2).
- 2× inline `useQuery` on `routes/app/compute_.runs_.$runId.tsx`
  (lines 23, 27 — route not loaderized at all).
- 2× `useEffect → router.invalidate()` (app/services.tsx:39,
  app/services_.$service.tsx:96).
- 3× `JsonValue` declarations (api.ts:5, dataModel.d.ts:4,
  scheduled_functions.ts:5).
- 4× `dialogRef` lines in danger-zone.tsx (52, 101, 182, 223).
- 2× `ActiveObservabilityTab` parallel-union sites.
- Workflow path filter missing `Cargo.toml`, `Cargo.lock`,
  `rust-toolchain*`.

Proof bundle directory created at
`docs/plans/proof/desktop-ui-architecture-residue/` with `before.md`
written. Zero source edits this phase. Ledger flipped pending → done
for R0 at close of phase.
