---
title: Desktop install
description: Install Nimbus Desktop — the native shell for the operator console — via Homebrew Cask on macOS, or AppImage, deb, rpm, and NSIS installers from GitHub Releases.
sidebar:
  label: Desktop install
  order: 13
---

Nimbus Desktop (`nimbus-desktop`) is the native shell for the Nimbus
operator console — the same `/ui/` console the server embeds, wrapped in
a desktop window with system tray integration and automatic updates. It
ships from its own repository,
[`nimbus/desktop`](https://github.com/nimbus/desktop), on a release
cadence independent of the `nimbus` CLI.

The shell does not bundle the server. Install the `nimbus` CLI first:

```bash
# macOS / Linux (Homebrew)
brew install --cask nimbus/tap/nimbus

# Linux (install script)
curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh
```

On launch the shell looks for a running `nimbus` server and connects to
it; if none is running, it spawns one in the background and waits for it
to become ready.

## macOS

Requires macOS 14 (Sonoma) or later.

### Homebrew Cask (recommended)

```bash
brew tap nimbus/tap
brew install --cask nimbus/tap/nimbus-desktop
```

The cask installs `nimbus-desktop.app` into `/Applications` from a
Developer ID-signed, notarized DMG — no quarantine prompt. The cask sets
`auto_updates`, so ongoing upgrades are handled by the app's built-in
updater rather than `brew upgrade`.

### Direct download

Download `nimbus-desktop-<version>-universal.dmg` from the
[latest release](https://github.com/nimbus/desktop/releases) and drag
`nimbus-desktop.app` into `/Applications`. The DMG is a universal build
for Apple Silicon and Intel. The release also publishes a ZIP variant
used internally by the auto-updater; prefer the DMG.

### Verify the signature

```bash
codesign --verify --deep --strict --verbose=2 /Applications/nimbus-desktop.app
spctl --assess --type execute --verbose=4 /Applications/nimbus-desktop.app
xcrun stapler validate /Applications/nimbus-desktop.app
```

Expect `valid on disk`, `satisfies its Designated Requirement`,
`source=Notarized Developer ID`, and `The validate action worked!`.

## Linux

Three x86_64 formats per release on the
[releases page](https://github.com/nimbus/desktop/releases). Pick the
one that matches your distribution.

### Debian / Ubuntu (.deb)

```bash
curl -LO https://github.com/nimbus/desktop/releases/latest/download/nimbus-desktop_<version>_amd64.deb
sudo apt install ./nimbus-desktop_<version>_amd64.deb
```

Installs under `/opt/nimbus-desktop/` with a launcher entry in the
Development category.

### Fedora / RHEL (.rpm)

```bash
curl -LO https://github.com/nimbus/desktop/releases/latest/download/nimbus-desktop-<version>.x86_64.rpm
sudo dnf install ./nimbus-desktop-<version>.x86_64.rpm
```

### Any distribution (AppImage)

```bash
curl -LO https://github.com/nimbus/desktop/releases/latest/download/nimbus-desktop-<version>-x86_64.AppImage
chmod +x nimbus-desktop-<version>-x86_64.AppImage
./nimbus-desktop-<version>-x86_64.AppImage
```

Linux artifacts are not code-signed; fetch them over TLS directly from
the GitHub Releases page.

## Windows

NSIS installers (`nimbus-desktop-Setup-<version>-x64.exe` and
`-arm64.exe`) are published on the
[releases page](https://github.com/nimbus/desktop/releases). They are
currently unsigned, so SmartScreen warns on first launch — downloading
directly from the GitHub release and accepting the prompt is the
supported path today. The installer defaults to a per-user install under
`%LOCALAPPDATA%\Programs\nimbus-desktop\`.

## Launch

```bash
# macOS
open -a nimbus-desktop

# Linux
nimbus-desktop
```

On Windows, launch `nimbus-desktop` from the Start menu. The window
opens to the operator console. If the shell reports that no server was
discovered, confirm the CLI is installed and starts cleanly:

```bash
nimbus --version
nimbus start
```

## Updates

The shell updates itself: it polls the GitHub release feed, downloads
new versions in the background, and installs on the next quit — never a
forced restart. This is independent of the `nimbus` CLI; see
[updates](/operators/updates/) for how the server binary updates and how
each surface reports its own version.

## Next steps

- [Self-host quickstart](/get-started/self-host/) — get a server running
  for the shell to connect to.
- [Updates](/operators/updates/) — CLI and shell update cadences.
