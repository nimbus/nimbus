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
upgrade popover (UL2), the desktop shell's IPC bridge for terminal-
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
                │   StalenessBanner        │   click [Update]    │ ┌──────────────────┐ │
                │   ├ available            │ ──────────────────▶ │ │ IPC: openUpgrade │ │
                │   ├ confirming (popover) │   click [Open Term] │ │ Terminal(tag)    │ │
                │   ├ launching            │ ──────────────────▶ │ │ tag → command    │ │
                │   ├ upgrading (spinner,  │ ◀────── { launched }─│ │ → osascript spawn│ │
                │   │  polls 2s × ≤10m)   │                     │ └──────────────────┘ │
                │   ├ upgraded (✓ 30s)    │                     │ ┌──────────────────┐ │
                │   └ hidden               │                     │ │ Notification     │ │
                │                          │  ◀── OS toast ──────│ │ on first stale   │ │
                │   Browser variant: skip  │                     │ │ (dedupe by       │ │
                │   [Open Term], use [Copy]│                     │ │  notified-versi- │ │
                │   Remote-host gate:      │                     │ │  ons.json)       │ │
                │   force [Copy] even on   │                     │ └──────────────────┘ │
                │   desktop                │                     └──────────┬───────────┘
                └─────────────────────────┘                                │
                                                                ┌──────────▼───────────┐
                                                                │ user's Terminal.app  │
                                                                │ `do script` AUTO-RUNS│
                                                                │ user watches brew/apt│
                                                                │ closes window when   │
                                                                │ done                 │
                                                                └──────────────────────┘

  Orthogonal: nimbus-desktop shell uses electron-updater for itself
  (already wired). Never polls api.github.com for the nimbus binary.
  Browser-only and remote-host operators see the same banner + popover
  but the popover's primary action is [Copy command] instead of
  [Open Terminal] — terminal-launch on the operator's local machine
  in a remote-nimbus topology would target the wrong host.
```

Three update axes, three independent owners:

| Axis | Detected by | Surfaced as | Applied by |
|---|---|---|---|
| **nimbus binary** stale | UL1 background task in nimbus | UL2 SPA banner + upgrade popover (browser + desktop) | operator's package manager (`brew/apt/dnf upgrade …`), launched in a terminal by UL3's IPC bridge on desktop or copy-paste in browser |
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
     opens the upgrade popover.
   - Dismissible per-session via `localStorage["nimbus-ui:staleness-
     dismissed-version"]` keyed to the *latest* version — dismissing
     0.1.41 still surfaces 0.1.42 when it lands.
   - Re-renders without a full page reload when the underlying value
     changes (poll completes, version flips).
2. New component `packages/nimbus-ui/src/components/upgrade-popover.tsx`
   — small popover **anchored to the banner's [Update] button**, not
   a full-screen modal. Matches the tightness of Podman Desktop's
   `extensionApi.window.showInformationMessage('Do you want to
   update to Y?', 'Yes', 'No')` (one line + two buttons; no
   explanatory body):
   - Triggered by the banner's [Update] CTA.
   - Header: "Run in Terminal?" (desktop) or "Copy command?"
     (browser / remote-host).
   - One row: `upgrade.command` in a monospace `<code>` block.
     **No body paragraph. No disclosure expander. No install-method
     subtitle.** The command is self-explanatory.
   - Two buttons: primary action + [Cancel]. Primary action depends
     on context (see below).
   - Esc dismisses the popover. Click-outside dismisses the popover.
     Dismissing the popover does **not** dismiss the banner — banner
     stays in `available` state.
3. **Banner state machine.** The banner is a finite state machine
   driven by `/api/system/version-info` polling + IPC signals from
   UL3. States, transitions, and visible copy:

   | State | Visible copy | Visible actions | Enters when |
   |---|---|---|---|
   | `hidden` | — | — | `available: false` OR dismissed-for-this-`latest` OR `checkStatus ∈ {disabled, never}` |
   | `available` | "Nimbus 0.1.41 available on `host.example.com`" | [Update] [Dismiss] | `available: true` and not dismissed |
   | `confirming` | (banner unchanged, popover open) | [Open Terminal] / [Copy command] / [Cancel] | user clicked [Update] |
   | `launching` | "Opening Terminal…" (brief, ≤1s) | — | desktop popover [Open Terminal] clicked, awaiting IPC ack |
   | `upgrading` | "Upgrading to 0.1.41 on `host.example.com`…" (spinner) | [Cancel] | IPC ack received OR (browser) [Copy command] clicked |
   | `upgraded` | "✓ Updated to 0.1.41" | [Dismiss] (auto-dismisses in 30s) | poll detects `current >= previously-known-latest` |

   - Transition `available → confirming`: open popover.
   - Transition `confirming → launching`: desktop only; click [Open
     Terminal] sends IPC `openUpgradeTerminal(method)` and the
     banner enters `launching`. On `{ launched: true }` ack from
     UL3, transition to `upgrading`. On `{ launched: false,
     fallback: 'copy' }`, treat as if [Copy command] was pressed.
   - Transition `confirming → upgrading`: browser variant; click
     [Copy command] writes to clipboard, shows a transient inline
     toast "Copied — paste into a terminal on `host.example.com`",
     enters `upgrading`.
   - Transition `upgrading → upgraded`: poll loop accelerates to
     2s after entering `upgrading`, capped at 10 minutes. On
     `current >= latest`, transition to `upgraded`. Save the
     incoming `latest` value at `upgrading` entry so we don't get
     fooled by a transient pre-poll cache.
   - Transition `upgrading → available` (timeout): if 10 minutes
     elapse with no version flip, fall back to `available` with
     subtext "Did the upgrade fail? You can try again." User may
     have closed Terminal without running the command, or the
     upgrade may have failed.
   - Transition `upgrading → available` (cancel): click [Cancel];
     same as timeout but without the failure subtext.
   - Transition `upgraded → hidden`: 30s elapsed.
4. **Remote-host gating.** Before showing the popover, compare
   `upgrade.host` (from the server response) with
   `window.location.host`. If they differ — e.g., browser at
   `https://nimbus.work.example.com` reading
   `upgrade.host: "work.example.com"` — **force the browser
   variant of the popover regardless of `window.nimbus`
   presence**, and replace "Run in Terminal?" with "Copy command
   to run on `host.example.com`?". The terminal-launch button
   would otherwise run brew on the *operator's laptop* instead of
   the remote server — wrong machine, wrong outcome. The clipboard
   path is correct because the operator copies it and SSHes into
   the right host.

   Local-host predicates: `upgrade.host` is `null`, `"localhost"`,
   `"127.0.0.1"`, `"::1"`, or matches `window.location.host`
   exactly.
5. **Fallback for `source` / `unknown` method.** When `upgrade.command`
   is `null`, the popover collapses to a one-liner "See the install
   docs" with a link to `upgrade.fallbackUrl` (opens in a new tab).
   No terminal-launch button, no copy button.
6. Banner + popover mounted in the global shell (`packages/nimbus-ui/
   src/routes/__root.tsx` or equivalent — confirm name during
   implementation since DU3 may have renamed).
7. Storybook stories (`staleness-banner.stories.tsx`,
   `upgrade-popover.stories.tsx`) covering: banner states
   (`available`, `upgrading`, `upgraded`) × two themes (six
   states); popover variants (desktop-local, browser-local,
   remote-host, source-method) × two themes (eight states) —
   ~14 visual states total; small enough to inspect end-to-end.

**Contract bullets:**

- **Single banner.** No version-specific dismissal layering — one
  active banner at a time, dismissed per-latest-version.
- **Popover, not modal.** The confirmation step is anchored to the
  [Update] button, not a full-screen overlay. Click-outside and
  Esc dismiss without affecting banner state. Matches GitHub
  Desktop's "Pull origin" popover and Podman Desktop's
  `showInformationMessage` dialog density — two buttons, minimal
  content.
- **A11y:** banner is `role="status"` with `aria-live="polite"` so
  screen readers announce it on first render but not on poll
  refreshes that don't change the visible content. Popover is
  `role="dialog"` with `aria-modal="false"` (non-blocking),
  focus-trapped while open, Esc-to-close. State transitions update
  `aria-live` regions so screen readers track `available →
  upgrading → upgraded`.
- **Theme-aware:** uses semantic state tokens from DU5's OKLCH
  palette (`--color-info` for `available`, `--color-warning` for
  `upgrading`, `--color-success` for `upgraded`). Passes axe-core
  AA in both themes per the DU5 verification standard.
- **No exit telemetry.** Dismissing the banner, clicking the
  Update CTA, or canceling the popover writes only to localStorage
  and (for desktop) sends an IPC message to the local main process.
  Nothing flows to the server.
- **Method tag, not command string.** The renderer's only outbound
  command surface is `window.nimbus.openUpgradeTerminal(method)`
  with `method` being one of six known strings. The renderer never
  constructs or forwards shell input. Mirrors the security model in
  `extensions/podman/packages/extension/src/extension.ts:1014`
  (`provider.registerUpdate({ update: () => … })`) — the renderer
  invokes a typed callback, not an arbitrary shell command.
- **Remote-host gating.** When the server's reported `host` does
  not match `window.location.host` (and isn't a localhost
  predicate), the terminal-launch button is *not rendered* even
  if `window.nimbus` is present. The clipboard path is the only
  option for remote-nimbus topologies — terminal-launch on the
  operator's local machine would target the wrong host.
- **Accelerated polling while upgrading.** Default poll is 5
  minutes (cheap, server caches anyway). On `upgrading` entry,
  poll cadence accelerates to 2s for up to 10 minutes, then falls
  back to `available` if no version flip is observed. Matches
  Podman Desktop's `extensionApi.window.withProgress(...)` pattern
  of giving the user immediate in-app feedback rather than
  silently going stale.

**Files touched:**

- `packages/nimbus-ui/src/components/staleness-banner.tsx` (new —
  banner shell that renders per-state copy)
- `packages/nimbus-ui/src/components/staleness-banner.stories.tsx`
  (new — one story per banner state)
- `packages/nimbus-ui/src/components/upgrade-popover.tsx` (new —
  small popover anchored to the banner button)
- `packages/nimbus-ui/src/components/upgrade-popover.stories.tsx`
  (new — desktop / browser / remote-host / source-method variants)
- `packages/nimbus-ui/src/hooks/use-upgrade-machine.ts` (new —
  state machine: state, transitions, accelerated polling. Plain
  React hook; no external state library.)
- `packages/nimbus-ui/src/lib/desktop-bridge.ts` (new — typed
  wrapper over `window.nimbus` that returns a null bridge when the
  preload isn't present, plus `isLocalHost(serverHost)` predicate
  for remote-host gating)
- `packages/nimbus-ui/src/routes/__root.tsx` (mount point)
- `packages/nimbus-ui/src/api/system.ts` (typed fetch wrapper for
  `/api/system/version-info`, new)

**Completion gate:**

- Vitest unit tests cover:
  - Dismissal-keying logic and `latest`-version-flips-re-shows.
  - The full banner state machine: each transition from the table
    above is asserted (available→confirming, confirming→launching,
    launching→upgrading, upgrading→upgraded, upgrading→available
    on timeout and on cancel, upgraded→hidden after 30s). Use
    `vi.useFakeTimers()` for the polling and timeout assertions.
  - The six-branch `upgrade.method` popover render matrix.
  - Remote-host gating: stub `window.location.host` and the
    response's `upgrade.host`; assert that with a mismatched host,
    the terminal-launch button is *absent* even with a present
    `window.nimbus` mock.
- Vitest unit test confirms that with a mocked
  `window.nimbus.openUpgradeTerminal`, clicking [Open Terminal]
  invokes it with exactly the method tag from the response (and
  no other arguments). With the bridge absent, the same click
  instead writes `upgrade.command` to the clipboard via
  `navigator.clipboard`.
- Storybook + Chromatic: stories cover banner states (`available`,
  `upgrading`, `upgraded`) × two themes; popover variants
  (desktop-local, browser-local, remote-host, source-method) ×
  two themes. ~16 visual states total — small enough to inspect.
- axe-core run against the embedded build: 0 violations in dark
  and light themes with the banner visible AND with the popover
  open (matches the DU5/DU6/DU6.5/DU7 bar).
- Manual end-to-end against a freshly-cut nimbus that exposes UL1:
  open `/ui/` in Chromium → banner appears in `available` state;
  click [Update] → popover opens; click [Copy command] → clipboard
  contains the brew upgrade command, banner enters `upgrading`;
  manually bump nimbus version, reload server → banner transitions
  to `upgraded` within 2 seconds; wait 30s → banner disappears.
- Manual remote-host test: point browser at a non-localhost
  nimbus, confirm the popover's primary action is [Copy command]
  (not [Open Terminal]) even in the desktop shell.

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
     hasTerminalLaunch(): boolean;
     openUpgradeTerminal(method: UpgradeMethod): Promise<{ launched: boolean; fallback?: 'copy' }>;
     openInstallTerminal(method: InstallMethod): Promise<{ launched: boolean; fallback?: 'copy' }>;
     retryResolveCli(): Promise<{ ok: boolean }>;
     onStaleness(handler: (info: VersionInfo) => void): () => void;
   };
   ```
   `UpgradeMethod` and `InstallMethod` are closed unions matching
   the server's `upgrade.method` set. An unknown tag is rejected
   at the IPC boundary; the renderer cannot smuggle commands
   through. `hasTerminalLaunch()` is a synchronous capability
   probe: returns `true` only on platforms where the main process
   has confirmed a terminal launcher is available (currently
   macOS-with-Terminal, see (2) below). The SPA reads this once at
   mount to decide whether to render [Open Terminal] vs [Copy
   command]. `onStaleness` is the desktop-side fan-out for the OS
   notification toast (see (5) below).
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
   Launcher behavior (canonical Podman Desktop semantics: launch,
   the command runs, the user watches):
   - **macOS**: `osascript -e 'tell app "Terminal" to do script
     "<cmd>"'`. Note: AppleScript's `do script` **auto-executes**
     the command in a new Terminal window — it does not pre-type.
     User watches brew/apt/dnf run, sees output, closes the window
     when done. Returns `{ launched: true }` after osascript
     completes (osascript returns synchronously after Terminal
     accepts the script; the actual upgrade runs asynchronously
     in Terminal).
   - **Linux**: try detection in order (`$TERMINAL` env var,
     `gnome-terminal`, `konsole`, `xterm`, `alacritty`, `kitty`).
     If found, spawn with the command. If not, return
     `{ launched: false, fallback: 'copy' }` so the SPA falls
     back to the clipboard path. **v1 ships copy-only on Linux**
     — terminal-launch on Linux is best-effort and not
     completion-gating.
   - **Windows**: spawn `wt.exe` (Windows Terminal) with the
     command if present, else `{ launched: false, fallback:
     'copy' }`. **v1 ships copy-only on Windows.**
   - For `method: "source"` or `"unknown"` where the command is
     `null`, return `{ launched: false, fallback: 'copy' }`
     immediately — the SPA shows the `fallbackUrl` link instead
     of either action.
   - `hasTerminalLaunch()` returns `true` only on macOS (v1) or
     Linux with a detected terminal at preload time. Computed
     once at startup so the SPA's render decision is stable.
3. **Notification toast on first detection.** The desktop main
   process polls `/api/system/version-info` on its own (in
   addition to the renderer's polling) and, on the *first*
   observed `available: true` transition for a given `latest`
   version, fires an Electron `Notification` toast: "Nimbus
   0.1.41 available — open the console to update." Click on the
   toast brings the existing window forward. Subsequent polls do
   not re-fire the toast for the same `latest`; the toast cache
   key is `latest`, persisted in `~/.config/nimbus-desktop/
   notified-versions.json` so a restart doesn't re-notify for
   a version the user has already seen.
4. **Setup card** `src/renderer/setup/CliNotFoundCard.tsx`
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
5. After the user installs, the Retry path picks up the new binary
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
- **Auto-run in terminal, watch don't type.** macOS `osascript do
  script` runs the command immediately in a new Terminal window.
  The user does not press Return to start; they watch brew/apt/dnf
  run and close the window when done. This matches Podman Desktop's
  `open <pkg> -W` semantics where the user observes the install,
  not types it.
- **No silent shell exec from main process.** The desktop never
  runs brew/apt/dnf out of view. The user always sees the install
  happen (in Terminal for v1; potentially in an in-app streaming
  progress widget in a future v2 if we want to mirror Podman's
  `ProgressLocation.TASK_WIDGET`).
- **No sudo escalation in the desktop process.** Commands that
  need root (`apt`, `dnf`) run inside the user's terminal where
  the standard sudo prompt handles auth. The desktop process
  never has elevated privileges.
- **Single notification toast per `latest`.** First detection of a
  given `latest` fires one toast, ever. Persisted in
  `notified-versions.json`. Re-detection (window-reload, restart,
  reconnect) does not re-notify until a new `latest` arrives.
- **Retry, don't restart.** The shell remains usable across the
  install/upgrade — a successful retry must not require quitting
  and relaunching.

**Files touched (in nimbus/desktop):**

- `src/main/server.ts` (signal cli-not-found instead of throwing
  into the void; trip the `retryResolveCli` re-run path)
- `src/main/ipc/upgrade.ts` (new — IPC handler + platform
  terminal launcher with `hasTerminalLaunch` capability probe)
- `src/main/ipc/upgrade.spec.ts` (new, vitest — tag whitelist,
  per-platform launcher behavior, capability-probe matrix)
- `src/main/notifications/staleness.ts` (new — main-process
  polling against `/api/system/version-info`, OS Notification
  fan-out, dedupe via `notified-versions.json`)
- `src/main/notifications/staleness.spec.ts` (new, vitest)
- `src/preload/index.ts` (extend with typed `window.nimbus`
  surface — `exposedInMainWorld` declarations follow the
  podman-desktop preload pattern at
  `packages/preload/exposedInMainWorld.d.ts`)
- `src/renderer/setup/CliNotFoundCard.tsx` (new)
- `src/renderer/setup/CliNotFoundCard.spec.ts` (new, vitest)
- `tests/e2e/cli-not-found.spec.ts` (new, packaged-shell
  Playwright)
- `tests/e2e/upgrade-terminal.spec.ts` (new — launch a fake
  Terminal binary on PATH via `TERMINAL=…` env override, assert
  the bridge spawns it with the expected command)
- `tests/e2e/staleness-notification.spec.ts` (new — assert the
  toast fires once on first detection, never again for the same
  `latest`, even across a window reload)

**Completion gate:**

- Vitest unit tests cover the IPC tag whitelist (six known
  methods + reject path for unknown), the platform launcher
  matrix, the `hasTerminalLaunch` capability probe per platform,
  retry semantics, the install-vs-upgrade method separation, and
  the notification dedupe via `notified-versions.json`.
- Packaged-shell Playwright E2E:
  - `cli-not-found.spec.ts`: launch with `PATH=/empty`, assert
    setup card renders; add a fake nimbus to PATH; click Retry;
    assert the card disappears and the normal `/ui/` window
    opens.
  - `upgrade-terminal.spec.ts`: launch with a stub terminal on
    PATH, render a banner with `method: "brew"`, click [Update]
    → [Open Terminal], assert the stub terminal received exactly
    `brew upgrade --cask nimbus/tap/nimbus` *and* that the
    renderer transitions to `upgrading` state.
  - `staleness-notification.spec.ts`: simulate version-info
    flipping to `available: true`, assert one OS notification
    fires; reload window; assert no second notification; flip
    `latest` to a new version, assert a second notification
    fires.
- Manual on macOS: install desktop cask only, launch, observe
  setup card; install CLI via Homebrew button (which opens
  Terminal, brew runs auto-executed), click Retry, observe
  normal `/ui/`; then artificially mark binary as stale
  (hand-edit `update-check.json`), reload `/ui/`, observe
  banner in `available` state and an OS notification toast;
  click [Update] → popover opens anchored to button; click
  [Open Terminal] → banner immediately enters `upgrading` state
  with spinner; in Terminal, brew auto-runs and completes; banner
  transitions to `upgraded` ✓ within 2 seconds; banner auto-
  dismisses after 30 seconds.

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
| 2026-05-16 | UX tightened to canonical Podman density | — | Honest comparison against Podman Desktop's actual surface revealed six gaps. Fixes: replaced `upgrade-modal.tsx` with `upgrade-popover.tsx` (anchored, two buttons, no body paragraph — same density as `showInformationMessage('Do you want to update to Y?', 'Yes', 'No')`); corrected `osascript do script` semantics (auto-runs, not pre-types); added a 6-state banner state machine (`hidden`/`available`/`confirming`/`launching`/`upgrading`/`upgraded`) with accelerated 2s polling during `upgrading` matching `withProgress` immediacy; added explicit success state with auto-dismiss; added remote-host gating (mismatched `upgrade.host` forces the [Copy command] branch — terminal-launch on the operator's local laptop would target the wrong machine); added Electron `Notification` toast on first stale detection with `notified-versions.json` dedupe. New surface files: `use-upgrade-machine.ts`, `desktop-bridge.ts`, `notifications/staleness.ts`. Existing files unchanged in shape; only the renderer-side UX gained density. |
