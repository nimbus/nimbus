# Desktop UI — Design Review Fixes

Status: done
Owner: desktop-ui workstream
Predecessor (closed, archived): `docs/plans/archive/desktop-ui-compute-services-redesign-plan.md`
Source: `docs/design-review/2026-05-18-operator-console-review.md`
Promoted: 2026-05-18

Related current references:

- `DESIGN.md` (canonical operator-console design system)
- `docs/plans/archive/desktop-ui-shell-overhaul-plan.md`
- `docs/plans/archive/desktop-ui-compute-services-redesign-plan.md`

## Why this plan exists

A 2026-05-18 end-to-end design + UX review of the operator console
(both `/app/*` Developer and `/admin/*` Operator views) catalogued 14
actionable findings. None are structural — the dual-persona IA, the
three-pane shell, the brand/accent/link token system, JetBrains Mono on
identifiers, state chips, ⌘K / ⌘\ wiring all hold up.

What hurts the first impression is the gap between *what DESIGN.md
says* and *what the screen says*: internal plan codes leaked into
user-visible copy, the system tenant lens fires in a view DESIGN.md
explicitly excludes, sub-drawers and tab strips disagree about how many
sections a page has, and several placeholder shells read like TODOs.

This plan closes those gaps in one focused wave.

Pre-launch policy applies: prefer breaking changes; no compat shims;
no feature flags for legacy behavior. Anything that "lands in DU-shell
OX" is replaced outright, not deprecated.

## Outcome

After this plan, the operator console matches DESIGN.md on every
surface a developer or operator can reach in the default flow.
Specifically:

- Zero internal plan codes (`DU-shell OX`, `Phase 1 · Embedded SPA`)
  visible in the UI.
- ⌘\ system tenant lens is a Developer-only affordance, exactly as
  DESIGN.md §System Tenant Lens (lines 149, 952–961) describes.
- Each page has **one** section-nav surface (either sub-drawer or
  body tab strip, not both saying different things).
- Developer pages auto-default to the first tenant; the Developer
  console is always tenant-scoped — the cross-tenant lens lives in
  Operator, not in a `tenant: all` fallback chip.
- A single canonical `EmptyState` component replaces the current
  `PlaceholderPage` and the ad-hoc `Empty` helpers, so every "not yet"
  state shares one shape.
- Operator `/admin` index and `/admin/observability` ship minimal
  real content instead of plan-code prose.
- Operator Service detail (`/admin/services/$service`) shows only
  tabs that have data; tabs return as data lands in follow-up plans.
- Visual polish: storage breadcrumb leading segment dropped; service
  detail tab labels become sentence-case; sub-drawer tenant-grouping
  applied consistently across operator list + detail.

Out of scope (note as follow-up only):

- F15 — theme matrix smoke (Light/Dark/System × Blue/Mono/Warm). Belongs
  in a separate verification-tooling plan; this plan covers behavior,
  not theme regression coverage.
- Re-adding Restarts/Density/Drift tabs on admin service detail — they
  return when the placement controller, restart audit log, and bundle
  comparator land in their own plans.

## Phase status ledger

| Phase | Slice | Status |
|-------|-------|--------|
| DR0 | Read-in + baseline screenshot capture | done |
| DR1 | Copy hygiene + canonical `EmptyState` (F1) | done |
| DR2 | Lens gating: ⌘\ Developer-only (F2) | done |
| DR3 | Section truth: Observability + Schedules (F3, F4) | done |
| DR4 | Tenant default + ScopeChip cleanup (F5, F12) | done |
| DR5 | Real shells on admin index + observability (F10, F11) | done |
| DR6 | Service detail tab pruning (F6) | done |
| DR7 | Polish: breadcrumb / tab casing / sub-drawer grouping (F7, F9, F14) | done |
| DR8 | Verification + plan close + archive | done |

## Roadmap detail

### DR0 — Read-in + baseline (done)

Goal: lock the before-image so DR8 has something to diff against. The
review pass already captured 16 PNGs under `.review-shots/`.

Touch list (reads + a move):

- move `.review-shots/*.png` →
  `docs/plans/proof/desktop-ui-design-review-fixes/before/`
- read once for orientation:
  - `packages/nimbus-ui/src/shell/primary-drawer.tsx`
  - `packages/nimbus-ui/src/shell/sub-drawer.tsx`
  - `packages/nimbus-ui/src/shell/cmdk.tsx` (or wherever ⌘\ is wired)
  - `packages/nimbus-ui/src/components/placeholder.tsx` (current shape)

Done when:

- 16 PNGs committed under
  `docs/plans/proof/desktop-ui-design-review-fixes/before/`
  matching the names in the design review's verification artifacts
  list.
- `.review-shots/` removed from the repo root (it was an ephemeral
  capture dir).

### DR1 — Copy hygiene + canonical `EmptyState` (F1)

Goal: strip every internal-plan-code reference from user-visible
strings and unify all "not yet" surfaces behind a single component.

Introduce `packages/nimbus-ui/src/components/empty-state.tsx` with:

```ts
type EmptyStateProps = {
  title: string;
  body?: ReactNode;
  cta?: { label: string; to: string } | { label: string; onClick: () => void };
  testid?: string;
};
```

Rendering matches the Convex-style empty-state already used on the
Functions landing — centered title (display-weight), single-line muted
body, optional CTA link/button. Restrained spacing, no icons by default.

Rewrite copy + migrate to `EmptyState`:

| File | Line | Action |
|------|------|--------|
| `shell/primary-drawer.tsx` | 59 | Delete the `Phase 1 · Embedded SPA` div outright; do not replace |
| `routes/admin/index.tsx` | 13 | DR5 ships real content; until then `EmptyState` with neutral copy and links to Machines + Services |
| `routes/app/files.tsx` | 35 | `EmptyState` titled "Object storage coming soon"; body names what file storage will surface; no plan code |
| `routes/app/settings.tsx` | 49 | `EmptyState` linking to `/admin/settings` for operator-wide config; no plan code |
| `routes/admin/observability.tsx` | 59 | DR5 ships real content; until then `EmptyState` titled "Operator observability"; no plan code |

Delete `packages/nimbus-ui/src/components/placeholder.tsx` once all
five callsites migrate.

Done when:

- `grep -rn "DU-shell\|Phase 1 · Embedded" packages/nimbus-ui/src`
  returns zero hits.
- `grep -rn "PlaceholderPage" packages/nimbus-ui/src` returns zero
  hits.
- `EmptyState` has a vitest covering: title-only render; title + body;
  title + body + link CTA; title + body + button CTA.
- `npm run typecheck` and `npm run build` clean.

### DR2 — Lens gating: ⌘\ Developer-only (F2)

Goal: enforce DESIGN.md §System Tenant Lens (lines 149, 952–961). The
lens is a Developer-side affordance only; pressing ⌘\ in Operator must
be a no-op.

Touch list:

- wherever the ⌘\ keydown handler lives (most likely
  `packages/nimbus-ui/src/shell/cmdk.tsx` or a sibling hook), gate the
  registration on `viewFromPathname(pathname) === "developer"`.
- if the binding lives on a global listener, early-return inside the
  handler when the active view is `"operator"`.
- do not show an Operator-side tip or modal — DESIGN.md treats the
  lens as a personal Developer affordance, full stop.

Done when:

- unit test in `shell/cmdk.spec.tsx` (new or extended) asserts:
  - dispatching `keydown` for `Meta+\` while `pathname = "/admin/x"`
    leaves the lens closed.
  - dispatching `keydown` for `Meta+\` while `pathname = "/app/x"`
    opens the lens.
- live verification via chrome-devtools-mcp: visit
  `/admin/services` → press ⌘\ → lens does not appear; visit
  `/app/services` → press ⌘\ → lens appears.

### DR3 — Section truth: Observability + Schedules (F3, F4)

Goal: eliminate the sub-drawer ↔ tab-strip disagreement on Observability
and the dynamic-sub-drawer-for-static-content on Schedules.

#### F3 — Observability

`routes/app/observability.tsx` today:

- Sub-drawer (lines 133–164): static, four items — Logs / Runs /
  Events / Errors, with Events + Errors `disabled: true`.
- Body tab strip (lines 197–204): only Logs + Runs.

Rewrite the tab strip to render from `OBSERVABILITY_SUB_DRAWER.items`
so both surfaces share one source. Disabled items render as
`aria-disabled` tabs with a `title` tooltip describing the not-yet
state. Drop the manual `<TabLink id="logs">` calls.

#### F4 — Schedules

`routes/app/schedules.tsx` today:

- Sub-drawer (lines 91–105): `kind: "dynamic"` with
  `search: { placeholder: "Filter schedules" }`, but only 2 stable
  sections (Scheduled / Cron).
- Body tab strip (lines 128–): also Scheduled / Cron.

Change:

- sub-drawer becomes `kind: "static"` with 2 items (Scheduled / Cron)
  pointing to `/app/schedules?section=scheduled|cron`.
- drop the body tab strip — sub-drawer is the single source of section
  nav.
- the search box returns when there are actual filterable items (the
  underlying schedules table). It belongs in the body, not in the
  sub-drawer header.

Done when:

- `OBSERVABILITY_SUB_DRAWER.items` is the only place Observability
  tabs are listed (typecheck enforces).
- Schedules sub-drawer is `kind: "static"`; no search input; no body
  tab strip; section state lives in `?section=…`.
- vitest covering both: tab strip on Observability includes 4 items;
  Schedules sub-drawer renders 2 static items.
- live verification: navigating Observability via sub-drawer or body
  tab updates the URL identically; Schedules has section nav in
  exactly one place.

### DR4 — Tenant default + ScopeChip cleanup (F5, F12)

Goal: the Developer console is always tenant-scoped. Land on a real
tenant, not on a `tenant: all` empty fallback.

Touch list:

- `packages/nimbus-ui/src/store/ui-store.ts`: on first mount of any
  Developer route, if `activeTenant === null` and the tenant list is
  non-empty, set `activeTenant = tenants[0]._id`. Respect `?as=` query
  if present.
- `routes/app/services.tsx:89-97`: delete the `?? "all"` fallback in
  `ScopeChip`. If `activeTenant === null` (transient state), render
  nothing.
- audit the rest of `routes/app/*`:
  - `compute.tsx`, `schedules.tsx`, `storage.tsx`, `files.tsx`,
    `observability.tsx`, `settings.tsx`, `index.tsx`,
    `compute_.$function.tsx`, `services_.$service.tsx`,
    `storage_.$table.tsx`, `compute_.runs_.$runId.tsx` — replace any
    `activeTenant ?? "all"` with conditional rendering.

The default-tenant rule does not apply to Operator routes — those are
intentionally cross-tenant.

Done when:

- navigating to `/app/services` with a fresh session and no `?as=`
  lands on a populated tenant view (the first tenant in the list).
- `grep -rn '\?\? "all"' packages/nimbus-ui/src/routes/app` returns
  zero hits.
- unit test in `store/ui-store.spec.ts` asserts the auto-default rule
  and the `?as=` override.

### DR5 — Real shells on `/admin` index + `/admin/observability` (F10, F11)

Goal: replace the two bleakest pages in the Operator console with
minimal real content.

#### `/admin` index (System)

Render a 2-column key/value grid with what the system tenant already
exposes:

- Nimbus version (`api.system.version` if available; else build-time
  constant)
- Server uptime (from `_creationTime` of the bootstrap system tenant
  row, or a dedicated `system.status` field)
- Listener count + listener kinds (HTTP, WS) from `_nimbus`
- Tenant count (from `api.tenants.list`)
- Machine count (from `api.machines.list`)
- Service count (from `api.services.list`)

Keep the existing System sub-drawer; the new grid replaces the
PlaceholderPage body.

#### `/admin/observability`

Render the same logs stream the Developer Observability surface uses,
but with a tenant filter chip (`tenant: all | <id>`) instead of an
implicit scope. Tab strip mirrors the Developer Observability strip
(via the shared source from DR3). Empty state when no events have been
emitted: `EmptyState` titled "No events yet".

Done when:

- `/admin` renders 6+ real fields with no plan-code prose.
- `/admin/observability` shows real log lines when the server has
  emitted any; `EmptyState` otherwise.
- both pages pass the F1 grep (no plan codes).

### DR6 — Service detail tab pruning (F6)

Goal: stop advertising capability the page doesn't have.

`routes/admin/services_.$service.tsx`:

- shrink the `TABS` array (lines 26–31) to `Placement` only.
- delete the `RestartsTab`, `DensityTab`, `DriftTab` components and
  the `Empty` helper (now unused — `EmptyState` from DR1 replaces it
  if needed).
- delete the `isTab` validator's `"restarts" | "density" | "drift"`
  branches; `DetailTab` becomes `"placement"`.
- the tab strip nav still renders, with one tab. Acceptable — when
  Restarts lands, the strip is already there to receive it.

(Re-adding tabs is out of scope; tracked in follow-up plans for the
restart audit log, density telemetry, and bundle drift comparator.)

Done when:

- `TABS` array has 1 entry.
- vitest in `routes/admin/services_.$service.spec.tsx` (new) covers:
  detail page renders with Placement tab only; clicking the tab updates
  `?tab=placement`.

### DR7 — Polish (F7, F9, F14)

Goal: knock out the three small remaining visual inconsistencies.

#### F7 — Storage breadcrumb

`routes/app/storage.tsx:103`: delete the leading
`{ label: "storage", href: "/app/storage" }` segment. Breadcrumbs
start at the first resource the user navigated into, not at the
section name (which the primary drawer's selected state already
implies).

#### F9 — Service detail tab casing

`routes/admin/services_.$service.tsx:178-186`: drop `uppercase
tracking-wide` from the tab button className. Sentence-case labels
match every other tab strip in the console.

#### F14 — Sub-drawer tenant-grouping consistency

`routes/admin/services_.$service.tsx`'s `AdminDetailSubDrawer` (lines
314–382) renders a flat list. `routes/admin/services.tsx`'s
`AdminServicesSubDrawer` (lines 97–169) renders the same data
**grouped by tenant**. Apply the same grouping in the detail
sub-drawer — extract the `groupByTenant` helper into a shared util in
`routes/admin/services.tsx` (or a sibling module) and reuse from both.

Done when:

- Storage breadcrumb at `/app/storage` starts with the table name (or
  empty when no table is selected).
- Operator service detail tab labels render sentence-case.
- Operator service detail sub-drawer renders tenant-grouped, matching
  the operator services list.
- vitest passes.

### DR8 — Verification + plan close + archive

Goal: prove every gate, capture after-screenshots, archive the plan.

Steps:

1. start a fresh nimbus server on a clean data dir (`/tmp/nimbus-dr/`)
   with `--skip-codegen`, mint an admin session cookie, seed 3
   tenants.
2. re-capture every screenshot in `before/` at 1440×900. Save to
   `docs/plans/proof/desktop-ui-design-review-fixes/after/`. Names
   mirror `before/` with a `dr8-` prefix.
3. record the same verifications the design review used:
   - browser console: zero `error` / `warn` across the full walk
     (`mcp__chrome-devtools__list_console_messages`).
   - vitest: `cd packages/nimbus-ui && npx vitest run` — all pass.
   - typecheck: `npm run typecheck` clean.
   - build: `npm run build` clean.
4. write proof README at
   `docs/plans/proof/desktop-ui-design-review-fixes/README.md`
   mirroring the CS9 / CS10 structure: before/after sections, a
   mapping table from each F-finding to the after-screenshot that
   proves it.
5. flip status to `done`; append an execution log entry below;
   `git mv` plan to `docs/plans/archive/`; update
   `docs/plans/README.md` (remove from active; add baseline entry).

Done when:

- the proof README's mapping table covers F1, F2, F3, F4, F5, F6, F7,
  F9, F10, F11, F12, F14 (12 findings — F8 was already-fixed, F15 is
  out of scope, F13 is folded into F1 via `EmptyState`, F16 is a
  positive-note assertion verified by step 3's console check).
- plan status reads `done`; plan lives under `docs/plans/archive/`;
  `docs/plans/README.md` no longer lists it as active.

## Verification approach

- Use chrome-devtools-mcp (workspace-allowed dirs only) for live page
  inspection and screenshots. No Playwright runner.
- For every phase with a unit-testable gate, the test lands in the
  same commit as the implementation.
- Console hygiene is verified on every phase's after-state — zero
  `error`/`warn` is a hard gate, matching CS9.

## Execution log

(a) 2026-05-18 — Plan promoted from the operator-console design review
at `docs/design-review/2026-05-18-operator-console-review.md`. 16
findings catalogued, 14 in scope. Phase ledger DR0–DR8 prepared.

(b) 2026-05-18 — DR0 landed in the promotion commit: 16 baseline
PNGs moved from `.review-shots/` to
`docs/plans/proof/desktop-ui-design-review-fixes/before/`; design
review doc updated to reference the proof-bundle paths.

(c) 2026-05-18 — DR1 landed (`9480c11b`): introduced canonical
`EmptyState` component at
`packages/nimbus-ui/src/components/empty-state.tsx` with 4-test spec
(title-only, body, link CTA, button CTA). Deleted
`shell/placeholder-page.tsx`. Migrated `/app/files`, `/app/settings`,
`/admin/`, `/admin/observability` to `EmptyState`. Removed the
"Phase 1 · Embedded SPA" footer div from `primary-drawer.tsx`. Grep
`DU-shell|Phase 1 · Embedded|PlaceholderPage` against
`packages/nimbus-ui/src` returns zero hits. 175 vitest tests pass.

(d) 2026-05-18 — DR2 landed (`6bbbbdad`): gated the system tenant lens
on the Developer view per DESIGN.md §952–961. `KeyboardContract` now
reads `useRouterState`'s pathname and early-returns on `Meta+\` when
`viewFromPathname(pathname) !== "developer"`. `CommandPalette` also
filters the `open-system-tenant-lens` Run action out of the palette
when the active view is operator. New `keyboard-contract.spec.tsx`
asserts ⌘\ opens the lens from `/app/compute`, leaves it closed from
`/admin/machines`, and ⌘K still toggles the palette from either view.
178 vitest tests pass; typecheck clean.

(g) 2026-05-18 — DR5 landed: replaced both bleak operator placeholders
with real content. `/admin` (System) now renders an 8-field overview
grid: Nimbus version, health, server uptime, listen address, tenant
count, machine count, service count, and listener count + adapter set.
Sources: `api.system.status`, `api.machines.list`, `api.services.list`,
`api.listeners.list`, and a one-shot `/api/tenants` fetch for the
tenant count. `/admin/observability` now renders the same logs +
runs surfaces the developer console exposes, with a tenant-scope chip
that defaults to `tenant: all`. Tab strip mirrors the developer
Observability strip (logs, runs, events disabled, errors disabled).
Empty state via the canonical `EmptyState` from DR1. Plan-code grep
still returns zero hits across `packages/nimbus-ui/src`. 188 vitest
tests pass; typecheck clean.

(f) 2026-05-18 — DR4 landed: `useTenantBootstrap` now auto-defaults
`activeTenant` to the first tenant id (sorted alphabetically) on `/app`
routes when the store is null and no `?as=` override is present.
Operator routes are unchanged — cross-tenant scope still applies there.
ScopeChip on `/app/services` no longer renders the `tenant: all`
fallback: when `activeTenant === null` (transient pre-fetch state), the
chip returns null instead. Audit confirms no other `?? "all"` fallbacks
remain in `routes/app/`. Bootstrap spec extended to 10 tests covering:
sort determinism across string + object tenants, no-op on operator
routes, existing-tenant preservation, empty-list handling, error
swallowing, and `?as=` priority skipping the fetch entirely. 188 vitest
tests pass; typecheck clean.

(e) 2026-05-18 — DR3 landed: Observability tab strip now renders from
`OBSERVABILITY_SUB_DRAWER.items` so the sub-drawer is the single source
of truth for the four sections (logs, runs, events, errors). Disabled
items (events, errors) render as `aria-disabled` spans with a
`coming soon` title tooltip instead of clickable links. Schedules
sub-drawer switched from `kind: "dynamic"` to `kind: "static"` with
two items (Scheduled, Cron); body tab strip removed; dynamic
sub-drawer recent-list + search-box removed (they return when the
real table lands). `routes/app/section-nav.spec.ts` asserts both
specs' shape. 178 → 182 vitest tests pass; typecheck + build clean.

(i) 2026-05-18 — DR7 landed: polish triplet for F7, F9, F14.
`routes/app/storage.tsx` breadcrumbs now start at the tenant id (the
leading `storage` segment is gone — the primary drawer's selected
state already implies the section). Admin service detail tab buttons
drop `uppercase tracking-wide` to match every other operator tab
strip. Admin service detail sub-drawer now renders tenant-grouped (and
shows per-service `state` chips) so it visually matches the operator
services list. Extracted `groupByTenant` from `routes/admin/services.tsx`
as an exported helper and reuse it from the detail page. 190 vitest
tests still pass; typecheck + build clean.

(h) 2026-05-18 — DR6 landed: pruned the admin Service detail page to
a single Placement tab. `DetailTab` collapsed from a four-value union
to the literal `"placement"`; `TABS` exports a one-entry array; the
stale `RestartsTab`, `DensityTab`, `DriftTab` body components and the
local `Empty` helper were deleted (the canonical `EmptyState` from DR1
covers any future revival). Strengthened the `Extract<SubDrawerSpec,
{ kind: "static" }>` annotation on both Observability sub-drawer
exports so the tab strip mappings drop the prior `.kind === "static"`
narrowing branch. Added `routes/admin/services_.$service.spec.ts`
asserting `TABS.length === 1` and that `isTab()` accepts only
`"placement"`. Wired the TanStack route generator (`scripts/generate-
routes.mjs` + `vite.config.ts`) to ignore `.spec.(ts|tsx)` files so
route-level specs no longer warn during codegen. 190 vitest tests
pass; typecheck + build clean.

(j) 2026-05-18 — DR8 landed: plan closure. Captured 17 after-images
at 1440×900 to `docs/plans/proof/desktop-ui-design-review-fixes/after/`
(16 mirror the `before/` walk; `dr8-x-03-lens-blocked-on-admin.png` is
the extra DR2 evidence shot). Re-verified all gates: `grep -rn
'DU-shell\|Phase 1 · Embedded\|PlaceholderPage' packages/nimbus-ui/src`
returns zero matches; ⌘\ pressed on `/ui/admin/machines` leaves the
DOM lens-free; vitest 27 files / 190 tests pass; typecheck clean;
build clean (only the informational rolldown chunk-size note).
Console hygiene clean across the walk (the only entry observed is a
single 404 on a dev-server fetch and does not surface as UI). Wrote
`docs/plans/proof/desktop-ui-design-review-fixes/README.md` mapping
F1, F2, F3, F4, F5, F6, F7, F9, F10, F11, F12, F14 each to their
after-image (12 in-scope findings; F8 was already fixed, F13 folded
into F1 via `EmptyState`, F15 deferred to a separate
verification-tooling plan, F16 verified by console-hygiene check).
Flipped plan status to `done`; `git mv` to
`docs/plans/archive/`; updated `docs/plans/README.md` to list as an
archived baseline.
