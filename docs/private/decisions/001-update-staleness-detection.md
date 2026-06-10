# 001 — Update lifecycle: server pings, both UIs render, both UIs offer to launch the upgrade

- **Status:** accepted (plan closed 2026-05-16)
- **Date:** 2026-05-16
- **Decision owner:** `nimbus/nimbus` maintainers
- **Parent plan:** `docs/plans/archive/update-lifecycle-plan.md`

---

## Context

Nimbus ships two operator-facing surfaces that need to know when the
nimbus binary is behind on releases:

1. **Browser** — any modern browser pointed at `http://<host>:<port>/ui/`,
   served from the same `rust_embed` bundle the binary compiles in.
2. **Desktop** — the `nimbus-desktop` Electron shell at
   `github.com/nimbus/desktop`, which is itself a Chromium window
   pointed at the same `/ui/` URL after a `nimbus start` discovery
   or spawn handshake.

Both surfaces render the same SPA bundle from the same running
`nimbus` server. The desktop is a thin Chromium kiosk, not a versioned
client of a versioned server. The interface between desktop and
nimbus today is `nimbus start`, `server.json` discovery, and plain
HTTP to `/ui/` — there is no proto-version handshake to negotiate.

There is also a third update axis — the desktop shell itself
(Electron + Chromium + tray + window state) — but that is already
solved by `electron-updater` on its own cadence
([nimbus/desktop decision 003](https://github.com/nimbus/desktop/blob/main/docs/decisions/003-auto-update-channel.md))
and is out of scope here. This decision only covers staleness of the
**nimbus binary**.

There are actually two coupled questions:

1. **Who detects when the nimbus binary is behind, and how does that
   signal reach both UIs?**
2. **When the operator wants to act on the signal, who runs the
   upgrade, and how does the UI mediate that handoff?**

Five viable shapes across that 2D space:

- **(α) Each client pings GitHub independently.** The desktop shell
  polls `api.github.com/repos/nimbus/nimbus/releases/latest`,
  compares to `nimbus --version`, shows a toast. Browser users get
  nothing.
- **(β-) Server detection, banner only.** Nimbus runs a
  stale-while-revalidate background check, exposes the result on a
  `/api/system/version-info` endpoint, and the SPA renders a banner
  that shows a `brew upgrade …` hint. The operator copies the command
  into their own terminal — the UI never launches anything.
- **(β+) Server detection, UI orchestrates the launch.** Same
  detection contract as (β-), but the SPA also renders a clickable
  **Update** CTA. In the desktop on macOS+brew, the CTA spawns
  `brew upgrade` in the **background** via `child_process.spawn`
  (no Terminal window opens) with stdout streamed into an in-app
  progress region; on `exit 0` the desktop SIGTERMs the nimbus
  child and respawns it so version state syncs naturally. For
  every other platform/method (apt, dnf, install-script, all of
  Windows, browser, remote nimbus), the CTA opens an anchored
  popover with a copy-to-clipboard chip and the operator runs
  the command in their own terminal. The package manager remains
  the source of truth — the UI just removes the typing friction
  on the one platform/method where it's safe to do so. **This is
  the Podman Desktop pattern**, adapted: Podman runs
  `open -W <bundled.pkg>` (no terminal — the installer has its
  own GUI); we run brew headless (no terminal — the progress
  region is the GUI).
- **(γ) Server detects AND applies upgrades itself.** Add a
  `nimbus self-upgrade` command that rewrites the binary in place.
  The SPA gets a "Restart with v0.x.y" button.

---

## Decision

Adopt **(β+): server-side detection with stale-while-revalidate
caching, surfaced via `/api/system/version-info` to whichever UI is
rendering the SPA, plus a UI-launched upgrade flow that orchestrates
the operator's package manager command without ever rewriting the
binary in place.**

The desktop shell continues to use `electron-updater` for shell
self-updates (orthogonal cadence). It does **not** independently
poll GitHub for nimbus-binary releases — that signal arrives through
the same `/api/system/version-info` channel any browser user sees.

We do **not** ship `nimbus self-upgrade` (alternative γ — rejected
below). Upgrades are applied by whatever installed the binary in the
first place: Homebrew, apt, dnf, the install script, or
`cargo install --path`. The server constructs an `upgrade` plan
keyed on `current_exe()` introspection; the UI surfaces it as an
**Update** CTA that, when clicked:

- **In the desktop shell on macOS with `method: "brew"`**, spawns
  `brew upgrade --cask nimbus/tap/nimbus` in the background via
  `child_process.spawn` (no shell, no Terminal window). Progress
  streams into an in-app region (last few lines of stdout). On
  `exit 0`, the desktop — which already owns the nimbus child
  process per DS3 — SIGTERMs the old child and respawns the
  new binary; the existing `DisconnectedOverlay` covers the
  WebSocket gap. The renderer never executes shell; the main
  process maps a method tag (`'brew'`) to a hardcoded **argv
  array** from a whitelist.
- **In a browser, against a remote nimbus, or with a method that
  requires sudo TTY (`apt`, `dnf`, `install-script`) or has no
  canonical command (`source`, `unknown`)**, opens an anchored
  popover with the command in a code block and a copy-to-clipboard
  chip. The operator pastes into a terminal on the right host.

Same status-bar slot, additive desktop affordance for the one
platform/method where headless execution is safe. Symmetric
outcome (the operator's package manager does the work) with
asymmetric ergonomics (desktop+brew saves the paste).

### Endpoint contract (sketch — refined in UL1)

```
GET /api/system/version-info
→ 200 OK
{
  "current": "0.1.31",
  "latest": "0.1.41",
  "available": true,
  "url": "https://github.com/nimbus/nimbus/releases/tag/v0.1.41",
  "publishedAt": "2026-05-14T18:22:00Z",
  "host": "host.example.com",
  "checkStatus": "fresh" | "stale" | "never" | "disabled" | "error",
  "upgrade": {
    "method": "brew" | "apt" | "dnf" | "install-script" | "source" | "unknown",
    "command": "brew upgrade --cask nimbus/tap/nimbus",
    "needsSudo": false,
    "interactive": true,
    "fallbackUrl": "https://github.com/nimbus/nimbus#install"
  }
}
```

- `current` is the running binary's compiled-in `CARGO_PKG_VERSION`.
- `latest` is the cached GitHub Releases tag (without leading `v`),
  or `null` when no check has succeeded yet.
- `available` is `current < latest` by semver.
- `host` is the server's reported hostname — surfaces correctly in
  remote-nimbus topologies so the banner can read "Update Nimbus on
  `host.example.com`" rather than "Update Nimbus" (which would
  ambiguously refer to the operator's local machine).
- `checkStatus` distinguishes a fresh result (<24h), a stale result
  (≥24h, refresh inflight), a first-ever-load with no cached value,
  the user-opted-out state, and a recent fetch error. Lets the UI
  pick the right banner copy.
- `upgrade.method` is the detected install method, derived from
  `std::env::current_exe()` introspection (Homebrew prefix, system
  package paths, build-from-source markers). Determines which
  whitelisted command the desktop main process will run.
- `upgrade.command` is the canonical command for the detected
  install method, constructed locally by the server from a hardcoded
  template — never echoed from network input. Shown to the operator
  in the popover and (for `brew` on macOS desktop) auto-executed
  by the desktop's background runner using a separate argv
  whitelist keyed on the same `method` tag.
- `upgrade.interactive` is `true` when the command needs a TTY
  (sudo prompts on `apt`/`dnf`/install-script). The desktop
  shell uses this to decide whether to enable the background
  [Update] action (interactive=false on a localhost+brew host)
  versus falling back to the copy-only popover branch. Initial
  values: `brew` → `false`; `apt`/`dnf`/install-script → `true`;
  `source`/`unknown` → `true` (irrelevant — falls back to
  `fallbackUrl`).
- `upgrade.fallbackUrl` is shown when `method` is `"unknown"` or
  `"source"` and we don't have a clean one-line command.

### Refresh semantics

- **No work on server boot.** A fresh `nimbus start` does not contact
  GitHub. Air-gapped operators get the same posture they have today.
- **Lazy trigger.** First call to `/api/system/version-info` after
  startup checks whether the on-disk cache (`~/.config/nimbus/
  update-check.json`, XDG-respecting) is missing or ≥24h old. If so,
  it spawns an async refresh task and returns the cached value
  (or `checkStatus: "never"`) immediately. Subsequent calls see the
  refreshed value once the async task completes.
- **Cache TTL: 24h.** A fresh result is treated as authoritative for
  24h. Operator-facing UIs typically open the console daily; this
  cadence gives ~1 GitHub request per nimbus instance per day in
  the steady state. Well under GitHub's 60 unauthenticated requests
  per hour per IP.
- **Failure handling.** Network failures, GitHub 5xx, parse errors:
  log at INFO, record `checkStatus: "error"` with the last good cached
  value, retry the next time a request comes in after the TTL.

### Update-launch flow (the Podman Desktop pattern, adapted)

The UI mediates the upgrade through the operator's package manager;
the server never executes installer commands, and the renderer never
constructs shell strings. The flow is a **five-state machine** in
the SPA — same density as Podman Desktop's
`extensionApi.window.showInformationMessage` + `withProgress`
sequence — not a chain of clicks.

```
                                    [Update]
        available ───────────────────────▶ confirming ──[Cancel]──▶ available
            ▲                                   │
            │                                   ▼ [Update] (desktop+macOS+brew, localhost)
            │                              upgrading ─── runUpgrade exit 0 ──▶ upgraded
            │                                   │            + child restart      │
            │                                   │                                 │ 30s
            │ [Copy command]                    │ exit ≠ 0                        ▼
            │ (any other path)                  ▼                              available
            └─────────────────────────────── available                          (new version
                                              (CTA still there)                  shown)
```

- `available`: status-bar version slot reads `"update to <latest> →"`
  with `--accent` dot (clickable; aria-label: "Update Nimbus to
  <latest>"). On first detection, a sonner toast also fires with
  `[Update]` and `[Dismiss]` actions.
- `confirming`: shadcn `Popover` anchored to the status-bar slot.
  Body is just `upgrade.command` in a `CopyChip`. Buttons: `[Update]`
  (desktop + macOS + `method: "brew"` + localhost predicate) or
  `[Copy command]` (every other path), and `[Cancel]`. No body
  paragraph. Same density as Podman's
  `showInformationMessage('Do you want to update from X to Y?',
  'Yes', 'No')`.
- `upgrading`: status-bar slot reads `"Updating to <latest>…"` with
  `--starting` half-filled dot. For the desktop+brew path, the
  popover stays open showing the last 8 lines of `brew` stdout
  streamed in via `runUpgrade`'s ProgressEvent stream. Poll
  cadence accelerates to 2s for ≤10 minutes (matches Podman's
  `ProgressLocation.TASK_WIDGET` immediacy). For the copy-only
  path, `upgrading` is entered the moment the user clicks
  `[Copy command]` — the SPA doesn't know when the operator
  finishes running it; the polling loop detects the new
  `current` and transitions to `upgraded` naturally.
- `upgraded`: status-bar slot reads `"<new-version>"` with a
  30-second `--success` dot, then returns to the normal CopyChip
  rendering.

Two additional surfaces:

- **OS notification toast** (desktop only): the main process fires
  one Electron `Notification` per `latest` on first detection,
  deduped via `~/.config/nimbus-desktop/notified-versions.json`.
  Clicking the toast brings the existing window forward. This is
  the second persistent reminder surface alongside the status-bar
  slot — even if the operator dismisses the sonner in-app toast,
  the OS notification preserves the announcement until they
  return to the console.
- **Remote-host gate**: when the server's `host` does not match
  `window.location.host` (and is not a localhost predicate —
  `null`, `localhost`, `127.0.0.1`, `::1`), the `[Update]`
  button is *not rendered* even with `window.nimbus` present;
  the popover shows `[Copy command]` only. Background-brew on
  the operator's local laptop would upgrade nimbus on the wrong
  machine.

**Security model (mirrors Podman Desktop's
`provider.registerUpdate({ update: () => … })`):**

- The renderer sends only a method tag (`'brew'`) over IPC. It
  never sends a shell string and never sends an argv array.
- The desktop main process maps the tag to a hardcoded **argv
  array** from a closed-set whitelist and spawns with
  `shell: false`. An unknown tag is rejected at the IPC
  boundary; no `sh -c` or `cmd.exe` is ever invoked.
- The server's `upgrade.command` is constructed locally from
  `current_exe()` introspection — never echoed from GitHub's
  response payload. A poisoned upstream cannot inject a malicious
  command, and the desktop's argv table is independent of the
  server response anyway.
- The user sees exactly what is about to run via the popover
  showing `upgrade.command` *before* the click, and the
  streaming progress region *during* the run. Cancellable in v1
  by closing the popover (SIGTERMs the runner via the desktop's
  cleanup hook).
- No sudo is escalated by the desktop itself — only `brew`
  (which doesn't need sudo) runs in-process. Methods that
  need root (`apt`/`dnf`/install-script) fall back to copy-only
  where the user's own terminal handles the sudo prompt.

**Post-upgrade version sync:**

Because the desktop process spawned the nimbus child (DS3), it
owns the lifecycle. On `runUpgrade('brew')` exit code 0:

1. Re-resolve the binary from PATH.
2. SIGTERM the current nimbus child; wait ≤5s; SIGKILL on timeout.
3. Spawn the new binary; await readiness probe.
4. Emit `{ kind: 'restarted', newVersion }` to the renderer.

The renderer's `useStaleness` hook receives `restarted` and
transitions to `upgraded`. The new process can only ever report
the new version — there is no separate version-sync signal
because the desktop is the only component that could have
caused the binary change in the first place. This is the
"robust sync solution naturally based on architecture"
property of (β+): version state and process state are the
same state.

### Privacy posture

- Disabled by `NIMBUS_DISABLE_UPDATE_CHECK=1` env var. When set, the
  background task is never spawned and the endpoint returns
  `checkStatus: "disabled"` with `latest: null`. The SPA renders
  no banner.
- The check sends only an HTTP GET to `api.github.com`; no
  identifying headers beyond the standard reqwest User-Agent
  (`nimbus/<version>`). GitHub logs the requesting IP — the same
  exposure as a manual `curl api.github.com/...`.
- The `nimbus/nimbus` README states that nimbus is "designed from
  day one to be the thing you actually deploy — on your own hardware,
  air-gapped if needed, with no telemetry." The update check is **not
  telemetry** (no data flows outward; only the GitHub Releases listing
  flows inward), but air-gapped operators still need a clean off
  switch. `NIMBUS_DISABLE_UPDATE_CHECK=1` is that switch and must be
  honored without prompting.

---

## Real-world analogs

Surveyed locally against `~/src/github.com/podman-desktop/podman-desktop`
and the wider Electron-operator-console landscape.

| App | Detection | Application | Why that shape | Maps to nimbus how |
|---|---|---|---|---|
| **Docker Desktop** | desktop checks for desktop updates | desktop self-applies; engine version locked to desktop | engine + CLI are private impl details of the GUI's VM | wrong shape — we want a standalone CLI |
| **Podman Desktop** | desktop checks for desktop *and* podman engine | persistent `Update to <ver>` button on provider tile (`ProviderUpdateButton.svelte:59`) → `open -W <bundled-pkg>` spawns macOS Installer.app (no terminal, admin prompt is the installer's own GUI) → `withProgress({ location: TASK_WIDGET })` for non-installer tasks | podman is a standalone product; desktop is one of many ways to drive it | **canonical analog** — same shape (Electron shell + standalone CLI). We adopt: the persistent CTA, no-terminal-visible UX (achieved via `child_process.spawn` of brew since we don't bundle `.pkg`), and the renderer-passes-tag / main-maps-to-argv security model |
| **Tailscale** | app checks GitHub for app+daemon | app self-applies, restarts daemon | daemon is small + tightly coupled to GUI's API | wrong shape — our CLI is not a GUI's private daemon |
| **GitHub Desktop** | app checks for app+git | app self-applies | embedded git is a private impl detail | wrong shape |
| **gh CLI** | gh itself checks api.github.com, caches, prints banner on stale | brew/apt/manual — user runs the upgrade | tool owns its own staleness story | **detection analog** — server-side check + banner is exactly what we want on the nimbus binary, and exactly what (β-)/(β+) implement |

The **Podman Desktop** code path is canonical for this class of app
(operator console wrapping a separately-installed CLI). Key files in
`extensions/podman/packages/extension/src/`:

- `installer/podman-install.ts` — `checkForUpdate(installed)`,
  `performUpdate(provider, installed)`. Confirmation dialog
  (`extensionApi.window.showInformationMessage('Do you want to update
  to Y?')`) gates the actual install call.
- `installer/mac-os-installer.ts` — `update(): Promise<boolean>`
  delegates to `install()`, which resolves the bundled `.pkg` from
  the assets folder and runs `processAPI.exec('open', [pkgToInstall,
  '-W'])` to launch macOS Installer.app. Success is detected by
  checking `fs.existsSync('/opt/podman/bin/podman')`.
- `extension.ts:1014` — `registerUpdatesIfAny()` binds the
  `update: () => Promise<void>` callback into the provider tile via
  `provider.registerUpdate({ version, update, preflightChecks })`.
  The renderer (Svelte) reads provider state and renders the CTA
  without ever knowing the install method.

**What we steal (β+):**

- **The persistent CTA pattern.** Podman's `Update to <version>`
  label on the provider tile is the ambient-persistent reminder
  surface; we replicate it via the status-bar version slot, which
  renders on every route. Plus the Settings → Server "Updates"
  row as a second persistent surface.
- **Confirmation gating.** The CTA click opens an anchored popover
  that shows the command about to run; nothing happens until the
  operator confirms. Same density as Podman's
  `showInformationMessage('Do you want to update to Y?', 'Yes', 'No')`
  — one row of content, two buttons, no body paragraph.
- **No terminal visible.** Podman achieves this via `.pkg` +
  Installer.app GUI; we achieve it via `child_process.spawn` of
  brew with stdout streamed into an in-app progress region. Same
  user observable: "click, watch progress in-app, done."
- **Whitelisted action mapping.** The renderer passes a method
  tag; the main process maps to a hardcoded argv array
  (not a shell string). `shell: false` on the spawn.
- **Background runner + post-exit lifecycle hook.** Podman's
  installer runs without blocking the UI; on success it
  refreshes the provider state. Ours runs brew without blocking
  the UI; on `exit 0` it restarts the nimbus child so version
  state syncs naturally.

**What we deliberately don't steal (yet):**

- **Bundling installer artifacts inside the desktop app.** Podman
  Desktop ships `podman-installer-macos-${arch}-v${ver}.pkg` inside
  its own .app. We don't currently produce nimbus `.pkg`/`.msi`/
  `.deb`/`.rpm` artifacts; we hand off to brew/apt/dnf instead. If
  the user audience grows beyond brew users, building our own pkg
  artifacts and bundling them in `nimbus-desktop` is the obvious
  next step. Tracked as a deferred follow-on, not in this decision.
- **Headless run of `apt`/`dnf`/install-script from the main
  process.** Unlike brew, these require interactive sudo, which
  has no TTY when spawned from Electron. v1 routes them through
  the copy-only popover branch where the user's own terminal
  drives the sudo prompt. Headless versions await a polkit /
  Authorization Services integration that we don't yet own.

---

## Consequences

### Positive

- **One code path serves both UIs.** Browser users and desktop users
  see the same staleness signal, with the same copy and the same
  dismissal behavior, because both render the SPA banner sourced
  from the same endpoint.
- **Survives remote-nimbus topologies.** The shape works equally well
  whether the operator is on `localhost` or has tunneled into a
  remote nimbus instance. The server is authoritative for its own
  staleness.
- **Cache lives with the server, not the client.** One GitHub API
  call per nimbus instance per day, regardless of how many UI tabs
  the operator opens. No risk of rate-limiting at scale.
- **No new desktop-specific update-checker.** The desktop shell
  already has `electron-updater` for itself; adding a second polling
  loop for the binary would be near-duplicate code in TypeScript when
  the same logic in Rust serves both UIs cleanly.

### Negative

- **First-load latency on a cold cache.** The first operator who
  opens the UI after a fresh install sees `checkStatus: "never"` and
  no banner; the banner appears on the next page load (typically
  seconds later when the async refresh completes). The SPA must
  poll or re-render after navigation rather than only on initial
  mount. Acceptable.
- **One-platform parity.** Only desktop + macOS + `method: "brew"`
  + localhost gets the one-click background runner; every other
  combination falls back to copy-only. That's a deliberate
  asymmetry — brew is the only method that runs sudoless,
  produces structured stdout we can stream, and lets us SIGTERM
  + respawn the nimbus child cleanly. Expanding the matrix waits
  on (a) a `.pkg`/`.msi`/`.deb`/`.rpm` artifact track from the
  distribution plan, or (b) a polkit/Authorization Services
  surface for sudoless apt/dnf. Both deferred.
- **Brew runs without operator gating mid-flight.** Once the
  user clicks `[Update]` in the popover, the background brew
  invocation starts immediately. Cancellation is via the
  popover's close button (SIGTERMs the runner). This matches
  Podman's `open -W <pkg>` semantics (Installer.app starts on
  the click; the user cancels via the installer GUI), but
  differs from a copy-paste flow where the user retains the
  pause-before-Return moment. Acceptable: the popover shows the
  exact command before the click, and the progress region shows
  brew's output during the run.
- **No `nimbus self-upgrade`.** We do not silently `process.exec`
  brew/apt/dnf out of view from a CLI flag; nor does the server
  rewrite its own binary (see "Rejected alternatives → (γ)").
  Upgrades are user-initiated from a UI surface, with the
  package manager as the source of truth.
- **GitHub Releases dependency.** If GitHub is down or the
  repository changes name, the check fails — degrades to "no banner
  shown" rather than blocking startup. Acceptable; matches
  electron-updater's own posture.

---

## Rejected alternatives

### (α) Each client pings GitHub independently

- **Browser users get nothing.** Same-origin restrictions and CORS
  on `api.github.com` make a browser-side fetch awkward; even if
  proxied, every page load by every operator counts against the
  60/hr unauthenticated rate limit.
- **Desktop-side polling is duplicate work.** A second
  implementation in TypeScript when Rust already needs to know the
  running binary's version for the `--version` flag and for the
  endpoint's `current` field anyway.
- **Asymmetric UX.** Two operators on the same nimbus instance —
  one in a browser, one in the desktop — would see different
  banners. Hard to support, hard to document.

### (γ) Server detects AND applies the upgrade

This is the alternative we still reject in favor of (β+). The
distinction is crucial: (β+) lets the UI **launch** the operator's
package manager command; (γ) would have the server **be** the
package manager. Two different things.

- **Self-rewriting binaries are a substantial security surface.**
  Signature verification, atomic swap, rollback on failure,
  partial-write recovery — all need to be correct. Significant
  engineering for marginal UX gain over "type one brew command."
- **Fights the package manager.** A `nimbus self-upgrade` succeeds,
  the binary becomes 0.1.41, but Homebrew's manifest still shows
  0.1.31 installed. Now `brew upgrade` does nothing surprising
  because brew thinks it has the current version. Confusing state
  for the operator. (β+) sidesteps this entirely — brew runs the
  upgrade, brew's manifest stays in sync.
- **Doesn't generalize cleanly across install methods.** A user who
  installed via `cargo install` doesn't want `nimbus self-upgrade`
  to pull a release binary; a user who installed via apt wants
  `apt upgrade`; a user who built from source wants `git pull`. The
  endpoint's `upgrade.command` field surfaces the right command for
  each; a one-size-fits-all self-upgrade would be wrong for at
  least two of those four cases.
- **Hides what's happening from the operator.** (β+) shows the
  command before running it; the user can read, edit, cancel.
  A self-upgrade button is opaque — the operator has to trust that
  the server did the right thing, with no visibility into the
  fetch/verify/swap path.

### (δ) Push notifications via a Nimbus-operated update service

- **Operational overhead.** Running a notification service ourselves
  to push staleness signals to running nimbus instances would
  require maintaining the service, its TLS posture, its auth model,
  and a mechanism for nimbus instances to subscribe.
- **GitHub Releases is already the canonical source.** Re-publishing
  that signal through our own infrastructure adds no value over a
  direct GitHub Releases poll.
- **Air-gapped pessimum.** Push from a Nimbus-operated service is
  strictly worse than pull-from-GitHub for the air-gapped operator
  audience.

---

## Open questions

- **Should the endpoint distinguish patch / minor / major staleness
  in its response?** A patch-version-behind operator may want a less
  intrusive banner than a major-version-behind operator. Easy
  follow-up; not blocking initial landing.
- **Pre-release filter.** Should the check ignore tags like
  `v0.2.0-rc.1` or surface them with a separate `prerelease: true`
  flag? Initial implementation pins to `releases/latest` which
  GitHub already filters to stable releases, so this is naturally
  excluded; revisit when we cut a real RC.
- **Should the desktop *also* poll independently as a fallback** for
  the case where the desktop shell is open against a nimbus that's
  too old to expose the endpoint? At the first ship of UL1 this is
  N/A (no shipped nimbus has the endpoint); after UL1 lands the
  fallback would only matter for operators downgrading nimbus, which
  is rare enough to defer.
