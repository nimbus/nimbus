# Nimbus UI Design System

This is the canonical product and interface design system for Nimbus operator
UIs, including the embedded `/ui/*` React console and the Electron desktop
shell. It follows the `DESIGN.md` pattern: keep enough visual, structural, and
interaction detail in plain text that agents can implement coherent UI without
rediscovering the product language from scratch.

## Product Stance

Nimbus is a local-first backend and service control plane. The UI is an
operator console, not a marketing site.

The first screen should be the usable product surface: health, resources,
recent activity, and the next concrete actions. Do not build a landing page,
hero section, illustrative splash screen, or feature tour as the app shell.

The UI must make three things feel like one system:

- Compute: functions, actions, HTTP routes, scheduled work, service runs,
  containers, microVMs, and macOS machine lifecycle.
- Storage: tenants, tables, collections, documents, schema, indexes,
  scheduled mutations, journals, and adapter-specific data shapes.
- Network: local server auth, HTTP endpoints, WebSocket subscriptions,
  published ports, machine API forwarding, and adapter listener status.

Adapters are lenses over the same Nimbus engine. The UI may use Convex,
MongoDB, Firebase, or Native wording inside adapter-specific views, but global
navigation and status should stay Nimbus-owned.

## Aesthetic Stance: Industrial Precision

Nimbus is industrial precision. Every pixel is data or affordance, nothing
is decoration. Lineage: Linear, GitHub CLI, Vercel — bold restraint, not
ornamentation.

Concretely:

- Tight grids, hairline borders, generous monospace, tabular numerals.
- Status is data, not decoration: states render as a labeled dot, not a
  full-color pill.
- Color is reserved for state and section identity. Surfaces are neutral.
- The interface should feel engineered — like a control panel for a system
  that an operator trusts. It should not feel "designed."
- Forbidden: gradients, blobs, bokeh, decorative orbs, hero illustrations,
  marketing copy, purple/blue dominance, soft pastels, drop shadows used
  as decoration, animated background graphics.

## Design Principles

1. **Operational Density**
   Show real state, controls, filters, and evidence. Favor tables, split panes,
   detail drawers, event timelines, and compact cards over editorial layouts.

2. **One Mental Model**
   Users should understand that Convex functions, MongoDB collections,
   Firestore documents, native REST tables, managed services, and machine
   lifecycle all flow through Nimbus. Avoid separate product shells per
   adapter.

3. **Adapter Honesty**
   Label unsupported or partial adapter capabilities directly. Do not copy a
   vendor console surface unless Nimbus implements the underlying behavior.

4. **Actionable Diagnostics**
   Every failure state should answer: what failed, where it failed, what
   request or resource ID identifies it, and which action is safe next.

5. **Local Trust**
   Local admin tokens, sessions, tenant identity, function identity, and
   machine actions must be visually explicit. Destructive actions require
   confirmation with resource names.

6. **No Legacy UX**
   Nimbus is pre-launch. Prefer clean, breaking UI contracts over compatibility
   detours. Do not create UI affordances for retired flows.

## Information Architecture

Nimbus serves two distinct personas. They ask categorically different
questions and benefit from separate top-level information architectures
rather than one tree gated by per-section scope toggles.

| Persona | Identity | Asks | Cares about |
| --- | --- | --- | --- |
| **Developer** | App owner shipping code against a tenant | "Did my function succeed? What's in this table? Did my cron fire? Where's the log for this request?" | one tenant's data, code, schedules, files, traces |
| **Operator** | DevOps / admin / host running Nimbus for others (or themselves) | "Is the server healthy? Which tenants exist? Are machines up? Are listeners reachable?" | server-wide state, infrastructure, multi-tenant administration |

The console renders these as **two views** with a **view switcher** in the
top horizontal nav. URL prefix is the source of truth:

- **Developer console** — `/app/*` — always tenant-scoped. Tenant selector
  always visible, always active.
- **Operator console** — `/admin/*` — server-wide. Tenant selector hidden
  by default; rendered as an optional cross-tenant filter on `/admin/
  observability` only (URL `?tenant=<id>`).

Switching the view persists `nimbus-ui:last-view` and the per-view
`nimbus-ui:last-route:developer` / `nimbus-ui:last-route:operator` so the
second toggle restores the previous route in each view. Cold load lands on
`/app` (Developer default) unless localStorage records a different last
view.

### Developer console — sidebar IA (`/app/*`)

| Section | Purpose | First required views |
| --- | --- | --- |
| Overview | This app's health and recent activity | Recent runs, error rate, last deploy, schedule status, latest events |
| Compute | Request-scoped execution | Functions list, function detail, function runner, runs |
| Schedules | Periodic and future-dated work | Scheduled jobs (next/last run, cancel/retry), cron jobs |
| Storage | Schema-aware data | Tables, document browser, schema panel, indexes, query builder |
| Files | Opaque bytes / blob storage | Buckets, object browser, presigned URLs (placeholder in this baseline) |
| Observability | Debugging and audit (this tenant) | Logs, events, traces, error groups |
| Settings (tenant) | Tenant-owned configuration | Environment, secrets, schema, integrations, adapter binding |

7 sections. Every section is tenant-scoped — the active tenant comes from
the top-nav selector, not the URL.

### Operator console — sidebar IA (`/admin/*`)

| Section | Purpose | First required views |
| --- | --- | --- |
| System | Host status | Version, uptime, listeners, build info, pending upgrades, embed integrity, admin audit |
| Tenants | Tenant lifecycle | List with backend/quota/table-count, create, archive, per-tenant adapter binding |
| Machines | Host/guest lifecycle | Machine list, detail (boot image, upgrade state, services placed on it), start/stop/restart/SSH/OS apply/remove |
| Network | Reachability | HTTP routes, WebSocket subscriptions, published ports, machine API forwarding, listener status, origin allowlist |
| Services | Long-running placement | Compose-declared services, service catalog, lifecycle state, endpoints, restart policy |
| Observability | Cross-tenant debugging and audit | Logs, events, traces, error groups — default cross-tenant; optional `?tenant=<id>` filter |
| Settings (server) | Server administration | General, endpoints, deploys, token/session, environment, integrations (adapter capability matrices), shutdown |

7 sections. Server-wide by default. Tenant selector appears only on
`/admin/observability`.

### Secondary navigation rules

- Within each view, every primary section can opt into a **sub-drawer**
  to its right with two modes: **static menu** (fixed list of sub-pages,
  e.g. Settings, Network) or **dynamic list** (resource list fed by a
  query, e.g. Storage tables, Compute functions, Tenants, Machines).
- Adapter capability matrices live under **Operator → Settings (server)
  → Integrations**, not as a top-level section. Adapter-specific resource
  views (a Convex function, a MongoDB collection) appear under their
  category surface with the adapter labeled inline.
- Use resource detail pages for durable objects: tenant, function, run,
  service, machine, table, collection, route, subscription, index.
- Use drawers for short-lived inspection: JSON value, log entry, run
  output, request error, pending action result.
- Use modals only for confirmation, creation, and credential reveal flows.
- The **system tenant lens** (⌘\\) is a Developer-side overlay onto
  `_nimbus`. It is gated to the Developer view; the Operator view inspects
  the same data through `/admin/tenants/_nimbus` instead.

## Core Screens

Each screen is owned by exactly one view. The screen entries below are
grouped by view. The Developer-side `Overview`, `Compute`, `Storage`, and
`Observability` screens were already specified in earlier revisions and
remain authoritative for the Developer side.

## Core Screens — Developer console

### Overview (Developer)

The Overview screen is a dense control panel:

- Health: server status, uptime, version, storage backend, adapter listeners.
- Compute: active functions, recent runs, failed runs, scheduler lag.
- Storage: tenant count, table/collection count, write activity, index health.
- Network: HTTP, WebSocket, MongoDB, Firebase, machine API listener state.
- Machines and services: state counts with direct links to details.
- Recent activity: unified event feed with level, source, request ID, time.

No large greeting, hero illustration, or marketing copy.

### Compute (Developer)

Compute owns request-scoped function execution for the active tenant.
Service lifecycle moved out to the Operator console (`Services`).

- Functions list: path, kind, adapter, bundle, args schema, returns schema,
  last run, failure rate, p95 duration when available.
- Function runner: schema-aware argument editor, identity/mock identity
  controls where supported, query result panel, logs/result correlation,
  and clear execution mode for queries, mutations, actions, HTTP
  handlers, and scheduled functions.
- Runs: status, function/action/route, request ID, duration, error,
  logs, trace waterfall. Filtered to the active tenant.

The Compute sub-drawer is a **dynamic list** of functions (grouped by
path / kind). Tenant is implicit from the top-nav selector — the runner
does not show a tenant chooser.

Convex-like function runner behavior is useful, but it must be Nimbus-aware:
show which adapter handles the function and which execution mode is in
play (query / mutation / action / HTTP route / scheduled job).

### Schedules (Developer)

Schedules owns periodic and future-dated work for the active tenant.

- Scheduled jobs list: function path, next run, last run, status,
  cancel / retry where supported.
- Cron jobs list: name, cron expression, next run, last run, history
  link to a run-level detail page.
- Schedule detail: queued runs, recent runs, error history, retry policy.

The Schedules sub-drawer is a **static menu** with two items
(`Scheduled` / `Cron`).

### Storage (Developer)

Storage owns user data and database structure for the active tenant.
Tenant lifecycle (create, archive) moved to **Operator → Tenants**; this
view assumes a tenant is selected.

- Table/collection tree with row/document counts and last write time.
- Document browser with cursor pagination, filters, sorting, column chooser,
  schema awareness, and stable keyboard navigation.
- JSON/BSON/Firestore value editor that preserves adapter-specific types.
- Document actions: insert new document, edit in a drawer with schema
  validation preview, delete with confirmation, and bulk delete only after
  explicit selection.
- Schema panel for optional Nimbus schemas and adapter-derived schema views,
  including create, edit, delete, and validation error display.
- Indexes panel with name, fields, status, usage when available, create/drop
  actions where implemented, and warnings about write cost or unsupported
  index types.
- Query builder that makes index use visible and refuses unbounded scans where
  the backend would be unsafe.

The Storage sub-drawer is a **dynamic list** of tables for the active
tenant. URL is store-driven (`/app/storage/<table>`), not
`/app/storage/<tenant>/<table>` — the tenant lives in the top-nav.

The Storage UI should feel familiar to Convex Data, MongoDB Atlas Data
Explorer, and Firebase Firestore Data, but the implementation should be one
Nimbus document browser with adapter-specific labels and type renderers.

### Files (Developer)

Files owns opaque-byte / S3-compatible blob storage for the active
tenant. Ships as a placeholder surface in this baseline; the routes and
sub-drawer are real, the underlying feature is not implemented yet.

- Buckets / namespaces list with object count and total bytes.
- Object browser with prefix navigation, last-modified / size columns,
  upload, download, copy presigned URL.
- Object detail drawer: metadata, content type, lifecycle policy if any.

The Files sub-drawer is a **dynamic list** of buckets.

The placeholder state honors the token system and renders an honest
"Not yet implemented" line — no fake bucket data, no synthesized objects.

### Observability (Developer)

Observability is the Developer-side debugging surface. Defaults to the
active tenant; never cross-tenant in this view. (The Operator console
owns the cross-tenant feed under `/admin/observability`.)

- Logs: structured records with level, timestamp, request ID, function
  path, tenant, search and filters.
- Events: ordered domain events (mutation applied, scheduler fired,
  service restarted) for the active tenant.
- Traces: per-request waterfall with span timing and inline log lines.
- Errors: grouped failures with last seen, count, sample traces.

The Observability sub-drawer is a **static menu**
(`Logs` / `Events` / `Traces` / `Errors`).

### Settings (tenant)

Tenant-owned configuration for the active tenant. Distinct from the
Operator-side **Settings (server)** — different surface, different
permissions.

- Environment variables: list, add, edit, delete; secret toggle.
- Secrets: redacted by default with reveal-on-click + audit.
- Schema: tenant-scoped schema declaration, validation status, history.
- Adapter binding: which adapter (Convex / MongoDB / Firebase / Native)
  this tenant routes through.
- Integrations enabled on this tenant.

The Settings (tenant) sub-drawer is a **static menu** of sub-pages
(`Environment`, `Secrets`, `Schema`, `Integrations`, `Adapter binding`).

## Core Screens — Operator console

### System overview (Operator)

The System screen is the Operator landing page. Server-wide, no tenant
context.

- Host status: version, build info, uptime, embed integrity hash.
- Listeners: per-adapter listener state (Convex HTTP/WS, MongoDB wire,
  Firebase REST/Listen, native WebSocket, machine API).
- Upgrades: pending release / upgrade state, last upgrade, current
  channel.
- Tenants snapshot: count by storage backend, busiest tenants, recent
  creates / archives.
- Machines snapshot: state counts, last boot, last upgrade.
- Recent admin actions: token rotation, tenant create, machine restart.

No sub-drawer.

### Tenants (Operator)

Tenants owns the tenant lifecycle (the Developer console can't create
tenants — that's an admin concern).

- Tenant list: name, backend, table count, quota, last write, current
  adapter binding.
- Create tenant: backend selector, adapter binding, optional schema
  bootstrap.
- Archive tenant: confirmation with resource-count warning.
- Per-tenant adapter binding override.
- Empty state on fresh install: prominent "Create your first tenant"
  CTA; matches the inline Developer-side fallback.

The Tenants sub-drawer is a **dynamic list** of tenants. Selecting a
tenant opens its admin detail page.

### Machines (Operator)

Machines owns host/guest platform lifecycle.

- Machine list: name, provider, architecture, OS image reference, digest,
  state, resource allocation, last boot, last upgrade.
- Machine detail: boot image, desired image, actual image, guest Nimbus
  version/hash, forwarded API, services placed on it, ports, logs,
  upgrade/rollback state.
- Actions: start, stop, restart, SSH, OS apply, OS upgrade, remove.
- macOS copy must be clear that services run inside the Linux guest and
  host actions converge machine state. Do not imply per-service nested
  microVMs on macOS.

The Machines sub-drawer is a **dynamic list** of machines.

### Network (Operator)

Network makes the active local topology inspectable.

- Local server endpoints: REST, Convex HTTP/WS, native WebSocket,
  Firebase REST/gRPC-Web/Listen, MongoDB wire listener.
- Route table: method, path, adapter, handler, auth requirement, last
  request.
- WebSocket subscriptions: tenant, query, client count, last delivery,
  error.
- Published ports: host port, guest port, service, machine, readiness.
- Machine API forwarding: socket path, SSH state, gvproxy/krunkit state
  on macOS, guest API version.
- Security: origin allowlist, session state, token rotation, denied
  requests.

The Network sub-drawer is a **static menu** (`Routes` / `WS` / `Ports`
/ `Listeners` / `Security`).

### Services (Operator)

Services owns long-running placement (Compose-declared services, service
catalog, lifecycle state, endpoints, restart policy). Ships as a
placeholder surface in this baseline.

- Service list: name, kind, lifecycle state, placement (machine), health,
  endpoints, restart policy.
- Service detail: backing image, environment, ports, dependencies,
  lifecycle history.
- Actions: start, stop, restart, drain, remove.

A service has both a service identity (here) and a machine placement
(under Operator → Machines). Cross-link both ways; do not duplicate the
full detail page on the machine side.

The Services sub-drawer is a **dynamic list** of services.

### Observability (Operator)

The Operator-side cross-tenant feed of the same data store. Defaults to
all tenants. An optional `?tenant=<id>` filter (set via the top-nav
selector when this route is active) narrows to one tenant without
leaving the Operator console.

- Logs: cross-tenant log stream with the same filter set as Developer.
- Events: cross-tenant ordered domain events.
- Traces: cross-tenant per-request waterfall.
- Errors: cross-tenant grouped failures.

The Observability sub-drawer is a **static menu** (`Logs` / `Events` /
`Traces` / `Errors`). The same `<ObservabilityShell>` component backs
both Developer and Operator routes; only the query input differs.

### Settings (server)

Server administration. Distinct from the Developer-side **Settings
(tenant)**.

- General: server name, build info.
- Endpoints: bind addresses, TLS posture, advertised URLs.
- Deploys: release channel, current release, rollout history.
- Token / session: admin token rotation, session policy.
- Environment: process-level env vars.
- Integrations: adapter capability matrices (Convex / MongoDB / Firebase
  / Cloud Functions / Native HTTP/WS).
- Shutdown: graceful shutdown with running-machine warning.

The Settings (server) sub-drawer is a **static menu** of sub-pages
(`General`, `Endpoints`, `Deploys`, `Token`, `Environment`,
`Integrations`, `Shutdown`).

### Operator → Settings (server) → Integrations (Adapters)

Adapter integration pages show how each protocol maps onto Nimbus. They
live under **Operator → Settings (server) → Integrations** as
capability/posture surfaces, not as top-level navigation:

| Adapter | Required UI surface |
| --- | --- |
| Convex | Functions, generated API refs, queries, mutations, actions, HTTP routes, live subscriptions, scheduler/crons, auth identity, runtime diagnostics |
| MongoDB | Listener status, driver URI, databases as tenants, collections, BSON documents, CRUD, aggregation coverage, indexes, transactions/sessions, change streams |
| Firebase | Project/default database mapping, Firestore collections/documents, query builder, WebSocket Listen status, indexes/rules posture |
| Cloud Functions | Target bindings, trigger registry, function list, invocation history, delivery model, retry status, deploy artifact source |
| Native HTTP/WS | REST endpoint catalog, tenant lifecycle, table schema, documents, scheduled mutations, crons, native WebSocket subscriptions |

Each adapter page needs a capability matrix with three states:

- Supported
- Supported with caveats
- Not claimed

Never hide caveats behind tooltips only. Caveats belong inline in the panel
where the user is about to depend on the feature.

## Layout System

The console uses a **three-pane shell** beneath a **top horizontal nav**:

```
┌───────────────────────────────────────────────────────────────────────┐
│ TopNav: logo · view switcher (Developer ⇄ Operator) · tenant select  │
├──────────────┬──────────────┬──────────────────────────────────────────┤
│              │              │                                          │
│  Primary     │  Sub-drawer  │  Main content                            │
│  drawer      │  (optional)  │  (route Outlet)                          │
│  (active     │  static menu │                                          │
│   view's     │  or dynamic  │                                          │
│   sidebar    │  list,       │                                          │
│   IA)        │  per route)  │                                          │
│              │              │                                          │
├──────────────┴──────────────┴──────────────────────────────────────────┤
│ Status bar: connection · embed integrity · build · time                │
└───────────────────────────────────────────────────────────────────────┘
```

### Top nav

A single horizontal row at the top of the window:

- **Left:** logo + dynamic wordmark (`Nimbus / developer console` or
  `Nimbus / operator console` depending on active view).
- **Middle:** **view switcher** — a segmented pill control with two
  options (Developer, Operator), keyboard accessible (←/→ to focus, Enter
  to activate), `aria-pressed` reflects active view.
- **Right:** **tenant selector** (visibility table below) and global
  controls (command palette button, theme toggle, session menu).

The view switcher is the source of truth for view, alongside the URL
prefix (`/app/*` → Developer, `/admin/*` → Operator). Clicking the
inactive segment navigates to the last-visited route in that view (or
the view's default landing) and persists `nimbus-ui:last-view`.

### Tenant selector behavior

| View | Selector visible? | Default | Notes |
| --- | --- | --- | --- |
| Developer | always | last-active tenant (or first tenant alphabetically on fresh install) | when zero tenants exist, the trigger is replaced by a compact "Create tenant" CTA that deep-links to `/admin/tenants?new=1` |
| Operator | hidden by default | n/a | rendered only on `/admin/observability` where it acts as an optional cross-tenant filter (default "All tenants"); selection encoded as `?tenant=<id>` |

The selector is rendered by the same component in both views; visibility
and the active-vs-filter mode are driven by view + active route.

### Primary drawer

The left-most column. Renders the active view's sidebar IA (7 items per
view; see Information Architecture above). Toggles between two widths:

- **Expanded** (`w-56` default): icon + label + count.
- **Collapsed** (`w-12`): icon only; label appears in a native `title`
  tooltip.

State persists to `nimbus-ui:primary-drawer-collapsed`. The toggle lives
at the bottom of the drawer (Convex pattern), is keyboard activatable
(Enter / Space), and never moves focus on toggle.

### Sub-drawer

A second column between the primary drawer and the main content. Rendered
when the active route opts into a sub-drawer; absent otherwise (the
content area reflows naturally). Fixed width `w-64` in this baseline.

Two contributor modes:

- **Static menu** — a fixed list of sub-pages with an active state.
  Used by Settings (both views), Network, Schedules, Observability.
  Pattern reference: Convex `SettingsSidebar`.
- **Dynamic list** — a resource list fed by a query, with optional
  search input. Used by Storage tables, Compute functions, Tenants,
  Machines, Services, Files. Pattern reference: Convex `DataSidebar`.

Routes declare their sub-drawer via a route-level `subDrawer` option
that resolves at the layout level. Routes without a sub-drawer reserve
no space.

### Main content patterns

- **Overview** (both views): responsive grid of compact status panels +
  a full-width activity table.
- **Resource list**: table on desktop, dense list on mobile, filters
  above.
- **Resource detail**: header summary, tabs, split panes for logs/JSON.
- **Data browser** (Storage): table plus right-side document drawer.
- **Logs / runs** (Observability): timeline table plus correlated
  detail drawer.

### Status bar

Persistent at the bottom across both views. Shows connection state,
embed integrity, build version, current time. Shared component.

### Responsive behavior

- **Desktop:** all three columns visible; primary drawer toggles
  collapse, sub-drawer toggles closed.
- **Tablet:** primary drawer collapses to icon rail by default;
  sub-drawer becomes an overlay sheet anchored to the right of the
  collapsed primary drawer.
- **Mobile:** primary drawer + sub-drawer collapse into a single
  hamburger sheet; bottom navigation surfaces the active view's
  sections; the view switcher moves into the session menu.

### Do-not list

- Do not put page sections inside decorative cards. Cards are for
  repeated items, small metrics, modals, and genuinely framed tools.
- Do not duplicate primary navigation in the sub-drawer. The sub-drawer
  is per-section; cross-section navigation always uses the primary
  drawer or the command palette.
- Do not mirror Settings between the two views. The split (tenant vs
  server) is deliberate and exclusive.

## Visual Language

Nimbus should feel crisp, technical, and calm.

### Product Palette

This palette governs the operator console (`packages/nimbus-ui/`) and every
native chrome surface in `nimbus/desktop`. For the logo, marketing surfaces,
favicon, app icon, and the desktop setup card, see **Brand Palette** below —
the two tiers are intentionally distinct.

The product palette has two orthogonal axes:

- **Mode** — `light` / `dark` / `system`. Controls neutrals + reading direction.
- **Palette** — `blue` (default) / `mono` / `warm`. Controls brand + accent +
  link identity. Each palette pairs a light variant with a dark variant from
  the Nimbus Color Palette (see Brand Palette below).

Both are user-controlled from Settings → Appearance and persist to
`localStorage` (`nimbus-ui:theme`, `nimbus-ui:palette`). The shell sets
`data-theme` and `data-palette` on `<html>` so the cascade resolves the
right token set per request.

Palette pairs (light → dark):

| Palette | Light variant     | Dark variant     | Use                              |
| ---     | ---               | ---              | ---                              |
| `blue`  | Cool Blue         | Night Blue       | Product default                  |
| `mono`  | Monochrome        | Reverse Mono     | Minimal / enterprise / print     |
| `warm`  | Warm              | Golden Hour      | Friendly / marketing-leaning     |

Colors are defined in OKLCH so light and dark perceptual lightness stay
parity-matched. Semantic state tokens (`--success`, `--warning`, `--danger`,
`--starting`, `--draining`, `--queued`, `--stale`, `--violet`) are stable
across all palettes — "Running = green" holds in every theme.

Tokens that swap per palette (Blue shown as the default):

| Token | Blue light (OKLCH) | Blue dark (OKLCH) | Use |
| --- | --- | --- | --- |
| `--bg` | `oklch(98% 0.005 248)` | `oklch(13% 0.026 263)` | App background |
| `--surface` | `oklch(100% 0 0)` | `oklch(19% 0.030 258)` | Panels, tables, popovers |
| `--surface-2` | `oklch(96% 0.008 248)` | `oklch(24% 0.030 258)` | Secondary panels, selected rows |
| `--border` | `oklch(91% 0.013 252)` | `oklch(32% 0.030 258)` | Hairline borders and dividers |
| `--border-strong` | `oklch(82% 0.020 250)` | `oklch(40% 0.030 258)` | Emphasis borders |
| `--text` | `oklch(20% 0.035 263)` | `oklch(91% 0.014 252)` | Primary text |
| `--muted` | `oklch(48% 0.028 255)` | `oklch(67% 0.026 248)` | Secondary text |
| `--brand` | `oklch(62% 0.20 258)` (`#3B82F6`) | `oklch(72% 0.17 248)` (`#60A5FA`) | Primary identity: active nav stripe, primary CTA fill, connection-state dot |
| `--accent` | `oklch(70% 0.13 207)` (`#06B6D4`) | `oklch(85% 0.10 197)` (`#67E8F9`) | Interactive feedback: focus ring, selection, `Running` state |
| `--link` | `oklch(54% 0.22 264)` (`#2563EB`) | `oklch(82% 0.11 247)` (`#93C5FD`) | Hyperlinks only — not a secondary accent |

Semantic tokens (stable across all palettes):

| Token | Light (OKLCH) | Dark (OKLCH) | Use |
| --- | --- | --- | --- |
| `--success` | `oklch(52% 0.14 145)` | `oklch(72% 0.16 145)` | `Ready`, `Healthy` |
| `--warning` | `oklch(65% 0.16 75)` | `oklch(78% 0.17 75)` | `Reconnecting`, `Degraded` |
| `--starting` | `oklch(70% 0.17 50)` | `oklch(80% 0.18 50)` | `Starting`, `Provisioning` |
| `--draining` | `oklch(55% 0.13 280)` | `oklch(72% 0.14 280)` | `Draining`, `Stopping` |
| `--queued` | `oklch(60% 0.020 240)` | `oklch(70% 0.020 240)` | `Queued`, `Pending` |
| `--danger` | `oklch(58% 0.20 25)` | `oklch(70% 0.20 25)` | `Failed`, destructive |
| `--stale` | `oklch(50% 0.012 240)` | `oklch(60% 0.012 240)` | Disconnected/stale (strikethrough) |
| `--violet` | `oklch(55% 0.18 295)` | `oklch(75% 0.16 295)` | Functions/runs only |

Rules:

- **Three identity tokens, three different jobs.** `--brand` carries
  primary identity (active nav, primary CTA, dominant brand fill).
  `--accent` is interactive feedback (focus, selection, `Running`).
  `--link` is hyperlinks. Never paint buttons with `--link`. Never paint
  active nav with `--accent` — that's `--brand`'s job.
- **State colors are universal.** Status state colors do not vary by
  palette. A red `Failed` chip looks the same in Mono as in Blue.
- **Status colors must always have text or icon labels.** Color alone is
  never the only signal.
- **Surfaces never use accent or brand as a fill.** Identity colors appear
  as a 1–2px left bar, an inline dot, a focus ring, a small icon, or a
  small CTA — never as a section background.
- Tailwind v4 `@theme` directive should expose the non-palette tokens as
  CSS variables; palette-scoped tokens live in `@layer base` under
  `[data-palette=…]` selectors.

### Brand Palette

The brand palette is **distinct from the product palette above**. Use it
only for:

- The logo mark and its variants (`docs/brand/logo/`, `nimbus-logo.svg`,
  `nimbus-mark.svg`)
- README hero images and marketing pages
- The favicon and desktop app icon
- The desktop "CLI not found" setup card (`cli-not-found.html`) — this is
  the user's *first* contact with the app and is intentionally brand-tier
- Print, social-media images, and external touchpoints

**Never** use brand-palette colors inside the operator console or native
chrome. The product tier wins inside the app; the brand tier wins outside
it. If you find yourself reaching for a brand color inside a console
surface, pick the equivalent product-tier token instead.

#### Two-Tier Bridge

Three values cross tiers, by design:

- **Brand.** Brand `#3B82F6` (Cool Blue primary, solid form, brand tier)
  drives `--brand` `oklch(62% 0.20 258)` / `oklch(72% 0.17 248)` in the
  product tier — same hex, used as primary identity inside the app.
- **Teal accent.** Brand "Interactive Elements" gradient
  `#67E8F9 → #06B6D4` (Tailwind cyan-300 → cyan-500) drives `--accent`
  `oklch(70% 0.13 207)` / `oklch(85% 0.10 197)` in solid form. The brand
  gradient is reserved for logos and marketing; the solid form is the
  in-app accent.
- **Ink.** Hex `#0F172A` is shared across tiers as primary text on light
  surfaces.

No other color crosses tiers.

#### Variants

| Variant       | Stroke (`--logo-stroke`) | Fill (`--logo-fill`) | Background |
|---------------|--------------------------|----------------------|------------|
| `warm`        | `#0F172A`                | `#FFE7B3`            | `#FFFAF2`  |
| `cool-blue`   | `#3B82F6`                | `#FFFFFF`            | `#F8FAFC`  |
| `night-blue`  | `#60A5FA`                | `#1E293B`            | `#0B1220`  |
| `monochrome`  | `#111827`                | `#FFFFFF`            | `#FFFFFF`  |
| `reverse-mono`| `#FFFFFF`                | `#111827`            | `#111827`  |
| `sunset-red`  | `#DC2626`                | `#FFFFFF`            | `#FEF2F2`  |
| `soft-purple` | `#9333EA`                | `#FFFFFF`            | `#FAF5FF`  |
| `golden-hour` | `#D97706`                | `#FFFFFF`            | `#FFFBEB`  |
| `slate`       | `#475569`                | `#FFFFFF`            | `#F1F5F9`  |

The canonical logo SVG (`packages/nimbus-ui/public/nimbus-logo.svg`) and
tight mark (`nimbus-mark.svg`) accept `--logo-stroke` and `--logo-fill` as
CSS variables. Variant rendering is parameter substitution — the path data
is identical across all variants.

#### Usage Guidelines

- **Warm** or **Golden Hour** — marketing pages, brand-forward touchpoints,
  app icon (the most marketing-facing in-product surface). Default for the
  desktop setup card.
- **Cool Blue** — product UI light-mode favicon. Matches the operator
  console's overall light-mode feel.
- **Night Blue** — product UI dark-mode favicon. Matches the operator
  console's overall dark-mode feel.
- **Monochrome** / **Reverse Mono** — minimal, enterprise, print. Tray
  icon uses monochrome on light menu bars; macOS auto-inverts for dark.
- **Sunset Red**, **Soft Purple**, **Slate** — reserved for future
  marketing variants and seasonal/event use; not currently wired in.

The completed execution record for brand rollout, including the variant
regenerator (`docs/brand/gen-variants.sh`) and per-surface wiring, lives
in `docs/plans/archive/brand-system-plan.md`.

### Typography

- Body / UI: system UI stack
  (`-apple-system, BlinkMacSystemFont, "Segoe UI", "Helvetica Neue", Arial, sans-serif`).
- Monospace: **JetBrains Mono** (self-hosted via `@fontsource/jetbrains-mono`)
  with `ui-monospace, SFMono-Regular, Menlo, Consolas, monospace` as
  fallbacks. Used for IDs, digests, request IDs, function paths, ports,
  bytes/duration values, code blocks, JSON/BSON values, and shell snippets.
- Body: 14px desktop, 15px mobile.
- Compact table text: 13px.
- Page title: 22px (single value, not a range).
- Section heading: 16px.
- Label/caption: 12px.
- Monospace baseline: matches body line-height so monospace IDs in a row
  align with surrounding sans-serif text.

Rules:

- Do not scale type with viewport width.
- Letter spacing is `0`. Monospace letter spacing is `-0.01em` so JetBrains
  Mono reads at the same visual cadence as body text inline.
- Reserve large display type for empty states and onboarding, not dashboards.
- Code, IDs, digests, function paths, ports, and bytes use monospace.
- **All numeric columns** (durations, counts, sizes, ports, rates, percentages,
  timestamps) must apply `font-variant-numeric: tabular-nums`. Without this,
  live tables jitter on every tick. This is a hard requirement, not a polish.
- Status badges use tabular lining figures so counters do not reflow.

### Spacing And Shape

- Base spacing: 4px grid.
- Dense table row: 36-40px.
- Comfortable row: 44-48px.
- Panel padding: 12-16px.
- Page gap: 16-24px.
- Radius: 6px default, 8px maximum for cards/panels.
- Icon button: 32px square, 36px on touch surfaces.

Stable dimensions are required for tables, metric panels, toolbars, counters,
and state badges so hover/loading states do not shift layout.

## Components

### Navigation

- Sidebar entries use Lucide icons plus labels.
- Active item uses a left accent bar or subtle filled background.
- Section groups are collapsible only when the information architecture grows.
- Tooltips are required for icon-only controls.

### Tables

Tables are the default shape for resources:

- Sticky header on scroll.
- Column resizing or at least column visibility for data-heavy tables.
- Row click opens detail; row checkbox selects for bulk actions.
- Inline actions appear on hover and are also reachable by keyboard.
- Empty state stays compact and includes the next useful action.
- Loading state preserves table geometry with skeleton rows.

### Forms And Editors

- Use segmented controls for mutually exclusive modes.
- Use toggles/checkboxes for binary settings.
- Use inputs/sliders/steppers for numeric values.
- Use menus/comboboxes for bounded option sets.
- Use JSON/code editors for document, argument, and config values.
- Validate on blur and before submit. Show field-specific errors.

### Badges

State badges render as a **labeled dot**, not a filled pill. The dot is
8px, the label uses tabular figures, the row stays calm. Filled pills are
reserved for adapter/kind/backend categorical badges.

State → token binding (mandatory; do not improvise mappings):

| State | Token | Dot glyph |
| --- | --- | --- |
| `Ready`, `Healthy` | `--success` | ● solid |
| `Running` | `--accent` | ● pulsing (respects `prefers-reduced-motion`) |
| `Starting`, `Provisioning` | `--starting` | ◐ half-filled |
| `Draining`, `Stopping` | `--draining` | ◐ half-filled |
| `Queued`, `Pending` | `--queued` | ○ outline |
| `NotReady`, `Degraded`, `Reconnecting` | `--warning` | ● solid |
| `Stopped` | `--muted` | ○ outline |
| `Failed`, `Crashed` | `--danger` | ● solid |
| `Stale` (post-disconnect) | `--stale` | ● solid + label strikethrough |
| `Unknown` | `--muted` | ? glyph |

Categorical (filled pill, monospace label, 11px):

- Function kind: `Query`, `Mutation`, `Action`, `HTTP`, `Scheduled`, `Cron`.
- Adapter: `Convex`, `MongoDB`, `Firebase`, `CloudFn`, `Native`.
- Backend: `redb`, `SQLite`, `Postgres`, `MySQL`, `libSQL`.

Badges are plain, compact, and readable. Do not use pill farms as decoration.
Do not place more than two categorical badges on the same row.

### Logs And Events

- Logs are a virtualized table by default.
- Required columns: time, level, source, request/run ID, message.
- Detail drawer shows structured fields and correlated entries.
- Search by request ID, execution ID, run ID, function path, source, and text.
- Preserve scroll position while new logs arrive.
- Provide pause/resume follow mode.

### Data Browser

- Use cursor pagination, not unbounded fetches.
- Show active filters and sort order as editable chips.
- Document values open in a drawer with JSON/BSON/Firestore type fidelity.
- Inline editing is allowed only when the backend supports the exact mutation
  and schema validation result is visible before commit.
- Bulk edits require explicit selection and confirmation.

### Function Runner

- Argument editor must be schema-aware when `argsSchema` is available.
- Query runs can auto-refresh/react when backed by subscriptions.
- Mutations and actions run only on explicit submit.
- Results and logs share the same request/run correlation ID.
- Identity controls are labeled as simulated/admin-local identity unless a
  real auth provider is active.

### Command Palette (⌘K)

A global command palette is table stakes for a developer console. Triggered
by `⌘K` (macOS) / `Ctrl-K` (Windows/Linux), the palette must:

- Open from anywhere — sidebar, table, drawer, modal, runner — without
  losing the underlying view's scroll or focus state.
- Provide three search modes in the same surface:
  - **Navigate**: jump to a resource by name, ID, or path (machines,
    tenants, tables, functions, runs, services, ports).
  - **Run**: invoke an action (Start machine X, Rotate token, Shutdown,
    Create tenant, Open function runner).
  - **Filter**: when triggered from a list, filter the current view.
- Show keyboard hints next to every action (`⏎ Run`, `⌘⏎ Open in new
  tab`, `⌘C Copy ID`).
- Surface recent commands at the top and persist across reloads.

Implementation: `cmdk` library, mounted at the app root.

### Bottom Status Bar

A persistent thin bar (24-28px) anchored to the bottom of the viewport.
Frees the sidebar from meta-state and gives the operator one always-visible
read of system identity. Reference: VS Code, Podman Desktop.

Required slots, left to right:

- Connection state dot + label (`Connected`, `Reconnecting`, `Offline`).
- Active server URL (monospace, truncated, click-to-copy).
- Server version + build hash (monospace, click opens release notes).
- Active tenant (monospace, click opens tenant switcher).
- Inflight request count (when > 0).
- Right side: keyboard hints (`⌘K palette` `⌘\\ system tenant lens`).

The bar never wraps. Truncate aggressively; rely on title attributes for
full values.

### Resource Breadcrumb

For nested resources, render a path-style breadcrumb where every segment is
both navigable and copyable. Reference: Firebase Firestore browser.

`_nimbus / tables / machines / m_abc123`

Rules:

- Use the chevron glyph `›` as separator, not `/` (slashes collide with
  function paths and URLs).
- Segments use monospace when they represent IDs or technical paths.
- Each segment has a hover affordance to copy that segment value.
- The trailing segment is the current resource and does not link.

### Copy-to-Clipboard Chip

Every machine-readable value — IDs, digests, request IDs, function paths,
ports, server URLs, tokens — must be paired with a copy affordance. Default
pattern: an inline icon button (12px) that appears on row/value hover and
gives a transient toast on success (`Copied m_abc123`).

For values that are *always* the canonical identifier (the resource ID in
the resource header), the chip is permanent rather than hover-only.

### Toast / Notification Queue

Use `sonner` for transient feedback. Anchor: bottom-right (above the status
bar). Rules:

- Mutations confirm via toast (`Started machine-01`), not via modal.
- Errors show until dismissed; never auto-disappear.
- Toasts include a correlation ID and an action button when a follow-up is
  meaningful (`View run`, `Retry`, `Undo`).
- Never stack more than three; collapse the rest into "+N more."
- Toasts do not block keyboard input or steal focus.

### Empty States

Three sizes, each with a clear next action:

| Scope | Format | When |
| --- | --- | --- |
| Row | One-line muted text in the row | Empty cell, no value yet (`—` is acceptable too) |
| Panel | Compact panel: 2-line message + primary action button | Filter result empty, sub-section has no rows |
| Whole-tab | Centered, monospace title + 2-line muted body + 1-2 primary actions | Brand-new install, no machines created, no tenant exists |

No illustrative artwork, no marketing-style blocks. Empty states are
operational onboarding hints, not decoration.

### Code Block

Inline `code` uses monospace + subtle surface-2 background + 1px border.

Multi-line code blocks use a 12px monospace, 1.5 line height,
surface-2 background, hairline border, and 12px padding. A header strip
shows the language label (lowercase, monospace, 11px) on the left and a
copy button on the right. Syntax highlighting via `shiki` with a dark
theme that uses the same hue palette as the rest of the UI — no rainbow
defaults.

### Diff Viewer

Used for schema migrations, configuration changes, deploy artifact
comparison, and document edits before save. Pattern:

- Side-by-side on desktop, unified on tablet/mobile.
- Line-level diff with intra-line highlights.
- Removed: `--danger` left border + `--danger`-tinted surface.
- Added: `--success` left border + `--success`-tinted surface.
- No saturated reds/greens — use the same OKLCH state tokens.
- Diffs over 200 lines collapse unchanged regions to `… N unchanged lines`.

### Keyboard Hints

Render keyboard shortcuts as monospace chips with a 1px border and a
half-step smaller font than the surrounding text. Glyph conventions:
`⌘` for meta on macOS, `Ctrl` elsewhere; `⇧` shift; `⌥` option; `⏎`
enter; `␣` space; `↑↓←→` arrows; `⌫` backspace; `⎋` escape.

Display next to action buttons in menus, drawers, and the command palette.
Do not show shortcuts in inline UI noise (table rows, sidebar links) —
reserve for surfaces where the operator is consciously taking action.

### System Tenant Lens (⌘\\)

A signature affordance unique to Nimbus. Triggered by `⌘\\`, the lens
flips any resource view into its raw `_nimbus` system-tenant document
representation, side-by-side with the operator view. Operators see the
engine's actual state, not an abstraction.

- Available on every resource list and detail view.
- Renders the underlying `_nimbus` document(s) as syntax-highlighted JSON.
- The same `⌘\\` toggles the lens off and restores focus to the row that
  was active before opening.
- Read-only — the lens never mutates. To edit, the operator must go through
  the normal action surface.
- When `_nimbus` does not have a document for the current resource (a
  cross-tenant user table, an unmanaged external service), the lens shows
  an honest "Not in `_nimbus`" empty state with a link to the underlying
  REST endpoint that owns the data.

This is the affordance that distinguishes Nimbus from other consoles. No
other operator console has a system tenant to expose. Treat it as a
first-class navigation primitive, not a debug toggle.

## Interaction Patterns

These rules apply across every screen in the console:

- **URL is state.** Every filter, sort, selected resource, drawer, and tab
  position must be reflected in the URL so the view is deep-linkable and
  shareable. Hard rule: refreshing the page returns the operator to the
  same view.
- **Right-click is a peer of click.** Every resource row exposes a
  context menu with the same actions available in the row's inline action
  set (start/stop/copy ID/open in new tab/view raw).
- **Bulk action toolbar.** When multi-select is engaged, an inline toolbar
  appears above the table with the active selection count and bulk
  actions. ESC clears selection.
- **Optimistic UI is the default for lifecycle.** Start/stop/restart actions
  reflect intent immediately with a `Starting`/`Stopping` state. On error,
  revert with an inline error envelope on the affected row, not a modal.
- **Undo for soft-destructive.** Document deletes, schedule cancellations,
  and tenant resource cleanups offer a 5-second toast-anchored undo before
  finalizing.
- **Live updates preserve scroll.** Tables, logs, and run lists never jump
  the operator's scroll position when new rows arrive. Follow-mode is opt-in
  (logs default to follow; tables default to anchored).
- **Column resize, visibility, and order persist** per resource type, per
  user, in `localStorage`.
- **Focus restoration on close.** Closing a drawer, modal, or palette
  returns focus to the element that opened it.

## Adapter Capability UX

### Convex

Use the Convex plugin and local Convex guidelines for system-tenant functions:

- Every public function has validators.
- Use generated `api` refs.
- Use indexed queries and bounded pagination.
- Do not use ad hoc filter scans for UI list views.
- Separate high-churn operational data from stable resource metadata.
- Do not accept user IDs or tenant IDs for auth decisions unless the server
  verifies them.

Convex-inspired UI capabilities to match for Nimbus where implemented:

- Health cards: failure rate, cache hit rate, scheduler status, last deployed.
- Data page: tables/documents, filters, create/edit/delete, custom query lane
  only when safely supported.
- Function page: deployed function list, function runner, paginated query
  support, identity simulation where appropriate, metrics.
- Schedules page: scheduled functions, cron jobs, cancel, execution history.
- Logs page: realtime activity, request ID correlation, filters by function,
  status, severity, and text.
- Settings page: URL/endpoints, environment/config, auth posture, backup or
  export surfaces when implemented.

### MongoDB

MongoDB UI expectations:

- Database names map to Nimbus tenants. Make that mapping visible.
- Collections map to Nimbus tables where the adapter routes them.
- Show `directConnection=true` in generated driver URIs.
- Surface supported operations from the adapter docs: CRUD, cursors,
  aggregation pipeline subset, indexes, sessions/transactions, change streams,
  admin commands, SCRAM-SHA-256 auth.
- Index UI should show fields, type, properties, status, and write-cost
  warnings. Do not claim Atlas Search, Vector Search, sharding, or Atlas
  Performance Advisor unless Nimbus implements the underlying feature.

### Firebase

Firebase UI expectations:

- Project/default database mapping must be explicit.
- Firestore browser uses collection/document language and path navigation.
- Query builder follows the implemented Firestore subset and explains missing
  index or unsupported query shape errors.
- WebSocket Listen status is visible under Network and Firebase adapter detail.
- Cloud Functions views show target bindings, route type, runtime, logs, and
  deployment artifact source where supported.
- Do not imply full Firebase Emulator Suite control-plane parity, offline
  persistence, bundles, or stock browser SDK drop-in until implemented.

### Native HTTP/WS

Native UI expectations:

- Show exact REST endpoint paths and request examples.
- Surface `nimbus.v2` WebSocket subscriptions and connection state.
- Schema/index controls reflect the native API directly.
- Scheduling and cron views should be first-class, not hidden under Convex.

## Settings And Deploys

Settings owns server administration and deployment management:

- Server info: version, uptime, listen address, storage backend, and active
  local server origin.
- Configuration display: runtime limits, license status and usage, auth
  provider config, adapter enablement, and storage topology. Configuration is
  read-only in Phase 1 unless a dedicated write API exists.
- Deploys: current active bundle with sha256/source/timestamp, function
  inventory, deploy history, and deploy trigger when the local-admin deploy
  endpoint can accept the selected artifact.
- Token and session: current session state, token rotation with confirmation,
  and forced re-auth after rotation.
- Shutdown: graceful shutdown with confirmation and clear disconnect state.

## Copy And Terminology

Preferred nouns:

- `Tenant`
- `Adapter`
- `Function`
- `Run`
- `Schedule`
- `Cron`
- `Table`
- `Collection`
- `Document`
- `Service`
- `Machine`
- `Endpoint`
- `Port`
- `Listener`
- `Session`

Avoid:

- `Project` as a global Nimbus noun unless mirroring Firebase/Convex copy.
- `Database` as the global storage noun. Use it only for MongoDB/Firebase
  adapter context.
- `VM` when the UI specifically means a managed service.
- `microVM` on macOS service screens, since macOS services run inside the
  machine guest.

Tone:

- Short, direct, operational.
- Prefer "Start machine", "Rotate token", "Create index", "View logs".
- Avoid explaining basic UI affordances in visible copy.

## Security And Trust UX

- Show the active local server origin and session status.
- Token rotation and shutdown require confirmation.
- Destructive actions name the exact resource that will be changed.
- Privileged actions show the actor/session and audit event after completion.
- Adapter capability caveats must be visible before a user depends on them.
- The UI must never bypass `Service` or write storage directly.
- During disconnect, show stale data with a stale marker and disable mutations.
- Never silently queue lifecycle or data mutations while disconnected.

## Implementation Rules

- UI code lives in `packages/nimbus-ui` when implemented.
- The embedded SPA is the primary UI. The Electron desktop shell (Phase 2)
  loads the same `/ui/*` bundle.
- Business logic stays in `nimbus-server`, the system tenant, and existing
  HTTP lifecycle endpoints.
- Reactive reads use the `_nimbus` system tenant Convex function surface.
- Lifecycle writes use HTTP endpoints when host orchestration is required.
- Cross-tenant user data browsing uses the REST API unless a safe generated
  function surface exists for that exact tenant.
- Do not introduce a second data orchestration path for the UI.
- Prefer shadcn/ui source components, Base UI (MUI) primitives, Tailwind v4
  with `@theme` OKLCH tokens, `cmdk` for the command palette, `sonner` for
  toasts, `shiki` for syntax highlighting, JetBrains Mono for monospace
  (via `@fontsource/jetbrains-mono`), Lucide for icons, TanStack Router,
  Zustand, Vitest, React Testing Library, and Playwright as described in
  `docs/plans/archive/desktop-ui-plan.md`.

## Accessibility And Quality Gates

Every UI feature must satisfy:

- Keyboard reachable controls and menus.
- Visible focus states.
- No critical or serious axe violations.
- Dark mode and light mode contrast checks.
- Reduced-motion support for transitions.
- Text fits in buttons, badges, cards, and sidebars across mobile and desktop.
- Tables remain usable at 1000+ rows through pagination or virtualization.
- Logs remain responsive at 100+ events/second.
- Bundle stays under the plan's gzipped size budget.

## References Used

- `docs/current-capabilities.md`
- `docs/plans/archive/desktop-ui-plan.md`
- `docs/plans/archive/system-tenant-api-plan.md`
- `docs/adapters/convex/compatibility.md`
- `docs/adapters/firebase/compatibility.md`
- `docs/adapters/mongodb/README.md`
- `docs/adapters/mongodb/operations.md`
- `docs/adapters/native/README.md`
- `docs/architecture/sandbox/microvm-service-baseline.md`
- VoltAgent `awesome-design-md` as the plain-text design-system pattern
- Convex dashboard docs for Health, Data, Functions, Schedules, Logs, Settings
- MongoDB Atlas docs for Data Explorer and Indexes
- Firebase docs for Firestore console and Cloud Functions logging
