# Updates

Nimbus ships as two independent binaries on independent release cadences:

- The **`nimbus` CLI** (the server, runtime, and embedded SPA assets — one
  artifact, packaged via Homebrew, apt, dnf, an install script, or built from
  source).
- The **`nimbus-desktop` shell** (a signed/notarized Electron app that wraps
  the operator console UI and spawns the CLI as a child process; ships as a
  separate Homebrew Cask + GitHub release).

This page is the single canonical doc for how each updates, what the
staleness indicators in the UI mean, and how to opt out for air-gapped
operation.

## How the `nimbus` CLI updates

Nimbus does not auto-upgrade itself. The recommended path depends on how it
was installed — the server detects this at startup (see
`crates/nimbus-server/src/system/install_method.rs`) and reports it back to
the SPA via `/api/system/version-info`, so the **Update button in the SPA
suggests the right command for your host**.

| Install method | Upgrade command | Detection signal |
| --- | --- | --- |
| Homebrew (macOS, Linux) | `brew upgrade nimbus/tap/nimbus` | binary lives under `/opt/homebrew/Cellar/`, `/usr/local/Cellar/`, or `/home/linuxbrew/.linuxbrew/Cellar/` |
| apt (Debian / Ubuntu, future) | `sudo apt update && sudo apt install --only-upgrade nimbus` | `dpkg -S` resolves the binary to an apt-managed package |
| dnf (Fedora / RHEL, future) | `sudo dnf upgrade nimbus` | `rpm -qf` resolves the binary to a dnf-managed package |
| Install script | `curl -fsSL https://nimbus.dev/install.sh \| sh` | the install script wrote a marker at `~/.local/share/nimbus/installed-by-script` |
| Build from source | `cd nimbus && git pull && cargo install --path crates/nimbus-bin` | nothing else matched |

If detection cannot decide, the SPA falls back to `Copy command` against
the build-from-source guidance.

### Why no auto-upgrade

Operators run nimbus in mixed environments — local laptops, CI runners,
fleet boxes, air-gapped hosts. Auto-upgrading a running server can change
behavior mid-deploy. Following the host's package manager (or the
operator's own pipeline) keeps that decision where the operator already
expects to make it.

## How the desktop shell updates

The desktop shell uses [`electron-updater`](https://www.electron.build/auto-update)
under `nimbus/desktop`. The shell polls the
[nimbus-desktop release feed](https://github.com/nimbus/desktop/releases) on
launch and every six hours, downloads the next version in the background,
and prompts to install on next quit. The shell upgrade does **not** touch
the `nimbus` CLI — you can have a brand-new shell and a stale CLI, or vice
versa, and both are surfaced separately in the UI.

The full distribution + release runbook lives in `nimbus/desktop` itself:
[`nimbus/desktop`](https://github.com/nimbus/desktop). The shell's own
update controls (check now, restart and install) live in the desktop tray
menu.

## What the staleness banner in the SPA means

Every route in the SPA shows a **version slot in the status bar**
(`packages/nimbus-ui/src/shell/status-bar.tsx`). Its visible state is
driven by `/api/system/version-info`:

| Visible state | Meaning | Slot rendering |
| --- | --- | --- |
| **fresh** | The server checked the latest release within the last ~6 hours; nothing newer is out. | The current version as a `CopyChip` (copyable). |
| **stale-but-cached** | A newer release is out. The server detected this via its background check. | `UpgradeDot` + a `Vx.y.z available` trigger that opens the Update popover. |
| **first-load-empty** | The server has not finished its first check yet (`checkStatus: "never"`). | The current version as a `CopyChip`; the trigger is hidden until a check completes. |
| **check-failed** | The last check failed (network, parse, 5xx). The server returns the last known good cached value with `checkStatus: "error"`. | Falls back to the previous visible state; the failure is logged at INFO server-side, not surfaced to the operator. |

Once per new `latest` version, a `sonner` toast announces the new release
with `[Update]` / `[Dismiss]` actions. Dismissal persists in
`localStorage["nimbus-ui:staleness-dismissed-version"]` so the same version
does not nag again.

In `Settings → Server`, the `Updates` row consumes the same context and
shows the current version + most recent latest + the command to run.

## What the Update button does

The Update popover anchors to the version slot and renders either an
`[Update]` button or a `[Copy command]` button — never both.
This is the (β+) pattern from
[`docs/plans/archive/update-lifecycle-plan.md`](../plans/archive/update-lifecycle-plan.md)'s
decision 001, modeled on
[Podman Desktop](https://podman-desktop.io/)'s `ProviderUpdateButton`:
the operator always confirms before any command runs.

- **`[Update]` (desktop shell on macOS + Homebrew):** clicking spawns
  `brew upgrade --cask nimbus/tap/nimbus` via `child_process.spawn` from
  the desktop main process with `shell: false` and a sanitized `PATH`. No
  Terminal window opens. stdout/stderr lines stream into the
  status-bar slot (`Updating…`) and the popover. On `exit 0`, the desktop
  SIGTERMs the running nimbus child, respawns it from the new on-disk
  binary, waits for the readiness probe to succeed, and emits a
  `restarted` event so the SPA flips to `upgraded`. The
  `DisconnectedOverlay` covers the WebSocket gap during the restart.
- **`[Copy command]` (everything else):** browsers, remote operators
  (where `upgrade.host` does not match the local host), apt / dnf /
  install-script methods (which require an interactive `sudo` TTY — not
  available in a headless spawn), and the `source` method. Clicking
  copies the suggested upgrade command to the clipboard; the operator
  pastes into their own terminal.

### Real-world analogs

This is the same control loop as:

- **Podman Desktop** — `ProviderUpdateButton.svelte` runs the package
  manager headless, streams output via `withProgress`, no Terminal.
- **Docker Desktop** — "An update is available" button installs and
  restarts the daemon in the background.
- **VS Code** — `Restart to update` action runs the installer silently
  on quit.

## Air-gapped operation

Set `NIMBUS_DISABLE_UPDATE_CHECK=1` in the environment of the nimbus
process. With this set:

- `/api/system/version-info` returns `{checkStatus: "disabled", latest:
  null, available: false, ...}` immediately and never touches the
  network.
- `~/.config/nimbus/update-check.json` is never written.
- The SPA renders the version slot as a plain `CopyChip` (no upgrade
  dot, no popover trigger).
- The sonner staleness toast never fires.

The desktop shell's `electron-updater` is a separate setting; to disable
the shell's check, follow the off-switch documented in `nimbus/desktop`.

## FAQ

### Why does the banner show a `brew` command on my apt machine?

It does not — if you installed via apt, `install_method::detect` resolves
the binary to a dpkg-managed package and `upgrade.method` becomes `"apt"`.
If the banner is suggesting `brew` on an apt host, detection failed; check
the server logs for `version_check` INFO lines and file an issue with the
path of the running binary.

### What if I'm offline?

Two options:

1. **Permanently:** set `NIMBUS_DISABLE_UPDATE_CHECK=1` (see above). The
   check never runs.
2. **Transiently:** the server tolerates `check-failed` automatically.
   The last cached value (or `checkStatus: "never"` on a cold start) is
   returned with no impact on serving traffic. The next successful
   background check refreshes the cache.

### How often does the check run?

The server polls the GitHub release feed roughly every six hours, served
from cache between polls. p99 latency for `GET /api/system/version-info`
is sub-5ms. The desktop shell's staleness notifier polls every five
minutes against the local server.

### Will the same notification fire twice?

No. The desktop's OS notification dedupes via
`notified-versions.json` under `userData/`; the SPA toast dedupes via
`localStorage["nimbus-ui:staleness-dismissed-version"]`. Both reset when a
genuinely new `latest` appears.
