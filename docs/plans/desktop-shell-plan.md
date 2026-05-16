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
  [`002-windows-code-signing.md`](https://github.com/nimbus/desktop/blob/main/docs/decisions/002-windows-code-signing.md)).
  DS1–DS10 pending. Driven autonomously per
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

**Status:** `pending`

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

**Status:** `pending`

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

**Status:** `pending`

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

**Status:** `pending`

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

**Status:** `pending`

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

**Status:** `pending`

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

**Status:** `pending`

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

**Status:** `pending`

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

**Status:** `pending`

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

**Status:** `pending`

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
