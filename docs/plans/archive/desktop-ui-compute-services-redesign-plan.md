# Desktop UI — Compute & Services Redesign

Status: done
Owner: desktop-ui workstream
Predecessor (closed, archived): `docs/plans/archive/desktop-ui-shell-overhaul-plan.md`
Related current references:
- `docs/architecture/sandbox/microvm-service-baseline.md`
- `docs/architecture/sandbox/macos-machine-flow.md`
- `docs/plans/archive/desktop-ui-plan.md`
- `docs/plans/archive/desktop-ui-shell-overhaul-plan.md`

## Why this plan exists

Two IA mistakes in the predecessor (now-archived) shell overhaul plan
need correction. Pre-launch policy applies: prefer breaking changes; no
compat shims; no feature flags for legacy behavior.

### Mistake 1: Services placed Operator-only

The predecessor declared, verbatim:

> The old top-level `Services` (promoted in the prior plan revision) →
> Operator-only.

That is wrong. Services is a primary **developer** surface, grounded
in `docs/architecture/sandbox/microvm-service-baseline.md`:

- developers author the spec — `compose.yaml` is a developer-owned file
  that lives next to function code;
- developers call services from JS — `ctx.services.<name>` and
  `ctx.services.get("<name>")` are runtime APIs invoked from function
  bodies (same inner loop as `ctx.db.*`);
- developers drive lifecycle — `nimbus compose up/down/ps/logs/inspect/top`
  is symmetric across Linux (krun microVMs) and macOS (containers inside
  the krunkit machine VM);
- tenant-scoped state is meaningful — a service's logs, ports, readiness
  state, env vars, and "which function references it" are all
  tenant-scoped facts a developer needs while building.

Operators care about the orthogonal half: cross-tenant placement,
fleet-level restart patterns, machine density, drift between declared
and running state.

The correct shape is **dual-persona**, parallel to how Observability
already exists in both views with different default scope.

### Mistake 2: Compute treated as a kitchen sink

Today `/app/compute` renders four inner tabs — Services / Functions /
Scheduled / Cron — and the sub-drawer is a flat function list whose
entries link to a separate `/app/compute/runner` route.

- Services moves out (see Mistake 1).
- Scheduled and Cron belong under `/app/schedules` (the predecessor
  plan promoted Schedules as Developer-only; the current
  `/app/schedules` route already exists but the content lives under
  Compute by accident).
- The standalone runner route is the wrong shape. The Convex pattern
  the user pointed at uses a hierarchical function tree in the
  sub-drawer; click a function in the tree to show **source / info /
  logs / runs** on the right, with a docked Function Input/Output
  runner at the bottom of that detail page. The runner is part of the
  detail page, not a separate route.

Compute becomes Functions-only with a detail-pane pattern that mirrors
Convex.

## Outcome

Two surfaces, one shared detail-pane idiom (sub-drawer list →
right-pane tabs + docked action panel), reused across Compute
(Functions) and Services (Developer view).

### Final IA after this plan

| View | Section | Route | Sub-drawer | Detail |
|------|---------|-------|------------|--------|
| Developer | Compute | `/app/compute` | hierarchical function **tree** (folders → modules → functions) | `/app/compute/$function` with Statistics / Source / Logs / Runs tabs + docked Input/Output runner |
| Developer | Services | `/app/services` | dynamic list of this tenant's compose-declared services | `/app/services/$service` with Overview / Logs / Env / Ports / Code-refs tabs |
| Operator | Services | `/admin/services` | dynamic list of all services across tenants | `/admin/services/$service` with Placement / Restarts / Density / Drift |
| Developer | Schedules | `/app/schedules` | (existing) | Scheduled + Cron jobs move here from Compute |

Top-level nav after this plan:

- Developer: Overview · Compute · **Services** · Schedules · Storage ·
  Files · Observability · Settings
- Operator: System · Tenants · Machines · Network · Services ·
  Observability · Settings

### Removed routes (breaking, pre-launch)

- `/app/compute/runner` — folded into `/app/compute/$function` as
  the docked Function Input/Output panel. Deep links to the standalone
  runner are not preserved.

### Renamed / moved routes

- `compute_.runs_.$runId.tsx` stays put (Runs detail is reachable from
  the Function detail Runs tab).

## Phase status ledger

| Phase | Slice | Status |
|-------|-------|--------|
| CS0 | Read-in + baseline screenshot capture | done |
| CS1 | nav-entries IA correction (add developer Services) | done |
| CS2 | Strip Compute inner tabs; Functions-only landing | done |
| CS3 | Hierarchical function tree sub-drawer | done |
| CS4 | Function detail page (`/app/compute/$function`) with Statistics/Source/Logs/Runs tabs | done |
| CS5 | Docked Function Input/Output runner; delete `/app/compute/runner` standalone route | done |
| CS6 | Services developer view: `/app/services`, `/app/services/$service` | done |
| CS7 | Services operator view: replace placeholder content under `/admin/services*` | done |
| CS8 | Schedules/Cron migration from Compute to `/app/schedules` | done |
| CS9 | Verification matrix (build, run, screenshot, navigate via MCP) | done |
| CS10 | Plan close + archive | done |

## Roadmap detail

### CS0 — Read-in + baseline (in_progress)

Goal: capture the current state of `/app/compute` and `/admin/services`
in screenshots so the redesign has a clear before-image, and confirm the
codebase entrypoints this plan touches.

Touch list (reads only):

- `packages/nimbus-ui/src/routes/app/compute.tsx`
- `packages/nimbus-ui/src/routes/app/compute_.runner.tsx`
- `packages/nimbus-ui/src/routes/admin/services.tsx`
- `packages/nimbus-ui/src/shell/nav-entries.ts`
- `packages/nimbus-ui/src/shell/sub-drawer.tsx`
- `packages/nimbus-ui/convex/functions.ts`
- `packages/nimbus-ui/convex/services.ts`

Done when:

- baseline screenshots captured at 1440×900 (Developer Compute,
  Operator Services) and stored under
  `docs/plans/proof/desktop-ui-compute-services-redesign/before/`
- confirmed `api.functions.list` returns docs with `source` field
  available; confirmed `api.services.list` accepts `tenantId` filter

### CS1 — nav-entries IA correction

Edit `packages/nimbus-ui/src/shell/nav-entries.ts`:

- insert a developer `services` entry between `compute` and `schedules`:

  ```ts
  {
    id: "services",
    label: "Services",
    to: "/app/services",
    icon: Boxes,
    view: "developer",
    countQuery: api.services.list as unknown as CountQuery,
    countArgs: { tenantId: null, machineId: null, state: null, limit: 200 },
  }
  ```

- leave the operator `services` entry unchanged.

Done when:

- nav-entries.ts compiles, `npm run typecheck` clean for the package
- Developer drawer shows 8 entries in the order above; Operator drawer
  unchanged

### CS2 — Strip Compute inner tabs; Functions-only landing

Rewrite `packages/nimbus-ui/src/routes/app/compute.tsx` so the page
landing renders only the Functions table (no inner tab strip). Header
copy:

- title: "Compute"
- subtitle: "Functions registered to this tenant. Click a function in
  the drawer to see its source, logs, and runs, or invoke it from the
  docked runner."

Remove:

- `ComputeSection` type and `SECTIONS` array
- the `<nav aria-label="Compute sections">` tab strip
- the standalone `runner →` button in the header (the runner becomes
  docked under CS5)
- the `ServicesTable`, `ScheduledTable`, `CronTable` helpers (services
  move to `/app/services`; scheduled/cron move under CS8)
- the `services`, `scheduled`, `cron` queries from this file

Keep:

- `FunctionsTable` rendering as the default body
- `BundleHint`
- the `api.functions.list` query

Done when:

- `/app/compute` renders Functions table only
- no references remain to `services` / `scheduled` / `cron` data in
  `compute.tsx`
- `npm run typecheck` and `npm run build` clean

### CS3 — Hierarchical function tree sub-drawer

Replace the flat function list in `compute.tsx`'s sub-drawer spec with
a grouped tree.

Grouping rule (Convex convention): a function path
`folder/module:fn` splits on `/` for folder hierarchy and on `:` for
the trailing module:function pair. A path without `:` is treated as
`module:default`.

Tree shape:

- root → folder nodes (collapsible, default expanded)
- folder → module nodes (collapsible, default expanded)
- module → function leaves (clickable, links to
  `/app/compute/$function`)

Function-leaf rendering:

- monospace `fn` name (post-colon)
- right-aligned status chip if `lastStatus` present
- `data-testid="sub-drawer-fn-<path>"`

Implementation note: build a `TreeNode` type in
`packages/nimbus-ui/src/shell/function-tree.ts` (new file). Keep the
tree builder pure; the sub-drawer spec memoizes it.

Done when:

- sub-drawer shows grouped tree with collapsible folders/modules
- clicking a function leaf navigates to `/app/compute/$function` with
  the encoded path
- search input filters across all leaves (folder/module names match
  too)

### CS4 — Function detail page

New route file:
`packages/nimbus-ui/src/routes/app/compute_.$function.tsx`

Route segment encoding: function paths contain `/` and `:`. TanStack
Router file-routes use `$param`; we URL-encode the function path into
that one segment. Decode in the route loader / component.

Detail page layout:

- header: function path (mono), bundle id chip, last-status chip
- tab strip: Statistics · Source · Logs · Runs (default = Statistics)
- tab body fills remaining vertical space above the docked runner

Tab content:

- **Statistics** — invocations count, p50/p95/p99 latency, success
  rate, last 24h sparkline. If the underlying telemetry table is not
  yet populated, render a "No statistics yet" empty state with a hint;
  do not block this phase on backend work.
- **Source** — render `fn.source` in a monospace `<pre>` with line
  numbers. If `source` is empty, show "Source not bundled for this
  function" empty state. (No syntax-highlighting in this phase;
  follow-up plan can add Shiki.)
- **Logs** — reuse the observability log-row component, filtered by
  `functionPath`. Use `api.events.list` (or whichever query the
  observability page already uses) and pass the path through.
- **Runs** — list of recent runs via `api.runs.recent`, scoped to this
  `functionPath`. Each row links to
  `/app/compute/runs/$runId`.

Done when:

- `/app/compute/$function` renders for every leaf in the tree
- tabs route via search param `?tab=statistics|source|logs|runs`
- empty states render correctly when data is missing

### CS5 — Docked Input/Output runner; delete standalone route

Move the input/output runner from `compute_.runner.tsx` into a docked
panel at the bottom of `compute_.$function.tsx`.

Docked panel shape (Convex pattern):

- collapsed by default with a "Run function" header bar; click expands
- expanded shows two-pane: Input JSON editor (left) and Output panel
  (right)
- "Run" button + run state chip (idle / running / ok / error)
- Output panel renders the run result with copy-to-clipboard

The runner's logic (already in `compute_.runner.tsx`) is extracted to
`packages/nimbus-ui/src/components/function-runner/` as a reusable
component. The standalone file is deleted.

Delete:

- `packages/nimbus-ui/src/routes/app/compute_.runner.tsx`

Update any caller links:

- search the repo for `/app/compute/runner` and remove; the docked
  runner is reached by navigating to a function's detail page.

Done when:

- no references to `compute_.runner.tsx` remain
- routing builds clean
- runner functions identically when expanded inside the detail page

### CS6 — Services developer view

Two new route files:

- `packages/nimbus-ui/src/routes/app/services.tsx`
- `packages/nimbus-ui/src/routes/app/services_.$service.tsx`

Landing page (`/app/services`):

- header: "Services"
- subtitle: "Services this tenant declares in compose.yaml. They run as
  microVMs on Linux and as containers inside the developer machine VM
  on macOS."
- body: services table (Name / State / Image / Endpoint / Updated)
  scoped to active tenant via `api.services.list({ tenantId:
  activeTenant, ... })`
- sub-drawer: dynamic list of this tenant's services; clicking an item
  navigates to `/app/services/$service`

Detail page (`/app/services/$service`):

- header: service name, state chip, machine chip
- tabs: Overview · Logs · Env · Ports · Code refs
  - **Overview** — state, image, last restart, declared endpoint, raw
    `meta` JSON
  - **Logs** — placeholder for now (links to operator log fetcher in
    `microvm-service-baseline.md` § log path)
  - **Env** — env vars from `meta.env`, with `********` masking for
    keys ending in `KEY`, `TOKEN`, `SECRET`, `PASSWORD`
  - **Ports** — declared ports from `meta.ports`; on Linux note
    "TSI-mapped host port"; on macOS note "host-forwarded via gvproxy"
  - **Code refs** — list of functions that reference this service via
    `ctx.services.<name>` (placeholder for now; static-analysis is
    follow-up)

Done when:

- `/app/services` renders tenant-scoped table
- `/app/services/$service` renders all five tabs with appropriate
  empty states
- sub-drawer dynamic list works for both list and detail routes

### CS7 — Services operator view

Replace the placeholder in
`packages/nimbus-ui/src/routes/admin/services.tsx` with real content
matching the operator persona, and add detail route
`packages/nimbus-ui/src/routes/admin/services_.$service.tsx`.

Landing page:

- header: "Services (operator)"
- subtitle: "All services across all tenants. Placement, restart
  patterns, and machine density."
- body: cross-tenant services table (Name / Tenant / State / Machine /
  Endpoint / Restarts / Updated)
- sub-drawer: dynamic list of all services; item navigates to
  `/admin/services/$service`

Detail page:

- header: service name, state chip, tenant chip, machine chip
- tabs: Placement · Restarts · Density · Drift
  - **Placement** — which machine, why (label-match or fallback);
    `meta.placement` raw if present
  - **Restarts** — last N restarts over time (use whatever events table
    is available; placeholder OK)
  - **Density** — services-per-machine table for this service's
    machine
  - **Drift** — declared (from compose) vs running (from state) diff;
    placeholder OK if backend doesn't expose this yet

Done when:

- `/admin/services` no longer renders `PlaceholderPage`
- `/admin/services/$service` renders with all four tabs and empty
  states

### CS8 — Schedules / Cron migration

Move the Scheduled and Cron tables out of `compute.tsx` (already
removed in CS2) and ensure they render under `/app/schedules`.

`packages/nimbus-ui/src/routes/app/schedules.tsx` already exists. Read
it first; if it is still a placeholder, fold the existing
`ScheduledTable` + `CronTable` (from the deleted Compute helpers) into
it as an inner two-tab layout (Scheduled · Cron). Use the same data
shapes as before.

If `schedules.tsx` already renders this content, this phase reduces to
deleting any leftover scheduled/cron code from `compute.tsx`.

Done when:

- `/app/schedules` renders Scheduled and Cron tabs with tenant-scoped
  data
- no Scheduled/Cron rendering remains anywhere under `/app/compute*`

### CS9 — Verification matrix

Run the full proof loop:

- `npm run typecheck` clean
- `npm run build` clean for `packages/nimbus-ui`
- `cargo build -p nimbus-server` (rust-embed picks up the new `dist/`)
- `cargo build -p nimbus-bin`
- start nimbus from a clean data dir; mint a session cookie; navigate
  via chrome-devtools-mcp at 1440×900 to:
  - `/ui/app/compute` (Functions list + tree sub-drawer)
  - `/ui/app/compute/<first-function-path>?tab=statistics` (detail
    Statistics)
  - `/ui/app/compute/<first-function-path>?tab=source` (detail Source)
  - `/ui/app/services` (Developer services list)
  - `/ui/app/services/<first-service>` (Developer service detail)
  - `/ui/admin/services` (Operator services list)
  - `/ui/admin/services/<first-service>` (Operator service detail)
  - `/ui/app/schedules` (Scheduled + Cron tabs)
- assert no 404s, no console errors (allow expected Convex socket
  reconnect noise)
- store screenshots under
  `docs/plans/proof/desktop-ui-compute-services-redesign/after/`

Done when:

- all eight screenshots captured
- short verification note at
  `docs/plans/proof/desktop-ui-compute-services-redesign/README.md`
  pairing before/after and listing the commit shas that landed each
  phase

### CS10 — Plan close + archive

- flip Status to `done`
- flip every CS row in the phase ledger to `done`
- append an Execution Log entry summarizing the wave
- `git mv` this file into `docs/plans/archive/`
- update `docs/plans/README.md`:
  - remove from active plans
  - add to "Current Reference Baselines" with a one-paragraph
    summary
- commit + push

## Execution log

- (a) 2026-05-18 — plan promoted. Predecessor archived shell-overhaul
  plan's "Services Operator-only" decision identified as the IA
  mistake to correct. Compute kitchen-sink shape (Services/Functions/
  Scheduled/Cron inner tabs) identified as the second mistake. Scope
  bounded to nav-entries IA fix, Compute strip + Function detail pane
  + docked runner, Services dual-persona scaffold, Schedules/Cron
  migration. Pre-launch breaking-change policy applies; standalone
  `/app/compute/runner` route will be deleted, not preserved.
- (b) 2026-05-18 — CS0-CS10 landed. Compute now a Functions-only
  landing with a hierarchical function tree in the sub-drawer; a new
  per-function detail route (`/app/compute_/$function`) carries
  Statistics/Source/Logs/Runs tabs plus a docked Input/Output runner
  (the standalone `/app/compute/runner` route was deleted). Services
  is dual-persona: `/app/services` + `/app/services/$service` for
  developers (Overview/Endpoints/Health/Bundle), `/admin/services` +
  `/admin/services/$service` for operators (Placement/Restarts/
  Density/Drift); both views share `ServicesTable` and `ServiceDoc`,
  toggling `showTenantColumn`. Scheduled and Cron tables migrated out
  of Compute into `/app/schedules` with `?section=scheduled|cron` URL
  state. Vitest: 171/171 pass. Proof bundle:
  `docs/plans/proof/desktop-ui-compute-services-redesign/after/`
  (5 screenshots, all with clean console). Commits on `main`:
  baseline through CS8 `037d6cb1`; CS9 verification + operator
  summary-chip `whitespace-nowrap` fix `67928461`.

## Risks

1. **Function source size.** Some bundled function sources can be
   large; rendering raw in a `<pre>` is fine for v1, but a follow-up
   plan should add Monaco/Shiki and lazy-load.
2. **Function path encoding.** Paths contain `:` and `/`. Encode/decode
   carefully in the route segment; add a unit test in CS4.
3. **Services Code-refs tab.** Static analysis of which functions call
   `ctx.services.<name>` is not implemented backend-side yet.
   Placeholder is acceptable for this plan; follow-up plan owns the
   backend index.
4. **Operator Drift tab.** Same — declared-vs-running diff is not
   exposed by the system tenant today. Placeholder acceptable.
5. **MCP browser session cookie.** Verification depends on minting a
   session cookie from the local admin token. This is the same setup
   used by the predecessor plan; no new risk.

## First step when you resume

If you stop and pick this back up:

1. read this file
2. read `docs/plans/archive/desktop-ui-shell-overhaul-plan.md` § "Renames
   vs. prior single-view IA" for the original IA error context
3. continue from the first non-`done` row in the phase ledger
