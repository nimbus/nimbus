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
install it. Two adjacent problems compound this:

1. The desktop shell, on first launch with no `nimbus` on PATH, throws
   `NimbusBinaryNotFoundError` rather than guiding the user to install
   it.
2. There is no canonical place to land operator-facing docs that explain
   *how* nimbus updates — across brew, apt, dnf, install script, and
   build-from-source — relative to how the desktop shell self-updates.

This plan owns landing the staleness signal (UL1 + UL2), the missing-CLI
first-run experience (UL3), and the operator-facing documentation that
ties them together (UL4). It does **not** own self-upgrade tooling or
package-manager-detecting upgrade helpers — those are explicitly
rejected in the decision doc and parked as deferred follow-ons.

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
                │   └─ /api/system/        │
                │      version-info        │
                └────────────┬────────────┘
                             │
                ┌────────────▼────────────┐
                │   /ui/* SPA              │  ← UL2
                │   StalenessBanner        │
                │   (rendered the same way │
                │   in browser + desktop)  │
                └─────────────────────────┘

  Orthogonal: nimbus-desktop shell uses electron-updater for itself
  (already wired). Never polls api.github.com for the nimbus binary.
```

Three update axes, three independent owners:

| Axis | Detected by | Surfaced as | Applied by |
|---|---|---|---|
| **nimbus binary** stale | UL1 background task in nimbus | UL2 SPA banner (browser + desktop) | operator runs `brew/apt/dnf upgrade …` |
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
     "upgradeHint": "brew upgrade --cask nimbus/tap/nimbus",
     "checkStatus": "fresh"
   }
   ```
3. `upgradeHint` resolution: detect install method by inspecting
   `std::env::current_exe()`. If the path is under a Homebrew prefix
   (`/opt/homebrew/`, `/usr/local/`, `/home/linuxbrew/`), emit the brew
   command. If under `/usr/bin/` or `/usr/local/bin/` and the host has
   `dpkg`/`rpm` markers, emit the apt/dnf command. Otherwise emit
   `"See https://github.com/nimbus/nimbus#install"`.
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
- Integration test using `wiremock` (already a workspace dep): boot
  a `nimbus-server` against a wiremock GitHub Releases mock, hit
  `/api/system/version-info`, assert each `checkStatus` branch.
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
   - Renders a top banner for `available: true` with copy that names
     the version, links to the release URL, and shows the
     `upgradeHint` in a `<code>` block with a copy-to-clipboard chip.
   - Dismissible per-session via `localStorage["nimbus-ui:staleness-
     dismissed-version"]` keyed to the *latest* version — dismissing
     0.1.41 still surfaces 0.1.42 when it lands.
   - Re-renders without a full page reload when the underlying value
     changes (poll completes, version flips).
2. Banner mounted in the global shell (`packages/nimbus-ui/src/
   routes/__root.tsx` or equivalent — confirm name during
   implementation since DU3 may have renamed).
3. Storybook story (`staleness-banner.stories.tsx`) covering all
   five `checkStatus` branches × two themes = ten visual states for
   the curated Chromatic matrix.

**Contract bullets:**

- **Single banner.** No version-specific dismissal layering — one
  active banner at a time, dismissed per-latest-version.
- **A11y:** banner is `role="status"` with `aria-live="polite"` so
  screen readers announce it on first render but not on poll refreshes
  that don't change the visible content.
- **Theme-aware:** uses semantic state tokens from DU5's OKLCH
  palette (`--color-info` for the available-update banner). Passes
  axe-core AA in both themes per the DU5 verification standard.
- **No exit telemetry.** Dismissing the banner only writes
  localStorage; nothing flows to the server.

**Files touched:**

- `packages/nimbus-ui/src/components/staleness-banner.tsx` (new)
- `packages/nimbus-ui/src/components/staleness-banner.stories.tsx`
  (new)
- `packages/nimbus-ui/src/routes/__root.tsx` (mount point)
- `packages/nimbus-ui/src/api/system.ts` (typed fetch wrapper for
  `/api/system/version-info`, new)

**Completion gate:**

- Vitest unit tests cover the dismissal-keying logic, the
  five-branch render matrix, and the `latest`-version-flips-re-shows
  case.
- Storybook + Chromatic: ten stories pass visual regression.
- axe-core run against the embedded build: 0 violations in dark and
  light themes with the banner visible (matches the DU5/DU6/DU6.5/DU7
  bar).
- Manual end-to-end against a freshly-cut nimbus that exposes UL1: open
  `/ui/` in Chromium → banner appears with `upgradeHint`; dismiss →
  reload → banner stays dismissed; hand-edit `update-check.json` to
  bump `latest` → banner re-appears.

### UL3 — Desktop shell: first-run "CLI not found" experience

**Goal:** a user who installs `nimbus-desktop` without the `nimbus` CLI
sees a setup card guiding them to install it — never a raw
`NimbusBinaryNotFoundError`.

**Owner:** `nimbus/desktop` (separate repo, separate release cadence).

**Deliverables:**

1. New `src/renderer/setup/CliNotFoundCard.tsx` (or equivalent — the
   desktop shell is plain Electron + a static renderer page, not a
   React app; verify during implementation against the current shell
   structure).
2. Wire the existing `resolveNimbusExecutable` failure path
   (`src/main/server.ts:200` throwing `NimbusBinaryNotFoundError`) to
   instead post a `cli-not-found` IPC message to the renderer, which
   swaps the window contents to the setup card.
3. The card surfaces:
   - **macOS**: button "Install with Homebrew" → opens Terminal with
     `brew install nimbus/tap/nimbus` pre-typed via `osascript`.
   - **Linux**: links to `https://github.com/nimbus/nimbus#install`.
   - **Windows**: links to the direct-download .zip and the install
     docs.
   - Common: a "Retry" button that re-runs `resolveNimbusExecutable`
     and proceeds to the normal flow once a binary is found.
4. After the user installs, the Retry path picks up the new binary on
   PATH without requiring a full app restart.

**Contract bullets:**

- **No bundled installer.** The card never downloads the nimbus
  binary itself. It hands off to a package manager (where one exists)
  or the install docs. This preserves the decision in the desktop
  README — "shell does not bundle nimbus."
- **No automatic install.** The user clicks; the shell never executes
  package-manager commands on the user's behalf without an explicit
  click.
- **Retry, don't restart.** The shell remains usable across the
  install — a successful retry must not require quitting and
  relaunching.

**Files touched (in nimbus/desktop):**

- `src/main/server.ts` (signal cli-not-found instead of throwing into
  the void)
- `src/main/ipc.ts` (new IPC channel `cli-not-found` / `cli-retry`)
- `src/renderer/setup/CliNotFoundCard.tsx` (new — verify file
  layout)
- `src/renderer/setup/CliNotFoundCard.spec.ts` (new, vitest)
- `tests/e2e/cli-not-found.spec.ts` (new, packaged-shell Playwright)

**Completion gate:**

- Vitest unit tests cover the IPC handshake, retry semantics, and
  per-platform button targets.
- Packaged-shell Playwright E2E: launch with `PATH=/empty`, assert
  setup card renders; add a fake nimbus to PATH; click Retry; assert
  the card disappears and the normal `/ui/` window opens.
- Manual on macOS + Linux: install only the desktop cask, launch,
  observe setup card; install the CLI cask, click Retry, observe
  normal flow.

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

- **`nimbus self-upgrade`.** Rejected in
  [decision 001](../decisions/001-update-staleness-detection.md) →
  rejected alternative (γ). Self-rewriting binaries fight the package
  manager and add security surface for marginal UX gain.
- **Package-manager-detecting `nimbus upgrade` wrapper.** Worth
  considering as a follow-on once we've shipped enough versions to
  feel the manual `brew upgrade` friction. Not in this plan — leave
  it as a deferred design note in `docs/plans/research/` if the need
  surfaces.
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
