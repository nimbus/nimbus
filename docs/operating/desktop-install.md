# Installing Nimbus Desktop

Nimbus Desktop is the native shell for the Nimbus operator console — the
embedded `/ui/*` SPA wrapped in a signed, notarized, auto-updating Electron
window with system tray and menu integration. It connects to a running
`nimbus` server (started separately with `nimbus start` or `nimbus serve`)
through the same `server.json` discovery seam that `nimbus ui` uses, so it
picks up an existing instance automatically.

This page covers user-facing install paths. For the build pipeline that
produces these artifacts, see [`nimbus/desktop`](https://github.com/nimbus/desktop).

## macOS

### Homebrew Cask (recommended)

```bash
brew tap nimbus/tap
brew install --cask nimbus/tap/nimbus-desktop
```

The cask installs `nimbus-desktop.app` into `/Applications`, with no
quarantine prompt — the DMG it pulls from the GitHub Release is
Developer ID-signed, notarized by Apple, and stapled.

To upgrade later:

```bash
brew upgrade --cask nimbus-desktop
```

### Direct download

Download the universal DMG from the latest release on
[`nimbus/desktop`](https://github.com/nimbus/desktop/releases) and drag
`nimbus-desktop.app` into `/Applications`. The DMG works on both Apple
Silicon and Intel Macs.

The release page also exposes a ZIP variant; the auto-updater uses that
internally to apply background updates without re-downloading the full
DMG. End users should prefer the DMG.

### Verifying the signature

```bash
codesign --verify --deep --strict --verbose=2 /Applications/nimbus-desktop.app
spctl --assess --type execute --verbose=4 /Applications/nimbus-desktop.app
xcrun stapler validate /Applications/nimbus-desktop.app
```

Expected: `valid on disk`, `satisfies its Designated Requirement`,
`source=Notarized Developer ID`, `The validate action worked!`.

## Linux

Three formats are produced per release. Pick whichever matches your
distribution; the install script in
[`nimbus/install`](https://github.com/nimbus/install) (still active per
`docs/plans/install-script-plan.md`) will eventually pick the right one
automatically.

### Debian / Ubuntu (`.deb`)

```bash
curl -LO https://github.com/nimbus/desktop/releases/latest/download/nimbus-desktop_0.1.0_amd64.deb
sudo dpkg -i nimbus-desktop_0.1.0_amd64.deb
```

The package installs `nimbus-desktop` into `/opt/nimbus-desktop/` with a
launcher entry under `Development`.

### Fedora / RHEL (`.rpm`)

```bash
curl -LO https://github.com/nimbus/desktop/releases/latest/download/nimbus-desktop-0.1.0.x86_64.rpm
sudo rpm -i nimbus-desktop-0.1.0.x86_64.rpm
```

### Distro-agnostic (AppImage)

```bash
curl -LO https://github.com/nimbus/desktop/releases/latest/download/nimbus-desktop-0.1.0-x86_64.AppImage
chmod +x nimbus-desktop-0.1.0-x86_64.AppImage
./nimbus-desktop-0.1.0-x86_64.AppImage
```

Linux artifacts are unsigned by convention; integrity is established by
fetching from the GitHub Releases URL over TLS and (optionally)
verifying against the SHA256 sums published alongside each release.

## Windows

Native Windows installers (NSIS x64 and arm64) are produced by the
release pipeline but are currently shipped unsigned per the Nimbus
desktop project's `docs/decisions/002-windows-code-signing.md`
deferral. Windows users will see a SmartScreen warning on first launch
until that decision is activated. Until then, downloading the
installer directly from the GitHub Release page and accepting the
SmartScreen prompt is the supported path.

## After install

`nimbus-desktop` starts up looking for a running `nimbus` server. If
none is found, it prompts to spawn one — the same lifecycle the `nimbus
ui --ensure` CLI flow uses. The shell does not bundle `nimbus`; install
the CLI separately:

```bash
brew install nimbus/tap/nimbus            # macOS
curl -fsSL https://nimbus.dev/install | sh # Linux / macOS install script
```

See [`docs/operating/cli.md`](./cli.md) for the full CLI surface and
[`docs/architecture/sandbox/macos-machine-flow.md`](../architecture/sandbox/macos-machine-flow.md)
for the macOS-specific developer-machine architecture the shell
operates against.
