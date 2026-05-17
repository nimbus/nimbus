# Plan: Desktop UI Shell Overhaul

Canonical active execution plan for upgrading the Nimbus operator console
chrome from a fixed-width sidebar shell into a three-pane drawer system with
a tenant-aware top nav. Owns the work to land:

1. A collapsible left drawer (full width ↔ icon-only) with a toggle that
   persists to `localStorage`.
2. A contextual sub-drawer to the right of the left drawer that holds
   section-level sub-navigation and list selection (the
   "Functions → Schedules / Indexes" pattern from Convex Dashboard, the
   "Containers → Logs / Inspect / Files" pattern from Docker/Podman Desktop).
3. A new top horizontal nav that owns the logo and a tenant dropdown wired
   to the existing `_nimbus` system-tenant data path.

Phase 1 of the operator console (DU1–DU11) shipped under
`docs/plans/archive/desktop-ui-plan.md`. This plan extends that shell with
the multi-pane navigation and multi-tenant discoverability that Phase 1
intentionally deferred.

The root [`DESIGN.md`](../../DESIGN.md) remains the design-system authority.
The two-axis token system (Mode × Palette, applied via compound
`[data-palette=X][data-theme=Y]` selectors on `<html>`) defined in
`packages/nimbus-ui/src/styles/globals.css` is a hard requirement — every
new shell surface uses `var(--color-*)` and the existing token-class
helpers (`bg-app`, `bg-surface`, `bg-surface-2`, `text-default`,
`text-muted`, `border-app`, `border-brand`, `text-brand`, etc.), never raw
hex.

Reference consoles benchmarked for interaction patterns (developer/operator
audience):

| Product | Pattern Nimbus borrows |
| --- | --- |
| Convex Dashboard | Left primary nav + contextual sub-nav (Functions → File list → Function detail). Tenant/deployment selector in top nav. |
| Firebase Console | Collapsible primary nav (icon-only state), project selector in top bar adjacent to logo. |
| Docker Desktop | Left primary nav with contextual right-column list (Containers → individual container detail). |
| Podman Desktop | Collapsible left nav (Svelte/Tailwind reference for icon-only state), contextual right-column inspector. |

---

## Status

- **Status:** `in_progress` — promoted 2026-05-17 as the active follow-on
  to the archived Phase 1 desktop UI plan.
- **Primary owner:** this plan.
- **Activation gate:** archived
  [`desktop-ui-plan.md`](archive/desktop-ui-plan.md) reached
  `implementation-complete; archive-pending`, palette/mode + DU7 followup
  fixes (commit `6ba937c2`) shipped on `main`, working tree clean.
- **Related plans / references:**
  - `docs/plans/archive/desktop-ui-plan.md` — Phase 1 shell baseline
  - `docs/plans/archive/desktop-shell-plan.md` — Electron wrapper that
    hosts this SPA; no changes expected
  - `docs/plans/archive/brand-system-plan.md` — brand tier vs product
    tier separation; the operator console is the Product tier
  - `DESIGN.md` — visual token authority
  - `packages/nimbus-ui/src/styles/globals.css` — token source of truth
  - `packages/nimbus-ui/src/routes/__root.tsx` — current shell composition

## Authorizations (durable)

Carried forward from the desktop-ui / desktop-shell autonomous-mode
memory (`feedback_desktop_plans_autonomous_mode.md`):

- Commit and push focused baselines to `main` without asking. Pre-launch,
  no PRs.
- Create repos and run `gh` workflows as part of this plan without
  confirmation.
- Verification tooling: `playwright-cli` and `chrome-devtools-mcp` for
  live UI checks. **Do not** use `@playwright/mcp` (~4× token cost,
  previously rejected).
- Commit baselines from the working tree; never blob-stage via `/tmp`
  (`feedback_commit_workflow.md`).
- Git editor stays `vim`. Do not suggest changing `core.editor`.

## Control Plan Rules

1. The current git worktree plus this plan's `Phase Status Ledger`,
   `Implementation Checkpoints`, and `Execution Log` are the source of
   truth. Prior chat transcripts are not progress state.
2. Implement exactly one phase at a time. Mark it `in_progress` before
   editing code, `done` only after the phase's acceptance criteria and
   verification have been recorded in the Execution Log.
3. No backwards-compatibility shims. The current sidebar is replaced, not
   wrapped. Pre-launch: delete old behavior rather than deprecate it.
4. Every new shell surface respects the Mode × Palette token system. Raw
   hex outside the Appearance section is a verification failure.
5. Persisted state lives under the existing `nimbus-ui:*` `localStorage`
   namespace and follows the read/write helper pattern in
   `packages/nimbus-ui/src/store/ui-store.ts`
   (`readStoredMode`/`readStoredPalette`).
6. New shell components ship with co-located `*.spec.tsx` vitest specs
   that mirror the dynamic-import + `vi.resetModules()` pattern from
   `packages/nimbus-ui/src/components/appearance-section.spec.tsx`.
7. Live verification via Chrome DevTools MCP on the running Vite dev
   server (`cd packages/nimbus-ui && npm run dev` → `:5173`) is mandatory
   before marking any visual phase `done`.

## Current Assessed State (2026-05-17)

Mapped from the working tree:

- Shell composition lives in `packages/nimbus-ui/src/routes/__root.tsx`
  (52 lines). It renders, in order:
  `ThemeController` → `KeyboardContract` →
  `<div flex h-screen flex-col bg-app text-default>` →
  `<div flex min-h-0 flex-1>` → `<Sidebar />` + `<main>` →
  `<StatusBar />` → `CommandPalette` → `SystemTenantLens` → `Toaster`.
- The primary nav is `packages/nimbus-ui/src/shell/sidebar.tsx` (110
  lines): fixed `w-56`, contains logo + `Nimbus / operator console`
  wordmark + the `NAV_ENTRIES` map from
  `packages/nimbus-ui/src/shell/nav-entries.ts` (7 entries: overview,
  compute, storage, network, machines, observability, settings).
- There is no top horizontal nav. There is no sub-drawer. The
  `system-tenant-lens.tsx` is a `Cmd-\\` right-side overlay (z-40, 50vw
  max), not a persistent right column — it should remain unchanged and
  the new sub-drawer is a separate surface.
- Tenant context: the active tenant string comes from
  `api.system.status` (`status?.activeTenant ?? "_nimbus"`, shown in
  `status-bar.tsx`). There is no current way to switch tenant from the
  UI; multi-tenant work requires URL editing today.
- UI state store: `packages/nimbus-ui/src/store/ui-store.ts` (151 lines)
  owns `paletteOpen`, `lensOpen`, `actionMenuOpen`, `themeMode`, `theme`,
  `palette`. Persistence helpers (`readStoredMode`, `readStoredPalette`,
  `persistMode`, `persistPalette`) are the pattern for the new persisted
  drawer state.
- Token system: `packages/nimbus-ui/src/styles/globals.css` (242 lines)
  defines 6 palette/mode combinations via `[data-palette=X]` and
  `[data-palette=X][data-theme="dark"]` compound selectors and the
  `bg-*`/`text-*`/`border-*` token classes in `@layer components`.
- All shell files are well under the 1500-line threshold. The largest
  shell file is `command-palette.tsx` at 321 lines.

## Architectural Decision (proposed, subject to Phase O0 confirmation)

Restructure the shell composition root rather than bolt the new panes onto
the existing `<Sidebar />`. The target tree in `__root.tsx`:

```
<AppErrorBoundary>
  <ThemeController />
  <KeyboardContract />
  <StalenessProvider>
    <div className="flex h-screen flex-col bg-app text-default">
      <TopNav />                                {/* new */}
      <div className="flex min-h-0 flex-1">
        <PrimaryDrawer />                       {/* renamed from Sidebar */}
        <SubDrawer />                           {/* new, conditionally rendered */}
        <main>
          <DisconnectedOverlay />
          <Outlet />
        </main>
      </div>
      <StatusBar />
    </div>
    <CommandPalette />
    <SystemTenantLens />   {/* unchanged: Cmd-\ overlay, not a pane */}
    <Toaster />
  </StalenessProvider>
</AppErrorBoundary>
```

Rationale:

- The current shell already uses flex composition. Adding `TopNav` above
  the `flex min-h-0 flex-1` row and `SubDrawer` beside `PrimaryDrawer`
  fits the existing layout primitives without introducing CSS Grid.
- A three-slot layout makes drawer collapse a width transition on a
  single pane rather than a structural change. The collapsed state
  reduces `PrimaryDrawer` from `w-56` to `w-12` (icon-only) and the rest
  of the row reflows naturally.
- Renaming `Sidebar` → `PrimaryDrawer` is a breaking rename, not a
  re-export. Pre-launch project rules forbid compatibility shims.

If Phase O0 finds that restructure is more disruptive than expected,
the fallback is additive: keep `Sidebar` as-is, mount `TopNav` above the
existing row, and mount `SubDrawer` between `Sidebar` and `<main>`.
Default position is restructure; alternative requires a recorded note in
the Execution Log.

## Phase Status Ledger

| Phase | Description | Status |
| --- | --- | --- |
| O0 | Confirm restructure vs additive; read full shell & decide | `todo` |
| O1 | Collapsible primary drawer (width transition, persistence, a11y) | `todo` |
| O2 | Sub-drawer surface (mount point, store state, slot API) | `todo` |
| O3 | Sub-drawer routes: Storage tables / Compute functions / Observability runs | `todo` |
| O4 | Top horizontal nav with logo + tenant dropdown | `todo` |
| O5 | Tenant switch wired to the `_nimbus` system-tenant data path | `todo` |
| O6 | Mode × Palette × Drawer-state matrix verification (live) | `todo` |
| O7 | Phase 1 archival: hand-off entry, archive close | `todo` |

Exactly one phase is `in_progress` at a time. Update this ledger and the
phase's own section before/after each work block.

## Phase Order and Dependencies

- O0 must complete before any of O1–O5. The decision it records (restructure
  vs additive) determines the shape of every subsequent diff.
- O1 unblocks O2 (sub-drawer must coexist with collapsed primary drawer).
- O2 unblocks O3 (the surface exists before its first consumer routes).
- O4 may run in parallel with O2/O3 once O0 is closed; it does not depend
  on the sub-drawer.
- O5 depends on O4 (dropdown UI) but is the wiring step, not the visual
  step.
- O6 depends on O1–O5 being landed in the working tree.
- O7 depends on O6 passing.

## Phase Details

### O0 — Confirm restructure vs additive

**Status:** `todo`

**Goal:** Record a definitive layout decision in the Execution Log before
editing the shell. Eliminate the risk of half-restructuring.

**Work:**

1. Re-read `packages/nimbus-ui/src/routes/__root.tsx`,
   `packages/nimbus-ui/src/shell/sidebar.tsx`,
   `packages/nimbus-ui/src/shell/system-tenant-lens.tsx`, and the
   `nav-entries.ts` shape.
2. Decide restructure (default) vs additive.
3. Record the decision and the reasoning in the Execution Log.
4. If restructure, rename plan: `Sidebar` → `PrimaryDrawer`, callers
   updated; `system-tenant-lens` untouched.

**Acceptance:**

- Decision logged with a one-line "why" in the Execution Log.
- No code changes yet.

### O1 — Collapsible primary drawer

**Status:** `todo`

**Goal:** Replace the fixed-width sidebar with a drawer that toggles
between full (`w-56`) and icon-only (`w-12`) states. State persists across
reloads. Keyboard accessible. Token-respecting.

**Work:**

1. Extend `ui-store.ts` with `primaryDrawerCollapsed: boolean`,
   `togglePrimaryDrawer()`, and a `persistPrimaryDrawerCollapsed` helper
   matching the existing `persistMode`/`persistPalette` shape. Storage
   key: `nimbus-ui:primary-drawer-collapsed`.
2. Create `packages/nimbus-ui/src/shell/primary-drawer.tsx` (replacing
   `sidebar.tsx`):
   - Width transitions via Tailwind (`w-56` ↔ `w-12`) with `transition-[width]`.
   - Icon-only mode hides text label and `NavCount`; tooltip on hover (use
     `title` attribute as the minimum; consider `@radix-ui/react-tooltip`
     only if it is already a dep — do not add new deps).
   - Toggle button at the top of the drawer, `data-testid="primary-drawer-toggle"`,
     `aria-expanded`, `aria-controls`, `aria-label="Collapse navigation"` /
     `"Expand navigation"`.
   - Logo wordmark hides in collapsed mode, mark stays.
   - Focus management: toggling does not move focus off the toggle.
3. Update `__root.tsx` to render `<PrimaryDrawer />` (or keep
   `<Sidebar />` if additive path was chosen in O0).
4. Delete `sidebar.tsx` if restructure path. Update all imports.
5. Add `packages/nimbus-ui/src/shell/primary-drawer.spec.tsx`:
   - Toggle click flips `aria-expanded` and the persisted value.
   - Hydration from persisted `true` mounts collapsed.
   - Keyboard `Enter`/`Space` on toggle works.
   - `data-testid="nav-overview"` etc. remain stable.

**Acceptance:**

- Toggle click changes width; reload restores it.
- All 7 nav entries reachable in both states.
- All vitest specs pass: `cd packages/nimbus-ui && npm run test`.
- `npm run typecheck` clean.
- Chrome DevTools MCP screenshot in both states for `light × blue`.

### O2 — Sub-drawer surface

**Status:** `todo`

**Goal:** Introduce a persistent right-of-primary column that holds
contextual sub-navigation. Coexists with collapsed primary drawer.

**Work:**

1. Extend `ui-store.ts` with `subDrawerOpen: boolean`,
   `setSubDrawerOpen(open)`, and persistence under
   `nimbus-ui:sub-drawer-open`.
2. Create `packages/nimbus-ui/src/shell/sub-drawer.tsx`:
   - Fixed width `w-64`. Rendered between `<PrimaryDrawer />` and
     `<main>`. Uses `border-r border-app bg-surface`.
   - Renders `null` when route does not register a sub-drawer
     contributor (see O3 contributor pattern).
   - Header row: section title + close button (collapses to `w-0` /
     unmounts via store). Close button has
     `data-testid="sub-drawer-close"`.
3. Define the contributor API. Two options to choose at implementation
   time, recorded in the Execution Log:
   - **Slot-based:** routes render `<SubDrawer.Slot title="…">…</SubDrawer.Slot>`
     via a React portal target inside `sub-drawer.tsx`.
   - **Outlet-based:** add a TanStack Router sub-route segment (e.g.
     `routes/__root.tsx` exposes a `subDrawer` slot via `Outlet`).
4. Add `sub-drawer.spec.tsx` with vitest covering: mount with no
   contributor renders nothing; mount with a contributor renders title +
   children; close button unmounts via store; persisted-open hydrates.

**Acceptance:**

- Routes that opt in show a populated sub-drawer; routes that do not
  render no sub-drawer at all.
- Sub-drawer remains correctly anchored when primary drawer is collapsed.
- Specs pass; typecheck clean.

### O3 — Sub-drawer consumer routes

**Status:** `todo`

**Goal:** Wire sub-drawer contributors for the three routes that benefit
most from list-style sub-navigation:

- `/storage` → tables list (data from `api.tables.list`)
- `/compute` → functions list (data from `api.functions.list`)
- `/observability` → recent runs list (data from `api.runs.recent`)

**Work:**

1. For each route, add a sub-drawer contributor that fetches the list,
   renders selectable items with `data-testid="sub-drawer-item-<id>"`,
   and navigates on click using the existing dynamic-segment routes
   (`storage_.$tenant.tsx`, `storage_.$tenant_.$table.tsx`, etc.).
2. Active item highlighting respects tokens (`bg-surface-2`,
   `text-default`, optional `border-l-2` with `border-brand`).
3. Empty states use `text-muted` and a short helper sentence.
4. Loading states match the existing `NavCount`-style `·` placeholder.
5. Routes that do not contribute (overview, network, machines, settings)
   render no sub-drawer.

**Acceptance:**

- Clicking a sub-drawer item navigates to the detail route and the
  selected item stays highlighted.
- Tests cover: list renders, click navigates, active selection, empty
  state.

### O4 — Top horizontal nav

**Status:** `todo`

**Goal:** Mount a top bar above the drawer row. Owns the logo and the
tenant dropdown trigger. Replaces the logo block currently inside the
sidebar.

**Work:**

1. Create `packages/nimbus-ui/src/shell/top-nav.tsx`:
   - Fixed height (e.g. `h-10`), `border-b border-app bg-surface`.
   - Left: existing `LogoMark` (lifted from `sidebar.tsx`) +
     `Nimbus / operator console` wordmark. Mirror current typography
     exactly so the brand mark survives the move.
   - Right of logo: `<TenantDropdown />`.
   - Right edge: room for future actions (status indicator, user menu).
     Do not pre-build placeholder slots.
2. Remove the logo + wordmark block from `primary-drawer.tsx` (post-O1).
3. Add `top-nav.spec.tsx`: logo renders; tenant trigger present.

**Acceptance:**

- Top nav renders in all 6 Mode × Palette combinations without raw hex.
- Sidebar no longer duplicates the logo.

### O5 — Tenant dropdown wired to system-tenant data

**Status:** `todo`

**Goal:** The tenant dropdown is the discoverable seam for multi-tenant
switching. It is a real data path, not a visual stub.

**Work:**

1. Read `api.system.status` to get `activeTenant` (already used by
   `status-bar.tsx`).
2. Source the list of tenants from the system-tenant API. **Open
   question for the implementer:** the current archived
   `system-tenant-api-plan.md` exposes `_nimbus` documents but not
   necessarily a `tenants.list` query. If the query exists, use it. If
   it does not, the implementer must:
   - Check `convex/_generated/api.d.ts` for any `tenants` namespace.
   - If absent, record a sub-task in the Execution Log to add
     `convex/tenants.ts` (a system-tenant-scoped `list`), then proceed.
3. Create `packages/nimbus-ui/src/shell/tenant-dropdown.tsx`:
   - Trigger displays active tenant name (default `_nimbus`).
   - Click opens a dropdown listing all tenants with the current one
     marked.
   - Selecting a tenant calls a setter — either a server-side
     `setActiveTenant` mutation (if it exists in the system-tenant API)
     or a client-side selection persisted to
     `nimbus-ui:active-tenant` while the UI is unaware of server-side
     multi-tenant routing. Record the chosen mechanism in the Execution
     Log.
4. Add `tenant-dropdown.spec.tsx`: open/close, listing, selection
   triggers the chosen setter, keyboard navigation.
5. Persistence + re-render: routes that depend on `tenantId` re-fetch
   when tenant changes. Confirm `storage_.$tenant.tsx` flow.

**Acceptance:**

- Switching tenant updates the rendered active tenant and re-fetches
  tenant-scoped data on the current route.
- Specs pass; typecheck clean.
- A live Chrome DevTools MCP run shows tenant switch causing route data
  to change.

### O6 — Live verification matrix

**Status:** `todo`

**Goal:** Prove the full shell works across the token matrix and drawer
states.

**Work:**

1. Boot the dev server: `cd packages/nimbus-ui && npm run dev`.
2. Use Chrome DevTools MCP (not `@playwright/mcp`) to:
   - Visit overview, compute, storage, network, machines, observability,
     settings.
   - For each, capture screenshots in the 6 Mode × Palette combinations
     (`light × {blue,mono,warm}` and `dark × {blue,mono,warm}`).
   - Toggle primary drawer collapsed/expanded; confirm sub-drawer
     reanchors.
   - Open tenant dropdown; switch tenant; confirm data refetch.
   - Hard reload after each persisted-state change.
3. Run `cd packages/nimbus-ui && npm run typecheck && npm run test`.
4. Record screenshot paths and the matrix coverage list in the
   Execution Log.

**Acceptance:**

- Every screenshot lands without visual regression in token treatment.
- All vitest specs pass.
- `npm run typecheck` clean.
- `cargo fmt --all --check`, `make clippy` clean for any incidental
  Rust touches (none expected from this plan).

### O7 — Phase 1 archival hand-off

**Status:** `todo`

**Goal:** Close the loop with the archived Phase 1 desktop UI plan.

**Work:**

1. Add a one-paragraph "Follow-up plan" reference to
   `docs/plans/archive/desktop-ui-plan.md` pointing here, only if that
   archived plan has not yet been frozen. If it is frozen, skip — this
   plan stands alone as the active control plane.
2. Add this plan to `docs/plans/README.md` under
   "Active execution plans".
3. Update the Status field at the top of this plan to `done` once O0–O6
   are all `done` and the verification artifacts are committed.

**Acceptance:**

- `docs/plans/README.md` lists this plan.
- Final commit + push to `main` lands with a green `make ci` (or the
  reduced subset relevant to JS-only changes:
  `npm run typecheck && npm run test && npm run build`).

## Implementation Checkpoints

| Phase | Checkpoint commit subject (proposed) |
| --- | --- |
| O0 | "DU-shell O0: lock restructure decision in plan ledger" |
| O1 | "DU-shell O1: collapsible primary drawer with persistence" |
| O2 | "DU-shell O2: sub-drawer surface + contributor API" |
| O3 | "DU-shell O3: storage/compute/observability sub-drawers" |
| O4 | "DU-shell O4: top nav with logo and tenant dropdown trigger" |
| O5 | "DU-shell O5: tenant dropdown wired to system-tenant data" |
| O6 | "DU-shell O6: live verification across mode × palette × drawer" |
| O7 | "DU-shell O7: close active plan, archive hand-off" |

Each checkpoint commits a focused baseline from the working tree (per
`feedback_commit_workflow.md`). No blob staging via `/tmp`.

## Verification Commands

- JS typecheck: `cd packages/nimbus-ui && npm run typecheck`
- JS tests (vitest + happy-dom): `cd packages/nimbus-ui && npm run test`
- JS build: `cd packages/nimbus-ui && npm run build`
- Dev server (manual + Chrome DevTools MCP): `cd packages/nimbus-ui && npm run dev`
  (binds `:5173`)
- Full repo CI: `make ci`

## Non-goals

- No new server-side endpoints unless O5 discovers `tenants.list` is
  missing — even then, only a single read query is in scope.
- No changes to the Electron desktop shell (`nimbus/desktop`).
- No changes to the existing `SystemTenantLens` (`Cmd-\\`) overlay — it
  remains a separate surface.
- No new third-party UI dependencies. Reuse `lucide-react`, `cmdk`,
  `sonner`, `tailwindcss`, Zustand, TanStack Router already present.
- No theming changes. The Mode × Palette token system is the contract.
- No backwards-compatibility shims for the renamed `Sidebar` →
  `PrimaryDrawer` (if restructure path taken).

## Execution Log

_Append-only. Entries dated. Capture: decision recorded, files touched,
verification run, anything surprising._

- **2026-05-17** — Plan promoted from prior compaction-prompt scratchpad.
  Phase O0 is the next entry point.

## First Step When You Resume

1. Re-read this plan top-to-bottom.
2. Open Phase O0 by reading `packages/nimbus-ui/src/routes/__root.tsx`,
   `packages/nimbus-ui/src/shell/sidebar.tsx`, and
   `packages/nimbus-ui/src/shell/system-tenant-lens.tsx` in full.
3. Decide restructure vs additive and write a single Execution Log entry
   recording the decision + one-line reasoning.
4. Mark O0 `done` and O1 `in_progress` in the Phase Status Ledger.
5. Begin O1.
