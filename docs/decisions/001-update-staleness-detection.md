# 001 — Update staleness detection: server pings, both UIs render

- **Status:** proposed
- **Date:** 2026-05-16
- **Decision owner:** `nimbus/nimbus` maintainers
- **Parent plan:** `docs/plans/update-lifecycle-plan.md`

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

The question is: **who detects when the nimbus binary is behind, and
how does that signal reach both UIs?**

Three viable shapes:

- **(α) Each client pings GitHub independently.** The desktop shell
  polls `api.github.com/repos/nimbus/nimbus/releases/latest`,
  compares to `nimbus --version`, shows a toast. Browser users get
  nothing.
- **(β) The nimbus server pings GitHub; both UIs render the result.**
  Nimbus runs a stale-while-revalidate background check, exposes the
  result on a `/api/system/version-info` endpoint, and the SPA
  renders a banner that both browser and desktop users see.
- **(γ) Server pings AND applies upgrades.** Add a
  `nimbus self-upgrade` command that rewrites the binary in place.
  The SPA gets a "Restart with v0.x.y" button.

---

## Decision

Adopt **(β): server-side detection with stale-while-revalidate
caching, surfaced via `/api/system/version-info` to whichever UI is
rendering the SPA.**

The desktop shell continues to use `electron-updater` for shell
self-updates (orthogonal cadence). It does **not** independently
poll GitHub for nimbus-binary releases — that signal arrives through
the same `/api/system/version-info` channel any browser user sees.

We do **not** ship `nimbus self-upgrade`. Upgrades are applied by
whatever installed the binary in the first place: Homebrew, apt, dnf,
the install script, or `cargo install --path`. The endpoint exposes
an `upgradeHint` string for the most likely path (e.g.
`brew upgrade --cask nimbus/tap/nimbus` when we can detect a brew
prefix install) but the human runs it.

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
  "upgradeHint": "brew upgrade --cask nimbus/tap/nimbus",
  "checkStatus": "fresh" | "stale" | "never" | "disabled" | "error"
}
```

- `current` is the running binary's compiled-in `CARGO_PKG_VERSION`.
- `latest` is the cached GitHub Releases tag (without leading `v`),
  or `null` when no check has succeeded yet.
- `available` is `current < latest` by semver.
- `checkStatus` distinguishes a fresh result (<24h), a stale result
  (≥24h, refresh inflight), a first-ever-load with no cached value,
  the user-opted-out state, and a recent fetch error. Lets the UI
  pick the right banner copy.

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
- **Operator must run the upgrade themselves.** No "click here to
  upgrade" button. The banner shows the recommended command but the
  human types it. This is intentional (see "Rejected alternatives →
  (γ)") and matches Homebrew / apt / dnf conventions where the
  package manager is the source of truth, not the running daemon.
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

- **Self-rewriting binaries are a substantial security surface.**
  Signature verification, atomic swap, rollback on failure,
  partial-write recovery — all need to be correct. Significant
  engineering for marginal UX gain over "type one brew command."
- **Fights the package manager.** A `nimbus self-upgrade` succeeds,
  the binary becomes 0.1.41, but Homebrew's manifest still shows
  0.1.31 installed. Now `brew upgrade` does nothing surprising
  because brew thinks it has the current version. Confusing state
  for the operator.
- **Doesn't generalize cleanly across install methods.** A user who
  installed via `cargo install` doesn't want `nimbus self-upgrade`
  to pull a release binary; a user who installed via apt wants
  `apt upgrade`; a user who built from source wants `git pull`. The
  endpoint's `upgradeHint` field surfaces the right command for
  each; a one-size-fits-all self-upgrade would be wrong for at
  least two of those four cases.

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
