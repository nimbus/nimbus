# Plan: Install Script — `curl | sh` Quick Start for All Platforms

Canonical execution plan for the neovex install script (distribution
Channel 1). The script handles platform detection, dependency installation,
binary download, and post-install verification on Linux (Debian/Ubuntu,
Fedora/RHEL) and macOS (Apple Silicon).

---

## Status

- **Status:** `todo`
- **Primary owner:** this plan
- **Parent plan:** `docs/plans/distribution-plan.md` (Channel 1, Phase D1)
- **Readiness:** implementation-ready after the 2026-04-18 contract refresh in
  this plan; I1 can start immediately
- **Hard deps:** initial v1 implementation now has its external release inputs:
  at least one `v*` Neovex release tag and at least one
  `agentstation/neovex-crun` release tag already exist
- **Related CI:**
  - `.github/workflows/release.yml` — neovex binary builds (linux x86_64,
    linux arm64, darwin arm64, windows x86_64) on `v*` tags, bundles
    `libexec/gvproxy` into the darwin archive, publishes checksums, dispatches
    machine-os, and updates the Homebrew cask
  - `agentstation/neovex-crun` release workflow — publishes
    `neovex-crun-linux-amd64` and `neovex-crun-linux-arm64`
  - `agentstation/neovex-machine-os/.github/workflows/build.yml` — machine
    guest image build/publish lane, called from the neovex `v*` release
    workflow and available for standalone image-repo `v*` tags

## Control Plan Rules

Source of truth:
1. this plan's `Phase Status Ledger` and `Execution Log`
2. the install script itself (`scripts/install.sh`)
3. the verification helper (`scripts/verify-install.sh`)

---

## Target UX

```bash
# One-line install (stable)
curl -fsSL https://neovex.dev/install.sh | sh

# Pinned version
# Linux direct-binary path in the initial cut; macOS initially follows the
# latest Homebrew cask rather than supporting arbitrary historical cask pins.
curl -fsSL https://neovex.dev/install.sh | sh -s -- --version v0.1.14

# Dry run (print what would happen)
curl -fsSL https://neovex.dev/install.sh | sh -s -- --dry-run

# Uninstall
curl -fsSL https://neovex.dev/install.sh | sh -s -- --uninstall

# Direct from GitHub (before neovex.dev is live)
curl -fsSL https://raw.githubusercontent.com/agentstation/neovex/main/scripts/install.sh | sh
```

---

## Channel 1 Contract

The install script is a bootstrapper, not a single artifact installer.

- On Linux in the initial cut, it installs distro dependencies via `apt` or
  `dnf`, then installs the released `neovex` and `neovex-crun` binaries
  directly from GitHub Releases with checksum verification.
- On macOS in the initial cut, it installs or upgrades the published
  `agentstation/tap/neovex` Homebrew cask. That cask owns `krunkit` as an
  explicit dependency and ships the bundled `libexec/gvproxy` helper.
- Once the public apt/COPR channels are fully proved, the Linux branch of the
  script can switch from direct-release installs to package-repo installs
  without changing the user-facing `curl | sh` entrypoint.

Do not design the macOS branch as a manual `curl`/untar copy into
`/usr/local/bin`. That would diverge from the shipped cask path and strand the
bundled `libexec/gvproxy` helper unless the script recreated the same prefix
layout itself.

---

## What Gets Installed

### Linux

| Component | Source | Install path |
|-----------|--------|-------------|
| `neovex` | GitHub Release `v*` | `/usr/local/bin/neovex` |
| `neovex-crun` | `agentstation/neovex-crun` GitHub Release `v*` | `/usr/libexec/neovex/crun` |
| System deps | OS package repos | System paths (via apt/dnf) |

**System dependencies installed via package manager:**

| Package | Debian/Ubuntu | Fedora/RHEL |
|---------|--------------|-------------|
| conmon | `apt-get install conmon` | `dnf install conmon` |
| buildah | `apt-get install buildah` | `dnf install buildah` |
| catatonit | `apt-get install catatonit` | `dnf install catatonit` |
| passt | `apt-get install passt` | `dnf install passt` |
| uidmap | `apt-get install uidmap` | `dnf install shadow-utils` |
| fuse-overlayfs | `apt-get install fuse-overlayfs` | `dnf install fuse-overlayfs` |
| libkrun | **Not in repos** (see below) | `dnf install libkrun` |
| libkrunfw | **Not in repos** (see below) | `dnf install libkrunfw` |

### macOS (Apple Silicon only)

| Component | Source | Install path |
|-----------|--------|-------------|
| `neovex` | Homebrew cask `agentstation/tap/neovex` | Homebrew Caskroom + `$(brew --prefix)/bin/neovex` symlink |
| `gvproxy` | Bundled inside the neovex darwin cask/archive | `$(brew --prefix)/Caskroom/neovex/<version>/libexec/gvproxy` |
| krunkit | Homebrew (`slp/krunkit/krunkit`) via cask dependency | Homebrew prefix |

No crun, conmon, buildah, or other Linux deps — everything runs inside the
machine VM guest.

---

## Script Design

### Language and compatibility

POSIX `sh` — no bashisms. Following the conventions of:
- [rustup install script](https://github.com/rust-lang/rustup/blob/master/rustup-init.sh)
- [Docker install script](https://github.com/docker/docker-install/blob/master/install.sh)

### Arguments

| Flag | Default | Description |
|------|---------|-------------|
| `--version <tag>` | latest | Pin neovex version (e.g., `v0.1.14`) |
| `--crun-version <tag>` | latest `agentstation/neovex-crun` release | Pin neovex-crun version (Linux only) |
| `--prefix <path>` | `/usr/local` | Install prefix for neovex binary |
| `--skip-deps` | false | Skip system dependency installation |
| `--dry-run` | false | Print what would happen, don't do anything |
| `--uninstall` | false | Remove neovex and neovex-crun |
| `-h`, `--help` | — | Show usage |

### Top-level flow

```
main()
  parse_args()
  detect_platform()           # uname -s, uname -m
  check_platform_support()    # gate unsupported platforms
  resolve_versions()          # GitHub API → latest release tags
  if Linux:
    check_kvm_access()        # warn if /dev/kvm missing or inaccessible
    install_system_deps()     # apt-get or dnf
    download_and_install_neovex()
    download_and_install_crun()
    verify_installation()
    print_getting_started_linux()
  if macOS:
    ensure_homebrew()
    install_or_upgrade_homebrew_cask()
    verify_installation()
    print_getting_started_macos()
```

### Platform detection

```
uname -s  →  Linux | Darwin
uname -m  →  x86_64 | aarch64 | arm64
```

| Detected | Mapped arch | Supported |
|----------|------------|-----------|
| Linux x86_64 | amd64 | Yes |
| Linux aarch64 | arm64 | Yes |
| Darwin arm64 | arm64 | Yes |
| Darwin x86_64 | — | No — hard fail: "Apple Silicon (M1+) required" |
| Other | — | No — hard fail |

### Linux distro detection

Source `/etc/os-release` for `ID` and `VERSION_ID`:

| `ID` | Package manager | libkrun strategy |
|------|----------------|-----------------|
| `debian`, `ubuntu` | apt-get | Manual instructions (Phase I1), prebuilt download (Phase I3), apt repo (Phase I5) |
| `fedora`, `rhel`, `centos`, `rocky`, `almalinux` | dnf | `dnf install libkrun libkrunfw` (in repos) |
| `amzn` | dnf | Same as Fedora family |
| Unknown | — | Warn, skip dep install, print manual instructions |

### Download tool selection

Try `curl` first, fall back to `wget`:

```sh
download() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$1"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- "$1"
  else
    err "need curl or wget to download files"
  fi
}
```

Respects `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` (inherited by curl/wget).

### Version resolution

For Linux, query GitHub API for the latest releases:

```
GET https://api.github.com/repos/agentstation/neovex/releases/latest
  → .tag_name → v0.1.14

GET https://api.github.com/repos/agentstation/neovex-crun/releases/latest
  → .tag_name → v1.27-neovex.1
```

If rate-limited (HTTP 403), suggest `--version` on Linux or `GITHUB_TOKEN`.

For macOS in the initial cut, the script installs the current Homebrew cask and
does not attempt to synthesize a historical cask for `--version`.

### Release asset naming

**neovex binary** (from `release.yml`):
```
neovex_linux_x86_64.tar.gz
neovex_linux_arm64.tar.gz
neovex_darwin_arm64.tar.gz
checksums-sha256.txt
```

**neovex-crun** (from `agentstation/neovex-crun` releases):
```
neovex-crun-linux-amd64
neovex-crun-linux-arm64
checksums.txt
```

Note the naming inconsistency: neovex uses `x86_64` while neovex-crun uses
`amd64`. The script maps between them.

### Checksum verification

For Linux, download `checksums-sha256.txt` (neovex) and `checksums.txt`
(`neovex-crun`) from the release, then verify with `sha256sum -c` or
`shasum -a 256 -c`.

For macOS, the install script delegates archive checksum verification to
Homebrew cask metadata rather than re-implementing cask install logic.

### Sudo handling

- Detect if running as root already
- If not root and installing to system paths, use `sudo` (check it exists)
- For `/usr/local/bin`: some systems allow user writes, check first
- For `/usr/libexec/neovex/crun`: always needs sudo
- For package manager commands (apt-get, dnf): always needs sudo
- For macOS Homebrew installs: do not use `sudo`; rely on the invoking user's
  Homebrew ownership and fail clearly if Homebrew itself is unavailable

### Non-interactive detection

When piped (`curl | sh`), stdin is the pipe, not the terminal. Detect with
`[ -t 0 ]` and skip interactive prompts when non-interactive.

### Idempotent behavior

- Check existing installation via `command -v neovex` and `neovex --version`
- If same version: skip with message
- If different version: replace (no confirmation in non-interactive mode)

---

## The libkrun Gap on Debian/Ubuntu

libkrun and libkrunfw are NOT in Debian/Ubuntu repos. This is the single
hardest problem in the install script. The strategy evolves across phases:

### Phase I1 (initial): Manual instructions

The script installs everything except libkrun/libkrunfw, then prints:

```
⚠ libkrun and libkrunfw are not yet available as Debian packages.

To build from source:
  git clone https://github.com/containers/libkrunfw && cd libkrunfw
  git checkout v5.3.0 && make && sudo make install

  git clone https://github.com/containers/libkrun && cd libkrun
  git checkout v1.17.4 && make && sudo make install

  echo "/usr/local/lib64" | sudo tee /etc/ld.so.conf.d/libkrun.conf
  sudo ldconfig

On Fedora, these are available from repos:
  sudo dnf install libkrun libkrunfw
```

### Phase I3 (follow-up): Prebuilt .so download

New CI workflow `neovex-libkrun.yml` builds libkrun+libkrunfw for
Debian (amd64+arm64) and publishes `.so` files as GitHub Release assets.
The install script downloads and installs them to `/usr/local/lib64/`,
creates the ldconfig entry, and runs ldconfig.

### Phase I5 (mature): Apt repository

The install script adds the neovex apt repo and installs libkrun as a
proper `.deb` package. This is the D2 phase from the distribution plan.

---

## Verification Helper

`scripts/verify-install.sh` — standalone post-install check, also called
at the end of the install script. Reuses the `print_line` / `check_command`
pattern from the existing `scripts/check-vmm-host.sh`.

### Linux checks

| Check | How | Required |
|-------|-----|----------|
| `neovex --version` | command + version output | yes |
| `/usr/libexec/neovex/crun --version` | command + `+LIBKRUN` in output | yes |
| `/dev/kvm` exists | `test -c /dev/kvm` | warn only |
| `/dev/kvm` accessible | `test -r /dev/kvm -a -w /dev/kvm` | warn only |
| conmon | `command -v conmon` | yes |
| buildah | `command -v buildah` | yes |
| catatonit | `command -v catatonit` | yes |
| passt | `command -v passt` | recommended |
| newuidmap | `command -v newuidmap` | recommended |
| fuse-overlayfs | `command -v fuse-overlayfs` | recommended |
| libkrun.so | `ldconfig -p \| grep libkrun` | yes |
| libkrunfw.so | `ldconfig -p \| grep libkrunfw` | yes |
| containers config | `/etc/containers/` or `/usr/share/containers/` exists | recommended |

### macOS checks

| Check | How | Required |
|-------|-----|----------|
| `neovex --version` | command + version output | yes |
| `krunkit --version` | `command -v krunkit` | yes |
| bundled `gvproxy` exists | resolve installed `neovex` path into Caskroom and check adjacent `libexec/gvproxy` | yes |
| macOS version >= 14 | `sw_vers -productVersion` | yes |
| Architecture arm64 | `uname -m` | yes |

### Output format

```
neovex              present path=/usr/local/bin/neovex version=neovex 0.1.0
neovex-crun         present path=/usr/libexec/neovex/crun version=crun version 1.27-dirty ... +LIBKRUN
kvm.device          present path=/dev/kvm
kvm.access          ok user=jack groups=kvm
conmon              present path=/usr/bin/conmon version=conmon version 2.1.12
buildah             present path=/usr/bin/buildah version=buildah (Buildah) 1.39.3
libkrun.so          present version=1.17.4
libkrunfw.so        present version=5.3.0
result              supported (0 failures)
```

---

## Files to Create

| File | Purpose |
|------|---------|
| `scripts/install.sh` | Main install script (POSIX sh) |
| `scripts/verify-install.sh` | Post-install verification helper (bash) |
| `scripts/verify-install-helper.sh` | Deterministic unit tests for verify-install |

---

## Prerequisites

| Prerequisite | Status | Needed by |
|-------------|--------|-----------|
| neovex binary CI (`release.yml`) | exists, publishes tagged releases | Phase I2 |
| neovex-crun release source (`agentstation/neovex-crun`) | exists, publishes tagged releases | Phase I2 |
| At least one `v*` Neovex release tag | pushed (`v0.1.14`) | Phase I2 |
| At least one `agentstation/neovex-crun` release tag | pushed (`v1.27-neovex.1`) | Phase I2 |
| Homebrew cask auto-update in release workflow | exists | Phase I4 |
| `neovex.dev` domain serving script | not configured | Phase I5 |
| libkrun prebuilt .so CI | does not exist | Phase I3 |
| neovex apt repo at `apt.neovex.dev` | does not exist | Phase I5 |

---

## Phase Plan

### Phase I1: Skeleton, Platform Detection, and Verification Helper

**Goal:** Install script structure with platform detection, argument parsing,
and the verification helper.

**Scope:**
- `scripts/install.sh` with POSIX sh argument parsing, platform detection,
  distro detection, utility functions (`say`, `err`, `need_cmd`,
  `download`), and `--dry-run` mode
- `scripts/verify-install.sh` with all Linux and macOS checks
- `scripts/verify-install-helper.sh` for deterministic testing
- `--dry-run` prints the full install plan without executing

**Acceptance criteria:**
- `bash -n scripts/install.sh` passes
- `sh scripts/install.sh --dry-run` prints correct platform detection and
  install plan on both Linux and macOS
- `bash scripts/verify-install.sh` runs all checks and reports results
- `bash scripts/verify-install-helper.sh` passes

### Phase I2: Linux Binary Download and Installation

**Goal:** Download `neovex` and `neovex-crun` on Linux from their GitHub
releases, verify checksums, and install to the correct paths.

**Scope:**
- Version resolution via GitHub API (latest or `--version`)
- Download Neovex tarball from `agentstation/neovex`
- Download `neovex-crun` binary from `agentstation/neovex-crun`
- SHA256 checksum verification
- Install to `/usr/local/bin/neovex` and `/usr/libexec/neovex/crun`
- Sudo handling for system paths
- Idempotent: skip if same version already installed

**Hard deps:** at least one `agentstation/neovex` tag and one
`agentstation/neovex-crun` tag pushed.

**Acceptance criteria:**
- `sh scripts/install.sh` on a clean Ubuntu VM downloads and installs both
  binaries
- `neovex --version` and `/usr/libexec/neovex/crun --version` work
- Checksums verified before install
- Running twice skips if same version

### Phase I3: Linux System Dependencies

**Goal:** Automatically install all system dependencies on supported distros.

**Scope:**
- `install_deps_debian()` — apt-get install for Debian/Ubuntu
- `install_deps_fedora()` — dnf install for Fedora/RHEL (includes libkrun)
- libkrun gap on Debian: print manual build instructions (Phase I1 approach)
- KVM access check with remediation instructions
- `--skip-deps` flag to bypass

**Acceptance criteria:**
- Fresh Debian 13 VM: script installs all deps except libkrun, prints
  manual instructions for libkrun
- Fresh Fedora 42 VM: script installs all deps including libkrun from repos
- `scripts/verify-install.sh` reports all deps present (except libkrun on
  Debian)

### Phase I4: macOS Installation

**Goal:** Install or upgrade the published `agentstation/tap/neovex` cask on
macOS Apple Silicon.

**Scope:**
- Check Homebrew is available
- `brew tap agentstation/tap` and `brew tap slp/krunkit`
- `brew install --cask agentstation/tap/neovex` or
  `brew upgrade --cask agentstation/tap/neovex`
- Verify the installed cask layout includes bundled `libexec/gvproxy`
- Print getting-started: `neovex machine init` + `neovex serve`

**Acceptance criteria:**
- `sh scripts/install.sh` on a clean macOS M1+ installs `agentstation/tap/neovex`
- `neovex --version` and `krunkit --version` work
- the installed cask layout includes bundled `libexec/gvproxy`
- Intel Mac: script fails with clear message

### Phase I5: Uninstall, Upgrade, and Polish

**Goal:** Complete lifecycle management and production readiness.

**Scope:**
- Linux `--uninstall` removes `neovex`, `neovex-crun`, and any apt repo entry
- macOS `--uninstall` uses `brew uninstall --cask neovex`
- Upgrade path: detect existing version, replace if different
- Host `install.sh` at `neovex.dev/install.sh` (GitHub Pages or redirect)
- Error messages and getting-started output polished

**Acceptance criteria:**
- `sh scripts/install.sh --uninstall` cleanly removes everything
- Upgrade from v0.1.11 to v0.1.14 works
- `curl -fsSL https://neovex.dev/install.sh | sh` works end-to-end

---

## Phase Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| I1: Skeleton + verification | `todo` | — | Can start immediately |
| I2: Linux binary download | `todo` | I1, release tags pushed | External release inputs now exist |
| I3: Linux system deps | `todo` | I2 | libkrun gap = manual instructions initially |
| I4: macOS installation | `todo` | I1 | Homebrew cask + bundled `gvproxy` + `krunkit` |
| I5: Uninstall + polish | `todo` | I3, I4 | Domain hosting, lifecycle |

---

## Edge Cases

### Rootless install

Linux user-prefix installs are out of scope for the first cut. The current
Linux runtime contract assumes `/usr/libexec/neovex/crun`, and the codebase
does not currently expose a supported runtime-path override for a user-prefix
install. If rootless/user-prefix support becomes important, document and land
that runtime-path contract explicitly first rather than letting the install
script invent it.

### GitHub API rate limits

Unauthenticated: 60 requests/hour. If rate-limited (HTTP 403), print:
```
GitHub API rate limit reached. Either:
  - Specify version: sh install.sh --version v0.1.14
  - Set GITHUB_TOKEN: export GITHUB_TOKEN=ghp_...
```

### Proxy/corporate environments

The script uses curl/wget which respect `HTTPS_PROXY`, `HTTP_PROXY`,
`NO_PROXY` automatically. No special handling needed.

### Existing Docker/Podman installations

The script installs buildah and conmon alongside any existing Docker or
Podman. It does NOT modify Docker configuration or conflict with existing
container runtimes. neovex-crun installs to `/usr/libexec/neovex/crun`,
not the system crun path.

On macOS, the script should not treat Homebrew `podman` or Podman Desktop as
Neovex's dependency manager. The supported install path is the Neovex cask,
which owns `krunkit` and carries the bundled `gvproxy` helper itself.

---

## Execution Log

| Date | Phase | Status | Notes | Verification | Next |
|------|-------|--------|-------|--------------|------|
| 2026-04-18 | planning refresh | `documented` | Rebased the install-script plan onto the current shipped distribution contract so implementation can start from a correct foundation. The initial Channel 1 design is now explicitly platform-split: Linux installs distro deps plus released `neovex` / `neovex-crun` artifacts directly from GitHub Releases, while macOS installs or upgrades the published `agentstation/tap/neovex` Homebrew cask instead of manually unpacking a single binary. The plan now points at the external `agentstation/neovex-crun` release source, treats the darwin bundled `libexec/gvproxy` helper as part of the required macOS install contract, drops the stale rootless `NEOVEX_CRUN_PATH` assumption, and updates prerequisites to the current `v0.1.14` / `v1.27-neovex.1` release reality. | plan review against `.github/workflows/release.yml`; plan review against `docs/reference/macos-machine-flow.md`; `gh release list --repo agentstation/neovex --limit 5`; `gh release list --repo agentstation/neovex-crun --limit 5` | Start I1 by writing `scripts/install.sh`, `scripts/verify-install.sh`, and `scripts/verify-install-helper.sh` to this refreshed contract |
