# Update lifecycle plan

- **Status:** proposed
- **Owners:** `nimbus/nimbus` (UL1, UL2, UL4) · `nimbus/desktop` (UL3)
- **Anchors:**
  - Decision: [`docs/decisions/001-update-staleness-detection.md`](../decisions/001-update-staleness-detection.md)
  - Desktop auto-update (out of scope for this plan):
    [`nimbus/desktop/docs/decisions/003-auto-update-channel.md`](https://github.com/nimbus/desktop/blob/main/docs/decisions/003-auto-update-channel.md)
- **Cross-cutting context:** Pre-launch — breaking changes preferred, no
  migration shims. The /ui/* SPA ships via `rust_embed` inside the
  `nimbus` binary; browser and desktop render the same bundle.

---

## Why this plan exists

Two operator-facing surfaces today — a browser pointed at
`http://<host>:<port>/ui/` and the `nimbus-desktop` Electron shell — both
render the SPA served by a running `nimbus` instance. Neither surface has
any way to tell the operator that the binary running underneath them is
out of date, that a newer release is on GitHub, or what command would
install it, *and* neither surface offers a one-click path to actually
run that command. Three adjacent problems compound this:

1. There is no staleness signal, full stop — the operator has no way
   to know there's a newer nimbus.
2. Even if a banner showed the upgrade command, the operator still has
   to context-switch to a terminal and paste it. Every analog of our
   shape (Podman Desktop is the canonical one) has solved this by
   offering an in-app **Update** CTA that orchestrates the package
   manager.
3. The desktop shell, on first launch with no `nimbus` on PATH, throws
   `NimbusBinaryNotFoundError` rather than guiding the user to install
   it.
4. There is no canonical place to land operator-facing docs that
   explain *how* nimbus updates — across brew, apt, dnf, install
   script, and build-from-source — relative to how the desktop shell
   self-updates.

This plan owns landing the staleness signal (UL1), the SPA banner +
upgrade modal (UL2), the desktop shell's IPC bridge for terminal-
launch + setup card for the missing-CLI case (UL3), and the
operator-facing documentation that ties them together (UL4).

It does **not** own `nimbus self-upgrade` (binary rewrites itself) —
that's explicitly rejected in the decision doc → (γ). It *does* own
the Podman Desktop pattern: server-side detection + UI-launched
package manager command. The distinction is in decision 001's
"Real-world analogs" section.

---

## Architecture summary (per the decision)

```
                ┌─────────────────────────┐
                │   api.github.com        │
                │   /repos/nimbus/nimbus  │
                │   /releases/latest      │
                └────────────┬────────────┘
                             │ HTTPS GET (≤1/day per instance)
                             │ If-Modified-Since
                ┌────────────▼────────────┐
                │   nimbus binary          │
                │   ├─ background refresh  │  ← UL1
                │   ├─ on-disk cache       │
                │   ├─ install-method      │
                │   │  detection           │
                │   └─ /api/system/        │
                │      version-info        │
                │      (with `upgrade` obj)│
                └────────────┬────────────┘
                             │
                ┌────────────▼────────────┐                     ┌──────────────────────┐
                │   /ui/* SPA              │  ← UL2              │ nimbus-desktop main  │  ← UL3
                │   StalenessBanner +      │   click [Update]    │ IPC: nimbus.openUp-  │
                │   UpgradeModal           │ ──────────────────▶ │ gradeTerminal(tag)   │
                │   (same in browser +     │                     │ method tag →         │
                │   desktop; desktop adds  │                     │ whitelisted command  │
                │   "Open in Terminal")    │                     │ → osascript spawn    │
                └─────────────────────────┘                     └──────────┬───────────┘
                                                                           │
                                                                ┌──────────▼───────────┐
                                                                │ user's Terminal.app  │
                                                                │ command pre-typed —  │
                                                                │ user reviews,        │
                                                                │ presses Return,      │
                                                                │ brew/apt/dnf runs    │
                                                                └──────────────────────┘

  Orthogonal: nimbus-desktop shell uses electron-updater for itself
  (already wired). Never polls api.github.com for the nimbus binary.
  Browser-only operators see the same banner + modal but the modal's
  primary action is "Copy command" instead of "Open in Terminal".
```

Three update axes, three independent owners:

| Axis | Detected by | Surfaced as | Applied by |
|---|---|---|---|
| **nimbus binary** stale | UL1 background task in nimbus | UL2 SPA banner + upgrade modal (browser + desktop) | operator's package manager (`brew/apt/dnf upgrade …`), launched in a terminal by UL3's IPC bridge on desktop or copy-paste in browser |
| **/ui/* SPA** stale | n/a (ships in lockstep with the binary) | n/a | upgrades when the binary upgrades |
| **desktop shell** stale | `electron-updater` polling release manifest | shell's own non-modal toast | `electron-updater` swaps `.app` on next quit |

---

## Milestones

### UL1 — Server-side `/api/system/version-info` with stale-while-revalidate

**Goal:** the running nimbus binary can answer "am I behind?" without
contacting GitHub on the hot path, without telemetry, and without
blocking startup.

**Deliverables:**

1. New module `crates/nimbus-server/src/system/version_check.rs` owning:
   - `pub struct VersionCheck` with `current`, `cached_latest`,
     `last_checked_at`, `check_status`.
   - A `tokio::spawn`'d async refresh task gated on the lazy-trigger
     contract below.
   - On-disk persistence under `<config_dir>/update-check.json`
     (XDG-respecting via existing `crates/nimbus-bin/src/dirs.rs::
     global_config_dir`).
2. New endpoint `GET /api/system/version-info` registered in the
   existing server route table. JSON shape locked to the decision doc's
   sketch:
   ```json
   {
     "current": "0.1.31",
     "latest": "0.1.41",
     "available": true,
     "url": "https://github.com/nimbus/nimbus/releases/tag/v0.1.41",
     "publishedAt": "2026-05-14T18:22:00Z",
     "host": "host.example.com",
     "checkStatus": "fresh",
     "upgrade": {
       "method": "brew",
       "command": "brew upgrade --cask nimbus/tap/nimbus",
       "needsSudo": false,
       "interactive": true,
       "fallbackUrl": "https://github.com/nimbus/nimbus#install"
     }
   }
   ```
3. **Install-method detection** (replaces the old flat `upgradeHint`).
   The server resolves `std::env::current_exe()` and matches its
   canonicalized path against a hardcoded set of markers:
   - Homebrew prefix (`/opt/homebrew/`, `/usr/local/Homebrew/`,
     `/home/linuxbrew/`) → `method: "brew"`,
     `command: "brew upgrade --cask nimbus/tap/nimbus"`,
     `needsSudo: false`.
   - `/usr/bin/` or `/usr/local/bin/` plus a `dpkg`-managed marker
     (`dpkg -S` returns the path) → `method: "apt"`,
     `command: "sudo apt update && sudo apt upgrade nimbus"`,
     `needsSudo: true`.
   - `/usr/bin/` plus an `rpm`-managed marker → `method: "dnf"`,
     `command: "sudo dnf upgrade nimbus"`, `needsSudo: true`.
   - `~/.local/bin/nimbus` or `~/.nimbus/bin/nimbus` (the install
     script's default targets) → `method: "install-script"`,
     `command: "curl -fsSL https://nimbus.dev/install.sh | sh"`,
     `needsSudo: false`.
   - `target/{debug,release}/nimbus` or a `cargo install` path
     under `~/.cargo/bin/` → `method: "source"`, `command: null`,
     `fallbackUrl: "https://github.com/nimbus/nimbus#build-from-source"`.
   - Anything else → `method: "unknown"`, `command: null`,
     `fallbackUrl: "https://github.com/nimbus/nimbus#install"`.

   The command strings are constructed locally from these
   hardcoded templates — never echoed from GitHub's response — so a
   poisoned upstream cannot inject a malicious upgrade command.
4. Opt-out: respect `NIMBUS_DISABLE_UPDATE_CHECK=1` — the background
   task is never spawned, on-disk cache is never read or written, and
   the endpoint returns `checkStatus: "disabled"` with `latest: null`.

**Contract bullets:**

- **No work on boot.** `nimbus start` never contacts GitHub directly.
  The refresh task only spawns in response to the first endpoint hit
  after startup if the cache is missing or ≥24h old.
- **Stale-while-revalidate.** The endpoint always returns a cached
  value (or `checkStatus: "never"`) within p99 < 5ms. A stale read
  triggers a fire-and-forget refresh; the next request sees the new
  value.
- **24h TTL.** A successful fetch marks the cache fresh for 24h.
  After 24h, the next endpoint hit triggers an async refresh and
  returns the still-cached stale value with `checkStatus: "stale"`.
- **Graceful failure.** Network failures, 5xx from GitHub, non-200
  responses, or parse errors log at INFO and set `checkStatus:
  "error"`; the last good cached value is preserved. Retry on the next
  endpoint hit after the TTL has elapsed.
- **Single in-flight refresh.** Concurrent requests must not spawn
  more than one in-flight refresh task. Use an `Arc<Mutex<...>>` or
  `tokio::sync::OnceCell` guard.
- **Semver comparison.** Use the existing `semver` crate (already in
  the workspace) for `current < latest` rather than string compare.
  Tags from GitHub strip the leading `v`.
- **Reqwest user-agent.** `User-Agent: nimbus/<version>` per GitHub's
  unauthenticated API requirements.

**Files touched:**

- `crates/nimbus-server/src/system/version_check.rs` (new)
- `crates/nimbus-server/src/system/mod.rs` (re-export)
- `crates/nimbus-server/src/router.rs` (route wiring)
- `crates/nimbus-server/Cargo.toml` (semver dep if not already present)
- `docs/operating/cli.md` (env var documentation)

**Completion gate:**

- `cargo test -p nimbus-server version_check::` covers: fresh hit,
  stale-while-revalidate refresh, opt-out path, error path with
  cached-value preservation, semver comparison, no-cache-on-disk first
  run.
- `cargo test -p nimbus-server install_method::` covers each of the
  six method-detection branches (brew, apt, dnf, install-script,
  source, unknown) by feeding synthetic `current_exe` paths, and
  asserts the `command` template comes from the hardcoded set rather
  than any input fixture.
- Integration test using `wiremock` (already a workspace dep): boot
  a `nimbus-server` against a wiremock GitHub Releases mock, hit
  `/api/system/version-info`, assert each `checkStatus` branch and
  the structured `upgrade` object is well-formed for the detected
  install method.
- `make ci` clean.
- Manual smoke against the live network: `nimbus start` → first GET to
  `/api/system/version-info` returns `checkStatus: "never"`,
  `latest: null`; second GET (after ~1s) returns the fresh value.
- `NIMBUS_DISABLE_UPDATE_CHECK=1 nimbus start` → endpoint returns
  `checkStatus: "disabled"` immediately and `~/.config/nimbus/
  update-check.json` is never created.

### UL2 — SPA staleness banner in `packages/nimbus-ui/`

**Goal:** the operator sees one consistent staleness signal, with the
same copy and the same dismissal behavior, in browser and desktop.

**Deliverables:**

1. New component `packages/nimbus-ui/src/components/staleness-banner.tsx`:
   - Reads `/api/system/version-info` via the existing convex/nimbus
     fetch client (one-shot, not subscription — the endpoint is
     intentionally simple HTTP, not WS).
   - Polls every 5 minutes while open (a stale-while-revalidate read
     on the server side is cheap).
   - Renders nothing for `checkStatus: "disabled"`, `"never"`, or
     `available: false`.
   - Renders a top banner for `available: true` with copy "Update
     Nimbus on `<host>` from `<current>` to `<latest>`" (host comes
     from the response so remote-nimbus topologies read correctly),
     a link to the release URL, and a primary **Update** CTA that
     opens the upgrade modal.
   - Dismissible per-session via `localStorage["nimbus-ui:staleness-
     dismissed-version"]` keyed to the *latest* version — dismissing
     0.1.41 still surfaces 0.1.42 when it lands.
   - Re-renders without a full page reload when the underlying value
     changes (poll completes, version flips).
2. New component `packages/nimbus-ui/src/components/upgrade-modal.tsx`
   (the Podman Desktop pattern, adapted):
   - Triggered by the banner's Update CTA.
   - Title: "Update Nimbus to v`<latest>`".
   - Body: a one-paragraph explanation, the detected install method
     (e.g., "We detected Homebrew."), and `upgrade.command` rendered
     in a `<code>` block.
   - Detects desktop context via the preload-injected
     `window.nimbus?.openUpgradeTerminal` capability (UL3):
     - **Desktop**: primary action "Open in Terminal" → calls
       `window.nimbus.openUpgradeTerminal(upgrade.method)`. The
       renderer only passes the method tag; never the command string.
     - **Browser (or desktop without terminal support, e.g., older
       Linux)**: primary action "Copy command" → writes
       `upgrade.command` to the clipboard and surfaces a transient
       toast "Copied — paste into your terminal".
   - Secondary action "Cancel" closes the modal without dismissing
     the banner.
   - Disclosure "Why doesn't Nimbus just update itself?" expands a
     short explanation citing decision 001 → (γ) — the package
     manager stays the source of truth, the user sees the command
     before it runs.
   - Renders the `upgrade.fallbackUrl` link as the primary action
     when `method` is `"source"` or `"unknown"` (no clean command).
3. Banner + modal mounted in the global shell (`packages/nimbus-ui/
   src/routes/__root.tsx` or equivalent — confirm name during
   implementation since DU3 may have renamed).
4. Storybook stories (`staleness-banner.stories.tsx`,
   `upgrade-modal.stories.tsx`) covering: five `checkStatus` branches
   × two themes for the banner (ten states); six `upgrade.method`
   branches × two themes × two contexts (desktop/browser) for the
   modal (twenty-four states) on the curated Chromatic matrix —
   trim ruthlessly if review fatigue sets in.

**Contract bullets:**

- **Single banner.** No version-specific dismissal layering — one
  active banner at a time, dismissed per-latest-version.
- **A11y:** banner is `role="status"` with `aria-live="polite"` so
  screen readers announce it on first render but not on poll refreshes
  that don't change the visible content. The modal is `role="dialog"`
  with `aria-modal="true"`, focus-trapped, Esc-to-close.
- **Theme-aware:** uses semantic state tokens from DU5's OKLCH
  palette (`--color-info` for the available-update banner). Passes
  axe-core AA in both themes per the DU5 verification standard.
- **No exit telemetry.** Dismissing the banner or clicking the
  Update CTA only writes localStorage and (for desktop) sends an IPC
  message to the local desktop main process. Nothing flows to the
  server.
- **Method tag, not command string.** The renderer's only outbound
  command surface is `window.nimbus.openUpgradeTerminal(method)` with
  `method` being one of six known strings. The renderer never
  constructs or forwards shell input. This mirrors the security
  model in `extensions/podman/packages/extension/src/extension.ts:1014`
  (provider.registerUpdate({ update: () => … })) — the renderer
  invokes a typed callback, not an arbitrary shell command.

**Files touched:**

- `packages/nimbus-ui/src/components/staleness-banner.tsx` (new)
- `packages/nimbus-ui/src/components/staleness-banner.stories.tsx`
  (new)
- `packages/nimbus-ui/src/components/upgrade-modal.tsx` (new)
- `packages/nimbus-ui/src/components/upgrade-modal.stories.tsx` (new)
- `packages/nimbus-ui/src/lib/desktop-bridge.ts` (new — typed
  wrapper over `window.nimbus` that returns `null` when the
  preload bridge isn't present, so all callsites are typed against
  the same surface)
- `packages/nimbus-ui/src/routes/__root.tsx` (mount point)
- `packages/nimbus-ui/src/api/system.ts` (typed fetch wrapper for
  `/api/system/version-info`, new)

**Completion gate:**

- Vitest unit tests cover the dismissal-keying logic, the
  five-branch banner render matrix, the six-branch
  `upgrade.method` modal matrix, and the `latest`-version-flips-
  re-shows case.
- Vitest unit test confirms that with a mocked
  `window.nimbus.openUpgradeTerminal`, clicking "Open in Terminal"
  invokes it with exactly the method tag from the response (and no
  other arguments). With the bridge absent, the same click instead
  writes `upgrade.command` to the clipboard via `navigator.clipboard`.
- Storybook + Chromatic: banner + modal stories pass visual
  regression.
- axe-core run against the embedded build: 0 violations in dark and
  light themes with the banner visible AND with the modal open
  (matches the DU5/DU6/DU6.5/DU7 bar).
- Manual end-to-end against a freshly-cut nimbus that exposes UL1: open
  `/ui/` in Chromium → banner appears; click Update → modal opens;
  click "Copy command" → clipboard contains the brew upgrade command;
  dismiss banner → reload → banner stays dismissed; hand-edit
  `update-check.json` to bump `latest` → banner re-appears.

### UL3 — Desktop shell: setup card + upgrade-terminal bridge

**Goal:** the desktop shell adds two install-method-aware surfaces:

1. A first-run setup card when no `nimbus` CLI is on PATH (replaces
   the raw `NimbusBinaryNotFoundError` death-screen).
2. An IPC bridge `nimbus.openUpgradeTerminal(method)` that lets the
   SPA's UL2 Update CTA launch the user's terminal with the
   install-method-specific command pre-typed.

Both are forms of the same pattern: the desktop renders a button,
the renderer passes a method tag, the main process maps that tag
to a whitelisted command and shells out via `osascript` (macOS) or
the platform-appropriate equivalent. This mirrors the architecture
in `extensions/podman/packages/extension/src/installer/mac-os-
installer.ts` where the `install()` / `update()` methods construct
their own paths from local state and call `processAPI.exec('open',
[pkgToInstall, '-W'])` — the renderer never passes a shell string.

**Owner:** `nimbus/desktop` (separate repo, separate release cadence).

**Deliverables:**

1. **Preload bridge** `src/preload/index.ts` exposes a typed
   `window.nimbus`:
   ```ts
   window.nimbus = {
     openUpgradeTerminal(method: UpgradeMethod): Promise<{ launched: boolean; fallback?: 'copy' }>;
     openInstallTerminal(method: InstallMethod): Promise<{ launched: boolean; fallback?: 'copy' }>;
     retryResolveCli(): Promise<{ ok: boolean }>;
   };
   ```
   `UpgradeMethod` and `InstallMethod` are closed unions matching
   the server's `upgrade.method` set. An unknown tag is rejected at
   the IPC boundary; the renderer cannot smuggle commands through.
2. **Main process handler** `src/main/ipc/upgrade.ts` (new) maps
   method tags to a hardcoded command table and a launcher:
   ```ts
   const UPGRADE_COMMANDS: Record<UpgradeMethod, string | null> = {
     brew: 'brew upgrade --cask nimbus/tap/nimbus',
     apt: 'sudo apt update && sudo apt upgrade nimbus',
     dnf: 'sudo dnf upgrade nimbus',
     'install-script': 'curl -fsSL https://nimbus.dev/install.sh | sh',
     source: null,
     unknown: null,
   };
   ```
   Launcher behavior:
   - **macOS**: `osascript -e 'tell app "Terminal" to do script "<cmd>"'`
     opens Terminal.app with the command pre-typed (not auto-
     executed; the user presses Return). Returns
     `{ launched: true }`.
   - **Linux**: try Windows Terminal-equivalent detection in order
     (`gnome-terminal`, `konsole`, `xterm`, env `TERMINAL`). If
     found, spawn with the command. If not, return
     `{ launched: false, fallback: 'copy' }` so the SPA falls back
     to the clipboard path. v1 acceptable to ship copy-only on
     Linux.
   - **Windows**: spawn `wt.exe` (Windows Terminal) with the
     command pre-typed if present, else `{ launched: false,
     fallback: 'copy' }`. v1 acceptable to ship copy-only on
     Windows.
   - For `method: "source"` or `"unknown"` where the command is
     `null`, return `{ launched: false, fallback: 'copy' }`
     immediately — the SPA shows the `fallbackUrl` link instead.
3. **Setup card** `src/renderer/setup/CliNotFoundCard.tsx`
   (or equivalent — verify the actual shell layout during
   implementation; DS3 may have shipped a different renderer
   structure):
   - Triggered when `resolveNimbusExecutable` at `src/main/server.ts:
     200` throws `NimbusBinaryNotFoundError`. Instead of bubbling
     the error to a death-screen, the main process posts
     `cli-not-found` to the renderer, which swaps the window
     contents to the setup card.
   - The card surfaces:
     - **macOS**: button "Install with Homebrew" →
       `window.nimbus.openInstallTerminal('brew')` (uses the same
       IPC bridge as the upgrade flow, distinct method).
     - **Linux**: link to `https://github.com/nimbus/nimbus#install`.
     - **Windows**: link to the direct-download .zip and install docs.
     - Common: a "Retry" button calling
       `window.nimbus.retryResolveCli()`.
4. After the user installs, the Retry path picks up the new binary
   on PATH without requiring a full app restart.

**Contract bullets:**

- **No bundled installer.** The shell never downloads or executes
  the nimbus binary itself. It hands off to a package manager (where
  one exists) or the install docs. Preserves the decision in the
  desktop README — "shell does not bundle nimbus."
- **Method tag whitelist.** The IPC boundary accepts only known
  method tags. The main process constructs the command string from
  a local hardcoded table; the renderer never sees or forwards a
  shell string.
- **No automatic install.** The terminal is *launched* with the
  command pre-typed, not executed silently. The user reviews and
  presses Return.
- **No sudo escalation in the desktop process.** Commands that need
  root (`apt`, `dnf`) run inside the user's terminal where the
  standard sudo prompt handles auth. The desktop process never has
  elevated privileges.
- **Retry, don't restart.** The shell remains usable across the
  install/upgrade — a successful retry must not require quitting
  and relaunching.

**Files touched (in nimbus/desktop):**

- `src/main/server.ts` (signal cli-not-found instead of throwing
  into the void; trip the `retryResolveCli` re-run path)
- `src/main/ipc/upgrade.ts` (new — IPC handler + platform
  terminal launcher)
- `src/main/ipc/upgrade.spec.ts` (new, vitest — tag whitelist,
  per-platform launcher behavior)
- `src/preload/index.ts` (extend with typed `window.nimbus`
  surface — exposedInMainWorld declarations follow the
  podman-desktop preload pattern)
- `src/renderer/setup/CliNotFoundCard.tsx` (new)
- `src/renderer/setup/CliNotFoundCard.spec.ts` (new, vitest)
- `tests/e2e/cli-not-found.spec.ts` (new, packaged-shell
  Playwright)
- `tests/e2e/upgrade-terminal.spec.ts` (new — launch a fake
  Terminal binary on PATH via `TERMINAL=…` env override, assert
  the bridge spawns it with the expected command)

**Completion gate:**

- Vitest unit tests cover the IPC tag whitelist (six known
  methods + reject path for unknown), the platform launcher
  matrix, retry semantics, and the install-vs-upgrade method
  separation.
- Packaged-shell Playwright E2E:
  - `cli-not-found.spec.ts`: launch with `PATH=/empty`, assert
    setup card renders; add a fake nimbus to PATH; click Retry;
    assert the card disappears and the normal `/ui/` window opens.
  - `upgrade-terminal.spec.ts`: launch with a stub terminal on
    PATH, render a banner with `method: "brew"`, click Update →
    Open in Terminal, assert the stub terminal received exactly
    `brew upgrade --cask nimbus/tap/nimbus`.
- Manual on macOS: install desktop cask only, launch, observe
  setup card; install CLI via Homebrew button (which opens Terminal,
  Return, brew runs), click Retry, observe normal `/ui/`; then
  artificially mark binary as stale (hand-edit `update-check.json`),
  reload `/ui/`, observe banner; click Update → Open in Terminal,
  observe Terminal opens with brew upgrade command pre-typed.

### UL4 — Operator-facing docs

**Goal:** a new operator can read one doc that explains the entire
update story — how the binary updates, how the shell updates, what the
banner means, what to do when offline.

**Deliverables:**

1. New `docs/operating/updates.md`:
   - "How nimbus updates" section covering brew, apt, dnf, install
     script, build-from-source — each with the recommended upgrade
     command.
   - "How the desktop shell updates" section pointing at
     `electron-updater` and the desktop release runbook.
   - "What the staleness banner means" section explaining the four
     visible states (fresh / stale-but-cached / first-load-empty /
     check-failed) and the `NIMBUS_DISABLE_UPDATE_CHECK=1` opt-out.
   - "What the Update button does" section explaining the (β+)
     pattern: in the desktop, the click opens a terminal with the
     command pre-typed; in the browser, the click copies the
     command to the clipboard. The user always confirms before the
     command runs. Cite decision 001's "Real-world analogs" for the
     why.
   - "Air-gapped operation" subsection making the off-switch explicit.
2. README cross-link: add a one-line pointer to `docs/operating/
   updates.md` in the Install section of `README.md` (right under the
   Desktop console subsection landed in commit `31c11311`).
3. Desktop repo cross-link: update `nimbus/desktop/README.md` to
   point at `docs/operating/updates.md` from its Update section.

**Files touched:**

- `docs/operating/updates.md` (new — `nimbus/nimbus`)
- `README.md` (cross-link — `nimbus/nimbus`)
- `README.md` (cross-link — `nimbus/desktop`)

**Completion gate:**

- A reviewer who has never seen this plan can read `updates.md` and
  answer: "How do I disable the update check?", "Why does the banner
  show a brew command on my apt machine?" (it doesn't — explain the
  detection heuristic), "What if I'm offline?".

---

## Sequencing

```
UL1 ─┬─► UL2 ─┐
     │         │
     └───────► UL4
               │
UL3 ───────────┘
```

- **UL1 lands first.** It's the load-bearing change; nothing else can
  ship without an endpoint to consume.
- **UL2 and UL3 can land in parallel** once UL1 is on a tagged nimbus
  release that the desktop shell can rely on (or behind a feature flag
  in the SPA that no-ops on a 404).
- **UL4 lands after UL1+UL2** so the docs reflect actually-shipped
  behavior, not aspirational behavior.

---

## Verification across milestones

Per [`CLAUDE.md`](../../CLAUDE.md) → "Execution Quality": every
milestone names its tests and asserts specific outcomes, not "it
didn't panic." The completion gates above enumerate per-milestone
tests; the cross-cutting gates are:

- `make ci` clean at each milestone's close.
- `npm run typecheck && npm run test && npm run build` clean for any
  milestone touching `packages/nimbus-ui/`.
- Live end-to-end proof recorded in the Execution Log table for each
  milestone, with screenshots stored under `.playwright-cli/` and
  referenced in the row.

---

## Out of scope

- **`nimbus self-upgrade` (server rewrites its own binary).**
  Rejected in [decision 001](../decisions/001-update-staleness-
  detection.md) → rejected alternative (γ). Self-rewriting binaries
  fight the package manager and add security surface for marginal
  UX gain. **Note the distinction from in-scope (β+)**: (β+) lets
  the *UI* launch the operator's *package manager*; (γ) would have
  the *server* *be* the package manager. We adopt the first, reject
  the second.
- **Silent `process.exec` of brew/apt/dnf from the desktop main
  process.** Podman Desktop's macOS installer runs the bundled .pkg
  silently via `open <pkg> -W` because the .pkg has its own GUI with
  admin prompts. brew/apt/dnf are CLI tools with no GUI — silent
  exec from the main process would hide stdout/stderr and confuse
  failures. We launch a terminal so the user sees what the package
  manager is doing.
- **Bundling installer artifacts inside `nimbus-desktop`.** Podman
  Desktop bundles `podman-installer-macos-*.pkg` (one of the
  pkg-arch-version assets) inside its .app. We don't currently
  produce nimbus `.pkg`/`.msi`/`.deb`/`.rpm` installer artifacts;
  we hand off to brew/apt/dnf. Once distribution lands its own pkg
  installer track (see `docs/plans/distribution-plan.md`), bundling
  is the obvious next step — until then, terminal-launch is the
  pragmatic v1.
- **Auto-execute (no terminal) for brew on macOS.** A future
  refinement could run `brew upgrade` headlessly from the main
  process and stream stdout into an in-app progress modal (matching
  Podman Desktop's `ProgressLocation.TASK_WIDGET` UX). Deferred —
  terminal-launch lets us ship v1 without owning a streaming-shell
  surface.
- **Desktop polling GitHub directly.** Rejected in
  [decision 001](../decisions/001-update-staleness-detection.md) →
  rejected alternative (α). The desktop's update signal arrives via
  `/api/system/version-info`, same as the browser's.
- **Pre-release / RC handling.** Pinning to `releases/latest`
  filters GitHub's own pre-release tags by default. When we cut a real
  RC, revisit whether to expose a `prerelease: true` field in the
  endpoint response.
- **Patch / minor / major banner variants.** The decision doc flags
  this as an open question. The initial UL2 banner is one shape; if
  we later want a less-intrusive "patch available" variant, that's a
  follow-on, not a UL milestone.

---

## Open questions

Carried forward from the decision doc, plus a few that emerged while
writing the plan:

- **Banner severity tiers** (patch vs. minor vs. major). Deferred —
  ship one banner first, learn from operator feedback.
- **Downgrade fallback.** What if `current` > `latest`? Treat as
  `available: false` and `checkStatus: "fresh"`. Operator running a
  newer build than what's on Releases is either testing locally or on
  a fork; no banner needed.
- **Multiple nimbus instances on one host.** The on-disk cache lives
  under the XDG config dir, shared across instances of the same user.
  Two instances will fight over the file. Acceptable — last writer
  wins, and they're both pulling the same upstream data. Document in
  UL4.
- **What does the banner say when the embedded SPA is *itself* the
  thing that's behind?** The SPA ships with the binary, so a stale
  binary means a stale SPA. The banner copy should reflect that
  upgrading the binary upgrades the UI too. Refine wording during
  UL2.

---

## Execution log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-05-16 | Plan authored | — | Decision doc 001 already landed; this plan is its parent execution sequencing. UL1/UL2/UL4 owned by `nimbus/nimbus`, UL3 owned by `nimbus/desktop`. |
| 2026-05-16 | Revised with (β+) UI-launched upgrade pattern | — | Surveyed Podman Desktop locally (`~/src/github.com/podman-desktop/podman-desktop/extensions/podman/packages/extension/src/installer/{podman-install.ts,mac-os-installer.ts}` + `extension.ts:1014` `registerUpdatesIfAny`). Adopted the renderer-passes-method-tag / main-process-maps-to-whitelisted-command security model. UL1 endpoint shape grew a structured `upgrade` object; UL2 gained `upgrade-modal.tsx` with desktop "Open in Terminal" / browser "Copy command" branches; UL3 expanded scope to include `window.nimbus.openUpgradeTerminal` IPC bridge alongside the original CLI-not-found setup card. |
