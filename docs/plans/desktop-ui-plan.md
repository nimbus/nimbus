# Plan: Desktop UI

Canonical execution plan for a Docker Desktop / Podman Desktop-style graphical
interface for Neovex. The UI surfaces machine lifecycle, service health,
runtime invocations, data browsing, and live logs through a reactive dashboard
that dogfoods the existing Convex-compatible WebSocket protocol and the
`neovex` JS SDK's `useQuery` / `useMutation` hooks.

Reviewed against:

- `crates/neovex-server/src/router.rs` — current HTTP/WebSocket route tree,
  `tower_http::services::ServeDir` static serving, CORS layer
- `crates/neovex-server/src/ws/mod.rs` — WebSocket upgrade handler, tenant
  extraction from headers/query params
- `crates/neovex-server/src/protocol.rs` — client/server frame types
  (`Authenticate`, `Subscribe`, `SubscriptionResult`, etc.)
- `crates/neovex-server/src/state.rs` — `AppState` (Service, ConvexRegistry,
  LicenseState, RuntimeServiceRegistry)
- `crates/neovex-bin/src/main.rs` — CLI subcommands (`serve`, `machine`,
  `service`)
- `packages/neovex/src/react.ts` — `NeovexProvider`, `useQuery`, `useMutation`,
  `useAction`, `usePaginatedQuery`
- `packages/neovex/src/browser.ts` — `NeovexClient`, WebSocket subscriptions,
  auth token management, `ConnectionState`
- `demos/` — existing static HTML/React demos served at `/demos`

Open source reference implementations studied:

| Project | Stars | Stack | Pattern | Key lesson |
| --- | --- | --- | --- | --- |
| Podman Desktop | 7.5k | Electron 41 + Svelte 5 + Tailwind 4 | Electron IPC to Podman socket | Monorepo split (main/renderer/preload/api/ui), co-located `.spec.ts` tests, typed IPC via `dts-for-context-bridge`, Electron Fuses |
| Jan | 42k | Tauri 2 + React 19 + Radix UI + Tailwind 4 | localhost REST API via embedded hyper proxy | Service Hub platform-abstraction pattern, UI works in Tauri and plain browser, Zustand 5 + TanStack Router |
| Portainer | 37k | React + Go | Go serves SPA, REST + WebSocket | Validates "server embeds and serves the SPA" pattern for infra dashboards |
| Rancher Desktop | 7.1k | Electron + Vue 3 + Go CLI | Go CLI ↔ Electron via REST API | Shared API contract between CLI and UI via OpenAPI spec |
| Supabase Studio | 35k+ | Next.js + React + Tailwind | Local mode via Docker Compose | Validates "web UI served by local server" for dev tools |
| Prisma Studio | — | React component lib | BFF pattern (host provides POST endpoint) | Cleanest embedded dev-UI pattern; UI is a React lib, host provides the backend |

Cloned reference repositories (shallow, for future agents to consult):

| Repo | Local path | Why |
| --- | --- | --- |
| `podman-desktop/podman-desktop` | `~/src/github.com/podman-desktop/podman-desktop` | Primary Electron architecture reference — IPC patterns, security hardening, Electron Fuses, co-located tests, cross-platform packaging |
| `janhq/jan` | `~/src/github.com/janhq/jan` | Primary React + localhost-server pattern reference — Service Hub abstraction, Radix UI + Tailwind stack, embedded proxy server, TanStack Router |

Key findings from cloned-repo audit:

- **Podman Desktop's preload is 2,724 lines** of hand-written IPC bridge code —
  a cautionary tale. Invest in typed IPC generation early.
  See `packages/preload/src/index.ts`.
- **Podman Desktop's PluginSystem is 3,466 lines** — the god-object `ipcHandle()`
  pattern at `packages/main/src/plugin/index.ts:303` should be split by domain.
- **Podman Desktop enables Electron Fuses** at build time
  (`.electron-builder.config.cjs:62`): `RunAsNode: false`,
  `EnableNodeOptionsEnvironmentVariable: false`.
- **Jan's Service Hub** (`web-app/src/services/index.ts`) cleanly abstracts
  platform differences (desktop/mobile/web) behind typed interfaces — the best
  pattern for making the React app testable without a real backend.
- **Jan's embedded proxy** (`src-tauri/src/core/server/proxy.rs`) runs a hyper
  HTTP server inside the app process — directly applicable to neovex's
  architecture where the Rust server is the backend.
- **Both projects use Vite + Tailwind CSS 4** — consensus stack for desktop app
  frontends in 2025-2026.
- **Both projects struggle with IPC type safety** — Podman solves it with
  generated `.d.ts` from the preload; Jan does not solve it at all.

Architecture decision: **embedded web UI first** (Portainer / Prisma pattern),
**Electron shell second** (Podman Desktop pattern). The existing WebSocket
transport, React SDK, and static-serving infrastructure make the embedded web
UI the highest-leverage first step. The Electron shell is Phase 2: it adds
~150 MB but provides Chromium consistency on all platforms (including Linux,
where Tauri's WebKitGTK is the weak link), mature auto-update, code signing,
and tray/dock integration. The shell is thin — it wraps `localhost:PORT/ui`
and manages the `neovex serve` lifecycle. All business logic stays in the
Rust server.

---

## Status

- **Status:** `active`
- **Primary owner:** this plan
- **Activation gate:** prompted by desktop-UI design session on 2026-04-18
- **Related plans:**
  - `docs/reference/microvm-service-baseline.md` — architecture context for
    machine/service tables the UI will surface
  - `docs/reference/macos-machine-flow.md` — macOS machine contract the UI
    reflects
  - `docs/plans/archive/machine-cli-dx-plan.md` — completed CLI DX baseline
    that the UI complements
    (shared API contract, not a replacement)

## Current Assessed State

- The server already serves static files at `/demos` via
  `tower_http::services::ServeDir` and has a working WebSocket upgrade path
  with Convex-compatible subscribe/unsubscribe framing.
- The JS SDK (`packages/neovex`) ships `useQuery`, `useMutation`, `useAction`,
  and `usePaginatedQuery` hooks with WebSocket-backed reactive subscriptions,
  automatic reconnection, and `ConnectionState` tracking.
- The React demos in `demos/convex/html/` prove the full stack end-to-end:
  codegen → React hooks → WebSocket → engine queries/mutations.
- No production UI, no `neovex ui` subcommand, no embedded SPA, no auth
  gating on localhost, no protocol version negotiation, no admin-scoped
  internal tables exist today.

## Control Plan Rules

1. The UI is a **consumer** of `Service`, not a bypass. Every mutation the UI
   issues flows through `Service::apply_mutation`. No direct storage writes.
2. The UI is served **from the same process** as the API — no separate server,
   no separate port, no Docker container for the UI.
3. The embedded SPA is the **primary** UI surface. A native shell (Tauri or
   Electron) is a wrapper, not a fork — it loads the same bundle from the
   same localhost URL.
4. Auth for the UI uses the same token written to `~/.neovex/auth/token` that
   the CLI uses. No separate credential system.
5. The WebSocket protocol spec (`docs/reference/websocket-protocol.md`) and
   the error schema (`docs/reference/errors.md`) are written **before** the UI
   code, not after.

## Verification Contract

Each roadmap item must satisfy before closing:

- `cargo fmt --all --check` — clean
- `make clippy` — clean
- `make test` — green (Rust)
- `npm run build --workspaces --if-present` — green (JS)
- `npm run test --workspaces --if-present` — green (JS)
- Manual verification described per item

## Architecture

### Phasing

```
Phase 1: Embedded Web UI          Phase 2: Native Shell (optional)
┌──────────────────────────┐      ┌──────────────────────────────┐
│  packages/neovex-ui/     │      │  neovex-desktop repo         │
│  React + shadcn/ui       │      │  Tauri 2 (mac/win) or        │
│  Vite build → dist/      │      │  Electron (mac/win/linux)    │
│         │                │      │         │                    │
│         ▼                │      │         ▼                    │
│  rust-embed in           │      │  loadURL(localhost:PORT/ui)  │
│  neovex-server           │      │  + tray, menus, auto-update  │
│         │                │      └──────────────────────────────┘
│         ▼                │
│  GET /ui/* routes        │
│  neovex ui subcommand    │
│  localhost-only auth     │
└──────────────────────────┘
```

Phase 1 is the plan scope. Phase 2 is documented for architecture alignment
but is a separate future plan with its own activation gate.

### Component stack

| Layer | Choice | Rationale |
| --- | --- | --- |
| Framework | React 19 | Already used by JS SDK; same stack as Jan (42k★), Portainer (37k★) |
| Components | shadcn/ui (Radix UI + Tailwind) | Copy-pasted source, no version lock-in; same primitives as Jan; dashboard components (tables, cards, sidebars, command palette) included |
| State | Zustand | Lightweight, works with React 19 concurrent features; Jan uses this successfully |
| Router | TanStack Router | Type-safe, file-based; proven at scale in Jan |
| Bundler | Vite | Fast dev server, production builds; Podman Desktop uses this |
| Icons | Lucide | MIT, tree-shakeable, same as shadcn/ui default |
| Embedding | `rust-embed` | Compile-time static asset inclusion in the Rust binary |

### Package layout

```
packages/neovex-ui/
├── package.json              # React 19, shadcn/ui, Tailwind, Vite
├── tsconfig.json
├── vite.config.ts
├── index.html
├── src/
│   ├── main.tsx              # entry, NeovexProvider + router
│   ├── routes/
│   │   ├── __root.tsx        # shell layout (sidebar, header, connection state)
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
│   │   ├── functions.tsx
│   │   ├── logs.tsx          # live log tail
│   │   ├── runs/
│   │   │   ├── index.tsx
│   │   │   └── $id.tsx
│   │   └── settings.tsx
│   ├── components/           # shadcn/ui components + app-specific composites
│   ├── hooks/                # thin wrappers over useQuery for typed access
│   └── lib/                  # auth, connection, utilities
├── dist/                     # Vite build output (gitignored, embedded by Rust)
└── .storybook/               # component documentation
```

### Server integration

The existing `neovex-server` router gains:

1. **`/ui/*` route** — serves the embedded SPA via `rust-embed`. Falls through
   to `index.html` for client-side routing. Separate from `/demos` (which
   stays for demo/example purposes).

2. **Session cookie bootstrap** — on first navigation to `/ui/`, if the
   request is a top-level navigation (`Sec-Fetch-Mode: navigate`,
   `Sec-Fetch-Site: none`), set a signed `neovex_session` cookie
   (`HttpOnly; SameSite=Strict; Path=/`) derived from the token file. The JS
   SDK opens the WebSocket; the cookie rides the upgrade.

3. **Content Security Policy** — the server sets a CSP header on all `/ui/*`
   responses:
   ```
   default-src 'self';
   script-src 'self';
   style-src 'self' 'unsafe-inline';
   img-src 'self' data:;
   font-src 'self' data:;
   connect-src 'self' ws://127.0.0.1:* ws://localhost:*;
   ```
   No `'unsafe-eval'` in production. If Vite HMR needs it during development,
   gate it behind a `#[cfg(debug_assertions)]` build flag.
   Reference: Podman Desktop omits CSP entirely and relies on Electron-level
   restrictions (`packages/main/src/security-restrictions.ts`) — we do both.

4. **Origin allowlist on WebSocket upgrade** — reject origins that are not
   `http://127.0.0.1:PORT`, `http://localhost:PORT`, or an approved native
   shell scheme. This blocks DNS rebinding before auth is checked.

5. **Middleware ordering on `/ws`** — request flows through layers in this
   order (outermost first):
   ```
   trace → request_id → origin_allowlist → rate_limit → auth_extract → protocol_select → ws_upgrade
   ```
   Origin before auth prevents leaking token-validity timing to hostile
   origins. Protocol after auth avoids wasting parser work on unauthenticated
   requests.

### `neovex ui` subcommand

```
neovex ui            # open browser to running server; error if none
neovex ui --ensure   # start server first if none running, then open browser
```

Discovery via `~/.neovex/run/server.json`:
```json
{
  "pid": 12345,
  "address": "127.0.0.1:6789",
  "startedAt": "2026-04-18T12:34:56Z",
  "version": "0.2.3",
  "protocolVersions": ["neovex.v1", "neovex.v2"]
}
```

`neovex serve` writes this file on bind (with a `RemoveOnDrop` guard) and
validates PID liveness on startup to handle stale files from `SIGKILL`.

### Protocol version handshake

Subprotocol negotiation on the WebSocket upgrade via
`Sec-WebSocket-Protocol: neovex.v2, neovex.v1`. Server picks highest overlap
or rejects with HTTP 400.

Post-upgrade, server sends `hello`:
```json
{
  "type": "hello",
  "protocol": "neovex.v2",
  "server": { "version": "0.2.3", "build": "git:abc123" },
  "features": ["machine.v1", "runtime.v2", "storage.indexes.v1"],
  "session": { "id": "s_01HX...", "serverNow": 1713571200000 }
}
```

Client replies `client_hello`:
```json
{
  "type": "client_hello",
  "protocol": "neovex.v2",
  "client": { "kind": "browser", "version": "0.2.3" },
  "capabilities": ["queries.v1", "mutations.v1", "subscriptions.v1"]
}
```

Features are individually negotiated capabilities. Missing features produce
per-operation errors so the UI can degrade gracefully (hide a tab, show an
upgrade hint) rather than fail the connection.

### Token-gate design

| Client | How it authenticates | Why |
| --- | --- | --- |
| CLI | Reads `~/.neovex/auth/token` directly | Same user, same filesystem |
| Native shell | Reads token file, sends `Authorization: Bearer <token>` on upgrade | Has filesystem access |
| Browser tab | Session cookie set on first `/ui/` navigation | Cannot read filesystem; must not hold raw token |

Token file (`~/.neovex/auth/token`, `0600`):
```json
{
  "version": 1,
  "token": "neovex_at_<base64url-256bit>",
  "generation": 1,
  "issuedAt": "2026-04-18T...",
  "scope": "local-admin"
}
```

`neovex token rotate` bumps `generation`, invalidating outstanding sessions
with `auth.token_revoked`.

Defense-in-depth layers (all active simultaneously):
- Bind loopback only (`127.0.0.1` / `::1`)
- Origin allowlist on WebSocket upgrade
- Private Network Access preflight denial for non-loopback origins
- Audit log at `~/.neovex/logs/access.jsonl`
- No tokens in URLs (headers or cookies only)
- No custom crypto — sign session cookies with a key derived from the token
  file using `ring` or equivalent

### Error schema

One shape everywhere — HTTP bodies, WebSocket close payloads, per-op errors:

```json
{
  "error": {
    "code": "protocol.no_overlap",
    "message": "Server does not support protocol neovex.v3.",
    "requestId": "req_01HX3PKGZT...",
    "timestamp": "2026-04-18T12:34:56.789Z",
    "severity": "fatal",
    "retryable": false,
    "detail": {
      "serverSupports": ["neovex.v1", "neovex.v2"],
      "clientOffered": ["neovex.v3"]
    },
    "remediation": {
      "action": "upgrade_server",
      "message": "Update Neovex to match this client.",
      "docsUrl": "https://neovex.dev/docs/errors/protocol.no_overlap"
    }
  }
}
```

Field contracts:

| Field | Rule |
| --- | --- |
| `code` | Machine-stable, snake_case, dotted namespace. Public API — never rename. |
| `message` | Human-readable. May change between versions. Never parse client-side. |
| `requestId` | Always present. Users paste in bug reports; server logs correlate. |
| `severity` | `fatal` (session done), `error` (this op failed), `warning` (succeeded with caveat). |
| `retryable` | Explicit boolean. Client must not infer from code. |
| `detail` | Per-code typed payload. Schema documented alongside the code. |
| `remediation` | Optional. `action` is an enum for UI "Fix this" buttons. |

Error code namespaces: `auth.*`, `protocol.*`, `rate.*`, `session.*`, `op.*`,
`machine.*`, `service.*`.

### Session loop and op dispatch

Single-writer on the WebSocket. Mutation workers and subscription streams
send via a bounded `mpsc` outbox; the session loop drains it:

```
loop {
    select! {
        biased;                          // shutdown always wins
        _ = shutdown.cancelled() => break,
        incoming = socket.recv() => dispatch(incoming),
        out = outbox.recv()      => socket.send(out),
    }
}
```

**Ordering guarantee**: for a mutation M that changes data observed by queries
Q1..Qn, the server emits `query.result(Q1..Qn)` frames **before** the
`mutation.result(M)` frame on the same socket. This makes optimistic UI
flicker-free — the `useQuery` cache updates before the overlay is discarded.

**Backpressure**: bounded outbox (256 frames) with per-query "latest value
wins" dedup. Event streams use sequence numbers so clients detect drops.

### Data model — internal tables

Internal tables use a reserved `_neovex.*` namespace:

| Table | Key fields | Purpose |
| --- | --- | --- |
| `machines` | name, kind, state, provider, resources, meta | Machine inventory |
| `services` | name, machineId, bundleId, kind, state, endpoints, health | Service registry |
| `bundles` | sha256, sizeBytes, sourceRef, status | Deployed bundles |
| `functions` | bundleId, path, kind, argsSchema, returnsSchema | Per-bundle function registry |
| `tables` | name, schema, rowCount, lastWriteAt | User-data table directory |
| `events` | source, level, category, message, data, correlationId | Event firehose |
| `runs` | bundleId, functionPath, kind, durationMs, status, error | Runtime invocations |

### Query surface

```
_neovex.machines.list({ filter? })            → Machine[]
_neovex.machines.byId({ id })                 → Machine | null
_neovex.services.list({ machineId? })         → Service[]
_neovex.services.byId({ id })                 → Service | null
_neovex.bundles.list()                        → Bundle[]
_neovex.functions.list({ bundleId })          → FunctionEntry[]
_neovex.tables.list()                         → TableSummary[]
_neovex.tables.browse({ name, limit, cursor? }) → { rows, nextCursor }
_neovex.events.recent({ filter?, limit })     → Event[]
_neovex.runs.recent({ filter?, limit })       → Run[]
_neovex.system.status()                       → { uptime, version, health }
```

### Mutation surface

```
_neovex.machines.{create,start,stop,restart,delete,rename}
_neovex.services.{create,start,stop,restart,delete}
_neovex.bundles.{delete,promote}
_neovex.tables.{create,setSchema,dropSchema,deleteRows}
_neovex.system.{tokenRotate,shutdown}
```

### UI tab → query map

| Tab | Queries / streams |
| --- | --- |
| Dashboard | `system.status`, `machines.list`, `services.list`, `events.recent{limit:20}`, `runs.recent{limit:10}` |
| Machines | `machines.list` |
| Machine detail | `machines.byId`, `services.list{machineId}`, stream `logs:machine:<id>` |
| Services | `services.list` |
| Service detail | `services.byId`, stream `logs:service:<id>` |
| Functions | `bundles.list` → `functions.list{bundleId}` |
| Data | `tables.list` → `tables.browse{name, cursor}` |
| Live Logs | stream `events:all` with filter controls |
| Runs | `runs.recent` → `runs.byId` |
| Settings | `system.status` + token/shutdown mutations |

## Roadmap

### UI1 — Spec: WebSocket protocol and error schema

Write `docs/reference/websocket-protocol.md` and `docs/reference/errors.md`
before any UI code. These are the contracts that the UI, CLI, SDK, and any
future native shell implement against.

Covers: subprotocol negotiation, hello/client_hello frames, op types
(query.subscribe, mutation, action, stream.subscribe, ping), frame envelope
schema, ordering guarantee, backpressure rules, reconnection semantics,
error code taxonomy, error field contracts, per-channel wrapping
(HTTP vs WebSocket fatal vs in-session op error), and client rendering
contract.

**Verification:** specs reviewed, error code taxonomy covers all existing
`AppError` variants, JSON examples validate against a JSON Schema.

**Status:** `pending`

### UI2 — Server: localhost auth and token-gate

Implement the token file lifecycle, session cookie bootstrap, origin
allowlist, and the middleware stack on the WebSocket upgrade path.

Covers: `~/.neovex/auth/token` generation on first `neovex serve`, `0600`
permissions (ACL on Windows), `generation` counter, `neovex token rotate`
subcommand, signed session cookie on `/ui/` navigation, origin check
middleware, rejection ordering (origin → auth → protocol → accept), audit
log entry on every connection.

**Verification:** `make test` green, integration test proving: (a) missing
token → 401, (b) wrong token → 401, (c) non-allowlisted origin → 403,
(d) valid token → upgrade succeeds, (e) `token rotate` invalidates existing
sessions.

**Status:** `pending`

### UI3 — Server: protocol version negotiation and hello handshake

Implement `Sec-WebSocket-Protocol` negotiation, `hello` / `client_hello`
frame exchange, feature advertisement, and the 10-second `client_hello`
timeout.

Extends the existing `ws/mod.rs` upgrade handler. Preserves backward
compatibility with the current Convex-compatible subscribe/unsubscribe
framing by treating it as `neovex.v1`.

**Verification:** integration test proving: (a) no subprotocol overlap → 400
with structured body, (b) `hello` sent immediately after upgrade,
(c) `client_hello` timeout → close with `protocol.hello_timeout`,
(d) negotiated subprotocol echoed in upgrade response.

**Status:** `pending`

### UI4 — Server: embedded static asset serving at `/ui/*`

Add `rust-embed` to `neovex-server`, configure it to embed
`packages/neovex-ui/dist/`, serve at `/ui/*` with `index.html` fallback for
client-side routing.

Builds the SPA asset pipeline: `packages/neovex-ui` scaffolded with Vite,
React 19, Tailwind, placeholder `index.html` that renders "Neovex UI" and
connection state. CI builds JS before Rust so `dist/` is populated.

**Verification:** `neovex serve` → `curl localhost:PORT/ui/` returns HTML,
`curl localhost:PORT/ui/nonexistent` returns `index.html` (SPA fallback),
`npm run build` in `packages/neovex-ui` succeeds.

**Status:** `pending`

### UI5 — CLI: `neovex ui` subcommand and server discovery

Add `neovex ui` and `neovex ui --ensure` subcommands. Implement
`~/.neovex/run/server.json` write on `neovex serve` startup with
`RemoveOnDrop` guard, PID liveness check for stale file recovery, and
`open::that` for cross-platform browser launch.

**Verification:** (a) `neovex serve &` + `neovex ui` opens browser,
(b) `neovex ui` without server → clear error message,
(c) `neovex ui --ensure` without server → starts server then opens browser,
(d) kill server with SIGKILL → next `neovex serve` cleans stale file.

**Status:** `pending`

### UI6 — UI: scaffold and shell layout

Scaffold `packages/neovex-ui` with the full component stack: React 19,
shadcn/ui (Radix + Tailwind), Zustand, TanStack Router, Vite.

Build the shell layout: sidebar with nav links (Dashboard, Machines,
Services, Functions, Data, Logs, Runs, Settings), header with connection
state indicator (`NeovexProvider` + `useConnectionState`), error boundary
with the error schema's rendering contract.

**Verification:** `npm run build` succeeds, `npm run dev` serves locally,
sidebar navigation works, connection state indicator shows
connected/reconnecting/errored states, error boundary catches and renders
`severity: fatal` errors full-screen.

**Status:** `pending`

### UI7 — UI: dashboard tab

Build the dashboard landing page. Cards for: system status (uptime, version),
machine summary (count by state), service summary (count by state), recent
events (last 20), recent runs (last 10, with status indicators).

All data from `useQuery` against the internal query surface. No polling —
purely reactive via WebSocket subscriptions.

**Verification:** dashboard renders with live data, machine start/stop
reflected within one render cycle, events list updates in real time.

**Status:** `pending`

### UI8 — UI: machines tab (list + detail + actions)

Machine list with state badges and action buttons (start, stop, restart,
delete). Machine detail page with config, resource usage, service list
for that machine, and a log tail stream.

Optimistic updates on start/stop/restart — overlay applied immediately,
reconciled when server pushes updated `query.result`.

**Verification:** machine list shows all machines, state transitions reflected
immediately via optimistic update, log tail streams without gaps, action
errors render inline with retry button for `retryable: true` errors.

**Status:** `pending`

### UI9 — UI: services, functions, data, logs, runs tabs

Build the remaining tabs:
- **Services:** list + detail with health snapshot and log tail
- **Functions:** bundle list → function list per bundle, with kind/schema info
- **Data:** table list → row browser with pagination (`usePaginatedQuery`)
- **Logs:** live event stream with level/category/source filter controls
- **Runs:** recent runs list → run detail with trace viewer

**Verification:** each tab renders with live data, pagination works in data
browser, log filters apply without losing stream position, run trace viewer
shows timing waterfall.

**Status:** `pending`

### UI10 — UI: settings tab and token management

Settings page with: server info (version, uptime, address), token rotation
button (calls `system.tokenRotate` mutation, handles session invalidation
gracefully), shutdown button with confirmation.

**Verification:** token rotation triggers re-auth flow in the UI without
manual reload, shutdown mutation shows disconnect state.

**Status:** `pending`

### UI11 — Testing: unit, integration, and E2E

Establish the testing pyramid for `packages/neovex-ui`:

| Layer | Tool | What it tests | Speed |
| --- | --- | --- | --- |
| Unit | Vitest + JSDOM | Hooks, utilities, pure logic | Fast |
| Component | Vitest + React Testing Library | Component rendering, interaction | Fast |
| Integration | Vitest + mocked WebSocket | useQuery/useMutation against mock server | Medium |
| E2E | Playwright (browser mode) | Full flows against running `neovex serve` | Slow |

Adopt Podman Desktop's co-located test pattern: every `.tsx` file gets a
`.spec.tsx` beside it. This is the single biggest test-coverage driver in
their codebase — see `packages/main/src/plugin/` where every handler file
has a co-located spec.

Reference: `~/src/github.com/podman-desktop/podman-desktop/packages/main/src/`

Cover the error rendering matrix: one Storybook story per
`(severity × remediation.action)` combination (~12 stories total). Add
Storybook for all shadcn/ui components used, plus app-specific composites
(machine card, service health badge, log line, run status indicator, error
banner).

**Verification:** `npm run test` green, `npm run storybook` launches,
co-located specs exist for all route and component files, error stories
cover fatal/error/warning × with/without remediation.

**Status:** `pending`

## Phase 2: Electron Shell (future plan scope)

Documented here for architecture alignment so Phase 1 decisions do not
preclude Phase 2. A separate plan with its own activation gate will be
authored when Phase 1 is stable and users request native-app behavior (dock
icon, tray, auto-update, deep links).

**Activation gate:** Phase 1 UI7 (dashboard tab) shipped and stable.

### Why Electron, not Tauri

| Factor | Electron | Tauri 2 |
| --- | --- | --- |
| Linux rendering | Chromium — consistent | WebKitGTK — lags Chromium, distro version divergence, GPU issues (Spacedrive reported significant effort) |
| Auto-update on Linux | Supported via `electron-updater` (AppImage, deb, rpm) | No built-in Linux auto-update |
| Shell complexity | Thin — `loadURL(localhost)`, no Rust business logic needed | Thin — but Rust build pipeline adds friction for a pure-wrapper shell |
| Bundle size | ~150 MB (ships Chromium) | ~10 MB (uses OS WebView) |
| Maturity for "wrap localhost" | Proven at scale (Portainer, Rancher Desktop, Lens) | Proven at scale (Jan), but WebKitGTK is the tax |

The shell's only job is "open a window at localhost and give native chrome."
We get none of Tauri's Rust-integration benefits because all state lives in
`neovex-server`. What we care about is Chromium consistency on Linux, mature
auto-update, and predictable rendering. That's Electron's sweet spot. The
150 MB bundle is the price, and for a desktop app it's a price users already
pay for every tool in this category (VS Code, Slack, Discord, Podman Desktop).

Reference: Podman Desktop ships Electron 41 (Chromium 146, Node 24) with
this exact "thin shell over local engine" pattern.
See `~/src/github.com/podman-desktop/podman-desktop/packages/main/`.

### Electron security configuration

Modern Electron security defaults (Electron 20+, current Electron 41):

```javascript
new BrowserWindow({
  show: false,                             // show on 'ready-to-show' to prevent flash
  webPreferences: {
    preload: path.join(__dirname, 'preload.js'),
    contextIsolation: true,                // default since Electron 12
    nodeIntegration: false,                // default since Electron 5
    sandbox: true,                         // default since Electron 20
    webSecurity: true,                     // default — do NOT disable
  },
});
```

**Note:** Podman Desktop sets `webSecurity: false` in their `mainWindow.ts:61`
to load `file://` resources — we must NOT do this. Our shell loads
`http://localhost:PORT/ui/` exclusively, so `webSecurity: true` is correct
and sufficient.

Additional hardening (reference: Electron security checklist, 20 items):

- **Electron Fuses** (`@electron/fuses`): disable `RunAsNode`,
  `EnableNodeOptionsEnvironmentVariable`, `EnableNodeCliInspectArguments` at
  build time. Podman Desktop does this at
  `.electron-builder.config.cjs:62`.
- **Permission request handler**: deny all permissions except clipboard.
  Reference: Podman Desktop `packages/main/src/security-restrictions.ts:31-60`.
- **Navigation restriction**: `will-navigate` handler rejects all navigation
  except to the localhost UI origin. External links open in default browser
  via `shell.openExternal()`.
- **Window creation control**: `setWindowOpenHandler()` denies all
  `window.open()` calls.
- **CSP in the loaded page**: the server already sets CSP headers on `/ui/*`
  responses (see Phase 1 UI4). The Electron shell benefits from this
  without additional configuration.
- **Sender validation**: every `ipcMain.handle` validates
  `event.senderFrame.url` against the localhost origin before processing.

### IPC architecture

The shell is a thin wrapper — IPC surface is minimal:

```typescript
// preload.ts — expose only what the renderer needs for native-shell features
contextBridge.exposeInMainWorld('desktop', {
  // Window chrome
  minimize: () => ipcRenderer.invoke('window:minimize'),
  maximize: () => ipcRenderer.invoke('window:maximize'),
  close: () => ipcRenderer.invoke('window:close'),
  isMaximized: () => ipcRenderer.invoke('window:isMaximized'),

  // Tray / notifications
  setBadgeCount: (count: number) => ipcRenderer.send('tray:badge', count),

  // Auto-update
  checkForUpdates: () => ipcRenderer.invoke('updater:check'),
  onUpdateAvailable: (cb: (version: string) => void) => {
    const handler = (_e: unknown, v: string) => cb(v);
    ipcRenderer.on('updater:available', handler);
    return () => ipcRenderer.removeListener('updater:available', handler);
  },

  // Server lifecycle
  getServerStatus: () => ipcRenderer.invoke('server:status'),
  startServer: () => ipcRenderer.invoke('server:start'),

  // Platform info
  platform: process.platform,
});
```

Type safety via shared type definitions (avoid Podman Desktop's 2,724-line
preload problem):

```typescript
// shared/ipc-types.ts
export type IpcHandlers = {
  'window:minimize': () => void;
  'window:maximize': () => void;
  'window:close': () => void;
  'window:isMaximized': () => boolean;
  'updater:check': () => { available: boolean; version?: string };
  'server:status': () => { running: boolean; address?: string; pid?: number };
  'server:start': () => { address: string };
};
```

**All business logic flows through the WebSocket to `neovex-server`.** The IPC
surface handles only window chrome, tray, auto-update, and server lifecycle —
never queries, mutations, or data.

### Process model

```
┌─────────────────────────────────────────────────────┐
│  Electron Main Process                               │
│  - Window management, tray, menus                    │
│  - Auto-updater (electron-updater)                   │
│  - Server lifecycle (spawn/discover neovex serve)    │
│  - IPC handlers (~10 channels, typed)                │
├─────────────────────────────────────────────────────┤
│  Renderer Process (sandboxed)                        │
│  - loadURL('http://localhost:PORT/ui/')               │
│  - Same React SPA as browser — no Electron-specific  │
│    code in the renderer                              │
│  - WebSocket to neovex-server for all data           │
│  - window.desktop.* for native chrome only           │
├─────────────────────────────────────────────────────┤
│  neovex serve (child process or pre-existing)        │
│  - Discovered via ~/.neovex/run/server.json          │
│  - Or spawned as child if not running                │
│  - Health-checked via GET /health before loadURL     │
└─────────────────────────────────────────────────────┘
```

Use `child_process.spawn` (not `utilityProcess`) for the neovex server
because it is an external Rust binary, not a Node.js module.
`utilityProcess.fork()` requires a JS entry point.

Server lifecycle in the main process:

1. Read `~/.neovex/run/server.json` — if PID alive and `/health` responds,
   use that address.
2. If no server, show splash window with "Start Neovex" button.
3. On start: `spawn('neovex', ['serve', '--port', '0'])`, parse the bound
   address from stdout or poll `server.json`.
4. On `before-quit`: if we spawned the server, send SIGTERM, wait 5s, then
   SIGKILL. On macOS, handle `activate` event (re-show window without
   quitting).
5. On `window-all-closed`: on macOS, do NOT quit (dock behavior). On
   Windows/Linux, quit.

### Packaging per platform

Build tool: **Electron Forge** (maintained by the Electron core team under
`electron/forge`). Preferred over `electron-builder` for first-party feature
parity (ASAR integrity, Fuses) unless Linux auto-update via `electron-updater`
is needed, in which case use `electron-builder`.

**Comparison** (as of 2026):

| Factor | Electron Forge | electron-builder |
| --- | --- | --- |
| Maintainer | `electron/forge` (core team) | `electron-userland` (community) |
| Linux auto-update | No built-in | Yes (AppImage, deb, rpm) |
| Delta updates | No | Yes (NSIS on Windows) |
| Electron Fuses | First-class | Supported |
| Vite integration | `@electron-forge/plugin-vite` | Via `electron-vite` |

Recommendation: start with **electron-builder** for its Linux auto-update and
delta update capabilities. Switch to Forge if/when Forge gains feature parity.

#### macOS

- **Format:** DMG for direct download, ZIP for Sparkle/Squirrel auto-update
- **Architectures:** Universal binary (x64 + arm64) via `--arch=universal`
- **Code signing:** Hardened runtime via `osxSign` configuration
- **Notarization:** `notarytool` (not `altool`, which is deprecated) via
  `@electron/notarize`. Auth via App Store Connect API key (recommended for
  CI) or Apple ID + app-specific password.
  ```javascript
  // electron-builder config
  mac: {
    hardenedRuntime: true,
    gatekeeperAssess: false,
    target: [
      { target: 'dmg', arch: ['universal'] },
      { target: 'zip', arch: ['universal'] },
    ],
  },
  afterSign: 'scripts/notarize.js',
  ```
- **Dock behavior:** `app.dock.setBadge()` for unread counts, `app.dock.hide()`
  when minimized to tray (user preference).
- **About panel:** `app.setAboutPanelOptions({ applicationName, version, ... })`
- **Minimum OS:** macOS 11+ (Electron 33+ dropped macOS 10.15)

#### Windows

- **Format:** NSIS installer (supports auto-update, delta updates, per-user
  install without admin)
- **Architectures:** x64 + arm64
- **Code signing:** Azure Trusted Signing (lowest cost, eliminates SmartScreen
  warnings) or EV certificate via cloud HSM (DigiCert KeyLocker). All private
  keys must be on HSM — no local `.pfx` files.
  ```javascript
  // electron-builder config
  win: {
    target: [
      { target: 'nsis', arch: ['x64', 'arm64'] },
    ],
    sign: 'scripts/sign-windows.js', // Azure Trusted Signing wrapper
  },
  ```
- **Auto-update:** `electron-updater` with NSIS supports delta updates and
  staged rollouts via `stagingPercentage` in `latest.yml`.
- **Windows on ARM:** Electron supports ARM64 natively — no special config
  beyond targeting `arm64` during packaging.

#### Linux

- **Formats:** AppImage (universal, auto-update via `electron-updater`),
  deb (Debian/Ubuntu repos), rpm (Fedora/RHEL repos)
- **Architectures:** x64 + arm64
- **Desktop integration:** `.desktop` file, icons at standard XDG sizes,
  MIME type registration — handled automatically by deb/rpm makers, requires
  manual setup for AppImage.
- **Wayland:** Electron 28+ uses the Ozone platform abstraction by default.
  Pass `--ozone-platform-hint=auto` via `app.commandLine.appendSwitch()` to
  auto-detect Wayland vs X11. Electron 36+ defaults to GTK 4 on GNOME.
- **GPU acceleration:** inconsistent across Mesa/NVIDIA drivers. Provide
  `--disable-gpu` fallback flag and document it.
- **Tray icon:** behavior varies wildly across GNOME (requires AppIndicator
  extension), KDE (native support), XFCE. Make tray optional — the app
  should work without it. Check `Tray.isSupported()` before creating.
- **Flatpak/Snap:** deferred to post-launch. Flatpak requires careful
  sandbox permission declarations (reference: Podman Desktop
  `.electron-builder.config.cjs:200-238` for their Flatpak permissions).

### Auto-update strategy

Use `electron-updater` with GitHub Releases as the update provider:

```javascript
// main process
import { autoUpdater } from 'electron-updater';

autoUpdater.autoDownload = false;  // let user choose
autoUpdater.on('update-available', (info) => {
  mainWindow.webContents.send('updater:available', info.version);
});
// User clicks "Update" in UI → ipcMain.handle('updater:download', ...)
```

Update flow:
1. Check for updates on launch + every 4 hours
2. Notify renderer via IPC → UI shows "Update available" banner
3. User clicks "Update" → download in background, show progress
4. On download complete → prompt restart
5. On restart → install update, relaunch

### Repo structure

Separate repo: `agentstation/neovex-desktop` (or post-rename equivalent).

```
neovex-desktop/
├── package.json              # electron, electron-builder, electron-updater
├── electron-builder.yml      # packaging config
├── tsconfig.json
├── tsconfig.node.json        # main/preload (Node target)
├── tsconfig.web.json         # renderer (DOM target, if any Electron-specific UI)
├── src/
│   ├── main/
│   │   ├── index.ts          # app lifecycle, window creation
│   │   ├── server.ts         # neovex serve lifecycle management
│   │   ├── ipc.ts            # IPC handler registrations (~10 channels)
│   │   ├── menu.ts           # native menus (macOS/Windows/Linux variants)
│   │   ├── tray.ts           # tray icon (optional, check isSupported)
│   │   ├── updater.ts        # auto-update logic
│   │   └── security.ts       # permission handler, navigation restriction, Fuses
│   ├── preload/
│   │   └── index.ts          # contextBridge — thin, <100 lines
│   └── shared/
│       └── ipc-types.ts      # shared type definitions
├── scripts/
│   ├── notarize.js           # macOS notarization hook
│   └── sign-windows.js       # Windows Azure Trusted Signing hook
├── buildResources/           # icons (icns, ico, png at XDG sizes)
└── .github/workflows/
    └── release.yml           # CI: build + sign + notarize + publish
```

No renderer source — the Electron shell loads
`http://localhost:PORT/ui/` directly. The renderer is the same SPA served by
`neovex-server`. The preload exposes `window.desktop.*` for native chrome
only.

### Testing for the Electron shell

| Layer | Tool | What it tests |
| --- | --- | --- |
| Unit | Vitest (Node target) | Server lifecycle, IPC handler logic, platform detection |
| E2E | Playwright for Electron | App launches, connects to server, window chrome works |

Reference: Playwright's Electron support (`@playwright/test` 1.52+):
```typescript
import { test, _electron as electron } from '@playwright/test';

test('app launches and connects', async () => {
  const app = await electron.launch({ args: ['.'] });
  const window = await app.firstWindow();
  await expect(window).toHaveTitle(/Neovex/);
  await app.close();
});
```

Spectron is deprecated (archived 2022). Playwright is the official
replacement.

### Build pipeline

1. **CI matrix:** macOS (arm64 runner), Windows (x64 runner), Linux (x64
   runner). Ubuntu 22.04+ for Linux builds.
2. **Build order:** `npm install` → `npm run build` (Vite bundles
   main/preload) → `electron-builder` packages the `out/` directory →
   platform-specific makers generate installers → code signing →
   notarization (macOS) → publish to GitHub Releases.
3. **Vite configuration:** use `electron-vite` for unified Vite config
   across main/preload contexts. Renderer is not bundled by Vite — it is
   loaded from the neovex server.
   ```typescript
   // electron.vite.config.ts
   import { defineConfig, externalizeDepsPlugin } from 'electron-vite';
   export default defineConfig({
     main: { plugins: [externalizeDepsPlugin()] },
     preload: { plugins: [externalizeDepsPlugin()] },
   });
   ```
4. **Electron version:** target the latest stable (currently Electron 41,
   Chromium 146, Node 24). Pin to an even-numbered Chromium release per
   Electron's 8-week cycle.

### Platform-specific behavior matrix

| Behavior | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Window close | Hide to dock (default Mac behavior) | Quit (unless tray enabled) | Quit (unless tray enabled) |
| Tray icon | Menu bar extra | System tray | AppIndicator (GNOME needs extension) — check `isSupported()` |
| `window-all-closed` | Do NOT quit (`app.on` no-op) | Quit | Quit |
| `activate` event | Re-show window | N/A | N/A |
| Native menus | Standard macOS menu bar (app name, Edit, View, Window, Help) | In-window menu bar | In-window menu bar |
| About dialog | `app.setAboutPanelOptions()` | Custom window | Custom window |
| File paths | `~/Library/Application Support/neovex/` | `%APPDATA%/neovex/` | `$XDG_CONFIG_HOME/neovex/` or `~/.config/neovex/` |
| Auto-update | DMG/ZIP via Squirrel or electron-updater | NSIS via electron-updater | AppImage via electron-updater |
| GPU fallback | Rarely needed | Rarely needed | `--disable-gpu` flag documented |
| Display scaling | Retina handled automatically | DPI-aware by default | Wayland DPI handled by Ozone; X11 may need `--force-device-scale-factor` |

## Implementation References

When implementing roadmap items, consult these specific files in the cloned
reference repos for proven patterns:

| Task | Reference file | What to study |
| --- | --- | --- |
| Electron security setup | `~/src/github.com/podman-desktop/podman-desktop/packages/main/src/security-restrictions.ts` | Origin allowlist, permission handler, navigation restriction |
| Electron Fuses | `~/src/github.com/podman-desktop/podman-desktop/.electron-builder.config.cjs` (line 62) | Build-time Fuse configuration |
| IPC typing pattern | `~/src/github.com/podman-desktop/podman-desktop/packages/preload/src/index.ts` | What a preload looks like (cautionary — too large at 2,724 lines) |
| IPC error serialization | `~/src/github.com/podman-desktop/podman-desktop/packages/main/src/plugin/index.ts` (line 303) | `ipcHandle()` wrapper that catches and serializes errors |
| Co-located test pattern | `~/src/github.com/podman-desktop/podman-desktop/packages/main/src/plugin/` | Every `.ts` has a `.spec.ts` beside it |
| Cross-platform packaging | `~/src/github.com/podman-desktop/podman-desktop/.electron-builder.config.cjs` | DMG/NSIS/Flatpak config with Fuses, notarization, code signing |
| Service Hub abstraction | `~/src/github.com/janhq/jan/web-app/src/services/index.ts` | Platform abstraction (desktop/mobile/web) behind typed interfaces |
| Embedded proxy server | `~/src/github.com/janhq/jan/src-tauri/src/core/server/proxy.rs` | Rust HTTP server inside app process |
| React + Radix + Tailwind | `~/src/github.com/janhq/jan/web-app/src/components/` | Component patterns with Radix UI primitives |
| TanStack Router | `~/src/github.com/janhq/jan/web-app/src/routes/` | File-based routing setup |
| Zustand stores | `~/src/github.com/janhq/jan/web-app/src/stores/` | Minimal state management pattern |
| CSP for localhost | `~/src/github.com/janhq/jan/src-tauri/tauri.conf.json` (lines 21-30) | CSP directives allowing localhost WebSocket |

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-18 | Plan authored | — | Architecture designed from Opus 4.7 session; reference implementations researched |
| 2026-04-18 | Deep research audit | — | Cloned podman-desktop and jan repos; audited Electron 41 security model, IPC patterns, packaging, testing; updated Phase 2 with full Electron architecture, platform-specific details, and implementation references |
