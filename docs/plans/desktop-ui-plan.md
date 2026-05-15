# Plan: Desktop UI

Canonical execution plan for a Docker Desktop / Podman Desktop-style graphical
interface for Nimbus. The UI is an embedded React SPA served by
`nimbus-server` at `/ui/*`, consuming the system tenant query surface and
HTTP lifecycle endpoints via the `nimbus` JS SDK's `useQuery` /
`useMutation` hooks over the existing Convex-compatible WebSocket.

This plan covers the **React frontend only** — the server-side prerequisites
are owned by separate plans (see Prerequisites below).

The root [`DESIGN.md`](../../DESIGN.md) is the UI design-system authority for
this plan. It defines Nimbus as an operational console spanning compute,
storage, network, machines, adapters, and observability. Implementers must
read it before DU1-DU10 work.

2026-05-15 readiness decision: Phase 1 remains the correct first desktop
surface. Do not create the `nimbus/desktop` Electron repository before the
embedded SPA and `_nimbus` system tenant API are working. When the native
shell is needed, it is an Electron thin wrapper in a separate
`nimbus/desktop` repository that starts or discovers the local `nimbus`
server and loads the same `/ui/*` bundle.

Reviewed against:

- `DESIGN.md` — product stance, information architecture, visual tokens,
  adapter capability UX, and implementation rules
- `crates/nimbus-server/src/router.rs` — current route tree,
  `tower_http::services::ServeDir` static serving at `/demos`
- `packages/nimbus/src/react.ts` — `NimbusProvider`, `useQuery`,
  `useMutation`, `useAction`, `usePaginatedQuery`, `useQueries`,
  `useNimbusAuth`, `useNimbusConnectionState`
- `packages/nimbus/src/browser.ts` — `NimbusClient`, `ConnectionState`
- `demos/convex/html/` — proven end-to-end: codegen → React hooks →
  WebSocket → engine queries/mutations

Open source reference implementations studied:

| Project | Stars | Stack | Pattern | Key lesson |
| --- | --- | --- | --- | --- |
| Podman Desktop | 7.5k | Electron 41 + Svelte 5 + Tailwind 4 | Electron IPC to Podman socket | Co-located `.spec.ts` tests, typed IPC via `dts-for-context-bridge`, Electron Fuses, 297+ IPC channels |
| Jan | 42k | Tauri 2 + React 19 + Radix UI + Tailwind 4 | localhost REST API via embedded hyper proxy | Service Hub platform-abstraction, Zustand 5 + TanStack Router, unified `radix-ui` package |
| Portainer | 37k | React + Go | Go serves SPA, REST + WebSocket | Validates "server embeds and serves the SPA" pattern |
| Prisma Studio | — | React component lib | BFF pattern | Cleanest embedded dev-UI pattern |
| zero-native | 3.5k | Zig shell + Web UI, system WebView or CEF | Policy-controlled `window.zero` bridge | Promising thin-shell candidate, but pre-release and not cross-platform enough for the enterprise default yet |

Product console references studied:

| Product | Reference | UI lesson |
| --- | --- | --- |
| Convex Dashboard | Health, Data, Functions, Schedules, Logs, Settings docs | Nimbus should offer comparable operator coverage for supported compute, storage, schedule, log, and settings surfaces. |
| MongoDB Atlas | Data Explorer and Indexes docs | MongoDB adapter screens should expose databases/collections/documents/indexes without claiming Atlas-only features. |
| Firebase Console | Firestore console and Cloud Functions logging docs | Firebase adapter screens should expose collection/document/query/listen/log concepts while clearly naming Nimbus's current emulator/control-plane limits. |
| VoltAgent `awesome-design-md` | Plain-text `DESIGN.md` pattern | Keep design tokens, interaction rules, and information architecture in a repo-visible file agents can follow. |

External source refresh, 2026-05-15:

- React docs list 19.2 as the current major/minor docs line.
- Vite 8 is stable and ships Rolldown as its unified Rust bundler.
- shadcn/ui documents Tailwind v4 + React 19 support and the `data-slot`
  component shape.
- Electron 41 is current enough for a shell proof and the official security
  checklist still requires current Electron, context isolation, process
  sandboxing, CSP, navigation limits, window-open limits, sender validation,
  and fuses.
- Tauri 2 is mature and small, but remains a system-WebView shell.
- zero-native v0.2.0 is promising but pre-release; current docs say the beta
  target is macOS desktop apps, CEF/Chromium packaging is macOS-first, Windows
  support is in progress, and Linux Chromium is not wired.

Cloned reference repositories (shallow, for future agents to consult):

| Repo | Local path | Why |
| --- | --- | --- |
| `podman-desktop/podman-desktop` | `~/src/github.com/podman-desktop/podman-desktop` | Electron architecture, security hardening, co-located tests, cross-platform packaging |
| `janhq/jan` | `~/src/github.com/janhq/jan` | React + Radix + Tailwind stack, TanStack Router, Zustand stores, Service Hub pattern |

---

## Prerequisites

These plans must complete before UI implementation begins:

| Plan | What it provides | Items |
| --- | --- | --- |
| `docs/plans/archive/websocket-protocol-plan.md` | Versioned protocol spec, error schema, subprotocol negotiation, structured error types | WP1–WP4 |
| `docs/plans/archive/localhost-server-security-plan.md` | Token file, origin allowlist, session cookie, CSP, server discovery, audit log, middleware stack | LS1–LS5 |
| `docs/plans/archive/system-tenant-api-plan.md` | `_nimbus` system tenant, state persistence, HTTP lifecycle endpoints, Convex function bundle | ST1–ST4 |

## Status

- **Status:** `active`
- **Primary owner:** this plan
- **Activation gate:** WebSocket protocol and localhost security prerequisites
  are complete; archived `docs/plans/archive/system-tenant-api-plan.md`
  ST1-ST4 now provide the non-UI data/control-plane surface for data-backed
  UI tabs.
- **Related plans:**
  - `docs/plans/archive/websocket-protocol-plan.md` — protocol and error schema
  - `docs/plans/archive/localhost-server-security-plan.md` — auth and server security
  - `docs/plans/archive/system-tenant-api-plan.md` — completed data layer the UI consumes
  - `docs/architecture/sandbox/microvm-service-baseline.md` — machine/service architecture

## Current Assessed State

- No production UI, no `nimbus ui` subcommand, no embedded SPA exist today.
- Localhost UI security is already implemented as a minimal bootstrap by the
  completed localhost security plan: `/ui/*`, `/ui/auth`,
  `/ui/auth/session`, signed session cookies, one-time launch tickets, and
  CSP exist in `crates/nimbus-server/src/http/ui.rs`.
- Current `/ui/*` product UI is only a bootstrap placeholder. The real UI
  must follow `DESIGN.md` and must not narrow into a VM manager with data tabs
  bolted on.
- The JS SDK ships all needed hooks (`useQuery`, `useMutation`, `useAction`,
  `usePaginatedQuery`, `useQueries`, `useNimbusConnectionState`).
- The server already serves static files at `/demos` via
  `tower_http::services::ServeDir`.
- The React demos in `demos/convex/html/` prove the full stack end-to-end on
  the current npm baseline: React 19.2.x, `@vitejs/plugin-react` 6.x, and
  Vite 8.0.x.
- The `_nimbus` system tenant, packaged backend query bundle, machine/service
  lifecycle API, network posture, scheduler/cron state, run history, table
  metadata, system status, token rotation, and shutdown surfaces are now in
  place under archived `docs/plans/archive/system-tenant-api-plan.md`. The
  headless React hook proof for generated `_nimbus` refs exists, and the CI-shaped Rust gate is
  green for the product runtime/workspace lanes. Machine rename is not a DU1
  blocker; add it later only if product design requires it. Document/schema/
  deploy writes remain on the HTTP lifecycle/data APIs by design rather than
  `_nimbus` Convex mutations.

## Control Plan Rules

1. The UI is a **consumer** of the system tenant query surface and HTTP
   lifecycle endpoints — no direct storage writes, no bypass of `Service`.
2. The embedded SPA is the **primary** UI surface. The Electron shell
   (Phase 2) loads the same bundle from the same localhost URL.
3. The UI is served **from the same process** as the API.
4. A native shell is optional packaging, not a second product architecture:
   it may own window chrome, tray/menu/update/deep-link/server-lifecycle
   integration, but not queries, mutations, service orchestration, or data
   access.
5. The app is a Nimbus operational console first. It must expose compute,
   storage, network, machines, adapters, and observability as one coherent
   system.
6. Adapter-specific screens must include capability posture. Use
   `supported`, `supported with caveats`, and `not claimed` instead of
   implying upstream Convex, MongoDB, or Firebase parity where Nimbus has not
   implemented it.
7. Use the Convex plugin and `docs/adapters/convex/ai-guidelines.md` for the
   `_nimbus` system-tenant function bundle and React hook usage. Do not use
   Convex as the visual design system or as the only information architecture
   for MongoDB, Firebase, Native, machine, or network screens.

## Verification Contract

Each roadmap item must satisfy before closing:

- `cargo fmt --all --check` — clean
- `make clippy` — clean
- Required Rust CI shape — green:
  `cargo test -p nimbus-runtime -- --skip runtime::tests::node_compat::`;
  `cargo nextest run --workspace --exclude nimbus-runtime`;
  `cargo test --workspace --exclude nimbus-runtime --doc`. When `nextest`
  is unavailable locally, use
  `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test --workspace --exclude nimbus-runtime`
  as the fallback proof. The raw `make test` target includes the dedicated
  Node-compat conformance corpus, which is tracked by runtime-owned evidence
  workflows rather than the desktop UI prerequisite gate.
- `npm run build --workspaces --if-present` — green (JS)
- `npm run test --workspaces --if-present` — green (JS)
- Keyboard navigation works for all interactive elements added
- `@axe-core/react` reports zero critical or serious a11y violations
- Dark mode renders correctly (no invisible text, no broken contrast)
- Bundle size of `packages/nimbus-ui/dist/` stays under 500 KB gzipped
- Browser-driven verification via `playwright-cli` and `chrome-devtools-mcp`
  (see [Verification Tooling](#verification-tooling))
- Manual verification described per item

## Verification Tooling

Two browser-driving tools are pre-installed for this plan:

| Tool | Form | When to use | Artifact location |
| --- | --- | --- | --- |
| `playwright-cli` | Claude Code Skill at `.claude/skills/playwright-cli/`, also `@playwright/cli` in repo devDependencies | Primary driver — `open` / `goto` / `snapshot` / `click eN` / `fill eN` / `press` / `console` / `network` / `screenshot` / `state-save` / `tracing-start` | `.playwright-cli/page-*.yml`, `console-*.log`, `*.png` (gitignored) |
| `chrome-devtools-mcp` | MCP at user scope and project `.mcp.json` | Perf traces, CDP heap/coverage, deeper network/CSP inspection (`performance_start_trace`, `list_console_messages`, `list_network_requests`) | tool results returned inline |

`@playwright/mcp` was researched and rejected: roughly 4× higher token
cost than the CLI for the same task (snapshots stream through context vs.
write to disk as ref-based YAML), and the CLI is the path Microsoft built
for Claude Code specifically.

Two dev URLs to know:

| URL | Source | When to use |
| --- | --- | --- |
| `http://localhost:5173/` | `npm run dev -w packages/nimbus-ui` (Vite) | Component iteration with HMR; no auth flow |
| `http://localhost:8080/ui/` | `cargo run -p nimbus-bin -- start --port 8080 ...` then `nimbus ui` | Auth/CSP/embedding/system-tenant verification — production-equivalent path |

Per-DU verification specifies which URL is required. Default to the Vite
URL until DU1 ships embedded assets.

**Accessibility (axe-core):** add `@axe-core/playwright` to
`packages/nimbus-ui` dev deps and call `AxeBuilder` from an inline
Playwright spec (no separate axe MCP — same engine, fewer moving parts).
Required for DU3, DU4, DU7, DU8.

**Snapshot discipline:** snapshot only when asserting, not after every
action. Element refs (`eN`) from one snapshot remain stable until the next
navigation or DOM mutation — chain `click eN` / `fill eN` without
re-snapshotting between steps.

**Artifact policy:** `.playwright-cli/` is gitignored. Treat it as scratch.
Promote a specific trace under `tests/visual/` or `tests/e2e/artifacts/`
only when it backs a checked-in test.

## Architecture

### Phasing

```
Phase 1: Embedded Web UI          Phase 2: Thin Native Shell
┌──────────────────────────┐      ┌──────────────────────────────┐
│  packages/nimbus-ui/     │      │  nimbus/desktop repo         │
│  React + shadcn/ui       │      │  Electron 41.x               │
│  Convex function bundle  │      │  (mac/win/linux)             │
│  Vite build → dist/      │      │         │                    │
│         │                │      │         ▼                    │
│         ▼                │      │  loadURL(localhost:PORT/ui)  │
│  rust-embed in           │      │  + tray, menus, auto-update  │
│  nimbus-server           │      └──────────────────────────────┘
│         │                │
│  GET /ui/* routes        │
│  nimbus ui subcommand    │
└──────────────────────────┘
```

### Component stack

| Layer | Choice | Version | Rationale |
| --- | --- | --- | --- |
| Framework | React | >=19.2.1, latest patch | Already used by JS SDK and demos; current React docs list 19.2 as latest; 19.2.1+ patches React2Shell security issue |
| Components | shadcn/ui source components + Base UI (MUI) primitives + Tailwind | pin generated dependencies exactly | Copy-pasted source, no hidden runtime abstraction; shadcn's Tailwind v4 docs explicitly support React 19 and `data-slot` components; use `shadcn init --style base` for Base UI primitives |
| Animations | tw-animate-css | 1.x (avoid v2 breaking change) | Pure CSS animations for Tailwind v4; shadcn deprecates `tailwindcss-animate` in favor of `tw-animate-css`; pin 1.x — v2 has breaking API changes |
| State | Zustand | 5.0.x | Lightweight, native `useSyncExternalStore`; v5 drops default exports |
| Router | TanStack Router | 1.x, exact current | Type-safe, file-based routing; keep route tree source-owned in the UI package |
| Bundler | Vite | 8.0.x | Current demos already use Vite 8; Vite 8's Rolldown baseline is the modern path |
| CSS | Tailwind CSS | >=4.3 | CSS-first config (`@theme` directive); colors use OKLCH; 4.3 adds scrollbar styling |
| Icons | Lucide | exact current | MIT, tree-shakeable, shadcn/ui default |
| Monospace font | JetBrains Mono | `@fontsource/jetbrains-mono` latest | Distinctive monospace for IDs, digests, ports, durations, code blocks; self-hosted to avoid Google Fonts dependency |
| Tabular figures | CSS `font-variant-numeric: tabular-nums` | — | Required on all numeric columns to prevent jitter on live updates |
| Theming | Tailwind v4 `@theme` directive + OKLCH tokens | — | OKLCH gives parity-matched light/dark; `prefers-color-scheme` detection |
| Command palette | `cmdk` | latest | Industry standard; used by Linear, Vercel, Raycast; powers ⌘K navigation + actions + filter |
| Toasts | `sonner` | latest | shadcn/ui default; anchored above bottom status bar |
| Syntax highlighting | `shiki` | latest | Used for code blocks, diff viewer, raw JSON in system tenant lens |
| Accessibility | Base UI ARIA primitives + axe-core | — | WCAG 2.1 AA target; Base UI implements WAI-ARIA 1.2 |
| Embedding | `rust-embed` | 8.x, exact current | Replace the current minimal `/ui` placeholder with embedded built assets |
| Testing | Vitest 4.x + React Testing Library 16.x | — | Vitest 4 matches the current Podman Desktop reference; RTL 16 supports React 19 |
| Visual regression | Chromatic | latest | Snapshot story matrix; catches token/density regressions |
| E2E | Playwright | 1.60.x | Pin version for API stability |

**Primitive layer rationale (Base UI over Radix):** Base UI is the active
headless primitive layer from MUI with full-time engineering, full shadcn/ui
component coverage (since January 2026), and WAI-ARIA 1.2 support. Radix UI
entered low-maintenance after the WorkOS acquisition — one active maintainer,
812+ open issues, last substantive code change November 2025, and its
co-creator publicly recommended against it for new projects. If Base UI proves
problematic, shadcn/ui supports switching to Radix via `components.json` style
configuration without a full rewrite.

### UI north star

`DESIGN.md` defines the product surface this plan implements. The minimum
Phase 1 scope is not "machines plus logs"; it is the full local operator
console below:

| Pillar | Required Phase 1 surfaces |
| --- | --- |
| Compute | Functions, actions, HTTP routes, runner, scheduled jobs, cron jobs, services |
| Storage | Tenants, tables/collections, documents, schema, indexes, query builder, path breadcrumb, copy-on-everything |
| Network | REST/Convex/Firebase/MongoDB listeners, WebSocket subscriptions, published ports, machine API status |
| Machines | Machine lifecycle, boot image/digest, guest Nimbus version/hash, upgrade/rollback state |
| Observability | Logs, runs, events, traces, request/run correlation (jump-to-run from logs), scheduler lag |
| Settings | Server info, configuration, deploys + diff viewer, token rotation, shutdown, **Integrations** (Convex, MongoDB, Firebase, Cloud Functions, Native capability matrices) |
| Shell affordances | Sidebar with live resource counts, bottom status bar, ⌘K command palette, ⌘\\ system tenant lens, toast queue, full keyboard contract |

### Package layout

```
packages/nimbus-ui/
├── package.json              # react 19.2, @base-ui-components/react, tailwindcss 4, vite 8
├── tsconfig.json
├── vite.config.ts
├── index.html
├── convex/                   # Convex function source (from system-tenant-api-plan)
│   ├── machines.ts           # queries: list, byId
│   ├── services.ts
│   ├── bundles.ts
│   ├── functions.ts
│   ├── tables.ts
│   ├── events.ts
│   ├── runs.ts
│   ├── scheduled_jobs.ts
│   ├── cron_jobs.ts
│   ├── routes.ts
│   ├── listeners.ts
│   ├── subscriptions.ts
│   ├── ports.ts
│   ├── adapter_capabilities.ts
│   └── system.ts             # query: status
├── src/
│   ├── main.tsx              # entry, NimbusProvider + router + ThemeProvider
│   ├── routes/
│   │   ├── __root.tsx        # shell (sidebar + bottom status bar + palette + lens)
│   │   ├── overview.tsx
│   │   ├── compute/
│   │   │   ├── index.tsx     # functions, actions, HTTP routes, services
│   │   │   ├── runner.tsx    # function runner
│   │   │   └── jobs.tsx      # scheduled jobs + cron jobs
│   │   ├── storage/
│   │   │   ├── index.tsx     # tenant list
│   │   │   └── $tenant/
│   │   │       ├── index.tsx # tables
│   │   │       └── $table.tsx # document browser, schema, indexes
│   │   ├── network.tsx       # routes, listeners, subscriptions, ports
│   │   ├── machines/
│   │   │   ├── index.tsx     # machine list
│   │   │   └── $id.tsx       # machine detail + log tail
│   │   ├── observability/
│   │   │   ├── logs.tsx      # live log tail
│   │   │   └── runs/
│   │   │       ├── index.tsx
│   │   │       └── $id.tsx
│   │   └── settings/
│   │       ├── index.tsx     # server, config, token, shutdown
│   │       ├── deploys.tsx   # bundles, deploy history, diff viewer
│   │       └── integrations/ # adapter capability matrices (folded from top-level)
│   │           ├── index.tsx
│   │           ├── convex.tsx
│   │           ├── mongodb.tsx
│   │           ├── firebase.tsx
│   │           ├── cloud-functions.tsx
│   │           └── native.tsx
│   ├── components/
│   │   ├── ui/               # shadcn/ui + Base UI primitives
│   │   ├── palette/          # ⌘K command palette
│   │   ├── lens/             # ⌘\\ system tenant lens
│   │   ├── status-bar/       # bottom status bar
│   │   ├── breadcrumb/       # path-style resource breadcrumb
│   │   ├── badge/            # state dot + label
│   │   ├── diff/             # side-by-side and unified diff viewer
│   │   ├── code/             # inline + block code with shiki
│   │   └── copy-chip/        # copy-to-clipboard affordance
│   ├── hooks/                # thin wrappers over useQuery for typed access
│   └── lib/                  # auth, connection, theme, keyboard, url-state, utilities
├── dist/                     # Vite build output (gitignored, embedded by Rust)
└── .storybook/               # component documentation
```

### Server integration

1. **`/ui/*` route** — serves the embedded SPA via `rust-embed`. Falls through
   to `index.html` for client-side routing. In debug builds, `rust-embed`
   with `debug_embed = false` serves from disk — no `cargo build` on every
   UI change.

2. **Build integration** — `build-ui` Make target
   (`npm run build -w packages/nimbus-ui`). Top-level `build` and `ci`
   targets depend on it. Release-build `build.rs` asserts
   `dist/index.html` exists.

### `nimbus ui` subcommand

```
nimbus ui            # open browser to running server; error if none
nimbus ui --ensure   # start server first if none running, then open browser
```

Discovers server via `$XDG_RUNTIME_DIR/nimbus/server.json` (written by
`nimbus start` — see `docs/plans/archive/localhost-server-security-plan.md` LS1). Uses
`open::that` for cross-platform browser launch.

### Disconnected state UX

When the WebSocket connection drops:

- **Header banner** transitions: "Reconnecting..." (amber, immediate) →
  "Server unreachable" (red, after 30s) with manual "Reconnect" button.
- **Tabs show last-known data** with stale-data overlay — not empty states.
- **Mutations disabled** during disconnect — buttons grayed out with tooltip.
  No silent queueing.
- **On reconnect**: subscriptions re-established, caches refreshed, banner
  disappears. Un-acked mutations surfaced as `op.session_lost` errors.

### UI tab → query map

| Tab | Queries / streams |
| --- | --- |
| Overview | `system.status`, `machines.list`, `services.list`, `events.recent{limit:20}`, `runs.recent{limit:10}` |
| Sidebar counts | reactive counts derived from the same queries powering each section |
| Status bar | `useNimbusConnectionState`, `system.status`, active tenant from URL state |
| Command palette (⌘K) | indexed cache of all `_nimbus` resources for Navigate; declarative action registry for Run |
| System tenant lens (⌘\\) | `_nimbus` query for the underlying document(s) of the active resource |
| Machines | `machines.list` |
| Machine detail | `machines.byId`, `services.list{machineId}`, stream `logs:machine:<id>` |
| Services | `services.list` |
| Service detail | `services.byId`, stream `logs:service:<id>` |
| Functions + Jobs | `bundles.list` → `functions.list{bundleId}`, `scheduled_jobs.list`, `cron_jobs.list` |
| Function Runner | `functions.list`, REST invoke paths, `runs.byId`, `events.byCorrelationId` |
| Tenants | REST `GET /api/tenants`, `tables.list{tenantId}` |
| Data | `tables.list` → REST `GET /api/tenants/{id}/documents/{table}` (cross-tenant), REST document CRUD |
| Schema | REST `GET/PUT/DELETE /api/tenants/{id}/schema/{table}` |
| Indexes | derived from `tables.schema` initially; REST index API when implemented |
| Network | `routes.list`, `listeners.list`, `subscriptions.list`, `ports.list` |
| Observability — Logs | stream `events:all` with filter controls (URL-encoded) |
| Observability — Runs | `runs.recent` → `runs.byId`, `events.byCorrelationId` (jump-to-run from logs) |
| Settings — Server/Config | `system.status`, license endpoint |
| Settings — Deploys | `bundles.list`, `functions.list`, HTTP lifecycle/deploy endpoints |
| Settings — Integrations | `adapter_capabilities.list`, `listeners.list{adapter}` |

## Roadmap

### DU0 — Design system and information architecture

Create the root `DESIGN.md` as the implementation-ready UI design system.
It must encode the product stance, information architecture, visual language,
component rules, adapter capability UX, Convex-plugin usage boundary, and
quality gates.

**Verification:** `DESIGN.md` exists, is linked from `docs/README.md`, maps
Compute/Storage/Network/Machines/Adapters/Observability, and explicitly
states how Convex, MongoDB, Firebase, and Native adapter screens differ.

**Status:** `done`

### DU1 — Server: embedded static asset serving at `/ui/*`

Add `rust-embed` 8.x to `nimbus-server`, embed `packages/nimbus-ui/dist/`,
and replace the current minimal `/ui/*` placeholder with the built SPA while
preserving `/ui/auth`, `/ui/auth/session`, signed session cookies, and CSP.
Serve with `index.html` fallback. `debug_embed = false` for dev.

Scaffold `packages/nimbus-ui` as a minimal npm package with Vite 8.0.x and
a placeholder `index.html`. Add `build-ui` Make target. Add `build.rs`
assertion for release builds.

**Verification:** `curl localhost:PORT/ui/` returns HTML, SPA fallback works,
`make build` includes UI build step.

**Verification commands:**

- `curl -i http://localhost:8080/ui/` → 200 + HTML, expected security headers
- `curl -i http://localhost:8080/ui/some/deep/route` → same `index.html`
  (SPA fallback) with 200, **not** a 404
- `curl -i http://localhost:8080/ui/__nonexistent.js` → 404 (assets must not
  fall through to `index.html`)
- `playwright-cli open http://localhost:8080/ui/` → snapshot shows page
  renders; `playwright-cli console` is empty; `chrome-devtools-mcp`
  `list_network_requests` shows all `/ui/*` assets 200 with correct
  `Content-Type`
- Inject an inline `<script>` via `playwright-cli eval` → console reports a
  CSP violation (proves CSP is in effect)

**Status:** `done`

### DU2 — CLI: `nimbus ui` subcommand

Add `nimbus ui` and `nimbus ui --ensure`. Reads server discovery file,
opens browser via `open::that`. `--ensure` starts server if not running.
Prefer Chromium-family browsers (Chrome → Chromium → Edge) for parity
with the Phase 2 Electron shell, falling back to the OS default if none
are installed.

**Verification:** (a) `nimbus start &` + `nimbus ui` opens browser,
(b) no server → clear error, (c) `--ensure` starts then opens.

**Verification commands:**

- `nimbus start &` then `nimbus ui` exits 0; reads the discovery file
  (`$TMPDIR/nimbus/server.json` on macOS, `$XDG_RUNTIME_DIR/nimbus/server.json`
  on Linux) and hands the URL to `open::with` / `open::that`
- Without a server: `nimbus ui` exits non-zero with an actionable
  "server not running, try `--ensure`" message — assert in a Rust
  integration test
- `nimbus ui --ensure` (no server): spawns server, blocks until ready,
  then opens; second invocation is idempotent (does not double-spawn)
- After spawn, `cat $TMPDIR/nimbus/server.json | jq` (macOS) or
  `cat $XDG_RUNTIME_DIR/nimbus/server.json | jq` (Linux) shows the live
  `ServerDiscoveryRecord` shape (`{pid, address, startedAt, version,
  protocolVersions}`)
- The opened URL responds to `playwright-cli goto $URL` + `snapshot`
  showing the SPA — wires DU1 and DU2 together end-to-end

**Status:** `done`

### DU3 — UI: scaffold and shell layout

Replace placeholder with full component stack: React >=19.2.1, shadcn/ui source
components with Base UI (MUI) primitives (`shadcn init --style base`),
Tailwind >=4.3 with `@theme` OKLCH tokens, `tw-animate-css` 1.x, Zustand 5.x,
TanStack Router 1.x, Lucide, `cmdk`, `sonner`, `shiki`,
`@fontsource/jetbrains-mono`, and Biome for JS linting and formatting. Pin
exact current versions during scaffold and keep them in `package-lock.json`.

Build the operator shell layout per `DESIGN.md`:

- **Sidebar nav** with primary sections — Overview, Compute, Storage, Network,
  Machines, Observability, Settings (7 entries; Adapters fold under Settings
  → Integrations). Each entry shows a live resource count next to the label
  (e.g., `Machines · 4`). Counts come from `useQuery` against the system
  tenant.
- **Bottom status bar** (persistent, 24-28px) with connection state dot,
  server URL (monospace, click-to-copy), version + build hash, active
  tenant, inflight request count, and right-side keyboard hints
  (`⌘K palette`, `⌘\\ system tenant lens`).
- **Command palette** (`cmdk`) mounted at app root. Three modes in one
  surface: Navigate, Run, Filter. Keyboard hints next to every action.
  Persisted recent commands.
- **System tenant lens** triggered by `⌘\\` from any resource view. Renders
  the underlying `_nimbus` document(s) as syntax-highlighted JSON
  side-by-side with the operator view. Read-only. Returns focus on close.
- **Toast queue** (`sonner`) anchored bottom-right, above the status bar.
- **Theme** defaults to dark mode with `prefers-color-scheme` light fallback.
  Tokens defined in OKLCH via Tailwind v4 `@theme`. JetBrains Mono pinned
  for monospace.
- **Keyboard contract**: `⌘K` palette, `⌘\\` system tenant lens, `⌘.`
  action menu on focused resource, `ESC` closes drawers/modals/palette and
  returns focus to opener, `/` focuses inline search where present.
- **Error boundary**, **disconnected state overlay**, and **focus restoration**
  on close.

**Verification:** `npm run build` succeeds, all seven sidebar entries
navigate, sidebar resource counts update reactively, status bar shows live
connection + server URL + version, `⌘K` opens the palette and supports
Navigate/Run/Filter, `⌘\\` opens the system tenant lens and toggles back
to the operator view, dark/light theme works via OKLCH tokens, JetBrains
Mono renders for all IDs/digests/ports/durations, tabular figures applied
to numeric columns, bundle < 500 KB gzipped.

**Verification commands:**

- `playwright-cli open http://localhost:5173/` then `snapshot` — asserts
  seven sidebar entries, status bar with state dot + URL + version + ⌘K /
  ⌘\\ hints, palette trigger button
- Keyboard contract: `playwright-cli press Meta+k` → snapshot shows palette
  overlay with role `dialog`; type a query; `press Escape` → snapshot
  shows palette gone and focus returned to opener
- `playwright-cli press Meta+Backslash` → snapshot shows system tenant
  lens panel; same key toggles back; lens body is read-only (no editable
  elements in snapshot)
- `playwright-cli press Meta+Period` on a focused resource → snapshot
  shows action menu rooted on that resource
- Theme: `playwright-cli eval "matchMedia('(prefers-color-scheme: dark)')
  .matches"`; force dark and light via `emulate-media`; screenshot each;
  diff stored under `tests/visual/du3-theme/`
- Font: `playwright-cli eval` to read `getComputedStyle` of a monospace
  element and assert `font-family` contains `"JetBrains Mono"`
- Tabular figures: `eval` returns `font-variant-numeric` includes
  `tabular-nums` for every numeric column
- Bundle: `gzip -c packages/nimbus-ui/dist/assets/*.js | wc -c` < 500 KB
- axe-core via inline Playwright spec — zero critical/serious violations

**Status:** `done`

### DU4 — UI: overview tab

Overview is a dense health panel, not a hero section.

- **Top strip**: system status (uptime, version, storage backend, license
  posture). Tabular figures throughout.
- **Resource counts grid**: machines/services/tenants/tables/functions/runs
  with per-state breakdowns (`Machines · 4 Ready · 1 Starting · 1 Stopped`).
  Each panel links to the corresponding section.
- **Recent activity**: unified events feed (last 20) and recent runs
  (last 10) side-by-side. Each row shows level, source, request/run ID
  (monospace, copy-on-hover), timestamp (tabular), and message.
- **Connection**: dock connection state to the bottom status bar (spec'd in
  DU3) — do not duplicate inside the Overview surface.
- All via `useQuery` against the system tenant — no polling.

**Verification:** live data renders, machine state changes reflected in
one render cycle, events list updates in real time, sidebar resource counts
match Overview grid counts (no divergence), status bar reflects connection
state, every ID on the page is copyable.

**Verification commands:**

- `playwright-cli open http://localhost:8080/ui/` then `snapshot` — top
  strip, resource counts grid, events feed, recent runs all render with
  live data (no skeletons after first frame)
- Cross-check from the same snapshot: each sidebar count equals the
  corresponding Overview grid count — no divergence allowed
- Mutate a machine via `curl -X POST http://localhost:8080/api/machines/<id>/stop`
  → next `playwright-cli snapshot` shows the new state within one render
  cycle (no manual refresh)
- Copy chips: snapshot includes a `button "Copy"` (or `[data-copy]`) for
  every monospace ID / digest / request ID on the page
- Status bar reflects connection: kill the server, snapshot shows
  "Reconnecting" then "Server unreachable" per DU3 disconnected state
- axe-core inline Playwright spec — zero critical/serious

**Status:** `done`

### DU5 — UI: machines tab

Machine list with state badges and action buttons (start, stop, restart,
delete via HTTP endpoints). Machine detail with config, services, log tail.
Optimistic updates on lifecycle actions.

**Verification:** state transitions via optimistic update, log tail
streams without gaps, action errors render inline.

**Verification commands:**

- `playwright-cli open http://localhost:8080/ui/machines` → snapshot shows
  machine list with state badges and action buttons per row
- `playwright-cli click <ref-of-Start-button>` → snapshot taken immediately
  after click shows optimistic state change (badge updates before WS event
  arrives)
- `chrome-devtools-mcp` `list_network_requests` filtered to `/api/machines`
  confirms POST shape and headers (session cookie present)
- WS authoritative event arrives → third snapshot shows the final state
  (`Ready` / `Stopped` / etc.) — proves engine path round-trip
- Force an error (e.g., `Start` on an already-running machine) → snapshot
  shows inline error attached to that row, not a global toast
- Machine detail: `goto .../machines/<id>` → snapshot shows config +
  services + log tail; idle for 10s and re-snapshot — log tail has
  appended lines, no duplicated lines

**Status:** `done`

### DU6 — UI: services and functions tabs

- **Services:** list + detail with health snapshot and log tail.
- **Functions + Jobs:** bundle list → function list with kind/schema,
  scheduled jobs with status, cron jobs with schedule/next-run.
- **HTTP routes:** route table with method/path/handler, last request, and
  adapter/source.
- **Function inventory:** every function row shows path, kind, adapter,
  bundle/source, args schema, returns schema when known, last run status, and
  cross-links to Runs and Function Runner.

**Verification:** live data, function kind badges, scheduled job status
updates reactively, cron next-run refreshes, route table links to run/log
correlation.

**Verification commands:**

- Services: `playwright-cli open .../ui/compute`, snapshot shows service
  list with health badges; click a service row → detail panel streams logs
- Functions+Jobs: snapshot shows bundles list → function inventory with
  kind/adapter/source/argsSchema; scheduled jobs with status; cron jobs
  with schedule + next-run
- Idle for >60s and re-snapshot — cron `next-run` decremented
- HTTP routes: snapshot shows route table; click row → snapshot shows
  cross-link to the matching run in Observability
- All function paths and digests render in monospace with copy chips
  (visible in snapshot)

**Status:** `done`

### DU6.5 — UI: function runner

Interactive function execution panel:

This item is required for the first usable operator UI. Without interactive
execution, Nimbus has a monitoring dashboard rather than a control plane. The
implementation may call existing adapter-specific invoke routes where they
provide the right contract; only a generic HTTP wrapper is optional.

- Argument editor with schema-aware field types when `argsSchema` is
  available, falling back to raw JSON.
- Tenant selector for cross-tenant invocation.
- Adapter/mode labeling so Convex queries, mutations, actions, HTTP routes,
  Cloud Functions handlers, and native scheduled mutations are not blurred
  together.
- Identity controls labeled as simulated/admin-local unless a real auth
  provider is active.
- Query-type functions can auto-refresh through subscriptions where supported.
- Mutations, actions, and HTTP handlers require explicit submit.
- Result panel shows JSON output plus request/run correlation ID.
- Log/event panel filters to the current run correlation ID.
- Errors render timeout, cancellation, validation, and user-code failures with
  actionable next-step copy.

**Verification:** schema-aware args for Convex functions, raw JSON for native
or adapter functions without schemas, mutation executes and result displays,
query auto-refreshes on data change, and run correlation links to Runs.

**Verification commands:**

- `playwright-cli open .../ui/compute/runner` → snapshot shows arg editor,
  tenant selector, adapter+mode label, identity controls (labeled
  simulated/admin-local when no auth provider is configured)
- For a Convex function with `argsSchema`: snapshot shows schema-aware
  fields (string/number/boolean inputs), not a raw JSON blob
- For a native function without schema: snapshot shows raw JSON textarea
- `playwright-cli fill <args-ref> '{"name":"jack"}'` →
  `click <Submit-ref>` → snapshot shows result panel with run correlation
  ID in monospace and a copy chip
- `goto .../ui/observability/logs?correlationId=<id>` → snapshot shows the
  events filtered to that run only (no cross-run noise)
- Submit a mutation with wrong shape → snapshot shows actionable
  validation error attached to the offending field
- For a query-type function: trigger a backing mutation in another
  session; runner result panel auto-refreshes without a manual submit

**Status:** `pending`

### DU7 — UI: data browser, schema, indexes, and tenant lifecycle

Tenant list → table/collection list → document browser with reactive
`_nimbus.tables` metadata and REST cursor pagination for cross-tenant document
browsing.

- **Resource breadcrumb:** Firestore-style path navigation at the top of
  every storage view (`_nimbus › tables › machines › m_abc123`). Each
  segment is navigable; hover reveals a copy affordance for that segment.
  Chevron `›` separator (not `/`) so it does not collide with function
  paths or URLs.
- **Copy-to-clipboard chip** on every machine-readable value: tenant ID,
  table name, document ID, sha256, request ID. Hover-revealed inline,
  permanent in resource headers.
- **Tenant lifecycle:** Create tenant form, delete with resource-count
  warning, and per-tenant storage backend indicator.
- **Document browser:** Cursor pagination, filters, sort, column chooser, and
  adapter-aware value rendering for Convex/Nimbus JSON, MongoDB BSON shapes,
  and Firebase collection/document paths.
- **Document CRUD:** Insert document form, edit in drawer with JSON editor and
  schema validation preview, single delete with confirmation, and bulk select
  plus bulk delete.
- **Schema panel:** View current schema per table, create/edit via JSON
  editor, delete schema with confirmation, and show validation errors before
  submit.
- **Index panel:** List indexes per table with fields, type, and status.
  Create/drop actions use the native index REST API once implemented; until
  then, read-only index display can be derived from `tables.schema`.
- **Query builder:** show active filters/sort, make index use visible, and
  refuse unsafe unbounded scans.
- Cursor invalidation on schema changes or table deletion shows a named stale
  resource state instead of an empty table.

**Verification:** pagination works, document CRUD round-trips, schema
create/edit/delete works, index display works, index create/drop works once the
REST endpoints exist, 1000+ row tables remain browseable, and adapter
capability caveats render inline.

**Verification commands:**

- Seed a table with 1000+ rows via the Rust HTTP API or a test fixture
- `playwright-cli open .../ui/storage/<tenant>/<table>` → snapshot renders
  without crash; `playwright-cli console` empty (no virtualization errors)
- Pagination: `click <Next-Page ref>` → snapshot shows different IDs from
  the first page (proves cursor pagination, not in-memory slice)
- Insert: `click Insert` → `fill <json-ref> '{...}'` → submit → snapshot
  shows new row in the listing
- Edit: `click <row-ref>` → drawer opens with JSON editor → mutate →
  submit → snapshot reflects change
- Bulk delete: select multiple → confirm → snapshot count decreases by
  the selected amount
- Schema panel: paste invalid JSON → snapshot shows validation error
  inline (submit button disabled)
- Breadcrumb: snapshot shows `_nimbus › tables › <table>` with chevron
  separator; `click <_nimbus ref>` → snapshot shows tenant list (root)
- Copy chips: every tenant ID, table name, doc ID, sha256 in the snapshot
  has an adjacent copy affordance
- Adapter capability caveats: snapshot shows caveat text inline next to
  the affected control (not tooltip-only)
- axe-core inline Playwright spec — zero critical/serious

**Status:** `pending`

### DU8 — UI: logs and runs tabs

- **Logs:** live event stream with level/category/source filters. Filter
  state encoded in the URL (deep-linkable). Live updates preserve scroll
  position; follow-mode is an explicit toggle. Pause-on-error optional.
- **Jump to run:** every log entry with a `correlationId` exposes a "Jump
  to run" affordance (keyboard `⏎` from the focused row, click on the
  badge, or "Open run" in the right-click context menu). Opens the
  corresponding run detail with the events panel filtered to that
  correlation ID.
- **Runs:** recent runs → run detail with trace viewer (timing waterfall),
  request/run ID with copy chip, function path (monospace), correlated
  events panel.
- **Adapter honesty:** If `_nimbus.runs` still only records Convex invocation
  paths, the UI must label the Runs view as Convex/runtime invocation history
  and cross-link to Events for other adapters. Do not claim cross-adapter
  Observability parity until native HTTP, scheduler, MongoDB, Firebase, and
  Cloud Functions traffic also records run entries.

**Verification:** filters apply without losing position, URL reflects active
filters, trace viewer shows timing, 100+ events/second without UI lag,
jump-to-run works from log entries via click + keyboard + context menu, and
any incomplete adapter coverage is explicitly labeled in the UI.

**Verification commands:**

- `playwright-cli open .../ui/observability/logs` → snapshot shows filter
  controls (level, category, source) and the live stream
- Apply a filter via `fill` / `click` → URL updates to encoded filter
  state; `goto` that URL in a new session → snapshot shows the same
  filtered view (deep-linkable)
- Push 100+ events/sec from a test harness (or `nimbus`-generated load)
  → `chrome-devtools-mcp` `performance_start_trace` → analyze insight
  shows FPS ≥ 50, no long tasks > 50 ms in steady state
- Scroll position: focus a row, push 50 new events, snapshot — the
  focused row is still visible (follow-mode off by default)
- Jump-to-run via click: `click <correlation-badge-ref>` → snapshot
  shows run detail with events filtered to that ID
- Jump-to-run via keyboard: focus row, `press Enter` → same effect
- Run detail: snapshot shows trace timing waterfall, request/run ID in
  monospace with copy chip, function path in monospace, correlated events
  panel populated
- Adapter honesty: if `_nimbus.runs` covers only Convex paths, snapshot
  shows the Runs view label includes "Convex/runtime invocation history"
  and a cross-link to Events; no claim of cross-adapter parity
- axe-core inline Playwright spec — zero critical/serious

**Status:** `pending`

### DU9 — UI: settings, configuration, integrations, and deploy management

- **Tenant header strip:** at the top of Settings, a thin strip showing the
  active tenant, storage backend, license posture, and primary quota/usage
  signal (modeled on Firebase's project header). Click opens tenant
  switcher.
- **Server info:** version, uptime, address, storage backend, active origin,
  and health.
- **Configuration:** read-only display of runtime limits, license status and
  usage from the existing license endpoint, auth provider config, adapter
  enablement, and storage topology.
- **Integrations (Adapters):** capability matrix for Convex, MongoDB,
  Firebase, Cloud Functions, and Native — `supported`, `supported with
  caveats`, `not claimed`. Caveats render inline next to the affected
  feature, not behind tooltips alone.
- **Deploys:** current active bundle with sha256, source, timestamp, function
  inventory, deploy history from `_nimbus.bundles`, and deploy trigger when
  the selected artifact can be passed to the local-admin deploy endpoint.
  Comparing two bundles opens the diff viewer (function inventory,
  argsSchema, returnsSchema deltas).
- **Token rotation:** button with confirmation, re-auth after rotation.
- **Shutdown:** button with confirmation, disconnect state after accepted
  shutdown.

**Verification:** tenant header strip reflects active tenant and license,
config displays match server state, integrations matrix renders all five
adapters with caveat inline rendering, deploy history shows correct function
counts, diffing two bundles renders side-by-side, token rotation triggers
re-auth, shutdown shows disconnect state.

**Verification commands:**

- `playwright-cli open .../ui/settings` → snapshot shows tenant header
  strip (tenant, storage backend, license, primary usage signal), server
  info section, configuration section
- Integrations: snapshot shows all five adapters (Convex, MongoDB,
  Firebase, Cloud Functions, Native) with `supported` / `supported with
  caveats` / `not claimed`; caveat text renders inline next to the
  affected feature, not tooltip-only
- Deploys: snapshot shows active bundle with sha256, source, timestamp,
  function inventory, and deploy history list
- Diff viewer: select two bundles → `click Compare` → snapshot shows
  side-by-side diff with function inventory, argsSchema, returnsSchema
  deltas highlighted
- Token rotation: `click Rotate` → confirm dialog → snapshot shows
  re-auth flow; `chrome-devtools-mcp` `list_network_requests` confirms
  subsequent requests carry the new token
- Shutdown: `click Shutdown` → confirm → snapshot shows disconnect
  state per DU3; `playwright-cli console` shows WS close event; no
  auto-reconnect attempt
- `playwright-cli state-save .auth/local.json` after rotation, then
  reload via `state-load` to confirm the new session round-trips

**Status:** `pending`

### DU10 — Testing: unit, integration, E2E, and Storybook

Testing pyramid:

| Layer | Tool | What it tests |
| --- | --- | --- |
| Unit | Vitest 4.1.x + JSDOM | Hooks, utilities, pure logic |
| Component | Vitest + RTL 16.3.x + `@axe-core/react` | Rendering, interaction, a11y |
| Integration | Vitest + MSW 2.x + mocked WebSocket | useQuery/useMutation against mock; MSW for HTTP API mocking |
| Visual regression | Chromatic on Storybook 9.x | OKLCH token regressions, badge state rendering, dark/light parity, density drift |
| E2E | Playwright 1.60.x | Full flows against `nimbus start` |

Co-located `.spec.tsx` beside every `.tsx` (Podman Desktop pattern).
Storybook 9.x for all components + a curated state-rendering matrix:
- All badge states (Ready/Running/Starting/Draining/Queued/NotReady/Stopped/Failed/Stale/Unknown) in both themes
- Empty states at all three sizes (row/panel/whole-tab)
- Command palette with each mode (Navigate/Run/Filter)
- System tenant lens open + closed
- Bottom status bar (Connected/Reconnecting/Offline)
- Diff viewer with sample schema delta
- Tables at 0, 1, 10, 100, 1000 rows
- Logs at 0, 100, 100/sec live

**React Compiler evaluation:** React 19 ships with an opt-in compiler that
auto-memoizes components and hooks. Do not enable during DU1–DU3 scaffold.
Evaluate during DU10 as a performance optimization for virtualized log/event
tables and the data browser — measure bundle size and render performance
before and after enabling.

**Verification:** `npm run test` green, `npm run storybook` launches,
co-located specs for all files, axe-core zero critical/serious, dark mode
correct in all stories.

**Verification commands:**

- `npm run test -w packages/nimbus-ui` → Vitest + RTL specs green
- `npx playwright test` → real E2E specs in `tests/e2e/*.spec.ts` green
  against `nimbus start`
- `npm run storybook -w packages/nimbus-ui` → launches; each story in
  the curated state matrix renders without console errors
- Chromatic publish step → diff shows zero unexpected visual regressions
- axe-core inside the story matrix — zero critical/serious in every
  story (light and dark)
- At this stage `playwright-cli` is no longer the iteration driver — it
  has been superseded by committed `*.spec.ts` files. The Skill remains
  available for ad-hoc exploration during ongoing maintenance.

**Status:** `pending`

## Phase 2: Native Desktop Shell (future plan scope)

A separate plan will be authored when Phase 1 is stable and users request
native-app behavior (dock icon, tray, auto-update, deep links).

**Activation gate:** all Phase 1 items (DU1–DU10) shipped and stable.

### Shell Choice

**Decision: Electron.** The Phase 2 native desktop shell uses Electron.

Electron is the enterprise-grade standard for desktop applications that wrap a
web UI. It ships a bundled Chromium renderer, which guarantees identical
rendering of Nimbus's data-dense operator console (dense tables, virtualized
100+ event/sec log streams, JSON editors, split panes) across macOS, Windows,
and Linux. It has mature, production-proven packaging, code signing,
notarization, and auto-update pipelines used by VS Code, Slack, Discord,
MongoDB Compass, Postman, Podman Desktop, and 1Password. The security model
(context isolation, process sandboxing, Electron Fuses, CSP) is well-documented
and auditable.

Binary size (~150 MB) is the tradeoff. This is acceptable for an enterprise
operator console — operators already run Docker (~1 GB), VS Code (~300 MB), and
browsers. The SPA at `/ui/*` remains the zero-cost browser-based option via
`nimbus ui` for operators who do not need tray, dock, auto-start, or OS menu
integration.

**Why not Tauri 2:** Tauri produces a smaller binary (~5 MB) and uses Rust for
the shell process, but it relies on system WebViews — three different rendering
engines across platforms (WKWebView on macOS, WebView2 on Windows, WebKitGTK on
Linux). WebKitGTK on Linux has disqualifying problems for a data-dense UI: font
weight renders ~100 units heavier than specified (open upstream bug, no fix
timeline), performance regressions across WebKitGTK versions, blank page
regressions after OS upgrades, and WebKitGTK version fragmentation across
distributions. The Tauri team is working on a CEF (Chromium) backend for Linux,
but it is not stable. Tauri may be reconsidered if CEF matures — see the
evaluation gate below.

**Why not zero-native:** Pre-release, macOS-first, Windows in progress, Linux
Chromium not wired. Not a shipping candidate.

The shell wraps `localhost:PORT/ui` and manages `nimbus start` lifecycle.
All business logic stays in the Rust server.

### Security configuration (Electron 41.x)

```javascript
new BrowserWindow({
  show: false,
  webPreferences: {
    preload: path.join(__dirname, 'preload.js'),
    contextIsolation: true,    // default since Electron 12
    nodeIntegration: false,    // default since Electron 5
    sandbox: true,             // default since Electron 20
    webSecurity: true,         // do NOT disable
  },
});
```

Hardening: Electron Fuses (`RunAsNode`, `EnableNodeOptionsEnvironmentVariable`,
`EnableNodeCliInspectArguments`, `EnableCookieEncryption`,
`EnableEmbeddedAsarIntegrityValidation`, `OnlyLoadAppFromAsar`),
`setPermissionRequestHandler` (deny all except clipboard),
`will-navigate` restriction, `setWindowOpenHandler` deny,
`event.senderFrame.url` validation on all IPC handlers.

### IPC architecture

**20-40 channels** (Podman Desktop has 297+; we're thinner because all
business logic flows over WebSocket). Preload target: **<500 lines**. If
IPC exceeds 50 channels, adopt `dts-for-context-bridge` codegen.

IPC handles window chrome, tray, auto-update, and server lifecycle only —
never queries, mutations, or data.

### Process model

- Main process: window management, tray, menus, auto-updater
  (`electron-updater` 6.8.x), server lifecycle
- Renderer: sandboxed, `loadURL('http://localhost:PORT/ui/')`, same SPA
- Server: `child_process.spawn` (not `utilityProcess`), discovered via
  `$XDG_RUNTIME_DIR/nimbus/server.json`
- macOS: `activate` → re-show window; `window-all-closed` → no-op
- Windows/Linux: `window-all-closed` → quit

### Packaging

Build tool: **electron-builder** 26.8.x.

| Platform | Format | Architectures | Signing | Auto-update |
| --- | --- | --- | --- | --- |
| macOS | DMG + ZIP | Universal (x64+arm64) | `notarytool` via `@electron/notarize` | electron-updater |
| Windows | NSIS | x64 + arm64 | Azure Trusted Signing or EV HSM | electron-updater (delta) |
| Linux | AppImage + deb + rpm | x64 + arm64 | — | electron-updater |

Linux: XWayland default (`--ozone-platform-hint=auto` opt-in), tray
optional (`Tray.isSupported()`), `--disable-gpu` fallback documented.

### Tauri 2 reconsideration gate

Electron is the committed shell. Before reconsidering Tauri 2, all of the
following must be proved from a real branch:

- identical rendering of dense tables, virtualized log streams (100+
  events/sec), JSON editors, and split-pane layouts across macOS (WKWebView),
  Windows (WebView2), and Ubuntu 24.04 LTS (stock WebKitGTK)
- font weight, spacing, and color fidelity match the browser baseline on all
  three platforms — WebKitGTK's +100 weight rendering bug must not affect the
  operator console
- auto-updater completes a signed update on macOS and Windows without
  `InvalidSignature` errors or network blocking
- packaged app starts or discovers a released `nimbus` binary and loads
  `http://localhost:<port>/ui/`
- E2E proves session bootstrap, reconnect banner, shutdown behavior, and
  accessibility in the packaged shell on all three platforms

### zero-native reconsideration gate

Before reconsidering zero-native for `nimbus/desktop`, prove all of the
following from a real branch:

- packaged macOS app starts or discovers a released `nimbus` binary and loads
  `http://localhost:<port>/ui/`
- exact-origin navigation policy, no broad bridge permissions, and no native
  command capable of bypassing local server access auth
- signed/notarized macOS package, documented update path, and strict
  `zero-native doctor --strict` equivalent in CI
- Windows shell support, Linux shell support, and Chromium/CEF rendering
  parity are available without relying on roadmap-only support
- E2E proves session bootstrap, reconnect banner, shutdown behavior, and
  accessibility in the packaged shell

### Repo structure

Separate repo: `nimbus/desktop`.

```
desktop/
├── package.json              # electron 41.x, electron-builder 26.x
├── electron-builder.yml
├── src/
│   ├── main/                 # lifecycle, server, ipc, menu, tray, updater, security
│   ├── preload/index.ts      # contextBridge — <500 lines
│   └── shared/ipc-types.ts
├── scripts/                  # notarize.js, sign-windows.js
├── buildResources/           # icons
└── .github/workflows/release.yml
```

## Implementation References

| Task | Reference file | What to study |
| --- | --- | --- |
| Electron security | `~/src/github.com/podman-desktop/podman-desktop/packages/main/src/security-restrictions.ts` | Permission handler, navigation restriction |
| Electron Fuses | `~/src/github.com/podman-desktop/podman-desktop/.electron-builder.config.cjs` (line 62) | Build-time Fuse config |
| IPC patterns | `~/src/github.com/podman-desktop/podman-desktop/packages/preload/src/index.ts` | Cautionary — 2,724 lines |
| Co-located tests | `~/src/github.com/podman-desktop/podman-desktop/packages/main/src/plugin/` | `.spec.ts` beside every `.ts` |
| Packaging | `~/src/github.com/podman-desktop/podman-desktop/.electron-builder.config.cjs` | DMG/NSIS/Flatpak with notarization |
| Service Hub | `~/src/github.com/janhq/jan/web-app/src/services/index.ts` | Platform abstraction pattern |
| React + headless primitives | `~/src/github.com/janhq/jan/web-app/src/components/` | Component composition patterns (Jan uses Radix; Nimbus uses Base UI via shadcn) |
| TanStack Router | `~/src/github.com/janhq/jan/web-app/src/routes/` | File-based routing |
| Zustand stores | `~/src/github.com/janhq/jan/web-app/src/stores/` | Minimal state management |

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-18 | Plan authored | — | Architecture designed from Opus 4.7 session; reference implementations researched |
| 2026-04-18 | Deep research audit | — | Cloned podman-desktop and jan repos; audited Electron 41, IPC, packaging, testing |
| 2026-04-18 | Adversarial review | — | 20 findings applied (F01-F20): Convex app architecture, XDG paths, dark mode, a11y, bundle size targets, etc. |
| 2026-04-18 | Plan decomposition | — | Extracted 3 prerequisite plans: websocket-protocol-plan.md (WP1-WP4), localhost-server-security-plan.md (LS1-LS5), system-tenant-api-plan.md (ST1-ST4). UI plan scoped to React SPA only (DU1-DU10). |
| 2026-05-15 | Readiness and zero-native review | — | Confirmed WP/LS prerequisites were complete and current code already had minimal `/ui` auth/CSP bootstrap; at the time of this review, ST1-ST4 were still unfinished for real tabs. Revalidated React 19.2, Vite 8, Tailwind v4/shadcn as the right Phase 1 stack. Reviewed zero-native v0.2.0 and docs; keep as a tracked proof lane, not the default enterprise desktop shell until Windows/Linux Chromium/package/update parity matures. Native shell repo should be `nimbus/desktop`, created only after Phase 1 is stable. |
| 2026-05-15 | Design system pass | done | Added root `DESIGN.md` using the plain-text design-system pattern. Reframed Phase 1 as a Nimbus operational console across Compute, Storage, Network, Machines, Adapters, Observability, and Settings. Clarified Convex plugin usage for system-tenant functions without making Convex the whole visual/product model. |
| 2026-05-15 | Non-UI prerequisite checkpoint | passed | Focused `_nimbus` system-tenant, machine lifecycle, and headless React hook proof lanes are implemented and green. The CI-shaped runtime lane passed with Node-compat skipped, and the workspace fallback lane passed outside the Codex sandbox after sandbox-only Unix socket and `ps` denials. `cargo fmt --all --check`, `make clippy`, `make deny`, npm build/test, and `git diff --check` are clean. Raw `make test` remains a runtime-owned Node-compat evidence signal, not a DU1 prerequisite gate. |
| 2026-05-15 | External UI coherence review applied | done | Folded the Claude product-coherence review into `DESIGN.md`, this plan, and the system-tenant plan. Phase 1 now explicitly includes Function Runner, tenant lifecycle, document CRUD, schema/index panels, deploy/settings management, and Cloud Functions as an adapter surface. System-tenant follow-up surfaces now track `events.byCorrelationId`, index APIs, optional function-runner wrapper APIs, and broader cross-adapter run recording. |
| 2026-05-15 | Tech stack review applied | done | Switched primitives from Radix to Base UI (MUI) — Radix in low-maintenance, co-creator recommends against it, Base UI has full shadcn/ui coverage and active MUI backing. Version pins: React >=19.2.1 (security), Tailwind >=4.3, Playwright 1.60.x, tw-animate-css 1.x. Added Biome linter to DU3, React Compiler evaluation + MSW + Storybook 9.x to DU10. |
| 2026-05-15 | Electron shell decision committed | done | Committed to Electron as the Phase 2 native desktop shell. Enterprise-grade: bundled Chromium guarantees consistent rendering of data-dense operator console across macOS/Windows/Linux, mature packaging/signing/updater pipeline proven at scale (VS Code, Slack, Discord, Podman Desktop). Tauri 2 demoted to reconsideration gate only — WebKitGTK font weight, performance, and version fragmentation issues on Linux disqualify it for data-dense UIs until CEF backend matures. zero-native remains pre-release. Updated DESIGN.md, phasing diagram, control plan rules, readiness decision, and all shell references to be definitive rather than evaluative. |
| 2026-05-15 | Design review applied (industrial precision) | done | Used frontend-design skill to benchmark DESIGN.md against Convex Dashboard, Firebase Console, Docker Desktop, and Podman Desktop. Committed to "industrial precision" aesthetic stance (Linear + GitHub CLI lineage). Tier 1 craft: OKLCH palette with cool neutrals, cut blue accent to link-only, JetBrains Mono pin, tabular-nums hard requirement, semantic state tokens (Starting/Draining/Queued/Running/Stale). Tier 2 patterns: command palette (cmdk), bottom status bar, resource breadcrumb, copy-to-clipboard chips, diff viewer, toast queue (sonner), three-tier empty states, code blocks (shiki), keyboard hints, IA collapse 8→7 (Adapters folded into Settings → Integrations), URL-state-as-truth interaction rules. Tier 3 signature: system tenant lens (⌘\\) — a flip-to-raw-`_nimbus`-JSON affordance unique to Nimbus, no other console can do this. DU3 expanded with shell affordances; DU4/DU7/DU8/DU9 expanded with specific patterns; DU10 adds Chromatic visual regression on a curated state matrix. |
| 2026-05-15 | Verification tooling installed | done | Researched browser-driving options for Claude Code: chose Microsoft's `@playwright/cli` (Skill-based, Bash-driven) as the primary driver after rejecting `@playwright/mcp` on ~4× token cost (snapshots stream through context vs. write to disk as ref-based YAML). Installed `@playwright/cli` 0.1.13 globally and as repo devDependency; scaffolded `.claude/skills/playwright-cli/` (SKILL.md + 10 reference docs) via `playwright-cli install --skills`. Added `chrome-devtools-mcp` at user scope and via project `.mcp.json` for perf/CDP work. Verified the loop end-to-end against `demos/convex/html`: snapshot YAML with stable element refs, console log capturing all errors with source paths and line numbers, screenshot to disk. Added `.playwright-cli/` to `.gitignore`. Replaced the stale "Playwright MCP" verification mapping with concrete per-DU `Verification commands:` blocks driving `playwright-cli` (open/snapshot/click/fill/press/console/network/screenshot/state-save) and `chrome-devtools-mcp` (list_network_requests/list_console_messages/performance_start_trace). axe-core runs via `@axe-core/playwright` inside inline specs rather than a separate MCP. Added new `## Verification Tooling` section between Verification Contract and Architecture documenting tooling, dev URLs (Vite 5173 vs nimbus-server 8080/ui/), snapshot discipline, and artifact policy. |
| 2026-05-15 | DU1 — Embed `/ui/*` SPA | done | Scaffolded `packages/nimbus-ui/` (Vite 8.0.13 + React 19.2.6 + TS 6.0.3) and wired `make build-ui` into `make build` / `make release`. Added `rust-embed` 8.11.0 with `interpolate-folder-path` to `nimbus-server`, replaced the `/ui/*` placeholder with embedded asset serving, kept SPA fallback for route-shaped paths and 404 for asset-shaped paths so missing JS/CSS never silently return `index.html`. Preserved `/ui/auth`, `/ui/auth/session`, signed session cookies, and the full CSP middleware. `build.rs` now errors release builds when `packages/nimbus-ui/dist/index.html` is missing and stubs it in debug so the workspace still builds standalone. Verification: `cargo fmt --all --check` clean, `cargo clippy -p nimbus-server --all-targets -- -D warnings` clean, `cargo test -p nimbus-server --lib tests::local_ui` 7/7 passed (added `ui_shell_serves_index_html_for_deep_routes_with_session_cookie`, `ui_root_response_carries_expected_csp`, `ui_asset_shaped_request_for_missing_file_returns_not_found`). Live browser proof against `target/debug/nimbus start --port 8080`: `playwright-cli open /ui/auth` → token-form snapshot; fill + submit → 200 session cookie issued; `playwright-cli goto /ui/` → snapshot renders `heading "Nimbus UI"`, console clean (0 errors, 0 warnings); `playwright-cli requests --static` shows `/ui/` (text/html; charset=utf-8, 321 B) and `/ui/assets/index-BckDJ3og.js` (application/javascript; charset=utf-8, 190 857 B) both 200 and both stamped with the full CSP header. CSP enforced live: injecting an inline `<script>` via `playwright-cli eval` does not execute (`executed: false`) and the page console captures `"Executing inline script violates the following Content Security Policy directive 'script-src 'self''"`. curl confirms SPA fallback (`/ui/machines/abc/services` returns the identical 321-byte `index.html`) and asset 404 (`/ui/assets/nope.js` → 404, no SPA fallthrough). Bundle: 190.85 KB raw / 59.94 KB gzip (well under the 450 KB pause / 500 KB cap). |
| 2026-05-15 | DU2 — `nimbus ui` subcommand | done | Added `nimbus ui` and `nimbus ui --ensure` (`crates/nimbus-bin/src/ui.rs`, wired through `main.rs`, `open = "5.3"` dependency). Reads `LocalServerPaths::resolve_for_current_platform()` + `read_live_server_discovery`, errors actionably when no live server is present ("Nimbus server is not running. Start one with `nimbus start` ... or rerun this command with `nimbus ui --ensure` ..."), and builds the URL via the shared `crate::local_server_client::normalize_loopback_connect_address` helper so wildcard binds become `http://127.0.0.1:<port>/ui/`. `--ensure` spawns a detached `nimbus start` (Unix `setsid` / Windows `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`), polls `read_live_server_discovery` + `GET /ui/auth` every 200 ms up to 60 s, then opens the browser; if a live server is already discoverable, `--ensure` reuses it (no double-spawn). Chromium-family preference: tries Google Chrome → Chromium → Microsoft Edge (per-platform app names) before falling back to `open::that`; prints `Opening Nimbus UI in Google Chrome at <url>` when a Chromium browser was found and `Opening Nimbus UI at <url>` otherwise — keeps the operator console aligned with the benchmark consoles (Convex/Firebase/Docker/Podman) and the Phase 2 Electron renderer. Verification: `cargo fmt --all --check` clean, `cargo clippy -p nimbus-bin --all-targets -- -D warnings` clean, `cargo test -p nimbus-bin --bin nimbus` 444/444 passed including 3 new `ui::tests::*` (`ui_command_without_running_server_returns_actionable_error` asserts both `nimbus start` and `--ensure` appear in the error string; `ui_command_resolves_live_discovery_record` spins a real `serve_with_options` + `ServerDiscoveryLease` and asserts the resolved URL starts with `http://127.0.0.1:` and ends with `/ui/`; `build_ui_url_normalizes_wildcard_address` confirms `0.0.0.0:8080` → `http://127.0.0.1:8080/ui/`). Live end-to-end on macOS against `target/debug/nimbus start --port 8082`: (a) `nimbus ui` with running server prints `Opening Nimbus UI in Google Chrome at http://127.0.0.1:8082/ui/` and exits 0; `playwright-cli goto http://127.0.0.1:8082/ui/` snapshot shows `heading "Nimbus UI"` proving DU1+DU2 chain; (b) with no server, `nimbus ui` exits 1 with the actionable `--ensure` message; (c) `nimbus ui --ensure` with no server spawns a detached child (`pid=55800`, `127.0.0.1:8080`), polls until ready, and opens the browser; a second `nimbus ui --ensure` reuses the live server (still only one `nimbus start` process); kill-then-rerun proves the discovery file is treated as stale via `pid_is_live`. CLI surface: `nimbus ui --help` renders the help template with `--ensure` flag + examples; `nimbus --help` lists `ui` between `token` and `machine`. |
| 2026-05-15 | DU3 — Scaffold + shell layout | done | Built the operator shell on React 19.2.6 + Vite 8 + TypeScript 6 + Tailwind v4.3 (`@theme` OKLCH tokens, `tw-animate-css`), TanStack Router 1.169 with file-based routing + autoCodeSplitting (programmatic Generator wired via `scripts/generate-routes.mjs`, schema parsed through `configSchema.parse` with explicit `tmpDir`), Zustand 5 store (focus-opener captured + `queueMicrotask` restore on close), cmdk command palette with Navigate/Run/Filter modes + localStorage recents, `sonner` toast queue, `@base-ui/react` 1.4.1 primitives (replacing deprecated `@base-ui-components/react`), `@fontsource/jetbrains-mono` 5.2.8 (400/500/600), Biome 2.4.15 lint+format. Shell pieces: `Sidebar` with 7 entries (Overview/Compute/Storage/Network/Machines/Observability/Settings) + per-entry `NavCount` driven by `useQuery` against the `_nimbus` system tenant; `StatusBar` (24-28px) with connection state derived from `useNimbusConnectionState`, click-to-copy chips (server URL / version / tenant) via `navigator.clipboard` + sonner confirmation, inflight request count with tabular figures, right-side `⌘K palette` / `⌘\` lens kbd hints; `CommandPalette` mounted at root with three modes + RECENT_KEY; `SystemTenantLens` as a fixed right-anchored aside (min(560px,50vw)) with stable hook order across all 7 surfaces; `KeyboardContract` window listener (⌘K, ⌘\\, ⌘., ESC priority chain palette > lens > actionMenu, `/` focuses `[data-inline-search]`); `AppErrorBoundary`, `DisconnectedOverlay`, theme bootstrap in `index.html` reading `localStorage['nimbus-ui:theme']` + matchMedia fallback to prevent FOUC, `data-theme="dark"` default. Verification: `tsc -p tsconfig.json --noEmit` clean; `cargo fmt --all --check` + `cargo clippy -p nimbus-server -- -D warnings` clean; `cargo build -p nimbus-bin` refreshed embedded `/ui/*` assets; `vite build` → 420.93 KB JS + 53.67 KB CSS = 152.7 KB gzipped (well under both the 450 KB pause threshold and the 500 KB cap). Live browser proof on Vite dev server (`http://localhost:5173`) and on the embedded build via `target/debug/nimbus start --port 8087` after POST `/ui/auth/session`: `chrome-devtools` snapshot shows `navigation "Primary"` with all 7 sidebar entries; `⌘K` opens `dialog "Command palette"` with Navigate/Run/Filter mode toggles + ↑↓/⏎/⎋ kbd hints; ESC closes the palette and returns focus to opener; `⌘\\` opens `region "System tenant lens"` rooted at `_nimbus` / `system.status` with read-only footer; ESC closes the lens. Theme proof: `document.documentElement.dataset.theme` toggles between `dark` (body bg `oklch(0.15 0.015 240)`) and `light` (body bg `oklch(0.98 0.005 240)`); screenshots captured at `.playwright-cli/du3-embedded-{dark,light}.png`. Font proof: `getComputedStyle` on a `nav .font-mono` element returns `font-family: "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`; tabular figures confirmed via `font-variant-numeric: tabular-nums` on every `[data-testid$="-count-loading"]` and `[data-testid$="-count"]` element. |
| 2026-05-15 | DU5 — Machines tab | done | Implemented `packages/nimbus-ui/src/routes/machines.tsx` as the operator machines surface — table view + slide-in detail panel — driven entirely by live `useQuery` subscriptions against the `_nimbus` system tenant for reads (`api.machines.list`, `api.services.byMachine`, `api.events.recent` with client-side `data.machineId` filter to match the server-side `source: "machine"` singleton event scheme) and HTTP `/api/machines/{name}/{create|start|stop|restart}` + `DELETE /api/machines/{name}` for lifecycle writes — never bypassing Service. Table renders state badge (`StateChip` via OKLCH semantic tokens) + provider/kind/CPU/MEM/DISK with `tabular-nums` + relative `UPDATED` + per-row `START`/`STOP`/`RESTART`/`DELETE` (with confirm dialog) actions. Detail panel: IDENTIFIERS section (state chip, provider, `_id` `CopyChip` with `data-testid="machines-detail-copy-id-<name>"`, created/updated), RESOURCES (CPUs/MEM/DISK), SERVICES list with state chip + machine link, RECENT EVENTS feed (last 100 events, client-filtered to this machine via `data.machineId`). Mid-DU fixes: (1) `packages/nimbus/src/browser-utils.ts` — added `pendingSent?: boolean` to `SubscriptionEntry` to track whether a queued subscribe has been written to the wire; (2) `packages/nimbus/src/browser.ts` — `onUpdate()` now calls `flushPendingSubscriptions()` directly when the socket is already connected (previously the call short-circuited into `scheduleReconnect` because `isWebSocketConnected` was true but flush was never triggered; the fix unblocks subscriptions registered after the socket open), `queueSubscription` resets `pendingSent = false`, `flushPendingSubscriptions` skips entries already marked `pendingSent` and marks them after `socket.send` — dedupes against duplicate `subscribe_named` frames being written on every flush iteration. Tri-state theme refactor (per operator review): `packages/nimbus-ui/src/store/ui-store.ts` separated `ThemeMode = "light" | "dark" | "system"` (user preference, persisted under `nimbus-ui:theme`) from `Theme = "light" | "dark"` (resolved value applied to `data-theme`), wired a module-level `prefers-color-scheme: dark` matchMedia listener that live-updates the resolved theme when `mode === "system"`, default mode `"system"` so the console matches OS preference at boot. Exposes `setThemeMode(mode)` and `cycleThemeMode()` (Light → Dark → System); dropped legacy `toggleTheme`/`setTheme` per pre-launch no-compat-shim rule. Color-contrast tightening to pass axe-core AA on both themes: dark `--color-stale` 60% → 75% (fixes STOPPED chip 3.5:1 → 7.8:1 against dark surface); light base `--color-danger` 58% → 50% (fixes DELETE button text 4.24:1 → 5.0:1 against light surface). axe-core 4.10 (553 KB) is now served same-origin from `dist/assets/axe.min.js` via rust_embed so injection bypasses the `script-src 'self'` CSP cleanly; copy step is part of the DU5 build flow. Verification: `npm run typecheck` clean; `vite build` → 421.73 KB JS + 57.15 KB CSS = 131.49 KB JS gzipped + 26.77 KB CSS gzipped (well under the 450 KB pause threshold); `cargo build --bin nimbus` clean; `cargo fmt --all --check` clean. Live end-to-end proof on `target/debug/nimbus start --port 8088` (HOME=/tmp/nimbus-du5-run/fake-home) after `POST /ui/auth/session` via in-browser fetch (HttpOnly cookie path): five seeded machines (`test-vm`/`web-1`/`web-2`/`web-3`/`worker-1`) all rendered with STOPPED state chips, real timestamps, working START/DELETE controls; click into `web-3` opens detail panel showing IDENTIFIERS + RESOURCES + `SERVICES (0)` + `RECENT EVENTS` populated with `INFO machine \`web-3\` create completed with state stopped 13m ago` — proves both the WS subscription fix and the events filter alignment with the server-side `source: "machine"` + `data.machineId` schema. axe-core run via same-origin script load against the open detail panel in both themes: **dark — 0 violations, 26 passes, 5 incomplete (decorative `aria-hidden` `·` separators flagged "Element content is too short to determine"); light — 0 violations, 26 passes, 5 incomplete (same separators).** No critical or serious violations in either theme. Screenshots captured at `.playwright-cli/du5-embedded-{dark,light}.png`. |
| 2026-05-15 | DU4 — Overview tab | done | Implemented `packages/nimbus-ui/src/routes/index.tsx` as the Overview page driven entirely by live `useQuery` subscriptions against the `_nimbus` system tenant. Composition: `TopStrip` (8 cells: Server/Version/Uptime/Storage/License/Started/Updated/Tenant — values from `api.system.status` with `RelativeTime`/`Uptime` ticking once a minute), `ResourceCountsGrid` (6 `CountPanel` tiles for Machines/Services/Tenants/Tables/Functions/Recent runs, each subscribed to its respective system-tenant query and linked to the corresponding section route, with `groupCount` deriving per-state breakdowns), `EventsFeed` (last 20 events; rows show level, source, correlationId, createdAt), `RecentRuns` (last 10 runs; rows show status, functionPath, _id, durationMs, startedAt). Extracted shared `CopyChip` from status-bar into `components/copy-chip.tsx` with `hideUntilHover` + `children` props; added `components/state-chip.tsx` mapping state strings to OKLCH semantic tokens (resolver normalizes input via `toLowerCase` before narrowing to `StateKind`, avoiding the TS cast trap); added `components/time.tsx` with `RelativeTime`/`Uptime` + `useNow(intervalMs)` ticker; added `lib/format.ts` with `formatRelativeTime`/`formatAbsoluteTime`/`formatUptime`/`formatDuration`/`shortId`. Switched `lib/nimbus-client.ts` to instantiate `NimbusReactClient` against `${origin}/convex/_nimbus` with `skipDeploymentUrlCheck: true` so HTTP queries hit `/convex/_nimbus/query` and the WebSocket hits `/convex/_nimbus/ws` (single tenant-bound origin — no client-side header surgery needed). Bundled two shared-package fixes required for live WS to connect: (1) `packages/nimbus/src/internal/shared.ts` — `websocketUrlFromBase` now strips the trailing slash from `url.pathname` before appending `/ws`, fixing the `ws://host//ws` 404 when the base URL had a bare origin; (2) `crates/nimbus-server/src/adapters/convex/handlers/socket.rs` — the convex WS handler now skips `ensure_tenant_exists_async` for the system tenant (mirroring the HTTP query handler's `registry_and_auth` short-circuit), since `_nimbus` has no tenant directory on disk and the old check 404'd before reaching the upgrade. Verification: `npx biome check src/{components/{copy-chip,state-chip,time},lib/{format,nimbus-client},routes/index,shell/status-bar}.{ts,tsx}` clean (3 redundant `role="list"` attrs removed); `npm run typecheck` (root) clean; `cargo fmt --all --check` clean; `cargo build -p nimbus-bin` refreshed embedded `/ui/*` assets; `vite build` → 421.15 KB JS + 55.97 KB CSS = 157.84 KB gzipped (still under the 450 KB pause threshold). Live embedded-build proof on `target/debug/nimbus start --port 8088` after POST `/ui/auth/session`: WebSocket upgrade returns `HTTP/1.1 101 Switching Protocols` with `sec-websocket-protocol: nimbus.v2` and the server hello frame advertises `auth.socket.v1` / `queries.v1` / `subscriptions.v1` / `convex.named_subscriptions.v1`; `chrome-devtools` snapshot of `http://127.0.0.1:8088/ui/` shows status-bar `image "Connected"` (green dot), TopStrip populated with `OK` / `0.1.31` / `14m` / `developer` / `14m ago` / `1m ago` / `_nimbus`, all six count panels rendering `0` with `No state breakdown` empty state, EventsFeed showing `No events recorded yet — the feed updates live.`, RecentRuns showing `No runs yet — invoke a function to populate this list.`, and sidebar `Network 39` count live-updating from the `_nimbus` events stream. Screenshots captured at `.playwright-cli/du4-embedded-{dark,light}.png` via `data-theme` toggle. |
| 2026-05-15 | DU6 — Services and functions tabs | done | Implemented two operator surfaces, both driven entirely by live `useQuery` subscriptions against the `_nimbus` system tenant. (1) `packages/nimbus-ui/src/routes/compute.tsx` — tabbed shell `Services` / `Functions` / `Scheduled` / `Cron` keyed off URL state via `?tab=` (default `services`); header `BundleHint` chip aggregates `api.bundles.list` and shows `<n> bundle(s) · <m> active`; per-section live counts in the tablist; tables for each section show empty states with section-appropriate copy when the registry is empty. ServicesTable renders state chip + `shortId` (`CopyChip`) + machine link, machine name, function path (mono), restarts, startedAt (`RelativeTime`); FunctionsTable renders path (mono), kind, `auth?` / `cache?` flags, bundle sha256 (`CopyChip`), updatedAt; ScheduledTable renders status chip, function path, args preview, runAt; CronTable renders state chip, name, function path, cronExpression, nextRunAt. All ids/digests/paths render in monospace with copy affordances. (2) `packages/nimbus-ui/src/routes/network.tsx` — HTTP routes table with method-tone palette (`GET=success`, `POST=link`, `PUT/PATCH=warning`, `DELETE=danger`, `OPTIONS/HEAD=muted`) and adapter filter chips (`role="tablist"`) populated dynamically from the live route set; inline search `[data-inline-search]` filters across method/path/handler/adapter (shell-level `/` focus contract intact). Renders 39 live routes from the registry across `convex`, `firebase`, `native`, and `ui` adapters with auth-required flag and `lastRequestAt` (`RelativeTime` / "never"). Color-contrast fix during verification: light `--color-warning` darkened from `oklch(65% 0.16 75)` (#c67d00, 3.32:1) → `oklch(53% 0.16 75)` (#9c5600, 5.61:1) in `packages/nimbus-ui/src/styles/globals.css` so the PATCH/PUT method label passes WCAG 2 AA on white surface; dark theme has its own override (`oklch(78% 0.17 75)`), unaffected. Tooling stewardship: added `packages/nimbus-ui/public/assets/axe.min.js` from `axe-core` 4.10 so the file survives `vite build` cleans and is automatically embedded by `rust_embed`; previous DU5 approach (manually copying into `dist/`) didn't survive rebuilds. Verification: `npm run typecheck` clean; `vite build` → 421.77 KB JS + 57.50 KB CSS = 131.51 KB JS gzipped (under the 450 KB pause threshold; `compute-C_UpyNEA.js` 10.15 KB / 2.26 KB gz, `network-CStrDTyR.js` 5.15 KB / 1.77 KB gz are autoCodeSplit lazy chunks); `cargo build --release -p nimbus-bin` clean. Live end-to-end proof on `target/release/nimbus serve --addr 127.0.0.1:8088` against an empty registry: `/ui/compute` shows BundleHint `1 bundle · 1 active`, tab counts `0/0/0/0`, section-specific empty states for each tab; `/ui/network` table renders all 39 routes with method tones, adapter chips render `all / convex / firebase / native / ui`, filter input narrows the list as expected. axe-core (WCAG2 A/AA + 2.1 A/AA) on the embedded build after `POST /ui/auth/session`: **dark — `/ui/compute` 0 violations / 23 passes / 1 incomplete; `/ui/network` 0 violations / 28 passes / 1 incomplete; light — `/ui/compute` 0 violations / 23 passes / 1 incomplete; `/ui/network` 0 violations / 28 passes / 1 incomplete.** Screenshots captured at `.playwright-cli/du6-compute-{dark,light}.png` and `.playwright-cli/du6-network-{dark,light}.png`. |
