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
                │   status-bar version     │   click slot        │ ┌──────────────────┐ │
                │   slot (always visible)  │ ──────────────────▶ │ │ IPC: runUpgrade  │ │
                │   ├ normal (vX.Y.Z)     │   click [Update]    │ │ (tag)            │ │
                │   ├ available           │ ──────────────────▶ │ │ tag → argv       │ │
                │   │  ("update to X →")  │   ProgressEvent     │ │ → child_process. │ │
                │   ├ confirming (popover) │ ◀──── stream ───── │ │   spawn(argv,    │ │
                │   ├ upgrading            │      stdout/stderr │ │   shell:false)   │ │
                │   │  (polls 2s × ≤10m)  │                     │ │ → on exit 0:     │ │
                │   └ upgraded (✓ 30s)    │ ◀── restarted ──── │ │   SIGTERM + re-  │ │
                │                          │                     │ │   spawn nimbus   │ │
                │   + sonner toast on     │                     │ │   child (DS3)    │ │
                │   first detection       │                     │ └──────────────────┘ │
                │   + Settings → Server   │                     │ ┌──────────────────┐ │
                │   "Updates" row         │                     │ │ Notification     │ │
                │                          │ ◀── OS toast ──────│ │ on first stale   │ │
                │   Browser/remote/non-    │                     │ │ (dedupe by       │ │
                │   brew: popover shows    │                     │ │ notified-versi-  │ │
                │   [Copy command] only.   │                     │ │ ons.json)        │ │
                │   Remote-host gate via   │                     │ └──────────────────┘ │
                │   localhost predicate.   │                     └──────────────────────┘
                └─────────────────────────┘

  No Terminal window ever opens. Background `brew` mirrors Podman's
  `open -W <pkg>` "no terminal visible" UX — the in-app progress
  region is the GUI surface. Methods that need sudo TTY (apt, dnf,
  install-script) and every other platform/topology fall back to
  copy-only, where the user pastes into their own terminal.

  Orthogonal: nimbus-desktop shell uses electron-updater for itself
  (already wired). Never polls api.github.com for the nimbus binary.
  Browser-only and remote-host operators see the same status-bar
  slot + popover but the popover's only action is [Copy command] —
  background brew on the operator's local machine in a remote-
  nimbus topology would target the wrong host.
```

Three update axes, three independent owners:

| Axis | Detected by | Surfaced as | Applied by |
|---|---|---|---|
| **nimbus binary** stale | UL1 background task in nimbus | UL2 SPA status-bar slot + sonner toast + upgrade popover (browser + desktop) | operator's package manager: `brew upgrade` run headless via UL3's `runUpgrade` IPC on desktop+macOS, or copy-paste from the popover on every other path |
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

### UL2 — Staleness surfaces in `packages/nimbus-ui/`

**Goal:** the operator sees one persistent staleness signal in the
status bar (always visible, every route), one transient toast on
first detection, and one anchored popover for confirmation. **No top
banner.** Reuse existing shell primitives — `shell/status-bar.tsx`,
the sonner Toaster already mounted in `routes/__root.tsx`,
`components/copy-chip.tsx`, `components/state-dot.tsx` — instead of
introducing a new top-of-window surface that DESIGN.md does not
specify.

**Why this shape (vs. a top banner):** DESIGN.md §"Bottom Status Bar"
already reserves a "Server version + build hash (monospace, click
opens release notes)" slot. That slot is the canonical persistent
location for version state. Adding a top banner duplicates the
surface and violates the §"Aesthetic Stance: Industrial Precision"
rule against decorative chrome. Podman Desktop's persistent reminder
is the `"Update to {version}"` button on the provider card
(`packages/renderer/.../ProviderUpdateButton.svelte:59`) — a tile
the operator must navigate to. The status bar version slot is
*more* conspicuous because it survives every route change.

**Deliverables:**

1. **Status-bar version slot enhancement** in
   `packages/nimbus-ui/src/shell/status-bar.tsx`. Today the version
   slot renders as a single `CopyChip` (lines 57-61). Replace it
   with a stateful slot driven by `/api/system/version-info`:

   | Banner state | Slot rendering |
   |---|---|
   | `hidden` (up-to-date / opt-out / check failed) | `CopyChip` exactly as today: `v0.1.40+abcd123` |
   | `available` | `[StateDot --accent]` + `v0.1.40 · update to 0.1.41 →`, the whole row is a button that opens the upgrade popover anchored to itself |
   | `upgrading` | `[StateDot --starting half-filled]` + `Updating to 0.1.41…` (no click target while in this state) |
   | `upgraded` (≤30 s after success) | `[StateDot --success]` + `v0.1.41+def0123` (the new build) |

   The slot keeps the same dimensions in every state so the rest of
   the status bar does not shift. The `StateDot` glyph mapping is
   per DESIGN.md §Badges (line 372): `--accent` solid for actionable
   state, `--starting` half-filled for in-progress, `--success` solid
   for healthy.
2. **First-detection sonner toast.** When the poll transitions from
   `available: false` → `available: true` (or sees `available: true`
   for the first time after a page load with no prior dismissal
   record), emit one `toast()` via the existing
   `sonner` Toaster (already mounted at
   `routes/__root.tsx:35-46`). Toast content:

   ```
   Nimbus 0.1.41 available
   Update from 0.1.40.
   [Update]  [Dismiss]
   ```

   Clicking `[Update]` opens the upgrade popover anchored to the
   status-bar version slot (not anchored to the toast — the toast
   dismisses immediately on click). Clicking `[Dismiss]`
   writes `localStorage["nimbus-ui:staleness-dismissed-version"] =
   "0.1.41"` and dismisses the toast. The status bar slot keeps
   showing the `available` row regardless of toast dismissal —
   the toast is the *announcement*; the status bar is the
   persistent reminder.

   Subsequent polls do not re-emit the toast for the same `latest`.
   When `latest` flips to a new value (0.1.42), a fresh toast fires
   even if 0.1.41 was dismissed.
3. **Upgrade popover** in
   `packages/nimbus-ui/src/components/upgrade-popover.tsx` — a shadcn
   Popover (the project already uses Base UI / shadcn primitives per
   DESIGN.md §"Implementation Rules") anchored to the status-bar
   version slot. Contents, in Podman density:

   ```
   ┌─────────────────────────────────────────┐
   │ Update Nimbus to 0.1.41?                │
   │                                          │
   │   brew upgrade nimbus/tap/nimbus  [📋]  │  ← CopyChip
   │                                          │
   │                  [ Cancel ]  [ Update ]  │
   └─────────────────────────────────────────┘
   ```

   - Header line: `Update Nimbus to 0.1.41?` (matches Podman's
     `Do you want to update to {newVersion}?` density, retitled per
     DESIGN.md §"Copy And Terminology" tone — action verb).
   - Body row: `upgrade.command` rendered with the existing
     `CopyChip` component. **No body paragraph. No disclosure. No
     install-method subtitle.** The command is self-explanatory.
   - Primary button label depends on context:
     - **Desktop + local-host**: `Update` (runs the upgrade in the
       background via UL3's IPC bridge; no terminal opens).
     - **Browser, or remote-host, or desktop without
       `window.nimbus`**: `Copy command` (writes the command to the
       clipboard and shows an inline "Copied" microtoast).
   - Secondary button: `Cancel`. Esc and click-outside also dismiss.
4. **Banner state machine** (drives the status-bar slot and the
   popover). Owned by `packages/nimbus-ui/src/hooks/use-staleness.ts`:

   | State | Status-bar slot | Popover | Enters when |
   |---|---|---|---|
   | `hidden` | normal version chip | closed | `available: false` OR `checkStatus ∈ {disabled, never, failed}` |
   | `available` | "update to X →" row | closed; user can click slot to open | `available: true` |
   | `confirming` | "update to X →" row | open, anchored to slot | user clicked slot OR clicked toast `[Update]` |
   | `upgrading` | "Updating to X…" row | closed | popover `[Update]` clicked (desktop) OR `[Copy command]` clicked (browser) |
   | `upgraded` | new version + success dot (30 s) | closed | poll detects `current >= upgrade-target latest` |

   - `available → confirming`: open popover.
   - `confirming → upgrading` (desktop+local):
     `window.nimbus.runUpgrade(method)` returns an `AsyncIterable`
     of progress events; the hook subscribes and stays in
     `upgrading` until either the iterable closes (success →
     `upgraded`) or yields `{ kind: 'error', ... }` (revert to
     `available` with a sonner error toast).
   - `confirming → upgrading` (browser/remote): write to clipboard
     via `navigator.clipboard.writeText(upgrade.command)`, emit a
     transient sonner toast `Copied — run on host.example.com`,
     enter `upgrading`. The hook then polls every 2 s waiting for
     `current >= latest`; if 10 minutes elapse, revert to
     `available` with sonner `Upgrade not detected. Try again?`.
   - `upgrading → upgraded`: poll loop accelerates to 2 s after
     `upgrading` entry, capped at 10 minutes. On `current >=
     upgrade-target-latest`, enter `upgraded` and emit a sonner
     success toast `Nimbus 0.1.41 running`.
   - `upgraded → hidden`: 30 s elapsed.

   The cap-and-target value is sampled at `upgrading` entry so a
   transient pre-poll cache hit cannot fool the transition.
5. **Remote-host gating.** When `upgrade.host` (server-reported)
   does not match `window.location.host` and is not a localhost
   predicate (`null`, `localhost`, `127.0.0.1`, `::1`), the popover
   *unconditionally* renders the `Copy command` branch — even with
   `window.nimbus` present. Calling `runUpgrade` on the operator's
   laptop would target the wrong machine. The popover header in
   this case reads `Copy command to run on host.example.com?`.
6. **Settings → Server section row.** Add one `Definition` row to
   the existing `ServerInfoSection` in
   `packages/nimbus-ui/src/routes/settings.tsx:317`:

   | Updates | "Up to date" *(--success state-chip)* OR "0.1.41 available — [Update]" *(button opens popover anchored here)* |

   This is the second persistent surface. Status bar is always
   visible (every route); Settings page surfaces the same state in
   context with version, uptime, listen address, encryption etc.
7. **Fallback for `source` / `unknown` method.** When
   `upgrade.command` is `null`, the popover collapses to a single
   line `See the install docs` linking to `upgrade.fallbackUrl`.
   No primary action button.
8. **Mount points.** Status-bar enhancement is in-place. Sonner
   toast and popover are both mounted under the existing
   `routes/__root.tsx` shell — no new top-level layout changes.
9. **Storybook stories** covering the status-bar slot in each of
   the four states (`hidden`/`available`/`upgrading`/`upgraded`)
   × two themes, and popover variants (desktop-local,
   browser-local, remote-host, source-method) × two themes. ~16
   visual states total; small enough to inspect end-to-end.

**Contract bullets:**

- **Reuse before invent.** Status-bar slot, sonner Toaster,
  CopyChip, StateDot, Settings → Server section, shadcn Popover.
  Two genuinely new files only: `use-staleness.ts` and
  `upgrade-popover.tsx`. No new top-level layout, no new banner
  component, no new toast queue.
- **Status bar is the persistent reminder.** Sonner is one-shot
  (per `latest`); the status bar version slot persists every
  route, every reload. Matches DESIGN.md §"Bottom Status Bar"
  exactly. Inspired by Podman's `ProviderUpdateButton.svelte:59`
  pattern of a persistent button on a relevant card.
- **Popover, not modal.** Anchored to the status-bar version
  slot. Click-outside and Esc dismiss without affecting state.
  Matches Podman's `showInformationMessage(...)` density —
  one row of content, two buttons.
- **A11y:** status-bar slot is `role="status"` with
  `aria-live="polite"`. Popover is shadcn-default `role="dialog"`
  with `aria-modal="false"`, focus-trapped while open, Esc-to-
  close. State transitions update `aria-live` regions so screen
  readers track `available → upgrading → upgraded`. Passes
  axe-core AA in both themes per the DU5/DU6 standard.
- **Theme-aware:** status-bar state-dot uses DESIGN.md tokens —
  `--accent` for `available`, `--starting` for `upgrading`,
  `--success` for `upgraded`.
- **No exit telemetry.** Dismissing the toast or canceling the
  popover writes only to localStorage and (for desktop) sends
  an IPC message to the local main process. Nothing flows to the
  server.
- **Method tag, not command string.** The renderer's only
  outbound command surface is `window.nimbus.runUpgrade(method)`
  with `method` being one of six known strings. The renderer
  never constructs or forwards shell input. Same security model
  as Podman's `provider.registerUpdate({ update: () => … })` at
  `extensions/podman/packages/extension/src/extension.ts:1014`
  — typed callback, not arbitrary shell.
- **Remote-host gating.** When the server's reported `host` does
  not match `window.location.host` (and isn't a localhost
  predicate), the popover always renders the `Copy command`
  branch. `runUpgrade` would otherwise run brew on the operator's
  laptop instead of the remote server.
- **Accelerated polling while upgrading.** Default poll is 5
  minutes (cheap, server caches anyway). On `upgrading` entry,
  cadence accelerates to 2 s for up to 10 minutes. Mirrors
  Podman's `extensionApi.window.withProgress(...)` immediacy.

**Files touched:**

- `packages/nimbus-ui/src/shell/status-bar.tsx` (modify in place —
  the version slot becomes stateful via the `useStaleness` hook;
  every other slot unchanged)
- `packages/nimbus-ui/src/components/upgrade-popover.tsx` (new —
  shadcn Popover wrapper; ~80 lines)
- `packages/nimbus-ui/src/components/upgrade-popover.stories.tsx`
  (new — desktop / browser / remote-host / source-method × theme)
- `packages/nimbus-ui/src/hooks/use-staleness.ts` (new — fetch
  `/api/system/version-info`, run the 5-state machine, expose
  current state + transitions. Plain React hook; no external
  state library; ~150 lines.)
- `packages/nimbus-ui/src/lib/desktop-bridge.ts` (new — typed
  wrapper over `window.nimbus`; returns a null bridge when the
  preload isn't present; exports `isLocalHost(serverHost)`
  predicate)
- `packages/nimbus-ui/src/api/system.ts` (new — typed fetch
  wrapper for `/api/system/version-info`)
- `packages/nimbus-ui/src/routes/settings.tsx` (modify in place —
  add the `Updates` `Definition` row to `ServerInfoSection`)
- `packages/nimbus-ui/src/stories/status-bar.stories.tsx` (new
  or extend if a status-bar story already exists — cover the
  four version-slot states × theme)

**Completion gate:**

- Vitest unit tests cover:
  - The state machine: each transition asserted using
    `vi.useFakeTimers()` for poll + timeout assertions.
  - Dismissal-keying logic (sonner dismiss writes localStorage;
    `latest` flip re-emits).
  - The six-branch `upgrade.method` popover render matrix.
  - Remote-host gating: stub `window.location.host` and the
    response's `upgrade.host`; assert the popover renders
    `Copy command` (not `Update`) even with `window.nimbus` mocked.
  - With `window.nimbus.runUpgrade` mocked, clicking `Update`
    invokes it with exactly the method tag — no command string
    crosses the boundary.
- Storybook + Chromatic: ~16 visual states (status-bar slot ×
  4 × 2 themes + popover × 4 × 2 themes). Small enough to
  inspect end-to-end.
- axe-core run against the embedded build: 0 violations in dark
  and light themes with the status-bar in `available` state AND
  with the popover open (matches the DU5/DU6 bar).
- Manual end-to-end against a freshly-cut nimbus that exposes UL1:
  open `/ui/` in Chromium → status-bar slot shows current
  version; trigger staleness (hand-edit `update-check.json`
  via UL1's cache file) → status-bar slot transitions to "update
  to 0.1.41 →" and a sonner toast appears; click slot → popover
  opens anchored beneath; click `Copy command` → clipboard
  contains `brew upgrade nimbus/tap/nimbus`, status-bar slot
  enters `Updating to 0.1.41…`; manually upgrade and reload
  server → status-bar slot transitions to `v0.1.41` with brief
  green dot, then back to normal.
- Manual remote-host test: point a desktop shell at a non-
  localhost nimbus, confirm the popover's primary action is
  `Copy command` (never `Update`).

### UL3 — Desktop shell: setup card + background upgrade runner

**Goal:** the desktop shell adds two install-method-aware surfaces:

1. A first-run setup card when no `nimbus` CLI is on PATH (replaces
   the raw `NimbusBinaryNotFoundError` death-screen).
2. An IPC bridge `nimbus.runUpgrade(method)` that lets the SPA's
   UL2 Update CTA spawn the package manager **in the background**
   (no Terminal window opens), stream progress events to the
   renderer, and restart the nimbus child process when the binary
   on disk changes so version state stays in sync naturally.

Both are forms of the same pattern: the desktop renders a button,
the renderer passes a method tag, the main process maps that tag
to a whitelisted argv via `child_process.spawn`, streams stdout/
stderr lines to the renderer as `ProgressEvent`s, and — for
upgrades — gracefully restarts the nimbus child when the runner
exits successfully. This matches Podman Desktop's "no Terminal
visible" semantics (`open -W <pkg>` blocks on the installer GUI;
`extensionApi.window.withProgress({ location: ProgressLocation
.TASK_WIDGET, ... })` renders in-app progress) without coupling
to a `.pkg` artifact we don't ship.

**Owner:** `nimbus/desktop` (separate repo, separate release cadence).

**Deliverables:**

1. **Preload bridge** `src/preload/index.ts` exposes a typed
   `window.nimbus`:
   ```ts
   type ProgressEvent =
     | { kind: 'started'; method: UpgradeMethod | InstallMethod; argv: readonly string[] }
     | { kind: 'stdout'; line: string }
     | { kind: 'stderr'; line: string }
     | { kind: 'exit'; code: number; signal: NodeJS.Signals | null }
     | { kind: 'restarted'; newVersion: string }
     | { kind: 'error'; message: string; fallback: 'copy' };

   window.nimbus = {
     canRunUpgrade(method: UpgradeMethod): boolean;
     canRunInstall(method: InstallMethod): boolean;
     runUpgrade(method: UpgradeMethod): AsyncIterable<ProgressEvent>;
     runInstall(method: InstallMethod): AsyncIterable<ProgressEvent>;
     retryResolveCli(): Promise<{ ok: boolean }>;
     onStaleness(handler: (info: VersionInfo) => void): () => void;
   };
   ```
   `UpgradeMethod` and `InstallMethod` are closed unions matching
   the server's `upgrade.method` set. An unknown tag is rejected
   at the IPC boundary; the renderer cannot smuggle commands or
   argv arrays through. `canRunUpgrade`/`canRunInstall` are
   synchronous capability probes: return `true` only when the
   main process has verified the binary for that method exists on
   PATH **and** the method does not require an interactive sudo
   TTY (so `brew` returns `true`; `apt`/`dnf` return `false` on
   their respective hosts; see (2) below). The SPA reads these
   once at mount to decide whether to render [Update] vs [Copy
   command]. `runUpgrade`/`runInstall` return an async-iterable
   stream of progress events, implemented over `ipcRenderer.on`
   with a per-call subscription token so concurrent calls (rare
   but possible across windows) stay isolated. `onStaleness` is
   the desktop-side fan-out for the OS notification toast
   (see (3) below).

2. **Main process runner** `src/main/upgrade/runner.ts` (new) maps
   method tags to a hardcoded argv table and spawns via
   `child_process.spawn` with `stdio: ['ignore', 'pipe', 'pipe']`
   (no shell, no TTY, no Terminal window):
   ```ts
   const UPGRADE_ARGV: Record<UpgradeMethod, readonly string[] | null> = {
     brew: ['brew', 'upgrade', '--cask', 'nimbus/tap/nimbus'],
     apt: null,           // requires sudo TTY — copy-only in v1
     dnf: null,           // requires sudo TTY — copy-only in v1
     'install-script': null, // sudo+pipe-to-sh — copy-only in v1
     source: null,
     unknown: null,
   };
   ```
   Runner behavior — canonical Podman Desktop semantics
   (`withProgress` + in-app progress widget; no Terminal):
   - **macOS + brew (v1 in-scope)**: `spawn('brew', [...])`
     inheriting the user's `HOME` and a sanitized `PATH` that
     includes `/opt/homebrew/bin` and `/usr/local/bin`. stdout
     and stderr are piped, split on `\n`, and emitted as
     `{ kind: 'stdout', line }` / `{ kind: 'stderr', line }`
     events. On `exit code === 0`, the runner triggers the
     **post-upgrade restart sequence** in (4) below and emits
     `{ kind: 'restarted', newVersion }` once the new child
     reports its version. On non-zero exit, emits
     `{ kind: 'exit', code, signal }` and the SPA transitions
     `upgrading → available` with the original Update CTA still
     in place.
   - **apt / dnf / install-script (all platforms, v1)**: the
     runner refuses to spawn — `canRunUpgrade('apt')` returns
     `false` — and the SPA falls back to the `Copy command`
     branch. Reason: these require interactive `sudo`, which has
     no TTY when spawned headless; running them silently would
     either fail or (worse) succeed under a passwordless sudo
     misconfiguration. Out-of-scope until we own a secure
     elevation surface.
   - **Windows (v1)**: copy-only across all methods. Defer the
     winget / scoop runner until distribution lands those.
   - For `method: "source"` or `"unknown"`, the runner refuses
     to spawn and the SPA shows the `fallbackUrl` link instead.

3. **Notification toast on first detection.** The desktop main
   process polls `/api/system/version-info` on its own (in
   addition to the renderer's polling) and, on the *first*
   observed `available: true` transition for a given `latest`
   version, fires an Electron `Notification`: "Nimbus 0.1.41
   available — open the console to update." Click brings the
   existing window forward. Subsequent polls do not re-fire for
   the same `latest`; the cache key is `latest`, persisted in
   `~/.config/nimbus-desktop/notified-versions.json` so a
   restart doesn't re-notify for a version the user has already
   seen. This is the desktop-only second persistent reminder
   surface (alongside the SPA's status-bar slot and Settings
   row from UL2).

4. **Post-upgrade restart sequence** `src/main/server.ts` (modify
   in place — DS3 already owns nimbus child-process lifecycle
   here via `resolveNimbusExecutable` at line ~200 and the
   spawn path). On `runUpgrade` exit code 0:
   1. Resolve the binary again from PATH (`brew upgrade` may have
      replaced `/opt/homebrew/bin/nimbus` in place — same path,
      new inode).
   2. SIGTERM the current nimbus child. Wait up to 5s for clean
      exit; SIGKILL on timeout.
   3. Spawn the new binary. Wait for the existing readiness probe
      (HTTP `GET /api/system/version-info`) to succeed.
   4. Read `current` from the readiness response and emit
      `{ kind: 'restarted', newVersion: current }` to the
      renderer over the active `runUpgrade` subscription.
   5. The existing `DisconnectedOverlay` in `packages/nimbus-ui/
      src/shell/disconnected-overlay.tsx` already covers the
      brief WebSocket gap during step (2)–(3); no new UI is
      required for the gap itself.
   The SPA's `useStaleness` hook, on receiving `restarted`,
   transitions state machine to `upgraded`, displays the new
   version with a 30-second `--success` dot in the status-bar
   slot, and resets the polling cadence to 5min. **This is the
   "robust sync solution naturally based on architecture"
   answer for question (5):** the desktop already owns the
   nimbus process per DS3, so version sync is a property of the
   restart, not a separate signal — the new process can only
   ever report the new version.

5. **Setup card** `src/renderer/setup/CliNotFoundCard.tsx`
   (new — verify the actual shell layout against DS3 during
   implementation):
   - Triggered when `resolveNimbusExecutable` at `src/main/server
     .ts:200` throws `NimbusBinaryNotFoundError`. The main
     process posts `cli-not-found` to the renderer, which swaps
     the window contents to the setup card.
   - The card surfaces:
     - **macOS**: button "Install with Homebrew" →
       `window.nimbus.runInstall('brew')`. Progress events
       stream into an in-card progress region (last 8 lines of
       stdout, monospace, JetBrains Mono per DESIGN.md). On
       `exit code === 0`, automatically calls
       `retryResolveCli()` and transitions to the normal `/ui/`
       window.
     - **Linux**: link to `https://github.com/nimbus/nimbus#install`
       (sudo+TTY requirement — copy-only).
     - **Windows**: link to direct-download docs.
     - Common: a "Retry" button calling
       `window.nimbus.retryResolveCli()` for users who installed
       out-of-band (e.g., curl install-script in their own
       terminal).
   - Visual treatment: DESIGN.md §"Empty And Error States"
     (line 535-557) — `Card` primitive, single primary action,
     short imperative copy, no marketing language.

**Contract bullets:**

- **No Terminal window opens — ever.** v1 background-brew runs
  under `child_process.spawn` with no shell and no TTY. Mirrors
  Podman Desktop's `open -W <pkg>` semantics (the installer
  appears as native chrome, not a terminal). The renderer
  surfaces stdout in-app via the progress region.
- **Background runner is bounded by method capability.** brew on
  macOS runs in-process; everything else (apt, dnf, install-
  script, all of Windows) falls back to copy-only. The decision
  is the capability probe `canRunUpgrade(method)`; the SPA
  branches on the probe, not the platform.
- **No bundled installer.** The shell never downloads or
  executes the nimbus binary itself. It invokes a package
  manager (where one exists) or hands off to install docs.
- **Method tag whitelist + argv table.** The IPC boundary
  accepts only known method tags. The main process constructs
  the **argv array** (not a shell string) from a local
  hardcoded table; no `cmd.exe`/`sh -c` is ever invoked. The
  renderer never sees or forwards a shell string.
- **No sudo escalation in the desktop process.** Commands that
  need root (`apt`, `dnf`, the install script) are out of scope
  for the runner in v1 — they go through the copy-only path
  where the user pastes into their own terminal and sudo
  prompts there. The desktop process never has elevated
  privileges and never invokes `sudo` directly.
- **Desktop owns version sync.** Because the desktop spawned
  the nimbus child (DS3), it is the only component that can
  authoritatively restart it. The renderer never restarts
  nimbus; it observes the `restarted` progress event. This
  eliminates the class of bug where the UI thinks an update
  is pending after the binary on disk already moved.
- **Single notification toast per `latest`.** First detection of
  a given `latest` fires one OS notification, ever. Persisted
  in `notified-versions.json`. Re-detection (window-reload,
  restart, reconnect) does not re-notify until a new `latest`
  arrives.
- **Retry, don't restart.** The shell remains usable across
  install/upgrade — a successful retry must not require
  quitting and relaunching the desktop app.

**Files touched (in nimbus/desktop):**

- `src/main/server.ts` (modify in place — signal `cli-not-found`
  instead of throwing into the void; add the post-upgrade
  restart sequence hooked into the runner's exit callback)
- `src/main/upgrade/runner.ts` (new — `child_process.spawn`
  runner, method→argv table, capability probes, line-split
  stdout/stderr → ProgressEvent stream)
- `src/main/upgrade/runner.spec.ts` (new, vitest — tag
  whitelist, argv-not-shell-string assertion, capability
  matrix, line-split behavior on partial reads)
- `src/main/notifications/staleness.ts` (new — main-process
  polling against `/api/system/version-info`, OS Notification
  fan-out, dedupe via `notified-versions.json`)
- `src/main/notifications/staleness.spec.ts` (new, vitest)
- `src/preload/index.ts` (extend with typed `window.nimbus`
  surface — `exposedInMainWorld` declarations follow the
  podman-desktop pattern at
  `packages/preload/exposedInMainWorld.d.ts`)
- `src/renderer/setup/CliNotFoundCard.tsx` (new)
- `src/renderer/setup/CliNotFoundCard.spec.ts` (new, vitest)
- `tests/e2e/cli-not-found.spec.ts` (new, packaged-shell
  Playwright — full install flow against a fake `brew` on PATH)
- `tests/e2e/upgrade-runner.spec.ts` (new — assert
  `runUpgrade('brew')` spawns the right argv with no shell;
  assert ProgressEvent stream order; assert restart-on-exit-0
  re-resolves the binary and emits `restarted`)
- `tests/e2e/staleness-notification.spec.ts` (new — assert one
  notification per `latest`, never re-fires across reloads)

**Completion gate:**

- Vitest unit tests cover the IPC tag whitelist (six known
  methods + reject path for unknown), the capability-probe
  matrix per (method, platform), the spawn argv (assert
  `shell: false` and no `cmd.exe`/`sh`), line-split on partial
  reads, restart-on-exit-0 sequence (SIGTERM → readiness probe
  → `restarted` event), retry semantics, the install-vs-upgrade
  method separation, and notification dedupe via
  `notified-versions.json`.
- Packaged-shell Playwright E2E:
  - `cli-not-found.spec.ts`: launch with `PATH=/empty`, assert
    setup card renders; install a fake `brew` that drops a
    fake `nimbus` into a temp PATH dir; click Install with
    Homebrew; assert the progress region streams brew's fake
    output; assert auto-retry succeeds and the normal `/ui/`
    window opens.
  - `upgrade-runner.spec.ts`: launch against a fake nimbus that
    reports `available: true` and `method: "brew"`; click
    [Update] → [Update] in popover; assert the spawn argv is
    exactly `['brew', 'upgrade', '--cask', 'nimbus/tap/nimbus']`
    with `shell: false`; assert the renderer transitions to
    `upgrading` immediately; have the fake brew exit 0 and
    swap the fake nimbus binary; assert the desktop SIGTERMs
    the old child, the readiness probe succeeds against the
    new child, and `{ kind: 'restarted', newVersion: '0.1.41'
    }` reaches the renderer; assert the status-bar slot enters
    `upgraded` with the new version and auto-dismisses after
    30s.
  - `staleness-notification.spec.ts`: simulate version-info
    flipping to `available: true`, assert one OS notification
    fires; reload window; assert no second notification; flip
    `latest` to a new version, assert a second notification
    fires.
- Manual on macOS: install desktop cask only, launch, observe
  setup card; install CLI via Homebrew button (brew runs in
  background, progress streams into the card, no Terminal
  opens); when brew exits, observe automatic transition to
  normal `/ui/`; then artificially mark binary as stale (hand-
  edit `update-check.json`), wait one poll cycle, observe an
  OS notification fires and the status-bar version slot enters
  `available` ("update to 0.1.41 →"); click slot → popover
  opens anchored beneath; click [Update] → slot enters
  `upgrading` (no Terminal opens); when brew exits the desktop
  restarts the nimbus child, the SPA's WebSocket reconnects via
  the existing `DisconnectedOverlay`, and the slot transitions
  to the new version with a 30-second green dot.

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
     pattern: in the desktop on macOS+brew, the click runs `brew
     upgrade` in the background (no Terminal opens) with progress
     streamed into the status-bar slot and a popover; on every
     other platform/method combination, the click copies the
     command to the clipboard so the operator pastes into their
     own terminal. The user always confirms before the command
     runs. Cite decision 001's "Real-world analogs" for the why.
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
- **Silent `process.exec` of `apt`/`dnf`/install-script from the
  desktop main process.** These require interactive sudo, which
  has no TTY when spawned headless from Electron. Silent exec
  would either fail outright or — under a passwordless sudo
  misconfig — succeed in a way the operator didn't authorize.
  v1 routes these methods through the copy-only path where the
  user pastes into their own terminal and sudo prompts there.
  **Note:** background `brew` on macOS *is* in scope per UL3 —
  brew does not require sudo and emits structured stdout we can
  stream into the in-app progress region.
- **Bundling installer artifacts inside `nimbus-desktop`.** Podman
  Desktop bundles `podman-installer-macos-*.pkg` (one of the
  pkg-arch-version assets) inside its .app and invokes it via
  `open -W <pkg>`. We don't currently produce nimbus
  `.pkg`/`.msi`/`.deb`/`.rpm` installer artifacts; we hand off to
  brew/apt/dnf. Once distribution lands its own pkg installer
  track (see `docs/plans/distribution-plan.md`), bundling is the
  obvious next step — until then, background-brew + copy-only is
  the pragmatic v1.
- **Sudo elevation surface in the desktop process.** A future
  refinement could front a polkit / Authorization Services prompt
  for `apt`/`dnf` and run them headlessly. Out of scope for v1 —
  the security review surface is non-trivial and the copy-only
  fallback already works on every platform.
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
| 2026-05-16 | UX restructured around DESIGN.md primitives + background brew | — | Five-question deep review found the prior revision still drifted from canonical patterns. Corrections: (1) `osascript do script` does open a visible Terminal window — Podman's `open -W <pkg>` does not; rewrote UL3 to spawn `brew` via `child_process.spawn` with `shell: false` (no Terminal opens, mirroring Podman's "in-app progress" UX without coupling to a `.pkg`). (2) DESIGN.md has no top-banner surface; eliminated the invented `staleness-banner.tsx` and routed the persistent reminder through the existing **status-bar version slot** (`shell/status-bar.tsx`, modify-in-place) — every route shows it, no new chrome. (3) Component reuse audit: sonner `Toaster` is already mounted at `routes/__root.tsx:35-46`; Popover/CopyChip/StateDot already exist; the rewrite now extends 2 files in place and adds 6 new files (down from 9 new). (4) Persistent reminder ladder is now three-tier per canonical patterns: announcement (sonner toast, dismissible) → ambient (status-bar slot, always visible) → contextual (Settings → Server "Updates" row); Podman's `ProviderUpdateButton.svelte:59` confirms the ambient-persistent surface as the canonical anchor. (5) Version sync post-upgrade is a property of DS3's existing nimbus-child-process ownership: on `brew` exit 0, desktop SIGTERMs the child and respawns it; the existing `DisconnectedOverlay` covers the WebSocket gap; the new child can only ever report the new version — no separate sync signal needed. State machine reduced from 6 to 5 states (dropped `launching` — no Terminal phase exists anymore). Background-brew moved from "out of scope" to in-scope for macOS+brew; `apt`/`dnf`/install-script remain copy-only in v1 (sudo TTY requirement). |
| 2026-05-16 | UL1 landed | done | `crates/nimbus-server/src/system/{mod,install_method,cache,version_check}.rs` + `http/version_info.rs` + `protocol::VersionInfoResponse` + `state::AppState::version_check` + `router::build_local_admin_router` route `/api/system/version-info`. 18 unit tests pass (12 `install_method::*` covering all 6 detection branches; 6 `version_check::*` covering `never`/`fresh`/`stale`/`error`/`disabled` and `v`-prefix semver). Live smoke against GitHub: first GET → `{checkStatus:"never", latest:null}`; second GET after ~4 s → `{checkStatus:"fresh", latest:"0.1.31", url:"https://github.com/nimbus/nimbus/releases/tag/v0.1.31", publishedAt:"2026-05-15T00:32:23Z", upgrade.method:"source"}` (host running from `target/release/`). `~/.config/nimbus/update-check.json` persisted with `cached` + `lastCheckedAt`. `NIMBUS_DISABLE_UPDATE_CHECK=1` returns `checkStatus:"disabled"` and never writes the cache file. `make clippy` clean (collapsed the workspace's three Windows `HOMEDRIVE`/`HOMEPATH` if-let chains for rust 1.93's stricter `collapsible_if`). |
| 2026-05-16 | UL2 landed | done | `packages/nimbus-ui/src/api/system.ts` (typed `fetchVersionInfo`), `lib/desktop-bridge.ts` (`window.nimbus` wrapper + `isLocalHost` predicate), `hooks/use-staleness.ts` (5-state machine + `StalenessProvider`/`useStalenessContext`), `components/upgrade-popover.tsx` (Base UI Popover anchored to the trigger). Status-bar version slot in `shell/status-bar.tsx` now switches between `CopyChip`/`UpgradeDot+trigger`/`Updating…`/`upgraded` per state. Sonner toast announces a new `latest` once per version with `[Update]`/`[Dismiss]` actions; dismissal keys on `localStorage["nimbus-ui:staleness-dismissed-version"]`. `routes/settings.tsx` `ServerInfoSection` gained the `Updates` row consuming the same context. 86 vitest tests pass (23 new: 8 staleness state-machine including dismissal+method-tag-only security, 9 popover render matrix covering desktop/browser/remote-host/source-method, 6 desktop-bridge predicate). `biome check` and `tsc --noEmit` clean. |
