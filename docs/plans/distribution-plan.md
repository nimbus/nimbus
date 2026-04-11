# Plan: Distribution — Packaging neovex for All Channels

Canonical plan for distributing neovex and its dependencies across all
target platforms and package channels.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** promote when `microvm-runtime-plan.md` Phase M2 is
  complete (neovex can generate OCI bundles and boot VMs)
- **Related plans:**
  - `vmm-infrastructure-plan.md` — produces the crun fork
  - `microvm-runtime-plan.md` — produces the neovex binary

## Control Plan Rules

Source of truth:
1. this plan's `Phase Status Ledger` and `Execution Log`
2. CI/CD pipeline configuration

---

## What We Ship

### Binaries

| Binary | Source | Size | Built by |
|--------|--------|------|----------|
| `neovex` | `agentstation/neovex` | ~60MB | Cargo (Rust + V8) |
| `neovex-crun` | upstream crun + build-time patch | ~2MB | autotools (C) |

### System dependencies (not shipped — installed from OS repos)

| Package | Debian/Ubuntu | Fedora/RHEL | macOS (Homebrew) |
|---------|--------------|-------------|-----------------|
| conmon | `apt install conmon` | `dnf install conmon` | N/A (no krun on Intel Mac) |
| buildah | `apt install buildah` | `dnf install buildah` | `brew install buildah` |
| containers-common | Comes with buildah | Comes with buildah | Comes with buildah |
| libkrun | **Not in repos** | `dnf install libkrun` | `brew install libkrun` |
| libkrunfw | **Not in repos** | `dnf install libkrunfw` | `brew install libkrunfw` |
| catatonit | `apt install catatonit` | `dnf install catatonit` | N/A |
| passt | `apt install passt` | `dnf install passt` | N/A |
| uidmap | `apt install uidmap` | `dnf install shadow-utils` | N/A |
| fuse-overlayfs | `apt install fuse-overlayfs` | `dnf install fuse-overlayfs` | N/A |

### Platform support

| Platform | KVM source | libkrun status | Supported |
|----------|-----------|----------------|-----------|
| Linux x86_64 (bare metal) | Hardware VT-x | Available | **Yes** (primary) |
| Linux x86_64 (cloud VM) | Nested virt | Available | **Yes** |
| Linux aarch64 | Hardware | Available | **Future** |
| macOS aarch64 (Apple Silicon) | Hypervisor.framework | Available via Homebrew | **Future** |
| macOS x86_64 (Intel) | No hypervisor | Not available | **No** |
| Windows | WSL2 + nested KVM | Experimental | **Future** |

---

## Distribution Channels

### Channel 1: Install Script (Quick Start)

```bash
curl -fsSL https://neovex.dev/install.sh | sh
```

The script:
1. Detects OS (Debian/Ubuntu, Fedora/RHEL, macOS)
2. Detects architecture (x86_64, aarch64)
3. Adds neovex apt/dnf repo (or Homebrew tap)
4. Installs neovex + all dependencies via package manager
5. Checks `/dev/kvm` access
6. Prints getting-started instructions

**Implementation reference:**
- [rustup install script](https://github.com/rust-lang/rustup/blob/master/rustup-init.sh)
- [Docker install script](https://github.com/docker/docker-install/blob/master/install.sh)

### Channel 2: Debian/Ubuntu (.deb)

**Package: `neovex`**

```
Package: neovex
Version: 0.1.0
Architecture: amd64
Depends: neovex-crun, conmon, buildah, containers-common
Recommends: catatonit, passt, uidmap, fuse-overlayfs
Description: Reactive document database with microVM runtime
```

**Package: `neovex-crun`**

```
Package: neovex-crun
Version: 1.19+neovex1
Architecture: amd64
Depends: libkrun (>= 1.17), libkrunfw, libcap2, libseccomp2, libyajl2
Description: crun OCI runtime with krun TSI port mapping (patched for neovex)
```

Built from upstream crun release tarball + build-time patch (see
`vmm-infrastructure-plan.md`). Installs to `/usr/libexec/neovex/crun`. Does
NOT conflict with or replace the system `crun` — neovex invokes it via
`conmon -r /usr/libexec/neovex/crun`. System Podman/CRI-O continue using
the distro `crun` undisturbed.

Version format: `{upstream_version}+neovex{patch_revision}`. The `+` separator
follows Debian convention for local modifications. When upstream merges the
port mapping PR, `neovex-crun` is dropped and replaced by a dependency on the
system `crun` (>= the version that includes the patch).

**Package: `libkrun` (for Debian/Ubuntu where it's not in repos)**

```
Package: libkrun
Version: 1.17.4
Architecture: amd64
Depends: libc6
Description: Dynamic library for KVM-based process isolation
```

**Apt repository:**
```
deb [signed-by=/usr/share/keyrings/neovex.gpg] https://apt.neovex.dev stable main
```

**Build system:** GitHub Actions → build .deb → upload to apt repo (hosted
on GitHub Pages, Cloudflare R2, or Packagecloud).

**Implementation reference:**
- [goreleaser nfpm](https://github.com/goreleaser/nfpm) — build deb/rpm from
  YAML config, Go binary
- [cargo-deb](https://crates.io/crates/cargo-deb) — build .deb from Cargo
  metadata

### Channel 3: Fedora/RHEL (.rpm)

**Package: `neovex`**

```
Name: neovex
Version: 0.1.0
Requires: neovex-crun conmon buildah containers-common
Recommends: catatonit passt shadow-utils fuse-overlayfs
```

On Fedora, libkrun and libkrunfw are already in the repos. The
`neovex-crun` package installs to `/usr/libexec/neovex/crun` alongside
the system crun (does not replace it).

**COPR or custom repo:**
```
dnf copr enable agentstation/neovex
dnf install neovex
```

**Implementation reference:**
- [Fedora COPR](https://copr.fedorainfracloud.org/) — free RPM build service

### Channel 4: Homebrew (macOS)

```ruby
# Formula: neovex.rb
class Neovex < Formula
  desc "Reactive document database with microVM runtime"
  homepage "https://neovex.dev"
  url "https://github.com/agentstation/neovex/releases/download/v0.1.0/neovex-darwin-arm64.tar.gz"
  sha256 "..."

  depends_on "buildah"
  depends_on "libkrun"
  depends_on "libkrunfw"
  depends_on :macos  # macOS only (Apple Silicon)
  depends_on arch: :arm64  # Hypervisor.framework on ARM only

  def install
    bin.install "neovex"
  end
end
```

**Homebrew tap:**
```bash
brew tap agentstation/neovex
brew install neovex
```

**Note:** macOS support requires Hypervisor.framework (Apple Silicon only).
conmon, catatonit, passt, uidmap, fuse-overlayfs are Linux-specific and
not needed on macOS. libkrun uses HVF instead of KVM on macOS.

**Implementation reference:**
- [Homebrew formulae docs](https://docs.brew.sh/Formula-Cookbook)

### Channel 5: Binary Tarball (Manual Install)

```bash
# Download
curl -L -o neovex.tar.gz \
  https://github.com/agentstation/neovex/releases/download/v0.1.0/neovex-linux-amd64.tar.gz

# Extract
tar xzf neovex.tar.gz
sudo mv neovex /usr/local/bin/
sudo mkdir -p /usr/libexec/neovex
sudo mv neovex-crun /usr/libexec/neovex/crun

# Install deps manually
sudo apt install conmon buildah catatonit passt
# For Debian: also install libkrun and libkrunfw from neovex apt repo
```

The tarball includes: `neovex` + `neovex-crun` + install instructions.

### Channel 6: Container Image (for CI/CD tooling)

```dockerfile
FROM debian:13-slim
RUN apt-get update && apt-get install -y \
    conmon buildah catatonit passt uidmap fuse-overlayfs
COPY neovex /usr/local/bin/
COPY neovex-crun /usr/libexec/neovex/crun
# Note: This container must run with --privileged and /dev/kvm access
```

**Use case:** CI/CD pipelines that need to run neovex. The container
provides all dependencies. Must run with `--privileged` and
`--device /dev/kvm` for KVM access.

```bash
docker run --privileged --device /dev/kvm \
  ghcr.io/agentstation/neovex:latest serve
```

### Channel 7: Cloud VM Images (Production)

Pre-baked VM images with everything installed.

**AWS AMI:**
- Based on Debian 13 or Amazon Linux 2023
- neovex + all deps pre-installed
- KVM enabled (use `.metal` or nested-virt-capable instance types)
- Published to AWS Marketplace or as community AMI

**GCP Image:**
- Based on Debian 13
- Nested virtualization enabled
- Published to GCP Compute Image library

**Build system:** Packer (HashiCorp) for reproducible image builds.

```hcl
# packer.hcl
source "amazon-ebs" "neovex" {
  ami_name      = "neovex-{{timestamp}}"
  instance_type = "c5.metal"
  source_ami    = "ami-debian-13-..."
}

build {
  sources = ["source.amazon-ebs.neovex"]
  provisioner "shell" {
    inline = [
      "curl -fsSL https://neovex.dev/install.sh | sh",
    ]
  }
}
```

**Implementation reference:**
- [Packer](https://www.packer.io/) — VM image builder

---

## Phase Plan

### Phase D1: CI Build Pipeline

**Goal:** Automated builds of neovex and neovex-crun for Linux x86_64.

**Scope:**
- GitHub Actions workflow: build neovex (cargo build --release)
- GitHub Actions workflow: build neovex-crun (download upstream tarball, apply patch, autotools)
- GitHub Releases: upload binaries as release assets
- Tarball (Channel 5): neovex + crun + README

**Acceptance criteria:**
- `git tag v0.1.0 && git push --tags` triggers build
- Release assets include: `neovex-linux-amd64.tar.gz`
- Tarball includes both binaries + install instructions

### Phase D2: Apt Repository (Debian/Ubuntu)

**Goal:** `apt install neovex` works on Debian 13 and Ubuntu 24.04+.

**Scope:**
- Build .deb packages: neovex, neovex-crun, libkrun, libkrunfw
- Host apt repository (GitHub Pages or Cloudflare R2)
- GPG-sign packages
- Install script (Channel 1) adds the repo and installs

**Acceptance criteria:**
- Fresh Debian 13 VM: `curl ... | sh && neovex serve` works
- Dependencies automatically pulled (conmon, buildah, etc.)

### Phase D3: Fedora/COPR (Fedora/RHEL)

**Goal:** `dnf install neovex` works on Fedora 40+.

**Scope:**
- Build .rpm packages: neovex, neovex-crun
- libkrun/libkrunfw already in Fedora repos — just depend on them
- Publish via COPR (free RPM build service)

**Acceptance criteria:**
- Fresh Fedora 40 VM: `dnf copr enable ... && dnf install neovex` works

### Phase D4: Homebrew (macOS)

**Goal:** `brew install neovex` works on macOS (Apple Silicon).

**Scope:**
- Build neovex for `aarch64-apple-darwin`
- Create Homebrew formula in `agentstation/homebrew-neovex` tap
- Depend on `libkrun` and `buildah` Homebrew packages

**Note:** macOS support is limited to Apple Silicon (Hypervisor.framework).
Not all features work (no conmon, no catatonit, no passt). Development and
testing use case, not production.

**Acceptance criteria:**
- `brew tap agentstation/neovex && brew install neovex` works on M1+ Mac

### Phase D5: Cloud VM Images

**Goal:** Pre-baked VM images for AWS and GCP.

**Scope:**
- Packer templates for AWS AMI and GCP Image
- Based on Debian 13
- All deps pre-installed
- KVM verified working

**Acceptance criteria:**
- Launch AMI on c5.metal → `neovex serve` works immediately
- Launch GCP VM with nested virt → `neovex serve` works immediately

---

## Phase Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| D1: CI build pipeline | `todo` | Neovex compiles | GitHub Actions |
| D2: Apt repo (Debian/Ubuntu) | `todo` | D1 | cargo-deb or nfpm |
| D3: COPR (Fedora) | `todo` | D1 | COPR build service |
| D4: Homebrew (macOS) | `todo` | D1 | Apple Silicon only |
| D5: Cloud VM images | `todo` | D2 or D3 | Packer |

---

## Execution Log

_Empty — no work started._
