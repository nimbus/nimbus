# Desktop UI — Architecture Hardening (proof bundle)

Proof artifacts for
`docs/plans/archive/desktop-ui-architecture-hardening-plan.md`
(promoted 2026-05-18 from a post-closure critical-reflection review of
the Followup-Hardening wave; closed 2026-05-18 across phases A0–A8).

This wave is **code/architecture only**. Every phase except A6's CI
lane changes the source tree (codegen emit, lifted types, decomposed
routes, hoisted loaders, catalog plumbing, file split) without
altering the rendered UI surface. The predecessor's sealed `h7-*`
capture bundle at
`docs/plans/proof/desktop-ui-followup-hardening/after/` remains the
canonical visual baseline for the current operator console — these
captures still describe the live UI as of plan closure. The proof for
this wave is therefore the four grep gates, the verification commands
below, and the wired CI lane.

## Verifications recorded at A8

- vitest: `cd packages/nimbus-ui && npx vitest run` → 35 files /
  236 tests passing (predecessor closed at 32 / 222; this wave adds
  the settings-decomposition specs, the codegen-typed cast removals
  caught by typecheck, and the smoke-harness CSP-hash test on the
  Rust side).
- typecheck: `cd packages/nimbus-ui && npx tsc -p tsconfig.json
  --noEmit` clean. Codegen runs as part of the gate; the typed
  per-handler return shapes from A1 catch any reintroduced `as never`
  cast at the type level, not just the grep gate.
- build: `cd packages/nimbus-ui && npm run build` clean (only the
  informational `chunkSizeWarningLimit` note from rolldown —
  unchanged from the predecessor; observability route now code-splits
  into two `observability-*.js` chunks post-A7).
- Rust: `cargo build -p nimbus-bin` clean. The new compile-time test
  `inline_fouc_script_hash_matches_csp` in `crates/nimbus-server/src/
  http/ui.rs` recomputes the SHA-256 of the embedded `index.html`
  inline theme-resolution script and asserts identity against the
  hash pinned in `UI_CSP` — any future edit to the inline script
  fails this Rust test before CI sees it.
- e2e smoke:
  `cd packages/nimbus-ui && npx playwright test tests/e2e/smoke.spec.ts`
  → 1 passed (5.2s). The 10-step deterministic walk asserts envelopes
  on `/ui/app/`, `/ui/admin/`, `/ui/app/services`, `/ui/admin/services`,
  `/ui/admin/services/<id>` (or the not-found envelope on the
  synthetic id), `/ui/admin/tenants`, `/ui/app/observability`, and the
  ⌘K command palette. Console hygiene: zero `console.error`, ≤1
  `console.warn` (none observed at close).
- Workspace check: `make check` clean.
- Grep gates (all clean at HEAD):
  - `grep -rn 'as never' packages/nimbus-ui/src` → 0 hits.
  - `grep -rn 'as ServiceDoc\[\] | undefined\|as ServiceDoc | null | undefined' packages/nimbus-ui/src` → 0 hits.
  - `grep -rn 'import.*ServiceDoc.*from ".*routes/app' packages/nimbus-ui/src/routes/admin` → 0 hits.
  - `grep -rn 'type ServiceDoc' packages/nimbus-ui/src` → 1 hit at
    `packages/nimbus-ui/src/lib/types/service.ts:3:export type ServiceDoc = Doc<"services">;`
    (the single canonical definition produced by A2's lift).
  - `grep -c '^' packages/nimbus-ui/src/routes/admin/settings.tsx` → 93
    (well under the ≤900 cap from A3).

## A-phase → after-evidence mapping

| A-phase | Summary | After-evidence |
|---------|---------|----------------|
| **A0** | Read-in + before-state confirmation | Plan execution log entry (b). Cast inventory at promotion confirmed at HEAD: 5× `as never`, 6× `as ServiceDoc[] | undefined`, 1× `as ServiceDoc | null | undefined`. |
| **A1** | Convex codegen typing — emit per-export typed surfaces (`{ default, _typed }`) | Commit `28769f50`. `packages/codegen/` emit walks the registered return shape; `useQuery(api.foo, ...)` returns the function's declared return type directly. Grep gate `'as never'` → 0 hits. |
| **A2** | Lift `ServiceDoc` to `lib/types/services.ts`; drop cross-persona import path | Commit `28769f50`. `routes/admin/index.tsx` and `routes/admin/services_*.tsx` import `ServiceDoc` from `lib/types/services`; the 6× `as ServiceDoc[] | undefined` and 1× `as ServiceDoc | null | undefined` casts removed. Grep gate `import.*ServiceDoc.*from ".*routes/app'` (under admin) → 0 hits. |
| **A3** | Decompose `routes/admin/settings.tsx` (1608 LOC → composition root + concept-owned children) | Commit `d343be08`. `routes/admin/settings.tsx` is now 93 LOC; sibling files under `routes/admin/settings/`: `configuration.tsx`, `deploys.tsx`, `server-info.tsx`, `danger-zone.tsx`. None of the siblings export `Route = createFileRoute(...)`, so the route-tree.gen.ts is unchanged. Visual identity is the predecessor's `h7-` baseline. |
| **A4** | Router-level loaders for five data routes | Commit `f325f669`. `/admin/tenants`, `/admin/services`, `/admin/services/$service`, `/app/services`, `/app/services/$service` use `Route.loader` + `Route.useLoaderData()`. Loading and error states owned by the route. Render-then-fetch flicker eliminated. |
| **A5** | Component catalog vehicle: Storybook (over Ladle) | Commit `745fcd43`. 11 stories under `packages/nimbus-ui/src/stories/`: CopyChip, StateChip, RelativeTime, ConfirmDialog, Breadcrumb, SubDrawer, ScopeChip, CommandPalette, Drawer, EmptyState, ErrorBoundary. `npm run catalog:build` clean. The Storybook ↔ Chromatic upgrade path stays available as named follow-on work. |
| **A6** | CI browser-smoke harness vehicle: playwright-cli (over chrome-devtools-mcp) | Commit `12250fa6`. `packages/nimbus-ui/tests/e2e/smoke.spec.ts` (10-step walk, console hygiene gate). `make verify-desktop-ui` target builds nimbus-bin, builds the UI, and runs the smoke spec. `.github/workflows/desktop-ui.yml` runs on push and PR to relevant paths. CSP regression caught and fixed: inline FOUC script hash pinned in `UI_CSP` with a compile-time drift test (`inline_fouc_script_hash_matches_csp`). |
| **A7** | Large-file audit pass | Commit `11c6a08d`. `routes/app/observability.tsx` SPLIT 978 → 180 LOC root + `observability/{logs.tsx,runs.tsx,types.ts}` (534/306/53 LOC). `routes/app/storage_.$table.tsx` (1154 LOC) and `routes/admin/machines.tsx` (715 LOC) KEPT with explicit justification recorded in plan execution log entry (h): both are under the CLAUDE.md 1500-LOC warning band, have single coherent ownership stories, and their drawers/panels/chrome share state with a single consumer page. |
| **A8** | Verification + close + archive | This README; plan execution log entry (i); ledger flipped to `done`; plan `git mv`'d to `docs/plans/archive/`; `docs/plans/README.md` updated. |

## Visual evidence approach

A1–A7 are code/architecture changes. The rendered UI surface as of
plan closure matches the predecessor's sealed `after/h7-*` bundle at
`docs/plans/proof/desktop-ui-followup-hardening/after/`. Rather than
duplicate identical pixels, this wave's visual baseline reference is
that predecessor bundle. Specifically:

- `h7-app-services-scope-chip.png` — `/app/services` ScopeChip
  `TENANT beta` is the post-A1+A2 visual identity (no cast required
  to render the typed services; visual is identical).
- `h7-admin-tenants-404-envelope.png` — the diagnostic envelope on
  `/admin/tenants` 404 is the post-A4 visual identity (loader-driven
  route-level error state).
- `h7-observability-disabled-chip.png` — Observability tab strip with
  disabled `EVENTS`/`ERRORS` chips. Post-A7 split, both `LOGS` and
  `RUNS` tab bodies still render from the same composition root; the
  disabled chips for `EVENTS`/`ERRORS` are unchanged.
- `h7-admin-services-detail.png` — `/admin/services/$service`
  rendered with the single Placement tab; post-A4 the data arrives
  via `Route.useLoaderData()` but the rendered surface is identical.

The one genuinely new artifact this wave introduces is the CI lane.
The workflow file at `.github/workflows/desktop-ui.yml` and the green
smoke run recorded above stand in for the optional A8 capture
`a8-e2e-lane.png`.

## Out of scope (recorded for traceability, not bundled as proof here)

- **F15 theme-matrix smoke** (Light/Dark/System × Blue/Mono/Warm)
  remains deferred. Owning plan is the verification-tooling plan;
  restate when it promotes.
- **F6 admin service-detail Restarts/Density/Drift** tabs remain
  deferred. Owning plan is the placement-controller /
  restart-audit-log work; the single `Placement` tab stays canonical
  until that lands.
- **Server-side schema changes.** A1 amended only the codegen emit,
  not the registered handler signatures. Handlers whose return shape
  is genuinely `unknown`/`JsonValue` fall back to `JsonValue` and
  keep narrower local types.
- **Loaders for every `useQuery` call site.** A4 was bounded to data
  routes with one or two queries and a route-owned loading state;
  multi-query fan-out pages stay on inline `useQuery` until a
  follow-on plan addresses them.
- **Catalog visual-diff / a11y / interaction tests.** A5 ships the
  catalog and stories only; Chromatic / Percy / a11y scanners are
  named follow-on work.
- **`EventDoc.tenantId` backend surfacing.** Inherited from the
  predecessor's H2(b) "Out of scope"; belongs to a backend plan.

## Screenshot inventory

This wave does not bundle new screenshots. Visual baseline points to
`docs/plans/proof/desktop-ui-followup-hardening/after/` (11 PNGs,
`h7-` prefix). See the mapping above for which `h7-*` capture each
A-phase touches without changing.
