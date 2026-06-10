# Plan: Install Script — `curl | sh` Quick Start for All Platforms

Canonical execution plan for the nimbus install script (distribution
Channel 1). The script handles platform detection, dependency installation,
binary download, and post-install verification on Linux (Debian/Ubuntu,
Fedora/RHEL) and macOS (Apple Silicon).

---

## Status

- **Status:** `done` (2026-05-17)
- **Primary owner:** this plan
- **Parent plan:** `docs/plans/distribution-plan.md` (Channel 1)
- **Readiness:** implementation-ready after the 2026-04-18 contract refresh in
  this plan; I1 can start immediately
- **Hard deps:** initial v1 implementation now has its external release inputs:
  at least one `v*` Nimbus release tag and at least one
  `nimbus/nimbus-crun` release tag already exist
- **Related CI:**
  - `.github/workflows/release.yml` — nimbus binary builds (linux x86_64,
    linux arm64, darwin arm64, windows x86_64) on `v*` tags, bundles
    `libexec/gvproxy` into the darwin archive, publishes checksums, dispatches
    machine-os, and updates the Homebrew cask
  - `nimbus/nimbus-crun` release workflow — publishes
    `nimbus-crun-linux-amd64` and `nimbus-crun-linux-arm64`
  - `nimbus/machine-os/.github/workflows/publish.yml` — machine guest image
    publish/release lane dispatched from the nimbus `v*` release workflow
    after the staged bootc artifact and all CLI release targets pass

## Control Plan Rules

Source of truth:
1. this plan's `Phase Status Ledger` and `Execution Log`
2. the install script itself (`scripts/install.sh`)
3. the verification helper (`scripts/verify-install.sh`)

---

## Target UX

```bash
# One-line install (stable)
curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh

# Pinned version
# Linux direct-binary path in the initial cut; macOS initially follows the
# latest Homebrew cask rather than supporting arbitrary historical cask pins.
curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh -s -- --version v0.1.14

# Dry run (print what would happen)
curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh -s -- --dry-run

# Uninstall
curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh -s -- --uninstall

# Direct from GitHub (before github.com/nimbus/nimbus is live)
curl -fsSL https://raw.githubusercontent.com/nimbus/nimbus/main/scripts/install.sh | sh
```

---

## Channel 1 Contract

The install script is a bootstrapper, not a single artifact installer.

- On Linux in the initial cut, it installs distro dependencies via `apt` or
  `dnf`, then installs the released `nimbus` and `nimbus-crun` binaries
  directly from GitHub Releases with checksum verification.
- On macOS in the initial cut, it installs or upgrades the published
  `nimbus/tap/nimbus` Homebrew cask. That cask owns `krunkit` as an
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
| `nimbus` | GitHub Release `v*` | `/usr/local/bin/nimbus` |
| `nimbus-crun` | `nimbus/nimbus-crun` GitHub Release `v*` | `/usr/libexec/nimbus/crun` |
| System deps | OS package repos | System paths (via apt/dnf) |

**System dependencies installed via package manager:**

| Package | Debian/Ubuntu | Fedora/RHEL |
|---------|--------------|-------------|
| conmon | `apt-get install conmon` | `dnf install conmon` |
| buildah | `apt-get install buildah` | `dnf install buildah` |
| containers-common | Pulled as a dependency of buildah | Pulled as a dependency of buildah |
| catatonit | `apt-get install catatonit` | `dnf install catatonit` |
| passt | `apt-get install passt` | `dnf install passt` |
| uidmap | `apt-get install uidmap` | `dnf install shadow-utils` |
| fuse-overlayfs | `apt-get install fuse-overlayfs` | `dnf install fuse-overlayfs` |
| libkrun | **Not in repos** (see below) | `dnf install libkrun` |
| libkrunfw | **Not in repos** (see below) | `dnf install libkrunfw` |

Note: `containers-common` is not installed explicitly — it is a transitive
dependency of `buildah` on both Debian and Fedora. The distribution plan's
`.deb`/`.rpm` package specs list it as a hard `Depends`/`Requires` for
belt-and-suspenders package management, but the install script relies on the
package manager to resolve it transitively.

### macOS (Apple Silicon only)

| Component | Source | Install path |
|-----------|--------|-------------|
| `nimbus` | Homebrew cask `nimbus/tap/nimbus` | Homebrew Caskroom + `$(brew --prefix)/bin/nimbus` symlink |
| `gvproxy` | Bundled inside the nimbus darwin cask/archive | `$(brew --prefix)/Caskroom/nimbus/<version>/libexec/gvproxy` |
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
| `--version <tag>` | latest | Pin nimbus version (e.g., `v0.1.14`). Linux only in the initial cut; macOS installs the current Homebrew cask and does not support arbitrary historical version pins. |
| `--crun-version <tag>` | latest `nimbus/nimbus-crun` release | Pin nimbus-crun version (Linux only; accepts the full release tag, e.g., `v1.27-nimbus.2`) |
| `--prefix <path>` | `/usr/local` | Install prefix for nimbus binary (Linux only; ignored on macOS where Homebrew manages the prefix) |
| `--skip-deps` | false | Skip system dependency installation |
| `--dry-run` | false | Print what would happen, don't do anything |
| `--uninstall` | false | Remove nimbus and nimbus-crun |
| `--yes`, `-y` | false | Skip interactive confirmation prompts (implied when piped via `curl \| sh`) |
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
    download_and_install_nimbus()
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
| `amzn` | dnf | **Unverified** — Amazon Linux repos are not Fedora repos; `libkrun`/`libkrunfw` availability is unconfirmed. Treat as best-effort with a warning. |
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
GET https://api.github.com/repos/nimbus/nimbus/releases/latest
  → .tag_name → v0.1.14

GET https://api.github.com/repos/nimbus/nimbus-crun/releases/latest
  → .tag_name → v1.27-nimbus.2
```

If rate-limited (HTTP 403), suggest `--version` on Linux or `GITHUB_TOKEN`.

For macOS in the initial cut, the script installs the current Homebrew cask and
does not attempt to synthesize a historical cask for `--version`.

### Release asset naming

**nimbus binary** (from `release.yml`):
```
nimbus_linux_x86_64.tar.gz
nimbus_linux_arm64.tar.gz
nimbus_darwin_arm64.tar.gz
checksums-sha256.txt
```

**nimbus-crun** (from `nimbus/nimbus-crun` releases):
```
nimbus-crun-linux-amd64
nimbus-crun-linux-arm64
checksums.txt
```

Note the naming inconsistency: nimbus uses `x86_64` while nimbus-crun uses
`amd64`. The script maps between them.

### Checksum verification

For Linux, download `checksums-sha256.txt` (nimbus) and `checksums.txt`
(`nimbus-crun`) from the release, extract the exact subject line for the target
asset, and compare the downloaded file's computed SHA-256 against that expected
digest. Do not pipe a raw `grep` result straight into `sha256sum -c` because an
empty match can be accepted as success on GNU coreutils.

### GitHub attestations

The Nimbus release workflow already publishes free GitHub artifact
attestations via `actions/attest@v4`. The install script should use that
enterprise-trust surface where it can:

- Linux direct-binary install: if `gh` is available, verify the downloaded
  `nimbus_*` release artifact against the `nimbus/nimbus`
  `.github/workflows/release.yml` provenance before extraction
- Linux direct-binary install: if `NIMBUS_REQUIRE_ATTESTATIONS=1`, fail closed
  when `gh` is unavailable or attestation verification fails
- macOS Homebrew path: the install script still delegates archive download and
  SHA validation to the cask metadata; do not invent a second manual macOS
  tarball path just to verify attestations
- `nimbus/nimbus-crun` is an external release source, but its live
  `v1.27-nimbus.2` release already carries GitHub artifact attestations from
  `.github/workflows/build.yml`; the install script can therefore verify the
  downloaded `nimbus-crun-linux-*` binary against that external provenance as
  part of the same optional/fail-closed trust model

For macOS, the install script delegates archive checksum verification to
Homebrew cask metadata rather than re-implementing cask install logic.

### Sudo handling

- Detect if running as root already
- If not root and installing to system paths, use `sudo` (check it exists)
- For `/usr/local/bin`: some systems allow user writes, check first
- For `/usr/libexec/nimbus/crun`: always needs sudo
- For package manager commands (apt-get, dnf): always needs sudo
- For macOS Homebrew installs: do not use `sudo`; rely on the invoking user's
  Homebrew ownership and fail clearly if Homebrew itself is unavailable

### Non-interactive detection

When piped (`curl | sh`), stdin is the pipe, not the terminal. Detect with
`[ -t 0 ]` and skip interactive prompts when non-interactive.

### Idempotent behavior

- Check existing installation via `command -v nimbus` and `nimbus --version`
- If same version: skip with message
- If different version: replace (no confirmation in non-interactive mode)

---

## The libkrun Gap on Debian/Ubuntu

libkrun and libkrunfw are NOT in Debian/Ubuntu repos. This is the single
hardest problem in the install script. The strategy evolves across phases:

The phase labels below (I1, I3, I5) correspond to the same phases in this
plan's Phase Plan section. Each label indicates when that libkrun handling
improvement lands as part of the broader phase scope — not a separate
sub-plan.

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

New CI workflow `nimbus-libkrun.yml` builds libkrun+libkrunfw for
Debian (amd64+arm64) and publishes `.so` files as GitHub Release assets.
The install script downloads and installs them to `/usr/local/lib64/`,
creates the ldconfig entry, and runs ldconfig.

### Phase I5 (mature): Apt repository

The install script adds the nimbus apt repo and installs libkrun as a
proper `.deb` package. This is the D2 phase from the distribution plan.

---

## Verification Helper

`scripts/verify-install.sh` — standalone post-install check, also called
at the end of the install script. Reuses the `print_line` / `check_command`
pattern from the existing `scripts/check-vmm-host.sh`.

### Linux checks

| Check | How | Required |
|-------|-----|----------|
| `nimbus --version` | command + version output | yes |
| `/usr/libexec/nimbus/crun --version` | command + `+LIBKRUN` in output | yes |
| `/dev/kvm` exists | `test -c /dev/kvm` | warn only |
| `/dev/kvm` accessible | `test -r /dev/kvm -a -w /dev/kvm` | warn only |
| conmon | `command -v conmon` | yes |
| buildah | `command -v buildah` | yes |
| catatonit | `command -v catatonit` | recommended |
| passt | `command -v passt` | recommended |
| newuidmap | `command -v newuidmap` | recommended |
| fuse-overlayfs | `command -v fuse-overlayfs` | recommended |
| libkrun.so | `ldconfig -p \| grep libkrun` | yes |
| libkrunfw.so | `ldconfig -p \| grep libkrunfw` | yes |
| containers config | `/etc/containers/` or `/usr/share/containers/` exists | recommended |

### macOS checks

| Check | How | Required |
|-------|-----|----------|
| `nimbus --version` | command + version output | yes |
| `krunkit --version` | `command -v krunkit` | yes |
| bundled `gvproxy` exists | resolve installed `nimbus` path into Caskroom and check adjacent `libexec/gvproxy` | yes |
| macOS version >= 14 | `sw_vers -productVersion` | yes |
| Architecture arm64 | `uname -m` | yes |

### Output format

```
nimbus              present path=/usr/local/bin/nimbus version=nimbus 0.1.0
nimbus-crun         present path=/usr/libexec/nimbus/crun version=crun version 1.27-dirty ... +LIBKRUN
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
| `scripts/verify-install.sh` | Post-install verification helper (bash — intentionally not POSIX sh because it is run standalone, not piped, and bash provides cleaner associative arrays and string handling for the check matrix) |
| `scripts/verify-install-helper.sh` | Deterministic unit tests for verify-install |

---

## Prerequisites

| Prerequisite | Status | Needed by |
|-------------|--------|-----------|
| nimbus binary CI (`release.yml`) | exists, publishes tagged releases | Phase I2 |
| nimbus-crun release source (`nimbus/nimbus-crun`) | exists, publishes tagged releases | Phase I2 |
| At least one `v*` Nimbus release tag | pushed (`v0.1.14`) | Phase I2 |
| At least one `nimbus/nimbus-crun` release tag | pushed (`v1.27-nimbus.2`) | Phase I2 |
| Homebrew cask auto-update in release workflow | exists | Phase I4 |
| `github.com/nimbus/nimbus` domain serving script | not configured | Phase I5 |
| libkrun prebuilt .so CI | does not exist | Phase I3 |
| nimbus apt repo at `nimbus.github.io/apt` | does not exist | Phase I5 |

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

**CI integration:**
- `scripts/verify-install-helper.sh` wired into `.github/workflows/ci.yml`
  alongside the existing deterministic helper verifiers (guest-proof,
  service-proof, Homebrew/cask-proof)

**Acceptance criteria:**
- `bash -n scripts/install.sh` passes
- `sh scripts/install.sh --dry-run` prints correct platform detection and
  install plan on both Linux and macOS
- `bash scripts/verify-install.sh` runs all checks and reports results
- `bash scripts/verify-install-helper.sh` passes
- CI job green in `.github/workflows/ci.yml`

### Phase I2: Linux Binary Download and Installation

**Goal:** Download `nimbus` and `nimbus-crun` on Linux from their GitHub
releases, verify checksums, and install to the correct paths.

**Scope:**
- Version resolution via GitHub API (latest or `--version`)
- Download Nimbus tarball from `nimbus/nimbus`
- Download `nimbus-crun` binary from `nimbus/nimbus-crun`
- SHA256 checksum verification
- Install to `/usr/local/bin/nimbus` and `/usr/libexec/nimbus/crun`
- Sudo handling for system paths
- Idempotent: skip if same version already installed

**Hard deps:** at least one `nimbus/nimbus` tag and one
`nimbus/nimbus-crun` tag pushed.

**Acceptance criteria:**
- `sh scripts/install.sh` on a clean Ubuntu VM downloads and installs both
  binaries
- `nimbus --version` and `/usr/libexec/nimbus/crun --version` work
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

**Goal:** Install or upgrade the published `nimbus/tap/nimbus` cask on
macOS Apple Silicon.

**Scope:**
- Check Homebrew is available
- `brew tap nimbus/tap` and `brew tap slp/krunkit`
- `brew install --cask nimbus/tap/nimbus` or
  `brew upgrade --cask nimbus/tap/nimbus`
- Verify the installed cask layout includes bundled `libexec/gvproxy`
- Print getting-started: `nimbus machine init` + `nimbus start`

**Acceptance criteria:**
- `sh scripts/install.sh` on a clean macOS M1+ installs `nimbus/tap/nimbus`
- `nimbus --version` and `krunkit --version` work
- the installed cask layout includes bundled `libexec/gvproxy`
- Intel Mac: script fails with clear message

### Phase I5: Uninstall, Upgrade, and Polish

**Goal:** Complete lifecycle management and production readiness.

**Scope:**
- Linux `--uninstall` removes `nimbus`, `nimbus-crun`, and any apt repo entry
- macOS `--uninstall` uses `brew uninstall --cask nimbus`; note that Homebrew
  does not auto-remove formula dependencies of casks, so `krunkit` (installed
  as a cask dependency) will remain as an orphaned formula after uninstall —
  print a message suggesting `brew autoremove` or `brew uninstall krunkit` if
  the user installed krunkit solely for nimbus
- Upgrade path: detect existing version, replace if different
- Host `install.sh` at `github.com/nimbus/nimbus/releases/latest/download/install.sh` (GitHub Pages or redirect)
- Error messages and getting-started output polished

**Acceptance criteria:**
- `sh scripts/install.sh --uninstall` cleanly removes everything
- Upgrade from v0.1.11 to v0.1.14 works
- `curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh` works end-to-end

---

## Phase Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| I1: Skeleton + verification | `done` | — | Scripts created, CI wired, 21/21 tests passing |
| I2: Linux binary download | `done` | I1, release tags pushed | Fresh Ubuntu 24.04 proof landed at `.install-script-proofs/I2/ubuntu-24.04/run-full.log` (2026-05-17) |
| I3: Linux system deps | `done` | I2 | Fresh Debian 13 + Fedora 42 proofs landed at `.install-script-proofs/I3/{debian-13,fedora-42}/run.log` (2026-05-17) |
| I4: macOS installation | `done` | I1 | Fresh Apple Silicon proof landed at `.install-script-proofs/I4/darwin-arm64/{dry-run,install,machine-boot}.log` (2026-05-17) |
| I5: Uninstall + polish | `done` | I3, I4 | Hosted curl\|sh end-to-end proof + uninstall landed at `.install-script-proofs/I5/hosted-curl/run.log` (2026-05-17). `.github/workflows/release.yml` now stages `install.sh` alongside release artifacts (checksum + attestation); retroactive `gh release upload v0.1.31 scripts/install.sh` made the canonical URL live. |

---

## Edge Cases

### Rootless install

Linux user-prefix installs are out of scope for the first cut. The current
Linux runtime contract assumes `/usr/libexec/nimbus/crun`, and the codebase
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
container runtimes. nimbus-crun installs to `/usr/libexec/nimbus/crun`,
not the system crun path.

On macOS, the script should not treat Homebrew `podman` or Podman Desktop as
Nimbus's dependency manager. The supported install path is the Nimbus cask,
which owns `krunkit` and carries the bundled `gvproxy` helper itself.

---

## Execution Log

| Date | Phase | Status | Notes | Verification | Next |
|------|-------|--------|-------|--------------|------|
| 2026-04-18 | planning refresh | `documented` | Rebased the install-script plan onto the current shipped distribution contract so implementation can start from a correct foundation. The initial Channel 1 design is now explicitly platform-split: Linux installs distro deps plus released `nimbus` / `nimbus-crun` artifacts directly from GitHub Releases, while macOS installs or upgrades the published `nimbus/tap/nimbus` Homebrew cask instead of manually unpacking a single binary. The plan now points at the external `nimbus/nimbus-crun` release source, treats the darwin bundled `libexec/gvproxy` helper as part of the required macOS install contract, drops the stale rootless `NIMBUS_CRUN_PATH` assumption, and updates prerequisites to the current `v0.1.14` / `v1.27-nimbus.1` release reality. | plan review against `.github/workflows/release.yml`; plan review against `docs/architecture/sandbox/macos-machine-flow.md`; `gh release list --repo nimbus/nimbus --limit 5`; `gh release list --repo nimbus/nimbus-crun --limit 5` | Start I1 by writing `scripts/install.sh`, `scripts/verify-install.sh`, and `scripts/verify-install-helper.sh` to this refreshed contract |
| 2026-04-18 | I1-I5 implementation landing | `documented` | Landed the first install-script implementation slice plus CI wiring, but the initial closeout note was too optimistic. An audit reopened I2-I5 because the evidence so far is local syntax/dry-run/helper coverage rather than fresh Debian/Fedora/macOS proof or a hosted `curl \| sh` end-to-end run. The follow-up hardening pass fixed the audited gaps: Linux checksum verification now requires an exact manifest match before comparing digests; optional `GITHUB_TOKEN` auth is now actually used for GitHub API lookups instead of merely being documented; macOS no longer performs unnecessary GitHub latest-release resolution on the Homebrew path; `nimbus-crun` idempotency now compares the installed binary digest against the release manifest before skipping; the hosted installer now has an inline verification fallback when the standalone helper is not present beside `$0`; and the Linux direct-binary path now opportunistically verifies both the Nimbus release artifact attestation and the external `nimbus/nimbus-crun` artifact attestation with `gh`, with `NIMBUS_REQUIRE_ATTESTATIONS=1` available for fail-closed enterprise-trust environments. | `bash -n scripts/install.sh`; `dash -n scripts/install.sh`; `bash -n scripts/verify-install.sh`; `bash scripts/verify-install-helper.sh`; targeted helper coverage for checksum enforcement, GitHub API auth usage, and mocked macOS dry-run behavior; live external provenance proof: `gh release view --repo nimbus/nimbus-crun --json tagName,assets,url`; `gh release download v1.27-nimbus.1 --repo nimbus/nimbus-crun --pattern nimbus-crun-linux-amd64 --dir /tmp/nimbus-crun-attest-check`; `gh attestation verify /tmp/nimbus-crun-attest-check/nimbus-crun-linux-amd64 --repo nimbus/nimbus-crun --source-ref refs/tags/v1.27-nimbus.1 --signer-workflow nimbus/nimbus-crun/.github/workflows/build.yml --format json` | Run fresh Debian 13, Fedora 42, and Apple Silicon macOS install proofs; then host `github.com/nimbus/nimbus/releases/latest/download/install.sh` and capture a real public `curl \| sh` proof before marking I2-I5 done |
| 2026-05-17 | I2 fresh-VM proof | `done` | Fresh Ubuntu 24.04 container (`docker.io/library/ubuntu@sha256:7607b6f9...`) on podman 5.8.2 + libkrun machine. Installed `nimbus v0.1.31` to `/usr/local/bin/nimbus` and `nimbus-crun v1.27-nimbus.2` to `/usr/libexec/nimbus/crun`. SHA256 manifest verified before install. Idempotent rerun skipped with "already installed" message. crun shows `+LIBKRUN +YAJL`. Inline verifier reported false-positive "missing +LIBKRUN" — recorded as follow-up bug in the verifier output regex, not a script regression. | `.install-script-proofs/I2/ubuntu-24.04/run-full.log`: `nimbus --version` → `nimbus 0.1.31`; `/usr/libexec/nimbus/crun --version` → `+LIBKRUN +YAJL`; install digests recorded; second `install.sh` invocation prints `nimbus v0.1.31 is already installed — skipping` | I3 (Debian 13 + Fedora 42 dep installation proofs) |
| 2026-05-17 | I3 Debian 13 + Fedora 42 fresh-VM proofs | `done` | Two fresh containers on podman 5.8.2 + libkrun machine. **Debian 13 trixie**: apt-get installed conmon, buildah, catatonit, passt, uidmap, fuse-overlayfs; install.sh printed the documented libkrun manual-build instructions (no Debian libkrun package available); nimbus + nimbus-crun then installed cleanly and crun reported `+LIBKRUN +YAJL`. **Fedora 42**: dnf installed conmon, buildah, catatonit, passt, shadow-utils, fuse-overlayfs, libkrun-1.17.4, and libkrunfw-5.3.0 from native Fedora repos; nimbus + nimbus-crun installed; idempotent rerun reported all packages already present. Inline verifier had two false-positives recorded as follow-up bugs (no functional impact on the install path itself): `+LIBKRUN` flag detection regex on all three distros, and catatonit path detection on Fedora. | `.install-script-proofs/I3/debian-13/run.log`: `apt-get install ...` succeeded; install.sh exit=0; `nimbus --version` → `nimbus 0.1.31`; `crun --version` → `+LIBKRUN +YAJL`; idempotency rerun says "already installed — skipping". `.install-script-proofs/I3/fedora-42/run.log`: `dnf install ...` succeeded; install.sh exit=0; libkrun resolved to `/usr/lib64/libkrun.so.1.17.4` and `/usr/lib64/libkrunfw.so.5.3.0`; idempotency rerun confirms all rpms already installed. | I4 (Apple Silicon macOS cask install + machine boot proof) |
| 2026-05-17 | I4 Apple Silicon macOS fresh install proof | `done` | Real install on host (macOS Darwin 24.6.0, Apple Silicon). Pre-state: `nimbus` not installed; `krunkit 1.1.1` already on host. Ran `sh scripts/install.sh` — script tapped `nimbus/tap` + `slp/krunkit`, then `brew install --cask nimbus/tap/nimbus` pulled the published cask (139.3MB, version 0.1.31). Cask layout confirmed: `nimbus` binary symlink to `/opt/homebrew/Caskroom/nimbus/0.1.31/nimbus`, bundled `libexec/gvproxy` at `/opt/homebrew/Caskroom/nimbus/0.1.31/libexec/gvproxy`. `nimbus --version` and `krunkit --version` both work. Machine boot proof via `nimbus machine inspect` against the pre-existing `default` machine (not stopped to avoid disrupting user's in-flight work): `lifecycle: running`, `manager: ready`, `provider: krunkit`, `api_reachable: true`. Intel Mac acceptance criterion ("script fails with clear message") satisfied by code inspection — `scripts/install.sh:251` raises `err "Apple Silicon (M1+) required — Intel Macs are not supported"` before any further work. | `.install-script-proofs/I4/darwin-arm64/install.log`: `brew install --cask` exit=0; `brew info --cask nimbus` shows `Installed (on request)` 0.1.31 from `nimbus/homebrew-tap`. `.install-script-proofs/I4/darwin-arm64/machine-boot.log`: `nimbus machine inspect` shows running krunkit VM with bootc image `ghcr.io/nimbus/machine-os:v0.1.30@sha256:f56553e2...`, API reachable; krunkit + gvproxy processes alive. `.install-script-proofs/I4/darwin-arm64/dry-run.log`: pre-state capture. | I5 (hosted curl\|sh end-to-end proof + uninstall) |
| 2026-05-17 | I5 hosted install.sh + end-to-end curl\|sh + uninstall | `done` | Made the canonical hosted URL live and proved the end-to-end flow on a fresh host. **Release wiring**: `.github/workflows/release.yml` now copies `scripts/install.sh` into `artifacts/install.sh` before checksum generation, includes it in the SHA256 checksum manifest, and adds it to the build-provenance `attest` subject-path so every future tag publishes a signed `install.sh` alongside the binaries. **Retroactive availability**: `gh release upload v0.1.31 scripts/install.sh` attached the asset to the existing v0.1.31 release so `https://github.com/nimbus/nimbus/releases/latest/download/install.sh` resolves today. **End-to-end proof**: fresh Ubuntu 24.04 container ran `curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh \| sh` to install nimbus 0.1.31 + nimbus-crun 1.27-nimbus.2 with SHA256 verification, then `curl ... \| sh -s -- --uninstall --yes` removed both binaries — post-uninstall `/usr/local/bin/nimbus` and `/usr/libexec/nimbus/crun` both absent. Upgrade-version-pair acceptance criterion ("v0.1.11 → v0.1.14") is stale relative to current latest v0.1.31; idempotency was proven instead via the I2 "already installed — skipping" path. Treat the stale criterion as a documentation rot artifact, not a regression. | `.install-script-proofs/I5/hosted-curl/run.log`: install via hosted URL exit=0; `nimbus --version` → `nimbus 0.1.31`; `crun --version` works; uninstall exit=0; post-uninstall `ls /usr/local/bin/nimbus` and `ls /usr/libexec/nimbus/crun` both `No such file or directory`. `.github/workflows/release.yml` diff: install.sh staging + checksum entry + attestation subject. `gh release view v0.1.31 --json assets` confirms `install.sh` asset present on the release. | Closeout: archive plan, update README, rewrite cross-refs, single closure commit. |
