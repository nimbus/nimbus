# Plan: Desktop UI

Canonical execution plan for a Docker Desktop / Podman Desktop-style graphical
interface for Neovex. The UI is an embedded React SPA served by
`neovex-server` at `/ui/*`, consuming the system tenant query surface and
HTTP lifecycle endpoints via the `neovex` JS SDK's `useQuery` /
`useMutation` hooks over the existing Convex-compatible WebSocket.

This plan covers the **React frontend only** — the server-side prerequisites
are owned by separate plans (see Prerequisites below).

Reviewed against:

- `crates/neovex-server/src/router.rs` — current route tree,
  `tower_http::services::ServeDir` static serving at `/demos`
- `packages/neovex/src/react.ts` — `NeovexProvider`, `useQuery`,
  `useMutation`, `useAction`, `usePaginatedQuery`, `useQueries`,
  `useNeovexAuth`, `useNeovexConnectionState`
- `packages/neovex/src/browser.ts` — `NeovexClient`, `ConnectionState`
- `demos/convex/html/` — proven end-to-end: codegen → React hooks →
  WebSocket → engine queries/mutations

Open source reference implementations studied:

| Project | Stars | Stack | Pattern | Key lesson |
| --- | --- | --- | --- | --- |
| Podman Desktop | 7.5k | Electron 41 + Svelte 5 + Tailwind 4 | Electron IPC to Podman socket | Co-located `.spec.ts` tests, typed IPC via `dts-for-context-bridge`, Electron Fuses, 297+ IPC channels |
| Jan | 42k | Tauri 2 + React 19 + Radix UI + Tailwind 4 | localhost REST API via embedded hyper proxy | Service Hub platform-abstraction, Zustand 5 + TanStack Router, unified `radix-ui` package |
| Portainer | 37k | React + Go | Go serves SPA, REST + WebSocket | Validates "server embeds and serves the SPA" pattern |
| Prisma Studio | — | React component lib | BFF pattern | Cleanest embedded dev-UI pattern |

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
| `docs/plans/websocket-protocol-plan.md` | Versioned protocol spec, error schema, subprotocol negotiation, structured error types | WP1–WP4 |
| `docs/plans/localhost-server-security-plan.md` | Token file, origin allowlist, session cookie, CSP, server discovery, audit log, middleware stack | LS1–LS5 |
| `docs/plans/system-tenant-api-plan.md` | `_neovex` system tenant, state persistence, HTTP lifecycle endpoints, Convex function bundle | ST1–ST4 |

## Status

- **Status:** `active`
- **Primary owner:** this plan
- **Activation gate:** all three prerequisite plans completed
- **Related plans:**
  - `docs/plans/websocket-protocol-plan.md` — protocol and error schema
  - `docs/plans/localhost-server-security-plan.md` — auth and server security
  - `docs/plans/system-tenant-api-plan.md` — data layer the UI consumes
  - `docs/reference/microvm-service-baseline.md` — machine/service architecture

## Current Assessed State

- No production UI, no `neovex ui` subcommand, no embedded SPA exist today.
- The JS SDK ships all needed hooks (`useQuery`, `useMutation`, `useAction`,
  `usePaginatedQuery`, `useQueries`, `useNeovexConnectionState`).
- The server already serves static files at `/demos` via
  `tower_http::services::ServeDir`.
- The React demos in `demos/convex/html/` prove the full stack end-to-end.

## Control Plan Rules

1. The UI is a **consumer** of the system tenant query surface and HTTP
   lifecycle endpoints — no direct storage writes, no bypass of `Service`.
2. The embedded SPA is the **primary** UI surface. The Electron shell
   (Phase 2) loads the same bundle from the same localhost URL.
3. The UI is served **from the same process** as the API.

## Verification Contract

Each roadmap item must satisfy before closing:

- `cargo fmt --all --check` — clean
- `make clippy` — clean
- `make test` — green (Rust)
- `npm run build --workspaces --if-present` — green (JS)
- `npm run test --workspaces --if-present` — green (JS)
- Keyboard navigation works for all interactive elements added
- `@axe-core/react` reports zero critical or serious a11y violations
- Dark mode renders correctly (no invisible text, no broken contrast)
- Bundle size of `packages/neovex-ui/dist/` stays under 500 KB gzipped
- Manual verification described per item

## Architecture

### Phasing

```
Phase 1: Embedded Web UI          Phase 2: Electron Shell
┌──────────────────────────┐      ┌──────────────────────────────┐
│  packages/neovex-ui/     │      │  neovex-desktop repo         │
│  React + shadcn/ui       │      │  Electron 41                 │
│  Convex function bundle  │      │  (mac/win/linux)             │
│  Vite build → dist/      │      │         │                    │
│         │                │      │         ▼                    │
│         ▼                │      │  loadURL(localhost:PORT/ui)  │
│  rust-embed in           │      │  + tray, menus, auto-update  │
│  neovex-server           │      └──────────────────────────────┘
│         │                │
│  GET /ui/* routes        │
│  neovex ui subcommand    │
└──────────────────────────┘
```

### Component stack

| Layer | Choice | Version | Rationale |
| --- | --- | --- | --- |
| Framework | React | 19.2.x | Already used by JS SDK; same stack as Jan (42k★), Portainer (37k★) |
| Components | shadcn/ui (unified `radix-ui` + Tailwind) | radix-ui 1.4.x | Copy-pasted source, no version lock-in; requires unified `radix-ui` package (not individual `@radix-ui/react-*`) for React 19 compatibility |
| Animations | tw-animate-css | 1.4.x | Pure CSS animations for Tailwind v4; replaces `tailwindcss-animate` |
| State | Zustand | 5.0.x | Lightweight, native `useSyncExternalStore`; v5 drops default exports |
| Router | TanStack Router | 1.168.x | Type-safe, file-based via `@tanstack/router-vite-plugin` 1.166.x |
| Bundler | Vite | 8.0.x | Rolldown (Rust-based), 10-30x faster builds |
| CSS | Tailwind CSS | 4.2.x | CSS-first config (`@theme` directive); colors use OKLCH |
| Icons | Lucide | lucide-react 1.8.x | MIT, tree-shakeable, shadcn/ui default |
| Theming | Tailwind `dark:` variant + CSS variables | — | shadcn/ui ships light + dark; `prefers-color-scheme` detection |
| Accessibility | Radix UI ARIA primitives + axe-core | — | WCAG 2.1 AA target |
| Embedding | `rust-embed` | 8.11.x | `debug_embed = false` in dev — serve from disk without `cargo build` |
| Testing | Vitest 4.1.x + React Testing Library 16.3.x | — | Vitest 4 targets Vite 8; RTL 16 supports React 19 |
| E2E | Playwright | 1.59.x | Pin version for API stability |

### Package layout

```
packages/neovex-ui/
├── package.json              # react 19.2, radix-ui, tailwindcss 4, vite 8
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
│   └── system.ts             # action: status
├── src/
│   ├── main.tsx              # entry, NeovexProvider + router + ThemeProvider
│   ├── routes/
│   │   ├── __root.tsx        # shell layout (sidebar, header, connection state, dark mode)
│   │   ├── dashboard.tsx
│   │   ├── machines/
│   │   │   ├── index.tsx     # machine list
│   │   │   └── $id.tsx       # machine detail + log tail
│   │   ├── services/
│   │   │   ├── index.tsx
│   │   │   └── $id.tsx
│   │   ├── data/
│   │   │   ├── index.tsx     # table list
│   │   │   └── $table.tsx    # table browser
│   │   ├── functions/
│   │   │   ├── index.tsx     # bundle list + function list
│   │   │   └── jobs.tsx      # scheduled jobs + cron jobs
│   │   ├── logs.tsx          # live log tail
│   │   ├── runs/
│   │   │   ├── index.tsx
│   │   │   └── $id.tsx
│   │   └── settings.tsx
│   ├── components/           # shadcn/ui components + app-specific composites
│   ├── hooks/                # thin wrappers over useQuery for typed access
│   └── lib/                  # auth, connection, theme, utilities
├── dist/                     # Vite build output (gitignored, embedded by Rust)
└── .storybook/               # component documentation
```

### Server integration

1. **`/ui/*` route** — serves the embedded SPA via `rust-embed`. Falls through
   to `index.html` for client-side routing. In debug builds, `rust-embed`
   with `debug_embed = false` serves from disk — no `cargo build` on every
   UI change.

2. **Build integration** — `build-ui` Make target
   (`npm run build -w packages/neovex-ui`). Top-level `build` and `ci`
   targets depend on it. Release-build `build.rs` asserts
   `dist/index.html` exists.

### `neovex ui` subcommand

```
neovex ui            # open browser to running server; error if none
neovex ui --ensure   # start server first if none running, then open browser
```

Discovers server via `$XDG_RUNTIME_DIR/neovex/server.json` (written by
`neovex start` — see `localhost-server-security-plan.md` LS1). Uses
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
| Dashboard | `system.status`, `machines.list`, `services.list`, `events.recent{limit:20}`, `runs.recent{limit:10}` |
| Machines | `machines.list` |
| Machine detail | `machines.byId`, `services.list{machineId}`, stream `logs:machine:<id>` |
| Services | `services.list` |
| Service detail | `services.byId`, stream `logs:service:<id>` |
| Functions + Jobs | `bundles.list` → `functions.list{bundleId}`, `scheduled_jobs.list`, `cron_jobs.list` |
| Data | `tables.list` → REST `GET /api/tenants/{id}/documents/{table}` (cross-tenant) |
| Live Logs | stream `events:all` with filter controls |
| Runs | `runs.recent` → `runs.byId` |
| Settings | `system.status` + HTTP lifecycle endpoints |

## Roadmap

### DU1 — Server: embedded static asset serving at `/ui/*`

Add `rust-embed` 8.11.x to `neovex-server`, embed `packages/neovex-ui/dist/`,
serve at `/ui/*` with `index.html` fallback. `debug_embed = false` for dev.

Scaffold `packages/neovex-ui` as a minimal npm package with Vite 8.0.x and
a placeholder `index.html`. Add `build-ui` Make target. Add `build.rs`
assertion for release builds.

**Verification:** `curl localhost:PORT/ui/` returns HTML, SPA fallback works,
`make build` includes UI build step.

**Status:** `pending`

### DU2 — CLI: `neovex ui` subcommand

Add `neovex ui` and `neovex ui --ensure`. Reads server discovery file,
opens browser via `open::that`. `--ensure` starts server if not running.

**Verification:** (a) `neovex start &` + `neovex ui` opens browser,
(b) no server → clear error, (c) `--ensure` starts then opens.

**Status:** `pending`

### DU3 — UI: scaffold and shell layout

Replace placeholder with full component stack: React 19.2.x, shadcn/ui
(unified `radix-ui` 1.4.x + Tailwind 4.2.x + `tw-animate-css` 1.4.x),
Zustand 5.0.x, TanStack Router 1.168.x, Lucide 1.8.x.

Build shell layout: sidebar nav, header with connection state indicator
(`useNeovexConnectionState`), dark mode toggle with system preference
detection, error boundary, disconnected state overlay.

**Verification:** `npm run build` succeeds, sidebar nav works, connection
states render correctly, dark mode works, bundle < 500 KB gzipped.

**Status:** `pending`

### DU4 — UI: dashboard tab

Dashboard cards: system status (uptime, version), machine summary (by
state), service summary (by state), recent events (last 20), recent runs
(last 10). All via `useQuery` — no polling.

**Verification:** live data renders, machine state changes reflected in
one render cycle, events list updates in real time.

**Status:** `pending`

### DU5 — UI: machines tab

Machine list with state badges and action buttons (start, stop, restart,
delete via HTTP endpoints). Machine detail with config, services, log tail.
Optimistic updates on lifecycle actions.

**Verification:** state transitions via optimistic update, log tail
streams without gaps, action errors render inline.

**Status:** `pending`

### DU6 — UI: services and functions tabs

- **Services:** list + detail with health snapshot and log tail.
- **Functions + Jobs:** bundle list → function list with kind/schema,
  scheduled jobs with status, cron jobs with schedule/next-run.

**Verification:** live data, function kind badges, scheduled job status
updates reactively, cron next-run refreshes.

**Status:** `pending`

### DU7 — UI: data browser tab

Table list → row browser with `usePaginatedQuery`. Schema display.
Filter and sort controls. Cursor invalidation on schema changes (table
dropped → "Table no longer exists").

**Verification:** pagination works, schema changes handled gracefully,
1000+ row tables browseable.

**Status:** `pending`

### DU8 — UI: logs and runs tabs

- **Logs:** live event stream with level/category/source filters.
- **Runs:** recent runs → run detail with trace viewer (timing waterfall).

**Verification:** filters apply without losing position, trace viewer
shows timing, 100+ events/second without UI lag.

**Status:** `pending`

### DU9 — UI: settings tab

Server info (version, uptime, address). Token rotation button (HTTP
endpoint). Shutdown button with confirmation.

**Verification:** token rotation triggers re-auth, shutdown shows
disconnect state.

**Status:** `pending`

### DU10 — Testing: unit, integration, E2E, and Storybook

Testing pyramid:

| Layer | Tool | What it tests |
| --- | --- | --- |
| Unit | Vitest 4.1.x + JSDOM | Hooks, utilities, pure logic |
| Component | Vitest + RTL 16.3.x + `@axe-core/react` | Rendering, interaction, a11y |
| Integration | Vitest + mocked WebSocket | useQuery/useMutation against mock |
| E2E | Playwright 1.59.x | Full flows against `neovex start` |

Co-located `.spec.tsx` beside every `.tsx` (Podman Desktop pattern).
Storybook for all components + error rendering matrix (~12 stories).

**Verification:** `npm run test` green, `npm run storybook` launches,
co-located specs for all files, axe-core zero critical/serious, dark mode
correct in all stories.

**Status:** `pending`

## Phase 2: Electron Shell (future plan scope)

A separate plan will be authored when Phase 1 is stable and users request
native-app behavior (dock icon, tray, auto-update, deep links).

**Activation gate:** all Phase 1 items (DU1–DU10) shipped and stable.

### Why Electron, not Tauri

| Factor | Electron | Tauri 2 |
| --- | --- | --- |
| Linux rendering | Chromium — consistent | WebKitGTK — lags, GPU issues |
| Auto-update on Linux | electron-updater (AppImage, deb, rpm) | No built-in |
| Shell complexity | `loadURL(localhost)` — no Rust logic needed | Rust build pipeline friction |
| Bundle size | ~150 MB | ~10 MB |

The shell wraps `localhost:PORT/ui` and manages `neovex start` lifecycle.
All business logic stays in the Rust server.

### Security configuration (Electron 41.2.x)

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
  `$XDG_RUNTIME_DIR/neovex/server.json`
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

### Repo structure

Separate repo: `agentstation/neovex-desktop`.

```
neovex-desktop/
├── package.json              # electron 41.2, electron-builder 26.8
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
| React + Radix | `~/src/github.com/janhq/jan/web-app/src/components/` | Radix UI component patterns |
| TanStack Router | `~/src/github.com/janhq/jan/web-app/src/routes/` | File-based routing |
| Zustand stores | `~/src/github.com/janhq/jan/web-app/src/stores/` | Minimal state management |

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-18 | Plan authored | — | Architecture designed from Opus 4.7 session; reference implementations researched |
| 2026-04-18 | Deep research audit | — | Cloned podman-desktop and jan repos; audited Electron 41, IPC, packaging, testing |
| 2026-04-18 | Adversarial review | — | 20 findings applied (F01-F20): Convex app architecture, XDG paths, dark mode, a11y, bundle size targets, etc. |
| 2026-04-18 | Plan decomposition | — | Extracted 3 prerequisite plans: websocket-protocol-plan.md (WP1-WP4), localhost-server-security-plan.md (LS1-LS5), system-tenant-api-plan.md (ST1-ST4). UI plan scoped to React SPA only (DU1-DU10). |
