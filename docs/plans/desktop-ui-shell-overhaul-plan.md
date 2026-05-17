# Plan: Desktop UI Shell Overhaul

Canonical active execution plan for upgrading the Nimbus operator console
chrome from a fixed-width sidebar shell into a three-pane drawer system
**with a revised top-level information architecture** that recognizes
distinct mental models for short-lived compute, long-running services,
scheduled work, schema-aware storage, and opaque blob storage.

This plan owns the work to land:

1. A **revised top-level IA** that promotes **Services**, **Schedules**,
   and **Files** out of their current homes into top-level peers, and
   reshapes `NAV_ENTRIES` and `DESIGN.md` accordingly.
2. A **collapsible left drawer** (full width ↔ icon-only) with a toggle
   that persists across reloads.
3. A **contextual sub-drawer** to the right of the left drawer with two
   modes: **static menu** (Settings sub-pages, Convex `SettingsSidebar`
   pattern) and **dynamic list** (Tables, Files, Functions, Schedules,
   Services, Machines — Convex `DataSidebar` pattern).
4. A **top horizontal nav** that owns the logo and a **tenant selector**
   with explicit `scoped`/`disabled` state on tenant-irrelevant sections
   (Machines, server-wide Settings, Network listener status).

Phase 1 of the operator console (DU1–DU11) shipped under
`docs/plans/archive/desktop-ui-plan.md`. This plan extends that shell with
the multi-pane navigation, multi-tenant discoverability, and the IA
correction that Phase 1 intentionally deferred.

The root [`DESIGN.md`](../../DESIGN.md) remains the design-system
authority. The two-axis token system (Mode × Palette, applied via compound
`[data-palette=X][data-theme=Y]` selectors on `<html>`) defined in
`packages/nimbus-ui/src/styles/globals.css` is a hard requirement — every
new shell surface uses `var(--color-*)` and the existing token-class
helpers (`bg-app`, `bg-surface`, `bg-surface-2`, `text-default`,
`text-muted`, `border-app`, `border-brand`, `text-brand`, etc.), never raw
hex. `DESIGN.md`'s IA section (lines 78–96 today) gets updated in
Phase O1 to match the revised IA below.

Reference consoles inspected directly from source or web (developer/operator
audience):

| Product | Pattern Nimbus borrows | Anchor file |
| --- | --- | --- |
| **Convex Dashboard** | Top-level peers: Health / Data / Functions / Files / Schedules / Logs + History/Settings. Sub-drawer for Tables (`DataSidebar`), Functions (`DirectorySidebar`), Settings (`SettingsSidebar`). Tenant-equivalent (`NentSwitcher`) in the **top header**, not sidebar. | `npm-packages/dashboard-common/src/layouts/DeploymentDashboardLayout.tsx:57-99`, `.../SettingsSidebar.tsx:10-19`, `.../features/data/components/DataSidebar.tsx`, `.../elements/Sidebar.tsx` |
| **Podman Desktop** | Containers/Pods/Images/Volumes/Networks/Extensions are root peers; Kubernetes nests an 11-item submenu including Services/Deployments/Jobs/CronJobs. Confirms long-running orchestration deserves its own root scope when present. | `packages/renderer/src/stores/navigation/navigation-registry.ts:65-75` |
| **Render** | Service *type* (Web Service / Background Worker / Cron Job / Static Site) is the primary nav unit; persistent storage attaches to a service rather than living globally. Confirms Services is a top-level mental model. | Render docs `service-types/` |
| **Vercel** (Feb 2026 redesign) | Vertical sidebar: Deployments / Functions / Storage / Observability / Logs / Settings / Firewall. Storage is top-level because Blob/KV/Postgres are conceptually separate from "the app". | Vercel changelog "dashboard-navigation-redesign-rollout" |
| **Firebase Console** | Authentication / Firestore / Storage / Functions as sibling sidebar items. Project selector in top bar adjacent to logo. | Firebase web console |
| **Docker Desktop** | Containers / Images / Volumes / Builds / Dev Environments / Extensions / Kubernetes as siblings. Volumes top-level peer of containers — analogous to Files being a peer of Storage in Nimbus. | Docker Desktop UI |

---

## Status

- **Status:** `in_progress` — promoted 2026-05-17 with IA revisions
  recorded 2026-05-17 after deep first-principles review.
- **Primary owner:** this plan.
- **Activation gate:** archived
  [`desktop-ui-plan.md`](archive/desktop-ui-plan.md) reached
  `implementation-complete; archive-pending`, palette/mode + DU7 followup
  fixes (commit `6ba937c2`) shipped on `main`, working tree clean.
- **Related plans / references:**
  - `docs/plans/archive/desktop-ui-plan.md` — Phase 1 shell baseline
  - `docs/plans/archive/desktop-shell-plan.md` — Electron wrapper hosting
    this SPA; no changes expected
  - `docs/plans/archive/brand-system-plan.md` — brand tier vs product tier
  - `DESIGN.md` — visual + IA token authority (this plan updates the IA section)
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
   wrapped. Renames are breaking. Pre-launch: delete old behavior rather
   than deprecate it.
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
8. **DESIGN.md is canonical.** Any IA change in this plan lands as a
   `DESIGN.md` edit in the same checkpoint that updates `NAV_ENTRIES` —
   never let the doc and the code disagree.

## Information Architecture (Revised)

### First-principles review

The current `DESIGN.md` IA (lines 82–96) bundles into Compute everything
that "runs server-side", but this conflates four genuinely distinct
operator mental models:

| Mental model | Operator question | Lifecycle | Examples |
| --- | --- | --- | --- |
| Request-scoped execution | "did my call succeed?" | ephemeral (<= seconds) | Convex query/mutation/action, HTTP handler, Cloud Function invocation |
| Long-running placement | "is my service up and reachable?" | persistent (days/weeks) | Compose-declared service, machine-resident process, Postgres replica |
| Scheduled work | "did the job fire on time?" | periodic / future-dated | Cron jobs, `scheduler.runAfter` mutations, retry queues |
| Schema-aware data | "what shape does this table have? where's the index?" | persistent | Convex tables, MongoDB collections, Firestore docs |
| Opaque bytes | "where is this file? what's its content-hash?" | persistent, content-addressed | S3-compatible blob, attachments, exported snapshots |
| Reachability | "what's listening? what's denied?" | persistent | HTTP/WS listeners, ports, gvproxy state |
| Host / guest lifecycle | "is the VM healthy? what image?" | persistent | macOS machines, microVMs, boot images |

A control plane that fuses these into one bucket forces operators to
mentally re-classify a resource each time they navigate. The remedy is
**one top-level per mental model**, which is what Convex, Vercel,
Firebase, and Render converge on independently.

### Revised top-level IA

The eight top-level sections (sidebar order, top to bottom):

1. **Overview** — health cards, recent runs, recent events, machine/service summary. Dashboard, no sub-drawer.
2. **Compute** — request-scoped execution only.
   - Functions (list, runner, schema-aware args, kind: query/mutation/action/HTTP)
   - Runs (status, request ID, duration, errors, trace correlation)
   - Sub-drawer mode: **dynamic list** of functions (by path or by kind), navigates to function detail.
3. **Schedules** — periodic / future-dated work (**promoted from Compute**).
   - Scheduled jobs (next run, last run, status, cancel/retry)
   - Cron jobs (expression, history)
   - Sub-drawer mode: **static menu** with two tabs (`Scheduled`, `Cron`), or **dynamic list** of jobs.
4. **Services** — long-running placement (**promoted from Compute**).
   - Service catalog (Compose-declared services, lifecycle state, health, endpoints)
   - Service detail (logs, machine placement, restart policy, environment)
   - Sub-drawer mode: **dynamic list** of services, navigates to service detail.
5. **Storage** — schema-aware data.
   - Tables / collections (with per-table row count, last write)
   - Document browser (cursor pagination, filters, schema-aware editor)
   - Schema, Indexes, Query builder
   - Sub-drawer mode: **dynamic list** of tables for the active tenant.
6. **Files** — opaque blob / S3-compatible storage (**new top-level**, deferred content; placeholder route in this plan).
   - Buckets / namespaces (when implemented)
   - File browser (grid, search, preview, presigned URLs)
   - Sub-drawer mode: **dynamic list** of buckets.
7. **Network** — reachability.
   - HTTP routes, WebSocket subscriptions, published ports, machine API forwarding, listener status per adapter, security (origin allowlist, denied requests).
   - Sub-drawer mode: **static menu** of sub-sections (Routes / WS / Ports / Listeners / Security).
8. **Machines** — host / guest lifecycle.
   - Machine list, detail (boot image, upgrade state, logs, services), actions (start/stop/restart/SSH/OS apply/remove).
   - Sub-drawer mode: **dynamic list** of machines.
9. **Observability** — debugging and audit.
   - Logs (filters, request ID join), events (unified feed), traces, scheduler lag, error groups.
   - Sub-drawer mode: **static menu** (Logs / Events / Traces / Errors) with route-driven filters in the URL.

Sidebar footer (visually grouped, separated by hairline divider, like
Convex's `explore` + `configure` groups):

10. **Settings** — server admin and integrations.
    - General, Endpoints, Deploys, Token/session, Environment, Integrations (adapter capability matrices: Convex / MongoDB / Firebase / Cloud Functions / Native), Shutdown.
    - Sub-drawer mode: **static menu** (Convex `SettingsSidebar` pattern).

That is **9 explore + 1 configure = 10 sidebar items**. This exceeds the
generic "5–7 max" sidebar heuristic, but operator consoles legitimately
run hotter (Podman has 6 root + 11 Kubernetes nested; Convex has 6
explore + 2 configure = 8; Docker Desktop has 7+). Operators tolerate
denser IA when each item maps to a stable mental model. Mitigation: the
sidebar collapses to icon-only mode (Phase O2), and `⌘K` palette
remains the primary navigation accelerator (already shipped).

### Tenant scope semantics

The tenant selector lives in the **top horizontal nav**, adjacent to the
logo, persistent across every section. Each section declares its tenant
scope as one of three states:

| Scope state | Selector visual | Sections | Behavior |
| --- | --- | --- | --- |
| `scoped` | active, clickable | Compute, Schedules, Storage, Files, Observability | Selecting a tenant filters list data and re-fetches. URL reflects active tenant. |
| `system` | shows `_nimbus`, dimmed | Network (listener status), Machines, Services (placement is server-wide), Settings | Selector is visible but disabled with tooltip "Server-wide view"; clicking opens an inert menu showing only `_nimbus`. |
| `unset` | shows "Select tenant", warning tone | Compute / Schedules / Storage / Files / Observability when zero tenants exist | Inline empty-state in the content area with "Create tenant" CTA. |

The selector renders the same component across all three states — only
the disabled flag and label differ. This makes the affordance
discoverable even on screens where it does not apply (per research
finding D5: do not hide; disable with tooltip).

### Renames in this revision

- `Compute` no longer includes scheduled jobs, cron jobs, services, or
  service catalog. These become top-level. `Compute` keeps Functions
  and Runs.
- `Storage` no longer includes blob/file storage (it never did
  technically, but the term "Storage" colloquially covers both — this
  plan disambiguates: schema-aware data is `Storage`, opaque bytes
  is `Files`).
- `Service catalog` (referenced in `DESIGN.md` line 86) moves to
  `Services` section.
- `Function runner` stays inside `Compute → Functions` detail page.

### Routes (new + renamed)

Current routes (from `packages/nimbus-ui/src/routes/`):

```
__root.tsx           index.tsx            compute.tsx
compute_.runner.tsx  storage.tsx          storage_.$tenant.tsx
storage_.$tenant_.$table.tsx              network.tsx
machines.tsx         observability.tsx
observability_.runs_.$runId.tsx           settings.tsx
```

Target routes after Phase O1:

```
__root.tsx                                    -- shell composition
index.tsx                                     -- Overview
compute.tsx                                   -- Compute landing (Functions list)
compute_.runner.tsx                           -- Function runner (kept)
compute_.runs.tsx                             -- Runs list (moved out of Observability)
compute_.runs_.$runId.tsx                     -- Run detail (renamed from observability_.runs_.$runId.tsx)
schedules.tsx                                 -- NEW top-level
schedules_.$schedule.tsx                      -- NEW detail
services.tsx                                  -- NEW top-level (placeholder)
services_.$service.tsx                        -- NEW detail (placeholder)
storage.tsx                                   -- Storage landing
storage_.$tenant.tsx                          -- Tenant detail
storage_.$tenant_.$table.tsx                  -- Table browser
files.tsx                                     -- NEW top-level (placeholder)
files_.$bucket.tsx                            -- NEW (placeholder)
network.tsx                                   -- Network landing
machines.tsx                                  -- Machines landing
machines_.$machine.tsx                        -- NEW detail (placeholder; current is flat list)
observability.tsx                             -- Logs / events / traces (Runs removed)
settings.tsx                                  -- Settings landing
settings_.$page.tsx                           -- NEW dynamic sub-page
```

Placeholder routes render a token-respecting "Not yet implemented" empty
state with the sub-drawer wired (so the sub-drawer surface is exercised
end-to-end). The detail content lands in a follow-on plan.

## Authoritative `DESIGN.md` updates (Phase O1)

Phase O1 edits `DESIGN.md` to keep it canonical. Specifically:

- Lines 78–96 (Information Architecture section) rewritten to the table
  above (9 explore + 1 configure top-levels).
- Line 86 entry for `Compute` rewritten to drop "scheduled jobs, cron
  jobs, service catalog".
- A new `Schedules` row inserted after `Compute`.
- A new `Services` row inserted before `Storage`.
- A new `Files` row inserted after `Storage`.
- Section 5 (Core Screens) extended with `### Services`, `### Files`,
  `### Schedules` subsections describing first-required views (mirroring
  the existing `### Compute`, `### Storage` blocks).
- Section 9 (Layout System) rewritten to describe the three-pane shell
  (top nav + primary drawer + sub-drawer) and the tenant scope semantics
  table above.

## Architectural Decision (proposed, subject to Phase O0 confirmation)

Restructure the shell composition root rather than bolt the new panes onto
the existing `<Sidebar />`. The target tree in `__root.tsx`:

```
<AppErrorBoundary>
  <ThemeController />
  <KeyboardContract />
  <StalenessProvider>
    <div className="flex h-screen flex-col bg-app text-default">
      <TopNav />                                {/* new: logo + tenant selector */}
      <div className="flex min-h-0 flex-1">
        <PrimaryDrawer />                       {/* renamed from Sidebar; collapsible */}
        <SubDrawer />                           {/* new: static-menu | dynamic-list */}
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

- A three-slot layout (`PrimaryDrawer | SubDrawer | main`) makes drawer
  collapse a width transition on a single pane rather than a structural
  change. Collapsed: `PrimaryDrawer` reduces from `w-56` to `w-12`
  (icon-only); the rest of the row reflows naturally.
- The `<SubDrawer />` slot renders `null` when the active route does not
  contribute a sub-drawer, which avoids reserving dead space.
- Renaming `Sidebar` → `PrimaryDrawer` is a breaking rename, not a
  re-export. Pre-launch rules forbid compatibility shims.

If Phase O0 finds restructure is more disruptive than expected, the
fallback is additive: keep `Sidebar` as-is, mount `TopNav` above the
existing row, and mount `SubDrawer` between `Sidebar` and `<main>`.
Default position is restructure; alternative requires a recorded note in
the Execution Log.

## Phase Status Ledger

| Phase | Description | Status |
| --- | --- | --- |
| O0 | Confirm restructure vs additive + lock IA decision | `todo` |
| O1 | IA migration: DESIGN.md + nav-entries + placeholder routes | `todo` |
| O2 | Collapsible primary drawer (width transition, persistence, a11y) | `todo` |
| O3 | Sub-drawer surface (mount point + dual-mode contributor API) | `todo` |
| O4 | Sub-drawer consumers: Storage, Compute, Schedules, Services, Files, Machines, Observability, Settings | `todo` |
| O5 | Top horizontal nav with logo + tenant selector trigger | `todo` |
| O6 | Tenant scope wiring (`scoped` / `system` / `unset` semantics) | `todo` |
| O7 | Live verification matrix (Mode × Palette × Drawer-state × Tenant-scope) | `todo` |
| O8 | Plan close: README registration, archive hand-off | `todo` |

Exactly one phase is `in_progress` at a time. Update this ledger and the
phase's own section before/after each work block.

## Phase Order and Dependencies

- O0 → O1: decision frozen before IA edits hit the worktree.
- O1 → O2: nav entries exist before the drawer that renders them is built.
- O2 → O3: sub-drawer must coexist with collapsed primary drawer (O2
  width-transition must be done before sub-drawer anchoring is verified).
- O3 → O4: surface exists before its first consumer route ships.
- O5 may run in parallel with O3/O4 once O1 is closed; it does not depend
  on the sub-drawer.
- O6 depends on O5 (selector UI) — wiring step, not visual step.
- O7 depends on O2–O6 landed in the working tree.
- O8 depends on O7 passing.

## Phase Details

### O0 — Confirm restructure & lock IA decision

**Status:** `todo`

**Goal:** Record decisions in the Execution Log before editing.
Eliminates risk of half-restructuring or half-migrating IA.

**Work:**

1. Re-read `packages/nimbus-ui/src/routes/__root.tsx`,
   `packages/nimbus-ui/src/shell/sidebar.tsx`,
   `packages/nimbus-ui/src/shell/system-tenant-lens.tsx`,
   `packages/nimbus-ui/src/shell/nav-entries.ts`, and
   `DESIGN.md` lines 78–250.
2. Decide restructure (default) vs additive.
3. Confirm IA: 9 explore + 1 configure top-levels. Note any IA-only
   objections (e.g. "operators prefer Services nested under Compute") in
   the Execution Log; the default stands unless evidence is recorded.
4. Confirm the `scoped` / `system` / `unset` tenant scope semantics.

**Acceptance:**

- Three decisions logged with one-line "why" each in the Execution Log.
- No code changes yet.

### O1 — IA migration (DESIGN.md + nav-entries + placeholder routes)

**Status:** `todo`

**Goal:** Land the IA change as a coherent baseline. `DESIGN.md`, the
`NAV_ENTRIES` array, and the route file set all agree.

**Work:**

1. Edit `DESIGN.md`:
   - Rewrite the Information Architecture table (current lines 82–96)
     to the 9 explore + 1 configure version above.
   - Add `### Services`, `### Schedules`, `### Files` core-screen
     subsections.
   - Rewrite "Layout System" section to describe the three-pane shell +
     tenant scope semantics.
2. Edit `packages/nimbus-ui/src/shell/nav-entries.ts`:
   - Add entries: `schedules`, `services`, `files`.
   - Drop "service catalog" / "scheduled jobs" references from the
     `compute` entry's implied scope.
   - Add a `tenantScope: "scoped" | "system"` field on each entry.
   - Group entries via a new `group: "explore" | "configure"` field
     (Convex pattern).
3. Scaffold placeholder routes:
   - `schedules.tsx`, `schedules_.$schedule.tsx`
   - `services.tsx`, `services_.$service.tsx`
   - `files.tsx`, `files_.$bucket.tsx`
   - `machines_.$machine.tsx`
   - `settings_.$page.tsx`
   - Move `observability_.runs_.$runId.tsx` →
     `compute_.runs_.$runId.tsx` and add `compute_.runs.tsx`.
   Each placeholder renders a token-respecting empty state with title,
   subtitle, and an honest "Not yet implemented in this phase" note.
4. Add `nav-entries.spec.ts` covering: entries have stable shape,
   `tenantScope` matches the scope table above, no duplicate `id`s.

**Acceptance:**

- `DESIGN.md` table and `NAV_ENTRIES` enumerate the same 10 sections.
- `npm run typecheck` clean.
- `npm run test` clean.
- All placeholder routes mount without console errors on the dev server.

### O2 — Collapsible primary drawer

**Status:** `todo`

**Goal:** Replace the fixed-width sidebar with a drawer that toggles
between full (`w-56`) and icon-only (`w-12`) states. State persists across
reloads. Keyboard accessible. Token-respecting. Renders the revised
`NAV_ENTRIES` with `explore`/`configure` group separation.

**Work:**

1. Extend `ui-store.ts` with `primaryDrawerCollapsed: boolean`,
   `togglePrimaryDrawer()`, and a `persistPrimaryDrawerCollapsed` helper
   matching the existing `persistMode`/`persistPalette` shape. Storage
   key: `nimbus-ui:primary-drawer-collapsed`.
2. Create `packages/nimbus-ui/src/shell/primary-drawer.tsx` (replacing
   `sidebar.tsx`):
   - Width transitions via Tailwind (`w-56` ↔ `w-12`) with
     `transition-[width] duration-150`.
   - Icon-only mode hides text label and `NavCount`; tooltip on hover
     uses the `title` attribute first (no new deps).
   - Toggle button at the bottom of the drawer (Convex pattern), with
     `data-testid="primary-drawer-toggle"`, `aria-expanded`,
     `aria-controls`, `aria-label="Collapse navigation"` /
     `"Expand navigation"`.
   - Logo wordmark **does not** live in the drawer anymore — it moves to
     `TopNav` in O5. (The drawer becomes pure nav.)
   - `explore` and `configure` groups separated by a hairline divider.
   - Focus management: toggling does not move focus off the toggle.
3. Update `__root.tsx` to render `<PrimaryDrawer />` (restructure path).
4. Delete `sidebar.tsx`. Update all imports.
5. Add `packages/nimbus-ui/src/shell/primary-drawer.spec.tsx`:
   - Toggle click flips `aria-expanded` and the persisted value.
   - Hydration from persisted `true` mounts collapsed.
   - Keyboard `Enter`/`Space` on toggle works.
   - All 10 nav entries reachable in both states (test IDs stable).
   - Group divider renders between explore and configure groups.

**Acceptance:**

- Toggle click changes width; reload restores it.
- All 10 nav entries reachable in both states.
- All vitest specs pass: `cd packages/nimbus-ui && npm run test`.
- `npm run typecheck` clean.
- Chrome DevTools MCP screenshot in both states for at least 2 palettes.

### O3 — Sub-drawer surface (dual-mode contributor API)

**Status:** `todo`

**Goal:** Introduce a persistent right-of-primary column. Supports two
contributor modes: **static menu** (fixed list of links, like Convex
`SettingsSidebar`) and **dynamic list** (resource list fed by a Convex
query, like Convex `DataSidebar`). Coexists with collapsed primary drawer.

**Work:**

1. Extend `ui-store.ts` with `subDrawerOpen: boolean` (default true) and
   `setSubDrawerOpen(open)`, persisted under
   `nimbus-ui:sub-drawer-open`. Width is fixed at `w-64` for v1 (no
   resize handle in this plan; Convex uses `react-resizable-panels` but
   that's deferred to keep deps lean).
2. Create `packages/nimbus-ui/src/shell/sub-drawer.tsx`:
   - Fixed width `w-64` when open, unmounts when closed.
   - `border-r border-app bg-surface`.
   - Header row: section title + close button
     (`data-testid="sub-drawer-close"`).
   - Optional search input slot (passed by the contributor).
   - Body: contributor children.
3. Define the contributor API. Recommend a route-driven slot pattern via
   TanStack Router context — each route exports an optional
   `subDrawer: () => SubDrawerSpec` that resolves at the layout level.
   Record the chosen mechanism in the Execution Log.

   ```ts
   type SubDrawerSpec =
     | { kind: "static"; title: string; items: SubDrawerItem[] }
     | {
         kind: "dynamic";
         title: string;
         search?: { placeholder: string };
         children: ReactNode;        // route renders its own list
       };
   ```

4. Add `sub-drawer.spec.tsx` covering: no contributor → renders null;
   static contributor → renders link list with active state; dynamic
   contributor → renders children; close button unmounts via store;
   persisted-closed hydrates.

**Acceptance:**

- Routes that opt in show a populated sub-drawer; routes that don't
  render no sub-drawer at all.
- Sub-drawer remains correctly anchored when primary drawer is collapsed.
- Both static and dynamic specs covered.
- Specs pass; typecheck clean.

### O4 — Sub-drawer consumers

**Status:** `todo`

**Goal:** Wire the eight sub-drawer contributors. Static where the
sub-section list is fixed; dynamic where it's a resource list.

**Sections × mode:**

| Route | Mode | Contributor source |
| --- | --- | --- |
| `/compute` | dynamic | `api.functions.list` (active tenant scope) |
| `/schedules` | static | tabs: `Scheduled` / `Cron` |
| `/services` | dynamic | placeholder list (`Coming soon`); supports the surface |
| `/storage/$tenant` | dynamic | `api.tables.list` for the tenant |
| `/files` | dynamic | placeholder list (`Coming soon`) |
| `/network` | static | sub-pages: Routes / WS / Ports / Listeners / Security |
| `/machines` | dynamic | `api.machines.list` |
| `/observability` | static | Logs / Events / Traces / Errors |
| `/settings` | static | General / Endpoints / Deploys / Token / Environment / Integrations / Shutdown |
| `/` | — | no sub-drawer |

**Work:**

1. For each route above, add the `subDrawer` route option returning the
   appropriate `SubDrawerSpec`.
2. Dynamic contributors use `useQuery` from `nimbus/react`, render items
   with `data-testid="sub-drawer-item-<id>"`, and navigate on click via
   the existing dynamic-segment routes.
3. Active item highlighting respects tokens (`bg-surface-2`,
   `text-default`, plus `border-l-2` with `border-brand`).
4. Empty / loading states match the existing `NavCount`-style
   placeholder (`·`) and `text-muted` empty-line.
5. Disabled / placeholder items (Services, Files) render with the
   `text-muted` tone and a tooltip explaining the placeholder.

**Acceptance:**

- Clicking a sub-drawer item navigates to the detail route and the
  selected item stays highlighted across navigation.
- Specs cover: list renders, click navigates, active selection, empty
  state, search filter (where present).

### O5 — Top horizontal nav with logo + tenant selector trigger

**Status:** `todo`

**Goal:** Mount the top bar. Owns logo + tenant selector trigger.
Visual-only in this phase — wiring to the system-tenant data path is O6.

**Work:**

1. Create `packages/nimbus-ui/src/shell/top-nav.tsx`:
   - Fixed height (`h-10`), `border-b border-app bg-surface`.
   - Left: `<LogoMark />` (lifted from the old `sidebar.tsx`,
     unchanged shape and viewBox) + the `Nimbus / operator console`
     wordmark, mirroring current typography.
   - Right of logo (12px gap): `<TenantSelector />` trigger.
   - Right edge: empty for now. Do not pre-build placeholder slots.
2. `<TenantSelector />` (visual stub): renders the current tenant from
   `api.system.status?.activeTenant ?? "_nimbus"` with a chevron.
   Disabled in this phase.
3. Add `top-nav.spec.tsx`: logo renders; tenant trigger present.

**Acceptance:**

- Top nav renders in all 6 Mode × Palette combinations without raw hex.
- Sidebar no longer duplicates the logo (post-O2).

### O6 — Tenant scope wiring

**Status:** `todo`

**Goal:** Wire the tenant selector to a real data path. Implement the
`scoped` / `system` / `unset` scope state per section.

**Work:**

1. Source the list of tenants:
   - Check `packages/nimbus-ui/convex/_generated/api.d.ts` for an
     existing `tenants.list` (or similar) query.
   - If absent: add `crates/nimbus-bin/.../system_tenant/tenants.ts` (or
     wherever the system-tenant functions live) exposing
     `tenants.list({ limit })` returning
     `{ id: string; name: string; backend?: string }[]`. Use the
     existing `_nimbus` system-tenant pattern (read
     `docs/adapters/convex/ai-guidelines.md` first).
2. Extend `ui-store.ts`:
   - `activeTenant: string` (default `"_nimbus"`, hydrate from
     `nimbus-ui:active-tenant` localStorage key).
   - `setActiveTenant(tenant)` — persists + triggers any consumer
     refetch.
3. Implement `<TenantSelector />` properly:
   - Click → dropdown listing tenants with the current one marked.
   - Each entry shows tenant name + backend pill (where available).
   - Keyboard navigation (↑↓ to move, ⏎ to select, Esc to close).
   - Selecting a tenant calls `setActiveTenant` and closes.
4. Per-section scope: `nav-entries.ts` already has
   `tenantScope: "scoped" | "system"`. `<TopNav />` reads the active
   route's owning section and:
   - `scoped`: selector active, shows active tenant, click opens menu.
   - `system`: selector disabled, shows `_nimbus`, tooltip "Server-wide view".
   - `unset` (no tenants exist *and* the route is `scoped`): selector
     shows "Select tenant" in a warning tone; route renders inline
     empty-state.
5. Routes that consume `activeTenant` (Storage tenant detail, Compute
   functions list when filtered by tenant) read it from the store and
   refetch on change. Verify `storage_.$tenant.tsx` already does this —
   if URL is the source of truth for tenant, reconcile so URL and store
   stay aligned (URL wins on conflict; store updates URL on selection).

**Acceptance:**

- Switching tenant updates the rendered active tenant and re-fetches
  tenant-scoped data on the current route.
- On Machines / Network / Settings routes the selector is visibly
  disabled with the system tooltip.
- Specs pass; typecheck clean.
- A live Chrome DevTools MCP run shows tenant switch causing route data
  to change.

### O7 — Live verification matrix

**Status:** `todo`

**Goal:** Prove the full shell works across the token matrix, drawer
states, and tenant scopes.

**Work:**

1. Boot the dev server: `cd packages/nimbus-ui && npm run dev`.
2. Use Chrome DevTools MCP (not `@playwright/mcp`):
   - Visit each of the 10 top-level routes.
   - For each, capture screenshots in the 6 Mode × Palette combinations.
   - Toggle primary drawer collapsed/expanded; confirm sub-drawer
     reanchors.
   - For sections with `system` scope, confirm the tenant selector shows
     disabled state.
   - For sections with `scoped` scope, switch tenant and confirm data
     refetch.
   - Hard reload after each persisted-state change.
3. Run `cd packages/nimbus-ui && npm run typecheck && npm run test && npm run build`.
4. Record screenshot paths and the matrix coverage list in the
   Execution Log.

**Acceptance:**

- Every screenshot lands without visual regression in token treatment.
- All vitest specs pass.
- `npm run typecheck` clean.
- `npm run build` clean.
- `cargo fmt --all --check`, `make clippy` clean for any incidental
  Rust touches (O6 may touch a system-tenant query file).

### O8 — Plan close, archive hand-off

**Status:** `todo`

**Goal:** Close the loop with `docs/plans/README.md` and the archived
Phase 1 desktop UI plan.

**Work:**

1. Add this plan to `docs/plans/README.md` under
   "Active execution plans".
2. Add a "Follow-up plan" reference at the top of
   `docs/plans/archive/desktop-ui-plan.md` pointing here (only if that
   plan has not yet been frozen).
3. Update the Status field at the top of this plan to `done` once
   O0–O7 are all `done` and the verification artifacts are committed.
4. Promote follow-up plans (if needed) for Services / Files / Schedules
   actual feature content — out of scope here.

**Acceptance:**

- `docs/plans/README.md` lists this plan.
- Final commit + push to `main` lands with a green `make ci` (or the
  reduced subset relevant to JS-only changes if no Rust touched:
  `npm run typecheck && npm run test && npm run build`).

## Implementation Checkpoints

| Phase | Checkpoint commit subject (proposed) |
| --- | --- |
| O0 | "DU-shell O0: lock IA + restructure decisions in plan ledger" |
| O1 | "DU-shell O1: IA migration — DESIGN.md, nav-entries, placeholder routes" |
| O2 | "DU-shell O2: collapsible primary drawer with persistence" |
| O3 | "DU-shell O3: sub-drawer surface + dual-mode contributor API" |
| O4 | "DU-shell O4: sub-drawer consumers across 9 sections" |
| O5 | "DU-shell O5: top nav with logo + tenant selector trigger" |
| O6 | "DU-shell O6: tenant scope wiring (scoped/system/unset)" |
| O7 | "DU-shell O7: live verification across mode × palette × drawer × scope" |
| O8 | "DU-shell O8: close active plan, archive hand-off" |

Each checkpoint commits a focused baseline from the working tree (per
`feedback_commit_workflow.md`). No blob staging via `/tmp`.

## Verification Commands

- JS typecheck: `cd packages/nimbus-ui && npm run typecheck`
- JS tests (vitest + happy-dom): `cd packages/nimbus-ui && npm run test`
- JS build: `cd packages/nimbus-ui && npm run build`
- Dev server (manual + Chrome DevTools MCP):
  `cd packages/nimbus-ui && npm run dev` (binds `:5173`)
- Full repo CI: `make ci` (only required if O6 touches Rust)

## Risks & Open Questions

(captured from first-principles review on 2026-05-17)

1. **Services ↔ Machines duplication.** A long-running service has a
   service identity *and* a machine placement. Decision: canonical detail
   page lives under `Services`; the same service appears under its
   `Machine` as a placement row that links back to the service detail.
   Don't deep-link both ways with full detail pages. Confirm in O0.

2. **Runs hybrid (Compute ↔ Observability).** Runs are execution receipts
   *and* the primary incident-debugging surface. Plan: Runs primary
   under `Compute → Runs`; Observability surfaces the same data via
   filtered Logs and Traces (single store, two lenses). Avoid
   duplicating the list UI — `Observability` filters live logs/traces by
   `runId` rather than re-implementing a runs list.

3. **`Files` name overlap.** If Nimbus ever exposes "files inside a
   guest" (logs, sockets, mounted volumes), the top-level `Files` will
   collide. Mitigation: keep `Files` for blob/object storage (matches
   Convex / Firebase / Vercel terminology); name guest-side surfaces
   `Logs`, `Volumes`, `Mounts` under `Machines`. Revisit if collision
   becomes confusing.

4. **Zero-tenant new-install state.** Convex's `NentSwitcher` returns
   `null` when 0–1 components exist; this is wrong for tenants because
   the operator needs to create the first one. Plan: render the
   selector with "Select tenant" label in a warning tone, and the
   target route renders an inline "Create tenant" empty-state CTA.

5. **10 top-level items exceeds the "5–7 max" sidebar heuristic.**
   Operator consoles legitimately run hotter (Podman 6+11, Convex 8,
   Docker Desktop 7+). Mitigations already in plan: collapsible
   primary drawer (O2), `⌘K` palette as primary nav accelerator
   (already shipped). Re-evaluate if user testing shows confusion.

6. **`tenants.list` query may not exist yet.** O6 step 1 may need to
   land a small system-tenant query before the selector can be wired.
   Honor the `docs/adapters/convex/ai-guidelines.md` rules when adding
   one. If the surface is significant, lift it to a separate plan
   instead of inlining here.

7. **Sub-drawer resize.** v1 ships fixed `w-64`. Convex uses
   `react-resizable-panels` with persisted size. Defer to a follow-up;
   note in the Execution Log if O4 consumers complain about the fixed
   width (Functions tree may need more, Storage tables list less).

## Non-goals

- No changes to the Electron desktop shell (`nimbus/desktop`).
- No changes to the existing `SystemTenantLens` (`⌘\\`) overlay — it
  remains a separate surface.
- No new third-party UI dependencies (no `react-resizable-panels`, no
  `@radix-ui/react-tooltip`). Reuse `lucide-react`, `cmdk`, `sonner`,
  `tailwindcss`, Zustand, TanStack Router already present.
- No theming changes. The Mode × Palette token system is the contract.
- No actual feature content for Services / Files / Schedules — those
  ship as placeholders here. Follow-up plans land the real surfaces.
- No backwards-compatibility shims for the renamed `Sidebar` →
  `PrimaryDrawer`, nor for the moved `observability_.runs` →
  `compute_.runs` route.

## Execution Log

_Append-only. Entries dated. Capture: decision recorded, files touched,
verification run, anything surprising._

- **2026-05-17 (a)** — Plan promoted from prior compaction-prompt
  scratchpad. Initial Phase O0–O7 ledger.
- **2026-05-17 (b)** — Plan rewritten with first-principles IA review:
  promoted **Services**, **Schedules**, **Files** to top-level peers;
  introduced **tenant scope** semantics
  (`scoped` / `system` / `unset`); split sub-drawer into **static menu**
  and **dynamic list** modes (Convex `SettingsSidebar` vs `DataSidebar`
  patterns). Reference benchmarks recorded with file paths and line
  numbers for Convex, Podman, Render, Vercel, Firebase, Docker Desktop.
  Phase O1 added for IA migration; total phases now O0–O8.

## First Step When You Resume

1. Re-read this plan top-to-bottom, including the IA section.
2. Open Phase O0 by reading `packages/nimbus-ui/src/routes/__root.tsx`,
   `packages/nimbus-ui/src/shell/sidebar.tsx`,
   `packages/nimbus-ui/src/shell/system-tenant-lens.tsx`,
   `packages/nimbus-ui/src/shell/nav-entries.ts`, and `DESIGN.md` lines
   78–250.
3. Record the three O0 decisions (restructure-vs-additive, IA confirmation,
   tenant scope semantics) in the Execution Log.
4. Mark O0 `done` and O1 `in_progress` in the Phase Status Ledger.
5. Begin O1 — `DESIGN.md` IA section edit first, then `nav-entries.ts`,
   then placeholder routes.
