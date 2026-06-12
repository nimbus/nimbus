---
title: Update Nimbus
description: Update the nimbus binary safely — per-install-method upgrade commands, restart and verify steps, built-in staleness checks, and the air-gapped opt-out.
sidebar:
  label: Updates
  order: 7
---

Nimbus never auto-upgrades itself. Updating is always an explicit
operator action: replace the binary through the channel you installed
with, restart the service, verify. The server does detect how it was
installed and suggests the matching upgrade command in the operator
console, but nothing runs without you.

## 1. Before you update

- Back up your data — see [backup and restore](/operators/backup-restore/).
- Read the release notes on the
  [releases page](https://github.com/nimbus/nimbus/releases).
- Note your current version:

```bash
nimbus --version
```

## 2. Update the binary

Use the same channel you installed with.

### Homebrew (macOS and Linux)

```bash
brew upgrade --cask nimbus/tap/nimbus
```

### Install script

Re-run the installer; it replaces the binary in place at
`/usr/local/bin/nimbus`:

```bash
curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh
```

Pass `--version vX.Y.Z` after `sh -s --` to pin a specific release. On
Linux the script also refreshes the paired sandbox runtime components
(`nimbus-libkrun`, `nimbus-crun`) under `/usr/libexec/nimbus` so they
stay matched to the binary.

### Release archive

If you installed from a release archive, download the new one from the
[releases page](https://github.com/nimbus/nimbus/releases), verify it
against `checksums-sha256.txt`, and replace the binary on disk.

### Container image

Pull the new version and digest, then recreate the container — see
[container image](/operators/container-image/) for digest pinning. For a
Quadlet-managed node, regenerate the unit with the new image reference:

```bash
sudo nimbus node install --container \
  --image ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest> \
  --system --overwrite
sudo systemctl restart nimbus.service
```

### Built from source

Pull the new tag and rebuild — see
[build from source](https://github.com/nimbus/nimbus#build-from-source).

Debian/RPM package repositories are not published yet; the channels
above are the supported paths today.

## 3. Restart and verify

A running server keeps executing the old binary until restarted:

```bash
sudo systemctl restart nimbus.service
```

(Or stop and rerun your foreground `nimbus start`.) Then verify:

```bash
nimbus --version
curl -s http://127.0.0.1:8080/health
```

## How Nimbus tells you an update exists

The server checks the latest published release at most once every 24
hours, caches the result at `~/.config/nimbus/update-check.json`, and
serves it from cache between checks. A failed check (offline, rate
limit) is tolerated silently — the last cached value is used and serving
traffic is unaffected.

In the operator console (`/ui/`), the status bar shows the running
version; when a newer release is out, it offers the upgrade command
matched to your install method (Homebrew, install script, or
build-from-source) for you to confirm or copy — it never upgrades on its
own.

The same data is available from the admin API with your
[local admin token](/get-started/self-host/):

```bash
curl -s http://127.0.0.1:8080/api/system/version-info \
  -H "Authorization: Bearer $NIMBUS_TOKEN"
```

It returns the running version, the newest published version, when it
last checked, and the suggested upgrade command.

## Air-gapped operation

Set this in the environment of the `nimbus` process:

```bash
NIMBUS_DISABLE_UPDATE_CHECK=1
```

With the check disabled the server never contacts the network for
version information, never writes `~/.config/nimbus/update-check.json`,
and `/api/system/version-info` reports the check as disabled. The
console shows the running version with no upgrade prompt.

## The desktop shell updates separately

[Nimbus Desktop](/operators/desktop-install/) is a separate application
on its own release cadence with its own built-in updater. Updating the
shell does not touch the `nimbus` binary, and vice versa — each surfaces
its own version.

## Next steps

- [Backup and restore](/operators/backup-restore/) — take a backup
  before every upgrade.
- [Run Nimbus as a service](/operators/node-lifecycle/) — the service units that
  restart the new binary.
- [Troubleshooting](/operators/troubleshooting/) — if the server doesn't
  come back after an upgrade.
