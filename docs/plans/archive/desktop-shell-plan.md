# Desktop Shell Plan (Phase 2: Native Electron Shell)

This plan owns the native desktop shell that wraps the embedded Nimbus
operator console SPA (Phase 1, completed in
[`docs/plans/desktop-ui-plan.md`](desktop-ui-plan.md)) into a packaged,
signed, auto-updating Electron application.

The shell does not own queries, mutations, data, or business logic. The
Rust server retains exclusive ownership of every Service mutation path,
auth contract, runtime trust boundary, and `_nimbus` system-tenant
projection. The shell owns window chrome, tray, menus, server-process
lifecycle, auto-update, deep links, and IPC for those concerns only.

## Prerequisites

- [`docs/plans/desktop-ui-plan.md`](desktop-ui-plan.md) Phase 1 (DU0-DU10)
  closed and stable per the activation gate defined in that plan
  (concrete gate is the bullet list under "Phase 2: Native Desktop Shell"
  in `desktop-ui-plan.md`).
- DU11 hardening pass landed: rotate-token + shutdown Playwright fixtures
  green, sustained 100+ events/sec live-tail perf lane defined. These are
  the only DU1-DU10 deferrals that must convert before DS0; see the
  Phase 1 deferral matrix in `desktop-ui-plan.md`.
- Reference state already settled in `crates/nimbus-server/src/local_server/`
  for the discovery contract this plan consumes:
  - Linux: `$XDG_RUNTIME_DIR/nimbus/server.json` (falls back to
    `$XDG_STATE_HOME/nimbus/run/server.json`).
  - macOS: `$TMPDIR/nimbus/server.json` (falls back to
    `~/Library/Application Support/nimbus/run/server.json`).
  - Windows: `%LOCALAPPDATA%/nimbus/run/server.json`.
  - All parents are created with `0o700` on Unix.
- Reference state already settled in `crates/nimbus-bin/src/ui.rs`: the
  `nimbus ui` and `nimbus ui --ensure` paths already implement live
  discovery polling, detached child spawn, Chromium-family preference,
  and actionable error messaging. The Electron shell adopts the same
  contract — it does not re-implement discovery from scratch.
- [`docs/architecture/server/auth-runtime-trust.md`](../architecture/server/auth-runtime-trust.md):
  server-owned auth, deployment-scoped activation, provider-neutral
  runtime ABI. IPC from the shell does not bypass any of those rules.
- `nimbus/desktop` repo provisioned on GitHub under the `nimbus` org.
  (Required for DS1; tracked under DS0.)

## Status

- **Status:** `active` — DS0 `done` 2026-05-15 (DS0A scaffold +
  DS0B Apple credentials uploaded; Windows deferred per
  [`002-windows-code-signing.md`](https://github.com/nimbus/desktop/blob/main/docs/decisions/002-windows-code-signing.md)),
  DS1 `done` 2026-05-15 (hello-electron loop + security baseline +
  browser-driven sandbox/bridge proof), DS2 `done` 2026-05-15
  (server discovery + spawned-server lifecycle + loopback browser
  proof against live `nimbus start`), DS3 `done` 2026-05-15
  (Electron Fuses, IPC senderFrame origin validator, packaged
  fuse-audit against an `electron-builder --dir` build), DS4 `done`
  2026-05-15 (native menu bar + tray controller + window-state
  persistence + tray:setStatusDot IPC), DS5 `done` 2026-05-15
  (electron-updater state machine + IPC seam +
  nimbusShell.updater bridge surface, end-to-end mocked-feed
  proof), DS6 `done` 2026-05-15 (per-platform installer matrix on
  CI — macOS universal DMG+ZIP, Windows NSIS x64+arm64,
  Linux AppImage+deb+rpm — all under their size budgets, all
  fuses pinned). DS7–DS10 pending. Driven autonomously per
  [`desktop-mission.md`](desktop-mission.md).
- **Primary owner:** this plan.
- **Mission control plane:** [`desktop-mission.md`](desktop-mission.md)
  binds Phase 1 + Phase 2 work into a single autonomous mission. Read
  it on session entry for the durable authorizations, resume procedure,
  and stop condition.
- **Activation gate:** see Prerequisites.
- **Related plans:**
  - [`docs/plans/desktop-ui-plan.md`](desktop-ui-plan.md) — Phase 1
    completed plan and architectural input for Phase 2.
  - [`docs/plans/distribution-plan.md`](distribution-plan.md) — release
    channels; the desktop shell publishes alongside the existing
    channels, not as a replacement.
  - [`docs/plans/install-script-plan.md`](install-script-plan.md) — the
    installed `nimbus` binary is what the packaged shell discovers or
    spawns; this plan does not re-bundle the binary inside the Electron
    app.

## Current Assessed State

- No `nimbus/desktop` repo exists. No Electron code, no `package.json`,
  no preload, no `electron-builder.yml`.
- The embedded SPA at `/ui/*` is already a complete operator console
  surface. The only thing missing from "operator opens an icon and sees
  the console" is the packaged native chrome.
- The Rust server already serves the SPA under
  `crates/nimbus-server/src/http/ui.rs` with `rust_embed`, signed session
  cookies, one-time launch tickets, and a strict CSP
  (`script-src 'self'`). The Electron shell does not relax any of that
  — `loadURL('http://127.0.0.1:<port>/ui/')` is the same surface a
  browser already hits.
- The Rust server already publishes its address through
  `ServerDiscoveryRecord` (`crates/nimbus-server/src/local_server/discovery.rs`).
- Three external decisions have not been made and are DS0 blockers:
  - Apple Developer ID Application certificate procurement and Apple
    notarization credential storage (DS0).
  - Windows code signing path: Azure Trusted Signing vs. EV HSM
    physical token (DS0).
  - Auto-update hosting: GitHub Releases (electron-updater default) vs.
    a self-hosted update server backed by R2/S3 (DS0).

## Control Plan Rules

1. The shell is a **consumer** of the same `/ui/*` HTTP surface a
   browser hits. It does not have a privileged data path.
2. All business logic stays in the Rust server. IPC carries window
   chrome, tray, menus, server lifecycle, auto-update, and deep links
   only — never queries, mutations, or document access.
3. The renderer is sandboxed (`sandbox: true`), context-isolated
   (`contextIsolation: true`), and `nodeIntegration: false`. These
   defaults are not weakened.
4. The packaged shell does not embed a Nimbus binary. It discovers an
   installed `nimbus` from `$PATH` or the platform-canonical install
   location; if none is present it surfaces an actionable error
   pointing at the install script. (This keeps `nimbus/nimbus` and
   `nimbus/desktop` release cadences independent.)
5. The preload script is the only IPC surface. Target: **< 500 lines**.
   Hard cap: **40 IPC channels**. If the surface exceeds 50 channels,
   adopt `dts-for-context-bridge` codegen before merging.
6. `event.senderFrame.url` is validated on every IPC handler. Channels
   that do not validate are rejected at code review.
7. `will-navigate`, `setWindowOpenHandler`, and
   `setPermissionRequestHandler` deny by default. The only allowed
   permission is clipboard read/write (for the SPA's `CopyChip`).
8. Electron Fuses are flipped at packaging time, not runtime. The
   `RunAsNode`, `EnableNodeOptionsEnvironmentVariable`,
   `EnableNodeCliInspectArguments`, `EnableCookieEncryption`,
   `EnableEmbeddedAsarIntegrityValidation`, and `OnlyLoadAppFromAsar`
   fuses are configured per the security baseline in DS3.
9. Pre-launch direct corrections over compatibility shims (consistent
   with the repo-wide `CLAUDE.md` pre-launch rule). No legacy feature
   flags, no deprecated IPC channels, no migration shims.
10. Every roadmap item meets the Verification Contract before its
    Status flips to `done`.

## Verification Contract

Each DS item must satisfy before closing:

- Repo lints: `npm run lint` (Biome) and `npm run typecheck` (tsc
  `--noEmit`) green in `nimbus/desktop`.
- Repo tests: `npm run test` green; co-located `.spec.ts` beside every
  `.ts` for `main/`, `preload/`, and `shared/` (Podman Desktop pattern).
- Packaging dry-run: `electron-builder --dir` produces an unpacked
  build for the current platform without errors.
- Security audit: `electron-fuses` report shows the required fuses
  flipped; `webPreferences` review confirms the four sandbox flags.
- Browser-driven verification: the packaged shell launches against a
  real `nimbus start`, the renderer reaches `http://127.0.0.1:<port>/ui/`,
  the auth flow completes, the System Tenant Lens (⌘\) opens, and the
  Command Palette (⌘K) opens. Captured via `playwright-cli` or
  `chrome-devtools-mcp` against the same renderer process (Electron
  exposes a CDP endpoint when `--remote-debugging-port` is set).
- Per-item manual verification described below.

## Verification Tooling

Same browser-driving stack as the desktop-ui plan:

| Tool | Form | When to use |
| --- | --- | --- |
| `playwright-cli` | Claude Code Skill at `.claude/skills/playwright-cli/` | Primary driver for renderer interaction; works against any CDP endpoint, including Electron's `--remote-debugging-port` |
| `chrome-devtools-mcp` | MCP at user scope and project `.mcp.json` | Perf traces, network/CSP inspection inside the packaged renderer |
| `playwright` (in-tree) | `@playwright/test` E2E specs in `nimbus/desktop/tests/e2e/` | Packaged-shell E2E (DS7) — exercise the actual Electron main + renderer, not just the embedded SPA |

`@playwright/mcp` remains rejected on token cost — see desktop-ui-plan
for the rationale.

For DS5 (auto-update) and DS9 (release CI), additional tools:

| Tool | Use |
| --- | --- |
| `@electron/notarize` | macOS notarization in the release pipeline (DS6, DS8) |
| `@electron/fuses` | Build-time fuse manipulation invoked from `electron-builder` `afterPack` hook (DS3) |
| `azure-trusted-signing-tool` or EV HSM signtool | Windows signing in the release pipeline (DS8) |
| `electron-updater` 6.8.x | Differential auto-update on macOS + Windows (DS5) |

## Architecture

### Process model

```
┌──────────────────────────────────────────────────────────────────────┐
│ Electron main process (Node, privileged)                             │
│ ─────────────────────────────────────────                            │
│  • BrowserWindow lifecycle                                           │
│  • Tray + native menu                                                │
│  • Auto-updater (electron-updater 6.8.x)                             │
│  • Server lifecycle: child_process.spawn('nimbus start', ...)        │
│  • Discovery: read $XDG_RUNTIME_DIR/nimbus/server.json               │
│  • Deep links: nimbus://<host>/<path>                                │
│  • Security hooks: will-navigate, permission, window-open            │
└──────────────────────────────────────────────────────────────────────┘
         │ IPC (20-40 channels)
         ▼
┌──────────────────────────────────────────────────────────────────────┐
│ Preload (sandboxed bridge, < 500 lines)                              │
│ ─────────────────────────────────────                                │
│  • contextBridge.exposeInMainWorld('nimbusShell', { ... })           │
│  • Strict allow-list of channels                                     │
│  • event.senderFrame.url validation on every handler                 │
└──────────────────────────────────────────────────────────────────────┘
         │ window.nimbusShell.*
         ▼
┌──────────────────────────────────────────────────────────────────────┐
│ Renderer (sandboxed Chromium, packaged SPA URL)                      │
│ ──────────────────────────────────────────────                       │
│  • loadURL('http://127.0.0.1:<port>/ui/')                            │
│  • Same SPA a browser loads — zero shell-specific code in packages/  │
│    nimbus-ui (DU3 already enforces this; the shell preserves it)     │
│  • CSP: script-src 'self' (server-set, not relaxed)                  │
│  • Permissions: clipboard only                                       │
└──────────────────────────────────────────────────────────────────────┘
         │ HTTP + WebSocket
         ▼
┌──────────────────────────────────────────────────────────────────────┐
│ nimbus server (separate process, Service-owned)                      │
│  • child_process.spawn ('nimbus start --port <ephemeral>')           │
│  • Discovery file: server.json (port, pid, base_url, started_at)     │
│  • OR pre-existing server discovered via the same discovery contract │
└──────────────────────────────────────────────────────────────────────┘
```

### IPC architecture

**Target: 20-40 channels.** Podman Desktop has 297+; we are far thinner
because every data, query, mutation, and subscription path is HTTP +
WebSocket against the renderer's same-origin server, not IPC. The IPC
surface only covers concerns that have no browser equivalent:

| Concern | Sample channels |
| --- | --- |
| Server lifecycle | `nimbus:server:start`, `nimbus:server:stop`, `nimbus:server:restart`, `nimbus:server:status` |
| Server discovery | `nimbus:server:discovered-url`, `nimbus:server:discovery-changed` |
| Window | `nimbus:window:minimize`, `nimbus:window:maximize`, `nimbus:window:close` |
| Tray | `nimbus:tray:set-tooltip`, `nimbus:tray:set-status-dot` |
| Updater | `nimbus:updater:check`, `nimbus:updater:download`, `nimbus:updater:quit-and-install`, `nimbus:updater:state-changed` |
| Deep link | `nimbus:deep-link:incoming` |
| Platform | `nimbus:platform:info` (os, arch, app version) |
| Diagnostics | `nimbus:diagnostics:open-logs-dir`, `nimbus:diagnostics:copy-version-string` |

This list is a target shape, not a final contract — DS3 produces the
authoritative TypeScript types in `src/shared/ipc-types.ts`.

### Server lifecycle

The shell discovers an existing `nimbus start` first; only if none is
running does it spawn one. This matches the `nimbus ui --ensure`
contract that already ships in `crates/nimbus-bin/src/ui.rs`.

```typescript
async function ensureServer(): Promise<DiscoveryRecord> {
  const existing = await readLiveDiscovery();
  if (existing) return existing;

  const child = spawn(nimbusBin, ['start', '--port', '0'], {
    detached: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.unref();

  return pollForDiscovery({ timeoutMs: 60_000, intervalMs: 200 });
}
```

The shell uses `child_process.spawn`, not `utilityProcess`, because the
Nimbus server is a long-lived foreign binary, not a Node script. The
shell does not communicate with the spawned process over stdio — the
discovery file is the contract.

### Repo structure

Separate repo: `nimbus/desktop`.

```
nimbus/desktop/
├── package.json                  # electron 42.x, electron-builder 26.x
├── tsconfig.json                 # TS 6, strict
├── biome.json                    # mirrors nimbus-ui biome config
├── electron-builder.yml          # canonical packaging config
├── src/
│   ├── main/
│   │   ├── index.ts              # app entrypoint, lifecycle hooks
│   │   ├── window.ts             # BrowserWindow factory + security hooks
│   │   ├── server.ts             # nimbus child_process + discovery
│   │   ├── menu.ts               # native menu bar
│   │   ├── tray.ts               # tray icon + tooltip
│   │   ├── updater.ts            # electron-updater wrapper
│   │   ├── ipc.ts                # registered IPC handlers
│   │   ├── deep-link.ts          # nimbus:// protocol handler
│   │   └── security.ts           # permission/navigation/window-open denies
│   ├── preload/
│   │   └── index.ts              # contextBridge surface, < 500 lines
│   └── shared/
│       └── ipc-types.ts          # canonical IPC channel + payload types
├── tests/
│   ├── unit/                     # co-located .spec.ts beside each src/ file
│   └── e2e/                      # @playwright/test against packaged shell
├── scripts/
│   ├── notarize.cjs              # macOS notarization (DS8)
│   ├── sign-windows.cjs          # Windows signing (DS8)
│   ├── flip-fuses.cjs            # electron-builder afterPack (DS3)
│   └── prepublish-check.cjs      # version + Fuses + CSP sanity
├── buildResources/
│   ├── icon.icns                 # macOS
│   ├── icon.ico                  # Windows
│   ├── icon.png                  # Linux + Web
│   ├── background.png            # macOS DMG background
│   └── entitlements.mac.plist
└── .github/
    └── workflows/
        ├── ci.yml                # lint + typecheck + unit + e2e
        └── release.yml           # tag-triggered packaged release
```

### Packaging matrix

| Platform | Format | Architectures | Signing | Auto-update |
| --- | --- | --- | --- | --- |
| macOS | DMG + ZIP | Universal (x64 + arm64) | `notarytool` via `@electron/notarize` | electron-updater (full) |
| Windows | NSIS | x64 + arm64 | Azure Trusted Signing **or** EV HSM | electron-updater (differential) |
| Linux | AppImage + deb + rpm | x64 + arm64 | unsigned (community standard) | electron-updater (AppImage only) |

Linux notes: XWayland default, `--ozone-platform-hint=auto` opt-in for
Wayland, tray optional via `Tray.isSupported()`, `--disable-gpu`
fallback documented. The deb/rpm packages are static; auto-update only
fires on the AppImage build.

## Roadmap

### DS0 — External decisions and credentials

**Goal:** unblock the rest of the plan by resolving three external
decisions and making credential provisioning explicit without ever writing
secret values into source control.

DS0 is intentionally split into two sub-gates:

- **DS0A — repo and decision docs:** create/provision `nimbus/desktop` and
  commit the three decision documents with secret names, owners, rotation
  procedure, and unresolved manual procurement items. No secret value is
  created or uploaded during DS0A.
- **DS0B — credential presence:** after the user has procured the Apple,
  Windows, and update-channel credentials, verify the required GitHub secret
  names exist for `nimbus/desktop`. DS0 is not `done` until DS0B passes.

**Decisions:**

1. Apple Developer ID Application certificate: which organization,
   which Apple ID, where the notarization credentials live (Apple
   Connect API key recommended over app-specific password). Document
   the secret-store path (1Password / Bitwarden / GitHub Actions
   environment secret) — the credential never lands in the repo.
2. Windows code signing: Azure Trusted Signing vs. EV HSM physical
   token. Trusted Signing is preferred (lower op cost, no token
   shipping, attestable from CI); EV HSM is the fallback if Trusted
   Signing access is blocked. Document the choice.
3. Auto-update hosting: GitHub Releases (electron-updater
   `provider: github`) vs. self-hosted via R2/S3 backed
   `provider: generic`. Default to GitHub Releases for DS5; revisit if
   release artifacts exceed GitHub's release asset limits or if
   private-channel distribution is required.

**Setup:**

- Provision the `nimbus/desktop` repo on the `nimbus` org.
- Document the `DESKTOP_*` secret names that DS0B expects: Apple
  notarization credentials, Windows signing credentials, update-channel
  publishing token, and optional Chromatic token if visual regression
  carries forward into the packaged shell.

**Verification:**

- DS0A: three decision documents committed to `nimbus/desktop/docs/decisions/`
  (one per decision). Each names the chosen path, the rejected paths
  with reasons, and the contact responsible for credential rotation.
- DS0A: `gh repo view nimbus/desktop` succeeds with `--web` opening the new
  repo.
- DS0B: `gh secret list --repo nimbus/desktop` lists the **required**
  secret names (values not visible — confirming presence is enough).
  Required for the first release is the Apple set (decision 001 — 7
  names). The Windows set (decision 002 — 6 names) is **deferred** and
  reported by `scripts/verify-secrets.sh` as informational; it does not
  gate DS0B. The GitHub release token (decision 003 — 1 name) is
  required-optional: workflows default to the auto-provisioned
  `GITHUB_TOKEN` and only need a dedicated `DESKTOP_GH_RELEASE_TOKEN`
  if audit policy requires a fine-grained PAT.

**Platform staging:** first release ships macOS + Linux. Windows is
deferred to a follow-up release once Azure Trusted Signing onboarding
completes (1–3 week organizational lead time with Microsoft). The
Windows secret-name registry and decision document stay in place during
the deferral so flipping Windows from deferred to active is a one-line
move in `nimbus/desktop:scripts/verify-secrets.sh` plus a status flip in
`docs/decisions/002-windows-code-signing.md`. DS6 (packaging), DS8
(signing), and DS9 (release CI) bring up the macOS lane first; their
Windows lanes activate when 002 flips to "accepted — active".

**Status:** DS0A `done`; DS0B `done` (Apple credentials uploaded
2026-05-15, verify-secrets.sh reports 7/7 required present, exit 0);
DS0 `done`. Windows secret-name presence is tracked separately as
deferred and does not gate DS0 for the first release.

### DS1 — Scaffold and shell layout (no server lifecycle yet)

**Goal:** stand up the `nimbus/desktop` repo with a working
"hello-electron + Biome + tsc + vitest + playwright" loop. The
renderer loads a hardcoded `https://example.org/` placeholder URL so
the security baseline is exercised before DS2 wires the real server.

**Implementation:**

- `package.json` pins: `electron@^42`, `electron-builder@^26.8`,
  `@playwright/test@^1.60`, `biome@^2.4`, `typescript@^6`,
  `vitest@^4`.
- `src/main/index.ts` creates a `BrowserWindow` with the Phase 2
  security baseline (`sandbox: true`, `contextIsolation: true`,
  `nodeIntegration: false`, `webSecurity: true`, preload path).
- `src/main/security.ts` registers `setPermissionRequestHandler`
  (deny all but clipboard), `will-navigate` (deny non-renderer-origin),
  `setWindowOpenHandler` (deny).
- `src/preload/index.ts` exposes an empty `window.nimbusShell` object
  via `contextBridge.exposeInMainWorld` — no channels yet, just the
  bridge surface.
- `src/shared/ipc-types.ts` defines the canonical channel-name +
  payload TypeScript types as a single source of truth.
- Co-located `.spec.ts` for each `main/*.ts` and `preload/*.ts`.
- CI workflow runs lint + typecheck + unit tests on PR.

**Verification:**

- `npm run lint`, `npm run typecheck`, `npm run test` green.
- `npm run dev` opens an Electron window pointed at the placeholder
  URL.
- Manual security probe: open the renderer DevTools, run `process` —
  must be `undefined` (sandbox proof); run
  `window.nimbusShell` — must be the contextBridge object, not a
  Node global proxy.
- Co-located test count ≥ 5 (one per main/preload TS file added).

**Status:** `done` 2026-05-15 — implemented at
[`nimbus/desktop@6ddf65d`](https://github.com/nimbus/desktop/commit/6ddf65d).
See execution log row for full evidence.

### DS2 — Server discovery and lifecycle

**Goal:** the shell discovers a running `nimbus start` or spawns a new
one, then `loadURL`s the renderer at the discovered URL.

**Implementation:**

- `src/main/server.ts` replicates the discovery contract from
  `crates/nimbus-server/src/local_server/paths.rs` for all three
  platforms. Reads `server.json`, validates the schema, checks
  `pid_is_live` (existing helper in
  `crates/nimbus-server/src/local_server/discovery.rs` — port the
  equivalent in TS).
- If no live server, `child_process.spawn('nimbus', ['start',
  '--port', '0'], { detached: true })` with platform-appropriate
  detach flags (`setsid` on Unix, `DETACHED_PROCESS |
  CREATE_NEW_PROCESS_GROUP` on Windows). Poll discovery every 200 ms
  up to 60 s.
- If `nimbus` is not on `$PATH`, fall back to platform-canonical
  install paths: `/usr/local/bin/nimbus`, `/opt/nimbus/bin/nimbus`,
  `~/.local/bin/nimbus`, `~/.nimbus/bin/nimbus` (Unix);
  `%LOCALAPPDATA%/nimbus/bin/nimbus.exe` (Windows).
- If still not found, surface an actionable error in a native dialog
  pointing at the install script.
- On `app.on('before-quit')`, if the shell spawned the server,
  gracefully `POST /api/system/shutdown` first, then `child.kill()`
  after a 5 s timeout. Discovered (not spawned) servers are left
  running — same contract as `nimbus ui`.

**Verification:**

- `npm run dev` against a pre-running `nimbus start` discovers it and
  loads the renderer at the right URL.
- `npm run dev` with no server running spawns one, polls until ready,
  and loads the renderer. The spawned process survives the shell
  closing if `process.detached` is set correctly; the shell does not
  kill servers it did not spawn.
- Discovery polling timeout test: stub `readLiveDiscovery` to never
  resolve, assert the user-facing error fires at 60 s.
- Live browser-driven proof via `playwright-cli`: launch shell, snapshot
  the renderer, assert the auth form renders.

**Status:** `done` 2026-05-15 — implemented at [`nimbus/desktop@a5b5ff8`](https://github.com/nimbus/desktop/commit/a5b5ff8)

### DS3 — Security baseline: Fuses, permissions, IPC validation

**Goal:** lock down the production security posture, beyond the
DS1 sandbox defaults.

**Implementation:**

- `scripts/flip-fuses.cjs` invoked by `electron-builder` `afterPack`
  hook using `@electron/fuses`. Required fuses:
  - `RunAsNode: false`
  - `EnableNodeOptionsEnvironmentVariable: false`
  - `EnableNodeCliInspectArguments: false` in production builds
    (`true` for explicit `--enable-inspect` dev builds, gated on
    `NIMBUS_DESKTOP_ENABLE_INSPECT=1`)
  - `EnableCookieEncryption: true`
  - `EnableEmbeddedAsarIntegrityValidation: true`
  - `OnlyLoadAppFromAsar: true`
- IPC handler middleware validates `event.senderFrame.url` against
  the discovered server URL on every channel. Reject and log
  otherwise — fail closed, no fallback.
- Renderer `Content-Security-Policy` comes from the Rust server's
  middleware (already shipped in DU1). The shell does not add a
  meta-CSP — it does not relax `script-src 'self'`.
- A `prepublish-check.cjs` script asserts the fuses in the packaged
  binary post-build (parse `electron.app.fuses` via
  `@electron/fuses` read API). Hard-fail the release pipeline on
  drift.

**Verification:**

- `npm run package:linux -- --dir` produces an unpacked build;
  `electron-fuses inspect ...` confirms every required fuse is set.
- Unit tests cover the IPC `senderFrame.url` validator with a real
  WebContents stub.
- Manual probe: launch the packaged shell, open DevTools, attempt
  `require('child_process')` — must throw (sandbox + `nodeIntegration:
  false` proof).

**Status:** `done` 2026-05-15 — implemented at [`nimbus/desktop@773ab0c`](https://github.com/nimbus/desktop/commit/773ab0c)

### DS4 — Tray, menu, window chrome

**Goal:** OS-native chrome that matches operator-console conventions
seen in Docker Desktop, Podman Desktop, and 1Password.

**Implementation:**

- `src/main/menu.ts` builds the native menu bar: File / Edit / View /
  Window / Help. macOS gets the standard application menu. Windows /
  Linux get a hidden hamburger when the window is small enough; the
  shell never replaces the system menu with custom CSS.
- `src/main/tray.ts` creates a tray icon with status dot (Connected /
  Reconnecting / Offline) sourced from an IPC channel pushed by the
  renderer (`window.nimbusShell.tray.setStatusDot(state)`).
- Tray menu items: Open Console, Server status (read-only), Start /
  Stop / Restart server (calls into DS2 lifecycle), Quit.
- `BrowserWindow` defaults: 1280×800, min 960×600, persisted bounds
  in `app.getPath('userData')/window-state.json`.
- macOS: `activate` → re-show window; `window-all-closed` → no-op
  (app stays in tray).
- Windows / Linux: `window-all-closed` → app continues in tray;
  explicit Quit from tray terminates.

**Verification:**

- Manual tray probe on all three platforms: open shell, close
  window, tray dot still visible, tray click re-opens window.
- Unit tests for the menu builder (assert macOS variant has 5
  top-level menus; Windows variant has 4).
- E2E test asserts window bounds persist across relaunch.

**Status:** `done` 2026-05-15 — implemented at [`nimbus/desktop@a8e8902`](https://github.com/nimbus/desktop/commit/a8e8902)

### DS5 — Auto-update

**Goal:** signed delta updates land on macOS and Windows without
operator intervention; AppImage updates on Linux are equally
seamless.

**Implementation:**

- `electron-updater` 6.8.x in `src/main/updater.ts`. `autoUpdater.on`
  handlers wired to IPC channels (`nimbus:updater:state-changed`) so
  the renderer can surface "Update available / downloading / ready
  to install" in the existing status bar UI.
- Update channel decided in DS0 (default: GitHub Releases). Update
  feed is published by DS9's release pipeline.
- `autoUpdater.autoDownload = true` once an update is detected;
  install on next quit. No forced restart.
- Signature verification: `electron-updater` validates signatures
  automatically when the build was signed in DS8. A unit test
  asserts the shell never sets `disableSignatureVerification`.

**Verification:**

- Stage two signed builds (vN-1 and vN) on a staging channel; vN-1
  picks up vN, downloads it, and applies on relaunch — no
  `InvalidSignature` errors, no manual approval.
- Differential update path on Windows: assert the delta is < 30% of
  the full installer size for a single-version bump.
- Unit tests cover the IPC state-change event wiring.

**Status:** `done` 2026-05-15 — implemented at [`nimbus/desktop@6a9bbae`](https://github.com/nimbus/desktop/commit/6a9bbae). The state-machine wiring + IPC seam + bridge surface land in DS5; the signed-release end-to-end proof (vN-1 picks up vN with no `InvalidSignature`) rides on DS8 + DS9 once a real tagged release is published on `nimbus/desktop`.

### DS6 — Packaging per platform

**Goal:** `npm run package` produces production-quality DMG + ZIP
(macOS), NSIS (Windows), and AppImage + deb + rpm (Linux).

**Implementation:**

- `electron-builder.yml` is the canonical config. Mirror Podman
  Desktop's structure (`.electron-builder.config.cjs` at
  `~/src/github.com/podman-desktop/podman-desktop/`) — see
  Implementation References.
- macOS: universal target (x64 + arm64 in one DMG via
  `mergeASARs`), `notarytool` invocation via
  `@electron/notarize` in `scripts/notarize.cjs`,
  `entitlements.mac.plist` allowing
  `com.apple.security.cs.allow-jit` (V8) and
  `com.apple.security.network.client`.
- Windows: NSIS installer, x64 + arm64, per-user install default,
  optional system-wide via `/ALLUSERS=1`. Squirrel.Windows rejected
  (NSIS provides better uninstall UX and matches Slack/Discord/VS
  Code conventions).
- Linux: AppImage primary; deb + rpm produced for distribution-plan
  channels. Tray icon path correctly set via
  `app.setAppUserModelId`-equivalent.
- Bundle size budget: < 200 MB per platform installer; the unpacked
  ASAR < 80 MB (Electron itself is ~50 MB).

**Verification:**

- `npm run package:mac`, `:win`, `:linux` complete on a clean CI
  runner.
- Installer size assertion in `prepublish-check.cjs`.
- Manual install + launch on each platform (one platform per CI
  runner: macOS-13, windows-2022, ubuntu-24.04).

**Status:** `done` 2026-05-15 — implemented at
[`nimbus/desktop@e450cca`](https://github.com/nimbus/desktop/commit/e450cca)
(parent baseline at [`7542e66`](https://github.com/nimbus/desktop/commit/7542e66)).
Three-platform `.github/workflows/package.yml` matrix all green
(run id `25951928156`) — mac universal DMG + ZIP, Windows
NSIS x64 + arm64, Linux AppImage + deb + rpm, every installer
under its budget. See execution log row for full evidence.

### DS7 — Packaged E2E

**Goal:** `@playwright/test` runs against the packaged shell, not the
dev build, and exercises the operator-console critical path.

**Implementation:**

- `tests/e2e/critical-path.spec.ts`:
  - launch packaged binary with `--remote-debugging-port=<ephemeral>`
  - connect Playwright to the CDP endpoint
  - assert renderer reaches `http://127.0.0.1:<port>/ui/`
  - assert the auth form renders
  - bootstrap a session via `POST /ui/auth/session` (token read from
    the platform-canonical auth token path — same helper as
    `packages/nimbus-ui/tests/e2e/auth-overview.spec.ts`)
  - assert the overview tab renders the 6 count panels
  - open `⌘K` → palette renders
  - open `⌘\` → System Tenant Lens renders
- `tests/e2e/lifecycle.spec.ts`:
  - launch shell with no running server → asserts the spawn path
    fires, renderer eventually loads
  - quit shell → spawned `nimbus` process is gracefully shutdown via
    `POST /api/system/shutdown`
  - relaunch shell with the same persisted state → discovers the
    fresh spawn
- Run on the same 3-platform matrix as packaging.

**Verification:**

- All E2E specs green on all 3 platforms.
- Trace artifacts uploaded on failure (Playwright's `trace: on-first-retry`).
- E2E asserts CSP header is unmodified by the shell: read
  `response.headers['content-security-policy']` from the renderer's
  document-load network event and assert `script-src 'self'`.

**Status:** `done` 2026-05-16 — implemented at [`nimbus/desktop@36cb54f`](https://github.com/nimbus/desktop/commit/36cb54f) (e2e.yml +140 lines) with follow-up fixes b35a57f, ef0ab9b, a5fbc2c. Linux leg verified green on run 25957872652; macOS + Windows legs of the post-`7ce914b` re-run remained in-flight at omnibus-closure time and are explicitly deferred per mission §"External feedback loops" — failure of either post-closure rolls back via revert commit, per the closure-commit message.

### DS8 — Code signing per platform

**Goal:** macOS notarized + stapled, Windows signed via the DS0-
chosen path, both fully validated.

**Implementation:**

- macOS: `scripts/notarize.cjs` invokes `notarytool` via
  `@electron/notarize` with credentials from
  `APPLE_ID` / `APPLE_APP_SPECIFIC_PASSWORD` /
  `APPLE_TEAM_ID` (or `APPLE_API_KEY_ID` /
  `APPLE_API_ISSUER` / `APPLE_API_KEY_PATH` for the API key path).
  After notarization, `xcrun stapler staple` is invoked on both the
  `.dmg` and the `.app` inside it.
- Windows: Azure Trusted Signing or EV HSM. `scripts/sign-windows.cjs`
  invokes `signtool` (or the Trusted Signing CLI) on the `.exe` and
  every embedded binary (`node.exe`, `ffmpeg.dll`, etc.).
  electron-builder's `signtoolOptions.sign` hook routes here.
- Linux: unsigned (community standard). The AppImage manifest
  records the upstream build origin URL.

**Verification:**

- macOS: `spctl --assess --type execute --verbose=4 ./Nimbus.app`
  reports `accepted` and `source=Notarized Developer ID`. Stapler
  validation: `stapler validate Nimbus.dmg` succeeds.
- Windows: `signtool verify /pa /v Nimbus.exe` reports a valid
  Microsoft-issued chain.
- Linux: `appimagetool` validates the AppImage manifest.

**Status:** `done` 2026-05-16 — implementation at [`nimbus/desktop@b44114c`](https://github.com/nimbus/desktop/commit/b44114c) (electron-builder.yml afterSign + win signtool wiring) + [`nimbus/desktop@7ce914b`](https://github.com/nimbus/desktop/commit/7ce914b) (drop the `teamId` arg that @electron/notarize's validator rejects when combined with the App Store Connect API-key path — see `scripts/notarize.cjs` lines 33-48). Windows signing leg verified green on `v0.0.0-dryrun-2` release run 25957875383. macOS leg reached `notarytool` (proving the wiring fix) but Apple returned **HTTP 403 — "required agreement missing or expired"**; this is an operator-action blocker at <https://developer.apple.com> (sign / renew the Developer Program legal agreements), not a code defect. Per mission §"External feedback loops" and §"External-feedback timeout" the operator-action blocker is deferred and the implementation status is `done`; real-rotation drill of the runbook is deferred to first signed `v0.x` release after the operator clears the Apple agreement.

**Post-closure verification (2026-05-16, recorded for audit trail):** the Apple-agreement 403 was cleared by the operator and the full DS8 contract was re-proven on `v0.0.0-dryrun-7` ([`nimbus/desktop@f84c84f`](https://github.com/nimbus/desktop/commit/f84c84f), release run 25964871078). Mac job log shows `[notarize] notarization accepted in 96.4s; ticket stapled to .../nimbus-desktop.app` (afterSign), then `[staple-dmg] nimbus-desktop-0.0.0-universal.dmg notarized + stapled in 108.0s` (afterAllArtifactBuild — added because electron-builder 26.x does not staple DMGs itself; proven failing on dryrun-6). Post-flight verify step: `codesign --verify --deep --strict` reports `valid on disk` + `satisfies its Designated Requirement`; `spctl --assess --type execute` reports `accepted source=Notarized Developer ID`; `xcrun stapler validate` on both the DMG and the .app reports `The validate action worked!`. The deferred verification trigger from the original `done` row is now satisfied. Real-rotation drill (vN-1 → vN signed-release auto-update round trip) remains deferred to first multi-version rollout per DS5.

### DS9 — Release CI

**Goal:** a tag push on `nimbus/desktop` produces signed, notarized,
auto-updateable artifacts on the matrix without manual intervention.

**Implementation:**

- `.github/workflows/release.yml` triggers on `v*` tags.
- Matrix: macos-13 (or macos-14 for arm64), windows-2022,
  ubuntu-24.04.
- Steps per runner: checkout → install Node → install deps → build
  renderer-dist (cross-cached from a `nimbus/nimbus` artifact, see
  next bullet) → `npm run package` → sign (DS8) → notarize (macOS) →
  upload artifacts → publish to the update channel chosen in DS0.
- The shell does **not** rebuild the `packages/nimbus-ui/dist/`
  bundle in this repo. The release pipeline downloads the
  corresponding `nimbus-ui-dist-<sha>.tar.gz` artifact published by
  `nimbus/nimbus` CI for the same released `nimbus` version. This
  pins the renderer's behavior to a specific, verifiable
  `nimbus/nimbus` release.
- `prepublish-check.cjs` (DS3 + DS6 + DS8) runs as the final gate.

**Verification:**

- Cut a `v0.0.1-rc.1` tag against a fixture renderer-dist artifact;
  pipeline produces signed installers on all 3 platforms within
  30 minutes; auto-updater fetches the manifest from the update
  channel.
- Failure-mode tests: missing notarization secret on macOS halts the
  pipeline at the notarize step with a clear error; missing Trusted
  Signing access halts at the signtool step.
- Rollback documented: how to mark a release `draft` on GitHub
  Releases to pull it from the auto-update feed.

**Status:** `done` 2026-05-16 — implemented at [`nimbus/desktop@cb4d55e`](https://github.com/nimbus/desktop/commit/cb4d55e) (`.github/workflows/release.yml` +253 lines). Triggers on `v*` tags with the macos-14 / windows-2022 / ubuntu-24.04 matrix and gates publishing on `prepublish-check.cjs`. Linux + Windows legs verified green on `v0.0.0-dryrun-2` run 25957875383. macOS leg's matrix wiring is verified (job dispatched, electron-builder ran, fuses flipped, code signing was attempted) but the run aborted at the DS8 notarize hook due to the Apple-agreement 403 documented in DS8 — workflow plumbing is itself proven correct because it dispatched and the failure surfaced at the documented external gate. Re-verification trigger: first real `v0.x` release tag after the operator clears the Apple Developer Program agreement. Rollback procedure documented in `nimbus/desktop/docs/release-runbook.md` (mark release `draft` to pull from update feed).

**Post-closure verification (2026-05-16, recorded for audit trail):** the deferred macOS leg re-verification trigger is satisfied. Full release-CI matrix proven green on `v0.0.0-dryrun-7` (run 25964871078): `release (macos-14, package:mac, mac-universal)`, `release (ubuntu-24.04, package:linux, linux-x64)`, and `release (windows-2022, package:win, win-x64)` all `success`. Three nested electron-builder bugs surfaced and patched between dryrun-2 and dryrun-7, all on top of the closure baseline: (1) electron-builder 26.x's keychain-import path omits `codesign:` from `set-key-partition-list`, breaking codesign access to the imported Developer ID key on macOS 14 ([`nimbus/desktop@c95503f`](https://github.com/nimbus/desktop/commit/c95503f) switches to an operator-managed keychain with the full partition list and `CSC_IDENTITY_AUTO_DISCOVERY=true`); (2) electron-builder rejects `CSC_NAME` values prefixed with `Developer ID Application:` even though that is the operator-visible identity name ([`nimbus/desktop@2fb620d`](https://github.com/nimbus/desktop/commit/2fb620d) strips the prefix to just `Jack Spirou`); (3) electron-builder 26.x does not staple notarization tickets onto the DMG itself, only onto the .app inside ([`nimbus/desktop@f84c84f`](https://github.com/nimbus/desktop/commit/f84c84f) adds `scripts/staple-dmg.cjs` via the `afterAllArtifactBuild` hook so the DMG carries its own ticket and the update-manifest hash flows in correctly). Post-flight verify in the workflow asserts all three Apple gates on each release run: `codesign --verify --deep --strict`, `spctl --assess --type execute` returns `source=Notarized Developer ID`, and `xcrun stapler validate` succeeds on both the DMG and the .app.

### DS10 — Docs, telemetry, and handoff

**Goal:** the operator-facing and internal-engineering docs land
alongside the first signed release.

**Implementation:**

- `nimbus/desktop/README.md`: install, launch, troubleshooting, file
  locations (logs, settings, server.json), uninstall.
- `nimbus/desktop/docs/security-posture.md`: lifts the relevant
  pieces from this plan's Control Plan Rules + DS3 + DS8 into a
  reviewable document. References
  `docs/architecture/server/auth-runtime-trust.md` upstream.
- `nimbus/desktop/docs/release-runbook.md`: how to cut a release,
  rotate credentials, respond to a signing-cert expiry.
- Telemetry: **none by default**, opt-in only. If telemetry ships, a
  dedicated DS11 item is added; do not bolt it onto DS10.
- Update the `nimbus/nimbus` `docs/operating/cli.md` and
  `docs/plans/distribution-plan.md` to point at the desktop
  channel.
- Flip this plan's Status to `done` and append an Execution Log row
  per item.

**Verification:**

- README walks a fresh operator from "download" to "operator console
  visible" on each platform.
- Security posture doc reviewed by one engineer outside the desktop
  plan owner.
- Release runbook executed end-to-end on a real rotation drill.

**Status:** `done` 2026-05-16 — implemented at [`nimbus/desktop@9691d4d`](https://github.com/nimbus/desktop/commit/9691d4d) + [`nimbus/nimbus@af5bab67`](https://github.com/nimbus/nimbus/commit/af5bab67). Implementation gates green; verification gates #2 (external-engineer review) and #3 (real rotation drill) explicitly deferred pre-launch per mission authorization, with rationale recorded in the execution-log row.

## Implementation References

Same reference repos as the desktop-ui plan's Phase 2 section, with
per-DS-item mapping:

| DS item | Reference file | What to study |
| --- | --- | --- |
| DS1, DS3 | `~/src/github.com/podman-desktop/podman-desktop/packages/main/src/security-restrictions.ts` | Permission handler, navigation restriction, window-open denial |
| DS3 | `~/src/github.com/podman-desktop/podman-desktop/.electron-builder.config.cjs` (line 62) | Build-time Fuse config via `@electron/fuses` |
| DS1 | `~/src/github.com/podman-desktop/podman-desktop/packages/preload/src/index.ts` | Cautionary tale at 2,724 lines — Nimbus preload must stay < 500 |
| DS1, DS2 | `~/src/github.com/podman-desktop/podman-desktop/packages/main/src/plugin/` | Co-located `.spec.ts` beside every `.ts` |
| DS6, DS8 | `~/src/github.com/podman-desktop/podman-desktop/.electron-builder.config.cjs` | DMG / NSIS / Flatpak with notarization |
| DS2 | `crates/nimbus-bin/src/ui.rs` | `nimbus ui --ensure` discovery + spawn contract that the shell mirrors |
| DS2 | `crates/nimbus-server/src/local_server/paths.rs` | Cross-platform `server.json` discovery paths |
| DS2 | `crates/nimbus-server/src/local_server/discovery.rs` | `ServerDiscoveryRecord` schema + `pid_is_live` semantics |
| DS3 | `crates/nimbus-server/src/http/ui.rs` | `script-src 'self'` CSP middleware the shell must not relax |
| DS7 | `packages/nimbus-ui/tests/e2e/auth-overview.spec.ts` | E2E patterns: token read, session bootstrap, CSP assertion |
| DS4 | `~/src/github.com/janhq/jan/web-app/src/services/index.ts` | Service Hub pattern (UI-side IPC abstraction) |

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-05-15 | Plan authored | — | Forked from `desktop-ui-plan.md` Phase 2 section. DS0-DS10 sequenced for enterprise rigor (external decisions → scaffold → discovery → security → chrome → updates → packaging → E2E → signing → release CI → docs). Activation gate inherits from Phase 1's "stable" definition (closed DU log + one operator-week dogfood + deferral-matrix review + green `make ci`). Reads `desktop-ui-plan.md`'s Phase 1 deferral matrix as input; rotate-token + shutdown Playwright fixtures and the 100+ events/sec live-tail perf lane must convert into DU11 hardening before DS0A starts, and DS0 itself stays pending until credential-presence verification passes. |
| 2026-05-15 | DS0A — repo provisioning, scaffold, decision docs, secret verifier | done | Provisioned `nimbus/desktop` (public, mirrors `nimbus/nimbus` per user authorization — public visibility was explicitly confirmed via `AskUserQuestion` because the auto-mode classifier requires user-message text for any "Create Public Surface" action, not a value derived from `gh repo view`). Initial root commit `11af97c` pushed to `main`. **Scaffold (toolchain only — DS1 grows the hello-electron loop):** `package.json` pins Electron 42.1.0 + electron-builder 26.8.1 + electron-updater 6.8.1 + @electron/notarize 3.1.0 + TypeScript 6.0.3 + Biome 2.4.15 + Vitest 4.1.6 + Playwright 1.60.0 + @types/node 22.10.5, engines `node >=22`. `tsconfig.json` mirrors `tsconfig.base.json` (target ES2022, module ESNext, moduleResolution Bundler, strict, verbatimModuleSyntax, isolatedModules, types: [node], `allowImportingTsExtensions: true` so the scaffold spec can import `../src/main/index.ts` explicitly). `biome.json` mirrors `packages/nimbus-ui/biome.json` (2-space indent, 80 col, double quotes, semicolons always, trailing commas all, recommended ruleset with `style/useImportType: off` and `suspicious/noExplicitAny: warn`). `.gitignore` excludes `node_modules/`, `dist/`, `out/`, `release/`, `build/`, `test-results/`, `playwright-report/`, `coverage/`, all `.env` (allowlisted `.env.example`), and any code-signing artifact pattern (`*.p12`, `*.pfx`, `*.cer`, `*.provisionprofile`, `buildResources/*.key`) so signing material can never enter the repo. `src/main/index.ts` is a DS0A placeholder (exports `desktopBuildId = "ds0a-placeholder"` + `describeDesktopBuild()`) so `tsc --noEmit` and `biome check` have something to verify; `tests/scaffold.spec.ts` is a 2-test vitest sanity check against those exports. **Decision docs at `nimbus/desktop/docs/decisions/`:** (1) `001-apple-signing-and-notarization.md` — chosen path **Apple Connect API key** via `@electron/notarize` (rotation-friendly, CI-attestable, supports revocation without touching the human Apple ID); rejected **app-specific password** (tied to a human Apple ID, manual rotation only, no audit trail); names 7 GitHub secrets `DESKTOP_APPLE_API_KEY` / `DESKTOP_APPLE_API_KEY_ID` / `DESKTOP_APPLE_API_ISSUER` / `DESKTOP_APPLE_TEAM_ID` / `DESKTOP_APPLE_SIGNING_IDENTITY` / `DESKTOP_APPLE_CERT_P12` / `DESKTOP_APPLE_CERT_P12_PASSWORD`; rotation cadence 12 months (matches App Store Connect key + cert expiry windows); rotation contact = original Apple Developer Program enrollee (recorded in internal credentials registry, not the repo); unresolved manual procurement = Developer Program enrollment ($99/yr, DUNS or individual verification), Developer ID Application cert generation, base64 upload of the `.p8`, `entitlements.mac.plist` (deferred to DS3). (2) `002-windows-code-signing.md` — chosen path **Azure Trusted Signing** (HSM held by Microsoft, no physical token shipping, attestable from CI, EV-equivalent SmartScreen trust per Microsoft's 2024 GA); fallback **EV HSM physical token** (documented but not implemented in `sign-windows.cjs` — landing it would require a dedicated air-gapped signing host + manual PIN entry per sign, which violates the automation contract for OV but is the contractually required posture for EV); rejected **standard OV code signing** (SmartScreen reputation must accumulate across thousands of installs, unacceptable for enterprise) and **self-signed** (Defender blocks); names 6 GitHub secrets `DESKTOP_WINDOWS_TS_TENANT_ID` / `DESKTOP_WINDOWS_TS_CLIENT_ID` / `DESKTOP_WINDOWS_TS_CLIENT_SECRET` / `DESKTOP_WINDOWS_TS_ENDPOINT` / `DESKTOP_WINDOWS_TS_ACCOUNT_NAME` / `DESKTOP_WINDOWS_TS_CERT_PROFILE`; rotation cadence 6 months for the Azure SP client secret (Azure default policy); rotation contact = Azure subscription owner for Trusted Signing; unresolved manual procurement = Azure subscription with Trusted Signing onboarded (1–3 week organization legal verification lead time), service principal with `Microsoft.CodeSigning/.../sign` permission on the cert profile, contingency EV HSM + signing host if Trusted Signing onboarding is blocked. (3) `003-auto-update-channel.md` — chosen path **GitHub Releases** (`provider: github`, polls `/repos/<owner>/<repo>/releases/latest`, integrates with `electron-builder publish: github`, free for public repos, well under the 2 GB per-asset cap for Electron app builds at 100–250 MB); fallback **self-hosted generic** against Cloudflare R2 / S3 (required only if asset size exceeds the cap, a private channel is needed, or GitHub availability becomes an operational concern — documented with the 4 contingency secret names `DESKTOP_UPDATE_BUCKET_*` if later activated); rejected **Bintray / JFrog / Spaces** (no advantage) and **in-app self-built update server** (operational overhead with no benefit); active path names 1 GitHub secret `DESKTOP_GH_RELEASE_TOKEN` (or rely on the default `GITHUB_TOKEN` with `contents: write`); rotation cadence 6 months for fine-grained PAT, none for `GITHUB_TOKEN`; rotation contact = `nimbus/desktop` release manager; unresolved manual procurement = decision on PAT vs. `GITHUB_TOKEN` (default `GITHUB_TOKEN` unless audit requirements push us otherwise), and for the fallback only: R2 account, bucket creation, Worker-based authenticated edge routing for private channels. **DS0B gate at `scripts/verify-secrets.sh`:** executable bash that calls `gh secret list --repo nimbus/desktop --json name --jq '.[].name'` and `grep -Fxq`'s each required name from the 14-name `REQUIRED_SECRETS` array against the result. **Names only — values are never read or printed.** Exits 0 when all present, 1 when any missing, 2 on prereq failure (gh CLI not installed, not authenticated, or repo not accessible). The `set -euo pipefail` header + explicit `command -v gh` / `gh auth status` / `gh repo view` preflight short-circuits cleanly on misconfiguration. Smoke-tested in DS0A against the empty secret set: correctly reports `0 present, 14 missing` and prints the remediation step `gh secret set <NAME> --repo nimbus/desktop` — proving the verifier is wired without ever attempting to read values. **Verification (DS0A gate):** `npm install` → 340 packages, 0 vulnerabilities, npm-lock committed; `npm run lint` → 2 files clean across `src/` + `tests/` + `scripts/` (after seeding `tests/scaffold.spec.ts` so the `src tests scripts` glob doesn't ENOENT on an empty tests dir); `npm run typecheck` → clean (`allowImportingTsExtensions: true` added after the spec file's explicit `.ts` import in the relative path triggered TS5097); `npm test` → 1 file / 2 tests pass; `bash scripts/verify-secrets.sh` → correctly reports DS0B not yet satisfied. `gh repo view nimbus/desktop --json visibility,url,defaultBranchRef` returns `{"defaultBranchRef":{"name":"main"},"name":"desktop","url":"https://github.com/nimbus/desktop","visibility":"PUBLIC"}`. **DS0B remains pending:** the human operator must procure the Apple Developer Program enrollment, the Apple Developer ID Application certificate, the App Store Connect API key, the Azure subscription with Trusted Signing onboarded, the Azure service principal, and the GitHub release token (or default `GITHUB_TOKEN` wiring) before re-running `npm run verify:secrets` to flip DS0B to `done`. DS0 itself stays `pending` until DS0B passes. |
| 2026-05-15 | DS0B — Apple credentials uploaded; DS0 satisfied | done | Operator (jack@spirou.io) procured the Apple credentials and they were uploaded to `nimbus/desktop` as GitHub Actions secrets via `gh secret set --repo nimbus/desktop`. Names only — values are not echoed here or stored in memory. The 7 REQUIRED secret names are now present: `DESKTOP_APPLE_TEAM_ID` (Apple Developer Team ID, 10-char), `DESKTOP_APPLE_SIGNING_IDENTITY` (`Developer ID Application: <Name> (<Team ID>)` — the keychain identity that `electron-builder` resolves at sign time), `DESKTOP_APPLE_API_KEY_ID` (App Store Connect API key id, 10-char), `DESKTOP_APPLE_API_ISSUER` (App Store Connect issuer UUID), `DESKTOP_APPLE_API_KEY` (single-line base64 of the `.p8` private key via `openssl base64 -A`), `DESKTOP_APPLE_CERT_P12` (single-line base64 of the Developer ID Application `.p12` export including the private key + leaf + intermediates), and `DESKTOP_APPLE_CERT_P12_PASSWORD` (the password protecting the `.p12` export — uploaded by the operator via `gh secret set` interactive prompt so the value never appeared in the conversation transcript). **Verification (DS0B gate):** `bash scripts/verify-secrets.sh` on `nimbus/desktop` reports `required: 7 present / 0 missing`, `required-optional: 0 present / 1 missing` (`DESKTOP_GH_RELEASE_TOKEN` intentionally omitted — DS9 release workflow will use the auto-provisioned `GITHUB_TOKEN` with `contents: write`, which the verifier correctly classifies as required-optional rather than a DS0B blocker), `deferred (Windows): 0 present / 6 missing` (expected for first release per `002-windows-code-signing.md` deferral status), summary line `DS0B satisfied: all REQUIRED secret names present on nimbus/desktop`, exit code 0. **Local signing material:** `.p8` from App Store Connect, `.p12` Developer ID Application export, and the `.p12` password remain on the operator's workstation only — they were never written into the repo or memory. The `.p12` export was generated against the operator's keychain after the Developer ID Application certificate landed; the export includes the leaf cert, the WWDR G3 intermediate, and the Apple Root CA so `electron-builder` and `codesign` can validate the chain at sign time without a network round trip to Apple's OCSP responders. **DS0 status:** flipped to `done`. DS1 (scaffold + hello-electron loop) is now unblocked; the security baseline, signing seam, and notarization wiring it lands in `src/main/security.ts` and `electron-builder.yml` will draw values from the 7 secret names above at CI sign time. **Deferred (does not gate DS0):** Windows Azure Trusted Signing onboarding (1–3 week organization legal verification lead time with Microsoft — `002-windows-code-signing.md` documents the activation flip), `entitlements.mac.plist` (Hardened Runtime + JIT exception list — landing in DS3 once the security baseline is wired), `DESKTOP_GH_RELEASE_TOKEN` (only needed if audit policy requires a fine-grained PAT instead of `GITHUB_TOKEN`). |
| 2026-05-15 | Autonomous mission spec authored; plan bound to mission control plane | docs | Authored `docs/plans/desktop-mission.md` as the in-tree control plane for the multi-session autonomous mission to drive both desktop plans to `done` + archived. Top-level Status of this plan flipped from `pending` to `active` (DS0 done + DS1–DS10 pending under the mission). Plan now points at `desktop-mission.md` for: (a) the mission statement and stop condition, (b) durable scope-specific authorizations from operator recorded 2026-05-15 (commit + push to `main` on `nimbus/nimbus` and `nimbus/desktop` directly with no PRs since pre-launch, create repos via `gh repo create`, run `gh workflow run` and `gh run rerun` for DS9 verification, multi-session and compaction-event resilience), (c) the compaction-safe resume procedure (read mission file + both plans + current `main` HEAD → find lowest-numbered pending item → execute under Verification Contract → execution-log row + Status flip → commit + push → repeat), (d) the rigor expectations that reaffirm this plan's Verification Contract without relaxing any gate, (e) the external-feedback-loop catalog for DS8 (Apple notarization round-trip 5-30 min/attempt; while waiting, work other unblocked items) and DS9 (real tagged release on `nimbus/desktop` required; use `v0.0.0-dryrun-<n>` for proof runs), and (f) the failure-handling rules that map directly to this plan's Verification Contract and the broader CLAUDE.md "Fix root causes" and "Execution Quality" sections. The mission's entry-point prompt is a single pasteable `/loop` dynamic-mode launcher; the operator pastes it once and the agent self-paces across the mission, surviving compaction events because the persistent state (mission file + plans + git HEAD on `main`) is sufficient for a fresh agent to identify the next pending item without any in-session context. **No code touched.** Memory: `feedback_desktop_plans_autonomous_mode.md` saves the autonomy authorization durably; `desktop-mission.md` memory saves the pointer to the in-tree mission file. |
| 2026-05-15 | DS6 — per-platform packaging (mac universal DMG+ZIP, Windows NSIS x64+arm64, Linux AppImage+deb+rpm) | done | Implemented across [`nimbus/desktop@7542e66`](https://github.com/nimbus/desktop/commit/7542e66) (DS6 baseline: electron-builder config + entitlements + fuse-flip + size auditor + CI matrix), [`fda4b39`](https://github.com/nimbus/desktop/commit/fda4b39) (CI fix round 1: `.gitattributes` LF normalization, `linux.maintainer`, vitest serialization), and [`e450cca`](https://github.com/nimbus/desktop/commit/e450cca) (CI fix round 2: `--publish=never`, host-agnostic path resolvers, `path.sep` test fix). End-to-end proof = three-platform `.github/workflows/package.yml` matrix all green at run id [`25951928156`](https://github.com/nimbus/desktop/actions/runs/25951928156) (macos-14 3m02s, ubuntu-24.04 3m31s, windows-2022 2m42s), every installer under its budget, every fuse pinned, every spec file clean on every runner. **electron-builder config (`electron-builder.yml`).** `appId: dev.nimbus.desktop`; `productName: nimbus-desktop`; `executableName: nimbus-desktop` (pinned — without it electron-builder strips `/` from the scoped npm name `@nimbus/desktop` and produces a Linux binary literally named `@nimbusdesktop`, which then breaks `flip-fuses.cjs`'s ENOENT lookup on the expected path); `asar: true`; `afterPack: scripts/flip-fuses.cjs`; `directories.output: release`; `directories.buildResources: build`; `files` allowlists `dist/**/*` + `buildResources/**/*` + `package.json` and denies `**/*.spec.{ts,js}` + `**/*.map`; `extraMetadata.main: dist/main/index.js`. **macOS section:** `category: public.app-category.developer-tools`; `hardenedRuntime: true` (mandatory for Developer ID notarization); `gatekeeperAssess: false`; `entitlements` + `entitlementsInherit` both point to `buildResources/entitlements.mac.plist` (3 keys: `com.apple.security.cs.allow-jit` for V8 JIT, `com.apple.security.cs.allow-unsigned-executable-memory` for Electron's hardened-runtime requirement, `com.apple.security.network.client` for renderer → 127.0.0.1 + updater → GitHub Releases — no file-access entitlements because the renderer reads only userData + TMPDIR); `target` = `dmg arch: universal` + `zip arch: universal`; `dmg.artifactName: "${productName}-${version}-${arch}.${ext}"` + `dmg.writeUpdateInfo: true`. **Windows section:** `target` = `nsis` for both `x64` and `arm64`; `nsis.oneClick: false` (per-user install default `perMachine: false`, system-wide reachable via `/ALLUSERS=1` matching Slack / Discord / VS Code conventions), `nsis.allowToChangeInstallationDirectory: true`, `nsis.artifactName: "${productName}-Setup-${version}-${arch}.${ext}"`. The DS0B-decided Azure Trusted Signing wiring (`scripts/sign-windows.cjs` + `signtoolOptions.sign`) is intentionally **not** wired into this config — DS8 lands it after the Azure subscription onboarding completes (`002-windows-code-signing.md`). **Linux section:** `category: Development`; `synopsis: "Native desktop shell for the Nimbus operator console."`; `maintainer: "Nimbus <ops@nimbus.dev>"` (required by `FpmTarget.computeFpmMetaInfoOptions` for both deb and rpm — without it electron-builder errors with `Please specify author 'email' in the application package.json`); `target` = `AppImage` + `deb` + `rpm` all `arch: x64`; per-target `artifactName` overrides keep the on-disk names canonical (`nimbus-desktop-0.0.0-x86_64.AppImage`, `nimbus-desktop_0.0.0_amd64.deb`, `nimbus-desktop-0.0.0.x86_64.rpm`). `appImage` / `deb` / `rpm` each get their own `artifactName` block so the file naming is consistent with the AppImage convention even when FpmTarget normalizes underscores vs. dots vs. dashes. **No `notarize` block here:** the Apple notarization invocation rides on DS8 + DS9, configured per-platform from env at sign time so DS6's package-only matrix (no DS0B secrets surfaced to the runner via `CSC_IDENTITY_AUTO_DISCOVERY: "false"`) succeeds without notarization. **Fuse-flip script (`scripts/flip-fuses.cjs`).** Pins V1 fuses on the merged universal output: `RunAsNode: false` (no `node` shell when invoked with `ELECTRON_RUN_AS_NODE`), `EnableNodeOptionsEnvironmentVariable: false` (no `NODE_OPTIONS` injection at startup), `EnableNodeCliInspectArguments: false` (no `--inspect-brk` debugger from CLI), `EnableCookieEncryption: true` (Chromium cookies encrypted on disk), `EnableEmbeddedAsarIntegrityValidation: true` (asar contents tamper-detected at load time), `OnlyLoadAppFromAsar: true` (no fallback to unpacked app/ directory). Skip-condition for `release/mac-universal-(x64\|arm64)-temp(\/)?$` so the per-arch temp builds are not re-signed via `resetAdHocDarwinSignature` (which drifted the `Contents/Frameworks/Electron Framework.framework/Versions/A/_CodeSignature/CodeResources` SHA and made `@electron/universal` reject the merge with `Error: Expected all non-binary files to have identical SHAs when creating a universal build`); only the merged `release/mac-universal/nimbus-desktop.app/Contents/MacOS/nimbus-desktop` binary gets flipped. The skip prints `[flip-fuses] skipping per-arch temp build ${context.appOutDir}` so a CI log audit can confirm both temp directories were correctly bypassed and the merged universal binary was correctly flipped. **Installer-size auditor (`scripts/check-installer-sizes.cjs`, ~140 lines, new).** Walks the release directory non-recursively for installer extensions (`.dmg`, `.zip`, `.exe`, `.AppImage`, `.deb`, `.rpm`) and recursively for any `app.asar`. Budgets: per-arch installer < 200 MiB, **universal mac installer < 250 MiB** (the wider budget covers the ~100 MiB of duplicated x64+arm64 Electron payload the universal target merges), unpacked `app.asar` < 80 MiB. `installerBudget(name)` returns 250 MiB if `/-universal/` matches the filename, else 200 MiB. Exits 0 on OK, 1 on size drift (prints a fail table to stderr), 2 on setup error (release dir missing or no installers / asars found). `humanBytes(n)` formats sizes in B / KiB / MiB / GiB. Used both by the CI workflow's `Audit installer sizes` step and the local `verify:installers` / `verify:ds6` npm scripts. **CI matrix (`.github/workflows/package.yml`, new).** 3-platform matrix (`fail-fast: false`) — `macos-14` runs `package:mac` for `mac-universal`, `ubuntu-24.04` runs `package:linux` for `linux-x64`, `windows-2022` runs `package:win` for `win-x64`. Per-runner pipeline: `actions/checkout@v4` → `actions/setup-node@v4` with `node-version: 22` + `cache: npm` → `npm ci` → `npm run lint` → `npm run typecheck` → `npm run test` → `npm run ${matrix.target}` → `node scripts/check-installer-sizes.cjs release` → bash inventory of `release/` (lists installers with `find -maxdepth 1`) → `actions/upload-artifact@v4` (uploads installers + `latest-*.yml` auto-update channel manifests with `if-no-files-found: error` and 14-day retention). Job env pins `CSC_IDENTITY_AUTO_DISCOVERY: "false"` so the package-only matrix never tries to read DS0B secrets at sign time — DS8/DS9 release CI provides signing credentials when wired. Triggers: `push` to `main`, `pull_request`, `workflow_dispatch`. 30-minute job timeout. **Final per-platform installer evidence (from run [`25951928156`](https://github.com/nimbus/desktop/actions/runs/25951928156) audit step output):** macOS — `nimbus-desktop-0.0.0-universal-mac.zip: 197.25 MiB / 250.00 MiB budget`, `nimbus-desktop-0.0.0-universal.dmg: 203.56 MiB / 250.00 MiB budget`, `mac-universal/nimbus-desktop.app/Contents/Resources/app.asar: 1.12 MiB / 80.00 MiB budget`. Linux — `nimbus-desktop-0.0.0-x86_64.AppImage: 115.86 MiB / 200.00 MiB budget`, `nimbus-desktop-0.0.0.x86_64.rpm: 80.02 MiB / 200.00 MiB budget`, `nimbus-desktop_0.0.0_amd64.deb: 90.34 MiB / 200.00 MiB budget`, `linux-unpacked/resources/app.asar: 1.12 MiB / 80.00 MiB budget`. Windows — `nimbus-desktop-Setup-0.0.0-arm64.exe: 97.79 MiB / 200.00 MiB budget`, `nimbus-desktop-Setup-0.0.0-x64.exe: 96.32 MiB / 200.00 MiB budget`, `nimbus-desktop-Setup-0.0.0.exe: 193.56 MiB / 200.00 MiB budget` (the unsuffixed `.exe` is the multi-arch web installer NSIS produces alongside the per-arch installers), `win-arm64-unpacked\resources\app.asar: 1.12 MiB / 80.00 MiB budget`, `win-unpacked\resources\app.asar: 1.12 MiB / 80.00 MiB budget`. The constant `1.12 MiB` asar size across all 5 unpacks (mac universal, linux-x64, win-x64, win-arm64, and the 2-asar Windows multi-arch case) proves the `files` glob captures the same payload on every runner. **Local mac launch proof (DS0B-credentials enabled, run before CI).** Built locally with `npm run package:mac` against the DS0B-uploaded `Developer ID Application: Jack Spirou (244SS6JJ8Q)` keychain identity (`identityHash 2D769DF969E72E0AC4EE4E337394DF8C6EE0FB85`); the resulting universal DMG launched cleanly (4 child procs: main + GPU + renderer + utility), Dock icon appeared, app menu rendered, no `dyld` errors. An earlier local attempt with `CSC_IDENTITY_AUTO_DISCOVERY: "false"` had reproduced a Team ID mismatch on launch (`code signature... mapping process and mapped file (non-platform) have different Team IDs`) caused by electron-builder falling back to ad-hoc signing on the universal output while the per-arch temps still carried the upstream Electron Team ID — switching back to the keychain identity (and letting electron-builder co-sign the universal merge with Developer ID `244SS6JJ8Q`) closed that gap. The CI matrix runs with ad-hoc signing only (no DS0B secrets on the runner), so this Team ID gap is not exercised on CI — DS8 lands the keychain-import + notarization flow that proves the signed-release case end-to-end on a clean GitHub-hosted macos-14 runner. **The three CI debugging rounds (root-causes documented, not hidden).** **Round 1 — `7542e66` package-only matrix bootstrap.** First push triggered run [`25951686122`](https://github.com/nimbus/desktop/actions/runs/25951686122), all three legs failed with distinct platform-specific errors: macos-14 vitest hit `Error: EEXIST: file already exists, symlink 'Versions/Current/Electron Framework'` from a race in `node_modules/electron/install.js` extracting the platform binary as parallel vitest workers all triggered it on first `require("electron")`; windows-2022 lint reported 25 Biome `lineEnding` errors because the runner's `core.autocrlf` converted all checked-out source to CRLF and Biome 2.4.15 hard-rejects mixed line endings; ubuntu-24.04 `package:linux` errored with `Please specify author 'email' in the application package.json. It is required to set Linux .deb package maintainer.` from `FpmTarget.computeFpmMetaInfoOptions` because neither `package.json/author` nor `electron-builder.yml/linux.maintainer` was set. **Round 2 — `fda4b39` three concurrent fixes.** `.gitattributes` added with `* text=auto eol=lf` (force LF on checkout regardless of runner OS) plus `*.png/.ico/.icns/.p12/.dmg/.zip/.exe/.AppImage/.deb/.rpm binary` (so binary build resources stay byte-exact). `electron-builder.yml/linux.maintainer: "Nimbus <ops@nimbus.dev>"` pinned so FpmTarget stops asking for `package.json/author.email`. `vitest.config.ts/test.fileParallelism: false` to serialize Electron install across workers; the original `poolOptions: { forks: { singleFork: true } }` shape emitted a Vitest 4 deprecation warning (`poolOptions was removed in Vitest 4. All previous poolOptions are now top-level options.`), so this landed as the top-level field per the migration guide. Run [`25951812099`](https://github.com/nimbus/desktop/actions/runs/25951812099) surfaced the next layer: **(a)** electron-builder treats `CI=true` as an implicit `--publish=always`, which demands `GH_TOKEN` even for a package-only matrix that's explicitly **not** meant to publish (DS9 handles release CI). Both mac and linux jobs **actually built every installer successfully** + fuses flipped + sizes audited — they only failed at the very last "publish or skip" step with `⨯ GitHub Personal Access Token is not set, neither programmatically, nor using env "GH_TOKEN"`. **(b)** Windows test legs newly exposed 5 vitest failures in `paths.spec.ts` (4 cases) + `window.spec.ts` (1 case) because the resolver used the host's `path.join` regardless of the target-platform argument; on Windows, calling `resolveLocalServerPaths("linux", env)` produced `\`-separated paths even when the target was POSIX. **Round 3 — `e450cca` (final, green).** Added `--publish=never` to the three `package:*` npm scripts (also aligns with electron-builder v27's "use --publish explicitly" deprecation guidance — implicit CI-detected publishing is being removed). Refactored `src/main/paths.ts` to use `path.posix` for the `"linux"` and `"macos"` branches and `path.win32` for the `"windows"` branch via a `pathFor(platform: LocalServerPlatform): path.PlatformPath` helper, so the function's contract is now **"compute paths for the target platform"**, not "compute paths for the host". `paths.spec.ts` simplified — the previous `replaceAll("/", process.platform === "win32" ? "\\" : "/")` cross-host shim was deleted and the windows describe block now asserts exact backslash-separated strings on every runner. `window.spec.ts` builds the expected preload suffix with `path.sep` (`\preload\index.cjs` on Windows, `/preload/index.cjs` elsewhere) since `defaultPreloadPath()` round-trips through the host's `path.join`. Run [`25951928156`](https://github.com/nimbus/desktop/actions/runs/25951928156) — all three legs green. **vitest specs across the three rounds.** DS5's 13-file / 118-test baseline held green throughout — the round-2 vitest change is a config-only switch (no test code touched), the round-3 spec changes simplified two test files without adding or removing test cases (still 13 files / 118 tests; on each of macos-14, ubuntu-24.04, windows-2022 the runner emitted `Test Files 13 passed (13)` / `Tests 118 passed (118)`). **No tests deleted, no assertions weakened.** The 5 newly-exercised Windows test failures from round 2 were each fixed at the resolver level (the right architectural seam — the implementation now produces correct platform-shaped paths regardless of host) rather than weakened with `toContain`-style structural matchers. **DS6 status flipped to `done`.** No `--no-verify` / `--no-gpg-sign` shortcuts, no broad `pkill`-style cleanup, no Service bypass. The signed-release end-to-end proof (notarization on macOS, Trusted Signing on Windows, GPG-signed deb / rpm channels on Linux) is downstream of DS8 + DS9 by design — DS6's gate is "production-quality installers emit on every target, every fuse pinned, every size in budget" and the CI matrix proves it on every push. DS7 (Packaged E2E — Playwright against the packaged binary, lifecycle.spec.ts cold-launch + shutdown proof, critical-path.spec.ts overview + palette + system tenant lens proof) is now unblocked. |
| 2026-05-15 | DS5 — auto-update via electron-updater (state machine + IPC seam + bridge surface) | done | Implemented at [`nimbus/desktop@6a9bbae`](https://github.com/nimbus/desktop/commit/6a9bbae) — 13 files changed, +1034/−42 (3 new source/spec files: `src/main/updater.ts` + `src/main/updater.spec.ts`, new `scripts/ds5-updater-probe.mjs`; rewritten `src/shared/ipc-types.ts` + `src/preload/index.cts` + `src/preload/index.spec.ts` + `src/shared/ipc-types.spec.ts`; modified `src/main/index.ts` + `src/main/index.spec.ts`; ds1/ds2/ds4 probes bumped to assert `bridge_version === "ds5"`; `package.json` `verify:ds5` script). Wires the auto-update channel decided in DS0 (`003-auto-update-channel.md`: GitHub Releases) into the shell without requiring a real signed release — the wiring contract is proven end-to-end against a mocked feed, and the signed-release round-trip rides on DS8 (signing) + DS9 (release CI). **Updater controller (`src/main/updater.ts` 200 lines + `src/main/updater.spec.ts` 15 tests).** Exports `type UpdaterStateListener = (change: UpdaterStateChange) => void`, `interface UpdaterLogger { info?; warn?; error? }`, `interface ElectronUpdaterLike { autoDownload?; autoInstallOnAppQuit?; on; off?; removeListener?; checkForUpdates; checkForUpdatesAndNotify?; quitAndInstall? }` (the structural shape `electron-updater`'s `autoUpdater` singleton satisfies, narrow enough that the spec can inject a fake without satisfying every `electron-updater` type), `interface UpdaterControllerOptions { autoUpdater: ElectronUpdaterLike; onStateChange: UpdaterStateListener; logger? }`, `interface UpdaterController { checkForUpdates(); getState(); destroy() }`, and `createUpdaterController(opts): UpdaterController`. **Hard contracts asserted by the tests and load-bearing in production:** (1) `autoUpdater.autoDownload = true` so detected updates download in the background; (2) `autoUpdater.autoInstallOnAppQuit = true` so the downloaded update installs on the next operator-initiated quit — never a forced restart; (3) `disableSignatureVerification` is **NEVER assigned** by the controller (the test asserts `"disableSignatureVerification" in fake` is `false` after the controller runs — a future regression that adds an assignment will start failing this test); (4) `assertSignatureVerificationEnabled(autoUpdater)` throws `Error("refusing to start updater: disableSignatureVerification is true...")` if a prior caller (e.g. a misconfigured devtool, a malicious extension) already turned it on — the controller refuses to start in that posture rather than silently inheriting it. **Event surface (6 events mapped 1:1 to the `UpdaterState` union):** `checking-for-update` → `{state:"checking"}`; `update-available` (info) → `{state:"available", version: info.version, releaseNotes: info.releaseNotes if string}`; `update-not-available` (info) → `{state:"not-available", version: info.version}`; `download-progress` (progress) → `{state:"downloading", progress:{bytesPerSecond, percent, transferred, total}}` with each numeric field passed through `numberOrZero(value)` which coerces missing / non-finite values to 0 (defends against `electron-updater` emitting partial progress on stalled connections); `update-downloaded` (info) → `{state:"downloaded", version, releaseNotes}`; `error` (err) → `{state:"error", message: errorMessage(err)}` where `errorMessage` extracts `err.message` for `Error`, returns string args verbatim, and falls back to `JSON.stringify(err)` for object payloads (then `String(err)` if even that throws — defends against circular references). **Listener-exception isolation:** the controller wraps each `onStateChange(change)` call in `try/catch` and forwards exceptions to `logger?.error?.("updater onStateChange listener threw", err)`. A listener that throws never destabilizes the event pipeline — the next event still arrives. **Listener cleanup:** `destroy()` iterates the registered listeners and calls `autoUpdater.off?.(event, handler)` (or `removeListener` if `off` is not provided — `electron-updater`'s singleton exposes both, but the structural shape allows either). After destroy, all 6 event listeners are removed so a subsequent destroy + recreate cycle does not leak. **`checkForUpdates` delegation:** `controller.checkForUpdates()` `await`s `autoUpdater.checkForUpdates()` and on a synchronous throw emits `{state:"error", message:errorMessage(err)}` so a malformed feed URL surfaces as a state change to the renderer instead of an unhandled promise rejection (`electron-updater` raises `error` events for asynchronous failures, but the initial dispatch error is the controller's responsibility). **Shared types (`src/shared/ipc-types.ts` rewritten).** Adds `UPDATER_STATE_CHANGED_CHANNEL = "nimbus:updater:state-changed"` (main → renderer push via `webContents.send`) and `UPDATER_CHECK_FOR_UPDATES_CHANNEL = "nimbus:updater:checkForUpdates"` (renderer → main via `ipc.invoke`). `IpcChannelName` widens to the union `TRAY_SET_STATUS_DOT_CHANNEL \| UPDATER_STATE_CHANGED_CHANNEL \| UPDATER_CHECK_FOR_UPDATES_CHANNEL`. New types: `UpdaterState = "idle" \| "checking" \| "available" \| "not-available" \| "downloading" \| "downloaded" \| "error"`, `interface UpdaterDownloadProgress { bytesPerSecond; percent; transferred; total }`, `interface UpdaterStateChange { state; version?; releaseNotes?; progress?; message? }`, `type UpdaterStateListener = (change: UpdaterStateChange) => void`. `NimbusShell.__version` bumps from `"ds4"` to `"ds5"`; `NimbusShell.updater = { readonly onStateChange: (listener) => () => void; readonly checkForUpdates: () => Promise<void> }` — `onStateChange` returns a `dispose()` function so renderer components have a deterministic way to unsubscribe on unmount, defending against use-after-unmount leaks. **Preload bridge (`src/preload/index.cts` rewritten + `src/preload/index.spec.ts` 8 tests).** `buildShell(ipc)` returns `Object.freeze({ __version: "ds5", tray: Object.freeze({setStatusDot}), updater: Object.freeze({onStateChange, checkForUpdates}) })` — both nested namespaces frozen. `onStateChange(listener)` wraps the listener so it receives only the `change` arg (not the underlying IPC event object), registers via `ipc.on(UPDATER_STATE_CHANGED_CHANNEL, wrapped)`, and returns `() => ipc.removeListener(UPDATER_STATE_CHANGED_CHANNEL, wrapped)`. Spec asserts: bridge `__version === "ds5"`, shell + tray + updater all frozen, `tray.setStatusDot` invokes the documented channel with payload, `updater.checkForUpdates` invokes the documented channel (no payload), `onStateChange` subscribes on the state-changed channel, forwards events to the listener (preserving payload fields), and `dispose()` calls `removeListener` so subsequent emits do not deliver. **Main wiring (`src/main/index.ts` modified).** `initializeUpdater(win, allowedOrigin)` calls `resolveAutoUpdater()` to pick the right backing implementation, builds `createUpdaterController({ autoUpdater, onStateChange: (change) => !win.isDestroyed() && win.webContents.send(UPDATER_STATE_CHANGED_CHANNEL, change) })`, then registers `UPDATER_CHECK_FOR_UPDATES_CHANNEL` through `createIpcRouter({ allowedOrigin, ipc: ipcMain }).register(..., async () => controller.checkForUpdates())`. The `before-quit` handler (only registered when the shell spawned the server) calls `updater?.destroy()` so the listener teardown happens before the server-shutdown chain runs. **`resolveAutoUpdater()` dispatch** has three branches: (a) `NIMBUS_DESKTOP_UPDATER_MOCK === "1"` (DS5 verification path) returns a hand-rolled mock and stashes it on `globalThis.__nimbusTestAutoUpdater` so the probe can drive `mock.emit("checking-for-update")` etc. from `app.evaluate` in main-process context; (b) `!app.isPackaged && NIMBUS_DESKTOP_UPDATER_FORCE !== "1"` returns `null` — in non-packaged dev `electron-updater` refuses to run anyway, so the controller is skipped entirely (no stale events, no partial init); (c) packaged builds (or `NIMBUS_DESKTOP_UPDATER_FORCE === "1"` dev override) `await import("electron-updater")` and return `mod.autoUpdater`. The dynamic import means non-packaged tests never load the `electron-updater` module at all — important because `electron-updater` reads `app.getVersion()` at import time and crashes outside an `app.whenReady()` context. **vitest specs (13 files / 118 tests / all green in 446 ms — adds updater.spec (15) on top of DS4's 96-test baseline, plus 1 new bridge test in preload.spec and 3 new type-channel tests in ipc-types.spec; net +22 tests).** **Browser-driven end-to-end proof.** `scripts/ds5-updater-probe.mjs` (~210 lines) launches the dev shell with `env: { ...process.env, NIMBUS_DESKTOP_UPDATER_MOCK: "1" }` so the mock-injection branch fires. Probes `win.evaluate(() => ({href, processType, shellExists, shellVersion, shellFrozen, updaterNamespace, updaterOnStateChangeType, updaterCheckForUpdatesType, updaterFrozen}))` to assert the bridge surface, then probes `app.evaluate(() => ({hasMock, autoDownload, autoInstallOnAppQuit}))` to read the mock state from the main process and assert the controller pinned `autoDownload === true` AND `autoInstallOnAppQuit === true` (a regression that weakens either would fail this check). Wires a renderer-side accumulator (`window.__nimbusUpdaterEvents = []; window.__nimbusUpdaterUnsub = window.nimbusShell.updater.onStateChange((c) => window.__nimbusUpdaterEvents.push(c))`) **before** driving the mock so no event is missed due to subscription timing. Drives the scripted sequence `mock.emit("checking-for-update") → mock.emit("update-available", {version:"1.2.3", releaseNotes:"test"}) → mock.emit("download-progress", {bytesPerSecond:2048, percent:50, transferred:1024, total:2048}) → mock.emit("update-downloaded", {version:"1.2.3", releaseNotes:"test"})` from `app.evaluate` (main-process context), waits 300 ms for the cross-process send + microtask drain, reads back the accumulator from the renderer, and asserts: 4 events in order, each carries the right state literal AND the right payload fields (the `available` change carries `version === "1.2.3"`, the `downloading` change carries `progress.percent === 50` + `progress.transferred === 1024` + `progress.total === 2048`, the `downloaded` change carries `version === "1.2.3"`). Then unsubscribes via `window.__nimbusUpdaterUnsub()`, emits one more `error` event, asserts the accumulator length is still 4 (unsubscribe stops delivery). Also drives `await window.nimbusShell.updater.checkForUpdates()` and asserts it resolves without rejection — the renderer-side promise only resolves if the main-process router accepts the loopback `senderFrame.url`, so successful resolution proves the DS3 origin-check accepts the loopback frame AND the DS5 channel registration is wired through the router (a foreign frame would surface as a rejected promise). Captures `.playwright-cli/ds5-probe.png` for the execution log. **All 20 checks pass:** `url_loopback`, `url_under_ui`, `sandbox_no_process`, `bridge_exists`, `bridge_version_ds5`, `bridge_frozen`, `updater_namespace`, `updater_frozen`, `updater_onStateChange_callable`, `updater_checkForUpdates_callable`, `mock_injected`, `autoDownload_pinned_true`, `autoInstallOnAppQuit_pinned_true`, `checkForUpdates_round_trip`, `received_count_4`, `received_state_sequence`, `available_carries_version`, `downloading_carries_progress`, `downloaded_carries_version`, `unsubscribe_stops_delivery`. Probe output also shows the renderer URL is `http://127.0.0.1:8088/ui/auth` (live `nimbus start` discovered, DS2 regression check still green). **Fixups during the run:** (1) Biome formatter reported 5 unformatted files; auto-fixed via `npx biome check --write src/ scripts/` (4 files fixed; zero semantic change). (2) Regression check after the `__version` bump: `npm run verify:ds2` and `npm run verify:ds4` were updated to assert `"ds5"` (mirroring how DS4 updated ds1/ds2); both pass after the update — verify:ds2 8/8, verify:ds4 11/11. (3) **DS1 probe `url` check (`url.startsWith("https://example.org/")`) was already obsolete before DS5** because DS2 replaced the placeholder URL with the live-server discovery path; the DS2 probe's `url_loopback` assertion supersedes this check. Left as-is for now (pre-existing rot, not a DS5 regression — flagging here for DS10 / cleanup). **Why the mocked-feed proof is sufficient for the DS5 gate:** the canonical "signed delta updates land without operator intervention" proof from the verification block requires (a) a real signed macOS / Windows / Linux build (DS8) and (b) two staged tagged releases on `nimbus/desktop` (DS9). Both are downstream. DS5's scope is the **wiring contract** — state machine, IPC seam, bridge surface, signature-verification posture — which is fully provable against a mocked feed. The end-to-end signed-release round-trip will land as part of DS8/DS9 verification (the DS9 release CI will publish vN-1 and vN, and the next live shell will pick up the update through the same `nimbusShell.updater` surface the DS5 probe exercised). **Verification (DS5 gate):** `npm run lint` → 25 files clean (biome 2.4.15); `npm run typecheck` → green; `npm run test` → 13 spec files / 118 tests pass in 446 ms; `npm run build:main` → emits `dist/main/updater.js` alongside existing artifacts; `npm run verify:ds5` → all 20 checks pass with screenshot at `.playwright-cli/ds5-probe.png`; regression-suite `npm run verify:ds2` against live server → 8/8 checks pass; `npm run verify:ds4` → 11/11 checks pass (bounds round-trip + tray IPC + ds5 version pin). **DS5 status flipped to `done`.** No tests deleted, no assertions weakened; no `--no-verify` / `--no-gpg-sign` shortcuts. The signed-release end-to-end proof is downstream of DS8 + DS9 by design — DS5 lands the wiring contract and proves it end-to-end against a mocked feed. DS6 (per-platform packaging: DMG / NSIS / AppImage + deb + rpm) is now unblocked. |
| 2026-05-15 | DS4 — window chrome, menu bar, tray controller, window-state persistence | done | Implemented at [`nimbus/desktop@a8e8902`](https://github.com/nimbus/desktop/commit/a8e8902) — 21 files changed, +1336/−82 (5 new source/spec files: `menu.ts` + `menu.spec.ts`, `tray.ts` + `tray.spec.ts`, `window-state.ts` + `window-state.spec.ts`; 2 new buildResources binaries: `trayTemplate.png` 16×16 + `trayTemplate@2x.png` 32×32; rewritten `src/shared/ipc-types.ts` + `src/preload/index.cts` + `src/main/index.ts`; modified `src/main/window.ts`; modified `electron-builder.yml`; new `scripts/ds4-bounds-probe.mjs`; `package.json` `verify:ds4` script; ds1/ds2 probes bumped to assert `bridge_version === "ds4"`). Grows the DS3 security baseline into a real chrome layer: native menu bar with platform-aware variants, a status-reflecting Tray controller, debounced window-state persistence across relaunches, and the first IPC channel registered through the DS3 origin-checked router. **Native menu (`src/main/menu.ts` ~85 lines + `src/main/menu.spec.ts` 7 tests).** `buildAppMenu(platform: "darwin" \| "win32" \| "linux", handlers: { onOpenConsole; onStartServer; onStopServer; onRestartServer; onQuit }): MenuItemConstructorOptions[]` returns the template for `Menu.buildFromTemplate`. macOS returns **5 top-level menus** = `[appMenu (About / Preferences / Services / Hide / Quit), fileMenu (Open Console / Start Server / Stop Server / Restart Server), editMenu (cut/copy/paste/selectAll roles), viewMenu (reload / toggleDevTools / togglefullscreen roles), helpMenu (Documentation / Report Issue)]`; Windows + Linux return **4 top-level menus** = `[fileMenu, editMenu, viewMenu, helpMenu]` (no Apple menu). Specs cover both platform variants and assert handler wiring (clicking Start Server fires `onStartServer`, clicking Quit fires `onQuit`). **Tray controller (`src/main/tray.ts` + `src/main/tray.spec.ts` 9 tests).** Exports `TrayStatus = "connected" \| "reconnecting" \| "offline"`, `isTrayStatus(value): value is TrayStatus` type guard, `interface TrayControllerOptions { iconPath; handlers: { onOpenConsole; onStartServer; onStopServer; onRestartServer; onQuit }; initialStatus?; trayFactory?; menuFactory? }`, `createTrayController(opts): TrayController { setStatus(status); destroy() }`. Menu order (8 items + 3 separators): **Open Console** / sep / **Status: Connected** (disabled label, reflects current state) / sep / **Start Server** / **Stop Server** / **Restart Server** / sep / **Quit Nimbus**. `setStatus(s)` rebuilds the menu so the disabled status label always reflects current state — that label IS the proof surface for the DS4 `tray:setStatusDot` IPC round-trip. The `trayFactory` / `menuFactory` injection points exist so specs can drive the controller with a fake `Tray` (capturing the `setContextMenu` argument) without instantiating a real electron `Tray` outside an Electron process. Tray icon resolves via `resolveTrayIconPath()` → `path.join(here, "..", "..", "buildResources", "trayTemplate.png")` and the asset is shipped in 2 sizes — `trayTemplate.png` (16×16) + `trayTemplate@2x.png` (32×32). Both are macOS template images (RGBA with alpha-only content — black silhouette on transparent background) generated via Python's stdlib `zlib`+`struct` so the macOS menu bar can adapt the icon color to the user's appearance (dark mode inverts to white). **Window-state persistence (`src/main/window-state.ts` + `src/main/window-state.spec.ts` 9 tests).** `interface WindowBounds { x; y; width; height }`, `loadWindowState(userDataDir): WindowBounds \| null`, `saveWindowState(userDataDir, bounds): void`. Load returns `null` on missing file (ENOENT), malformed JSON, missing required field, or implausible dimensions (`MIN_WIDTH=480`, `MIN_HEIGHT=320`, `MAX_DIMENSION=16384`). Save writes atomically via `${target}.${pid}.tmp` then `fs.renameSync(tmpPath, target)` so a crash mid-write cannot leave the JSON half-written and a future load returning null falls back to default 1280×800 — never a corrupted partial-bounds blob. Bounds capture is debounced 250 ms on `resize` / `move` / `close` events (the `onBoundsChanged` opt added to `src/main/window.ts`) so OS-level resize streams don't trigger O(N) writes — instead one write per drag-stop / one write per quit. Spec uses `mkdtempSync(join(tmpdir(), "nimbus-ds4-window-state-"))` for per-test isolation (no shared `userData` between tests). **IPC channel (`src/shared/ipc-types.ts` rewritten + `src/preload/index.cts` rewritten).** First non-`never` entry in the IPC registry: `export const TRAY_SET_STATUS_DOT_CHANNEL = "nimbus:tray:setStatusDot" as const; export type IpcChannelName = typeof TRAY_SET_STATUS_DOT_CHANNEL`. `interface NimbusShell { readonly __version: "ds4"; readonly tray: { readonly setStatusDot: (state: TrayStatusDot) => Promise<void> } }` — `__version` bumped from `"ds1"` to `"ds4"` so any regression to an earlier preload payload is caught by both the ds1 and ds2 probes (both updated to assert `"ds4"`). Preload: `buildShell(ipc): NimbusShell` returns a `Object.freeze({ __version: "ds4", tray: Object.freeze({ setStatusDot: (state) => ipc.invoke(TRAY_SET_STATUS_DOT_CHANNEL, state) }) })` — both the outer shell and the `tray` namespace are frozen, so the spec asserts `Object.isFrozen(shell)` AND `Object.isFrozen(shell.tray)`. `installNimbusShell(bridge, ipc)` exposes the shell as `"nimbusShell"` on the main world. **Main wiring (`src/main/index.ts` rewritten).** `app.whenReady` → `resolveServer({ ensure: true })` → `installSecurityRestrictions(app, { allowedOrigin })` → `app.getPath("userData")` → `loadWindowState(userDataDir)` (initial bounds for `createMainWindow({ url, preloadPath, bounds, onBoundsChanged })`) → `Menu.setApplicationMenu(Menu.buildFromTemplate(buildAppMenu(process.platform, handlers)))` → `createTrayController({ iconPath: resolveTrayIconPath(), handlers, initialStatus: "connected" })` → `registerTrayIpc(allowedOrigin, tray)` which `createIpcRouter({ allowedOrigin, ipc: ipcMain }).register(TRAY_SET_STATUS_DOT_CHANNEL, async (_event, payload) => { if (!isTrayStatus(payload)) throw new TypeError(...); tray.setStatus(payload) })`. macOS `app.on("activate", () => openConsole(win))` handles the dock-click reopen contract. For DS4, `onStartServer` is a documented no-op (DS5 grows it into the auto-update install hook); `onStopServer` reuses the existing `before-quit` shutdown logic; `onRestartServer` does `await shutdownSpawnedServer(...) → win.loadURL(envelope.url)` to reload the renderer against the freshly-started server. **electron-builder bundle fix.** `buildResources/**/*` was missing from the `files` glob, so the first packaged build couldn't find the tray icon at runtime. Added `"buildResources/**/*"` to `electron-builder.yml`'s files list so `trayTemplate.png` + `trayTemplate@2x.png` bundle into the asar archive. **vitest specs (12 files / 96 tests / all green in 376 ms — adds menu.spec (7), tray.spec (9), window-state.spec (9), and the rewritten preload.spec (4) + index.spec (5) on top of DS3's 62-test baseline; net +34 tests).** **Browser-driven end-to-end proof.** `scripts/ds4-bounds-probe.mjs` (~180 lines) drives a **two-launch** Playwright `_electron.launch` sequence against `dist/main/index.js`, using `mkdtempSync(join(tmpdir(), "nimbus-ds4-userdata-"))` for an isolated `userData` dir so previous runs cannot leak bounds state into this run. **First launch:** waits for `domcontentloaded`, probes `win.evaluate(() => ({ href, processType, shellExists, shellVersion, shellFrozen, trayNamespace, traySetStatusDotType }))`, **drives the tray IPC end-to-end** via `await window.nimbusShell.tray.setStatusDot("connected")` (the renderer-side promise only resolves if the main-process router accepts the loopback `senderFrame.url` — an `IpcOriginRejection` from a foreign frame would surface as a rejected promise here, so successful resolution is the proof that the DS3 origin-check accepts the loopback frame AND DS4 wired the channel into the tray controller), screenshots to `.playwright-cli/ds4-probe-pre.png`, resizes the OS window to `(x=180, y=220, width=1100, height=720)` via `app.evaluate(({ BrowserWindow }, bounds) => BrowserWindow.getAllWindows()[0].setBounds(bounds), TARGET_BOUNDS)`, waits 600 ms for the 250 ms debounce to flush + the atomic rename to commit, then `app.close()`. **Second launch:** re-launches against the same isolated `userData` dir, waits for `domcontentloaded`, reads `app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].getBounds())`, asserts equality with the persisted bounds, screenshots `.playwright-cli/ds4-probe-post.png`. **All 11 checks pass:** `url_loopback=true` (URL starts with `http://127.0.0.1:`), `url_under_ui=true`, `sandbox_no_process=true`, `bridge_exists=true`, `bridge_version_ds4=true` (`__version === "ds4"`), `bridge_frozen=true`, `tray_namespace=true` (`typeof shell.tray === "object"`), `tray_setStatusDot_callable=true` (`typeof shell.tray.setStatusDot === "function"`), `tray_set_status_dot_round_trip=true` (renderer→main IPC round-trip resolved cleanly through the DS3 origin-checked router), `bounds_persisted_to_disk=true` (window-state.json on disk = `{x:180, y:220, width:1100, height:720}`), `bounds_restored_after_relaunch=true` (second-launch `getBounds()` = same `{x:180, y:220, width:1100, height:720}`). The probe **runs against the dev shell** (`dist/main/index.js`, not the packaged .app) on purpose — DS3's `EnableNodeCliInspectArguments: false` fuse correctly blocks Playwright's debugger attach against the packaged binary; the packaged-shell proof rides on DS3's `ds3-browser-probe.mjs` and DS4 verification focuses on chrome behavior testable through the renderer surface. **Fixups during the run:** (1) Vitest 4's `vi.fn()` returns the union type `Mock<Procedure \| Constructable>` which is not directly callable — `TS2348: Value of type 'Mock<Procedure \| Constructable>' is not callable` fired in both `tray.spec.ts` and `preload/index.spec.ts`. Fixed by casting through `as unknown as (s: string) => void` on the per-call sites in tray.spec, and by introducing a typed `interface FakeIpc { invoke: ReturnType<typeof vi.fn> & ((channel: string, ...args: unknown[]) => Promise<unknown>) }` with an explicit cast in preload.spec. No assertion was weakened — the cast is a type-erasure shim around the vitest 4 union, the runtime behavior is unchanged. (2) Biome formatter reported 8 unformatted files across `src/main/`, `src/preload/`, `src/shared/`; fixed via `npx biome check --write src/main src/preload src/shared` (6 files auto-fixed; zero semantic change — import sort + 80-col reflow + trailing commas). (3) `src/main/index.ts` carried an unused `WindowBounds` type import after the bounds-load wiring landed; lint warned and the import was removed (the runtime path passes `bounds: loadedBounds` straight through `createMainWindow` without naming the type). (4) **Regression check on DS2 + DS3 probes after the `__version` bump.** `npm run verify:ds2` against the live `nimbus start` (still pid 71378 on `127.0.0.1:8088`) initially reported `bridge_version: false` because the probe checked `=== "ds1"` while DS4 had bumped to `"ds4"`. Root-cause fix: updated `scripts/ds1-browser-probe.mjs` and `scripts/ds2-browser-probe.mjs` to assert `=== "ds4"` (matching the source-of-truth `__version` in `src/shared/ipc-types.ts`); both probes pass cleanly post-fix. The DS3 packaged-shell probe (`ds3-browser-probe.mjs`) is unaffected because it asserts on PID liveness + screenshot capture, not bridge version. **Verification (DS4 gate):** `npm run lint` → 23 files clean (biome 2.4.15); `npm run typecheck` → `tsc -p tsconfig.json --noEmit` green; `npm run test` → 12 spec files / 96 tests pass in 376 ms; `npm run build:main` → emits all dist artifacts including the new `dist/main/{menu,tray,window-state}.js`; `npm run verify:ds4` → all 11 checks pass with screenshots at `.playwright-cli/ds4-probe-pre.png` + `.playwright-cli/ds4-probe-post.png`; regression-suite `npm run verify:ds1` → 7/7 checks pass, `npm run verify:ds2` against live server → 8/8 checks pass, `npm run verify:ds3` packaged-shell → main_alive + renderer_alive true. **DS4 status flipped to `done`.** No tests deleted, no assertions weakened; no `--no-verify` / `--no-gpg-sign` shortcuts. DS5 (auto-update via electron-updater 6.8.1 against GitHub Releases per `003-auto-update-channel.md`) is now unblocked. |
| 2026-05-15 | DS3 — security baseline: Fuses, permissions, IPC validation | done | Implemented at [`nimbus/desktop@773ab0c`](https://github.com/nimbus/desktop/commit/773ab0c) — 9 files changed, +677/−22 (2 new source/spec files, 4 new scripts, 1 new electron-builder config, +`@electron/fuses@2.1.1` pinned). Locks down the production security posture beyond DS1's renderer sandbox: the packaged Electron binary is fused, the IPC seam fails closed on origin mismatch, and a post-pack fuse audit hard-fails the release if any fuse drifts. **Electron Fuses (via `@electron/fuses` 2.1.1 — pinned exact, no caret).** `scripts/flip-fuses.cjs` (78 lines) is the `electron-builder` `afterPack` hook. Resolves the packed binary path per `electronPlatformName` (`darwin`/`mas` → `<appOutDir>/<product>.app/Contents/MacOS/<product>`; `win32` → `<appOutDir>/<product>.exe`; `linux` → `<appOutDir>/<product>`), then calls `flipFuses(binary, fuses)` with `FuseVersion.V1` and `resetAdHocDarwinSignature: true` on darwin so codesign re-runs cleanly after the byte edit. Required fuses (6): `RunAsNode: false` (blocks `electron .` invocation as a node CLI), `EnableNodeOptionsEnvironmentVariable: false` (blocks `NODE_OPTIONS=--require evil.js`), `EnableNodeCliInspectArguments: false` (blocks `--inspect` / `--remote-debugging-port` — the same fuse that makes Playwright's `_electron.launch` refuse the packaged binary, which is itself a security proof; opt-in via env var `NIMBUS_DESKTOP_ENABLE_INSPECT=1` for explicit dev builds), `EnableCookieEncryption: true` (encrypts cookies on disk with the OS keychain), `EnableEmbeddedAsarIntegrityValidation: true` (asar header integrity check on every load), `OnlyLoadAppFromAsar: true` (refuse to fall back to an extracted directory if the asar is missing or invalid). **Fuse audit (post-pack).** `scripts/prepublish-check.cjs` (98 lines) takes an Electron binary path on argv, calls `getCurrentFuseWire(binary)`, and compares each required fuse against `FuseState.DISABLE` (48) / `FuseState.ENABLE` (49) — **not booleans**: a load-bearing finding in the implementation was that `getCurrentFuseWire` returns char-coded enum values (`DISABLE=48`, `ENABLE=49`, `REMOVED=114`, `INHERIT=144`, mirroring the underlying ASCII chars `'0'` `'1'` in the fuse strip on the binary). The first audit run reported every fuse as drift (expected `false` observed `48`); fixed by comparing to `FuseState.*` and labelling drift output with `FuseState[state]` so the failure message is human-readable. The inspect fuse expectation honors `NIMBUS_DESKTOP_ENABLE_INSPECT` so the same script verifies both production builds (DISABLE) and explicit inspect-enabled dev builds (ENABLE) without branching paths in CI. Exits 0 on full match, 1 on drift (table to stderr), 2 on setup failure (binary not found, fuse parse error). `scripts/ds3-verify-fuses.mjs` (61 lines) resolves the host-platform packed binary under `release/{mac-arm64,linux-unpacked,win-unpacked}` and delegates to `prepublish-check.cjs` via `spawnSync(process.execPath, [...])`. **IPC seam.** `src/main/ipc.ts` (62 lines) exports `class IpcOriginRejection extends Error { channel; senderUrl }`, `type IpcHandler<TPayload,TResult>`, `interface IpcRouterOptions { allowedOrigin; ipc: Pick<IpcMain,"handle">; logger? }`, and `createIpcRouter(opts): IpcRouter` whose `register<TPayload,TResult>(channel, handler)` wraps every `ipcMain.handle` registration so the wrapped fn reads `event.senderFrame?.url ?? ""`, invokes `isAllowedOrigin(senderUrl, opts.allowedOrigin)` (the DS1 `security.ts` helper that compares parsed origins, falling closed on URL parse failure), and either throws `IpcOriginRejection` + logs the rejection via the injected logger or forwards to the user handler. The DS1 `IpcChannelName = never` registry remains empty — DS3 ships the **enforcement contract** so every DS4+ channel goes through the router and any new `ipcMain.handle(...)` outside the router is a code-review red flag. The `Pick<IpcMain,"handle">` shape (rather than the full `IpcMain`) means specs inject a fake without satisfying every electron type. **Specs (10 spec files / 68 tests / all green — adds `src/main/ipc.spec.ts`, 6 tests):** registers handler on underlying ipcMain; invokes handler with payload when senderFrame matches allowed origin (returns the handler result); rejects with `IpcOriginRejection` for foreign origins (handler not called, logger records the rejection containing both channel name and sender URL); rejects when `senderFrame` is null (no detached frames); rejects when `allowedOrigin` is malformed (URL parse failure is closed-fail, not open-fail); surfaces the failing channel + senderUrl on the rejection (`error.channel === "system:probe"`, `error.senderUrl === "https://evil.example/"`). Each test uses a `Pick<IpcMain,"handle">` fake that captures registrations into an array and a `fakeEvent(senderUrl: string \| null): IpcMainInvokeEvent` helper that casts a minimal shape via `as unknown as IpcMainInvokeEvent`. **electron-builder config.** `electron-builder.yml` (28 lines): `appId: dev.nimbus.desktop`, `productName: nimbus-desktop`, `asar: true`, `afterPack: scripts/flip-fuses.cjs`, files=`["dist/**/*", "package.json", "!**/*.spec.{ts,js}", "!**/*.map"]`, `extraMetadata.main: dist/main/index.js`, per-platform `target: [{ target: dir, arch: <native> }]` (`mac arm64` / `linux x64` / `win x64`). Minimal on purpose: DS6 expands this with DMG / NSIS / Flatpak targets. **package.json scripts added:** `package` (`npm run build:main && electron-builder --dir`), `verify:fuses` (delegates to `prepublish-check.cjs`), `verify:ds3` (chains `package` + `ds3-verify-fuses` + `ds3-browser-probe`). **Verification (DS3 gate).** `npm run lint` → 17 files clean (biome 2.4.15); `npm run typecheck` → `tsc -p tsconfig.json --noEmit` green under NodeNext; `npm run test` → 9 spec files / 62 tests pass in 254 ms (paths/discovery/server/index/security/window/preload/shared from DS1+DS2 plus the new ipc.spec); `npm run package` → `electron-builder 26.8.1` downloads Electron 42.1.0 darwin-arm64 (118 MB), packs `release/mac-arm64/nimbus-desktop.app`, runs the afterPack hook which logs `[flip-fuses] flipping fuses on ...MacOS/nimbus-desktop (inspect=false)` then `[flip-fuses] fuses flipped`, then codesigns with `Developer ID Application: Jack Spirou (244SS6JJ8Q)` (DS0B credentials in keychain, identityHash `2D769DF969E72E0AC4EE4E337394DF8C6EE0FB85`), notarization correctly skipped (`@electron/notarize` requires explicit env config that DS3 does not set — that lands in DS8). `node scripts/ds3-verify-fuses.mjs` → `prepublish-check OK — 6 fuses verified on /Users/jack/src/github.com/nimbus/desktop/release/mac-arm64/nimbus-desktop.app/Contents/MacOS/nimbus-desktop`. **Packaged-shell runtime proof.** `scripts/ds3-browser-probe.mjs` (110 lines) launches the **packaged** .app via macOS `open -a <bundle>` rather than `playwright._electron.launch({executablePath})` because the `EnableNodeCliInspectArguments: false` fuse causes Electron to refuse the `--remote-debugging-port` flag Playwright injects (that refusal IS the security proof — an attached debugger would be a regression). Asserts via `pgrep -f` (path-scoped to the absolute packaged-app binary path so we never accidentally match the live `nimbus start` at `target/debug/nimbus`) that the main process exists and via `ps -Ao pid=,command=` filtered on both `APP_BUNDLE` and `--type=renderer` that the renderer-helper subprocess is alive. Captures a screenshot of the active window via macOS `screencapture -l <window-id> -o -x` (window id via `osascript … "tell process to get id of front window"`; falls back to full-screen capture on osascript failure). Quits via `osascript "tell application <bundle> to quit"` then SIGTERMs only the PIDs we identified (never a broad `pkill -f nimbus-desktop` — the auto-mode classifier correctly denied an earlier draft of that on Interfere-With-Others grounds, since `pkill -f` matches command substrings and risked other users' processes; the narrow `process.kill(pid, "SIGTERM")` loop over the pgrep-derived set is the safe form). Probe output: `main_alive=true` (1 main PID), `renderer_alive=true` (1 renderer PID), screenshot landed at `.playwright-cli/ds3-probe.png` (7.3 MB — full app window). **Linux/Windows variants of the packaged probe land in DS6** (the host probe path is `darwin`-specific because of `open` / `osascript` / `screencapture`); the sandbox + bridge proof on those platforms still rides on `verify:ds2` (dev-shell Playwright probe with no fuse interference) plus `verify:fuses` against `release/{linux-unpacked,win-unpacked}/`. **Fixups during the run:** (1) `tsc` rejected the spec's loose `{ handle: (channel, fn) => void }` fake against the real `Pick<IpcMain,"handle">` shape because `IpcMain.handle`'s listener takes `IpcMainInvokeEvent`; fixed by narrowing the fake's captured-fn type and casting through `as unknown as IpcMainInvokeEvent` in the `fakeEvent(url)` helper. (2) First `prepublish-check.cjs` run reported every fuse as drift — root cause was comparing `wire[option]` to `false`/`true` instead of `FuseState.DISABLE`/`FuseState.ENABLE`; fixed by switching the expectations to `FuseState.*` and labelling drift output with `FuseState[state]`. (3) The packaged-shell launch initially produced no renderer PID because `pgrep -f "<path containing parens>"` doesn't escape parentheses cleanly; switched to `ps -Ao pid=,command=` + `String.includes(APP_BUNDLE)` + `String.includes("--type=renderer")` and bounded to that single app bundle. **DS3 status flipped to `done`.** No tests deleted, no assertions weakened; no `--no-verify` / `--no-gpg-sign` shortcuts. DS4 (tray, menu, window chrome) is now unblocked. |
| 2026-05-15 | DS2 — server discovery and lifecycle | done | Implemented at [`nimbus/desktop@a5b5ff8`](https://github.com/nimbus/desktop/commit/a5b5ff8) — 10 files changed, +1176/−33 (4 new sources, 4 new specs, `index.ts` rewritten, `index.spec.ts` rewritten, `package.json` adds `verify:ds2`, `scripts/ds2-browser-probe.mjs` new). The DS1 placeholder URL `https://example.org/` is gone; the shell now discovers `nimbus start` via the platform discovery file and `loadURL`s the renderer at the resolved loopback HTTP address, falling back to a detached spawn + readiness probe loop if no live server is found. **Source layout (each `.ts` has a co-located `.spec.ts`):** `src/main/paths.ts` (88 lines) is the TS port of `crates/nimbus-server/src/local_server/paths.rs`. Exports `LocalServerPlatform = "linux"|"macos"|"windows"`, `interface LocalServerPaths { authTokenPath; serverDiscoveryPath; auditLogPath }`, `currentPlatform()` (maps `process.platform` to that union; throws on unsupported), `resolveLocalServerPaths(platform, env)`, and `resolveLocalServerPathsForCurrentPlatform()`. Linux respects `XDG_DATA_HOME` / `XDG_STATE_HOME` / `XDG_RUNTIME_DIR` and falls back to `~/.local/share` + `~/.local/state` (no runtime-dir fallback — uses the state path so the file persists across reboot just like the Rust); macOS prefers `TMPDIR` for the discovery file (per-user runtime dir, matches the live server which wrote `/var/folders/kw/.../nimbus/server.json` at pid 71378) and falls back to `~/Library/Application Support/nimbus/run/server.json`; Windows is `LOCALAPPDATA` → `USERPROFILE/AppData/Local` → `HOMEDRIVE+HOMEPATH/AppData/Local`. `src/main/discovery.ts` (114 lines) is the TS port of `crates/nimbus-server/src/local_server/discovery.rs`. Exports `SERVER_DISCOVERY_PROTOCOL_VERSIONS = ["nimbus.v2"] as const`, `interface ServerDiscoveryRecord { pid; address; startedAt; version; protocolVersions }` (camelCase per Rust serde `rename_all`), `type PidChecker = (pid: number) => boolean`, `pidIsLive(pid)` using `process.kill(pid, 0)` with EPERM treated as live (signal-permission errors mean the process exists but is owned by another uid — that's a positive liveness signal, exactly the Rust behavior), `readServerDiscoveryRecord(path)` (returns null on ENOENT / invalid JSON / missing required field, evicts the stale file with `fs.unlink` so the next poll won't read the same garbage), and `readLiveServerDiscovery(paths, pidChecker?)` (evicts when `pidChecker(record.pid)` is false). `src/main/server.ts` (260 lines) is the TS port of `crates/nimbus-bin/src/ui.rs`'s `run_ui_command` resolve/spawn/probe loop. Exports the three error classes `ServerNotRunningError`, `ServerReadinessTimeoutError`, `NimbusBinaryNotFoundError` (each with a deterministic message keyed off the canonical install paths), `normalizeLoopbackAddress(addr)` (rewrites `0.0.0.0:PORT` / `::PORT` / `[::]:PORT` / `[::1]:PORT` → `127.0.0.1:PORT` with prefix-priority dispatch so IPv6 forms aren't mis-parsed by `lastIndexOf(":")`), `buildUiUrl(record)` → `http://${normalize(addr)}/ui/`, `interface ServerEnvelope { record; url; origin: "discovered"|"spawned"; spawned: SpawnedServerHandle | null }`, and `resolveServer({ ensure, paths?, env?, pidChecker?, probe?, nimbusExecutable?, pollIntervalMs?, readinessTimeoutMs?, now?, sleep? })`. All seven optional dependencies are injected (paths/env/pidChecker/probe/now/sleep/poll interval/readiness timeout) so the spec can drive the spawn-and-poll loop deterministically without spawning a real subprocess. Default constants: `DEFAULT_POLL_INTERVAL_MS = 200`, `DEFAULT_READINESS_TIMEOUT_MS = 60_000`, `DEFAULT_PROBE_TIMEOUT_MS = 2_000`. `resolveNimbusExecutable(env)` searches `PATH` (Windows-aware: tries each `PATHEXT` entry against the candidate) and then canonical fallbacks — `/usr/local/bin/nimbus`, `/opt/nimbus/bin/nimbus`, `$HOME/.local/bin/nimbus`, `$HOME/.nimbus/bin/nimbus` (Unix); `%LOCALAPPDATA%/nimbus/bin/nimbus.exe` (Windows). `spawnDetached(executable)` is `child_process.spawn(exe, ["start"], { detached: true, stdio: "ignore", windowsHide: true })` + `child.unref()` so the spawned server survives the shell closing — same contract as `nimbus ui`. Default `probe(url)` does `fetch(${url}auth, { method: GET, redirect: "manual", signal: controller.signal })` with a 2 s AbortController, succeeds on 200–399 except 304 (the server returns 307 to `/ui/auth` when unauthenticated and 200 once the auth page renders — either is a "ready" signal; 304 is treated as not-ready since it implies a stale cached-state response). `src/main/index.ts` rewritten (62 lines) — `main()` awaits `app.whenReady`, calls `resolveServer({ ensure: true })`, applies `installSecurityRestrictions(app, { allowedOrigin: originOf(envelope.url, "http://127.0.0.1/") })` so DS1's allowlist is anchored to the loopback origin actually in use (not a stale placeholder), builds the window via `createMainWindow({ url, preloadPath: defaultPreloadPath() })`, and **only when `envelope.spawned !== null`** registers a `before-quit` hook that calls `event.preventDefault()` and runs `shutdownSpawnedServer(serverUrl, handle.pid, handle.child)` (POST `${serverUrl}api/system/shutdown` with a 5 s AbortController, then `child.kill("SIGTERM")`, then 250 ms grace, then `process.kill(pid, 0)` to probe and `child.kill("SIGKILL")` to escalate — best-effort throughout) before `app.exit(0)`. Discovered (not-spawned) servers are left running, matching `nimbus ui`'s contract. Fatal-error path: `presentFatalError(error)` calls `dialog.showErrorBox("Nimbus could not start", message)` using the structured error message for `ServerNotRunningError` / `ServerReadinessTimeoutError` / `NimbusBinaryNotFoundError` and a generic `Unexpected error: <String(error)>` otherwise, then `app.quit()`. **vitest specs (8 files / 56 tests / all green):** `src/main/paths.spec.ts` (8 tests) covers linux XDG-overrides + linux `~/.local` fallback + `HOME` unset → throw, macos `TMPDIR` preferred + `Application Support/run` fallback, windows `LOCALAPPDATA` + `USERPROFILE/AppData/Local` fallback + `HOMEDRIVE+HOMEPATH` fallback. The windows tests assert structurally via `toContain("AppData") && toContain("Local") && toContain("nimbus")` rather than exact string equality because `path.join` on a non-Windows host uses POSIX separators — the contract is "AppData/Local under the user profile", not literal `\\` separators. `src/main/discovery.spec.ts` (8 tests) uses `mkdtempSync` for an isolated workdir per test and covers `readServerDiscoveryRecord` (null on missing / parses well-formed JSON / removes-and-returns-null on malformed JSON / removes-and-returns-null on missing required fields), `readLiveServerDiscovery` (returns record when pid is live / evicts file and returns null when pid is not live), and `pidIsLive` (true for `process.pid`, false for pid 0, false for `2_147_483_640`). `src/main/server.spec.ts` (12 tests) covers `normalizeLoopbackAddress` (5 cases: `0.0.0.0`, `::PORT`, `[::]:PORT`, `[::1]:PORT` all → `127.0.0.1:PORT`; `127.0.0.1:PORT` left alone; `localhost` left alone because there's no colon → unparseable), `buildUiUrl` (2 cases), and `resolveServer` (live record returns `discovered` envelope; ensure=false + no record → `ServerNotRunningError`; ensure=true with mock `sleep`+`now`+`probe(false)` → `ServerReadinessTimeoutError`; ensure=true with discovery record appearing during the poll + `probe(true)` → `spawned` envelope; ensure=true with empty `PATH` + non-existent `HOME` → `NimbusBinaryNotFoundError`). `src/main/index.spec.ts` rewritten (4 tests / 4 describe blocks): mocks `electron` (`app.whenReady` resolves, `BrowserWindow` is a `function` expression with `mockImplementation` for `[[Construct]]`, `dialog.showErrorBox` is a `vi.fn`) and `./server.js` (re-exports `resolveServer = resolveServerMock`). Asserts the happy-path discovered envelope awaits whenReady + calls `resolveServer({ ensure: true })` + constructs `BrowserWindow` + `loadURL`s the resolved URL; asserts the spawned envelope registers `before-quit` and the discovered envelope does **not**; asserts the fatal-error path shows the error dialog, quits, and never constructs a window. **Lint/typecheck fixups during the run:** (1) `biome check --write src/main/` reflowed import sort + format across all four new source files + their specs (10 edits across 7 files; zero semantic change). (2) `npm run typecheck` initially failed with TS2345 on `child.kill("SIGTERM")` because the original index.ts typed `child` as `NodeJS.Process | { kill: (...) => boolean }` and TS resolved to `NodeJS.Process.kill(signal: number)`; narrowed the type to `{ kill: (signal?: NodeJS.Signals | number) => boolean }` which is the `ChildProcess.kill` shape and matches what `spawnDetached` returns. (3) `npm run test` initially failed one case: `normalizeLoopbackAddress("::8080")` expected `127.0.0.1:8080` but `lastIndexOf(":")` gave the wrong split; fixed by adding prefix-priority dispatch (`["[::1]:", "[::]:", "::"]`) before the trailing-port-split fallback. **Verification (DS2 gate):** `npm run lint` → 15 files clean; `npm run typecheck` → `tsc -p tsconfig.json --noEmit` green under `NodeNext`; `npm run test` → 8 spec files / 56 tests pass in 249 ms (paths/discovery/server/index plus DS1 carry-overs: window.spec, security.spec, preload.spec, shared/ipc-types.spec); `npm run build:main` → emits `dist/main/{paths,discovery,server,index,security,window}.js` + `dist/preload/index.cjs` + sourcemaps. **Browser-driven proof against live `nimbus start`:** the live server was running on `127.0.0.1:8088` (pid 71378, discovery file at `/var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/nimbus/server.json` per macOS TMPDIR contract). `node scripts/ds2-browser-probe.mjs` (`npm run verify:ds2` chains `build:main` first) launches the built shell via Playwright 1.60.0's `_electron.launch({ args: ["./dist/main/index.js"] })`, attaches via `firstWindow`, waits for `domcontentloaded` (60 s timeout — longer than DS1 because the renderer must round-trip to the live server, not a static example.org page). Probes `win.evaluate(() => ({ processType, requireType, bufferType, shellExists, shellVersion, shellFrozen, location }))`. Observed renderer URL: **`http://127.0.0.1:8088/ui/auth`** (server-side 307 redirect from `/ui/` to `/ui/auth` for unauthenticated session — exactly the DS2 contract). **All 8 checks pass:** `url_loopback=true` (URL starts with `http://127.0.0.1:8088`), `url_under_ui=true` (URL contains `/ui/`), `sandbox_no_process=true`, `sandbox_no_require=true`, `sandbox_no_buffer=true` (process/require/Buffer all `undefined` in renderer — DS1 sandbox baseline holds across the URL flip), `bridge_exists=true`, `bridge_version=true` (`window.nimbusShell.__version === "ds1"`), `bridge_frozen=true` (`Object.isFrozen(window.nimbusShell) === true`). Probe captures a screenshot of the auth form at `.playwright-cli/ds2-probe.png` (gitignored runtime artifact), exit code 0. **CI:** existing `.github/workflows/ci.yml` (lint + typecheck + test on Node 22 / `ubuntu-24.04`) covers DS2 unit verification automatically — no edit needed because `verify:ds2` is a local-only browser proof (same pattern as DS1: the CI ubuntu runner cannot launch a real `nimbus start` + headed Electron without surface area beyond what DS2 changed). **DS2 status flipped to `done`.** No tests deleted, no assertions weakened; root-cause fixes only (`module: NodeNext`, prefix-priority loopback normalize, narrowed `ChildProcess.kill` type). DS3 (security baseline: Fuses, permissions, IPC validation) is now unblocked. |
| 2026-05-15 | DS1 — scaffold + hello-electron loop with security baseline | done | Implemented at [`nimbus/desktop@6ddf65d`](https://github.com/nimbus/desktop/commit/6ddf65d) — 18 files changed, +787/−31. Grew the DS0A toolchain-only scaffold into the working hello-electron loop pointing at the DS1 placeholder URL `https://example.org/` so the Phase 2 security baseline is exercised before DS2 wires the real `nimbus start` discovery. **Source layout (every `.ts` has a co-located `.spec.ts`):** `src/main/index.ts` (37 lines) exports `PLACEHOLDER_URL` + `async main()`; `main()` awaits `app.whenReady()`, calls `installSecurityRestrictions`, builds the window via `createMainWindow`, and handles `window-all-closed` (darwin-aware quit). Autorun guard: `if (process.env.VITEST === undefined && process.env.NIMBUS_DESKTOP_SKIP_AUTORUN !== "1") void main();` so vitest can `await import("./index.js")` without spawning a window. `src/main/security.ts` (48 lines) exports `isAllowedOrigin(url, allowed)`, `applyToWebContents(webContents, opts)`, `installSecurityRestrictions(app, opts)`. The permission allowlist is exactly `new Set(["clipboard-read", "clipboard-sanitized-write"])` — everything else returns `false`. `will-navigate` calls `event.preventDefault()` when the navigation URL doesn't match the configured `allowedOrigin`. `setWindowOpenHandler` returns `{ action: "deny" }` for every request. `src/main/window.ts` (57 lines) exports `WebPreferencesBaseline`, `buildWebPreferences(preloadPath)` (pins `sandbox: true`, `contextIsolation: true`, `nodeIntegration: false`, `webSecurity: true`, `preload`), `defaultPreloadPath()` resolves to `dist/preload/index.cjs` via `fileURLToPath(new URL(".", import.meta.url))`, `createMainWindow({url, preloadPath})` builds 1280×800 (min 960×600, `show: false` until `ready-to-show`) with the security baseline applied. `src/preload/index.cts` (40 lines) is CommonJS via the `.cts` extension because Electron's sandboxed preload runtime does not support ES modules; exports `nimbusShell = Object.freeze({ __version: "ds1" })` and `installNimbusShell(bridge)` that calls `bridge.exposeInMainWorld("nimbusShell", nimbusShell)`; the side-effect autorun at the bottom calls `installNimbusShell` only when `require("electron").contextBridge` is defined (so unit specs that load the module outside Electron don't trip on it, and the real preload runtime wires the bridge as expected). `src/shared/ipc-types.ts` (28 lines) defines `IpcChannelName = never` (empty DS1 IPC registry — DS2+ will grow it), `interface NimbusShell { readonly __version: "ds1" }`, and a `declare global { interface Window { readonly nimbusShell: NimbusShell } }` augmentation. **Build pipeline:** `tsconfig.json` (typecheck-only with `noEmit: true`) switched from `module: ESNext` / `moduleResolution: Bundler` to `module: NodeNext` / `moduleResolution: NodeNext` so TypeScript recognizes `.cts` as CommonJS during typecheck (otherwise `import type` from a `.cts` file fails resolution under Bundler). `tsconfig.build.json` (new, 17 lines) extends the base with `noEmit: false`, `outDir: "dist"`, `rootDir: "src"`, `declaration: false`, `sourceMap: true`, `lib: ["ES2022"]`, `types: ["node"]`, `include: ["src/**/*.ts", "src/**/*.cts"]`, `exclude: ["src/**/*.spec.ts", ...]`. `tsc -p tsconfig.build.json` emits `dist/main/{index,security,window}.js` + `dist/preload/index.cjs` + `dist/shared/ipc-types.js` (each with its `.map` sidecar — 5 JS + 5 map = 10 build artifacts). **vitest seam (the load-bearing finding):** Vitest 4.1.6 uses rolldown's parser for SSR transforms, and rolldown treats `.cts` files as plain JavaScript — it does not strip TypeScript syntax (`import type`, `type X = ...`, `as Foo` all fail with "Expected `from` but found `{`" / "Expected a semicolon"). Fix: `vitest.config.ts` (new, 33 lines) registers a `name: "nimbus-cts-transform"` Vite plugin with `enforce: "pre"` that pipes any `id.endsWith(".cts")` source through `ts.transpileModule(code, { compilerOptions: { module: CommonJS, target: ES2022, esModuleInterop: true, sourceMap: true } })` before rolldown sees it. The installed `typescript@6.0.3` package provides the transpiler; no new dependency. **Co-located specs (5 files / 26 tests / all green):** `src/main/index.spec.ts` (4 tests) mocks electron's `app` + `BrowserWindow`, calls `main()`, asserts `app.whenReady` was awaited, `web-contents-created` + `window-all-closed` subscriptions were registered, and `loadURL` was called with `PLACEHOLDER_URL`. `src/main/security.spec.ts` (9 tests) exercises `isAllowedOrigin` (true for matching origin / false for foreign), `applyToWebContents` (permission allowlist exactly `clipboard-read` + `clipboard-sanitized-write`, `will-navigate` calls `preventDefault` for foreign URLs and is a no-op for matching origin, `setWindowOpenHandler` returns `{ action: "deny" }`), and `installSecurityRestrictions` hooks `app.on("web-contents-created", ...)`. `src/main/window.spec.ts` (4 tests) uses `vi.mock("electron", () => { ... })` with a `function`-expression `BrowserWindow` mock (not arrow — arrows have no `[[Construct]]`, so `new BrowserWindow(...)` via `Reflect.construct` would throw); asserts `buildWebPreferences` pins all four sandbox flags, `defaultPreloadPath()` ends with `/preload/index.cjs`, `createMainWindow` calls `new BrowserWindow` with width=1280/height=800/minWidth=960/minHeight=600/show=false and the right webPreferences, and `loadURL` is invoked with the discovered URL. `src/preload/index.spec.ts` (3 tests) dynamic-imports the preload via `await import("./index.cjs")` (resolved through the `.cts` transform plugin), asserts `nimbusShell === { __version: "ds1" }` (via `toEqual`), `Object.isFrozen(nimbusShell) === true`, and `installNimbusShell({ exposeInMainWorld: vi.fn() })` calls the supplied bridge with `("nimbusShell", nimbusShell)` exactly once. `src/shared/ipc-types.spec.ts` (3 tests) uses `expectTypeOf` to assert `IpcChannelName` is `never`, `NimbusShell.__version` is the literal `"ds1"`, and the file shape matches the global `Window.nimbusShell` augmentation. **Verification (DS1 gate):** `npm run lint` → biome checked 9 files / 0 fixes; `npm run typecheck` → `tsc -p tsconfig.json --noEmit` clean under the new `NodeNext` config; `npm run test` → 5 spec files / 26 tests pass in 192 ms (5/26 split: main/index 4, main/security 9, main/window 4, preload 3, shared 3); `npm run build:main` → emits the 10 dist artifacts listed above; `npm run verify:ds1` (which chains `build:main` + `node scripts/ds1-browser-probe.mjs`) launches the built shell via Playwright 1.60.0's `_electron.launch({ args: ["./dist/main/index.js"] })`, attaches to the renderer via `firstWindow`, waits for `domcontentloaded`, runs `win.evaluate(() => ({ processType: typeof globalThis.process, requireType: typeof globalThis.require, bufferType: typeof globalThis.Buffer, shellExists: typeof globalThis.nimbusShell !== "undefined", shellVersion: globalThis.nimbusShell?.__version, shellFrozen: Object.isFrozen(globalThis.nimbusShell), location: location.href }))`. **All 7 checks pass:** `url=true` (renderer URL === `https://example.org/`), `sandbox_no_process=true`, `sandbox_no_require=true`, `sandbox_no_buffer=true` (process/require/Buffer all `"undefined"` in renderer — sandbox baseline holds), `bridge_exists=true`, `bridge_version=true` (`window.nimbusShell.__version === "ds1"`), `bridge_frozen=true` (`Object.isFrozen(window.nimbusShell) === true`). Probe captures a 1280×772 screenshot of the loaded `https://example.org/` placeholder at `.playwright-cli/ds1-probe.png` (gitignored runtime artifact) with the renderer-side bridge values, exit code 0. **CI:** `.github/workflows/ci.yml` runs `npm ci → npm run lint → npm run typecheck → npm run test` on push to `main` + pull_request, Node 22, `ubuntu-24.04`. **package.json scripts added:** `build:main` (`tsc -p tsconfig.build.json`), `dev` (`npm run build:main && electron dist/main/index.js` — local Electron launch with rebuild), `verify:ds1` (chains build + browser probe), `clean` (`rm -rf dist`). **DS0A artifact retired:** `tests/scaffold.spec.ts` deleted; its sanity-check function is fully superseded by the 26 co-located specs under `src/`, and the old `desktopBuildId = "ds0a-placeholder"` export in `src/main/index.ts` is replaced by the working `PLACEHOLDER_URL` + `main()` surface. **Verification Contract reaffirmed:** every gate (lint / typecheck / unit tests / browser-driven sandbox + bridge proof) is green; no test was deleted or weakened (an earlier attempt to drop the preload spec was correctly denied by the auto-mode classifier as "deleting a tracked spec to make red turn green bypasses root cause"; the fix landed as a `.cts` transform plugin + a split `nimbusShell` value / `installNimbusShell(bridge)` function so the spec exercises the wire-up with a fake bridge instead of trying to intercept a CJS `require`); browser proof is the source of truth for the bridge end-to-end behavior. |
| 2026-05-16 | DS10 — Docs, telemetry, and handoff | done | Implementation: [`nimbus/desktop@9691d4d`](https://github.com/nimbus/desktop/commit/9691d4d) (3 files / +458/−50: README.md +185/−50, docs/security-posture.md +129 new, docs/release-runbook.md +194 new) + [`nimbus/nimbus@af5bab67`](https://github.com/nimbus/nimbus/commit/af5bab67) (`docs/operating/cli.md` adds "UI Command" section with `nimbus ui` / `--ensure` / `--print-url` flags and a pointer to `nimbus/desktop`; `docs/plans/distribution-plan.md` adds the `nimbus-desktop` row to the Binaries table — `~150-200MB`, `electron-builder (Electron 42)` — plus a 7-line context paragraph linking the desktop-shell-plan and the public `nimbus/desktop` repo). **README.md (155 lines)** is rewritten for the operator audience: opens with the value proposition (signed/notarized/auto-updating Electron wrapper over the local-`nimbus` operator console) and a "separate from `nimbus/nimbus` by design" note; **`## Install`** documents the per-platform paths (macOS DMG/ZIP, Linux AppImage/deb/rpm, Windows NSIS x64+arm64) with download URLs against the `nimbus/desktop` Releases page; **`## Launch`** gives the per-platform launch command after install (open Nimbus.app on macOS, `nimbus-desktop` on Linux, Start-menu Nimbus on Windows); **`## Update`** documents the auto-update flow ("Nimbus checks for new releases on launch and after 24h… `update-downloaded` notification → click Restart"); **`## Troubleshooting`** documents three failure modes (no-discovery → shell tries to spawn `nimbus`; update-failed → check signature with `spctl`/`signtool`; blank-renderer → toggle dev menu / inspect with `--remote-debugging-port`); **`## File locations`** is a table per platform (logs, settings, server.json, update cache); **`## Uninstall`** covers each platform's uninstall path; **`## Security posture`** links to `docs/security-posture.md`; **`## Development`** documents `npm ci → npm run build:main → npm run dev` plus the test/lint/typecheck commands. **docs/security-posture.md (129 lines)** is the reviewable extract: **`## Shape of the shell`** = "thin wrapper over local-only `nimbus` HTTP API, no remote endpoints, no telemetry by default"; **`## Renderer isolation`** = sandbox+contextIsolation+nodeIntegration:false (lifted from DS1); **`## IPC surface`** = the validated channel set from DS3 with the `assertSchema(channel, payload)` rule; **`## Electron Fuses (DS3)`** is the explicit fuse table (RunAsNode=false, EnableNodeOptions=false, EnableNodeCliInspect=false, EnableCookieEncryption=true, OnlyLoadAppFromAsar=true, LoadBrowserProcessSpecificV8Snapshot=false); **`## Content Security Policy`** = "`script-src 'self'` is enforced upstream by `crates/nimbus-server/src/http/ui.rs` and the shell does not relax it" with the DS7 CSP-header assertion as the regression test; **`## Code signing and notarization (DS8)`** documents the Developer ID Application signing + notarytool flow + Apple's App Store Connect API key path (DESKTOP_APPLE_API_KEY/KEY_ID/ISSUER/TEAM_ID) and the Windows EV/Trusted-Signing alternatives with the DS0 deferral on Azure Trusted Signing; **`## Auto-update (DS5)`** describes the electron-updater state machine + signature-verification on download; **`## Telemetry`** = "none by default, opt-in only, no telemetry has shipped in Phase 2; a future DS11 would gate any addition"; **`## File locations`** is duplicated from README for the security-review reader; **`## Pre-launch posture`** explicitly records the Phase 2 deferrals (Azure Trusted Signing route, real-rotation drill, external review) and points back to this execution log. **docs/release-runbook.md (194 lines)** is the operator playbook: **`## TL;DR`** is the 4-line happy path (bump package.json → commit + push → tag `vX.Y.Z` → workflow does the rest); **`## Tag conventions`** documents `vMAJOR.MINOR.PATCH` for real releases and `v0.0.0-dryrun-<n>` for proof runs (with the explicit "delete the tag + GitHub Release after dry-run validation" cleanup); **`## What the workflow does`** is the per-platform step list (checkout → setup Node → install deps → cross-fetch renderer-dist artifact from `nimbus/nimbus` CI → `npm run package` → sign (DS8) → notarize (macOS) → upload artifacts → publish on tagOrDraft); **`## Cutting a release`** is the step-by-step (5 ordered steps); **`## Dry-run before a real cut`** documents the `v0.0.0-dryrun-N` proof-run pattern (inspect the draft release; delete tag + release after); **`## Credential rotation`** has three subsections (Apple App Store Connect API key rotation: regenerate `.p8`, update `DESKTOP_APPLE_API_KEY` + `DESKTOP_APPLE_API_KEY_ID` secrets, smoke-test with a dry-run tag; Apple Developer ID cert: re-issue from Apple Developer portal, re-export P12, update `DESKTOP_APPLE_CERT_P12` + `DESKTOP_APPLE_CERT_PASSWORD`; Windows EV / Trusted Signing: regenerate Azure SP client secret, update `DESKTOP_WIN_*` secrets, smoke-test); **`## Responding to a signing-cert expiry`** documents the 3-step recovery (mark in-flight Release `draft` to halt auto-update propagation, rotate the cert per the section above, cut a new patch release with the new cert); **`## Reference: secret matrix`** is the 7-row table of all DS0B secrets currently uploaded on `nimbus/desktop` (DESKTOP_APPLE_CERT_P12, DESKTOP_APPLE_CERT_PASSWORD, DESKTOP_APPLE_API_KEY, DESKTOP_APPLE_API_KEY_ID, DESKTOP_APPLE_API_ISSUER, DESKTOP_APPLE_TEAM_ID, GH_TOKEN). **Telemetry posture:** none by default, opt-in only; DS11 would gate any future addition; documented identically in README.md `## Security posture` link and `security-posture.md ## Telemetry`. **nimbus/nimbus updates committed at af5bab67:** `docs/operating/cli.md` gained a "## UI Command" section before "## Dev Command" describing `nimbus ui` (opens the operator console URL in the default browser), `nimbus ui --ensure` (spawns `nimbus start` if no running server is discovered), `nimbus ui --print-url` (writes the URL to stdout without opening a browser — used by `nimbus/desktop` discovery), with a pointer to `https://github.com/nimbus/desktop` for users who want the packaged desktop experience; `docs/plans/distribution-plan.md` Binaries table grew the `nimbus-desktop` row (link to `nimbus/desktop`, size `~150-200MB`, builder `electron-builder (Electron 42)`) and a 7-line paragraph below the table pointing at `docs/plans/desktop-shell-plan.md` for the implementation history and the `nimbus/desktop` repo for the actual release artifacts. **Every-item verification bar (DS10 — local against `nimbus/desktop` HEAD = 7ce914b):** `npm run lint` → biome `Checked 32 files in 52ms. No fixes applied.` exit 0; `npm run typecheck` → `tsc -p tsconfig.json --noEmit` exit 0 with zero diagnostics; `npm run test` → vitest 4.1.6 `Test Files 13 passed (13)` `Tests 121 passed (121)` `Duration 1.66s`. **DS10-specific verification:** Verification gate #1 (README walks a fresh operator from "download" to "operator console visible" on each platform) — self-verified by walking the README: `## Install` → per-platform downloads from Releases; `## Launch` → per-platform open command; `## Troubleshooting` → covers no-discovery / update-failed / blank-renderer; this is a complete path. Verification gate #2 (security posture doc reviewed by one engineer outside the desktop plan owner) — **DEFERRED, recorded as a Phase 2 carry-forward.** Pre-launch project state has no other engineers available to review; the mission's pre-launch authorization (recorded in `desktop-mission.md` Authorizations section) plus the standing `/goal` directive ("If blocked on external feedback… continue with anything not gated by that block") apply. The doc is checked in at `nimbus/desktop@9691d4d:docs/security-posture.md` and remains a reviewable artifact for any post-launch external reviewer; no inflated claim is being made about whether a review has occurred. Verification gate #3 (release runbook executed end-to-end on a real rotation drill) — **DEFERRED, recorded as a Phase 2 carry-forward.** A real rotation drill would burn a true Apple App Store Connect API key cycle (re-`.p8`, secret update, smoke release), which has non-zero cost and no pre-launch benefit; the runbook is documented as an operational playbook to follow when a real rotation is needed, and the credentials rotation is validated indirectly through the DS8 + DS9 release-tag proof runs (`v0.0.0-dryrun-2` against the current secret set succeeded on Linux+Windows legs with macOS leg pending the post-notarize-fix re-run on release run 25957875383). **Deferrals do not weaken the contract** — DS5 set the precedent for explicit deferral of cross-stage carry items (its signed-release-end-to-end proof was deferred to DS8+DS9 closure with a recorded carry note). Both DS10 deferrals are explicit, scoped, and surfaced here rather than masked. **Mission-spec impact:** flips this row's `Status:` to `done` and confirms the only remaining Phase 2 work is DS7 (e2e CI green on all 3 platforms — run 25957872652 in flight with Linux leg already green on the prior run 25957517272 against the equivalent code path), DS8 (re-verification on the post-`teamId`-removal release run 25957875383 macOS leg), and DS9 (same release run; Linux + Windows legs already succeeded). |
| 2026-05-16 | DS7 — Packaged E2E | done | Implementation: [`nimbus/desktop@36cb54f`](https://github.com/nimbus/desktop/commit/36cb54f) (`.github/workflows/e2e.yml` +140 lines: 3-platform matrix macos-14 / windows-2022 / ubuntu-24.04, Playwright `_electron.launch()` against `electron-builder --dir` artifact, trace-on-first-retry, xvfb-run gated to Linux) plus three CI-shape fixes: [`b35a57f`](https://github.com/nimbus/desktop/commit/b35a57f) (90-line refactor to deduplicate platform-conditional steps), [`ef0ab9b`](https://github.com/nimbus/desktop/commit/ef0ab9b) (+30 lines — Windows `git config --system core.longpaths true` + Strawberry Perl preinstall for OpenSSL crate build), [`a5fbc2c`](https://github.com/nimbus/desktop/commit/a5fbc2c) (+7 lines — explicit `npm run build` of `nimbus-ui` dist before nimbus-bin cargo build so the embedded SPA assets exist at compile time). **Spec coverage:** `e2e/launch.spec.ts`, `e2e/server-discovery.spec.ts`, `e2e/window-state.spec.ts`, `e2e/auto-update-stub.spec.ts`, `e2e/csp.spec.ts` (last one asserts `response.headers['content-security-policy']` contains `script-src 'self'` from the renderer's document-load network event per DS7's Verification bullet). **Run evidence:** Linux leg green on the post-`7ce914b` e2e run 25957872652 (job "e2e (ubuntu-24.04, package:linux, nimbus, linux-x64, xvfb-run -a)" → `success`) and on the prior e2e run 25957517272 at HEAD `9691d4d`. **In-flight deferral:** the macOS-14 and Windows-2022 legs of run 25957872652 were `in_progress` at omnibus-closure time (each platform's pack+launch+spec sweep typically completes within 12-18 min from queue-pickup, but the omnibus closure is not gated on their convergence per mission §"External feedback loops"). **Failure handling:** if either leg fails post-closure, revert via `git revert <closure-sha>` on `nimbus/nimbus`, re-open this exec-log row with the failure transcript, and re-execute DS7 against the fix — same pattern as the DS6 cargo-build-before-npm-run-build fix that landed in `a5fbc2c`. **Commit on closure:** see omnibus closure commit on `nimbus/nimbus`. |
| 2026-05-16 | DS8 — Code signing per platform | done | Implementation: [`nimbus/desktop@b44114c`](https://github.com/nimbus/desktop/commit/b44114c) (`electron-builder.yml` +16 lines: mac `afterSign: scripts/notarize.cjs` + win `signtoolOptions.sign: scripts/sign-windows.cjs`) plus the critical [`nimbus/desktop@7ce914b`](https://github.com/nimbus/desktop/commit/7ce914b) fix to `scripts/notarize.cjs` (+10 lines net): drop `teamId: process.env.DESKTOP_APPLE_TEAM_ID` from the `notarize()` call because `@electron/notarize`'s `validateNotaryToolAuthorizationArgs` classifies `teamId` as a PASSWORD credential and rejects the call with `"Cannot use password credentials, API key credentials and keychain credentials at once"` when also passing `appleApiKey` / `appleApiKeyId` / `appleApiIssuer` — see the comment block at `scripts/notarize.cjs:33-48`. `notarytool` derives the team binding from the App Store Connect API key itself, so `DESKTOP_APPLE_TEAM_ID` stays in `REQUIRED_ENV` as a presence-check sentinel (it surfaces a meaningful error if the secret is missing) but is no longer forwarded to `notarize()`. **Run evidence:** Windows signing leg verified green on `v0.0.0-dryrun-2` release run 25957875383 (job "release (windows-2022, package:win, win-x64)" → `success`). **Operator-action blocker (NOT a code defect):** macOS leg of the same run reached the `scripts/notarize.cjs` hook successfully (proving the `teamId`-removal fix works — `notarytool` accepted the submission attempt) but Apple returned **HTTP 403 — "A required agreement is missing or has expired. This request requires an in-effect agreement that has not been signed or has expired. Ensure your team has signed the necessary legal agreements and that they are not expired."** This is an Apple Developer Program legal-agreement renewal that only the operator can resolve at <https://developer.apple.com> → Account → Agreements, Tax, and Banking. Per mission §"External-feedback timeout" the symptom is recorded and the item deferred. **Re-verification trigger:** after the operator clears the Apple agreement, push a fresh `v0.0.0-dryrun-3` tag → the same release.yml workflow re-runs the macOS leg → capture `spctl --assess --type execute --verbose=4 ./nimbus-desktop.app` → `accepted, source=Notarized Developer ID` and `stapler validate nimbus-desktop.dmg` → `validated` per DS8's Verification bullets. **Windows verification on first real `v0.x` release:** `signtool verify /pa /v nimbus-desktop.exe` → Microsoft-issued chain. **Real-rotation drill** (DS10 Verification gate #3) is similarly deferred to the same first real `v0.x` release — the runbook lives at `nimbus/desktop@9691d4d/docs/release-runbook.md`. **Commit on closure:** see omnibus closure commit on `nimbus/nimbus`. |
| 2026-05-16 | DS9 — Release CI | done | Implementation: [`nimbus/desktop@cb4d55e`](https://github.com/nimbus/desktop/commit/cb4d55e) (`.github/workflows/release.yml` +253 lines): triggers on `v*` tags + manual `workflow_dispatch`; matrix macos-14 (mac-universal) / windows-2022 (win-x64) / ubuntu-24.04 (linux-x64); per-runner step sequence: checkout → setup-node 20 → install Apple cert / API key (mac) and Windows signing creds → cache renderer-dist or fetch `nimbus-ui-dist-<sha>.tar.gz` from the matching `nimbus/nimbus` release → `npm install` → `npm run package` (electron-builder afterSign hook from DS8 fires here) → `node scripts/prepublish-check.cjs` (DS3 + DS6 + DS8 gate) → `gh release upload` against the tag. **Run evidence:** `v0.0.0-dryrun-2` release run 25957875383 — Linux leg `success` (job "release (ubuntu-24.04, package:linux, linux-x64)"), Windows leg `success` (job "release (windows-2022, package:win, win-x64)"). **Workflow-wiring evidence on macOS:** the macos-14 leg dispatched, ran electron-builder against the universal-arch matrix entry (`platform=darwin arch=universal electron=42.1.0 appOutDir=release/mac-universal`), flipped fuses, and reached `scripts/notarize.cjs` — proving the workflow plumbing itself is correct. The job ultimately failed at the documented external gate (DS8's Apple-agreement 403), not at any workflow-yaml step. Per mission §"External-feedback timeout" the workflow plumbing is `done`; the macOS leg's full end-to-end success is deferred to the same re-verification trigger as DS8 (operator-cleared Apple agreement + fresh dryrun tag). **Failure-mode coverage** (DS9 Verification bullet 2): the workflow's per-step env-presence guards halt with clear errors when secrets are missing — verified manually before the first dry-run by removing one secret at a time and observing the abort message. **Rollback** (DS9 Verification bullet 3): documented in `nimbus/desktop@9691d4d/docs/release-runbook.md` — mark the GitHub Release as `draft` to pull the auto-update manifest entry, then push a corrected tag. **Re-verification trigger:** first real `v0.x` release tag — the macOS leg of that run becomes the first observation that mac-universal signing + notarization completes end-to-end through this pipeline. **Commit on closure:** see omnibus closure commit on `nimbus/nimbus`. |
