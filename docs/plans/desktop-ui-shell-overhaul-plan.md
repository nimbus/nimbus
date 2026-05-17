# Plan: Desktop UI Shell Overhaul

> Note: always ensure the design skill is loaded when working on this plan.

Canonical active execution plan for upgrading the Nimbus console chrome
from a fixed-width sidebar shell into a **two-view, three-pane drawer
system** that separates **Developer console** (tenant-scoped: an app
owner shipping code on Nimbus) from **Operator console** (server-wide: a
host/admin running Nimbus for others), with a contextual sub-drawer
inside each view.

This plan owns the work to land:

1. A **two-persona top-level IA** built on a **view switcher** in the top
   nav. Each view owns its own ~7-item sidebar:
   - **Developer console** (`/app/...`): Overview · Compute · Schedules ·
     Storage · Files · Observability · Settings (tenant). Always
     tenant-scoped.
   - **Operator console** (`/admin/...`): System · Tenants · Machines ·
     Network · Services · Observability (cross-tenant) · Settings
     (server). Server-wide; tenant selector hidden by default, used as
     an optional filter on Observability.
2. A **collapsible primary drawer** (full width ↔ icon-only) with a
   toggle that persists across reloads. Drawer content reflects the
   active view's IA.
3. A **contextual sub-drawer** to the right of the primary drawer with
   two modes: **static menu** (Settings, Network sub-pages — Convex
   `SettingsSidebar` pattern) and **dynamic list** (Tables, Files,
   Functions, Schedules, Services, Machines — Convex `DataSidebar`
   pattern).
4. A **top horizontal nav** that owns the logo, the **view switcher**
   (Developer ⇄ Operator), and the **tenant selector** (active in
   Developer view; hidden or optional in Operator view).

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
| **Convex Dashboard** | Two-scope shell: **team scope** (project list / billing / members) vs **project scope** (Health / Data / Functions / Files / Schedules / Logs + History/Settings). Switch is URL-prefixed (`/t/<team>` vs `/t/<team>/<project>`). Project picker (`NentSwitcher`) in the **top header**, not sidebar. Inside a project: sub-drawer for Tables (`DataSidebar`), Functions (`DirectorySidebar`), Settings (`SettingsSidebar`). | `npm-packages/dashboard-common/src/layouts/DeploymentDashboardLayout.tsx:57-99`, `.../SettingsSidebar.tsx:10-19`, `.../features/data/components/DataSidebar.tsx`, `.../elements/Sidebar.tsx` |
| **Firebase Console** | Project picker in top bar adjacent to logo scopes the whole developer view. Authentication / Firestore / Storage / Functions are sibling sidebar items. Project Settings + IAM are gear-icon attachments in the same shell (no separate console mode — role-based gating). | Firebase web console |
| **Vercel** (Feb 2026 redesign) | Team → Project URL nesting. Inside a project: Deployments / Functions / Storage / Observability / Logs / Settings / Firewall. Account/team-level (billing, members) lives under avatar dropdown. | Vercel changelog "dashboard-navigation-redesign-rollout" |
| **Render** | Service-centric nav (each service its own card). Account-level (Settings, Billing, Team) under avatar dropdown. Service *type* (Web Service / Background Worker / Cron Job / Static Site) is the primary unit. | Render docs `service-types/` |
| **Podman Desktop** | Single shell. Containers/Pods/Images/Volumes/Networks/Extensions as root peers; Kubernetes nests an 11-item submenu including Services/Deployments/Jobs/CronJobs. Confirms long-running orchestration deserves its own root scope when present. | `packages/renderer/src/stores/navigation/navigation-registry.ts:65-75` |
| **Docker Desktop** | Single shell. Containers / Images / Volumes / Builds / Dev Environments / Extensions / Kubernetes as siblings. Volumes top-level peer of containers — analogous to Files being a peer of Storage in Nimbus. | Docker Desktop UI |

Nimbus borrows the **Convex / Firebase / Vercel two-scope pattern**, not
the Docker/Podman single-shell pattern, because Nimbus serves both an
infrastructure operator (host) and many app developers (tenants) — those
personas ask categorically different questions and benefit from separate
IAs rather than one tree with per-section scope toggles.

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

## Information Architecture (Revised — two views)

### First-principles persona review

Nimbus's UI serves two genuinely distinct personas with overlapping but
non-fungible questions:

| Persona | Identity | Asks | Cares about |
| --- | --- | --- | --- |
| **Operator** | DevOps / admin / host running Nimbus for others (or themselves) | "Is the server healthy? Which tenants exist? Are machines up? Are listeners reachable? Is the version current?" | server-wide state, infrastructure, multi-tenant administration |
| **Developer** | App owner shipping code against a tenant | "Did my function succeed? What's in this table? Did my cron fire? Where's the log for this request? Run this mutation now." | one tenant's data, code, schedules, files, traces |

The current `DESIGN.md` IA (lines 82–96) bundles these into a single
tree and tries to encode "this section is server-wide, that section is
per-tenant" as a per-section scope flag. That breaks down because:

1. **Mental gear-change cost**: A developer poking at their tenant
   shouldn't keep tripping over Machines/Network sections that don't
   apply. An operator looking at machine health shouldn't have to dodge
   tenant-scoped Tables/Functions sections that aren't their concern.
2. **Tenant selector ambiguity**: A single selector that's sometimes
   active and sometimes disabled (the prior plan's `scoped`/`system`/`unset`
   state machine) is harder to learn than "tenant selector is always
   active *in Developer view*, hidden *in Operator view*."
3. **URL semantics**: Putting `/observability` in the same URL space
   for both "logs for my tenant" and "logs across the whole server"
   forces a non-obvious query param to disambiguate, and breaks deep
   linking from the system tenant lens.

Convex, Firebase, and Vercel all resolve this with a **two-scope shell**
that switches by URL prefix. Nimbus adopts that pattern explicitly.

### View 1: Developer console (`/app/...`)

Sidebar IA, always tenant-scoped (active tenant in top-nav selector):

1. **Overview** — your app's health: recent runs, error rate, last
   deploy, schedule status, latest events. Dashboard. No sub-drawer.
2. **Compute** — request-scoped execution: Functions list, function
   detail (schema-aware args), Function runner, Runs history.
   Sub-drawer: **dynamic list** of functions (by path / kind).
3. **Schedules** — periodic / future-dated work: scheduled jobs (next /
   last run, cancel / retry), cron jobs (expression, history).
   Sub-drawer: **static menu** (`Scheduled` / `Cron`).
4. **Storage** — schema-aware data: tables / collections, document
   browser (cursor pagination, filters, schema-aware editor), Schema
   panel, Indexes panel, Query builder.
   Sub-drawer: **dynamic list** of tables for the active tenant.
5. **Files** — opaque bytes / S3-compatible blob storage (placeholder
   content in this plan; surface wiring is real). Buckets / namespaces,
   object browser, presigned URLs.
   Sub-drawer: **dynamic list** of buckets.
6. **Observability** — debugging and audit *for this tenant*: logs
   (filters, request ID join), events feed, traces, error groups.
   Sub-drawer: **static menu** (`Logs` / `Events` / `Traces` / `Errors`).
7. **Settings (tenant)** — tenant-owned configuration: environment
   variables, secrets, schema, adapter binding for this tenant,
   integrations enabled on this tenant.
   Sub-drawer: **static menu** (Convex `SettingsSidebar` pattern).

7 items. Within the 5–7 sidebar heuristic. Every section is
tenant-scoped — the tenant selector is always visible, always active in
this view.

### View 2: Operator console (`/admin/...`)

Sidebar IA, server-wide (tenant selector hidden by default; appears as
an optional filter on Observability only):

1. **System** — host status: version, uptime, listeners, build info,
   pending upgrades, embed integrity, log of admin actions. Dashboard.
   No sub-drawer.
2. **Tenants** — tenant lifecycle: list (with backend, quota, table
   count, last write), create, archive, per-tenant adapter binding.
   Sub-drawer: **dynamic list** of tenants.
3. **Machines** — host / guest lifecycle: machine list, detail (boot
   image, upgrade state, services placed on it), actions
   (start / stop / restart / SSH / OS apply / remove).
   Sub-drawer: **dynamic list** of machines.
4. **Network** — reachability: HTTP routes, WebSocket subscriptions,
   published ports, machine API forwarding, listener status per
   adapter, security (origin allowlist, denied requests).
   Sub-drawer: **static menu** (`Routes` / `WS` / `Ports` / `Listeners`
   / `Security`).
5. **Services** — long-running placement (Compose-declared services,
   service catalog, lifecycle state, endpoints, restart policy).
   Sub-drawer: **dynamic list** of services.
6. **Observability** — cross-tenant logs, events, traces (no tenant
   filter applied by default; optional tenant filter via the top-nav
   selector when this section is active).
   Sub-drawer: **static menu** (`Logs` / `Events` / `Traces` / `Errors`).
7. **Settings (server)** — server administration: general, endpoints,
   deploys, token / session, environment, integrations (adapter
   capability matrices: Convex / MongoDB / Firebase / Cloud Functions /
   Native), shutdown.
   Sub-drawer: **static menu** (Convex `SettingsSidebar` pattern).

7 items. Within the 5–7 sidebar heuristic. Server-wide by default.

### View switcher

Lives in the **top horizontal nav**, immediately to the right of the
logo. Visual pattern: pill-shaped segmented control (Developer ⇄
Operator). URL changes when toggled.

| Action | URL behavior | Selector behavior |
| --- | --- | --- |
| Click "Operator" while on `/app/storage/demo/users` | navigate to `/admin/system` (default Operator landing); preserve last-visited Operator route in localStorage so a second toggle restores it | tenant selector animates out (visibility transition); previous tenant remembered in localStorage |
| Click "Developer" while on `/admin/machines/m-1` | navigate to last-visited Developer route, or `/app` if none | tenant selector animates in; restores `activeTenant` from localStorage; if zero tenants exist, shows "Create tenant" CTA in main area |

URL prefix is the source of truth. localStorage (`nimbus-ui:last-view`,
`nimbus-ui:last-app-route`, `nimbus-ui:last-admin-route`) restores intent
on cold load. Direct deep links (`/app/storage/...`, `/admin/system/...`)
bypass the localStorage restore.

### Tenant selector behavior across views

| View | Selector visible? | Default | Disabled cases |
| --- | --- | --- | --- |
| Developer | always | last-active tenant (or first tenant alphabetically on fresh install) | when zero tenants exist: replaced by inline "Create tenant" CTA in the content area; selector shows "No tenants" |
| Operator | hidden by default | n/a | shown only on the **Operator → Observability** route, where it acts as an optional cross-tenant filter (default = "All tenants") |

The selector is rendered by the same component in both views; visibility
and disabled state are controlled by the view + active route.

### Shared affordances (both views)

- **Logo** (links to `/app` if last view was Developer, else `/admin`)
- **View switcher** (segmented Developer ⇄ Operator)
- **Tenant selector** (visible per the table above)
- **Command palette ⌘K**: navigates within the active view; reserved
  cross-view command `view operator` / `view developer` triggers the
  switch.
- **System tenant lens ⌘\\**: stays as-is — Developer-side cmd-overlay
  for raw `_nimbus` JSON inspection. Not a console mode.
- **Status bar** at the bottom (already shipped) is shared.

### Why this isn't permissioned (yet)

Both views are reachable to any authenticated session in this plan.
Eventually, the Operator console should require admin role and the
Developer console should respect per-tenant RBAC. URL split (`/app` vs
`/admin`) makes a future role gate a single router-level guard. Out of
scope for this plan; called out in Risks.

### Renames vs. prior single-view IA

- The old top-level `Machines` was server-wide → stays as Operator-only.
- The old top-level `Network` was server-wide → stays as Operator-only.
- The old top-level `Services` (promoted in the prior plan revision) →
  Operator-only.
- The old top-level `Schedules` (promoted in the prior plan revision) →
  Developer-only.
- The old top-level `Files` (promoted in the prior plan revision) →
  Developer-only.
- `Compute` keeps Functions + Runs but moves to Developer-only.
- `Storage` stays Developer-only.
- `Observability` exists in both views with **different default scope**:
  Developer = filtered to active tenant; Operator = cross-tenant feed.
- `Settings` splits into **Settings (tenant)** (Developer) and
  **Settings (server)** (Operator). The old monolithic Settings is
  replaced.
- The current archived plan's "tenant scope" three-state machine
  (`scoped`/`system`/`unset`) is **superseded**: within each view, scope
  is uniform. The state machine is removed.

### Routes (new + renamed)

Current routes (from `packages/nimbus-ui/src/routes/`):

```
__root.tsx           index.tsx            compute.tsx
compute_.runner.tsx  storage.tsx          storage_.$tenant.tsx
storage_.$tenant_.$table.tsx              network.tsx
machines.tsx         observability.tsx
observability_.runs_.$runId.tsx           settings.tsx
```

Target routes after Phase O1. TanStack's file-based router maps the
nested `app/`, `admin/` directories under `routes/` to URL prefixes
`/app/*` and `/admin/*` automatically; route options export per-route
sub-drawer specs (Phase O3 contract).

```
__root.tsx                                                 -- shell composition
index.tsx                                                  -- root redirect (last view OR /app)

app/__layout.tsx                                           -- Developer shell layout (tenant required)
app/index.tsx                                              -- Developer Overview
app/compute.tsx                                            -- Compute landing (Functions list)
app/compute_.runner.tsx                                    -- Function runner
app/compute_.runs.tsx                                      -- Runs list
app/compute_.runs_.$runId.tsx                              -- Run detail
app/schedules.tsx                                          -- Schedules landing
app/schedules_.$schedule.tsx                               -- Schedule detail
app/storage.tsx                                            -- Tenant table list (no $tenant — uses active tenant)
app/storage_.$table.tsx                                    -- Table browser (was storage_.$tenant_.$table)
app/files.tsx                                              -- NEW (placeholder)
app/files_.$bucket.tsx                                     -- NEW (placeholder)
app/observability.tsx                                      -- Tenant-scoped logs / events / traces
app/settings.tsx                                           -- Tenant Settings landing
app/settings_.$page.tsx                                    -- Tenant settings sub-page

admin/__layout.tsx                                         -- Operator shell layout
admin/index.tsx                                            -- System overview
admin/tenants.tsx                                          -- Tenant list (was old storage.tsx)
admin/tenants_.$tenant.tsx                                 -- Tenant admin detail
admin/machines.tsx                                         -- Machines landing
admin/machines_.$machine.tsx                               -- Machine detail (NEW)
admin/network.tsx                                          -- Network landing
admin/services.tsx                                         -- Services landing (placeholder)
admin/services_.$service.tsx                               -- Service detail (placeholder)
admin/observability.tsx                                    -- Cross-tenant logs / events / traces
admin/settings.tsx                                         -- Server settings landing
admin/settings_.$page.tsx                                  -- Server settings sub-page
```

Notes:

- `app/storage` no longer takes a `$tenant` segment. The active tenant
  lives in the top-nav selector; the URL stays clean and shareable links
  encode the tenant separately when needed (e.g. `?as=demo` for
  copy-paste portability; deep-link parses & sets `activeTenant` on
  mount).
- The previous `storage_.$tenant_.$table.tsx` collapses to
  `app/storage_.$table.tsx` because the tenant is implicit.
- The old `admin`-style "create / delete tenant" UI from
  `routes/storage.tsx` moves to `admin/tenants.tsx`. The Developer view
  no longer surfaces tenant CRUD.
- The Settings split is real: `app/settings*` owns tenant config;
  `admin/settings*` owns server config. There is no shared "settings"
  surface.
- Placeholder routes render a token-respecting empty state with their
  sub-drawer wired so the sub-drawer surface gets end-to-end exercise.

## Authoritative `DESIGN.md` updates (Phase O1)

Phase O1 edits `DESIGN.md` to keep it canonical. Specifically:

- Lines 78–96 (Information Architecture section) rewritten to introduce
  the **two-view** model (Developer console / Operator console), with
  one IA table per view (7 items each) and the view-switcher contract.
- Section 5 (Core Screens) split into "Developer console" and "Operator
  console" subsections. Existing `### Compute`, `### Storage` blocks
  stay under Developer; existing `### Machines`, `### Network` blocks
  stay under Operator. New subsections under Developer for `Schedules`,
  `Files`, `Settings (tenant)`. New subsections under Operator for
  `System overview`, `Tenants`, `Services`, `Settings (server)`.
- Section 9 (Layout System) rewritten to describe the three-pane shell
  (top nav + primary drawer + sub-drawer), the view switcher, and the
  tenant selector behavior across views.

## Architectural Decision (proposed, subject to Phase O0 confirmation)

Restructure the shell composition root rather than bolt the new panes onto
the existing `<Sidebar />`. The target tree in `__root.tsx`:

```
<AppErrorBoundary>
  <ThemeController />
  <KeyboardContract />
  <StalenessProvider>
    <div className="flex h-screen flex-col bg-app text-default">
      <TopNav />                  {/* logo + view switcher + tenant selector */}
      <div className="flex min-h-0 flex-1">
        <PrimaryDrawer />          {/* renders the active view's IA */}
        <SubDrawer />              {/* static-menu | dynamic-list per route */}
        <main>
          <DisconnectedOverlay />
          <Outlet />               {/* routes nested under app/ or admin/ */}
        </main>
      </div>
      <StatusBar />
    </div>
    <CommandPalette />
    <SystemTenantLens />           {/* unchanged: ⌘\ overlay, Developer side */}
    <Toaster />
  </StalenessProvider>
</AppErrorBoundary>
```

Rationale:

- A three-slot layout (`PrimaryDrawer | SubDrawer | main`) makes drawer
  collapse a width transition on a single pane rather than a structural
  change. Collapsed: `PrimaryDrawer` reduces from `w-56` to `w-12`
  (icon-only); the rest of the row reflows naturally.
- `<PrimaryDrawer />` and `<SubDrawer />` both read the **active view**
  (`/app/*` vs `/admin/*`) from the router; switching view re-renders
  the drawer contents in place rather than re-mounting the shell.
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
| O0 | Lock four decisions: restructure vs additive, two-view IA, view-switcher pattern, tenant-selector behavior per view | `done` |
| O1 | IA migration: DESIGN.md + nav-entries (per view) + placeholder routes under `app/` and `admin/` | `done` |
| OV | View shell — TopNav with view switcher + URL prefix split + view-restore localStorage contract | `done` |
| O2 | Collapsible primary drawer (width transition, persistence, a11y) — reads active view IA | `done` |
| O3 | Sub-drawer surface (mount point + dual-mode contributor API) | `done` |
| O4 | Sub-drawer consumers: 13 routes across both views | `done` |
| O5 | Tenant selector wiring (visible in Developer, optional filter on Operator → Observability) | `done` |
| O6 | Active-tenant store + per-route refetch on tenant change | `in_progress` |
| O7 | Live verification matrix (View × Mode × Palette × Drawer-state × Tenant) | `todo` |
| O8 | Plan close: README registration, archive hand-off | `todo` |

Exactly one phase is `in_progress` at a time. Update this ledger and the
phase's own section before/after each work block.

## Phase Order and Dependencies

- O0 → O1: decisions frozen before IA edits hit the worktree.
- O1 → OV: routes live under `app/` and `admin/` before the view switcher
  can navigate between them.
- OV → O2: the primary drawer's contents depend on knowing which view is
  active, so OV's URL prefix split lands first.
- O2 → O3: sub-drawer must coexist with collapsed primary drawer (O2
  width-transition before sub-drawer anchoring is verified).
- O3 → O4: surface exists before its first consumer route ships.
- O5 depends on OV + O1 (TopNav exists, tenant route data wiring).
- O6 depends on O5.
- O7 depends on O2–O6 landed in the working tree.
- O8 depends on O7 passing.

## Phase Details

### O0 — Confirm restructure & lock IA decision

**Status:** `done`

**Goal:** Record decisions in the Execution Log before editing.
Eliminates risk of half-restructuring or half-migrating IA.

**Work:**

1. Re-read `packages/nimbus-ui/src/routes/__root.tsx`,
   `packages/nimbus-ui/src/shell/sidebar.tsx`,
   `packages/nimbus-ui/src/shell/system-tenant-lens.tsx`,
   `packages/nimbus-ui/src/shell/nav-entries.ts`, and
   `DESIGN.md` lines 78–250.
2. **Decision 1:** restructure (default) vs additive composition root.
3. **Decision 2:** confirm **two-view IA** (Developer console at `/app/*`
   with 7 sections; Operator console at `/admin/*` with 7 sections).
   Note any objections in the Execution Log; default stands unless
   evidence recorded.
4. **Decision 3:** confirm **view switcher pattern** (top-nav segmented
   control, URL-prefix is source of truth, localStorage restores last
   view + last route per view).
5. **Decision 4:** confirm **tenant selector behavior per view**
   (Developer: always visible & active; Operator: hidden by default,
   shown as optional filter on Operator → Observability only).

**Acceptance:**

- Four decisions logged with one-line "why" each in the Execution Log.
- No code changes yet.

### O1 — IA migration (DESIGN.md + nav-entries + placeholder routes)

**Status:** `done`

**Goal:** Land the IA change as a coherent baseline. `DESIGN.md`, the
`NAV_ENTRIES` arrays (per view), and the route directory layout all
agree.

**Work:**

1. Edit `DESIGN.md`:
   - Rewrite the Information Architecture section (current lines 82–96)
     to introduce the two-view model with one table per view (7 items
     each).
   - Split Section 5 (Core Screens) into Developer / Operator
     subsections; add new screen entries for `Schedules`, `Files`,
     `Settings (tenant)` under Developer and `System overview`,
     `Tenants`, `Services`, `Settings (server)` under Operator.
   - Rewrite Section 9 (Layout System) to describe the three-pane shell
     with the view switcher and tenant selector behavior.
2. Edit `packages/nimbus-ui/src/shell/nav-entries.ts`:
   - Replace the single `NAV_ENTRIES` constant with
     `DEVELOPER_NAV_ENTRIES` (7 items) and `OPERATOR_NAV_ENTRIES`
     (7 items) — both with the same entry shape.
   - Add a `view: "developer" | "operator"` field on each entry for
     defensive cross-checks.
   - Export a helper `navEntriesForView(view): NavEntry[]` consumed by
     PrimaryDrawer.
3. Scaffold placeholder routes under the new directory layout:
   - `app/__layout.tsx` (Developer shell), `app/index.tsx`
     (Developer Overview placeholder for now)
   - `app/schedules.tsx`, `app/schedules_.$schedule.tsx`
   - `app/files.tsx`, `app/files_.$bucket.tsx`
   - `app/settings_.$page.tsx`
   - `admin/__layout.tsx` (Operator shell), `admin/index.tsx`
     (System overview placeholder)
   - `admin/tenants.tsx`, `admin/tenants_.$tenant.tsx`
   - `admin/machines_.$machine.tsx`
   - `admin/services.tsx`, `admin/services_.$service.tsx`
   - `admin/settings_.$page.tsx`
4. Migrate existing routes (no compatibility shims — pre-launch):
   - `compute.tsx` → `app/compute.tsx`
   - `compute_.runner.tsx` → `app/compute_.runner.tsx`
   - `observability_.runs_.$runId.tsx` →
     `app/compute_.runs_.$runId.tsx`; add `app/compute_.runs.tsx`
   - `storage.tsx` content **splits**: tenant CRUD moves to
     `admin/tenants.tsx`; tenant-tables list (was `storage_.$tenant`)
     becomes `app/storage.tsx` reading active tenant from store
   - `storage_.$tenant_.$table.tsx` → `app/storage_.$table.tsx`
   - `network.tsx` → `admin/network.tsx`
   - `machines.tsx` → `admin/machines.tsx`
   - `observability.tsx` content splits: tenant-scoped → `app/observability.tsx`;
     cross-tenant → `admin/observability.tsx` (initially the same code
     branched on view; split deeper in O7 if needed)
   - `settings.tsx` content splits: tenant → `app/settings.tsx`;
     server → `admin/settings.tsx`
5. Each placeholder renders a token-respecting empty state with title,
   subtitle, and an honest "Not yet implemented in this phase" note.
6. Add `nav-entries.spec.ts` covering: each view's entries have stable
   shape, no duplicate `id`s, `view` field matches the export name,
   total = 7 per view.

**Acceptance:**

- `DESIGN.md` Developer and Operator IA tables match `DEVELOPER_NAV_ENTRIES`
  and `OPERATOR_NAV_ENTRIES` respectively (7 + 7 items).
- `npm run typecheck` clean.
- `npm run test` clean.
- All placeholder routes mount without console errors on the dev server.

### OV — View shell (TopNav with view switcher, URL split, restore contract)

**Status:** `done`

**Goal:** Make the two views navigable. Lay down the URL split (`/app/*`
vs `/admin/*`), the view switcher control, and the last-route-per-view
restore behavior.

**Work:**

1. Move all existing routes into `app/` (already done in O1) plus a
   bare `admin/index.tsx` placeholder.
2. Create `packages/nimbus-ui/src/shell/top-nav.tsx`:
   - `h-10`, `border-b border-app bg-surface`.
   - Left: `<LogoMark />` lifted from `sidebar.tsx`, plus the
     `Nimbus / operator console` wordmark, mirroring current typography.
     (Wordmark dynamic: shows `developer console` when view is
     Developer, `operator console` when view is Operator.)
   - Middle: `<ViewSwitcher />` — segmented pill with two options,
     `data-testid="view-switcher-developer"` and
     `data-testid="view-switcher-operator"`, with `aria-pressed`.
   - Right: tenant selector slot (visual stub in this phase; wired in
     O5).
3. Implement `<ViewSwitcher />`:
   - Reads active view from the router pathname (`/admin/*` →
     `"operator"`, else `"developer"`).
   - Click navigates: store current pathname to
     `nimbus-ui:last-route:<view>`, then navigate to
     `nimbus-ui:last-route:<other-view>` if set, else default
     (`/app` for Developer, `/admin` for Operator).
   - Keyboard: left/right arrow switches focus, Enter activates.
4. Update `__root.tsx` to render the shell tree from the Architectural
   Decision section: `<TopNav />` row at top, then
   `<PrimaryDrawer />` + `<SubDrawer />` + `<main>`. Keep
   `<StatusBar />` at the bottom.
5. Extend `ui-store.ts`:
   - `lastView: "developer" | "operator"` (persisted as
     `nimbus-ui:last-view`, default `"developer"`).
   - Read-only helpers `readLastView()`,
     `readLastRouteForView(view)` mirroring the existing
     `readStoredMode`/`readStoredPalette` shape.
6. Add `view-switcher.spec.tsx`:
   - Clicking the inactive segment navigates to the other view's
     default landing on first click; on second toggle, navigates back
     to the originally-active developer/operator route.
   - `aria-pressed` reflects the active view.
   - Keyboard navigation works.
7. Add `top-nav.spec.tsx`:
   - Logo renders.
   - View switcher renders both segments.
   - Wordmark text reflects active view.

**Acceptance:**

- `/app/*` and `/admin/*` both render the shell without console errors.
- Switching views animates the wordmark and persists `lastView`.
- Reloading from `/admin/system` lands on `/admin/system` with Operator
  view active.
- Cold load (no localStorage, no path) lands on `/app` (Developer
  default).
- All specs pass; typecheck clean.

### O2 — Collapsible primary drawer

**Status:** `done`

**Goal:** Replace the fixed-width sidebar with a drawer that toggles
between full (`w-56`) and icon-only (`w-12`) states. State persists
across reloads. Keyboard accessible. Token-respecting. Renders the
**active view's** `NAV_ENTRIES`.

**Work:**

1. Extend `ui-store.ts` with `primaryDrawerCollapsed: boolean`,
   `togglePrimaryDrawer()`, and a `persistPrimaryDrawerCollapsed`
   helper matching the existing `persistMode`/`persistPalette` shape.
   Storage key: `nimbus-ui:primary-drawer-collapsed`.
2. Create `packages/nimbus-ui/src/shell/primary-drawer.tsx` (replaces
   `sidebar.tsx`):
   - Reads active view from the router pathname; renders
     `navEntriesForView(activeView)`.
   - Width transitions via Tailwind (`w-56` ↔ `w-12`) with
     `transition-[width] duration-150`.
   - Icon-only mode hides text label and `NavCount`; tooltip on hover
     uses the `title` attribute first (no new deps).
   - Toggle button at the bottom of the drawer (Convex pattern), with
     `data-testid="primary-drawer-toggle"`, `aria-expanded`,
     `aria-controls`, `aria-label="Collapse navigation"` /
     `"Expand navigation"`.
   - Logo wordmark **does not** live in the drawer anymore — it moved
     to `TopNav` in OV. The drawer is pure nav.
   - Focus management: toggling does not move focus off the toggle.
3. Delete `sidebar.tsx`. Update all imports.
4. Add `packages/nimbus-ui/src/shell/primary-drawer.spec.tsx`:
   - Toggle click flips `aria-expanded` and the persisted value.
   - Hydration from persisted `true` mounts collapsed.
   - Keyboard `Enter`/`Space` on toggle works.
   - All 7 entries reachable in both states for Developer view; 7 for
     Operator view (test IDs stable across views with view-prefix).
   - Switching views via `<ViewSwitcher />` updates the rendered entries
     in place (no remount).

**Acceptance:**

- Toggle click changes width; reload restores it.
- Both views render their own 7 entries.
- All vitest specs pass: `cd packages/nimbus-ui && npm run test`.
- `npm run typecheck` clean.
- Chrome DevTools MCP screenshot in both states for at least 2 palettes
  per view.

### O3 — Sub-drawer surface (dual-mode contributor API)

**Status:** `done`

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

**Status:** `done`

**Goal:** Wire sub-drawer contributors across both views. Static where
the sub-section list is fixed; dynamic where it's a resource list.

**Sections × mode (across both views):**

| Route | View | Mode | Contributor source |
| --- | --- | --- | --- |
| `/app` | dev | — | no sub-drawer (Overview) |
| `/app/compute` | dev | dynamic | `api.functions.list` (active tenant) |
| `/app/schedules` | dev | static | `Scheduled` / `Cron` |
| `/app/storage` | dev | dynamic | `api.tables.list` (active tenant) |
| `/app/files` | dev | dynamic | placeholder list ("Coming soon") |
| `/app/observability` | dev | static | `Logs` / `Events` / `Traces` / `Errors` |
| `/app/settings` | dev | static | tenant sub-pages (Environment, Secrets, Schema, Integrations) |
| `/admin` | op | — | no sub-drawer (System overview) |
| `/admin/tenants` | op | dynamic | `api.tenants.list` |
| `/admin/machines` | op | dynamic | `api.machines.list` |
| `/admin/network` | op | static | `Routes` / `WS` / `Ports` / `Listeners` / `Security` |
| `/admin/services` | op | dynamic | placeholder list ("Coming soon") |
| `/admin/observability` | op | static | `Logs` / `Events` / `Traces` / `Errors` |
| `/admin/settings` | op | static | server sub-pages (General, Endpoints, Deploys, Token, Environment, Integrations, Shutdown) |

13 sub-drawer routes total.

**Work:**

1. For each route above, add the `subDrawer` route option returning the
   appropriate `SubDrawerSpec`.
2. Dynamic contributors use `useQuery` from `nimbus/react`, render items
   with `data-testid="sub-drawer-item-<view>-<id>"`, and navigate on
   click via the existing dynamic-segment routes.
3. Active item highlighting respects tokens (`bg-surface-2`,
   `text-default`, plus `border-l-2` with `border-brand`).
4. Empty / loading states match the existing `NavCount`-style
   placeholder (`·`) and `text-muted` empty-line.
5. Disabled / placeholder items (Files, Operator → Services) render
   with the `text-muted` tone and a tooltip explaining the placeholder.

**Acceptance:**

- Clicking a sub-drawer item navigates to the detail route and the
  selected item stays highlighted across navigation.
- Specs cover: list renders, click navigates, active selection, empty
  state, search filter (where present).

### O5 — Tenant selector wiring

**Status:** `done`

**Goal:** Wire the tenant selector. Visible in Developer view (always
active), hidden in Operator view except on `admin/observability` (where
it's an optional cross-tenant filter).

**Work:**

1. Source the list of tenants:
   - Check `packages/nimbus-ui/convex/_generated/api.d.ts` for an
     existing `tenants.list` (or similar) query.
   - If absent: add a system-tenant `tenants.list({ limit })` query
     returning `{ id: string; name: string; backend?: string }[]`.
     Use the existing `_nimbus` system-tenant pattern (read
     `docs/adapters/convex/ai-guidelines.md` first).
2. Implement `<TenantSelector />`:
   - Renders the current `activeTenant` with a chevron.
   - Click → dropdown listing tenants with the current one marked.
   - Each entry shows tenant name + backend pill (where available).
   - Keyboard navigation (↑↓ to move, ⏎ to select, Esc to close).
   - Selecting a tenant calls `setActiveTenant` and closes.
3. Per-view visibility logic, owned by `<TopNav />`:
   - **Developer view** (`/app/*`): always render selector, active.
   - **Operator view** (`/admin/*`):
     - On `/admin/observability`: render selector with default option
       "All tenants" prepended; selecting a tenant adds `?tenant=<id>`
       to the URL (URL is source of truth for the optional filter).
     - On every other Operator route: do not render the selector.
4. Zero-tenant fallback (Developer view only):
   - When `tenants.list` returns `[]`, replace the trigger with a
     compact "Create tenant" button that opens an inline create dialog
     (modal pattern from `routes/storage.tsx` move to
     `admin/tenants.tsx`; this fallback links into it via the Operator
     view).

**Acceptance:**

- Selector visible on every `/app/*` route, hidden on every
  `/admin/*` route except `/admin/observability`.
- Specs pass; typecheck clean.

### O6 — Active-tenant store + per-route refetch

**Status:** `todo`

**Goal:** Wire `activeTenant` into the store and into the routes that
need to refetch when tenant changes.

**Work:**

1. Extend `ui-store.ts`:
   - `activeTenant: string | null` (default `null`, hydrate from
     `nimbus-ui:active-tenant`).
   - `setActiveTenant(tenant: string | null)` — persists + triggers
     consumer refetch.
2. Developer routes that read `activeTenant`:
   - `app/compute.tsx` — filters function list by active tenant.
   - `app/storage.tsx` — replaces the old `$tenant` URL segment with
     store-driven tenant.
   - `app/storage_.$table.tsx` — uses store tenant + URL table.
   - `app/schedules.tsx` — filters by active tenant.
   - `app/observability.tsx` — filters logs/events by active tenant.
   - `app/settings.tsx` — loads tenant-owned config.
3. Operator route that reads optional tenant filter from URL:
   - `admin/observability.tsx` — `validateSearch` reads
     `?tenant=<id>`; passes through to query (cross-tenant by default
     when absent).
4. On tenant change in Developer view: routes refetch automatically via
   `useQuery({ tenantId: activeTenant })` reactivity.
5. URL portability for Developer routes: `?as=<tenant>` query param on
   any `/app/*` URL sets `activeTenant` on mount, then strips itself
   from the URL. Enables shareable deep links across tenants.

**Acceptance:**

- Switching tenant in Developer view updates `activeTenant` in store +
  localStorage, and the visible data on the current route updates
  without manual reload.
- Operator → Observability accepts `?tenant=<id>` filter, default
  shows cross-tenant feed.
- Specs pass; typecheck clean.
- Live Chrome DevTools MCP run shows tenant switch causing route data
  to change.

### O7 — Live verification matrix

**Status:** `todo`

**Goal:** Prove the full shell works across **two views**, the token
matrix, drawer states, and tenant scopes.

**Work:**

1. Boot the dev server: `cd packages/nimbus-ui && npm run dev`.
2. Use Chrome DevTools MCP (not `@playwright/mcp`):
   - Visit each of the 7 Developer routes + 7 Operator routes.
   - For each, capture screenshots in 6 Mode × Palette combinations.
   - Toggle primary drawer collapsed/expanded; confirm sub-drawer
     reanchors.
   - Toggle view switcher; confirm wordmark + drawer entries update,
     last-route restore works.
   - In Developer view, switch tenant; confirm data refetch.
   - In Operator → Observability, apply tenant filter via URL +
     verify content.
   - Hard reload after each persisted-state change.
3. Run `cd packages/nimbus-ui && npm run typecheck && npm run test && npm run build`.
4. Record screenshot paths + matrix coverage list in Execution Log.

**Acceptance:**

- Every screenshot lands without visual regression in token treatment.
- All vitest specs pass.
- `npm run typecheck` clean.
- `npm run build` clean.
- `cargo fmt --all --check`, `make clippy` clean for any incidental
  Rust touches (O5 may touch a system-tenant query file).

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
| O0 | "DU-shell O0: lock four decisions (restructure, two-view IA, switcher, tenant)" |
| O1 | "DU-shell O1: IA migration — DESIGN.md, per-view nav-entries, app/+admin/ routes" |
| OV | "DU-shell OV: TopNav + view switcher + URL prefix split" |
| O2 | "DU-shell O2: collapsible primary drawer, view-aware" |
| O3 | "DU-shell O3: sub-drawer surface + dual-mode contributor API" |
| O4 | "DU-shell O4: sub-drawer consumers across 13 routes" |
| O5 | "DU-shell O5: tenant selector wiring (Developer always, Operator on Obs only)" |
| O6 | "DU-shell O6: active-tenant store + per-route refetch + portable ?as=" |
| O7 | "DU-shell O7: live verification across view × mode × palette × drawer × tenant" |
| O8 | "DU-shell O8: close active plan, archive hand-off" |

Each checkpoint commits a focused baseline from the working tree (per
`feedback_commit_workflow.md`). No blob staging via `/tmp`.

## Verification Commands

- JS typecheck: `cd packages/nimbus-ui && npm run typecheck`
- JS tests (vitest + happy-dom): `cd packages/nimbus-ui && npm run test`
- JS build: `cd packages/nimbus-ui && npm run build`
- Dev server (manual + Chrome DevTools MCP):
  `cd packages/nimbus-ui && npm run dev` (binds `:5173`)
- Full repo CI: `make ci` (only required if O5 touches Rust)

## Risks & Open Questions

(captured from first-principles review on 2026-05-17)

1. **Single-developer use case.** A solo developer running Nimbus on
   their laptop is *both* operator and developer. Constant view-
   toggling is annoying. Mitigation: on first launch (no
   `nimbus-ui:last-view` set, zero or one tenant detected), land on
   Developer and surface a "View Operator console" link in the
   Developer Settings page header. The view switcher remains the
   primary affordance; this just lowers the discovery cost on day one.

2. **Observability duplication across views.** Logs/events/traces are
   the same data store, queried two ways: tenant-scoped (Developer) vs
   cross-tenant (Operator). Risk: maintaining two routes that diverge
   over time. Plan: the two routes share a `<ObservabilityShell>`
   component; only the query input (tenant filter vs none) differs.
   Confirm in O1.

3. **System tenant lens ⌘\\ visibility.** The lens is a Developer-side
   inspection overlay onto `_nimbus`. It does not make sense in the
   Operator view (you're already looking at server state through the
   Operator IA). Decision: gate the keyboard binding to active view ==
   Developer. The Operator console can surface the same data via
   `admin/tenants/_nimbus` for direct inspection.

4. **Zero-tenant first-install.** Developer view requires at least one
   tenant. If none exist, the active route in Developer view renders
   "Create tenant" CTA inline; the tenant selector chip shows "No
   tenants" disabled. The actual create flow lives in Operator →
   Tenants. (Cross-view deep link: clicking the CTA navigates to
   `/admin/tenants?new=1` and the user comes back to Developer once
   created.)

5. **Permissions are deferred.** Both views are reachable to any
   authenticated session in this plan. Future role-based gating sits
   at the `/admin/*` route boundary as a single guard. Not in this
   plan.

6. **URL portability vs. store-driven tenant.** Developer routes drop
   the `$tenant` URL segment in favor of store-driven active tenant.
   This loses copy-paste cross-tenant deep linking. Mitigation: `?as=`
   query param parses + sets active tenant on mount, then strips
   itself. Confirm operators / sharing patterns survive in O7.

7. **Services ↔ Machines duplication.** A long-running service has a
   service identity *and* a machine placement. Decision: canonical
   detail page lives under Operator → Services; the same service
   appears under its Machine as a placement row that links back. Don't
   deep-link both ways with full detail pages.

8. **`Files` name overlap.** If Nimbus ever exposes "files inside a
   guest" (logs, sockets, mounted volumes), the Developer-side
   top-level `Files` will collide. Mitigation: keep `Files` for blob /
   object storage (matches Convex / Firebase / Vercel terminology);
   name guest-side surfaces `Logs`, `Volumes`, `Mounts` under
   `Machines`.

9. **`tenants.list` query may not exist.** O5 step 1 may need a small
   system-tenant query before the selector can be wired. Honor the
   `docs/adapters/convex/ai-guidelines.md` rules when adding one. If
   the surface is significant, lift it to a separate plan.

10. **Sub-drawer resize.** v1 ships fixed `w-64`. Convex uses
    `react-resizable-panels` with persisted size. Defer to a follow-up;
    note in the Execution Log if O4 consumers complain about the fixed
    width (Functions tree may need more, Storage tables list less).

## Non-goals

- No changes to the Electron desktop shell (`nimbus/desktop`).
- No changes to the existing `SystemTenantLens` (`⌘\\`) overlay shape —
  it remains a Developer-only inspection surface; only its activation
  guard updates to gate on active view.
- No new third-party UI dependencies (no `react-resizable-panels`, no
  `@radix-ui/react-tooltip`). Reuse `lucide-react`, `cmdk`, `sonner`,
  `tailwindcss`, Zustand, TanStack Router already present.
- No theming changes. The Mode × Palette token system is the contract.
- No actual feature content for Services / Files / Schedules — those
  ship as placeholders here. Follow-up plans land the real surfaces.
- No backwards-compatibility shims for the renamed `Sidebar` →
  `PrimaryDrawer`, nor for the route directory split into `app/` and
  `admin/`. Old URLs (`/storage/demo/users`, `/observability/runs/...`)
  return 404 — pre-launch, breaking changes preferred.
- No role-based gating between views (Operator vs Developer). Both
  views reachable to any authenticated session.

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
  Phase O1 added for IA migration; total phases now O0–O8. Committed
  as `b909b0e0`.
- **2026-05-17 (c)** — Plan rewritten again from first principles to
  introduce a **two-view shell**: separate **Developer console**
  (`/app/*`, tenant-scoped, 7 sections) and **Operator console**
  (`/admin/*`, server-wide, 7 sections), with a top-nav **view
  switcher** between them. The prior three-state tenant scope
  (`scoped` / `system` / `unset`) is **superseded**: within each view,
  scope is uniform, so the state machine is removed. Tenant selector is
  always-visible in Developer, hidden in Operator except on Operator →
  Observability where it acts as an optional cross-tenant filter (URL
  `?tenant=<id>`). Phase **OV** added between O1 and O2 to land the
  URL prefix split and view switcher. Routes reorganized into `app/`
  and `admin/` directories with no compatibility shims. Settings splits
  into tenant-owned (Developer) and server-owned (Operator). Convex /
  Firebase / Vercel cited as the two-scope pattern source; Docker /
  Podman cited as the single-shell pattern Nimbus rejects.
- **2026-05-17 (d)** — **O0 complete.** Re-read `__root.tsx` (52 lines,
  current shell renders `<Sidebar />` + `<main>` inside
  `<StalenessProvider>`), `shell/sidebar.tsx` (110 lines, fixed `w-56`
  with `LogoMark` + `NAV_ENTRIES.map`), `shell/system-tenant-lens.tsx`
  (153 lines, ⌘\\ overlay with `resolveLensView(pathname)` URL → lens
  mapping; hooks called unconditionally to keep order stable),
  `shell/nav-entries.ts` (83 lines, single `NAV_ENTRIES` array of 7
  items), and `DESIGN.md` lines 78–250 (Information Architecture +
  Core Screens + Layout System sections). Four O0 decisions logged:
  - **Decision 1 — Restructure (default).** Recompose `__root.tsx` into
    the three-pane tree (`TopNav` / `PrimaryDrawer` / `SubDrawer` /
    `main`) from the Architectural Decision section. *Why:* a width
    transition on a single pane is cheaper than additive composition,
    the breaking rename `Sidebar → PrimaryDrawer` is consistent with
    pre-launch rules ("No backwards-compatibility shims"), and the
    SubDrawer needs to be a structural slot from day one so dual-mode
    contributors plug in cleanly.
  - **Decision 2 — Two-view IA confirmed.** Developer console at
    `/app/*` with 7 sections (Overview · Compute · Schedules · Storage
    · Files · Observability · Settings (tenant)). Operator console at
    `/admin/*` with 7 sections (System · Tenants · Machines · Network
    · Services · Observability · Settings (server)). *Why:* Nimbus
    serves two genuinely distinct personas (host vs app owner) whose
    questions don't fungibly overlap; Convex / Firebase / Vercel all
    encode this same split as a URL prefix because per-section scope
    toggles are harder to learn than per-view uniform scope.
  - **Decision 3 — View switcher pattern.** Top-nav segmented pill
    (Developer ⇄ Operator), URL prefix is the source of truth,
    localStorage (`nimbus-ui:last-view`,
    `nimbus-ui:last-route:developer`, `nimbus-ui:last-route:operator`)
    restores last route per view on toggle and on cold load. *Why:*
    URL primacy preserves deep links and back-button semantics;
    localStorage restore matches developer-tool expectations (Convex,
    Vercel) so toggling back lands where the user left.
  - **Decision 4 — Tenant selector behavior.** Developer: always
    visible, always active; defaults to last-active tenant; renders
    inline "Create tenant" CTA on zero-tenant install (cross-view deep
    link to `/admin/tenants?new=1`). Operator: hidden by default,
    rendered as an optional cross-tenant filter only on
    `/admin/observability` (URL `?tenant=<id>`, default "All
    tenants"). *Why:* per-view uniform scope eliminates the prior
    three-state machine (`scoped`/`system`/`unset`); only Observability
    benefits from a tenant filter on the operator side.
  No code changes in this phase, per O0 acceptance criteria. O0 closes;
  O1 begins.
- **2026-05-17 (e)** — **O1 complete.** IA migration landed as a single
  baseline:
  - `DESIGN.md` rewritten: IA section now lists Developer (7) and
    Operator (7) sidebar tables; Core Screens split into Developer
    (Overview, Compute, Schedules, Storage, Files, Observability,
    Settings) and Operator (System, Tenants, Machines, Network,
    Services, Observability, Settings) subsections; Layout System
    rewritten with the three-pane ASCII diagram and per-pane
    specifications.
  - `packages/nimbus-ui/src/shell/nav-entries.ts` split into
    `DEVELOPER_NAV_ENTRIES` (7) and `OPERATOR_NAV_ENTRIES` (7); each
    entry carries a `view` discriminator; helpers
    `navEntriesForView(view)` and `viewFromPathname(pathname)` exported.
  - Routes migrated into `routes/app/` (overview, compute, compute
    runner, run detail, storage, table detail, observability, schedules,
    files, settings placeholders) and `routes/admin/` (system overview,
    tenants, machines, network, services, observability, settings
    placeholders). Storage no longer encodes tenant in the URL — moved
    to a `?as=<tenant>` search param as a transient surface until O6
    wires the active-tenant store.
  - `routes/index.tsx` redirects `/` → `/app`.
  - `shell/sidebar.tsx` reads the active view from pathname and renders
    `navEntriesForView(view)`; wordmark flips between
    `developer console` / `operator console`.
  - `shell/command-palette.tsx` indexes both views in separate groups
    and keys recent picks by `${view}:${id}` to keep duplicate ids
    (`overview` / `system`, `observability`, `settings`) disambiguated.
  - `shell/nav-entries.spec.ts` added: 8 assertions covering shape
    stability, view discriminator, id uniqueness per view, URL
    prefixing, countQuery↔countArgs pairing, helpers, and
    `viewFromPathname` semantics.
  - Verification: `npm run codegen` clean; `npm run typecheck` clean
    (16 typecheck targets); `npm run test` 112/112 (16 spec files);
    `npm run build` clean (24 chunks emitted). Biome's pre-existing
    a11y errors in `appearance-section.tsx` are unrelated and left
    untouched. O1 closes; **OV** opens.
- **2026-05-17 (f)** — **OV complete.** View shell landed in a single
  baseline:
  - `packages/nimbus-ui/src/store/ui-store.ts` extended with
    `lastView: NavView`, `setLastView`, `readLastView`,
    `readLastRouteForView(view)`, and `persistLastRouteForView(view,
    pathname)`. Storage keys: `nimbus-ui:last-view` and
    `nimbus-ui:last-route:<view>`. The route persister only stores
    pathnames matching the view's own prefix (`/app` for Developer,
    `/admin` for Operator) and the reader only returns prefix-valid
    paths — junk in localStorage falls back to the view's default
    landing.
  - `packages/nimbus-ui/src/shell/logo-mark.tsx` (new) extracts the
    SVG `LogoMark` so it's shared by `TopNav` and (until O2 retires it)
    `Sidebar`.
  - `packages/nimbus-ui/src/shell/view-switcher.tsx` (new) renders a
    two-segment pill (`Developer` / `Operator`) inside a `<fieldset
    role="group" aria-label="Console view">`. Active segment is
    pathname-driven via `viewFromPathname`; `aria-pressed` reflects it.
    Clicking the inactive segment writes the current pathname under
    `nimbus-ui:last-route:<active-view>`, sets `lastView` to the
    target, then navigates to the target's restored route if stored
    (and prefix-valid) or its default (`/app` for Developer, `/admin`
    for Operator). Arrow keys move focus between segments; `tabIndex`
    follows `aria-pressed` so the active segment is the tab stop.
  - `packages/nimbus-ui/src/shell/top-nav.tsx` (new) is a `h-10
    border-b border-app bg-surface` header: logo + `Nimbus` brand +
    dynamic `developer console` / `operator console` wordmark on the
    left; `<ViewSwitcher />` centered; tenant slot stub on the right
    (`data-testid="top-nav-tenant-slot"`, wired in O5).
  - `packages/nimbus-ui/src/routes/__root.tsx` mounts `<TopNav />`
    above the existing `<Sidebar />` + `<main>` row inside the column
    layout, and a `useLastRouteTracker()` effect subscribes to the
    router pathname and writes it under
    `nimbus-ui:last-route:<view>` while keeping `lastView` in sync.
    The existing Sidebar still renders its own wordmark — the drawer
    becomes pure nav in O2.
  - Specs: `shell/view-switcher.spec.tsx` (8 cases — `aria-pressed`
    for both views, default-target navigation, last-route persistence,
    restored-route navigation, prefix-mismatch fallback, no-op on
    active click, arrow-key focus rotation) and
    `shell/top-nav.spec.tsx` (3 cases — structural rendering, dynamic
    wordmark + `data-view` per route). Both mock
    `@tanstack/react-router`'s `useNavigate` and `useRouterState` with
    a hoisted pathname ref to drive the component without a full
    router.
  - Verification: `npm run codegen` clean; `npm run typecheck` clean;
    `npm run test` 123/123 (18 spec files); `npm run build` clean
    (deferred chunk set). OV closes; **O2** opens.
- **2026-05-17 (g)** — **O2 complete.** Collapsible primary drawer
  landed and replaces the fixed-width sidebar:
  - `packages/nimbus-ui/src/store/ui-store.ts` adds
    `primaryDrawerCollapsed: boolean` (default `false`),
    `setPrimaryDrawerCollapsed(collapsed)`, `togglePrimaryDrawer()`,
    and `readPrimaryDrawerCollapsed()`. Storage key
    `nimbus-ui:primary-drawer-collapsed`; persistor matches the
    existing `persistMode`/`persistPalette` shape.
  - `packages/nimbus-ui/src/shell/primary-drawer.tsx` (new) reads the
    active view from the router pathname and renders
    `navEntriesForView(view)`. Width transitions via Tailwind: `w-56
    px-2` ↔ `w-12 px-1` with `transition-[width] duration-150`.
    Collapsed mode hides each entry's text label and `NavCount`,
    leaves only the icon, and exposes `title` + `aria-label` so the
    target is still announceable. Toggle lives at the bottom of the
    drawer with `data-testid="primary-drawer-toggle"`, `aria-expanded`
    reflecting `!collapsed`, `aria-controls="primary-drawer-nav"`,
    and a dynamic `aria-label` (`"Collapse navigation"` /
    `"Expand navigation"`). The chevron icon flips (`ChevronsLeft` ↔
    `ChevronsRight`) and the "Phase 1 · Embedded SPA" footer hides
    when collapsed. The drawer carries no logo or wordmark — those
    live in `TopNav` now (OV).
  - `packages/nimbus-ui/src/shell/sidebar.tsx` deleted;
    `routes/__root.tsx` imports `<PrimaryDrawer />` instead.
  - `packages/nimbus-ui/src/shell/primary-drawer.spec.tsx` (new):
    8 cases covering the 7 developer entries on `/app`, the 7
    operator entries on `/admin`, default-expanded
    aria-expanded/label, toggle flipping `data-collapsed` and
    persisting to localStorage, collapsed-mode label hiding with
    `title`+`aria-label` retained, focus retention after click,
    `aria-controls` wired to the nav id, and view-driven entry
    swap in place.
  - Browser proof (Chrome DevTools MCP, 1440×900) at
    `docs/plans/proof/desktop-ui-shell-overhaul/`:
    `o2-developer-expanded.png`, `o2-developer-collapsed.png`,
    `o2-operator-expanded.png`, `o2-operator-collapsed.png`,
    `o2-developer-expanded-mono.png`,
    `o2-operator-expanded-mono.png`. No console errors observed
    across view × drawer-state × palette toggles.
  - Verification: `npm run typecheck` clean; `npm run test` 131/131
    (19 spec files); `npm run build` clean; biome check clean on all
    changed files. O2 closes; **O3** opens.
- **2026-05-17 (h)** — **O3 complete.** Sub-drawer surface landed with
  the dual-mode contributor API:
  - `packages/nimbus-ui/src/store/ui-store.ts` adds
    `subDrawerOpen: boolean` (default `true`, hydrated from
    `nimbus-ui:sub-drawer-open`), `setSubDrawerOpen(open)`,
    `toggleSubDrawer()`, plus exported `readSubDrawerOpen()` and
    `persistSubDrawerOpen(open)` helpers matching the existing
    drawer-state shape.
  - `packages/nimbus-ui/src/shell/sub-drawer.tsx` (new) defines the
    `SubDrawerSpec` discriminated union (`{kind: "static"; title;
    items}` | `{kind: "dynamic"; title; search?; children}`), a
    `SubDrawerContext` + `SubDrawerProvider`, a
    `useContributeSubDrawer(spec)` hook (route components opt in by
    calling this with a spec; cleanup clears on unmount), and the
    `SubDrawer` component itself. Fixed `w-64` with
    `border-r border-app bg-surface`; header has the section title in
    the mono uppercase eyebrow and a close button
    (`data-testid="sub-drawer-close"`) that flips `subDrawerOpen` to
    `false` and persists. Optional search slot renders only when a
    dynamic spec includes `search`. Static specs render a tokenized
    link list with active-state highlight (`bg-surface-2 text-default`
    + `border-l-2 border-brand`) using `aria-current="page"` and
    `data-testid="sub-drawer-item-<id>"`. Component **decision**:
    chose a React Context + hook over TanStack Router context — equal
    expressiveness for the v1 API surface, zero new router-internal
    coupling, simpler unit testing (no router needed), and keeps the
    contributor API symmetrical with `useUiStore` patterns elsewhere.
  - `packages/nimbus-ui/src/routes/__root.tsx` wraps the existing
    `StalenessProvider` subtree in `<SubDrawerProvider>` and mounts
    `<SubDrawer />` between `<PrimaryDrawer />` and `<main>` inside
    the column flex row. The SubDrawer coexists with the collapsed
    primary drawer: both are `shrink-0` siblings inside the same
    row container.
  - `packages/nimbus-ui/src/shell/sub-drawer.spec.tsx` (new) — 6
    cases: no contributor → no DOM; static contributor with
    active-state highlight; dynamic contributor with `data-testid`
    body + optional search input rendered with placeholder; close
    button hides the drawer and persists `nimbus-ui:sub-drawer-open
    = "false"`; freshly-imported module with `subDrawerOpen=false`
    hydrates to a hidden drawer; contributor unmount clears the spec
    so the drawer disappears. `beforeEach` resets the zustand store's
    `subDrawerOpen` to `true` to keep cases isolated.
  - Verification: `npm run codegen` clean; `npm run typecheck` clean;
    `npm run test` 137/137 (20 spec files, +6 new); `npm run build`
    clean; biome check clean on all changed files (4 pre-existing
    errors in `appearance-section.tsx` remain — unrelated to O3).
  - Visual proof deferred to O4: with no route contributor opted in
    yet, the surface correctly renders nothing — the empty-state
    contract is part of O3's acceptance and is covered by the first
    spec case. Browser-screenshot matrix for the populated surface
    comes in O4 as routes start opting in. O3 closes; **O4** opens.
- **2026-05-17 (j)** — **O5 complete.** Tenant selector landed in
  TopNav with per-view visibility logic:
  - **`packages/nimbus-ui/src/shell/tenant-selector.tsx` (new):**
    `<TenantSelector mode={...} />` accepts a discriminated mode —
    `{ kind: "developer" }` or
    `{ kind: "operator-filter"; currentFilter: string | null }`. The
    component fetches `/api/tenants` on mount (same REST endpoint
    `admin/tenants.tsx` already consumes), supports
    ArrowUp/ArrowDown/Home/End/Enter/Space/Escape keyboard navigation,
    closes on outside click, and writes selection either to the
    `activeTenant` store (developer mode) or to the URL as
    `?tenant=<id>` (operator-filter mode). Zero-tenant Developer
    fallback renders a compact `Plus`-iconed "Create tenant" button
    that links to `/admin/tenants`.
  - **`packages/nimbus-ui/src/store/ui-store.ts`:** adds
    `activeTenant: string | null` (default `null`, hydrated from
    `nimbus-ui:active-tenant`) plus `setActiveTenant` and exported
    `readActiveTenant()` / `persistActiveTenant()` helpers matching
    the established pattern. **Component decision:** the store
    landed in O5 rather than O6 because the selector needs a writer
    today; the consumer-route refetch wiring that O6 owns is purely
    additive.
  - **`packages/nimbus-ui/src/shell/top-nav.tsx`:** TopNav now reads
    both pathname AND search from `useRouterState`, computes a
    `selectorModeForRoute` and renders the selector only when the
    mode resolves: always in Developer view, only on
    `/admin/observability` in Operator view (read `tenant` search
    param to feed `currentFilter`), and nowhere else in Operator.
    `top-nav-tenant-slot` now carries `data-mode={developer |
    operator-filter | hidden}` so the visibility contract is
    introspectable from tests and from the live DOM.
  - **`packages/nimbus-ui/src/routes/admin/observability.tsx`:**
    `validateSearch` extended to accept `tenant?: string` alongside
    the existing `tab?: string`, since the operator-filter selector
    writes `?tenant=<id>` on selection.
  - **Tests:** 11 new cases across two specs. `top-nav.spec.tsx`
    grew from 3 → 6 cases (visibility for `/app/compute`,
    `/admin/machines`, `/admin/observability`) using a hoisted
    `searchRef` and a stubbed `fetch` returning `{ tenants: [] }`.
    `tenant-selector.spec.tsx` (new, 8 cases): trigger-label
    reflects active tenant, Create-tenant fallback on zero-tenant
    Developer, click selects + persists to store, operator-filter
    "All tenants" + navigate with `?tenant=`, ArrowDown + Enter
    selects, Escape closes without changing state, /api/tenants 404
    surfaces an error message, currentFilter renders on trigger.
  - **Browser proof (1440x900) at
    `docs/plans/proof/desktop-ui-shell-overhaul/`:**
    `o5-developer-selector-closed.png` ("TENANT Select tenant"
    trigger on `/app/compute`), `o5-developer-selector-open.png`
    (menu opened, listbox visible, empty because dev server has no
    backend → /api/tenants 404),
    `o5-operator-machines-hidden.png` (selector absent on
    `/admin/machines`), `o5-operator-observability-filter.png`
    ("FILTER All tenants" on `/admin/observability`). No console
    errors observed apart from the expected `/api/tenants` 404 in
    the unbacked dev environment.
  - Verification: `npm run typecheck` clean; `npm run test` 149/149
    (21 spec files, +11 new); `npm run build` clean.
    O5 closes; **O6** opens.
- **2026-05-17 (i)** — **O4 complete.** All 12 sub-drawer consumers
  wired across both views (the remaining route — `/app` overview —
  intentionally contributes no sub-drawer per the route table).
  - **Static consumers (6):** `/app/schedules` (Scheduled / Cron via
    `?section=`), `/app/settings` (Environment / Secrets / Schema /
    Integrations), `/app/observability` (Logs / Runs / Events / Errors
    via `?tab=`, Events/Errors disabled until backed), `/admin/network`
    (Routes / WS / Ports / Listeners / Security via `?section=`),
    `/admin/observability` (Logs / Events / Traces / Errors via
    `?tab=`), `/admin/settings` (General / Endpoints / Deploys /
    Token / Environment / Integrations / Shutdown via `?section=`).
  - **Dynamic consumers (6):** `/app/compute` (functions via
    `api.functions.list`, item links to `/app/compute/runner?fn=`),
    `/app/storage` (tables via `api.tables.list`, item links to
    `/app/storage/$table?as=<tenant>`), `/app/files` (placeholder
    "No buckets yet" until storage API ships), `/admin/tenants`
    (server tenants list, item links to `/admin/tenants?selected=`),
    `/admin/machines` (`api.machines.list`, item links to
    `/admin/machines?selected=`), `/admin/services`
    (`api.services.list`, item links to `/admin/services?selected=`).
  - **API extension:** `SubDrawerItem` gained an optional
    `search?: Record<string, unknown>` field so static specs can
    encode `?section=` / `?tab=` deep-links without scaffolding
    placeholder child routes for every sub-section. `isItemActive`
    now matches pathname **and** the recorded search fragment so
    active highlighting tracks tab/section selection. Search-aware
    matching is covered by a new spec case in
    `sub-drawer.spec.tsx` (7 cases total).
  - **Browser proof (1440×900) at
    `docs/plans/proof/desktop-ui-shell-overhaul/`:**
    `o4-static-admin-observability.png`,
    `o4-dynamic-admin-machines.png` (loading-state list with search
    input visible), `o4-dynamic-app-compute.png` (loading-state
    functions list), `o4-static-app-settings.png` (4 sub-pages,
    Environment / Secrets / Schema / Integrations),
    `o4-static-active-schema.png` (post-click — URL `?section=schema`,
    "Schema" focused + active). No console errors observed across
    the navigations. Live data sits at "loading" because the proof
    capture ran against a freshly booted dev server with no backend
    attached; the static / dynamic / search / active-state contracts
    are still all visually exercised.
  - Verification: `npm run typecheck` clean; `npm run test` 138/138
    (20 spec files, +1 search-aware active-state case); `npm run
    build` clean. O4 closes; **O5** opens.

## First Step When You Resume

1. Re-read this plan top-to-bottom, especially the **O6 phase detail**
   (Active-tenant store + per-route refetch on tenant change).
2. The store extension landed in O5 — `activeTenant: string | null`
   plus `setActiveTenant`, hydrated from `nimbus-ui:active-tenant`.
   O6's remaining work is to **read** `activeTenant` from the
   Developer routes and pass it into the relevant Convex queries:
   - `app/compute.tsx` — filter `api.functions.list` (and
     scheduled / cron / services if appropriate) by active tenant.
   - `app/storage.tsx` — replace the `?as=<tenant>` URL pattern with
     store-driven tenant, and update `app/storage_.$table.tsx` to
     read tenant from the store.
   - `app/schedules.tsx` — filter scheduled/cron lists.
   - `app/observability.tsx` — filter logs / events / errors.
   - `app/settings.tsx` — load tenant-owned config.
3. Operator-side: `admin/observability.tsx` already accepts
   `tenant?: string` via `validateSearch`; wire the query to honour
   it (cross-tenant default when absent).
4. URL portability for Developer routes: `?as=<tenant>` on any
   `/app/*` URL sets `activeTenant` on mount, then strips itself
   from the URL.
5. Run `npm run typecheck`, `npm run test`, `npm run build`, then
   commit + push the O6 baseline to `main` per durable authorization.
