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
| **Operator** | DevOps / admin / host running Nimbus for others (or themselves) | "Is the node healthy? Which tenants exist? Are machines up? Are listeners reachable?" | server-wide state, infrastructure, multi-tenant administration |

The console renders these as **two views** with a **view switcher** in the
top horizontal nav. URL prefix is the source of truth:

- **Developer console** — `/developer/*` — always tenant-scoped. Tenant selector
  always visible, always active.
- **Operator console** — `/operator/*` — server-wide. Tenant selector hidden
  by default; rendered as an optional cross-tenant filter on `/operator/
  observability` only (URL `?tenant=<id>`).

Switching the view persists `nimbus-ui:last-view` and the per-view
`nimbus-ui:last-route:developer` / `nimbus-ui:last-route:operator` so the
second toggle restores the previous route in each view. Cold load lands on
`/developer` (Developer default) unless localStorage records a different last
view.

### Developer console — sidebar IA (`/developer/*`)

| Section | Purpose | First required views |
| --- | --- | --- |
| Overview | This app's health and recent activity | Recent runs, error rate, last deploy, schedule status, latest events |
| Compute | Request-scoped execution | Functions list, function detail, function runner, runs |
| Deploys | Bundle activations on this server | History newest first with each bundle's SHA-256, kind, generation, actor and function count; function-path diff of any row against the active bundle; rollback to a retained bundle behind a confirmation |
| Services | Long-running placement (this tenant's view) | Compose-declared services in the active tenant, lifecycle state, endpoints, restart policy |
| Sandboxes | Isolated workloads (microVMs and containers) | Live sandboxes in the active tenant, lifecycle state, endpoints, conditions, a console on each, create and stop |
| Schedules | Periodic and future-dated work | Scheduled jobs (next/last run, cancel/retry), cron jobs |
| Storage | Schema-aware data | Tables, document browser, schema tab, indexes tab, query builder |
| Files | Opaque bytes / blob storage | Buckets, object browser, upload, preview, download over the console session |
| Observability | Debugging and audit (this tenant) | Logs, events, traces, error groups |
| Settings (tenant) | Tenant-owned configuration | Environment, secrets, schema, integrations, adapter binding (all planned; the page is one empty state until the first tenant-scoped setting has an API) |

10 sections. Every section but Deploys is tenant-scoped — the active tenant
comes from the sidebar tenant selector, not the URL. Deploys is server-wide:
a bundle activation is one event for the whole server, whichever silo ran
it, and the page reads the local-admin deploy routes. Services is **dual-persona** (it also
appears in the Operator IA below); both consoles back onto the same
`ServicesTable` and `ServiceDoc` shape, with the Developer side filtered
to the active tenant. See
`docs/private/plans/archive/desktop-ui-compute-services-redesign-plan.md` for the
IA decision rationale.

### Operator console — sidebar IA (`/operator/*`)

| Section | Purpose | First required views |
| --- | --- | --- |
| Nodes | Hosts running the Nimbus binary | Node list + detail: identity, health, role, version, uptime, listen address, and the tenants/machines/services/listeners hosted on it. One node today (the local host); shaped to scale to a cluster. |
| Tenants | Tenant lifecycle | List with backend/quota/table-count, create, archive, per-tenant adapter binding |
| Machines | Outer dev-VM lifecycle (macOS/Windows) | Machine list, detail (boot image, upgrade state, services placed on it), start/stop/restart/SSH/OS apply/remove. A machine is a guest VM that hosts sandboxes — **not** a cluster node. Absent on pure-Linux nodes. |
| Network | Reachability | HTTP routes, WebSocket subscriptions, published ports, machine API forwarding, listener status, origin allowlist |
| Services | Long-running placement (cross-tenant) | Compose-declared services across every tenant, service catalog, lifecycle state, endpoints, restart policy. **Dual-persona** with the Developer IA above; both sides share `ServicesTable`/`ServiceDoc` with a `showTenantColumn` toggle |
| Observability | Cross-tenant debugging and audit | Logs, runs, and later events, traces, error groups — default cross-tenant; the tenant facet narrows through `?tenant=<id>` |
| Settings (server) | Server administration | General, system, integrations (adapter capability matrices), shutdown; endpoints, token/session, and environment are planned |

7 sections. Server-wide by default. Tenant selector appears only on
`/operator/observability`.

### Secondary navigation rules

- Within each view, every primary section can opt into a **sub-panel**
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
  the same data through `/operator/tenants/_nimbus` instead.

## Core Screens

Each screen is owned by exactly one view. The screen entries below are
grouped by view. The Developer-side `Overview`, `Compute`, `Storage`, and
`Observability` screens were already specified in earlier revisions and
remain authoritative for the Developer side.

## Core Screens — Developer console

### Overview (Developer)

The Overview answers one question first, then the three things a developer
comes here for. Top to bottom:

- Headline: the mascot in the state the server is in (idle, working, error,
  empty) and one sentence that says why, for example "The server is up. 2
  runs failed in the last 24 hours." Under it, one mono fact line: tenant,
  server URL, version. A fact the server did not report is left out.
- Connect: the server URL and one snippet per client, as tabs: curl against
  the native HTTP API, the TypeScript SDK, the Convex client. Each snippet
  is addressed to the active tenant and names a function and a table the
  server actually has, so it runs as pasted.
- Stats: one row of at most four tiles — functions, tables, runs in the last
  24 hours, errors in the last 24 hours — each a link to the page that owns
  it. Runs and errors carry an hourly sparkline from the chart seam. A tile
  with nothing behind it is not shown.
- Recent runs: the five newest runs as a table, with a link to the Runs tab
  of Observability.

A server with no functions, no tables in the active tenant, and no runs shows
the first-run panel in place of the stats and the runs table: the empty
mascot, three steps (install the CLI, run `nimbus dev` in an app, call a
function) with the command for each, and live completion from the same
queries that fill the stats. The connect panel stays.

No greeting, no marketing copy, and no tile for a number the server has not
reported.

### Compute (Developer)

Compute owns request-scoped function execution for the active tenant.
Service lifecycle lives in `Services` — a dual-persona surface present
in both consoles. Compute and Services are siblings, not parent/child.

- The sub-panel is the **function tree**: bundle → module → function, built
  from the deployed functions, with a filter box at the top. The same tree
  backs the Compute page and every function page, so the list an operator is
  scanning keeps its shape when they open an item from it.
- The Compute page has three **page tabs**: Functions (a DataTable of path,
  kind, adapter, last status, last run), Sandboxes (live runtime state, no
  placeholder data), and Graph (the call graph of the deployed bundle). Search
  and filters stay in the sub-panel; the table has no second toolbar.
- A function page has four tabs: **Overview** (kind, adapter, bundle, last
  status, last run, and the argument list read from the validator), **Source**,
  **Runs**, and **Graph** (the call graph focused on this function). Logs are
  not a function tab; Observability owns logs and the run page links there.
- Source tab: the deployed module source, served from the **content-addressed
  source-package store** (`GET /api/console/source`), hash-verified, syntax-
  highlighted, with the source-package digest shown as provenance. Source is the
  read-artifact (original TS), distinct from the runtime bundle; it is captured
  at `nimbus deploy` and deduplicated by content digest. A navigable
  **DEFINES / CALLS / CALLED BY** symbols strip (oxc structural index — exports
  + `api.*` / `internal.*` references) links across functions. When no source
  was captured the tab is an empty state that shows the exact
  `nimbus dev --app-dir <dir>` command with a copy control.
- Function runner: the bottom drawer of every function page. See
  [Function Runner](#function-runner) for the contract.
- Runs: a DataTable of status, run id, duration, and start time. A row opens
  the run page, whose breadcrumb is Compute › Runs › id and whose kind is a
  category pill and whose timings are monospaced. Filtered to the active
  tenant.
- Sandboxes: live runtime state (not a deployment record); the view reads from
  the sandbox runtime (wiring in progress) — no placeholder data.

Deploys are gated by a client-side TypeScript typecheck (`--typecheck
enable|try|disable`, mirroring `convex deploy`): codegen → bundle → typecheck,
aborting on type errors. Tenant is implicit from the sidebar tenant selector — the
runner does not show a tenant chooser.

Convex-like function runner behavior is useful, but it must be Nimbus-aware:
show which adapter handles the function and which execution mode is in
play (query / mutation / action / HTTP route / scheduled job).

### Services (Developer)

Services owns long-running placement for the active tenant. Same surface
as the Operator-side `/operator/services` (see §Services below); the
Developer side renders `ServicesTable` for the active tenant and hides
the cross-tenant column.

- Service list on `DataTable`: name, lifecycle state as a `StatePill`,
  kind, placement (machine), endpoint count, updated. Scoped to the
  active tenant. Row activation opens the detail; right-click or the
  trailing actions button opens a `RowContextMenu` with `Open service`,
  the lifecycle actions the state allows, and `Show logs` (the
  Observability Logs tab narrowed to `source=service`).
- Lifecycle actions: `start`, `stop`, `restart`. A live service offers
  stop and restart, a stopped one offers start, a failed one offers
  start and restart, and a service in flight offers none. The row shows
  the optimistic state (`starting`, `stopping`, `restarting`) while the
  request is out, returns to the real state when it settles, and keeps
  the real state plus the refusal text under the pill when the route
  says no. `restart` sends the row's `sourceGeneration` and a fresh
  request id.
- Service detail: header with kind, state pill, bundle chip, and the
  same lifecycle buttons; tabs `Overview` (stats, endpoints, health),
  `Logs` (the live `source=service` event stream for the tenant, with a
  link to Observability), `Config` (bundle and endpoints). Cross-links
  to **Operator → Machines** for the underlying machine record.

The Services sub-panel (Developer) is a **dynamic list** of services
declared in the active tenant's `compose.yaml`, each with a `StateDot`.
The Developer side never lists system services from `_nimbus`; the
System Tenant Lens (⌘\\) is the only Developer-side path to system
service state.

### Sandboxes (Developer)

Sandboxes owns the isolated workloads of the active tenant: microVMs
(`krun`) and containers, each with a lifecycle, endpoints, conditions,
and a console. The pages read the service-control routes
(`/api/tenants/{tenant}/sandboxes`) and the session routes
(`/api/sessions`); a server that runs no service manager answers 404 on
all of them, and the list shows one plain "Sandbox routes not available
on this server" state with the server's message instead of an empty
list.

- Sandbox list on `DataTable`: name (the owner's display name, else the
  id), lifecycle state as a `StatePill`, profile and backend as
  `CategoryPill`s, health, endpoint count, updated. Scoped to the
  active tenant. Row activation opens the detail; the row menu offers
  `Open sandbox`, `Open console`, and `Stop sandbox` on a sandbox that
  can still stop. Stop sits behind a `ConfirmDialog`, shows `stopping`
  on the row while the request is out, and keeps the real state plus
  the refusal text under the pill when the route says no. The list
  polls every 2 s while any row is transitional (`pending`, `starting`,
  `stopping`) and stops when every row settles; a poll keeps the last
  rows on screen, it never blinks back to the skeleton.
- `New sandbox` opens a picker: id, display name, profile (`worker` or
  `desktop`), backend (`krun` or `container`), OCI image reference, and
  the command one argument per line. An id that does not match
  `[a-z0-9][a-z0-9-]*` or a missing image never reaches the server. A
  created sandbox opens its detail.
- Sandbox detail: breadcrumb `Sandboxes › name`, profile and backend
  pills, the state pill, the id as a copy chip, and `Stop` behind the
  same confirmation. Tabs `Overview` (facts, endpoints as `host:port`,
  and the conditions with their reason, message and transition time),
  `Console`, and `Spec` (owner, root image, and the process with argv
  and environment shown as `N values, redacted`; the server never sends
  the values).
- Console: the panel opens a `stdio` session on the sandbox
  (`POST /api/sessions`, `channels: ["stdio"]`, 30 min TTL) as soon as
  the sandbox is `ready`, reads the channel stream
  (`application/x-ndjson`) and appends every frame in arrival order:
  `opened`, `stdout`, `stderr`, `exit`, `closed`, each on its own line
  with its kind. A sandbox that is not ready waits for an explicit
  `Connect`. The input line posts to the channel with a trailing
  newline and echoes locally; a refused write is a line in the log. The
  panel closes its session when it unmounts or when the operator
  disconnects, with the reason in the close body. No sandbox system
  events are recorded, so the console is the live log and the
  conditions on Overview are the lifecycle history.

The Sandboxes sub-panel is a **dynamic list** of the tenant's sandboxes,
each with a `StateDot`. Compute's Sandboxes view links here.

### Schedules (Developer)

Schedules owns periodic and future-dated work for the active tenant.

- Scheduled jobs on `DataTable`: function path, status pill, scheduled
  time, finished time, outcome (with the error text when the job
  failed). Row activation opens the job sheet through `?job=`; the row
  menu offers `Open job`, `Run now`, and `Cancel job` on a pending job.
- Cron jobs on `DataTable`: name, function path, schedule as an
  operator would say it (`every 30s`), status pill, next run, last run.
  Row activation opens the cron sheet through `?cron=`; the row menu
  offers `Open cron`, `Run now`, and `Delete cron` behind a
  `ConfirmDialog`.
- Schedule sheet: facts, the recorded mutation, the error when there
  is one, and the footer actions. `Run now` re-enqueues the job's
  mutation as a new one-shot job with no delay; for a cron it reads the
  cron's mutation from the crons route first, because the system record
  does not hold it. The scheduler writes a job's outcome into its
  system record when the job history route is read, so a row can sit on
  `pending` after it ran; recording outcomes as they happen belongs to
  the scheduler, not the console.

The Schedules sub-panel is a **static menu** with two items
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
- Schema tab for optional Nimbus schemas and adapter-derived schema views,
  including create, edit, delete, and validation error display. The tab is
  a full-width view of the table page, addressed by `?tab=schema`, not a
  side inspector beside the grid. Apply goes through the checked route
  (`POST /schema/{table}/apply`): the server scans every document first and
  refuses the whole draft with the violating ids when one breaks it, so a
  schema never lands on a table that already violates it. Check runs the
  same scan as a dry run.
- Indexes tab with name, fields, and a polled status pill. Create and drop
  re-apply the table schema with the index list edited, so they get the same
  document check. Storage builds an index inside the schema commit, so a new
  index reads `enabled` as soon as apply returns. Addressed by
  `?tab=indexes`.
- Query builder (`?tab=query`) that marks each field as indexed or scan,
  refuses an unindexed sort until the operator says "scan anyway", and shows
  the request it compiles to as code. The builder, the grid, and the pager
  compile through one function, so the code shown is the request sent.

The Storage sub-panel is a **dynamic list** of tables for the active
tenant. URL is store-driven (`/developer/storage/<table>`), not
`/developer/storage/<tenant>/<table>` — the tenant lives in the sidebar.

The Storage UI should feel familiar to Convex Data, MongoDB Atlas Data
Explorer, and Firebase Firestore Data, but the implementation should be one
Nimbus document browser with adapter-specific labels and type renderers.

### Files (Developer)

Files owns opaque-byte / S3-compatible object storage for the active
tenant. It reads and writes through the console's own object route
family on `/api/tenants/{tenant}/objects`, which rides the console
session like every other console write; the S3 listener is a separate
front door with its own credentials, which the console never holds.

- The sub-panel is a **dynamic list** of buckets with object count and
  total bytes. A bucket exists once it holds an object; **New bucket**
  names one and opens its (empty) listing so the first upload lands
  there.
- The page lists one prefix level on the shared `DataTable`: Name,
  Size, Type, Modified, and a row menu. Keys that continue past the next
  slash fold into a folder row. The bucket, the prefix, and the open
  object all live in the address (`?bucket=&prefix=&object=`). A
  breadcrumb above the table walks the prefix.
- Upload is a drop anywhere on the listing or the Upload chooser. Each
  file becomes one whole object keyed by its name under the current
  prefix; an upload strip shows progress, the result, or the server's
  reason for a refusal. The route takes whole objects up to 16 MiB and
  never creates a tenant.
- The object sheet holds the facts (size, type, modified, etag), a
  preview for images and text under 256 KiB, Download, Copy link, and
  Delete. The link is the console's object route, so it answers for a
  signed-in console session only. There are no presigned URLs.
- Downloads and previews come back with `content-security-policy:
  sandbox` and `nosniff`, so a stored page cannot script the console.
- The listing is read on demand (there is no live query over objects)
  and again after every write the console makes. A listing carries up
  to 1000 keys under the prefix and says when it stopped short.

### Observability (Developer)

Observability is the Developer-side debugging surface. Defaults to the
active tenant; never cross-tenant in this view. (The Operator console
owns the cross-tenant feed under `/operator/observability`.)

- Logs: one stream of runs and the log lines that belong to them. Each
  run is a group with its status, function path, kind, duration, and line
  count; a line that names no run sits in the `server` group. The facet
  bar holds tenant, level, category, source, and correlation. `Follow`
  keeps the newest line in view; `Pause on error` freezes the stream at
  the first line at `error` level or above.
- Runs: recent function runs on `DataTable` with status pills. A row
  opens a right-side sheet that shows the run summary, the error, and
  the correlated lines; `Show in logs` narrows the Logs tab to that run
  and `Open run` goes to the full run page.
- Traces: the run list beside one run's waterfall. A run's spans are the
  function's own span and one span per host call under it (`db`,
  `scheduler`, `storage`, `function` for a nested call), drawn on one
  time axis with the error glyph on any span that failed; `?run=` names
  the trace on show. The same waterfall sits in the run sheet and on the
  run page, so the three surfaces agree on what a run did.
- Errors: failed runs folded by fingerprint (function, error class,
  normalized message) with run count, first and last seen, and location.
  A row drills into Runs narrowed to that group, where a chip names the
  group and clears it.

Observability has no sub-panel. Its views are page-header tabs
(`Logs` / `Runs` / `Traces` / `Errors`, driven by `?tab=`), and a tab
appears only once its page exists: the strip never names a view the
operator cannot open.

Every filter lives in the address (`?tenant=`, `?level=`, `?category=`,
`?source=`, `?correlationId=`, `?status=`, `?functionPath=`, `?run=`,
`?fingerprint=`), so
a view is a link. Run and event rows name their tenant, so the tenant
facet is an index read on both tables; a line the server writes outside
any tenant shows only under the operator page's `all tenants` scope.
`?q=` is the log search: the field commits 300 ms after the last
keystroke, the tab reads `GET /api/console/logs` with the same facets the
stream reads, and a count line says how many lines matched and how far
the server looked (every line, or the newest 2,000). The live stream
resumes when the search clears.

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

The Settings (tenant) sub-panel is a **static menu** of sub-pages
(`Environment`, `Secrets`, `Schema`, `Integrations`, `Adapter binding`).
None of the five is built: no tenant-scoped setting has a write API in
this build. A static menu lists only pages that exist, so the page
contributes no sub-panel and shows one empty state that names the five
planned sub-pages and links to the operator settings. The menu and the
`?section=` search arrive with the first built sub-page.

## Core Screens — Operator console

### Nodes (Operator)

The Nodes screen is the Operator landing page. A **node** is a host
running the Nimbus binary (the `nimbus node` lifecycle). Multi-node
clustering is not wired yet, so this deployment is exactly one node — the
local host, sourced from system status. The screen is shaped as a node
list so it scales to a real cluster without a redesign. A node is
distinct from a **machine** (the outer dev VM under Machines).

The page reads top to bottom the way the Developer overview does:

- Headline: the mascot beside one sentence that says what the node is
  doing. Priority order: connection dropped, status read failed,
  inventory read failed, reading, unhealthy, failing services, no
  services placed, every service running, or "R of N services running".
  The face state follows the sentence (error, working, empty, idle).
- Facts line under the headline: listen address, version, uptime, and
  data directory as copy chips separated by `·`. Identity lives here
  once; the node card does not repeat it.
- Node card: name (`local node`), role subline ("standalone ·
  clustering is not active"), a health pill, and three cells for
  started, last update, and the build hash. Loading, offline, and error
  render inside the cells through `LoadingCell`.
- Hosted on this node: four tiles that link to Tenants, Machines,
  Services, and Network. Each tile shows a large count and a subline
  that says what the count is made of: machines and services carry a
  state summary ("2 running · 1 stopped"), listeners carry the adapter
  list ("http, ws"). A count never stands alone.
- Recent events: the five newest events as a `DataTable` (level dot and
  word, source, message, relative time) with a "View all logs" link.
  Activating a row opens Observability on the Logs tab narrowed to the
  event's correlation id when it has one.

No sub-panel. When clustering lands the node card becomes one row per
node with a per-node detail page (Raft role, peer reachability,
placement). Upgrade state and recent admin actions are not shown yet;
Settings owns upgrades.

### Tenants (Operator)

Tenants owns the tenant lifecycle (the Developer console can't create
tenants — that's an admin concern).

- Tenant list: a `DataTable` with id (as a copy chip), table count, and
  a row menu. Clicking a row opens the tenant in the Developer storage
  view (`/developer/storage?as=<id>`). Right-click or the `⋯` button
  opens the row menu: open, copy id, delete.
- Create tenant: `Create` is the one primary action in the header. It
  opens a dialog with a single id input; the server owns the id rule
  and a refusal (for example "tenant already exists") stays in the
  dialog beside the input. While the request is in flight the submit
  reads "Creating…". Success closes the dialog, toasts "Created tenant
  <id>", and the row appears through a loader invalidation. The
  Developer console hands off with `?create=1`, which opens the dialog
  on arrival and clears itself on close.
- Delete tenant: lives in the row menu only, never inline, and runs
  through `ConfirmDialog`. The description says how many tables go with
  the tenant; when the count is above zero the operator types the
  tenant id before `Delete` enables. A server refusal stays in the
  dialog.
- Empty state on fresh install: "Create your first tenant" CTA that
  opens the same dialog; matches the inline Developer-side fallback.
- Backend selector, adapter binding, quotas, and last write are not
  wired; the server exposes ids and table counts only.

The Tenants sub-panel is a **dynamic list** of tenants. Selecting a
tenant opens it in the Developer storage view.

### Machines (Operator)

Machines owns the **outer dev-VM** lifecycle on macOS and Windows
(krunkit / WSL2). A machine is a single long-lived guest Linux VM that
supplies the kernel sandboxes need; it is **not** a cluster node (see
Nodes), and pure-Linux nodes have none. Do not frame this screen as
"host" lifecycle — the host is the node.

- Header summary: the trailing slot says what the fleet is, not a bare
  total: "3 machines · 2 running · 1 stopped" on the first line and the
  summed allocation ("6 vCPU · 12 GiB memory · 120 GiB disk") on the
  second. Zero capacity leaves the second line out; an in-flight read
  says "loading…".
- Machine list: name, state as a `StateDot` with the state word beside
  it (an in-flight action shows its optimistic state and the row error
  under it), provider, kind, CPU, memory, disk, updated, and a pinned
  actions column. The table keeps its nine columns through the skeleton
  so the swap from loading to loaded moves nothing.
- Machine detail: the inspector beside the table opens from a row or
  from the sub-panel; it shows the identity, resources, and metadata
  the server records. Boot image, desired versus actual image, guest
  Nimbus hash, forwarded API, and placed services arrive with the
  provider work that records them.
- Actions: start, stop, restart, delete (behind `ConfirmDialog`). SSH,
  OS apply, and OS upgrade are not wired.
- macOS copy must be clear that services run as containers inside the
  Linux guest and that machine actions converge the guest VM's state. Do
  not imply per-service nested microVMs on macOS.

The Machines sub-panel is a **dynamic list** of machines. Each item is a
button with a `StateDot`, the name, and the state word; it selects the
machine in place (the inspector opens beside the table) and carries
`aria-current` while selected, so the panel and the table agree on
which machine is open.

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

Every count on Network is labelled with what it counts (listeners by
adapter, active subscriptions, published ports); a number never stands
alone next to a heading.

The Network sub-panel is a **static menu** (`Routes` / `WS` / `Ports`
/ `Listeners` / `Security`).

### Services (Operator)

The Operator-side cross-tenant view of the same Services surface
described under **Services (Developer)** above. The same
`ServicesTable` component backs both routes; only the query and the
visible columns differ.

- Service list: cross-tenant, with a `Tenant` column not present on
  the Developer side, the same row menu and optimistic lifecycle
  states. Includes system services (`_nimbus`-owned) that the Developer
  console hides.
- Service detail: the same header, state pill, and lifecycle buttons;
  tabs `Placement` (machine, attachment, provider) and `Logs` (the same
  live service stream). The difference is permissions: Operator actions
  are not tenant-gated.
- Actions: start, stop, restart. Operator can act on any tenant's
  services and on system services.

A service has both a service identity (here) and a machine placement
(under **Operator → Machines**). Cross-link both ways; do not duplicate
the full detail page on the machine side.

The Services sub-panel (Operator) is a **dynamic list** of services,
grouped by tenant. State chips render per service. The Developer and
Operator sub-panels share the same item template — only the grouping
and the tenant scope of the query differ.

### Observability (Operator)

The Operator-side cross-tenant feed of the same data store. Defaults to
all tenants. The tenant facet in the bar adds an `all tenants` option
ahead of the tenant list; picking one narrows the view through
`?tenant=<id>` without leaving the Operator console.

- Logs: the same run-grouped stream and facet bar as Developer, across
  every tenant.
- Runs: the same `DataTable` and run sheet as Developer, across every
  tenant.
- Traces: the same run list and waterfall as Developer, across every
  tenant.
- Errors: the same error-group table as Developer, across every tenant;
  each group names its tenant.

The Operator page uses the same page-header tab strip as the Developer
page (`Logs` / `Runs` / `Traces` / `Errors`, driven by `?tab=`) through
the shared `PageTabs`
component; only the default tenant scope differs.

### Settings (server)

Server administration. Distinct from the Developer-side **Settings
(tenant)**.

- General: appearance (mode only: light, dark, system), license and
  usage, effective configuration.
- System: server identity. Version and update posture, health, uptime,
  listen address, local origin, data directory, storage backend,
  encryption at rest. General is what the operator sets; System is what
  the server reports.
- Endpoints (planned): bind addresses, TLS posture, advertised URLs.
- Token / session (planned): admin token rotation, session policy.
  Rotation itself lives under Shutdown until this sub-page exists.
- Environment (planned): process-level env vars.
- Integrations: adapter capability matrices (Convex / MongoDB / Firebase
  / Cloud Functions / Native HTTP/WS).
- Shutdown: admin-token rotation and graceful shutdown. Both writes go
  through `ConfirmDialog`: rotation asks for the current bearer and
  keeps Confirm inert until one is typed; shutdown asks the operator to
  type `shutdown`, because the write reaches past this browser.

The Settings (server) sub-panel is a **static menu** of the built
sub-pages (`General`, `System`, `Integrations`, `Shutdown`). Deploys is
its own page under the Developer view.
`Endpoints`, `Token`, and `Environment` join the menu when their panes
land; the route rejects their ids until then.

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

The console is a **three-column shell**: the sidebar, an optional
sub-panel, and the page.

```
┌──────────────┬──────────────┬──────────────────────────────────────────┐
│ mascot nimbus│              │                                          │
│ Dev ⇄ Op     │  Sub-panel   │  Main content                            │
│ tenant/server│  (optional,  │  (route Outlet)                          │
│              │  resizable;  │                                          │
│  grouped     │  static menu │                                          │
│  nav rows    │  or dynamic  │                                          │
│              │  list)       │                                          │
│ status · ver │              │                                          │
│ update row   │              │                                          │
│ theme · fold │              │                                          │
└──────────────┴──────────────┴──────────────────────────────────────────┘
```

### Sidebar

The left column is the console's one navigation surface
(`packages/nimbus-ui/src/shell/sidebar/`). Top to bottom:

- **Brand row** (56px): the mascot next to the lowercase wordmark; a link
  to the active view's home page.
- **Scope row:** the **view switcher**, a two-segment `SegmentedControl`
  (Developer, Operator) at full width, then the **tenant selector**
  (Developer) or the **server identity line** (Operator: the hostname,
  copyable, in mono).
- **Nav groups:** the active view's rows under uppercase group labels
  (Build / Run / Observe for Developer; Fleet / Access / Observe for
  Operator). The home row and Settings sit outside the groups. Rows carry
  no count badges.
- **Footer:** the connection dot with its label and the server version,
  the theme toggle labelled with the current theme name (`Light theme` /
  `Dark theme`; it flips between the two, and Settings keeps the
  system option), and the Collapse control.

The view switcher is the source of truth for view, alongside the URL
prefix (`/developer/*` → Developer, `/operator/*` → Operator). Clicking the
inactive segment navigates to the last-visited route in that view (or
the view's default landing) and persists `nimbus-ui:last-view`.

Two widths:

- **Expanded** (`w-60`, 240px): everything above.
- **Rail** (`w-16`, 64px): icons only. Every row keeps its name in an
  `aria-label` and shows it in a tooltip to the right; the view switcher
  becomes two stacked icon radios; the tenant or server becomes one button
  that expands the sidebar.

The Collapse control is the last footer row, carries
`aria-expanded` and `aria-controls="sidebar"`, is keyboard activatable,
and never moves focus on toggle. The choice persists to
`nimbus-ui:sidebar-collapsed` on desktop only; below the desktop tier the
rail is the default and an expand is held in memory for that tier.

### Tenant selector behavior

| View | Selector visible? | Default | Notes |
| --- | --- | --- | --- |
| Developer | always | last-active tenant (or first tenant alphabetically on fresh install) | when zero tenants exist, the trigger is replaced by a compact "Create tenant" CTA that deep-links to `/operator/tenants?new=1` |
| Operator | hidden | n/a | the sidebar shows the server identity line instead; `/operator/observability` narrows through the tenant facet in its own bar (default `all tenants`, encoded as `?tenant=<id>`) |

The selector is one component with two modes: the developer mode sets the
active tenant, the `operator-filter` mode writes `?tenant=`.

### Sub-panel

A second column between the sidebar and the main content. Rendered
when the active route contributes a sub-panel spec; absent otherwise
(the content area reflows naturally). The page never remounts when a
spec arrives or leaves: one resizable group wraps the main column on
every route, and the sub-panel column and its separator are the only
conditional children.

Geometry, on desktop:

- Resizable between 180px and 400px; 240px by default. A 1px separator
  carries the drag handle; it takes the accent colour on hover, focus,
  and drag.
- The separator is a keyboard target: `ArrowLeft` / `ArrowRight` step
  the width, `Home` / `End` jump to the limits, `Enter` collapses and
  expands. A step past the minimum collapses the panel.
- Collapsed, the panel is a 32px rail with the expand button and one
  icon per sub-view. Expanding restores the last chosen width.
- The width and the collapsed state persist per section under
  `nimbus-ui:panel:<section>` (`storage`, `compute`, `tenants`,
  `settings`, ...), so Storage can stay wide while Settings stays
  narrow. Stored widths clamp to the limits on read.

Two contributor modes:

- **Static menu** — a fixed list of sub-pages with an active state.
  Used by Settings (both views), Network, Schedules. Pattern reference:
  Convex `SettingsSidebar`. A static menu lists only pages that exist;
  a page that is not built yet is not a menu item, disabled or
  otherwise.
- **Dynamic list** — a resource list fed by a query. Used by Storage
  tables, Compute functions, Tenants, Machines, Services, Files. Pattern
  reference: Convex `DataSidebar`. The search field appears once the
  list exceeds twenty rows; below that, a list scans faster than it
  filters.

Routes contribute their spec with `useContributeSubPanel`; the layout
resolves it at the root. Routes without a sub-panel reserve no space.

### Main content patterns

- **Overview** (both views): responsive grid of compact status panels +
  a full-width activity table.
- **Resource list**: table on desktop, dense list on mobile, filters
  above.
- **Resource detail**: header summary, tabs, split panes for logs/JSON.
- **Data browser** (Storage): table plus right-side document drawer.
- **Logs / runs** (Observability): run-grouped log stream; runs table
  plus right-side run sheet.

### Page tabs

A page with a short, fixed set of views (Observability: Logs / Runs)
switches between them with a tab strip under its header, driven by the
`?tab=` search param so every view has an address. `PageTabs` is the
shared component. A list the operator can grow is a sub-panel instead.

### Responsive behavior

- **Desktop** (≥1024px): all three columns visible; the sidebar
  toggles between expanded and rail, the sub-panel resizes and
  collapses to its rail.
- **Tablet** (768–1023px): the sidebar is the rail by default; the
  sub-panel is always its 32px rail, and the expand button opens the
  list as an overlay sheet anchored to the right of the rail. The
  desktop width and collapsed state are not written at this tier.
- **Mobile** (<768px): the sub-panel overlay as on tablet.
- **Small screen** (<640px): the sidebar leaves the flow. A 48px top bar
  holds the menu button and the mascot lockup, and the whole sidebar body
  (scope row, nav groups, theme toggle) opens in a left sheet that closes
  on every navigation. The sidebar has one breakpoint; the tiers above
  belong to the sub-panel.

### Do-not list

- Do not put page sections inside decorative cards. Cards are for
  repeated items, small metrics, modals, and genuinely framed tools.
- Do not duplicate primary navigation in the sub-panel. The sub-panel
  is per-section; cross-section navigation always uses the sidebar or
  the command palette.
- Do not mirror Settings between the two views. The split (tenant vs
  server) is deliberate and exclusive.

## Visual Language

Nimbus should feel crisp, technical, and calm.

### Product Palette

This palette governs the operator console (`packages/nimbus-ui/`) and every
native chrome surface in `nimbus/desktop`. For the logo, marketing surfaces,
favicon, app icon, and the desktop setup card, see **Brand Palette** below —
the two tiers are intentionally distinct.

The product palette has one axis: **mode**, `light` / `dark` / `system`.
There is one palette: neutral grounds and one amber accent. Mode is user
controlled from Settings → Appearance and from the appearance menu in the
shell, and persists to `localStorage` (`nimbus-ui:theme`). The shell sets
`data-theme` on `<html>`. Dark is the default token set on `:root`; light is
the override under `[data-theme="light"]`.

Tokens live in `packages/nimbus-ui/src/styles/tokens.css` as hex and rgba
literals. `@theme inline` bridges them to Tailwind utilities (`bg-bg-panel`,
`text-text-3`, `border-border-2`, `text-accent-link`, ...) and to the shadcn
registry names (`background`, `muted`, `ring`, ...), so a registry component
paints the same tokens without edits.

Grounds and borders:

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `--bg-canvas` | `#0a0b0c` | `#ffffff` | The page |
| `--bg-panel` | `#101112` | `#fafafa` | Sidebar, cards, tables, popovers |
| `--bg-raised` | `#18191b` | `#f4f4f5` | Inputs, code chips, menus, selected rows |
| `--bg-hover` | `#1f2124` | `#ececee` | Hovered rows and controls |
| `--border-1` | `rgba(255,255,255,.05)` | `rgba(0,0,0,.06)` | Hairlines inside a panel |
| `--border-2` | `rgba(255,255,255,.08)` | `rgba(0,0,0,.10)` | Panel and control edges |
| `--border-3` | `rgba(255,255,255,.12)` | `rgba(0,0,0,.15)` | Emphasis edges, checkbox boxes |

Text:

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `--text-1` | `#f6f7f8` | `#18181b` | Primary text, headings, values |
| `--text-2` | `#c9ced6` | `#3f3f46` | Body copy, cell text |
| `--text-3` | `#8b909a` | `#66666e` | Labels, metadata, icons at rest |
| `--text-4` | `#62666d` | `#a1a1aa` | Disabled only; below AA by design |

Accent:

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `--accent` | `#f0b23e` | `#f0b23e` | Nimbus gold. Fills only, always under `--accent-ink` |
| `--accent-hover` | `#f6c35c` | `#e0a230` | Hovered accent fill |
| `--accent-ink` | `#1a1204` | `#1a1204` | Text on an accent fill |
| `--accent-edge` | `#f0b23e` | `#866423` | The ring, a 1–2px bar, a dot, an icon, accent text |
| `--accent-link` | `#f0b23e` | `#866423` | Hyperlinks only |
| `--accent-tint` | `rgba(240,178,62,.14)` | `rgba(240,178,62,.22)` | Selected-row wash |

The mark:

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `--mark` | `#f0b23e` | `#f0b23e` | The mascot body, nothing else |
| `--mark-ink` | `#1a1204` | `#1a1204` | The mascot face |

`--mark` holds the same literal as `--accent`, and the two are still
separate tokens. A logo is exempt from the 3:1 non-text floor (WCAG SC
1.4.11); the accent is not. The mark can therefore be the gold anywhere,
unmeasured, while the accent may only be the gold where `--accent-ink`
sits on top of it. Collapsing them would lose the reason the mascot is
allowed on a white sidebar at all.

Semantic tokens (each has a `-tint` at 14% for washes):

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `--success` | `#4ade80` | `#15753a` | `Ready`, `Healthy`, additions |
| `--warning` | `#fb923c` | `#b53b0a` | `Degraded`, `Starting`, `Reconnecting` |
| `--error` | `#f87171` | `#b91c1c` | `Failed`, destructive, removals |
| `--error-ink` | `#1f0a0a` | `#ffffff` | Text on an error fill |
| `--success-ink` | `#06210f` | `#ffffff` | Text on a success fill |
| `--info` | `#60a5fa` | `#1d4ed8` | `Running`, informational |

Rules:

- **One gold, and the gold is a fill.** `--accent` is `#f0b23e` in both
  themes — the same literal as the mark, so the console has one identity
  colour and light does not fork it. Gold cannot be darkened and stay
  gold: at its own hue it turns olive, and the red-shifted ambers that
  stay vivid (`#b45309`) are a different colour standing next to the
  mascot. So the gold is never asked to carry contrast on its own. It
  appears as a fill under `--accent-ink`, as `::selection`, and as the
  `-tint` wash.
- **`--accent-edge` carries every thin accent.** The focus ring, a 1–2px
  selection bar, a 6px dot, an accent icon, accent text: all take
  `--accent-edge`, which is measured against all four grounds. It is the
  gold in dark, where the gold already clears the floors, and the gold
  darkened at its own hue (`#866423`) in light. `--accent-link` is
  hyperlinks and nothing else: never paint a button or a nav item with it.
- **Semantic colours never use the accent hue.** `Running` is `--info`
  (blue), never amber. A state token and the accent are never the same
  literal, so a status never reads as "selected".
- **Contrast is measured, not assumed.** `src/styles/contrast.spec.ts`
  reads `tokens.css` and holds every text token to 4.5:1 on all four
  grounds, `--accent-edge` to 3:1 on all four grounds (SC 1.4.11), every
  ink token to 4.5:1 on its own fill, and `--text-4` below `--text-3`. It
  also asserts `--accent` and `--mark` are one literal in both themes, and
  greps the components for a utility that would paint the gold as text or
  as a hairline. `--accent` itself is deliberately ungated against the
  grounds: it is 1.60:1 on light `--bg-hover`, which is why it is a fill.
  The light deviations from the exemplar (`--text-3`, `--success`,
  `--warning`, `--accent-edge`) exist to pass those gates.
- **Status colours must always have text or icon labels.** Colour alone is
  never the only signal.
- **Surfaces never use the accent as a fill.** The accent appears as a
  1–2px bar, an inline dot, the ring, a small icon, or a small CTA — never
  as a section background. A wash uses the `-tint`.
- **One focus ring.** The unlayered `:focus-visible` rule in `globals.css`
  paints a 2px ring of `--accent` at 40% and a 4px halo at 20% on every
  focusable element, registry primitives included. No Nimbus-owned
  component binds its own ring or outline colour under a focus variant. A
  text field may add `focus-visible:border-accent` so the edge and the ring
  agree.
### Brand Palette

The brand palette is **distinct from the product palette above**. Use it
only for:

- The mascot mark files under `docs/brand/mascot/`
- README hero images and marketing pages
- The mascot in fixed gold as favicon, app icon, and sign-in sticker (see
  **Mascot** below)
- The desktop "CLI not found" setup card (`cli-not-found.html`), which
  reads the product palette but draws the mark at the brand colours
- Print, social-media images, and external touchpoints

**Never** use brand-palette colors inside the operator console or native
chrome. The product tier wins inside the app; the brand tier wins outside
it. If you find yourself reaching for a brand color inside a console
surface, pick the equivalent product-tier token instead.

#### Two-Tier Bridge

One family crosses tiers, by design:

- **Nimbus gold.** `#f0b23e` is the sticker colour of the mascot and the
  product `--accent` in both themes. It is the one brand colour that
  appears inside the product at full strength, and it crosses the tier
  boundary unchanged because the console never asks it to do a job it
  cannot do: it fills, and `--accent-ink` sits on it.
- **Golden Hour.** Brand `#D97706` (Golden Hour stroke) is the amber the
  darkened product tokens are built from. `--accent-edge` and
  `--accent-link` are `#866423` in light — the gold taken down at its own
  hue until it clears 4.5:1 on `--bg-hover`. These are measured against the
  product grounds rather than copied from the brand sheet.

No other colour crosses tiers. The product has no blue identity and no
teal accent, and the marketing surfaces take the same night and paper
grounds as the product (`#0a0b0c` and `#ffffff`).

#### Mark files

`docs/brand/mascot/` holds the static exports of the mascot. Every file is
the same 120x92 drawing from `mascot.tsx`, cropped to `0 8 120 80` so a
lockup controls its own spacing.

| File                    | What it is                                                   | Used by                                   |
|-------------------------|--------------------------------------------------------------|-------------------------------------------|
| `mascot-gold.svg`       | Gold `#f0b23e` body, `#1a1204` face                          | README, docs nav, OG art                  |
| `mascot-tile.svg`       | Gold mascot centred on a `#0a0b0c` tile with a 104 radius    | App icon, apple-touch-icon                |
| `mascot-template.svg`   | Black silhouette with the face cut out, heavier face         | macOS tray template                       |
| `render.sh`             | Renders `icon-512.png` and, with `DESKTOP_DIR`, the desktop `icon.png`, `icon.icns`, `icon.ico`, and tray PNGs | Release prep |

The wordmark is lowercase `nimbus` whenever it is set next to the mark, at
semibold with `-0.01em` tracking, and the mark sits at 24 to 30px beside
it.

#### Mascot

The Nimbus mark is a cloud with a face. The body is the union of three
circles and a rounded base in a 120×92 box, with no wisp, so it reads as
one shape from 16px up. The face carries the state; nothing else moves.
`packages/nimbus-ui/src/components/mascot.tsx` is the single source of the
drawing; the static assets are exports of it.

There is one drawing and it is always filled. In the console the body takes
`--mark` and the face takes `--mark-ink`: Nimbus gold with an ink face on
the light grounds, white with an ink face on the dark ones. Outside the
console the same drawing is fixed at the sticker colours, `#f0b23e` on
`#1a1204`, on a `#0a0b0c` tile where it needs a ground.

| Where                                                       | Body / face                              |
|-------------------------------------------------------------|------------------------------------------|
| Sidebar brand row, mobile top bar, overview and nodes headlines, empty states, first run, disconnected banner | `--mark` / `--mark-ink` |
| Favicon, app icon, desktop icon and tray, sign-in card, README, docs nav | Fixed `#f0b23e` / `#1a1204`; the README and docs swap to white / `#18181b` under a dark scheme |

The outline variant is gone. A stroked cloud on the panel ground read as a
second logo next to the filled one on the app icon, and at 24px its
1px stroke lost to the semibold wordmark beside it.

- **States.** `idle` (dot eyes, smile), `working` (eyes to the side, flat
  mouth, thought dots), `error` (crossed eyes, wobble, one drop), `empty`
  (closed eyes, flat mouth, zz), `celebrate` (arc eyes, grin, sparks). A
  state is a prop, never a separate asset.
- **Motion.** The eyes blink once every six seconds and nothing else
  animates. The component leaves the animation out of the DOM under
  `prefers-reduced-motion: reduce`; the `globals.css` backstop is the
  second net.
- **Sizes.** 16 in a tab, 24 to 30 in the nav, 32 in a card header, 48 and
  up in an empty state. Below 40px the face thickens (mouth 4 to 5.5 units,
  eyes 3.7 to 4.6) so it survives the tab bar.
- **Static exports.** `favicon.svg` and `favicon.ico` (16, 32, 48) and
  `icon-512.png` (gold face on a `#0a0b0c` tile) under
  `packages/nimbus-ui/public/`, plus the brand set under
  `docs/brand/mascot/` (see **Mark files** above). The favicon is one
  fixed-colour drawing, so the console does not swap it when the theme
  changes. The desktop app icon, the tray template, and the docs favicon
  are renders of the same drawing.
- **Mark rule.** `--mark` paints the mascot body and nothing else. It is
  not a second accent: no button, badge, wash, or text takes it. The
  accessories outside the body (thought dots, drop, zz, sparks) take
  `currentColor`.

### Documentation Site (nimbusdocs.com)

The Documentation site is the third brand surface, sitting between the
product tier (operator console) and the brand tier (marketing). Its
governing rule: **the doc body is product-tier; the home page is the site's
single brand-tier moment.** Renderer: a Next static export drawn by
fumadocs in `website/`; tokens live in `website/src/styles/tokens.css`, the
host theme in `website/src/styles/docs-theme.css`, and the home page in
`website/src/styles/odyssey.css`.

- **Doc body = product tier.** Fumadocs paints every surface through a
  `--color-fd-*` custom property. `docs-theme.css` answers each one from a
  Nimbus role token rather than from the vendor gray scale, so the docs and
  the console they document share one set of roles. `--color-fd-primary`
  takes `--accent-edge`, not `--accent`: fumadocs uses "primary" as text far
  more often than as a fill, and a thin accent is `--accent-edge` under the
  §Colour rule. `--color-fd-accent` is the hover ground, because fumadocs
  means the hovered surface by that word where Nimbus means the brand
  colour. One identity family per mode — teal and blue stay out of the doc
  body, and the doc body carries no gradient.
- **Home page = brand tier, once.** `/` is the Odyssey: one scroll-driven
  canvas that follows a request from an app through the binary and back,
  twenty chapters mixed from a single progress value. It is the only place
  on the surface that leaves the role tokens, and it ends on Nimbus gold.
  Under reduced motion, or on a viewport too short to hold the stage, it
  renders as a storyboard of stills carrying the same arc. `/docs/` is the
  written entrance to the same story and is product tier throughout.
- **Logo + wordmark + favicon.** The top nav renders the mascot next to the
  lowercase wordmark `nimbus`. The mark is the mascot in fixed gold —
  `--mark` `#f0b23e` body on `--mark-ink` `#1a1204` face, the same
  `favicon.svg` the operator console ships (see **Mascot** above). The mark
  does not theme, so the docs site swaps nothing on `data-theme` and needs
  no per-theme copies: one sticker on every surface, at every size. The
  social card at `/og.png` is the same mark on Night, rendered from
  `docs/brand/mascot/og.svg` by `docs/brand/mascot/render.sh`.
- **Typography.** Body and headings use Geist; code, IDs and paths use Geist
  Mono. Both are self-hosted woff2 under `website/public/fonts/`, so the
  site loads no third-party font. Tables apply `tabular-nums`. Radius 6px
  default / 8px cards, per §Spacing And Shape.

#### Messaging canon — one sentence, three surfaces

The canonical sentence is:

> **The single-binary backend for apps and AI agents. Drop-in compatible
> with Convex, Firestore, MongoDB, and DynamoDB.**

It must appear, identically or as a tight variant, on exactly three
surfaces: the GitHub repo description, the README banner sub-line, and the
docs splash-hero tagline. "**BaaS in a binary. For apps and agents.**" is
the short spoken hook (README banner headline). Nimbus is
**source-available** (`LICENSING.md`); no surface may claim "open source".

### Typography

- Body / UI: **Geist** (self-hosted from `packages/nimbus-ui/public/fonts/`,
  weights 400, 500, 600) with `system-ui, "Segoe UI", sans-serif` as
  fallbacks.
- Monospace: **Geist Mono** (self-hosted, weights 400, 500, 600) with
  `"JetBrains Mono", ui-monospace, monospace` as fallbacks. Used for IDs,
  digests, request IDs, function paths, ports, bytes/duration values, code
  blocks, JSON/BSON values, and shell snippets.
- Scale (`--text-*` in `tokens.css`; the Tailwind defaults are reset, so
  only these steps exist):

| Step | Size / line | Tracking | Use |
| --- | --- | --- | --- |
| `xs` | 12 / 16 | 0 | Labels, captions, table metadata |
| `sm` | 13 / 18 | 0 | Compact table text, menus, chips |
| `base` | 14 / 20 | 0 | Body |
| `md` | 16 / 24 | 0 | Section heading |
| `lg` | 20 / 28 | `-0.01em` | Page title |
| `xl` | 24 / 32 | `-0.01em` | Empty states and onboarding |
| `2xl` | 32 / 38 | `-0.02em` | Metric values |

- Monospace inline runs at `0.93em` with `-0.01em` tracking so an ID in a
  row reads at the same cadence as the sans around it. Code blocks run at
  `1em`.

Rules:

- Do not scale type with viewport width.
- **Labels are sentence case** in the sans at `text-xs font-medium
  text-text-3`. Tracked small caps (`uppercase` + `tracking-*`) are retired;
  the sidebar group heading is the one place that may keep them.
- **Mono is the voice of data, never of labels.** A category chip, a column
  header, or a form label is sans. A value, an ID, or a path is mono.
- Reserve `xl` and `2xl` for empty states, onboarding, and metric values,
  not for dashboard chrome.
- **All numeric columns** (durations, counts, sizes, ports, rates,
  percentages, timestamps) must apply `font-variant-numeric: tabular-nums`.
  Without this, live tables jitter on every tick. This is a hard
  requirement, not a polish.
- Status badges use tabular lining figures so counters do not reflow.

### Spacing And Shape

- Base spacing: 4px grid.
- Dense table row: 36-40px.
- Comfortable row: 44-48px.
- Panel padding: 12-16px.
- Page gap: 16-24px.
- Radius scale (`--radius-*` in `tokens.css`; the Tailwind defaults are
  reset): `xs` 4px for chips and inline code, `sm` 6px for controls and
  inputs, `md` 8px for cards, panels and popovers, `lg` 12px for dialogs,
  `xl` 16px for onboarding cards, `full` for dots and pills. Bare `rounded`
  and the `2xl`+ steps compile to nothing; a spec gates them.
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
- Loading state preserves table geometry with skeleton rows. `DataTable`
  draws them itself (`loading`, `skeletonRows`) under the real header, so a
  page never swaps a centered label for the table.
- Past 100 rows `DataTable` virtualizes: only the rows in and around the
  viewport are in the DOM (`data-virtual="true"`, `aria-rowcount` for the
  full count). A 200-row page costs the same as a short one.
- Right-click on a row is a peer of click. `DataTable` reports the row and
  an anchor (`onRowContextMenu`); Shift+F10 and the ContextMenu key raise
  the same menu from the keyboard. Arrow keys move focus between rows, one
  row at a time in the tab order, and Enter or Space activates.

### Forms And Editors

- Use **`SegmentedControl`** as the canonical exclusive-choice control for
  ≤4 options (`packages/nimbus-ui/src/components/segmented-control.tsx`).
  Both the sidebar Developer/Operator view switcher and the Settings
  appearance mode toggle render through it so they cannot drift. `role="radiogroup"`
  with each segment `role="radio"`; ArrowLeft/Right (and ArrowUp/Down)
  move focus, Home/End jump to the edges, Enter/Space commit.
- Use **`Select`** for >4 options or when a label-prefixed dropdown reads
  better than a row of segments (`packages/nimbus-ui/src/components/select.tsx`).
- Use toggles/checkboxes for binary settings.
- Use inputs/sliders/steppers for numeric values.
- Use JSON/code editors for document, argument, and config values.
- Validate on blur and before submit. Show field-specific errors.

### Badges

State badges render as a **labeled dot**, not a filled pill. The dot is
8px, the label uses tabular figures, the row stays calm. Filled pills are
reserved for adapter/kind/backend categorical badges.

State → token binding (mandatory; do not improvise mappings):

| State | Token | Dot glyph |
| --- | --- | --- |
| `Ready`, `Healthy`, `OK`, `Active`, `Connected`, `Completed` | `--success` | ● solid |
| `Running` | `--info` | ● pulsing (respects `prefers-reduced-motion`) |
| `Starting`, `Provisioning`, `Restarting` | `--warning` | ◐ half-filled |
| `Draining`, `Stopping`, `Deleting` | `--text-3` | ◐ half-filled |
| `Queued`, `Pending` | `--text-3` | ○ outline |
| `NotReady`, `Degraded`, `Reconnecting` | `--warning` | ● solid |
| `Stopped`, `Created`, `Idle`, `Paused`, `Uninitialized` | `--text-3` | ○ outline |
| `Failed`, `Crashed`, `Offline` | `--error` | ● solid |
| `Stale` (post-disconnect) | `--text-3` | ● solid + label strikethrough |
| `Unknown` | `--text-3` | ? glyph |

Each row is a state *family*. Names after the first are aliases that fold onto
that family — they do not get their own tone. Add a new state by folding it onto
the family it belongs to; introduce a new row only when the state is genuinely a
new lifecycle position, and give it a token from the table above.

Any state string the console can produce but this table omits renders as
`Unknown` — a literal `?`. That is the correct failure mode (it reads as "the
console lost track of this resource"), which makes an unlisted state a visible
bug rather than a silent miscolour. Lock the two sides together with a test that
asserts every state a route or hook can emit renders a non-`?` glyph.

Categorical (filled pill on `--bg-raised`, sentence case, `text-xs
font-medium` in the sans):

- Function kind: `Query`, `Mutation`, `Action`, `HTTP`, `Scheduled`, `Cron`.
- Adapter: `Convex`, `MongoDB`, `Firebase`, `CloudFn`, `Native`.
- Backend: `redb`, `SQLite`, `Postgres`, `MySQL`, `libSQL`.

Badges are plain, compact, and readable. Do not use pill farms as decoration.
Do not place more than two categorical badges on the same row.

### Logs And Events

- Logs are one table whose rows group under the run that wrote them. A
  group head carries the run status pill, function path, kind, duration,
  relative time, and line count; lines that belong to no run sit under
  the `server` head.
- Required columns: time, level, source, message, run.
- The run sheet shows the run summary, the error, and the correlated
  lines. A log line jumps to its run page; a run row opens the sheet.
- Filters are a facet bar: search, tenant, level, category, source, and
  correlation on Logs; tenant, status, and function on Runs. Every facet
  lives in the address.
- Search is a request, not a subscription: `?q=` reads a bounded page of
  the newest matching lines from the server, with the match count and
  the reach of the scan, and the matches group under their runs like the
  stream. A run detail page links to the Logs tab narrowed to that run,
  where the same field searches the run's lines alone.
- `Follow` keeps the newest line in view. `Pause on error` freezes the
  stream at the first line at `error` level or above and `Resume` picks
  the stream back up.
- Preserve scroll position while new lines arrive when follow is off.

### Data Browser

- Use cursor pagination, not unbounded fetches. One page is 200 documents,
  drawn by the virtualized `DataTable`; sorting stays server-side
  (`manualSorting`) and every click on the active column flips its
  direction.
- Show active filters and sort order as editable chips.
- Document values open in a drawer with JSON/BSON/Firestore type fidelity.
- Inline editing is allowed only when the backend supports the exact mutation
  and schema validation result is visible before commit.
- Bulk edits require explicit selection and confirmation.

### Function Runner

- The runner is a bottom drawer on the function page. Its toggle bar shows the
  kind and adapter as category pills, the active tenant as read-only text, and
  the state of the last run as a pill (running, ok with duration, error).
- The argument editor is schema-aware when `argsSchema` is available. Both the
  SDK validator shape (`{kind, fields}`) and the Convex JSON shape
  (`{type, value}`) produce **Form** mode: one field per argument, text for
  string and id, number for number, a checkbox for boolean, a JSON textarea for
  everything else, with the optional marker on the type. **JSON** mode is
  always available and the two modes carry values across the switch. Without a
  validator the editor is JSON only.
- Submit is the **Run function** button or ⌘⏎ (Ctrl+⏎ elsewhere). Plain Enter
  in a text field does not submit, so a mutation never runs by accident.
- Tenant is implicit from the sidebar tenant selector. The runner shows it and
  never offers a second chooser.
- A failed run renders one of two cards. `function.thrown` (the handler's own
  `throw`) is the developer's error: the card says which function threw,
  shows the thrown message with its `(at module:line)` location, folds the
  stack under a **Stack** disclosure, and offers **View runs**, which opens
  the function's Runs tab where the run row keeps the same message, location,
  and stack. Every other code (`op.*`, `runtime.*`, `service.*`) is a request,
  runtime, or service fault, and the card keeps the server's remediation copy
  (for `service.internal`, the operator-investigation instruction keyed by
  request id). The card never shows a stack for a service fault, because the
  server does not have one to show.
- Query runs can auto-refresh/react when backed by subscriptions.
- Mutations and actions run only on explicit submit.
- Results and logs share the same request/run correlation ID; the result panel
  shows it as a copy chip for both success and error envelopes.
- Identity controls are labeled as simulated/admin-local identity unless a
  real auth provider is active.

### Command Palette (⌘K)

A global command palette is table stakes for a developer console. Triggered
by `⌘K` (macOS) / `Ctrl-K` (Windows/Linux), the palette must:

- Open from anywhere — sidebar, table, drawer, modal, runner — without
  losing the underlying view's scroll or focus state. Closing it returns
  focus to the control that opened it.
- Open on one list, 640px wide, with three fixed groups and no mode toggle:
  - **Routes**: every page of the current console first, then the other
    console's pages, each tagged `Developer` or `Operator`.
  - **Tenants**: every tenant, the active one checked. A pick in the
    developer console switches the active tenant in place; a pick from the
    operator console switches and opens the developer console.
  - **Actions**: switch console, open the system tenant lens (developer
    only), refresh the current view, switch theme.
- Add resource groups as the operator types: tables, functions, services,
  machines, HTTP routes, each found by name, ID, or path.
- Show recent picks at the top of the empty list, persisted under
  `nimbus-ui:commands:recent`.
- Write the keyboard contract in its footer: `↑ ↓` move, `⏎` open, `⎋`
  close, `⌘K` palette, `⌘\` tenant lens, `/` filter page. Show the
  shortcut beside every action that has one.

Implementation: the shadcn `command` primitive (`cmdk` inside a Base UI
dialog), mounted at the app root.

### Sidebar Footer

The console has no bottom status bar. What was in it lives in the sidebar
footer, which is on every screen and reads top to bottom: the connection
line (state dot, `Connected`, server version), the update row, the theme
toggle, and the collapse control. The server URL is on the Settings page.

The update row is the one footer row that is not always there. It appears
under the connection line when the server is behind the latest release
(`Update to 0.2.0`, pending dot, opens the upgrade popover), follows the
upgrade while it runs (`Updating to 0.2.0…`), holds `Updated to 0.2.0`
for a moment after, and is absent when the console is current. In the rail
the row is its dot with a tooltip; the button keeps its label for a screen
reader. The three tones read from the shared state palette (`pending`,
`starting`, `ready`), never a private table.

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

Use `sonner` for transient feedback. Anchor: bottom-right, with the
default offset; nothing fixed sits under the toast stack. Rules:

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

The next action is on the page, not on another page. An empty state that
says "open Compute" or "click Create tenant in the top nav" sends the
reader away without telling them what to do when they get there. Instead,
every empty list on a tenant-scoped page carries the one command that puts
the first row on it, addressed to this server and this tenant, with a copy
control: create a tenant, insert a document (which also creates the table),
call a function (which records a run and its log lines), schedule a job,
register a cron, put an object (which also creates the bucket). The
commands live in `src/components/onboarding/next-action.ts`, so they share
one server address, one token convention (`$NIMBUS_TOKEN`), and the
quick-start names (`demo`, `messages`, `messages:send`) as fallbacks. A
one-line command is a chip that truncates; a multi-line command is a
left-aligned block that scrolls sideways, because a truncated second line
hides the part the reader came for. A button beside the command opens the
console's own form for the same action when one exists (insert document,
create tenant, new bucket).

A filtered-empty result (a facet or a filter chip is set) blames the
filter and offers to clear it; it carries no command, because the row the
reader wants may already exist.

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
- Removed: `--error` left border + `--error-tint` surface.
- Added: `--success` left border + `--success-tint` surface.
- No saturated reds/greens — use the same state tokens.
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

Settings owns server administration; the Deploys page owns deployment
history and rollback:

- Server info: version, uptime, listen address, data directory, storage
  backend, and active local server origin. This is the System sub-page.
- Configuration display: runtime limits, license status and usage, auth
  provider config, adapter enablement, and storage topology. Configuration is
  read-only in Phase 1 unless a dedicated write API exists.
- Deploys (`/developer/deploys`): every bundle activation the server has
  recorded, newest first, from `GET /api/admin/deploys`. Each row carries
  the bundle's SHA-256 provenance hash, the activation kind (`deploy`,
  `rollback`, `startup`), the generation, the actor, and the function count
  with the paths added and removed against the previous activation. A row
  opens a strip that lists the function paths that come back, stop
  resolving, and stay unchanged against the active bundle. **Roll back to
  this bundle** posts `POST /api/admin/deploys/{sha256}/rollback` behind a
  `ConfirmDialog` that names how many paths stop resolving; the server
  stages the retained files, runs the same integrity check a deploy runs,
  and refuses a tampered bundle, the active bundle, and a bundle whose
  files were not retained (a startup row). The menu disables the action
  in those cases with the reason as the hint. A deploy itself arrives
  through `nimbus deploy`; the console has no deploy trigger.
- Token and session: current session state, token rotation with confirmation,
  and forced re-auth after rotation. Rotation lives on the Shutdown sub-page
  until the Token sub-page exists.
- Shutdown: graceful shutdown with typed confirmation and clear disconnect
  state. Both writes use the shared `ConfirmDialog`; nothing on the page
  hand-rolls a modal.

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
- Prefer shadcn `base-nova` registry components on Base UI primitives,
  Tailwind v4 with the hex role tokens in `tokens.css` bridged through
  `@theme inline`, `cmdk` for the command palette, `sonner` for toasts,
  `shiki` for syntax highlighting, Geist and Geist Mono self-hosted for
  type, Lucide for icons, TanStack Router, Table, Virtual and Charts,
  Zustand, Vitest, React Testing Library, and Playwright as described in
  `docs/private/plans/archive/desktop-ui-plan.md`.

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

- `docs/private/current-capabilities.md`
- `docs/private/plans/archive/desktop-ui-plan.md`
- `docs/private/plans/archive/system-tenant-api-plan.md`
- `docs/private/adapters/convex/compatibility.md`
- `docs/private/adapters/firebase/compatibility.md`
- `docs/private/adapters/mongodb/README.md`
- `docs/private/adapters/mongodb/operations.md`
- `docs/private/adapters/native/README.md`
- `docs/private/architecture/sandbox/microvm-service-baseline.md`
- VoltAgent `awesome-design-md` as the plain-text design-system pattern
- Convex dashboard docs for Health, Data, Functions, Schedules, Logs, Settings
- MongoDB Atlas docs for Data Explorer and Indexes
- Firebase docs for Firestore console and Cloud Functions logging
